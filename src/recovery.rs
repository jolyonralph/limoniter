//! Keeps a copy of unsaved edits on disk, so a crash, a forced kill or a power cut can't lose them.
//!
//! Each running Limoniter has its own session folder, with a lock file held open for as long as it
//! runs. A folder whose lock can be taken belongs to a program that has gone, so what's in it is
//! offered back at the next start. Copies are written by a thread of their own: the window only hands
//! over a snapshot, and the newest snapshot of a document replaces any still waiting.
//!
//! A copy is written to a temporary name, flushed to the disk and then renamed, keeping the previous
//! copy as a fallback, and ends in a checksum, so a half-written or damaged copy is noticed.

use crate::charset;
use crate::codec::{Certainty, Display, Eol, NewlinePolicy, TextFormat};
use crate::line_store::{ImagePiece, SourceRef, StoreImage};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, HashSet};
use std::fs::{self, File, OpenOptions};
use std::io::{self, BufWriter, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Condvar, Mutex, OnceLock};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

const MAGIC: &str = "LIMONITER-RECOVERY 1";
const LOCK_NAME: &str = "session.lock";
/// The least time between writes, so that typing doesn't keep the disk busy.
const MIN_GAP: Duration = Duration::from_millis(200);

/// What a copy records about a document, besides its text.
#[derive(Clone, Debug, PartialEq)]
pub struct Meta {
    pub name: String,
    pub path: Option<PathBuf>,
    pub charset_id: String,
    pub newline: String,
    pub display: String,
    pub state: usize,
    pub has_bom: bool,
    pub ensure_final_newline: bool,
    pub preset_eol: Eol,
    pub caret: (usize, usize),
    /// When this copy was taken, in seconds since 1970.
    pub taken: u64,
    /// The size and time the file on disk had when last read or saved, to notice if it's changed since.
    pub file_stamp: Option<(u64, Option<SystemTime>)>,
}

impl Meta {
    pub fn new(name: String, path: Option<PathBuf>, format: &TextFormat, caret: (usize, usize)) -> Self {
        Self {
            name,
            path,
            charset_id: format.charset.id.clone(),
            newline: format.newline.key(),
            display: format.display.key().to_string(),
            state: format.state,
            has_bom: format.has_bom,
            ensure_final_newline: format.ensure_final_newline,
            preset_eol: format.preset_eol,
            caret,
            taken: SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs()),
            file_stamp: None,
        }
    }

    /// The document's settings. A set that's no longer defined is an error rather than a silent swap.
    pub fn format(&self) -> Result<TextFormat, String> {
        let set = charset::by_id(&self.charset_id).ok_or_else(|| format!("The character set {} isn't available.", self.charset_id))?;
        Ok(TextFormat {
            charset: set,
            has_bom: self.has_bom,
            newline: NewlinePolicy::from_key(&self.newline).unwrap_or(NewlinePolicy::Standard),
            state: self.state,
            display: Display::from_key(&self.display).unwrap_or(Display::Plain),
            ensure_final_newline: self.ensure_final_newline,
            preset_eol: self.preset_eol,
            preset: None,
            certainty: Certainty::Chosen,
        })
    }
}

pub enum Body {
    Text(StoreImage),
    Hex(Vec<u8>),
    /// Edits to a file too large to copy, which stays where it is: the pieces, and the bytes they add.
    HexOnDisk { source: SourceRef, pieces: Vec<crate::byte_store::Piece>, added: Vec<u8> },
}

pub struct Snapshot {
    pub doc_id: u64,
    pub meta: Meta,
    pub body: Body,
}

// ---------- The file format ----------

pub fn eol_code(eol: Eol) -> u8 {
    match eol {
        Eol::CrLf => 0,
        Eol::Lf => 1,
        Eol::Cr => 2,
        Eol::Atascii => 3,
        Eol::None => 4,
        Eol::Nl => 5,
    }
}

pub fn eol_from(code: u8) -> Result<Eol, String> {
    match code {
        0 => Ok(Eol::CrLf),
        1 => Ok(Eol::Lf),
        2 => Ok(Eol::Cr),
        3 => Ok(Eol::Atascii),
        4 => Ok(Eol::None),
        5 => Ok(Eol::Nl),
        _ => Err("It holds an unknown line ending.".to_string()),
    }
}

