//! The bytes hex mode edits, as a piece table: the file's own bytes stay where they are, read a block at a time
//! when they're looked at, and everything typed or pasted goes into a buffer of added bytes. The document is a
//! list of pieces, each a stretch of one or the other, so an edit anywhere in a file of any size only changes the
//! list, and undoing it puts the old pieces back. A small file is read into memory and edited the same way.

use std::collections::{HashMap, VecDeque};
use std::fs::File;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

/// How much of the file is read at a time, and how many such blocks are kept.
const BLOCK: u64 = 64 * 1024;
const CACHED_BLOCKS: usize = 64;

/// Reads from a place in a file without moving its cursor, on any system.
pub fn read_at(file: &File, buf: &mut [u8], offset: u64) -> std::io::Result<usize> {
    #[cfg(windows)]
    {
        std::os::windows::fs::FileExt::seek_read(file, buf, offset)
    }
    #[cfg(unix)]
    {
        std::os::unix::fs::FileExt::read_at(file, buf, offset)
    }
}

/// Fills `buf` from a place in a file, as far as the file goes; gives how much was read.
pub fn read_full_at(file: &File, buf: &mut [u8], offset: u64) -> std::io::Result<usize> {
    let mut done = 0;
    while done < buf.len() {
        match read_at(file, &mut buf[done..], offset + done as u64)? {
            0 => break,
            n => done += n,
        }
    }
    Ok(done)
}

/// The blocks read so far, and the order they were read in, so the oldest goes first.
type BlockCache = (HashMap<u64, Arc<Vec<u8>>>, VecDeque<u64>);

/// A file being edited in place: its length and modified time when opened, and the blocks read so far.
pub struct FileBytes {
    file: File,
    pub path: PathBuf,
    pub len: u64,
    pub modified: Option<std::time::SystemTime>,
    cache: Mutex<BlockCache>,
}

impl FileBytes {
    pub fn open(path: &Path) -> std::io::Result<Arc<Self>> {
        let file = File::open(path)?;
        let meta = file.metadata()?;
        Ok(Arc::new(Self { file, path: path.to_path_buf(), len: meta.len(), modified: meta.modified().ok(), cache: Mutex::new((HashMap::new(), VecDeque::new())) }))
    }

    fn block(&self, index: u64) -> Arc<Vec<u8>> {
        let mut cache = self.cache.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        if let Some(block) = cache.0.get(&index) {
            return block.clone();
        }
        let size = (self.len - index * BLOCK).min(BLOCK) as usize;
        let mut data = vec![0u8; size];
        // A file that shrank under us reads as zeros past its new end, rather than failing every draw.
        let _ = read_full_at(&self.file, &mut data, index * BLOCK);
        let block = Arc::new(data);
        cache.0.insert(index, block.clone());
        cache.1.push_back(index);
        if cache.1.len() > CACHED_BLOCKS
            && let Some(old) = cache.1.pop_front()
        {
            cache.0.remove(&old);
        }
        block
    }

    /// Bytes from the file, through the cache of blocks.
    fn read_into(&self, mut offset: u64, out: &mut Vec<u8>, mut count: usize) {
        // A long read, as saving or searching makes, goes straight to the file rather than through the blocks.
        if count >= 4 * BLOCK as usize {
            let count = count.min(self.len.saturating_sub(offset) as usize);
            let start = out.len();
            out.resize(start + count, 0);
            let got = read_full_at(&self.file, &mut out[start..], offset).unwrap_or(0);
            // Past the end of a file that shrank under us reads as zeros, as the blocks do.
            out[start + got..].fill(0);
            return;
        }
        while count > 0 && offset < self.len {
            let block = self.block(offset / BLOCK);
            let within = (offset % BLOCK) as usize;
            let take = (block.len() - within).min(count);
            out.extend_from_slice(&block[within..within + take]);
            offset += take as u64;
            count -= take;
        }
    }
}

/// A stretch of the document: some of the file's bytes, or some of the added ones.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Piece {
    pub added: bool,
    pub offset: u64,
    pub len: usize,
}

#[derive(Default, Clone)]
pub struct ByteStore {
    source: Option<Arc<FileBytes>>,
    added: Vec<u8>,
    pieces: Vec<Piece>,
    /// Where each piece starts in the document.
    starts: Vec<usize>,
    len: usize,
}

