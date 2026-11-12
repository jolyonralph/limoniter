//! Keeps a file's undo history after it's closed, in a file of its own beside the settings.
//!
//! Each state is written as what's different from the one it came from, so that a history costs about what its edits
//! do. Anything unexpected on reading, from a damaged file to one from another version, means no history and nothing else.

use crate::codec::Eol;
use crate::recovery::{eol_code, eol_from};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

const MAGIC: &[u8; 8] = b"ECUNDO1\n";
/// Longer than this and a history isn't kept. It isn't worth slowing every save for.
pub const MOST_FILE_BYTES: usize = 64 * 1024 * 1024;
/// The same for how long a document can be.
pub const MOST_LINES: usize = 50_000;
/// How many files' histories are kept. The least recently saved go first.
const MOST_HISTORIES: usize = 300;
const MOST_TOTAL_BYTES: u64 = 1024 * 1024 * 1024;
const CHECKSUM_BYTES: usize = 32;

pub struct NodeImage {
    pub parent: Option<usize>,
    pub when: u64,
    pub label: String,
    pub caret: (usize, usize),
    pub anchor: (usize, usize),
    pub lines: Vec<Arc<str>>,
    pub base: Eol,
    pub insert: Eol,
    pub eols: BTreeMap<usize, Eol>,
}

pub struct Image {
    /// What the file held when this was written. A file changed since then has moved on from this history.
    pub file_hash: [u8; 32],
    pub current: usize,
    pub nodes: Vec<NodeImage>,
}

pub fn hash_of(bytes: &[u8]) -> [u8; 32] {
    Sha256::digest(bytes).into()
}

/// Which file holds a document's history. Going by its path, with capitals not counting, as on Windows.
pub fn file_for(root: &Path, path: &Path) -> PathBuf {
    let name = path.to_string_lossy().to_lowercase();
    let hash = hash_of(name.as_bytes());
    let text: String = hash[..16].iter().map(|byte| format!("{byte:02x}")).collect();
    root.join(format!("{text}.undo"))
}

fn put_number(out: &mut Vec<u8>, number: u64) {
    out.extend_from_slice(&number.to_le_bytes());
}

fn put_text(out: &mut Vec<u8>, text: &str) {
    put_number(out, text.len() as u64);
    out.extend_from_slice(text.as_bytes());
}

/// The file's bytes, or None if they'd be too many.
pub fn encode(image: &Image) -> Option<Vec<u8>> {
    let mut out = Vec::new();
    out.extend_from_slice(MAGIC);
    out.extend_from_slice(&image.file_hash);
    put_number(&mut out, image.current as u64);
    put_number(&mut out, image.nodes.len() as u64);
    let empty = Vec::new();
    for node in &image.nodes {
        let parent = node.parent.and_then(|index| image.nodes.get(index)).map_or(&empty, |parent| &parent.lines);
        put_number(&mut out, node.parent.map_or(u64::MAX, |index| index as u64));
        put_number(&mut out, node.when);
        put_text(&mut out, &node.label);
        for place in [node.caret.0, node.caret.1, node.anchor.0, node.anchor.1] {
            put_number(&mut out, place as u64);
        }
        out.push(eol_code(node.base));
        out.push(eol_code(node.insert));
        put_number(&mut out, node.eols.len() as u64);
        for (line, eol) in &node.eols {
            put_number(&mut out, *line as u64);
            out.push(eol_code(*eol));
        }
        // Lines are shared between states wherever they're unchanged, so most of this is a quick look at two pointers.
        let same = |a: &Arc<str>, b: &Arc<str>| Arc::ptr_eq(a, b) || a == b;
        let start = parent.iter().zip(&node.lines).take_while(|(a, b)| same(a, b)).count();
        let room = parent.len().min(node.lines.len()) - start;
        let end = parent.iter().rev().zip(node.lines.iter().rev()).take(room).take_while(|(a, b)| same(a, b)).count();
        put_number(&mut out, start as u64);
        put_number(&mut out, end as u64);
        let changed = &node.lines[start..node.lines.len() - end];
        put_number(&mut out, changed.len() as u64);
        for line in changed {
            put_text(&mut out, line);
        }
        if out.len() > MOST_FILE_BYTES {
            return None;
        }
    }
    let checksum = hash_of(&out);
    out.extend_from_slice(&checksum);
    Some(out)
}