fn hex_of(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn from_hex(text: &str) -> Option<Vec<u8>> {
    if !text.len().is_multiple_of(2) {
        return None;
    }
    (0..text.len()).step_by(2).map(|i| text.get(i..i + 2).and_then(|pair| u8::from_str_radix(pair, 16).ok())).collect()
}

/// A Windows path exactly, even one that isn't valid Unicode.
#[cfg(windows)]
fn path_to_hex(path: &Path) -> String {
    use std::os::windows::ffi::OsStrExt;
    let bytes: Vec<u8> = path.as_os_str().encode_wide().flat_map(u16::to_le_bytes).collect();
    hex_of(&bytes)
}

#[cfg(windows)]
fn path_from_hex(text: &str) -> Option<PathBuf> {
    use std::os::windows::ffi::OsStringExt;
    let bytes = from_hex(text)?;
    let units: Vec<u16> = bytes.as_chunks::<2>().0.iter().map(|pair| u16::from_le_bytes(*pair)).collect();
    Some(PathBuf::from(std::ffi::OsString::from_wide(&units)))
}

/// A path's own bytes, which on macOS and Linux needn't be valid Unicode either.
#[cfg(not(windows))]
fn path_to_hex(path: &Path) -> String {
    use std::os::unix::ffi::OsStrExt;
    hex_of(path.as_os_str().as_bytes())
}

#[cfg(not(windows))]
fn path_from_hex(text: &str) -> Option<PathBuf> {
    use std::os::unix::ffi::OsStringExt;
    Some(PathBuf::from(std::ffi::OsString::from_vec(from_hex(text)?)))
}

fn stamp_text(time: Option<SystemTime>) -> String {
    time.and_then(|t| t.duration_since(UNIX_EPOCH).ok()).map_or_else(|| "-".to_string(), |d| format!("{}.{}", d.as_secs(), d.subsec_nanos()))
}

fn stamp_from(text: &str) -> Option<SystemTime> {
    let (seconds, nanos) = text.split_once('.')?;
    Some(UNIX_EPOCH + Duration::new(seconds.parse().ok()?, nanos.parse().ok()?))
}

/// Writes through to the file, working out the checksum on the way.
struct Checked<W: Write> {
    out: W,
    hash: Sha256,
}

impl<W: Write> Checked<W> {
    fn put(&mut self, bytes: &[u8]) -> io::Result<()> {
        self.hash.update(bytes);
        self.out.write_all(bytes)
    }

    fn number(&mut self, value: u64) -> io::Result<()> {
        self.put(&value.to_le_bytes())
    }
}

/// Writes a whole copy. Its last 32 bytes are the checksum of everything before them.
fn write_snapshot<W: Write>(out: W, snapshot: &Snapshot) -> io::Result<W> {
    let mut w = Checked { out, hash: Sha256::new() };
    let m = &snapshot.meta;
    let mut head = format!("{MAGIC}\n");
    head += &format!("name={}\n", hex_of(m.name.as_bytes()));
    if let Some(path) = &m.path {
        head += &format!("path={}\n", path_to_hex(path));
    }
    head += &format!(
        "charset={}\nnewline={}\ndisplay={}\nstate={}\nbom={}\nfinal={}\npreset_eol={}\ncaret={},{}\ntaken={}\n",
        hex_of(m.charset_id.as_bytes()),
        m.newline,
        m.display,
        m.state,
        m.has_bom,
        m.ensure_final_newline,
        eol_code(m.preset_eol),
        m.caret.0,
        m.caret.1,
        m.taken
    );
    if let Some((length, time)) = m.file_stamp {
        head += &format!("file_len={length}\nfile_time={}\n", stamp_text(time));
    }
    match &snapshot.body {
        Body::Hex(_) => head += "kind=hex\n",
        Body::HexOnDisk { source, .. } => {
            head += "kind=hex-on-disk\n";
            head += &format!("source={}\nsource_len={}\nsource_time={}\n", path_to_hex(&source.path), source.len, stamp_text(source.modified));
        }
        Body::Text(image) => {
            head += "kind=text\n";
            if let Some(source) = &image.source {
                head += &format!("source={}\nsource_len={}\nsource_time={}\n", path_to_hex(&source.path), source.len, stamp_text(source.modified));
            }
        }
    }
    head.push('\n');
    w.put(head.as_bytes())?;
    match &snapshot.body {
        Body::Hex(bytes) => w.put(bytes)?,
        Body::HexOnDisk { pieces, added, .. } => {
            w.number(pieces.len() as u64)?;
            for piece in pieces {
                w.put(&[u8::from(piece.added)])?;
                w.number(piece.offset)?;
                w.number(piece.len as u64)?;
            }
            w.number(added.len() as u64)?;
            w.put(added)?;
        }
        Body::Text(image) => {
            w.put(&[eol_code(image.base), eol_code(image.insert)])?;
            w.number(image.eols.len() as u64)?;
            for (line, eol) in &image.eols {
                w.number(*line as u64)?;
                w.put(&[eol_code(*eol)])?;
            }
            w.number(image.bookmarks.len() as u64)?;
            for line in &image.bookmarks {
                w.number(*line as u64)?;
            }
            w.number(image.pieces.len() as u64)?;
            for piece in &image.pieces {
                match piece {
                    ImagePiece::Disk { first, count } => {
                        w.put(&[0])?;
                        w.number(*first as u64)?;
                        w.number(*count as u64)?;
                    }
                    ImagePiece::Memory(lines) => {
                        w.put(&[1])?;
                        w.number(lines.len() as u64)?;
                        for line in lines {
                            w.number(line.len() as u64)?;
                            w.put(line.as_bytes())?;
                        }
                    }
                }
            }
        }
    }
    let Checked { mut out, hash } = w;
    out.write_all(&hash.finalize())?;
    Ok(out)
}

struct Reader<'a> {
    data: &'a [u8],
}

