//! The lines of a document. A small file sits in memory. A huge one stays on disk: a background
//! scan notes where every 256th line starts, only the lines being looked at are read, and edits
//! are kept as small in-memory pieces between untouched stretches of the file.

use crate::charset::{self, Charset};
use crate::codec::{self, Certainty, Eol, EolCounts, Lossy, Problems, TextFormat};
use std::collections::{BTreeMap, BTreeSet, HashMap, VecDeque};
use std::fs::File;
use std::io::{BufWriter, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, RwLock};
use std::time::SystemTime;

const BLOCK_LINES: usize = 256;
const CACHED_BLOCKS: usize = 128;
const SCAN_CHUNK: usize = 4 * 1024 * 1024;
const COPY_CHUNK: usize = 1024 * 1024;
const MERGE_LIMIT: usize = 4096;

/// Enough to rebuild a document's lines after a crash: what's been edited, and where the rest is in the file.
pub struct StoreImage {
    pub base: Eol,
    pub insert: Eol,
    pub eols: Vec<(usize, Eol)>,
    pub bookmarks: Vec<usize>,
    pub pieces: Vec<ImagePiece>,
    pub source: Option<SourceRef>,
}

pub enum ImagePiece {
    Disk { first: usize, count: usize },
    Memory(Vec<Arc<str>>),
}

/// Which file a store's untouched lines are in, exactly as it was when it was read.
pub struct SourceRef {
    pub path: PathBuf,
    pub len: u64,
    pub modified: Option<SystemTime>,
}

struct Block {
    lines: Vec<Arc<str>>,
    /// Where each line starts in the file, plus where the block ends.
    starts: Vec<u64>,
    /// How each line ended in the file; None for the very last line.
    endings: Vec<Option<Eol>>,
}

#[derive(Default)]
struct BlockCache {
    blocks: HashMap<usize, Arc<Block>>,
    order: VecDeque<usize>,
}

/// A file on disk, open for reading, with its line positions being worked out in the background.
pub struct Source {
    file: File,
    path: PathBuf,
    len: u64,
    modified: Option<SystemTime>,
    bom_len: u64,
    index: RwLock<Vec<u64>>,
    complete_lines: AtomicUsize,
    known_end: AtomicU64,
    total_lines: AtomicUsize,
    scanned: AtomicU64,
    finished: AtomicBool,
    cancel: AtomicBool,
    /// Not valid UTF-8, so a byte set was suggested instead.
    not_utf8: AtomicBool,
    charset: RwLock<&'static Charset>,
    /// The set was chosen by the person, so bytes that aren't UTF-8 don't change it.
    chosen: bool,
    failed: Mutex<Option<String>>,
    longest: AtomicUsize,
    crlf: AtomicUsize,
    lf: AtomicUsize,
    cr: AtomicUsize,
    cache: Mutex<BlockCache>,
}

impl Source {
    /// Opens a file and starts finding its lines; the first lines can be read straight away.
    #[cfg(test)]
    pub fn open(path: &Path) -> std::io::Result<Arc<Self>> {
        Self::open_as(path, None)
    }