struct Reader<'a> {
    data: &'a [u8],
    at: usize,
}

impl Reader<'_> {
    fn take(&mut self, count: usize) -> Result<&[u8], String> {
        let end = self.at.checked_add(count).filter(|end| *end <= self.data.len()).ok_or("It ends too soon.")?;
        let part = &self.data[self.at..end];
        self.at = end;
        Ok(part)
    }

    fn number(&mut self) -> Result<u64, String> {
        let part = self.take(8)?;
        Ok(u64::from_le_bytes(part.try_into().map_err(|_| "A number is the wrong size.")?))
    }

    /// A count of things that each take at least some bytes, so a damaged count can't ask for endless memory.
    fn count(&mut self, each: usize) -> Result<usize, String> {
        let count = self.number()?;
        let left = (self.data.len() - self.at) as u64;
        if count.saturating_mul(each as u64) > left {
            return Err("A count is more than there's room for.".into());
        }
        Ok(count as usize)
    }

    fn text(&mut self) -> Result<String, String> {
        let length = self.count(1)?;
        let bytes = self.take(length)?;
        std::str::from_utf8(bytes).map(str::to_string).map_err(|_| "Some text isn't valid.".to_string())
    }

    fn eol(&mut self) -> Result<Eol, String> {
        eol_from(self.take(1)?[0])
    }
}

pub fn decode(data: &[u8]) -> Result<Image, String> {
    if data.len() < MAGIC.len() + CHECKSUM_BYTES || !data.starts_with(MAGIC) {
        return Err("It isn't one of these files.".into());
    }
    let (body, checksum) = data.split_at(data.len() - CHECKSUM_BYTES);
    if hash_of(body) != checksum {
        return Err("It's been damaged.".into());
    }
    let mut reader = Reader { data: body, at: MAGIC.len() };
    let file_hash: [u8; 32] = reader.take(32)?.try_into().map_err(|_| "A hash is the wrong size.")?;
    let current = reader.number()? as usize;
    let count = reader.count(1)?;
    let mut nodes: Vec<NodeImage> = Vec::with_capacity(count);
    for index in 0..count {
        let parent = match reader.number()? {
            u64::MAX => None,
            number if (number as usize) < index => Some(number as usize),
            _ => return Err("A state comes before the one it came from.".into()),
        };
        let when = reader.number()?;
        let label = reader.text()?;
        let mut places = [0usize; 4];
        for place in &mut places {
            *place = reader.number()? as usize;
        }
        let base = reader.eol()?;
        let insert = reader.eol()?;
        let mut eols = BTreeMap::new();
        for _ in 0..reader.count(9)? {
            let line = reader.number()? as usize;
            eols.insert(line, reader.eol()?);
        }
        let empty = Vec::new();
        let from = parent.map_or(&empty, |index| &nodes[index].lines);
        let start = reader.number()? as usize;
        let end = reader.number()? as usize;
        if start.checked_add(end).is_none_or(|kept| kept > from.len()) {
            return Err("A state keeps more lines than it came from.".into());
        }
        let changed = reader.count(8)?;
        let mut lines: Vec<Arc<str>> = Vec::with_capacity(start + changed + end);
        lines.extend_from_slice(&from[..start]);
        for _ in 0..changed {
            lines.push(Arc::from(reader.text()?));
        }
        lines.extend_from_slice(&from[from.len() - end..]);
        if lines.is_empty() {
            return Err("A state has no lines.".into());
        }
        nodes.push(NodeImage { parent, when, label, caret: (places[0], places[1]), anchor: (places[2], places[3]), lines, base, insert, eols });
    }
    if current >= nodes.len() {
        return Err("It doesn't say where the text is.".into());
    }
    Ok(Image { file_hash, current, nodes })
}