impl Reader<'_> {
    fn take(&mut self, count: usize) -> Result<&[u8], String> {
        if count > self.data.len() {
            return Err("It ends too soon.".to_string());
        }
        let (head, rest) = self.data.split_at(count);
        self.data = rest;
        Ok(head)
    }

    fn number(&mut self) -> Result<usize, String> {
        let bytes = self.take(8)?;
        let mut value = [0u8; 8];
        value.copy_from_slice(bytes);
        usize::try_from(u64::from_le_bytes(value)).map_err(|_| "It holds a number too large.".to_string())
    }

    fn byte(&mut self) -> Result<u8, String> {
        Ok(self.take(1)?[0])
    }
}

/// Reads a copy, checking first that every byte is what was written.
pub fn read_snapshot(data: &[u8]) -> Result<(Meta, Body), String> {
    if data.len() < 32 {
        return Err("It's too short to be a recovery copy.".to_string());
    }
    let (content, sum) = data.split_at(data.len() - 32);
    if Sha256::digest(content).as_slice() != sum {
        return Err("Its checksum doesn't match, so it's damaged.".to_string());
    }
    let end = content.windows(2).position(|pair| pair == b"\n\n").ok_or("It has no header.")?;
    let head = std::str::from_utf8(&content[..end]).map_err(|_| "Its header can't be read.")?;
    let mut lines = head.lines();
    if !matches!(lines.next(), Some(MAGIC)) {
        return Err("It isn't a recovery copy this version can read.".to_string());
    }
    let values: BTreeMap<&str, &str> = lines.filter_map(|line| line.split_once('=')).collect();
    let text = |key: &str| values.get(key).copied().unwrap_or("");
    let decoded = |key: &str| from_hex(text(key)).and_then(|bytes| String::from_utf8(bytes).ok()).ok_or_else(|| format!("Its {key} can't be read."));
    let (caret_line, caret_char) = text("caret").split_once(',').ok_or("Its cursor position can't be read.")?;
    let meta = Meta {
        name: decoded("name")?,
        path: values.get("path").and_then(|p| path_from_hex(p)),
        charset_id: decoded("charset")?,
        newline: text("newline").to_string(),
        display: text("display").to_string(),
        state: text("state").parse().unwrap_or(0),
        has_bom: text("bom") == "true",
        ensure_final_newline: text("final") == "true",
        preset_eol: eol_from(text("preset_eol").parse().unwrap_or(0))?,
        caret: (caret_line.parse().unwrap_or(0), caret_char.parse().unwrap_or(0)),
        taken: text("taken").parse().unwrap_or(0),
        file_stamp: values.get("file_len").and_then(|length| length.parse().ok()).map(|length| (length, stamp_from(text("file_time")))),
    };
    let mut r = Reader { data: &content[end + 2..] };
    if text("kind") == "hex" {
        return Ok((meta, Body::Hex(r.data.to_vec())));
    }
    let source = values.get("source").and_then(|p| path_from_hex(p)).map(|path| SourceRef { path, len: text("source_len").parse().unwrap_or(0), modified: stamp_from(text("source_time")) });
    if text("kind") == "hex-on-disk" {
        let source = source.ok_or("It doesn't say which file it edits.")?;
        let mut pieces = Vec::new();
        for _ in 0..r.number()? {
            let added = r.byte()? != 0;
            let offset = r.number()? as u64;
            let len = r.number()?;
            pieces.push(crate::byte_store::Piece { added, offset, len });
        }
        let count = r.number()?;
        let added = r.take(count)?.to_vec();
        return Ok((meta, Body::HexOnDisk { source, pieces, added }));
    }
    let base = eol_from(r.byte()?)?;
    let insert = eol_from(r.byte()?)?;
    let mut eols = Vec::new();
    for _ in 0..r.number()? {
        let line = r.number()?;
        eols.push((line, eol_from(r.byte()?)?));
    }
    let mut bookmarks = Vec::new();
    for _ in 0..r.number()? {
        bookmarks.push(r.number()?);
    }
    let mut pieces = Vec::new();
    for _ in 0..r.number()? {
        if r.byte()? == 0 {
            let first = r.number()?;
            let count = r.number()?;
            pieces.push(ImagePiece::Disk { first, count });
        } else {
            let count = r.number()?;
            let mut lines = Vec::new();
            for _ in 0..count {
                let length = r.number()?;
                let bytes = r.take(length)?;
                let line = std::str::from_utf8(bytes).map_err(|_| "A line can't be read.")?;
                lines.push(Arc::from(line));
            }
            pieces.push(ImagePiece::Memory(lines));
        }
    }
    Ok((meta, Body::Text(StoreImage { base, insert, eols, bookmarks, pieces, source })))
}

fn copy_path(dir: &Path, doc_id: u64) -> PathBuf {
    dir.join(format!("doc-{doc_id}.rec"))
}

fn previous_of(copy: &Path) -> PathBuf {
    copy.with_extension("prev")
}

/// Puts a copy on the disk. If anything goes wrong, the copies already there are untouched.
fn write_copy(dir: &Path, snapshot: &Snapshot) -> io::Result<()> {
    let copy = copy_path(dir, snapshot.doc_id);
    let temp = dir.join(format!("doc-{}.tmp{:?}", snapshot.doc_id, std::thread::current().id()).replace(['(', ')'], ""));
    let result = (|| {
        let file = File::create(&temp)?;
        let mut out = write_snapshot(BufWriter::with_capacity(1024 * 1024, file), snapshot)?;
        out.flush()?;
        out.get_ref().sync_all()?;
        drop(out);
        if copy.exists() {
            // If the fallback can't be kept, the new copy still matters more.
            let _ = fs::rename(&copy, previous_of(&copy));
        }
        fs::rename(&temp, &copy)
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temp);
    }
    result
}