impl ByteStore {
    pub fn from_vec(bytes: Vec<u8>) -> Self {
        let len = bytes.len();
        let mut store = Self { source: None, added: bytes, pieces: Vec::new(), starts: Vec::new(), len: 0 };
        if len > 0 {
            store.pieces.push(Piece { added: true, offset: 0, len });
        }
        store.reindex();
        store
    }

    /// A file of any size, read only as it's looked at.
    pub fn from_file(source: Arc<FileBytes>) -> Self {
        let len = source.len as usize;
        let mut store = Self { source: Some(source), added: Vec::new(), pieces: Vec::new(), starts: Vec::new(), len: 0 };
        if len > 0 {
            store.pieces.push(Piece { added: false, offset: 0, len });
        }
        store.reindex();
        store
    }

    /// The file this reads from, if it isn't all in memory.
    pub fn source(&self) -> Option<&Arc<FileBytes>> {
        self.source.as_ref()
    }

    pub fn is_on_disk(&self) -> bool {
        self.source.is_some()
    }

    fn reindex(&mut self) {
        self.starts.clear();
        let mut at = 0;
        for piece in &self.pieces {
            self.starts.push(at);
            at += piece.len;
        }
        self.len = at;
    }

    pub fn len(&self) -> usize {
        self.len
    }

    /// The piece a document offset is in, and how far into it.
    fn locate(&self, offset: usize) -> (usize, usize) {
        let index = self.starts.partition_point(|&start| start <= offset).saturating_sub(1);
        (index, offset - self.starts.get(index).copied().unwrap_or(0))
    }

    pub fn byte(&self, offset: usize) -> u8 {
        let (index, within) = self.locate(offset);
        let piece = self.pieces[index];
        if piece.added {
            self.added[piece.offset as usize + within]
        } else {
            let mut out = Vec::with_capacity(1);
            if let Some(source) = &self.source {
                source.read_into(piece.offset + within as u64, &mut out, 1);
            }
            out.first().copied().unwrap_or(0)
        }
    }

    /// Bytes start..start+count, or as many as there are.
    pub fn read(&self, start: usize, count: usize) -> Vec<u8> {
        let end = (start + count).min(self.len);
        let mut out = Vec::with_capacity(end.saturating_sub(start));
        if start >= end {
            return out;
        }
        let (mut index, mut within) = self.locate(start);
        while out.len() < end - start && index < self.pieces.len() {
            let piece = self.pieces[index];
            let take = (piece.len - within).min(end - start - out.len());
            if piece.added {
                let from = piece.offset as usize + within;
                out.extend_from_slice(&self.added[from..from + take]);
            } else if let Some(source) = &self.source {
                source.read_into(piece.offset + within as u64, &mut out, take);
            }
            index += 1;
            within = 0;
        }
        out
    }

    pub fn to_vec(&self) -> Vec<u8> {
        self.read(0, self.len)
    }

    /// The pieces covering start..start+count, cut to fit.
    pub fn pieces_of(&self, start: usize, count: usize) -> Vec<Piece> {
        let mut out = Vec::new();
        if count == 0 || start >= self.len {
            return out;
        }
        let (mut index, mut within) = self.locate(start);
        let mut left = count.min(self.len - start);
        while left > 0 && index < self.pieces.len() {
            let piece = self.pieces[index];
            let take = (piece.len - within).min(left);
            out.push(Piece { added: piece.added, offset: piece.offset + within as u64, len: take });
            left -= take;
            index += 1;
            within = 0;
        }
        out
    }

    /// Adds bytes to the buffer of added bytes, as a piece.
    pub fn add(&mut self, bytes: &[u8]) -> Vec<Piece> {
        if bytes.is_empty() {
            return Vec::new();
        }
        let offset = self.added.len() as u64;
        self.added.extend_from_slice(bytes);
        vec![Piece { added: true, offset, len: bytes.len() }]
    }

    /// Cuts the piece list so that a piece starts at `offset`; gives that piece's index.
    fn split(&mut self, offset: usize) -> usize {
        if offset >= self.len {
            return self.pieces.len();
        }
        let (index, within) = self.locate(offset);
        if within == 0 {
            return index;
        }
        let piece = self.pieces[index];
        self.pieces[index].len = within;
        self.pieces.insert(index + 1, Piece { added: piece.added, offset: piece.offset + within as u64, len: piece.len - within });
        self.starts.insert(index + 1, offset);
        index + 1
    }