    /// The same, reading it through a chosen set. Read as a byte set, a UTF-8 mark is just its bytes.
    pub fn open_as(path: &Path, chosen: Option<&'static Charset>) -> std::io::Result<Arc<Self>> {
        let file = File::open(path)?;
        let meta = file.metadata()?;
        let mut head = [0u8; 3];
        let got = crate::byte_store::read_at(&file, &mut head, 0)?;
        let wants_mark = chosen.is_none_or(Charset::is_unicode);
        let bom_len = if wants_mark && got == 3 && head == [0xEF, 0xBB, 0xBF] { 3 } else { 0 };
        let source = Arc::new(Self {
            file,
            path: path.to_path_buf(),
            len: meta.len(),
            modified: meta.modified().ok(),
            bom_len,
            index: RwLock::new(vec![bom_len]),
            complete_lines: AtomicUsize::new(0),
            known_end: AtomicU64::new(bom_len),
            total_lines: AtomicUsize::new(0),
            scanned: AtomicU64::new(bom_len),
            finished: AtomicBool::new(false),
            cancel: AtomicBool::new(false),
            not_utf8: AtomicBool::new(false),
            charset: RwLock::new(chosen.unwrap_or_else(charset::utf8)),
            chosen: chosen.is_some(),
            failed: Mutex::new(None),
            longest: AtomicUsize::new(0),
            crlf: AtomicUsize::new(0),
            lf: AtomicUsize::new(0),
            cr: AtomicUsize::new(0),
            cache: Mutex::new(BlockCache::default()),
        });
        let worker = source.clone();
        std::thread::spawn(move || {
            // Whatever goes wrong here, the window is told, rather than waiting for ever.
            match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| worker.scan())) {
                Ok(Ok(())) => {}
                Ok(Err(e)) => *worker.failed.lock().unwrap() = Some(e.to_string()),
                Err(_) => *worker.failed.lock().unwrap() = Some("Reading the file failed unexpectedly.".to_string()),
            }
            worker.finished.store(true, Ordering::SeqCst);
        });
        Ok(source)
    }

    fn scan(&self) -> std::io::Result<()> {
        let mut buffer = vec![0u8; SCAN_CHUNK];
        let mut position = self.bom_len;
        let mut line_start = self.bom_len;
        let mut lines = 0usize;
        let mut skip_before = 0u64;
        let mut carry: Vec<u8> = Vec::new();
        let mut valid = true;
        while position < self.len {
            if self.cancel.load(Ordering::Relaxed) {
                return Ok(());
            }
            let got = crate::byte_store::read_at(&self.file, &mut buffer, position)?;
            if got == 0 {
                break;
            }
            let chunk = &buffer[..got];
            if valid {
                valid = check_utf8(chunk, &mut carry);
                if !valid {
                    self.use_suggested_set();
                }
            }
            for hit in memchr::memchr2_iter(b'\n', b'\r', chunk) {
                let at = position + hit as u64;
                if at < skip_before {
                    continue;
                }
                let next = if chunk[hit] == b'\n' {
                    self.lf.fetch_add(1, Ordering::Relaxed);
                    at + 1
                } else if self.byte_after(chunk, hit, at)? == Some(b'\n') {
                    self.crlf.fetch_add(1, Ordering::Relaxed);
                    skip_before = at + 2;
                    at + 2
                } else {
                    self.cr.fetch_add(1, Ordering::Relaxed);
                    at + 1
                };
                self.longest.fetch_max((at - line_start) as usize, Ordering::Relaxed);
                line_start = next;
                lines += 1;
                if lines.is_multiple_of(BLOCK_LINES) {
                    self.index.write().unwrap().push(next);
                }
            }
            position += got as u64;
            self.known_end.store(line_start, Ordering::SeqCst);
            self.complete_lines.store(lines, Ordering::SeqCst);
            self.scanned.store(position, Ordering::Relaxed);
        }
        if valid && !carry.is_empty() {
            self.use_suggested_set();
        }
        self.longest.fetch_max((self.len.saturating_sub(line_start)) as usize, Ordering::Relaxed);
        self.total_lines.store(lines + 1, Ordering::SeqCst);
        self.scanned.store(self.len, Ordering::Relaxed);
        Ok(())
    }

    fn byte_after(&self, chunk: &[u8], hit: usize, at: u64) -> std::io::Result<Option<u8>> {
        if let Some(b) = chunk.get(hit + 1) {
            return Ok(Some(*b));
        }
        let mut one = [0u8; 1];
        Ok((crate::byte_store::read_at(&self.file, &mut one, at + 1)? == 1).then_some(one[0]))
    }

    /// Bytes that aren't UTF-8 stay exactly as they are; this only chooses how they're shown, as a suggestion.
    fn use_suggested_set(&self) {
        if self.chosen {
            return;
        }
        self.not_utf8.store(true, Ordering::SeqCst);
        self.set_charset(charset::known("windows-1252"));
    }

    /// Reads the file through another set from now on. The file itself isn't touched.
    fn set_charset(&self, set: &'static Charset) {
        *self.charset.write().unwrap() = set;
        let mut cache = self.cache.lock().unwrap();
        cache.blocks.clear();
        cache.order.clear();
    }

    pub fn has_bom(&self) -> bool {
        self.bom_len > 0
    }

    /// How many of each line ending the scan has found.
    pub fn eol_counts(&self) -> EolCounts {
        EolCounts { crlf: self.crlf.load(Ordering::Relaxed), lf: self.lf.load(Ordering::Relaxed), cr: self.cr.load(Ordering::Relaxed), atascii: 0, nl: 0 }
    }

    /// How a line ended in the file. The last line has no ending.
    fn terminator(&self, line: usize) -> Option<Eol> {
        let block = self.block(line / BLOCK_LINES).ok()?;
        block.endings.get(line % BLOCK_LINES).copied().flatten()
    }

    pub fn is_finished(&self) -> bool {
        self.finished.load(Ordering::SeqCst)
    }

    pub fn failure(&self) -> Option<String> {
        self.failed.lock().unwrap().clone()
    }

    pub fn progress(&self) -> f32 {
        if self.len == 0 { 1.0 } else { self.scanned.load(Ordering::Relaxed) as f32 / self.len as f32 }
    }

    pub fn stop(&self) {
        self.cancel.store(true, Ordering::Relaxed);
    }

    /// How many lines can be shown so far; the last line is only known at the end.
    pub fn known_lines(&self) -> usize {
        match self.total_lines.load(Ordering::SeqCst) {
            0 => self.complete_lines.load(Ordering::SeqCst),
            total => total,
        }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn file_size(&self) -> u64 {
        self.len
    }

    /// Whether another program has changed the file since it was opened.
    pub fn changed_on_disk(&self) -> bool {
        match std::fs::metadata(&self.path) {
            Ok(m) => m.len() != self.len || m.modified().ok() != self.modified || !same_identity(&self.file, &m),
            Err(_) => true,
        }
    }

    /// The text format found by the scan.
    pub fn format(&self) -> TextFormat {
        let set = *self.charset.read().unwrap();
        let certainty = if self.not_utf8.load(Ordering::SeqCst) {
            Certainty::Suggested
        } else if self.bom_len > 0 {
            Certainty::Bom
        } else {
            Certainty::Valid
        };
        TextFormat { charset: set, has_bom: set.holds_characters() && self.bom_len > 0, certainty, ..TextFormat::default() }
    }

    fn decode(&self, bytes: &[u8]) -> Arc<str> {
        let set = *self.charset.read().unwrap();
        Arc::from(codec::decode_line(bytes, set))
    }

    fn block(&self, number: usize) -> std::io::Result<Arc<Block>> {
        if let Some(found) = self.cache.lock().unwrap().blocks.get(&number) {
            return Ok(found.clone());
        }
        let lines_from = |lines: usize| lines.saturating_sub(number * BLOCK_LINES).min(BLOCK_LINES);
        // How many lines it holds has to come from the same place as where it ends. The scan notes a block's end as
        // soon as it reaches it, but only counts its lines at the end of each chunk it reads, so going by that count
        // while it's still scanning would keep a whole block of blank lines.
        let (start, end, complete, expected) = {
            let index = self.index.read().unwrap();
            let start = index.get(number).copied().unwrap_or(self.len);
            let total = self.total_lines.load(Ordering::SeqCst);
            match index.get(number + 1) {
                Some(end) => (start, *end, true, BLOCK_LINES),
                None if total > 0 => (start, self.len, true, lines_from(total)),
                None => {
                    // In this order, there are at least as many bytes as the lines say.
                    let lines = self.complete_lines.load(Ordering::SeqCst);
                    (start, self.known_end.load(Ordering::SeqCst).max(start), false, lines_from(lines))
                }
            }
        };
        let mut bytes = vec![0u8; (end - start) as usize];
        let mut filled = 0;
        while filled < bytes.len() {
            let got = crate::byte_store::read_at(&self.file, &mut bytes[filled..], start + filled as u64)?;
            if got == 0 {
                bytes.truncate(filled);
                break;
            }
            filled += got;
        }
        let block = Arc::new(self.parse_block(&bytes, start, expected));
        if complete {
            let mut cache = self.cache.lock().unwrap();
            if cache.order.len() >= CACHED_BLOCKS
                && let Some(oldest) = cache.order.pop_front()
            {
                cache.blocks.remove(&oldest);
            }
            cache.order.push_back(number);
            cache.blocks.insert(number, block.clone());
        }
        Ok(block)
    }

    fn parse_block(&self, bytes: &[u8], base: u64, expected: usize) -> Block {
        let mut lines = Vec::with_capacity(expected);
        let mut starts = Vec::with_capacity(expected + 1);
        let mut endings = Vec::with_capacity(expected);
        let mut line_start = 0usize;
        let mut cursor = 0usize;
        while lines.len() < expected {
            starts.push(base + line_start as u64);
            match memchr::memchr2(b'\n', b'\r', &bytes[cursor..]) {
                Some(hit) => {
                    let at = cursor + hit;
                    let next = if bytes[at] == b'\r' && bytes.get(at + 1) == Some(&b'\n') { at + 2 } else { at + 1 };
                    endings.push(Some(if next == at + 2 {
                        Eol::CrLf
                    } else if bytes[at] == b'\r' {
                        Eol::Cr
                    } else {
                        Eol::Lf
                    }));
                    lines.push(self.decode(&bytes[line_start..at]));
                    line_start = next;
                    cursor = next;
                }
                None => {
                    endings.push(None);
                    lines.push(self.decode(&bytes[line_start..]));
                    line_start = bytes.len();
                    cursor = bytes.len();
                }
            }
        }
        starts.push(base + line_start as u64);
        Block { lines, starts, endings }
    }

    fn line(&self, i: usize) -> Arc<str> {
        match self.block(i / BLOCK_LINES) {
            Ok(block) => block.lines.get(i % BLOCK_LINES).cloned().unwrap_or_else(|| Arc::from("")),
            Err(_) => Arc::from(""),
        }
    }

    /// The line that holds a byte of the file, if the file has been read that far.
    pub fn line_at_offset(&self, offset: u64) -> Option<usize> {
        let block_index = {
            let index = self.index.read().unwrap_or_else(std::sync::PoisonError::into_inner);
            index.partition_point(|&start| start <= offset).checked_sub(1)?
        };
        if block_index * BLOCK_LINES >= self.known_lines() {
            return None;
        }
        let block = self.block(block_index).ok()?;
        let within = block.starts.partition_point(|&start| start <= offset).checked_sub(1)?;
        Some((block_index * BLOCK_LINES + within).min(self.known_lines().saturating_sub(1)))
    }

    pub fn line_start(&self, i: usize) -> std::io::Result<u64> {
        if i >= self.known_lines() {
            return Ok(self.len);
        }
        let block = self.block(i / BLOCK_LINES)?;
        Ok(block.starts.get(i % BLOCK_LINES).copied().unwrap_or(self.len))
    }
}