// ---------- The running session ----------

enum Job {
    Write(Box<Snapshot>),
    Remove,
}

#[derive(Default)]
struct WriterState {
    pending: BTreeMap<u64, Job>,
    busy: bool,
    stop: bool,
    problem: Option<String>,
}

struct Shared {
    state: Mutex<WriterState>,
    changed: Condvar,
}

impl Shared {
    fn lock(&self) -> std::sync::MutexGuard<'_, WriterState> {
        // A copy of someone's edits matters more than a thread having failed earlier.
        self.state.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    /// Waits for everything handed over to reach the disk. Returns false if time ran out.
    fn flush(&self, longest: Duration) -> bool {
        let deadline = Instant::now() + longest;
        let mut state = self.lock();
        while !state.pending.is_empty() || state.busy {
            let left = deadline.saturating_duration_since(Instant::now());
            if left.is_zero() {
                return false;
            }
            state = self.changed.wait_timeout(state, left.min(Duration::from_millis(50))).map_or_else(|e| e.into_inner().0, |r| r.0);
        }
        true
    }
}

/// The session's writer, for the panic hook, which has nothing else to reach it by.
static ACTIVE: OnceLock<Arc<Shared>> = OnceLock::new();

/// Set while the window draws a frame, where a failure is caught and handled by the window itself.
pub static IN_FRAME: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// For when the program is failing: gives what's already handed over a moment to reach the disk.
pub fn flush_for_crash() {
    if let Some(shared) = ACTIVE.get() {
        shared.flush(Duration::from_secs(5));
    }
}

fn writer_thread(dir: PathBuf, shared: Arc<Shared>) {
    loop {
        let job = {
            let mut state = shared.lock();
            loop {
                if let Some(id) = state.pending.keys().next().copied() {
                    let job = state.pending.remove(&id);
                    state.busy = true;
                    break job.map(|job| (id, job));
                }
                if state.stop {
                    return;
                }
                state = shared.changed.wait(state).unwrap_or_else(std::sync::PoisonError::into_inner);
            }
        };
        let Some((id, job)) = job else {
            continue;
        };
        let started = Instant::now();
        // Whatever goes wrong with one copy, the thread that keeps the others must survive it.
        let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| match &job {
            Job::Write(snapshot) => write_copy(&dir, snapshot),
            Job::Remove => {
                let copy = copy_path(&dir, id);
                let _ = fs::remove_file(previous_of(&copy));
                match fs::remove_file(&copy) {
                    Err(e) if e.kind() != io::ErrorKind::NotFound => Err(e),
                    _ => Ok(()),
                }
            }
        }));
        let problem = match outcome {
            Ok(Ok(())) => None,
            Ok(Err(e)) => Some(e.to_string()),
            Err(_) => Some("The recovery writer failed.".to_string()),
        };
        {
            let mut state = shared.lock();
            state.busy = false;
            state.problem = problem;
            shared.changed.notify_all();
        }
        // The longer a copy takes, the less often it's taken, so a big file doesn't keep the disk busy.
        let gap = (started.elapsed() * 4).max(MIN_GAP);
        let state = shared.lock();
        if !state.stop {
            let _ = shared.changed.wait_timeout_while(state, gap, |state| !state.stop);
        }
    }
}

/// This run's folder of recovery copies.
pub struct Session {
    dir: PathBuf,
    shared: Arc<Shared>,
    /// Held open for as long as the program runs: this is what marks the folder as in use.
    lock: Option<File>,
    known: HashSet<u64>,
}

#[cfg(windows)]
pub fn take_lock(path: &Path) -> io::Result<File> {
    use std::os::windows::fs::OpenOptionsExt;
    // No sharing: while this is open, nobody else can open it, and Windows lets go when the process ends, however it ends.
    OpenOptions::new().read(true).write(true).create(true).truncate(false).share_mode(0).open(path)
}

#[cfg(not(windows))]
pub fn take_lock(path: &Path) -> io::Result<File> {
    // A lock of its own on the file, which the system lets go of when the process ends, however it ends.
    let file = OpenOptions::new().read(true).write(true).create(true).truncate(false).open(path)?;
    file.try_lock().map_err(|e| match e {
        std::fs::TryLockError::WouldBlock => io::Error::new(io::ErrorKind::WouldBlock, "Another Limoniter is using it."),
        std::fs::TryLockError::Error(e) => e,
    })?;
    Ok(file)
}