    /// Replaces `remove` bytes at `position` with these pieces; gives the pieces that were there, to undo it.
    pub fn replace(&mut self, position: usize, remove: usize, with: &[Piece]) -> Vec<Piece> {
        let position = position.min(self.len);
        let remove = remove.min(self.len - position);
        let removed = self.pieces_of(position, remove);
        let first = self.split(position);
        let end = self.split(position + remove);
        let with: Vec<Piece> = with.iter().copied().filter(|p| p.len > 0).collect();
        let count = with.len();
        self.pieces.splice(first..end, with);
        // Pieces that carry straight on from each other are joined, so typing doesn't splinter the list.
        let mut i = first.saturating_sub(1);
        let stop = (first + count + 1).min(self.pieces.len());
        let mut stop = stop;
        while i + 1 < stop {
            let (a, b) = (self.pieces[i], self.pieces[i + 1]);
            if a.added == b.added && a.offset + a.len as u64 == b.offset {
                self.pieces[i].len += b.len;
                self.pieces.remove(i + 1);
                stop -= 1;
            } else {
                i += 1;
            }
        }
        self.reindex();
        removed
    }

    /// The bytes some pieces stand for.
    pub fn bytes_of(&self, pieces: &[Piece]) -> Vec<u8> {
        let mut out = Vec::with_capacity(pieces.iter().map(|p| p.len).sum());
        for piece in pieces {
            if piece.added {
                out.extend_from_slice(&self.added[piece.offset as usize..piece.offset as usize + piece.len]);
            } else if let Some(source) = &self.source {
                source.read_into(piece.offset, &mut out, piece.len);
            }
        }
        out
    }

    /// What a recovery copy needs to put this back: the pieces, with only the added bytes they still use.
    pub fn image(&self) -> (Vec<Piece>, Vec<u8>) {
        let mut added = Vec::new();
        let pieces = self
            .pieces
            .iter()
            .map(|piece| {
                if !piece.added {
                    return *piece;
                }
                let offset = added.len() as u64;
                added.extend_from_slice(&self.added[piece.offset as usize..piece.offset as usize + piece.len]);
                Piece { added: true, offset, len: piece.len }
            })
            .collect();
        (pieces, added)
    }

    /// Puts back what `image` gave, over the same file. None if the pieces don't fit the file or the bytes.
    pub fn from_image(source: Arc<FileBytes>, pieces: Vec<Piece>, added: Vec<u8>) -> Option<Self> {
        let fits = pieces.iter().all(|p| if p.added { p.offset as usize + p.len <= added.len() } else { p.offset + p.len as u64 <= source.len });
        if !fits {
            return None;
        }
        let mut store = Self { source: Some(source), added, pieces, starts: Vec::new(), len: 0 };
        store.reindex();
        Some(store)
    }

    #[cfg(test)]
    pub fn piece_count(&self) -> usize {
        self.pieces.len()
    }

    /// Writes the document out, a piece at a time.
    #[cfg(test)]
    pub fn write_to(&self, out: &mut dyn Write) -> std::io::Result<()> {
        self.write_counting(out, &std::sync::atomic::AtomicU64::new(0))
    }

    /// Writes the document out, keeping count of the bytes written so far.
    pub fn write_counting(&self, out: &mut dyn Write, written: &std::sync::atomic::AtomicU64) -> std::io::Result<()> {
        const CHUNK: usize = 4 * 1024 * 1024;
        let mut at = 0;
        while at < self.len {
            let chunk = self.read(at, CHUNK);
            if chunk.is_empty() {
                return Err(std::io::Error::other("The file being edited could no longer be read."));
            }
            out.write_all(&chunk)?;
            at += chunk.len();
            written.store(at as u64, std::sync::atomic::Ordering::Relaxed);
        }
        Ok(())
    }

    /// Saves to a file through a temporary one beside it, so the original stays whole until the copy is complete.
    pub fn write_file(&self, path: &Path, written: &std::sync::atomic::AtomicU64) -> std::io::Result<()> {
        let mut temp_name = path.as_os_str().to_owned();
        temp_name.push(".limoniter-tmp");
        let temp = PathBuf::from(temp_name);
        let result = (|| {
            let mut out = std::io::BufWriter::with_capacity(1 << 20, File::create(&temp)?);
            self.write_counting(&mut out, written)?;
            out.into_inner().map_err(|e| e.into_error())?.sync_all()?;
            std::fs::rename(&temp, path)
        })();
        if result.is_err() {
            let _ = std::fs::remove_file(&temp);
        }
        result
    }