/// Whether the open file is still the one at its path. A save puts a new file in place, which the modified time
/// alone can miss where it's only kept to a few milliseconds, as on Linux.
#[cfg(unix)]
fn same_identity(file: &File, now: &std::fs::Metadata) -> bool {
    use std::os::unix::fs::MetadataExt;
    file.metadata().is_ok_and(|open| open.ino() == now.ino() && open.dev() == now.dev())
}

#[cfg(not(unix))]
fn same_identity(_file: &File, _now: &std::fs::Metadata) -> bool {
    true
}

/// Reports whether the data so far is valid UTF-8, holding back a character split across chunks.
fn check_utf8(chunk: &[u8], carry: &mut Vec<u8>) -> bool {
    let joined;
    let data: &[u8] = if carry.is_empty() {
        chunk
    } else {
        joined = [carry.as_slice(), chunk].concat();
        &joined
    };
    match std::str::from_utf8(data) {
        Ok(_) => {
            carry.clear();
            true
        }
        Err(e) if e.error_len().is_none() => {
            let tail = data[e.valid_up_to()..].to_vec();
            *carry = tail;
            true
        }
        Err(_) => false,
    }
}

#[derive(Clone)]
enum Piece {
    Disk { source: Arc<Source>, first: usize, count: usize },
    Memory(Vec<Arc<str>>),
}

impl Piece {
    fn len(&self) -> usize {
        match self {
            Piece::Disk { count, .. } => *count,
            Piece::Memory(lines) => lines.len(),
        }
    }
}

/// All the lines of a document, wherever each one currently lives.
#[derive(Clone)]
pub struct LineStore {
    pieces: Vec<Piece>,
    starts: Vec<usize>,
    total: usize,
    bookmarks: BTreeSet<usize>,
    /// Lines changed since the file was opened, each saying whether that's been saved since.
    changes: Arc<BTreeMap<usize, bool>>,
    /// Folded stretches: the line that stays showing, and the last line hidden after it.
    folds: Arc<BTreeMap<usize, usize>>,
    /// Colour-tagged stretches: the first line, and the last line and colour.
    tags: Arc<BTreeMap<usize, (usize, u8)>>,
    /// The first line changed since this was last asked, for the colouring to catch up from.
    changed_from: usize,
    /// The ending of a line in memory, unless it's listed in `eols`.
    base_eol: Eol,
    /// What a new line ends with.
    insert_eol: Eol,
    /// Lines ending differently from `base_eol`. Lines still on disk keep what the file has.
    eols: Arc<BTreeMap<usize, Eol>>,
}

impl LineStore {
    pub fn from_vec(lines: Vec<Arc<str>>) -> Self {
        let total = lines.len();
        Self { pieces: vec![Piece::Memory(lines)], starts: vec![0], total, bookmarks: BTreeSet::new(), changes: Arc::default(), folds: Arc::default(), tags: Arc::default(), changed_from: 0, base_eol: Eol::CrLf, insert_eol: Eol::CrLf, eols: Arc::new(BTreeMap::new()) }
    }

    /// Lines as a file had them, each with its own ending.
    pub fn from_decoded(lines: Vec<Arc<str>>, base: Eol, exceptions: BTreeMap<usize, Eol>) -> Self {
        let mut store = Self::from_vec(lines);
        store.base_eol = base;
        store.insert_eol = base;
        store.eols = Arc::new(exceptions);
        store
    }

    pub fn from_source(source: Arc<Source>) -> Self {
        let count = source.known_lines();
        let base = source.eol_counts().predominant(Eol::CrLf);
        Self { pieces: vec![Piece::Disk { source, first: 0, count }], starts: vec![0], total: count, bookmarks: BTreeSet::new(), changes: Arc::default(), folds: Arc::default(), tags: Arc::default(), changed_from: 0, base_eol: base, insert_eol: base, eols: Arc::new(BTreeMap::new()) }
    }

    pub fn len(&self) -> usize {
        self.total
    }

    pub fn is_disk_backed(&self) -> bool {
        self.pieces.iter().any(|p| matches!(p, Piece::Disk { .. }))
    }

    /// Lines held in memory, which is what undo snapshots cost.
    pub fn memory_lines(&self) -> usize {
        self.pieces.iter().filter(|p| matches!(p, Piece::Memory(_))).map(Piece::len).sum()
    }

    /// What a recovery copy needs. Only edited lines are held in it; the rest stay in the file.
    pub fn image(&self) -> StoreImage {
        StoreImage {
            base: self.base_eol,
            insert: self.insert_eol,
            eols: self.eols.iter().map(|(line, eol)| (*line, *eol)).collect(),
            bookmarks: self.bookmarks(),
            pieces: self
                .pieces
                .iter()
                .map(|piece| match piece {
                    Piece::Disk { first, count, .. } => ImagePiece::Disk { first: *first, count: *count },
                    Piece::Memory(lines) => ImagePiece::Memory(lines.clone()),
                })
                .collect(),
            source: self.source().map(|source| SourceRef { path: source.path.clone(), len: source.len, modified: source.modified }),
        }
    }

    /// Rebuilds a document from a recovery copy. Returns None if it needs the file and hasn't got it.
    pub fn from_image(image: StoreImage, source: Option<Arc<Source>>) -> Option<Self> {
        let mut pieces = Vec::with_capacity(image.pieces.len());
        for piece in image.pieces {
            pieces.push(match piece {
                ImagePiece::Disk { first, count } => Piece::Disk { source: source.clone()?, first, count },
                ImagePiece::Memory(lines) => Piece::Memory(lines),
            });
        }
        if pieces.is_empty() {
            pieces.push(Piece::Memory(vec![Arc::from("")]));
        }
        let mut store = Self {
            pieces,
            starts: Vec::new(),
            total: 0,
            bookmarks: image.bookmarks.into_iter().collect(),
            changes: Arc::default(),
            folds: Arc::default(),
            tags: Arc::default(),
            changed_from: 0,
            base_eol: image.base,
            insert_eol: image.insert,
            eols: Arc::new(image.eols.into_iter().collect()),
        };
        store.reindex();
        if store.total == 0 {
            store.pieces = vec![Piece::Memory(vec![Arc::from("")])];
            store.reindex();
        }
        Some(store)
    }

    /// While a file is still being scanned, its line count keeps growing.
    pub fn refresh_from_source(&mut self) {
        if let [Piece::Disk { source, first: 0, count }] = self.pieces.as_mut_slice() {
            *count = source.known_lines().max(1);
            self.total = *count;
            let base = source.eol_counts().predominant(Eol::CrLf);
            self.base_eol = base;
            self.insert_eol = base;
        }
    }

    pub fn source(&self) -> Option<Arc<Source>> {
        self.pieces.iter().find_map(|p| match p {
            Piece::Disk { source, .. } => Some(source.clone()),
            Piece::Memory(_) => None,
        })
    }

    /// The longest line in bytes, an upper limit for how wide the view needs to scroll.
    pub fn longest_estimate(&self) -> usize {
        let mut longest = 0;
        for piece in &self.pieces {
            match piece {
                Piece::Disk { source, .. } => longest = longest.max(source.longest.load(Ordering::Relaxed)),
                Piece::Memory(lines) => {
                    for line in lines {
                        longest = longest.max(line.len());
                    }
                }
            }
        }
        longest
    }

    fn locate(&self, i: usize) -> (usize, usize) {
        let piece = self.starts.partition_point(|s| *s <= i).saturating_sub(1);
        (piece, i - self.starts[piece])
    }

    /// A line's text. Asking past the end gets an empty line: a slip in a position is never worth a crash.
    pub fn get(&self, i: usize) -> Arc<str> {
        let (piece, offset) = self.locate(i);
        match &self.pieces[piece] {
            Piece::Disk { source, first, .. } => source.line(first + offset),
            Piece::Memory(lines) => lines.get(offset).cloned().unwrap_or_else(|| Arc::from("")),
        }
    }