impl Session {
    pub fn start(root: &Path) -> io::Result<Self> {
        let stamp = SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_nanos());
        let dir = root.join(format!("session-{stamp}-{}", std::process::id()));
        fs::create_dir_all(&dir)?;
        let lock = take_lock(&dir.join(LOCK_NAME))?;
        let shared = Arc::new(Shared { state: Mutex::new(WriterState::default()), changed: Condvar::new() });
        let _ = ACTIVE.set(shared.clone());
        let (worker_dir, worker_shared) = (dir.clone(), shared.clone());
        std::thread::Builder::new().name("recovery-writer".to_string()).spawn(move || writer_thread(worker_dir, worker_shared))?;
        Ok(Self { dir, shared, lock: Some(lock), known: HashSet::new() })
    }

    /// Hands over the newest copy of a document. Any older copy still waiting is dropped for it.
    pub fn offer(&mut self, snapshot: Snapshot) {
        self.known.insert(snapshot.doc_id);
        let mut state = self.shared.lock();
        state.pending.insert(snapshot.doc_id, Job::Write(Box::new(snapshot)));
        self.shared.changed.notify_all();
    }

    #[cfg(test)]
    pub fn has_copy(&self, doc_id: u64) -> bool {
        self.known.contains(&doc_id)
    }

    /// The edits are saved or thrown away, so the copy isn't needed any more.
    pub fn remove(&mut self, doc_id: u64) {
        if self.known.remove(&doc_id) {
            let mut state = self.shared.lock();
            state.pending.insert(doc_id, Job::Remove);
            self.shared.changed.notify_all();
        }
    }

    /// Drops the copies of documents that are no longer open.
    pub fn keep_only(&mut self, open: &HashSet<u64>) {
        let gone: Vec<u64> = self.known.iter().copied().filter(|id| !open.contains(id)).collect();
        for id in gone {
            self.remove(id);
        }
    }

    /// Why the last copy couldn't be written, if it couldn't.
    pub fn problem(&self) -> Option<String> {
        self.shared.lock().problem.clone()
    }

    /// Writes a copy straight away, for recovered documents and when failing. Waits for anything already
    /// being written first, so that an older copy can't land on top of this one.
    pub fn write_now(&mut self, snapshot: &Snapshot) -> io::Result<()> {
        self.known.insert(snapshot.doc_id);
        {
            let mut state = self.shared.lock();
            state.pending.remove(&snapshot.doc_id);
        }
        self.shared.flush(Duration::from_secs(5));
        write_copy(&self.dir, snapshot)
    }

    #[cfg(test)]
    pub fn flush(&self, longest: Duration) -> bool {
        self.shared.flush(longest)
    }

    /// A clean exit: nothing here is needed any more, so the folder goes.
    pub fn finish(mut self) {
        self.shared.flush(Duration::from_secs(10));
        {
            let mut state = self.shared.lock();
            state.stop = true;
            self.shared.changed.notify_all();
        }
        self.lock = None;
        let _ = fs::remove_dir_all(&self.dir);
    }

    #[cfg(test)]
    pub fn dir(&self) -> &Path {
        &self.dir
    }

    /// For tests: stops as a crash would, leaving the folder behind but letting go of the lock.
    #[cfg(test)]
    pub fn abandon(mut self) {
        self.shared.flush(Duration::from_secs(5));
        let mut state = self.shared.lock();
        state.stop = true;
        self.shared.changed.notify_all();
        drop(state);
        self.lock = None;
    }
}

// ---------- What an earlier run left behind ----------

/// A session folder from a program that's gone. Holding its lock stops another window offering it too.
pub struct Orphan {
    pub dir: PathBuf,
    _lock: File,
}

/// One copy found, ready to offer.
pub struct Found {
    pub file: PathBuf,
    pub outcome: Result<(Meta, Body), String>,
}

fn read_copy(path: &Path) -> Result<(Meta, Body), String> {
    let mut data = Vec::new();
    File::open(path).and_then(|mut file| file.read_to_end(&mut data)).map_err(|e| e.to_string())?;
    read_snapshot(&data)
}

/// Finds what earlier runs left. A folder in use by a running Limoniter is left alone, and empty ones are tidied up.
pub fn find_orphans(root: &Path) -> (Vec<Orphan>, Vec<Found>) {
    let mut orphans = Vec::new();
    let mut found = Vec::new();
    let Ok(entries) = fs::read_dir(root) else {
        return (orphans, found);
    };
    let mut dirs: Vec<PathBuf> = entries.flatten().map(|entry| entry.path()).filter(|path| path.is_dir()).collect();
    dirs.sort();
    for dir in dirs {
        let Ok(lock) = take_lock(&dir.join(LOCK_NAME)) else {
            continue;
        };
        let mut copies: Vec<PathBuf> = fs::read_dir(&dir).map_or_else(|_| Vec::new(), |list| list.flatten().map(|e| e.path()).filter(|p| p.extension().is_some_and(|x| x == "rec")).collect());
        // A crash between the two renames leaves only the previous copy under its own name.
        if let Ok(list) = fs::read_dir(&dir) {
            for path in list.flatten().map(|e| e.path()) {
                if path.extension().is_some_and(|x| x == "prev") && !path.with_extension("rec").exists() {
                    copies.push(path);
                }
            }
        }
        copies.sort();
        if copies.is_empty() {
            drop(lock);
            let _ = fs::remove_dir_all(&dir);
            continue;
        }
        for file in copies {
            let mut outcome = read_copy(&file);
            // If the newest copy is damaged, the one before it is still most of the work.
            if outcome.is_err()
                && file.extension().is_some_and(|x| x == "rec")
                && let Ok(previous) = read_copy(&previous_of(&file))
            {
                outcome = Ok(previous);
            }
            found.push(Found { file, outcome });
        }
        orphans.push(Orphan { dir, _lock: lock });
    }
    (orphans, found)
}