/// Writes a document's history, whole or not at all, and makes room among the others.
pub fn write(root: &Path, path: &Path, image: &Image) -> std::io::Result<()> {
    let target = file_for(root, path);
    let Some(bytes) = encode(image) else {
        // An older, smaller history no longer matches the file, and there's no use keeping it.
        let _ = std::fs::remove_file(&target);
        return Ok(());
    };
    std::fs::create_dir_all(root)?;
    crate::codec::write_file(&target, &bytes)?;
    tidy(root);
    Ok(())
}

/// The history kept for a file, if it's for what the file holds now.
pub fn read(root: &Path, path: &Path, file_bytes: &[u8]) -> Option<Image> {
    let data = read_limited(&file_for(root, path))?;
    let image = decode(&data).ok()?;
    (image.file_hash == hash_of(file_bytes)).then_some(image)
}

pub fn exists(root: &Path, path: &Path) -> bool {
    file_for(root, path).is_file()
}

fn read_limited(path: &Path) -> Option<Vec<u8>> {
    if std::fs::metadata(path).ok()?.len() > (MOST_FILE_BYTES + CHECKSUM_BYTES) as u64 {
        return None;
    }
    std::fs::read(path).ok()
}

pub fn forget(root: &Path, path: &Path) {
    let _ = std::fs::remove_file(file_for(root, path));
}