    /// Finds `pattern` starting at a position in lo..=hi: the first such going forward, or the last going backward.
    /// Read a stretch at a time, so it can be given any part of a file of any size.
    pub fn find_range(&self, pattern: &[u8], lo: usize, hi: usize, backward: bool, compare: Compare) -> Option<usize> {
        const STRETCH: usize = 8 * 1024 * 1024;
        if pattern.is_empty() || pattern.len() > self.len {
            return None;
        }
        let hi = hi.min(self.len - pattern.len());
        if lo > hi {
            return None;
        }
        if backward {
            let mut end = hi + 1;
            while end > lo {
                let start = end.saturating_sub(STRETCH).max(lo);
                let data = self.read(start, end - start + pattern.len() - 1);
                if let Some(p) = compare.last(&data, pattern, end - start) {
                    return Some(start + p);
                }
                end = start;
            }
        } else {
            let mut start = lo;
            while start <= hi {
                let count = (hi + 1 - start).min(STRETCH);
                let data = self.read(start, count + pattern.len() - 1);
                if let Some(p) = compare.first(&data, pattern, 0, count) {
                    return Some(start + p);
                }
                start += count;
            }
        }
        None
    }

    /// Finds `pattern` from `from` on (or back from it), wrapping round the end.
    #[cfg(test)]
    pub fn find(&self, pattern: &[u8], from: usize, backward: bool, compare: Compare) -> Option<usize> {
        if pattern.is_empty() || pattern.len() > self.len {
            return None;
        }
        let last = self.len - pattern.len();
        let from = from.min(last);
        if backward {
            self.find_range(pattern, 0, from, true, compare).or_else(|| self.find_range(pattern, from + 1, last, true, compare))
        } else {
            self.find_range(pattern, from, last, false, compare).or_else(|| (from > 0).then(|| self.find_range(pattern, 0, from - 1, false, compare)).flatten())
        }
    }

    /// Goes on with replacing every match, over at most `budget` more bytes. True when it has reached the end.
    pub fn replace_step(&self, job: &mut ReplaceAll, budget: usize) -> bool {
        let n = job.pattern.len();
        if n == 0 || job.at + n > self.len {
            job.pieces.extend(self.pieces_of(job.kept_from, self.len - job.kept_from));
            return true;
        }
        let take = budget.min(self.len + 1 - n - job.at);
        let data = self.read(job.at, take + n - 1);
        let mut p = 0;
        while let Some(q) = job.compare.first(&data, &job.pattern, p, take) {
            let at = job.at + q;
            job.pieces.extend(self.pieces_of(job.kept_from, at - job.kept_from));
            job.pieces.extend(job.replacement.iter().copied());
            job.count += 1;
            job.kept_from = at + n;
            p = q + n;
        }
        // A match may have run past the stretch, so the next starts after it.
        job.at = (job.at + take).max(job.kept_from);
        false
    }

    /// Starts replacing every match of `pattern` with `with`.
    pub fn begin_replace(&mut self, pattern: &[u8], with: &[u8], compare: Compare) -> ReplaceAll {
        ReplaceAll { pattern: pattern.to_vec(), replacement: self.add(with), compare, pieces: Vec::new(), kept_from: 0, at: 0, count: 0 }
    }
}

/// How a search compares bytes: exactly, ignoring the case of ASCII letters, or of EBCDIC ones.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Compare {
    Exact,
    AsciiCase,
    EbcdicCase,
}

impl Compare {
    pub fn same(self, a: &[u8], b: &[u8]) -> bool {
        match self {
            Self::Exact => a == b,
            Self::AsciiCase => a.eq_ignore_ascii_case(b),
            Self::EbcdicCase => a.len() == b.len() && a.iter().zip(b).all(|(x, y)| crate::ebcdic::same_ignoring_case(*x, *y)),
        }
    }

    /// The first place from `from` up to (not including) `limit` where the pattern starts in `data`.
    fn first(self, data: &[u8], pattern: &[u8], from: usize, limit: usize) -> Option<usize> {
        let n = pattern.len();
        let fits = |p: usize| p < limit && p + n <= data.len();
        match self {
            Self::Exact => {
                let end = (limit + n - 1).min(data.len());
                memchr::memmem::find(data.get(from..end)?, pattern).map(|p| p + from).filter(|&p| fits(p))
            }
            Self::AsciiCase => {
                let (a, b) = (pattern[0].to_ascii_lowercase(), pattern[0].to_ascii_uppercase());
                let mut p = from;
                while p < limit.min(data.len()) {
                    let q = p + memchr::memchr2(a, b, &data[p..limit.min(data.len())])?;
                    if fits(q) && self.same(&data[q..q + n], pattern) {
                        return Some(q);
                    }
                    p = q + 1;
                }
                None
            }
            Self::EbcdicCase => (from..limit).find(|&p| fits(p) && self.same(&data[p..p + n], pattern)),
        }
    }