/// The files that recovery copies, in every session, running or not, read their untouched lines from.
pub fn sources_named(root: &Path) -> HashSet<PathBuf> {
    let mut named = HashSet::new();
    let Ok(dirs) = fs::read_dir(root) else {
        return named;
    };
    for dir in dirs.flatten().map(|entry| entry.path()).filter(|path| path.is_dir()) {
        for file in fs::read_dir(&dir).into_iter().flatten().flatten().map(|entry| entry.path()) {
            if !file.extension().is_some_and(|x| x == "rec" || x == "prev") {
                continue;
            }
            // Only the header is needed, which comes first and is short.
            let mut data = Vec::new();
            if File::open(&file).and_then(|f| io::Read::take(f, 64 * 1024).read_to_end(&mut data)).is_err() {
                continue;
            }
            let end = data.windows(2).position(|pair| pair == b"\n\n").unwrap_or(data.len());
            for line in String::from_utf8_lossy(&data[..end]).lines() {
                if let Some(path) = line.strip_prefix("source=").and_then(path_from_hex) {
                    named.insert(path);
                }
            }
        }
    }
    named
}

/// Removes a copy that's been dealt with, along with its fallback.
pub fn delete_copy(file: &Path) -> io::Result<()> {
    let _ = fs::remove_file(file.with_extension("prev"));
    let _ = fs::remove_file(file.with_extension("rec"));
    if file.exists() { fs::remove_file(file) } else { Ok(()) }
}

/// Removes session folders with no copies left in them.
pub fn tidy(orphans: Vec<Orphan>) {
    for orphan in orphans {
        let has_copies = fs::read_dir(&orphan.dir).is_ok_and(|list| list.flatten().any(|e| e.path().extension().is_some_and(|x| x == "rec" || x == "prev")));
        let Orphan { dir, _lock } = orphan;
        drop(_lock);
        if !has_copies {
            let _ = fs::remove_dir_all(&dir);
        }
    }
}