    pub fn range_vec(&self, start: usize, end: usize) -> Vec<Arc<str>> {
        (start..end).map(|i| self.get(i)).collect()
    }

    pub fn insert_eol(&self) -> Eol {
        self.insert_eol
    }

    pub fn set_insert_eol(&mut self, eol: Eol) {
        self.insert_eol = eol;
    }

    /// The ending a line has, whether from the file or from an edit.
    pub fn terminator(&self, i: usize) -> Eol {
        if let Some(eol) = self.eols.get(&i) {
            return *eol;
        }
        let (piece, offset) = self.locate(i);
        match &self.pieces[piece] {
            Piece::Disk { source, first, .. } => source.terminator(first + offset).unwrap_or(self.base_eol),
            Piece::Memory(_) => self.base_eol,
        }
    }

    /// Gives a line in memory an ending, keeping the list of exceptions as short as it can be.
    fn set_terminator(&mut self, i: usize, eol: Eol) {
        let eols = Arc::make_mut(&mut self.eols);
        if eol == self.base_eol {
            eols.remove(&i);
        } else {
            eols.insert(i, eol);
        }
    }

    /// For when a line has just been joined to the ones after it: it now ends the way the last of them did.
    pub fn take_terminator_from(&mut self, to: usize, from: usize) {
        if from < self.total {
            let eol = self.terminator(from);
            self.set_terminator(to, eol);
        }
    }

    /// For when a line has just been split in two or more: the old ending stays at the far end,
    /// and only the breaks that are new use the insertion style.
    pub fn split_terminator(&mut self, line: usize, added: usize) {
        let eol = self.terminator(line);
        self.set_terminator(line + added, eol);
        self.set_terminator(line, self.insert_eol);
    }

    /// How many of each line ending the document has now.
    pub fn eol_counts(&self) -> EolCounts {
        let last = self.total.saturating_sub(1);
        let listed = self.eols.range(..last).count();
        if self.is_disk_backed() {
            let from_source = self.source().map_or_else(EolCounts::default, |source| source.eol_counts());
            return from_source;
        }
        let listed_kinds = self.eols.range(..last).map(|(_, eol)| (*eol, 1));
        EolCounts::of(listed_kinds.chain(std::iter::once((self.base_eol, last - listed))))
    }

    /// Makes every line end the same way, and new lines too. Only for a document held in memory.
    pub fn convert_all_eols(&mut self, eol: Eol) {
        self.base_eol = eol;
        self.insert_eol = eol;
        self.eols = Arc::new(BTreeMap::new());
    }

    /// Encodes the whole document, for one held in memory.
    pub fn encode(&self, format: &TextFormat, lossy: Lossy) -> Result<Vec<u8>, Problems> {
        let lines = self.range_vec(0, self.total);
        codec::encode_with(&lines, &|i| self.terminator(i), format, lossy)
    }

    pub fn note_changed(&mut self, line: usize) {
        self.changed_from = self.changed_from.min(line);
    }

    /// The first line changed since the last time this was asked.
    pub fn take_changed_from(&mut self) -> Option<usize> {
        let from = self.changed_from;
        self.changed_from = usize::MAX;
        (from != usize::MAX).then_some(from)
    }

    /// Changes many lines at once, in one pass, however many there are. `edits` is in line order. Doing these
    /// one by one on a file kept on disk gets slower with each, and a million of them would never finish.
    /// Each comes with the ending its line has now, which a line leaving the disk for memory holds on to. They're
    /// passed in because whoever found the lines has just read them, and reading them again here would freeze the window.
    pub fn set_many(&mut self, edits: Vec<(usize, Arc<str>, Eol)>) {
        let Some(first) = edits.first().map(|edit| edit.0) else {
            return;
        };
        self.note_changed(first);
        for (line, _, eol) in &edits {
            self.set_terminator(*line, *eol);
        }
        let changes = Arc::make_mut(&mut self.changes);
        for (line, _, _) in &edits {
            changes.insert(*line, false);
        }
        if !self.folds.is_empty() {
            let kept = self.folds.iter().filter(|(start, last)| {
                let from = edits.partition_point(|edit| edit.0 <= **start);
                edits.get(from).is_none_or(|edit| edit.0 > **last)
            });
            self.folds = Arc::new(kept.map(|(start, last)| (*start, *last)).collect());
        }
        let mut edits = edits.into_iter().peekable();
        let mut result: Vec<Piece> = Vec::with_capacity(self.pieces.len());
        let push_line = |result: &mut Vec<Piece>, text: Arc<str>| match result.last_mut() {
            Some(Piece::Memory(lines)) if lines.len() < MERGE_LIMIT => lines.push(text),
            _ => result.push(Piece::Memory(vec![text])),
        };
        let mut position = 0;
        for piece in std::mem::take(&mut self.pieces) {
            let end = position + piece.len();
            match piece {
                Piece::Memory(mut lines) => {
                    while let Some((line, text, _)) = edits.next_if(|edit| edit.0 < end) {
                        lines[line - position] = text;
                    }
                    result.push(Piece::Memory(lines));
                }
                Piece::Disk { source, first, count } => {
                    let mut done = 0;
                    while let Some((line, text, _)) = edits.next_if(|edit| edit.0 < end) {
                        let at = line - position;
                        if at > done {
                            result.push(Piece::Disk { source: source.clone(), first: first + done, count: at - done });
                        }
                        push_line(&mut result, text);
                        done = at + 1;
                    }
                    if done < count {
                        result.push(Piece::Disk { source, first: first + done, count: count - done });
                    }
                }
            }
            position = end;
        }
        self.pieces = result;
        self.reindex();
    }

    pub fn set(&mut self, i: usize, line: Arc<str>) {
        self.note_changed(i);
        Arc::make_mut(&mut self.changes).insert(i, false);
        let (piece, offset) = self.locate(i);
        if let Piece::Memory(lines) = &mut self.pieces[piece] {
            lines[offset] = line;
        } else {
            self.replace_range(i, i + 1, vec![line]);
        }
    }

    pub fn insert(&mut self, i: usize, line: Arc<str>) {
        self.replace_range(i, i, vec![line]);
    }

    pub fn insert_many(&mut self, i: usize, lines: Vec<Arc<str>>) {
        self.replace_range(i, i, lines);
    }

    pub fn remove(&mut self, i: usize) {
        self.replace_range(i, i + 1, Vec::new());
    }

    pub fn remove_range(&mut self, start: usize, end: usize) {
        self.replace_range(start, end, Vec::new());
    }

    pub fn push(&mut self, line: Arc<str>) {
        self.replace_range(self.total, self.total, vec![line]);
    }

    /// Swaps whole lines for new ones. The last of them ends as the last of the old ones did, so what's
    /// after them doesn't move, and a file with mixed line endings keeps them.
    pub fn replace_lines(&mut self, start: usize, end: usize, new: Vec<Arc<str>>) {
        let ending = (end > start).then(|| self.terminator(end - 1));
        let count = new.len();
        self.replace_range(start, end, new);
        if let Some(ending) = ending.filter(|_| count > 0) {
            self.set_terminator(start + count - 1, ending);
        }
    }