    /// The last place before `limit` where the pattern starts in `data`.
    fn last(self, data: &[u8], pattern: &[u8], limit: usize) -> Option<usize> {
        let n = pattern.len();
        let end = (limit + n - 1).min(data.len());
        match self {
            Self::Exact => memchr::memmem::rfind(&data[..end], pattern),
            Self::AsciiCase => {
                let (a, b) = (pattern[0].to_ascii_lowercase(), pattern[0].to_ascii_uppercase());
                let mut stop = limit.min(data.len());
                while let Some(q) = memchr::memrchr2(a, b, &data[..stop]) {
                    if q + n <= data.len() && self.same(&data[q..q + n], pattern) {
                        return Some(q);
                    }
                    stop = q;
                }
                None
            }
            Self::EbcdicCase => (0..limit).rev().find(|&p| p + n <= data.len() && self.same(&data[p..p + n], pattern)),
        }
    }
}

/// A Replace All under way: the pieces worked out so far, and where it has got to.
pub struct ReplaceAll {
    pattern: Vec<u8>,
    replacement: Vec<Piece>,
    compare: Compare,
    pub pieces: Vec<Piece>,
    kept_from: usize,
    pub at: usize,
    pub count: usize,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn edits_split_and_join_pieces() {
        let mut store = ByteStore::from_vec(b"hello world".to_vec());
        let with = store.add(b"HELLO");
        let removed = store.replace(0, 5, &with);
        assert_eq!(store.to_vec(), b"HELLO world");
        store.replace(0, 5, &removed);
        assert_eq!(store.to_vec(), b"hello world", "undone with the old pieces");
        // Typing at the end of what was just added joins on to it.
        let a = store.add(b"!");
        store.replace(11, 0, &a);
        let b = store.add(b"?");
        store.replace(12, 0, &b);
        assert_eq!(store.to_vec(), b"hello world!?");
        assert!(store.piece_count() <= 2, "{}", store.piece_count());
        assert_eq!(store.read(4, 3), b"o w");
        assert_eq!(store.byte(12), b'?');
    }

    #[test]
    fn a_file_is_read_where_it_lies_and_written_out_with_its_edits() {
        let dir = std::env::temp_dir().join(format!("limoniter-bytes-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("big.bin");
        let original: Vec<u8> = (0..300_000u32).map(|i| (i % 251) as u8).collect();
        std::fs::write(&path, &original).unwrap();
        let mut store = ByteStore::from_file(FileBytes::open(&path).unwrap());
        assert!(store.is_on_disk());
        assert_eq!(store.byte(200_000), original[200_000]);
        assert_eq!(store.read(65_530, 20), &original[65_530..65_550], "across a block boundary");
        let x = store.add(b"\xFF\xFE\xFD");
        store.replace(100_000, 1000, &x);
        assert_eq!(store.len(), 300_000 - 997);
        let mut out = Vec::new();
        store.write_to(&mut out).unwrap();
        let mut expected = original[..100_000].to_vec();
        expected.extend(b"\xFF\xFE\xFD");
        expected.extend(&original[101_000..]);
        assert_eq!(out, expected);
        let exact = Compare::Exact;
        assert_eq!(store.find(b"\xFF\xFE\xFD", 0, false, exact), Some(100_000));
        assert_eq!(store.find(b"\xFF\xFE\xFD", 100_001, false, exact), Some(100_000), "wrapping round");
        let back = store.find(&original[250..253], 290_000, true, exact).unwrap();
        assert!(back <= 290_000 && store.read(back, 3) == original[250..253], "{back}");
        assert!(store.find(&original[250..253], back - 1, true, exact).unwrap() < back);
        let mut job = store.begin_replace(b"\xFF\xFE\xFD", b"", exact);
        while !store.replace_step(&mut job, 7000) {}
        let (pieces, count) = (job.pieces, job.count);
        assert_eq!(count, 1);
        let all = store.len();
        store.replace(0, all, &pieces);
        assert_eq!(store.len(), 300_000 - 1000);
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