/// For tests: waits for every session under a folder to be let go of. Another test starting a program shares this
/// process's open files with it for a moment, and a lock on one of them is held until that program has started.
#[cfg(test)]
pub fn wait_until_let_go(root: &Path) {
    let began = std::time::Instant::now();
    let dirs: Vec<PathBuf> = fs::read_dir(root).unwrap().flatten().map(|e| e.path()).filter(|p| p.is_dir()).collect();
    for dir in dirs {
        while take_lock(&dir.join(LOCK_NAME)).is_err() {
            assert!(began.elapsed() < std::time::Duration::from_secs(10), "the session's lock was never let go");
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::line_store::LineStore;
    use crate::text_buffer::TextBuffer;

    fn temp_root(tag: &str) -> PathBuf {
        let root = std::env::temp_dir().join(format!("limoniter-recovery-{tag}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        root
    }

    fn snapshot_of(id: u64, data: &[u8], charset_id: &str, edit: impl FnOnce(&mut TextBuffer)) -> (Snapshot, Vec<u8>) {
        let mut b = TextBuffer::default();
        b.format = TextFormat { charset: charset::known(charset_id), ..TextFormat::default() };
        let decoded = crate::codec::decode(data, &mut b.format);
        b.set_decoded(decoded);
        edit(&mut b);
        let bytes = b.encode(&b.format, crate::codec::Lossy::Refuse).unwrap();
        let meta = Meta::new("name \u{e9}.txt".into(), Some(PathBuf::from(r"C:\some\file.txt")), &b.format, b.caret_char_position());
        (Snapshot { doc_id: id, meta, body: Body::Text(b.store().image()) }, bytes)
    }

    fn bytes_after_recovery(meta: &Meta, body: Body) -> Vec<u8> {
        let Body::Text(image) = body else {
            panic!("expected text");
        };
        let store = LineStore::from_image(image, None).unwrap();
        store.encode(&meta.format().unwrap(), crate::codec::Lossy::Refuse).unwrap()
    }

    #[test]
    fn a_copy_gives_back_the_exact_bytes_of_the_edited_document() {
        for (data, id) in [
            (&b"a\r\nb\nc\rd\xFF\xFE\n"[..], "utf-8"),
            (b"\xEF\xBB\xBFwith mark\r\n", "utf-8"),
            (b"caf\x82 \x81\x1A\x00", "cp437"),
            (&[0x41, 0x0E, 0x61, 0xC1, 0x0D, 0x93], "petscii-c64"),
            (b"\xFF\xFEa\x00\n\x00", "utf-16le"),
        ] {
            let (snapshot, expected) = snapshot_of(1, data, id, |b| {
                b.move_document_end(false);
                b.type_text("x");
                b.new_line();
            });
            let mut file = Vec::new();
            write_snapshot(&mut file, &snapshot).unwrap();
            let (meta, body) = read_snapshot(&file).unwrap();
            assert_eq!(meta, snapshot.meta);
            assert_eq!(bytes_after_recovery(&meta, body), expected, "for {id}");
        }
    }

    #[test]
    fn any_damage_to_a_copy_is_noticed() {
        let (snapshot, _) = snapshot_of(1, b"some text\nmore\n", "utf-8", |_| {});
        let mut file = Vec::new();
        write_snapshot(&mut file, &snapshot).unwrap();
        for at in (0..file.len()).step_by(7) {
            let mut bad = file.clone();
            bad[at] ^= 0x40;
            assert!(read_snapshot(&bad).is_err(), "a change at {at}");
        }
        for keep in (0..file.len()).step_by(11) {
            assert!(read_snapshot(&file[..keep]).is_err(), "cut short at {keep}");
        }
    }

    #[test]
    fn a_session_that_stopped_is_offered_and_one_still_running_is_not() {
        let root = temp_root("orphan");
        let mut running = Session::start(&root).unwrap();
        let (one, expected) = snapshot_of(7, b"typed before the crash\n", "utf-8", |_| {});
        running.offer(one);
        assert!(running.flush(Duration::from_secs(5)));
        let (orphans, found) = find_orphans(&root);
        assert!(orphans.is_empty() && found.is_empty(), "this one is still running");

        running.abandon();
        wait_until_let_go(&root);
        let (orphans, found) = find_orphans(&root);
        assert_eq!((orphans.len(), found.len()), (1, 1));
        let (second_look, _) = find_orphans(&root);
        assert!(second_look.is_empty(), "another window can't offer the same ones at the same time");
        let (meta, body) = found.into_iter().next().unwrap().outcome.unwrap();
        assert_eq!(bytes_after_recovery(&meta, body), expected);
        tidy(orphans);
        assert_eq!(find_orphans(&root).1.len(), 1, "undecided copies stay for next time");
        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn the_newest_copy_wins_and_a_damaged_one_falls_back_to_the_one_before() {
        let root = temp_root("fallback");
        let mut session = Session::start(&root).unwrap();
        for text in [&b"first\n"[..], b"second\n", b"third\n"] {
            let (snapshot, _) = snapshot_of(3, text, "utf-8", |_| {});
            session.write_now(&snapshot).unwrap();
        }
        let copy = copy_path(session.dir(), 3);
        let dir = session.dir().to_path_buf();
        session.abandon();
        wait_until_let_go(&root);
        let read = |path: &Path| {
            let (meta, body) = read_copy(path).unwrap();
            bytes_after_recovery(&meta, body)
        };
        assert_eq!(read(&copy), b"third\n");
        assert_eq!(read(&previous_of(&copy)), b"second\n");

        let mut data = fs::read(&copy).unwrap();
        data[40] ^= 1;
        fs::write(&copy, &data).unwrap();
        let (orphans, found) = find_orphans(&root);
        let (meta, body) = found.into_iter().next().unwrap().outcome.unwrap();
        assert_eq!(bytes_after_recovery(&meta, body), b"second\n");
        drop(orphans);

        // A crash between the two renames leaves only the previous copy.
        fs::remove_file(&copy).unwrap();
        let (orphans, found) = find_orphans(&root);
        assert_eq!(found.len(), 1);
        assert!(found[0].outcome.is_ok());
        drop(orphans);
        assert!(dir.exists());
        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn a_half_written_temporary_file_is_ignored() {
        let root = temp_root("half");
        let session = Session::start(&root).unwrap();
        fs::write(session.dir().join("doc-1.tmpThreadId3"), b"half a copy").unwrap();
        session.abandon();
        wait_until_let_go(&root);
        let (orphans, found) = find_orphans(&root);
        assert!(found.is_empty());
        assert!(orphans.is_empty(), "with no copies in it, the folder is tidied away");
        assert_eq!(fs::read_dir(&root).unwrap().count(), 0);
        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn a_clean_finish_leaves_nothing_and_saving_removes_the_copy() {
        let root = temp_root("clean");
        let mut session = Session::start(&root).unwrap();
        let (snapshot, _) = snapshot_of(5, b"text", "utf-8", |_| {});
        session.offer(snapshot);
        session.flush(Duration::from_secs(5));
        let copy = copy_path(session.dir(), 5);
        assert!(copy.exists());
        assert!(session.has_copy(5));
        session.remove(5);
        session.flush(Duration::from_secs(5));
        assert!(!copy.exists());
        let (snapshot, _) = snapshot_of(6, b"text", "utf-8", |_| {});
        session.offer(snapshot);
        session.keep_only(&HashSet::new());
        session.flush(Duration::from_secs(5));
        assert!(!copy_path(session.dir(), 6).exists(), "a tab that's closed takes its copy with it");
        session.finish();
        assert_eq!(fs::read_dir(&root).unwrap().count(), 0);
        fs::remove_dir_all(&root).unwrap();
    }

    /// The other half of the crash tests: run as its own process, so that it can really die.
    #[test]
    #[ignore = "only run by the crash tests, as a separate process"]
    fn crash_child() {
        let Some(root) = std::env::var_os("LIMONITER_CRASH_ROOT") else {
            return;
        };
        let how = std::env::var("LIMONITER_CRASH_HOW").unwrap_or_default();
        let mut session = Session::start(Path::new(&root)).unwrap();
        let data = "typed before the crash \u{e9}\r\nmixed\nendings\r".as_bytes();
        let (snapshot, _) = snapshot_of(11, data, "utf-8", |_| {});
        session.offer(snapshot);
        assert!(session.flush(Duration::from_secs(10)));
        match how.as_str() {
            // No clean-up of any kind runs after either of these.
            "abort" => std::process::abort(),
            "wait" => {
                // Keeps writing, as typing would, until it's killed from outside.
                let mut count = 0u32;
                loop {
                    let (snapshot, _) = snapshot_of(11, data, "utf-8", |b| {
                        b.move_document_end(false);
                        b.type_text(&count.to_string());
                    });
                    session.offer(snapshot);
                    count += 1;
                    std::thread::sleep(Duration::from_millis(3));
                }
            }
            _ => panic!("a failure on purpose"),
        }
    }

    fn start_child(root: &Path, how: &str) -> std::process::Child {
        std::process::Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "recovery::tests::crash_child", "--ignored", "--nocapture"])
            .env("LIMONITER_CRASH_ROOT", root)
            .env("LIMONITER_CRASH_HOW", how)
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
            .unwrap()
    }

    fn recovered_bytes(root: &Path) -> Vec<u8> {
        let (orphans, found) = find_orphans(root);
        assert_eq!(found.len(), 1, "one document to offer");
        let (meta, body) = found.into_iter().next().unwrap().outcome.unwrap();
        drop(orphans);
        bytes_after_recovery(&meta, body)
    }

    #[test]
    fn edits_survive_a_process_that_really_aborts_or_panics() {
        for how in ["abort", "panic"] {
            let root = temp_root(how);
            let status = start_child(&root, how).wait().unwrap();
            assert!(!status.success(), "{how}: it really did fail");
            assert_eq!(recovered_bytes(&root), "typed before the crash \u{e9}\r\nmixed\nendings\r".as_bytes(), "{how}");
            fs::remove_dir_all(&root).unwrap();
        }
    }

    /// A forced kill can land in the middle of writing a copy. Whenever it lands, a whole copy must be there.
    #[test]
    fn edits_survive_being_killed_at_any_moment() {
        for round in 0..12u64 {
            let root = temp_root(&format!("kill{round}"));
            let mut child = start_child(&root, "wait");
            let deadline = Instant::now() + Duration::from_secs(30);
            // Wait for the first copy, then let it run on for a varying moment before killing it.
            while Instant::now() < deadline {
                let has_copy = fs::read_dir(&root).is_ok_and(|dirs| dirs.flatten().any(|d| fs::read_dir(d.path()).is_ok_and(|mut files| files.any(|f| f.is_ok_and(|f| f.path().extension().is_some_and(|x| x == "rec"))))));
                if has_copy {
                    break;
                }
                std::thread::sleep(Duration::from_millis(5));
            }
            std::thread::sleep(Duration::from_millis(round * 37));
            child.kill().unwrap();
            child.wait().unwrap();
            let bytes = recovered_bytes(&root);
            assert!(bytes.starts_with("typed before the crash".as_bytes()), "round {round}: a whole, readable copy");
            fs::remove_dir_all(&root).unwrap();
        }
    }

    #[test]
    fn hex_documents_and_odd_paths_come_back_exactly() {
        let bytes: Vec<u8> = (0..=255).collect();
        let format = TextFormat::default();
        // A path that isn't valid Unicode, as each system allows.
        #[cfg(windows)]
        let odd = <std::ffi::OsString as std::os::windows::ffi::OsStringExt>::from_wide(&[0x43, 0x3A, 0xD800, 0x41]);
        #[cfg(not(windows))]
        let odd = <std::ffi::OsString as std::os::unix::ffi::OsStringExt>::from_vec(vec![b'/', b'C', 0xFF, b'A']);
        let meta = Meta { path: Some(PathBuf::from(odd)), ..Meta::new("bin".into(), None, &format, (0, 9)) };
        let snapshot = Snapshot { doc_id: 2, meta: meta.clone(), body: Body::Hex(bytes.clone()) };
        let mut file = Vec::new();
        write_snapshot(&mut file, &snapshot).unwrap();
        let (read, body) = read_snapshot(&file).unwrap();
        assert_eq!(read, meta);
        assert!(matches!(body, Body::Hex(found) if found == bytes));
    }

    #[test]
    fn a_failed_write_is_reported_and_keeps_the_writer_going() {
        let root = temp_root("fail");
        let mut session = Session::start(&root).unwrap();
        let dir = session.dir().to_path_buf();
        // Folders in the way of both of the copy's names, so it can't be written.
        for name in [copy_path(&dir, 9), previous_of(&copy_path(&dir, 9))] {
            fs::create_dir_all(&name).unwrap();
            fs::write(name.join("x"), b"x").unwrap();
        }
        let (snapshot, _) = snapshot_of(9, b"text", "utf-8", |_| {});
        session.offer(snapshot);
        session.flush(Duration::from_secs(5));
        assert!(session.problem().is_some(), "said out loud, not silent");
        let (snapshot, _) = snapshot_of(10, b"text", "utf-8", |_| {});
        session.offer(snapshot);
        session.flush(Duration::from_secs(5));
        assert!(session.problem().is_none());
        assert!(copy_path(&dir, 10).exists());
        session.finish();
        fs::remove_dir_all(&root).unwrap();
    }
}