    /// Some lines by themselves, keeping their endings and bookmarks, numbered from 0.
    pub fn extract(&self, top: usize, bottom: usize) -> Self {
        let mut exceptions = BTreeMap::new();
        for line in top..=bottom {
            let eol = self.terminator(line);
            if eol != self.base_eol {
                exceptions.insert(line - top, eol);
            }
        }
        let mut store = Self::from_decoded(self.range_vec(top, bottom + 1), self.base_eol, exceptions);
        store.insert_eol = self.insert_eol;
        store.bookmarks = self.bookmarks.range(top..=bottom).map(|line| line - top).collect();
        store.tags = Arc::new(self.tags.iter().filter(|(first, (last, _))| **first >= top && *last <= bottom).map(|(first, (last, colour))| (first - top, (last - top, *colour))).collect());
        store
    }

    /// How much of the start and end of these lines is the same in `region`, endings included.
    fn region_overlap(&self, top: usize, bottom: usize, region: &Self) -> (usize, usize) {
        let old = self.range_vec(top, bottom + 1);
        let new = region.range_vec(0, region.len());
        let same = |i: usize, j: usize| old[i] == new[j] && self.terminator(top + i) == region.terminator(j);
        let room = old.len().min(new.len());
        let start = (0..room).take_while(|at| same(*at, *at)).count();
        let end = (0..room - start).take_while(|back| same(old.len() - 1 - back, new.len() - 1 - back)).count();
        (start, end)
    }

    /// Whether `region` holds exactly what these lines do.
    pub fn region_is_same(&self, top: usize, bottom: usize, region: &Self) -> bool {
        let (start, end) = self.region_overlap(top, bottom, region);
        let count = bottom + 1 - top;
        region.len() == count && start + end == count
    }

    /// Puts lines from `extract` back in place of these. Lines that haven't changed aren't touched, so a line still
    /// on disk stays there, and only what's really different counts as changed.
    pub fn splice_region(&mut self, top: usize, bottom: usize, region: &Self) {
        let (start, end) = self.region_overlap(top, bottom, region);
        let new_len = region.len();
        if start + end < new_len || start + end < bottom + 1 - top {
            let middle = region.range_vec(start, new_len - end);
            self.replace_range(top + start, bottom + 1 - end, middle);
            for line in start..new_len - end {
                self.set_terminator(top + line, region.terminator(line));
            }
        }
        let inside: Vec<usize> = self.bookmarks.range(top..top + new_len).copied().collect();
        for line in inside {
            self.bookmarks.remove(&line);
        }
        self.bookmarks.extend(region.bookmarks.iter().map(|line| line + top));
        let tags = Arc::make_mut(&mut self.tags);
        tags.retain(|first, (last, _)| *last < top || *first >= top + new_len);
        tags.extend(region.tags.iter().map(|(first, (last, colour))| (first + top, (last + top, *colour))));
    }

    /// How line endings are held, for writing them down.
    pub fn eol_parts(&self) -> (Eol, Eol, BTreeMap<usize, Eol>) {
        (self.base_eol, self.insert_eol, (*self.eols).clone())
    }

    /// Swaps lines start..end for new ones, without reading any of the lines it removes.
    pub fn replace_range(&mut self, start: usize, end: usize, new: Vec<Arc<str>>) {
        if start == end && new.is_empty() {
            return;
        }
        self.note_changed(start);
        self.move_bookmarks(start, end, new.len());
        self.move_changes(start, end, new.len());
        self.move_folds(start, end, new.len());
        self.move_tags(start, end, new.len());
        self.move_terminators(start, end, new.len());
        // A change inside one memory piece is done on the spot.
        if self.total > 0 {
            let (piece, offset) = self.locate(start.min(self.total - 1));
            let position = self.starts[piece] + offset;
            let piece_len = self.pieces[piece].len();
            let inside = start >= self.starts[piece] && end <= self.starts[piece] + piece_len;
            if inside && position <= start && let Piece::Memory(lines) = &mut self.pieces[piece] {
                let from = start - self.starts[piece];
                let to = end - self.starts[piece];
                lines.splice(from..to, new);
                self.reindex();
                return;
            }
        }
        let mut result: Vec<Piece> = Vec::with_capacity(self.pieces.len() + 2);
        let mut position = 0;
        let mut inserted = false;
        let mut pending = Some(new);
        for piece in self.pieces.drain(..) {
            let piece_start = position;
            let piece_end = position + piece.len();
            position = piece_end;
            if let Some(left) = slice_piece(&piece, piece_start, 0..start.min(piece_end).saturating_sub(piece_start)) {
                result.push(left);
            }
            if !inserted && start <= piece_end {
                if let Some(lines) = pending.take()
                    && !lines.is_empty()
                {
                    result.push(Piece::Memory(lines));
                }
                inserted = true;
            }
            let keep_from = end.max(piece_start) - piece_start;
            if let Some(right) = slice_piece(&piece, piece_start, keep_from..piece_end - piece_start) {
                result.push(right);
            }
        }
        if let Some(lines) = pending.take()
            && !lines.is_empty()
        {
            result.push(Piece::Memory(lines));
        }
        self.pieces = merge_memory(result);
        if self.pieces.is_empty() {
            self.pieces.push(Piece::Memory(Vec::new()));
        }
        self.reindex();
    }

    pub fn bookmarks(&self) -> Vec<usize> {
        self.bookmarks.iter().copied().collect()
    }

    pub fn is_bookmarked(&self, line: usize) -> bool {
        self.bookmarks.contains(&line)
    }

    pub fn toggle_bookmark(&mut self, line: usize) {
        if !self.bookmarks.remove(&line) {
            self.bookmarks.insert(line);
        }
    }

    pub fn folds(&self) -> &BTreeMap<usize, usize> {
        &self.folds
    }

    pub fn set_fold(&mut self, start: usize, last: usize) {
        if last > start && start < self.total {
            Arc::make_mut(&mut self.folds).insert(start, last.min(self.total - 1));
        }
    }

    pub fn remove_fold(&mut self, start: usize) -> bool {
        self.folds.contains_key(&start) && Arc::make_mut(&mut self.folds).remove(&start).is_some()
    }

    pub fn clear_folds(&mut self) {
        if !self.folds.is_empty() {
            self.folds = Arc::default();
        }
    }

    pub fn tags(&self) -> &BTreeMap<usize, (usize, u8)> {
        &self.tags
    }

    /// The tagged stretch a line is in, as its first line, last line and colour.
    pub fn tag_at(&self, line: usize) -> Option<(usize, usize, u8)> {
        let (first, (last, colour)) = self.tags.range(..=line).next_back()?;
        (*last >= line).then_some((*first, *last, *colour))
    }

    /// Tags some lines, in place of any tags they overlapped.
    pub fn set_tag(&mut self, first: usize, last: usize, colour: u8) {
        if self.total == 0 || first > last {
            return;
        }
        let last = last.min(self.total - 1);
        self.remove_tags(first, last);
        Arc::make_mut(&mut self.tags).insert(first, (last, colour));
    }

    /// Takes away every tag that reaches into these lines. Says whether there were any.
    pub fn remove_tags(&mut self, first: usize, last: usize) -> bool {
        let before = self.tags.len();
        if before > 0 {
            Arc::make_mut(&mut self.tags).retain(|start, (end, _)| *end < first || *start > last);
        }
        self.tags.len() != before
    }

    pub fn set_tags(&mut self, tags: BTreeMap<usize, (usize, u8)>) {
        self.tags = Arc::new(tags);
    }

    /// A tag stays with its lines when others come and go, and grows or shrinks with what's changed inside it.
    /// One whose lines all go goes with them.
    fn move_tags(&mut self, start: usize, end: usize, new_len: usize) {
        if self.tags.is_empty() {
            return;
        }
        let place = |line: usize, last: bool| -> Option<usize> {
            if line < start {
                Some(line)
            } else if line >= end {
                Some(line - (end - start) + new_len)
            } else if last {
                (start + new_len).checked_sub(1)
            } else {
                Some(start)
            }
        };
        let moved = self
            .tags
            .iter()
            .filter_map(|(&first, &(last, colour))| {
                let (first, last) = (place(first, false)?, place(last, true)?);
                (first <= last).then_some((first, (last, colour)))
            })
            .collect();
        self.tags = Arc::new(moved);
    }