/// Keeps the folder to a sensible size. Only ever touches its own kind of file.
fn tidy(root: &Path) {
    let Ok(entries) = std::fs::read_dir(root) else {
        return;
    };
    let mut files: Vec<(std::time::SystemTime, u64, PathBuf)> = entries
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| path.extension().is_some_and(|extension| extension == "undo"))
        .filter_map(|path| {
            let meta = std::fs::metadata(&path).ok()?;
            Some((meta.modified().ok()?, meta.len(), path))
        })
        .collect();
    files.sort_by_key(|file| std::cmp::Reverse(file.0));
    let mut total = 0;
    for (place, (_, size, path)) in files.iter().enumerate() {
        total += size;
        if place >= MOST_HISTORIES || total > MOST_TOTAL_BYTES {
            let _ = std::fs::remove_file(path);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lines(rows: &[&str]) -> Vec<Arc<str>> {
        rows.iter().map(|row| Arc::from(*row)).collect()
    }

    fn node(parent: Option<usize>, rows: &[&str]) -> NodeImage {
        NodeImage { parent, when: 100, label: "Line 1: x".into(), caret: (1, 2), anchor: (0, 1), lines: lines(rows), base: Eol::CrLf, insert: Eol::Lf, eols: BTreeMap::from([(0, Eol::Lf)]) }
    }

    fn sample() -> Image {
        Image { file_hash: hash_of(b"file"), current: 2, nodes: vec![node(None, &["a", "b", "c"]), node(Some(0), &["a", "new", "c"]), node(Some(0), &["a", "b", "c", "d \u{10FF41} \u{20AC}"]), node(Some(2), &["only"])] }
    }

    #[test]
    fn what_goes_in_comes_back() {
        let image = sample();
        let back = decode(&encode(&image).unwrap()).unwrap();
        assert_eq!(back.current, 2);
        assert_eq!(back.file_hash, image.file_hash);
        assert_eq!(back.nodes.len(), 4);
        for (a, b) in image.nodes.iter().zip(&back.nodes) {
            assert_eq!(a.parent, b.parent);
            assert_eq!(a.lines, b.lines);
            assert_eq!((a.caret, a.anchor, a.when, &a.label), (b.caret, b.anchor, b.when, &b.label));
            assert_eq!((a.base, a.insert, &a.eols), (b.base, b.insert, &b.eols));
        }
    }

    #[test]
    fn a_long_history_only_costs_what_its_edits_do() {
        let rows: Vec<String> = (0..5000).map(|n| format!("line number {n} of the document, with a little more to it")).collect();
        let shared: Vec<Arc<str>> = rows.iter().map(|row| Arc::from(row.as_str())).collect();
        let mut nodes = vec![NodeImage { lines: shared.clone(), ..node(None, &["x"]) }];
        for n in 1..100 {
            let mut changed = shared.clone();
            changed[n] = Arc::from("changed");
            nodes.push(NodeImage { lines: changed, ..node(Some(n - 1), &["x"]) });
        }
        let image = Image { file_hash: [0; 32], current: 99, nodes };
        let size = encode(&image).unwrap().len();
        let whole: usize = rows.iter().map(|row| row.len() + 8).sum();
        assert!(size < whole * 2, "{size} against {whole} for one copy of the text");
        assert_eq!(decode(&encode(&image).unwrap()).unwrap().nodes[50].lines[50], Arc::from("changed"));
    }

    #[test]
    fn damage_of_any_kind_means_no_history_and_never_a_crash() {
        let good = encode(&sample()).unwrap();
        for cut in 0..good.len() {
            assert!(decode(&good[..cut]).is_err(), "cut to {cut}");
        }
        for place in (0..good.len()).step_by(7) {
            let mut bad = good.clone();
            bad[place] ^= 0x55;
            assert!(decode(&bad).is_err(), "changed at {place}");
        }
        assert!(decode(b"").is_err());
        assert!(decode(b"not one of these files at all, as anyone could see, not even close").is_err());
    }

    /// What a damaged file with a correct checksum would be read as, to be sure the checks don't lean on the checksum.
    fn with_checksum(mut body: Vec<u8>) -> Vec<u8> {
        let checksum = hash_of(&body);
        body.extend_from_slice(&checksum);
        body
    }

    #[test]
    fn nonsense_with_a_good_checksum_is_still_refused_safely() {
        let good = encode(&sample()).unwrap();
        let body = &good[..good.len() - CHECKSUM_BYTES];
        for place in 0..body.len() {
            for flip in [0x01, 0xFF] {
                let mut bad = body.to_vec();
                bad[place] ^= flip;
                let _ = decode(&with_checksum(bad));
            }
        }
    }

    #[test]
    fn a_history_too_big_to_keep_isnt_kept() {
        let huge: Arc<str> = Arc::from("x".repeat(MOST_FILE_BYTES + 1));
        let image = Image { file_hash: [0; 32], current: 0, nodes: vec![NodeImage { lines: vec![huge], ..node(None, &["x"]) }] };
        assert!(encode(&image).is_none());
    }

    #[test]
    fn a_file_changed_since_has_moved_on_from_its_history() {
        let root = std::env::temp_dir().join(format!("limoniter-history-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let path = Path::new(r"C:\Some\File.txt");
        write(&root, path, &sample()).unwrap();
        assert!(exists(&root, path));
        assert!(read(&root, path, b"file").is_some());
        assert!(read(&root, Path::new(r"c:\some\file.TXT"), b"file").is_some(), "capitals don't count");
        assert!(read(&root, path, b"changed since").is_none());
        assert!(read(&root, Path::new(r"C:\Other.txt"), b"file").is_none());
        forget(&root, path);
        assert!(!exists(&root, path));
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn the_folder_is_kept_to_a_sensible_size_and_only_its_own_files_are_touched() {
        let root = std::env::temp_dir().join(format!("limoniter-history-tidy-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(root.join("keep.txt"), "not ours").unwrap();
        for n in 0..MOST_HISTORIES + 5 {
            std::fs::write(root.join(format!("{n}.undo")), "x").unwrap();
        }
        tidy(&root);
        let names: Vec<_> = std::fs::read_dir(&root).unwrap().flatten().map(|entry| entry.file_name().to_string_lossy().into_owned()).collect();
        assert_eq!(names.iter().filter(|name| name.ends_with(".undo")).count(), MOST_HISTORIES);
        assert!(names.contains(&"keep.txt".to_string()));
        let _ = std::fs::remove_dir_all(&root);
    }
}