    /// A fold stays with its lines when others come and go. One whose hidden lines are edited opens again,
    /// because that edit should be seen.
    fn move_folds(&mut self, start: usize, end: usize, new_len: usize) {
        if self.folds.is_empty() {
            return;
        }
        let moved = self
            .folds
            .iter()
            .filter_map(|(&first, &last)| {
                if last < start {
                    Some((first, last))
                } else if first >= end {
                    Some((first - (end - start) + new_len, last - (end - start) + new_len))
                } else {
                    None
                }
            })
            .collect();
        self.folds = Arc::new(moved);
    }

    /// Whether a line has changed, and if so whether that's been saved.
    pub fn change_at(&self, line: usize) -> Option<bool> {
        self.changes.get(&line).copied()
    }

    pub fn changed_lines(&self) -> &BTreeMap<usize, bool> {
        &self.changes
    }

    /// Everything changed so far has now been saved.
    pub fn changes_saved(&mut self) {
        if !self.changes.is_empty() {
            Arc::make_mut(&mut self.changes).values_mut().for_each(|saved| *saved = true);
        }
    }

    /// Takes on the marks from before a file's lines were read afresh.
    pub fn set_changes(&mut self, changes: BTreeMap<usize, bool>) {
        self.changes = Arc::new(changes);
    }

    /// Keeps the marks with their lines when lines start..end become `new_len` lines, and marks the new ones.
    /// Deleted lines have nothing left to mark, so the line that takes their place shows the change.
    fn move_changes(&mut self, start: usize, end: usize, new_len: usize) {
        let mut moved: BTreeMap<usize, bool> = self
            .changes
            .iter()
            .filter_map(|(&line, &saved)| {
                if line < start {
                    Some((line, saved))
                } else if line >= end {
                    Some((line - (end - start) + new_len, saved))
                } else {
                    None
                }
            })
            .collect();
        for line in start..start + new_len {
            moved.insert(line, false);
        }
        if new_len == 0 && start < self.total - (end - start) {
            moved.insert(start, false);
        }
        self.changes = Arc::new(moved);
    }

    /// Keeps bookmarks on their lines when lines start..end become `new_len` lines.
    fn move_bookmarks(&mut self, start: usize, end: usize, new_len: usize) {
        if self.bookmarks.is_empty() {
            return;
        }
        self.bookmarks = self
            .bookmarks
            .iter()
            .filter_map(|&line| {
                if line < start {
                    Some(line)
                } else if line >= end {
                    Some(line - (end - start) + new_len)
                } else if line - start < new_len {
                    Some(line)
                } else {
                    None
                }
            })
            .collect();
    }

    /// Keeps line endings with their lines when lines start..end become `new_len` lines.
    fn move_terminators(&mut self, start: usize, end: usize, new_len: usize) {
        let kept = (end - start).min(new_len);
        // A line leaving the disk for memory holds on to the ending it had in the file.
        for i in start..start + kept {
            let (piece, _) = self.locate(i);
            if matches!(self.pieces[piece], Piece::Disk { .. }) {
                let eol = self.terminator(i);
                self.set_terminator(i, eol);
            }
        }
        // Adding after the last line gives that line an ending for the first time.
        if start == self.total && self.total > 0 {
            self.set_terminator(self.total - 1, self.insert_eol);
        }
        if !self.eols.is_empty() {
            let moved: BTreeMap<usize, Eol> = self
                .eols
                .iter()
                .filter_map(|(&line, &eol)| {
                    if line < start {
                        Some((line, eol))
                    } else if line >= end {
                        Some((line - (end - start) + new_len, eol))
                    } else if line - start < new_len {
                        Some((line, eol))
                    } else {
                        None
                    }
                })
                .collect();
            self.eols = Arc::new(moved);
        }
        // Lines that are really new use the insertion style.
        if self.insert_eol != self.base_eol {
            for i in start + kept..start + new_len {
                Arc::make_mut(&mut self.eols).insert(i, self.insert_eol);
            }
        }
    }

    fn reindex(&mut self) {
        self.starts.clear();
        let mut position = 0;
        for piece in &self.pieces {
            self.starts.push(position);
            position += piece.len();
        }
        self.total = position;
    }

    /// Writes the document out, copying untouched stretches byte for byte.
    /// Lists every edited character the encoding can't hold, before writing anything.
    pub fn check_encodable(&self, format: &TextFormat) -> Result<(), Problems> {
        let mut problems = Problems::default();
        let mut start = 0;
        for piece in &self.pieces {
            if let Piece::Memory(lines) = piece {
                let eol_of = |_: usize| Eol::None;
                if let Err(found) = codec::encode_with(lines, &eol_of, format, Lossy::Refuse) {
                    problems.total += found.total;
                    for mut problem in found.listed {
                        problem.line += start;
                        if problems.listed.len() < codec::MAX_LISTED_PROBLEMS {
                            problems.listed.push(problem);
                        }
                    }
                }
            }
            start += piece.len();
        }
        if problems.is_empty() { Ok(()) } else { Err(problems) }
    }

    pub fn write_file(&self, path: &Path, format: &TextFormat, progress: &AtomicU64) -> std::io::Result<()> {
        let mut temp_name = path.as_os_str().to_owned();
        temp_name.push(".limoniter-tmp");
        let temp = PathBuf::from(temp_name);
        let result = self.write_to(&temp, format, progress).and_then(|()| std::fs::rename(&temp, path));
        if result.is_err() {
            let _ = std::fs::remove_file(&temp);
        }
        result
    }

    fn write_to(&self, temp: &Path, format: &TextFormat, progress: &AtomicU64) -> std::io::Result<()> {
        let mut out = BufWriter::with_capacity(COPY_CHUNK, File::create(temp)?);
        if format.has_bom
            && let Some(mark) = format.charset.bom()
        {
            out.write_all(mark)?;
        }
        let mut index = 0;
        for piece in &self.pieces {
            let count = piece.len();
            if count == 0 {
                continue;
            }
            let last_here = index + count - 1;
            let ends_document = last_here + 1 == self.total;
            match piece {
                Piece::Disk { source, first, count } => {
                    let start = source.line_start(*first)?;
                    let mut end = source.line_start(first + count)?;
                    let reaches_end = first + count >= source.known_lines();
                    if !reaches_end && ends_document {
                        end -= eol_length_before(source, end)?;
                    }
                    copy_bytes(source, start, end, &mut out, progress)?;
                    // The file's last line has no ending of its own to copy.
                    if reaches_end && !ends_document {
                        out.write_all(&codec::eol_bytes(self.terminator(last_here), format.charset))?;
                    }
                }
                Piece::Memory(lines) => {
                    let encoded = codec::encode_with(lines, &|i| self.terminator(index + i), &TextFormat { has_bom: false, ..format.clone() }, Lossy::Omit).unwrap_or_default();
                    out.write_all(&encoded)?;
                    if !ends_document {
                        out.write_all(&codec::eol_bytes(self.terminator(last_here), format.charset))?;
                    }
                }
            }
            index += count;
            progress.fetch_add(0, Ordering::Relaxed);
        }
        out.flush()?;
        out.get_ref().sync_all()?;
        Ok(())
    }
}

fn slice_piece(piece: &Piece, _piece_start: usize, range: std::ops::Range<usize>) -> Option<Piece> {
    let end = range.end.min(piece.len());
    if range.start >= end {
        return None;
    }
    Some(match piece {
        Piece::Disk { source, first, .. } => Piece::Disk { source: source.clone(), first: first + range.start, count: end - range.start },
        Piece::Memory(lines) => Piece::Memory(lines[range.start..end].to_vec()),
    })
}

/// Joins neighbouring small memory pieces, so that many edits don't pile up many pieces.
fn merge_memory(pieces: Vec<Piece>) -> Vec<Piece> {
    let mut merged: Vec<Piece> = Vec::with_capacity(pieces.len());
    for piece in pieces {
        if piece.len() == 0 {
            continue;
        }
        if let (Some(Piece::Memory(previous)), Piece::Memory(lines)) = (merged.last_mut(), &piece)
            && previous.len() + lines.len() <= MERGE_LIMIT
        {
            previous.extend(lines.iter().cloned());
            continue;
        }
        merged.push(piece);
    }
    merged
}

fn eol_length_before(source: &Source, end: u64) -> std::io::Result<u64> {
    if end == 0 {
        return Ok(0);
    }
    let mut last = [0u8; 2];
    let start = end.saturating_sub(2);
    let want = (end - start) as usize;
    let got = crate::byte_store::read_at(&source.file, &mut last[..want], start)?;
    let bytes = &last[..got];
    Ok(if bytes.ends_with(b"\r\n") {
        2
    } else if bytes.ends_with(b"\n") || bytes.ends_with(b"\r") {
        1
    } else {
        0
    })
}

fn copy_bytes(source: &Source, start: u64, end: u64, out: &mut impl Write, progress: &AtomicU64) -> std::io::Result<()> {
    let mut buffer = vec![0u8; COPY_CHUNK];
    let mut position = start;
    while position < end {
        let want = ((end - position) as usize).min(COPY_CHUNK);
        let got = crate::byte_store::read_at(&source.file, &mut buffer[..want], position)?;
        if got == 0 {
            return Err(std::io::Error::new(std::io::ErrorKind::UnexpectedEof, "the file got shorter while it was being saved"));
        }
        out.write_all(&buffer[..got])?;
        position += got as u64;
        progress.fetch_add(got as u64, Ordering::Relaxed);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn temp_dir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("limoniter-store-{tag}-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn open(path: &Path) -> Arc<Source> {
        let source = Source::open(path).unwrap();
        while !source.is_finished() {
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
        source
    }

    /// The moment that matters is when the scan has noted where a block ends but not yet counted its lines,
    /// which it only does at the end of each chunk it reads. Lines read then must not be kept as blanks.
    #[test]
    fn lines_read_just_as_the_scan_passes_them_arent_kept_blank() {
        let dir = temp_dir("scan-race");
        let path = dir.join("lines.txt");
        let text: String = (0..1000).map(|n| format!("line {n}\n")).collect();
        fs::write(&path, &text).unwrap();
        let source = open(&path);
        for counted in [0, 100, 300, 600] {
            source.total_lines.store(0, Ordering::SeqCst);
            source.complete_lines.store(counted, Ordering::SeqCst);
            {
                let mut cache = source.cache.lock().unwrap();
                cache.blocks.clear();
                cache.order.clear();
            }
            for line in [0, 1, 255, 256, 511, 767] {
                assert_eq!(&*source.line(line), format!("line {line}"), "with {counted} lines counted");
            }
            // Where a line starts is only asked of lines already counted.
            if counted > 255 {
                assert_eq!(source.line_start(255).unwrap(), text.find("line 255\n").unwrap() as u64);
            }
        }
        let _ = fs::remove_dir_all(&dir);
    }

    fn all(store: &LineStore) -> Vec<String> {
        store.range_vec(0, store.len()).iter().map(|l| l.to_string()).collect()
    }

    fn arcs(lines: &[&str]) -> Vec<Arc<str>> {
        lines.iter().map(|l| Arc::from(*l)).collect()
    }

    #[test]
    fn finds_lines_like_the_in_memory_reader() {
        let dir = temp_dir("lines");
        for data in [&b"a\r\nb\nc\rd"[..], b"a\n", b"\n\n", b"", b"only", b"x\r", b"x\r\n"] {
            let path = dir.join("f.txt");
            fs::write(&path, data).unwrap();
            let store = LineStore::from_source(open(&path));
            let mut format = TextFormat::default();
            let expected = crate::codec::decode(data, &mut format);
            assert_eq!(all(&store), expected.lines, "for {data:?}");
        }
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn reads_across_block_and_chunk_edges() {
        let dir = temp_dir("edges");
        let path = dir.join("f.txt");
        let lines: Vec<String> = (0..2000).map(|i| format!("line {i}")).collect();
        fs::write(&path, lines.join("\r\n")).unwrap();
        let source = open(&path);
        let store = LineStore::from_source(source.clone());
        assert_eq!(store.len(), 2000);
        for i in [0, 255, 256, 257, 511, 512, 1999] {
            assert_eq!(&*store.get(i), lines[i]);
        }
        assert_eq!(store.terminator(5), Eol::CrLf);
        assert_eq!(source.eol_counts().crlf, 1999);
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_crlf_split_across_scan_chunks_counts_once() {
        let dir = temp_dir("split");
        let path = dir.join("f.txt");
        let mut data = vec![b'x'; SCAN_CHUNK - 1];
        data.extend_from_slice(b"\r\nsecond");
        fs::write(&path, &data).unwrap();
        let store = LineStore::from_source(open(&path));
        assert_eq!(store.len(), 2);
        assert_eq!(&*store.get(1), "second");
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn bytes_that_are_not_utf8_are_only_read_differently_and_survive_saving() {
        let dir = temp_dir("latin");
        let path = dir.join("f.txt");
        let data = b"caf\xE9\x81\nsecond\n".to_vec();
        fs::write(&path, &data).unwrap();
        let source = open(&path);
        let format = source.format();
        assert_eq!(format.charset.id, "windows-1252");
        assert_eq!(format.certainty, Certainty::Suggested);
        let mut store = LineStore::from_source(source);
        assert_eq!(&*store.get(0), format!("caf\u{e9}{}", charset::raw_char(0x81)));
        store.set(1, Arc::from("deux\u{e9}"));
        let out = dir.join("out.txt");
        store.write_file(&out, &format, &AtomicU64::new(0)).unwrap();
        assert_eq!(fs::read(&out).unwrap(), b"caf\xE9\x81\ndeux\xE9\n");
        store.set(1, Arc::from("\u{4E2D}"));
        let problems = store.check_encodable(&format).unwrap_err();
        assert_eq!((problems.total, problems.listed[0].line, problems.listed[0].ch), (1, 1, '\u{4E2D}'));
        fs::remove_dir_all(&dir).unwrap();
    }

    /// Applies the same random edits to the store and to a plain list, then compares them and the saved file.
    #[test]
    fn edits_match_a_plain_list_and_save_exactly() {
        let dir = temp_dir("model");
        for (eol, last_eol) in [("\r\n", true), ("\r\n", false), ("\n", true), ("\n", false)] {
            let path = dir.join("source.txt");
            let mut model: Vec<String> = (0..1500).map(|i| format!("original line {i}")).collect();
            if last_eol {
                model.push(String::new());
            }
            fs::write(&path, model.join(eol)).unwrap();
            let source = open(&path);
            let format = source.format();
            let mut store = LineStore::from_source(source);
            assert_eq!(all(&store), model);

            let mut seed = 12345u64;
            let mut next = move |limit: usize| {
                seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
                ((seed >> 33) as usize) % limit.max(1)
            };
            for step in 0..400 {
                let len = model.len();
                match next(5) {
                    0 if len > 0 => {
                        let i = next(len);
                        let text = format!("edited {step}");
                        store.set(i, Arc::from(text.as_str()));
                        model[i] = text;
                    }
                    1 => {
                        let i = next(len + 1);
                        let text = format!("inserted {step}");
                        store.insert(i, Arc::from(text.as_str()));
                        model.insert(i, text);
                    }
                    2 if len > 1 => {
                        let i = next(len);
                        let j = (i + 1 + next(40)).min(len);
                        store.remove_range(i, j);
                        model.drain(i..j);
                    }
                    3 => {
                        let i = next(len + 1);
                        let new = [format!("many {step} a"), format!("many {step} b")];
                        store.insert_many(i, arcs(&[&new[0], &new[1]]));
                        model.splice(i..i, new);
                    }
                    _ => {
                        store.push(Arc::from("pushed"));
                        model.push("pushed".to_string());
                    }
                }
                assert_eq!(store.len(), model.len(), "after step {step}");
            }
            assert_eq!(all(&store), model);

            let out = dir.join("out.txt");
            store.write_file(&out, &format, &AtomicU64::new(0)).unwrap();
            assert_eq!(fs::read_to_string(&out).unwrap(), model.join(eol), "eol {eol:?}, ends with eol {last_eol}");
        }
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn untouched_mixed_line_endings_and_bom_are_kept() {
        let dir = temp_dir("mixed");
        let path = dir.join("f.txt");
        let data = b"\xEF\xBB\xBFa\r\nb\nc\rd\n".to_vec();
        fs::write(&path, &data).unwrap();
        let source = open(&path);
        let format = source.format();
        assert!(format.has_bom);
        let mut store = LineStore::from_source(source);
        assert_eq!(&*store.get(0), "a");
        let out = dir.join("out.txt");
        store.write_file(&out, &format, &AtomicU64::new(0)).unwrap();
        assert_eq!(fs::read(&out).unwrap(), data);
        store.set(0, Arc::from("A"));
        store.write_file(&out, &format, &AtomicU64::new(0)).unwrap();
        assert_eq!(fs::read(&out).unwrap(), b"\xEF\xBB\xBFA\r\nb\nc\rd\n", "the edited line keeps its own ending");
        fs::remove_dir_all(&dir).unwrap();
    }

    fn mixed_store() -> LineStore {
        LineStore::from_decoded(arcs(&["a", "b", "c", "d"]), Eol::CrLf, BTreeMap::from([(1, Eol::Lf), (2, Eol::Cr)]))
    }

    fn encoded(store: &LineStore) -> Vec<u8> {
        store.encode(&TextFormat::default(), Lossy::Refuse).unwrap()
    }

    #[test]
    fn each_line_keeps_its_own_ending_through_edits_in_memory() {
        let mut store = mixed_store();
        assert_eq!(encoded(&store), b"a\r\nb\nc\rd");
        store.set(1, Arc::from("B"));
        assert_eq!(encoded(&store), b"a\r\nB\nc\rd");
        store.insert(1, Arc::from("new"));
        assert_eq!(encoded(&store), b"a\r\nnew\r\nB\nc\rd", "a new line uses the insertion style and the rest stay put");
        store.remove(0);
        assert_eq!(encoded(&store), b"new\r\nB\nc\rd");
        let counts = store.eol_counts();
        assert_eq!((counts.crlf, counts.lf, counts.cr), (1, 1, 1));
    }

    #[test]
    fn new_lines_use_the_chosen_style_and_old_ones_are_not_rewritten() {
        let mut store = mixed_store();
        store.set_insert_eol(Eol::Lf);
        store.push(Arc::from("e"));
        assert_eq!(encoded(&store), b"a\r\nb\nc\rd\ne");
        store.insert(0, Arc::from("z"));
        assert_eq!(encoded(&store), b"z\na\r\nb\nc\rd\ne");
    }

    #[test]
    fn splitting_and_joining_keep_the_ending_at_the_far_end() {
        let mut store = mixed_store();
        store.set(2, Arc::from("c1"));
        store.insert(3, Arc::from("c2"));
        store.split_terminator(2, 1);
        assert_eq!(encoded(&store), b"a\r\nb\nc1\r\nc2\rd");
        store.set(1, Arc::from("bc1"));
        store.take_terminator_from(1, 2);
        store.remove(2);
        assert_eq!(encoded(&store), b"a\r\nbc1\r\nc2\rd");
    }

    #[test]
    fn converting_all_endings_is_complete_and_a_clone_can_undo_it() {
        let mut store = mixed_store();
        let before = store.clone();
        store.convert_all_eols(Eol::Lf);
        assert_eq!(encoded(&store), b"a\nb\nc\nd");
        assert_eq!(encoded(&before), b"a\r\nb\nc\rd");
    }

    #[test]
    fn a_huge_file_edit_keeps_mixed_endings_on_both_sides() {
        let dir = temp_dir("disk-mixed");
        let path = dir.join("f.txt");
        fs::write(&path, b"a\r\nb\nc\rd\n").unwrap();
        let source = open(&path);
        let format = source.format();
        let mut store = LineStore::from_source(source);
        store.set(1, Arc::from("B"));
        store.set(2, Arc::from("C"));
        store.push(Arc::from("end"));
        let out = dir.join("out.txt");
        store.write_file(&out, &format, &AtomicU64::new(0)).unwrap();
        assert_eq!(fs::read(&out).unwrap(), b"a\r\nB\nC\rd\n\nend", "the old endings stay and the new line uses the file's commonest");
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn deleting_the_end_leaves_no_stray_line_ending() {
        let dir = temp_dir("end");
        let path = dir.join("f.txt");
        fs::write(&path, b"a\r\nb\r\nc").unwrap();
        let source = open(&path);
        let format = source.format();
        let mut store = LineStore::from_source(source);
        store.remove(2);
        let out = dir.join("out.txt");
        store.write_file(&out, &format, &AtomicU64::new(0)).unwrap();
        assert_eq!(fs::read(&out).unwrap(), b"a\r\nb");
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn saving_over_the_open_file_keeps_undo_working() {
        let dir = temp_dir("over");
        let path = dir.join("f.txt");
        fs::write(&path, b"one\ntwo\nthree").unwrap();
        let source = open(&path);
        let format = source.format();
        let mut store = LineStore::from_source(source.clone());
        let before = store.clone();
        store.set(1, Arc::from("TWO"));
        store.write_file(&path, &format, &AtomicU64::new(0)).unwrap();
        assert_eq!(fs::read(&path).unwrap(), b"one\nTWO\nthree");
        assert!(source.changed_on_disk());
        assert_eq!(all(&before), ["one", "two", "three"], "the snapshot can still read what it had");
        assert_eq!(all(&store), ["one", "TWO", "three"]);
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn an_edit_on_a_huge_range_does_not_read_it() {
        let dir = temp_dir("lazy");
        let path = dir.join("f.txt");
        let lines: Vec<String> = (0..5000).map(|i| i.to_string()).collect();
        fs::write(&path, lines.join("\n")).unwrap();
        let mut store = LineStore::from_source(open(&path));
        store.set(2500, Arc::from("x"));
        assert_eq!(store.memory_lines(), 1);
        assert!(store.is_disk_backed());
        assert_eq!(store.pieces.len(), 3);
        store.remove_range(1, 4999);
        assert_eq!(all(&store), ["0", "4999"]);
        assert_eq!(store.memory_lines(), 0);
        fs::remove_dir_all(&dir).unwrap();
    }
}
