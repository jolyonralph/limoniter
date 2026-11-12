//! Work over the whole of a file too large to hold in memory, done in the background a stretch of lines at a time:
//! counting its words, writing a copy in another encoding, and changing every line, which writes the changed file
//! out for the document to carry on from.

use crate::codec::{self, Eol, Lossy, Problems, TextFormat};
use crate::line_store::LineStore;
use crate::text_buffer::{Counts, count_lines};
use std::io::Write;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

/// How many lines are read and worked on at a time.
const LINES_AT_A_TIME: usize = 50_000;

pub enum Outcome {
    /// Something to tell the person.
    Done(String),
    /// Characters the copy's encoding can't hold; nothing was written.
    Problems(Problems, TextFormat),
    /// Every line changed and written to a new file, for the document to carry on from in this format.
    Rewritten { path: PathBuf, format: TextFormat },
    /// The numbers of the lines that contain some text.
    Lines { needle: String, lines: Vec<usize> },
    /// Two files compared, ready to show.
    Compared(Box<crate::compare::Comparison>),
    Failed(String),
}

pub struct BigJob {
    pub title: &'static str,
    pub doc_id: u64,
    done: Arc<AtomicU64>,
    total: u64,
    cancel: Arc<AtomicBool>,
    result: Arc<Mutex<Option<Outcome>>>,
}

impl BigJob {
    fn start(title: &'static str, doc_id: u64, total: u64, work: impl FnOnce(&AtomicU64, &AtomicBool) -> Outcome + Send + 'static) -> Self {
        let (done, cancel, result) = (Arc::new(AtomicU64::new(0)), Arc::new(AtomicBool::new(false)), Arc::new(Mutex::new(None)));
        let (worker_done, worker_cancel, worker_result) = (done.clone(), cancel.clone(), result.clone());
        std::thread::spawn(move || {
            let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| work(&worker_done, &worker_cancel))).unwrap_or_else(|_| Outcome::Failed("It failed unexpectedly.".to_string()));
            *worker_result.lock().unwrap_or_else(std::sync::PoisonError::into_inner) = Some(outcome);
        });
        Self { title, doc_id, done, total, cancel, result }
    }

    pub fn share(&self) -> f32 {
        self.done.load(Ordering::Relaxed) as f32 / self.total.max(1) as f32
    }

    /// The outcome, once it's finished.
    pub fn take(&self) -> Option<Outcome> {
        self.result.lock().unwrap_or_else(std::sync::PoisonError::into_inner).take()
    }

    pub fn cancel(&self) {
        self.cancel.store(true, Ordering::Relaxed);
    }
}

impl Drop for BigJob {
    fn drop(&mut self) {
        self.cancel();
    }
}

/// Adds up a stretch's counts. Every stretch but the last is counted as if a line followed it, so that its last
/// line's ending is counted and the line isn't taken as the file's last.
fn add_counts(total: &mut Counts, part: &Counts, first_line: u64) {
    if part.longest_chars > total.longest_chars {
        total.longest_chars = part.longest_chars;
        total.longest_line = first_line + part.longest_line;
    }
    total.words += part.words;
    total.lines += part.lines;
    total.non_space += part.non_space;
    total.characters += part.characters;
    total.non_empty += part.non_empty;
}

/// Counts the words, lines and characters of every line, as Word Count does for a file held in memory.
pub fn count(store: LineStore, eol_chars: u64, doc_id: u64) -> BigJob {
    let total = store.len() as u64;
    BigJob::start("Word Count", doc_id, total, move |done, cancel| {
        let mut sum = Counts { longest_line: 1, ..Counts::default() };
        let mut without_endings = 0u64;
        let mut at = 0;
        while at < store.len() {
            if cancel.load(Ordering::Relaxed) {
                return Outcome::Failed("It was stopped.".to_string());
            }
            let end = (at + LINES_AT_A_TIME).min(store.len());
            let mut lines = store.range_vec(at, end);
            let last = end == store.len();
            if !last {
                lines.push(Arc::from(""));
            }
            let mut part = count_lines(&lines, eol_chars);
            let real = (end - at) as u64;
            if !last {
                part.lines -= 1;
            }
            without_endings += part.characters - eol_chars * if last { real - 1 } else { real };
            add_counts(&mut sum, &part, at as u64);
            at = end;
            done.store(at as u64, Ordering::Relaxed);
        }
        sum.average = without_endings / sum.lines.max(1);
        Outcome::Done(format!("Number of Words: {}\nNumber of Unwrapped Lines: {}\nNumber of characters (non-space): {}", sum.words, sum.lines, sum.non_space))
    })
}

/// Writes every line to `path` in another encoding, through a temporary file beside it. With `Lossy::Refuse`,
/// the first stretch with characters the encoding can't hold stops it, and nothing is written.
pub fn save_converted(store: LineStore, from: TextFormat, to: TextFormat, lossy: Lossy, path: PathBuf, doc_id: u64) -> BigJob {
    let total = store.len() as u64;
    BigJob::start("Save a copy", doc_id, total, move |done, cancel| {
        let mut temp_name = path.as_os_str().to_owned();
        temp_name.push(".limoniter-tmp");
        let temp = PathBuf::from(temp_name);
        let written = (|| -> Result<Option<Problems>, String> {
            let mut out = std::io::BufWriter::with_capacity(1 << 20, std::fs::File::create(&temp).map_err(|e| e.to_string())?);
            let mut at = 0;
            while at < store.len() {
                if cancel.load(Ordering::Relaxed) {
                    return Err("It was stopped.".to_string());
                }
                let end = (at + LINES_AT_A_TIME).min(store.len());
                let mut lines = store.range_vec(at, end);
                // Every stretch but the last ends with its last line's ending, so a line follows it.
                if end < store.len() {
                    lines.push(Arc::from(""));
                }
                let mut target = to.clone();
                target.has_bom &= at == 0;
                match codec::export(&lines, &|i| store.terminator(at + i), &from, &target, lossy) {
                    Ok(bytes) => out.write_all(&bytes).map_err(|e| e.to_string())?,
                    Err(mut problems) => {
                        for problem in &mut problems.listed {
                            problem.line += at;
                        }
                        return Ok(Some(problems));
                    }
                }
                at = end;
                done.store(at as u64, Ordering::Relaxed);
            }
            out.into_inner().map_err(|e| e.to_string())?.sync_all().map_err(|e| e.to_string())?;
            std::fs::rename(&temp, &path).map_err(|e| e.to_string())?;
            Ok(None)
        })();
        match written {
            Ok(None) => Outcome::Done(format!("Saved a copy as {} in {}.", path.display(), to.encoding_name())),
            Ok(Some(problems)) => {
                let _ = std::fs::remove_file(&temp);
                Outcome::Problems(problems, to)
            }
            Err(problem) => {
                let _ = std::fs::remove_file(&temp);
                Outcome::Failed(format!("Couldn't save {}.\n\n{problem}", path.display()))
            }
        }
    })
}

/// Finds every line that contains some text.
pub fn find_lines(store: LineStore, needle: String, doc_id: u64) -> BigJob {
    let total = store.len() as u64;
    BigJob::start("Filter on selection", doc_id, total, move |done, cancel| {
        let mut found = Vec::new();
        let mut at = 0;
        while at < store.len() {
            if cancel.load(Ordering::Relaxed) {
                return Outcome::Failed("It was stopped.".to_string());
            }
            let end = (at + LINES_AT_A_TIME).min(store.len());
            found.extend(store.range_vec(at, end).iter().enumerate().filter(|(_, line)| line.contains(needle.as_str())).map(|(i, _)| at + i));
            at = end;
            done.store(at as u64, Ordering::Relaxed);
        }
        Outcome::Lines { needle, lines: found }
    })
}

/// How each line is changed: given its number and its text, the text it becomes.
pub type LineChange = Box<dyn FnMut(usize, &str) -> String + Send>;

/// Changes every line of a file too large to hold in memory, writing the result to `path` in the `to` encoding,
/// with every line ending `eol` if given, or as it was. With `Lossy::Refuse`, characters the encoding can't hold stop
/// it, and nothing is used.
#[allow(clippy::too_many_arguments, reason = "everything a rewrite needs, each of it different")]
pub fn rewrite(title: &'static str, store: LineStore, from: TextFormat, to: TextFormat, eol: Option<Eol>, lossy: Lossy, mut change: LineChange, path: PathBuf, doc_id: u64) -> BigJob {
    let total = store.len() as u64;
    BigJob::start(title, doc_id, total, move |done, cancel| {
        let written = (|| -> Result<Option<Problems>, String> {
            if let Some(folder) = path.parent() {
                std::fs::create_dir_all(folder).map_err(|e| e.to_string())?;
            }
            let mut out = std::io::BufWriter::with_capacity(1 << 20, std::fs::File::create(&path).map_err(|e| e.to_string())?);
            let mut at = 0;
            while at < store.len() {
                if cancel.load(Ordering::Relaxed) {
                    return Err("It was stopped.".to_string());
                }
                let end = (at + LINES_AT_A_TIME).min(store.len());
                let mut lines: Vec<String> = store.range_vec(at, end).iter().enumerate().map(|(i, line)| change(at + i, line)).collect();
                // Every stretch but the last ends with its last line's ending, so a line follows it.
                if end < store.len() {
                    lines.push(String::new());
                }
                let mut target = to.clone();
                target.has_bom &= at == 0;
                let ending = |i: usize| match eol {
                    Some(eol) if at + i + 1 < store.len() => eol,
                    _ => store.terminator(at + i),
                };
                match codec::export(&lines, &ending, &from, &target, lossy) {
                    Ok(bytes) => out.write_all(&bytes).map_err(|e| e.to_string())?,
                    Err(mut problems) => {
                        for problem in &mut problems.listed {
                            problem.line += at;
                        }
                        return Ok(Some(problems));
                    }
                }
                at = end;
                done.store(at as u64, Ordering::Relaxed);
            }
            out.into_inner().map_err(|e| e.to_string())?.sync_all().map_err(|e| e.to_string())?;
            Ok(None)
        })();
        match written {
            Ok(None) => Outcome::Rewritten { path, format: to },
            Ok(Some(problems)) => {
                let _ = std::fs::remove_file(&path);
                Outcome::Problems(problems, to)
            }
            Err(problem) => {
                let _ = std::fs::remove_file(&path);
                Outcome::Failed(problem)
            }
        }
    })
}

/// How a table's records are put back in a rewrite.
pub enum Arrange {
    /// In the order they're in.
    AsTheyAre,
    /// By one column's values, as the table sorts them, any headings staying first.
    SortedBy { column: usize, headings: bool },
}

/// A change to one record's text.
pub type RecordChange = Box<dyn FnMut(&str) -> String + Send>;

/// Lines written out in stretches, each encoded as the file was.
struct Writer {
    out: std::io::BufWriter<std::fs::File>,
    format: TextFormat,
    eol: Eol,
    waiting: Vec<String>,
    first: bool,
}

impl Writer {
    fn create(path: &std::path::Path, format: TextFormat, eol: Eol) -> Result<Self, String> {
        if let Some(folder) = path.parent() {
            std::fs::create_dir_all(folder).map_err(|e| e.to_string())?;
        }
        let out = std::io::BufWriter::with_capacity(1 << 20, std::fs::File::create(path).map_err(|e| e.to_string())?);
        Ok(Self { out, format, eol, waiting: Vec::new(), first: true })
    }

    /// Adds lines; every one is followed by a line ending.
    fn lines(&mut self, lines: impl IntoIterator<Item = String>) -> Result<Option<Problems>, String> {
        self.waiting.extend(lines);
        if self.waiting.len() <= LINES_AT_A_TIME {
            return Ok(None);
        }
        // The newest line stays behind, so the last write always has a line to end the file as it ended.
        let newest = self.waiting.pop();
        let result = self.flush(None);
        self.waiting.extend(newest);
        result
    }

    /// Writes what's waiting. The last stretch's last line ends as given; every other line with the usual ending.
    fn flush(&mut self, last: Option<Eol>) -> Result<Option<Problems>, String> {
        let mut lines = std::mem::take(&mut self.waiting);
        let count = lines.len();
        // A stretch that isn't the last ends with its last line's ending, so a line follows it.
        if last.is_none() {
            lines.push(String::new());
        }
        let mut target = self.format.clone();
        target.has_bom &= self.first;
        let eol = self.eol;
        let ending = |i: usize| match last {
            Some(last) if i + 1 == count => last,
            _ => eol,
        };
        match codec::export(&lines, &ending, &self.format, &target, Lossy::Refuse) {
            Ok(bytes) => self.out.write_all(&bytes).map_err(|e| e.to_string())?,
            Err(problems) => return Ok(Some(problems)),
        }
        self.first = false;
        Ok(None)
    }

    fn finish(mut self, last: Eol) -> Result<Option<Problems>, String> {
        if let Some(problems) = self.flush(Some(last))? {
            return Ok(Some(problems));
        }
        self.out.into_inner().map_err(|e| e.to_string())?.sync_all().map_err(|e| e.to_string())?;
        Ok(None)
    }
}

/// How much record text is sorted in memory at a time; larger tables are sorted in runs that are then merged.
#[cfg(not(test))]
const SORTED_AT_A_TIME: usize = 128 << 20;
#[cfg(test)]
const SORTED_AT_A_TIME: usize = 300;

/// Rewrites a table too large to hold, a record at a time, into a new file: each record changed, and the records
/// in the order asked for. Lines after the last record (most often the empty line a file ends with) stay last.
///
/// Sorting reads the records in the order they're in, a stretch at a time, sorts each stretch into a run file
/// beside the new file, and then merges the runs, so it never reads the file out of order or holds all of it.
#[allow(clippy::too_many_arguments, reason = "everything a rewrite needs, each of it different")]
pub fn rewrite_records(title: &'static str, store: LineStore, format: TextFormat, delimiter: char, records: Arc<Vec<crate::table::Record>>, arrange: Arrange, mut change: RecordChange, path: PathBuf, doc_id: u64) -> BigJob {
    use crate::table::{SortKey, fields, lines_of, record_text};
    let sort = match arrange {
        Arrange::AsTheyAre => None,
        Arrange::SortedBy { column, headings } => Some((column, usize::from(headings && !records.is_empty()))),
    };
    let total = records.len() as u64 * if sort.is_some() { 2 } else { 1 };
    BigJob::start(title, doc_id, total, move |done, cancel| {
        let mut runs: Vec<PathBuf> = Vec::new();
        let written = (|| -> Result<Option<Problems>, String> {
            let stopped = || Err("It was stopped.".to_string());
            let key_of = |text: &str, column: usize| SortKey::new(fields(text, delimiter).into_iter().nth(column).map(|f| f.value).unwrap_or_default());
            let mut out = Writer::create(&path, format.clone(), store.insert_eol())?;
            let tail = records.last().map_or(0, |last| last.last + 1);
            let last_eol = store.terminator(store.len().saturating_sub(1));
            match sort {
                None => {
                    for (i, record) in records.iter().enumerate() {
                        if i % LINES_AT_A_TIME == 0 {
                            if cancel.load(Ordering::Relaxed) {
                                return stopped();
                            }
                            done.store(i as u64, Ordering::Relaxed);
                        }
                        if let Some(problems) = out.lines(lines_of(&change(&record_text(&store, *record))))? {
                            return Ok(Some(problems));
                        }
                    }
                }
                Some((column, skip)) => {
                    // The headings stay first.
                    for record in &records[..skip] {
                        if let Some(problems) = out.lines(lines_of(&change(&record_text(&store, *record))))? {
                            return Ok(Some(problems));
                        }
                    }
                    // Sorted runs, each a list of (original place, text).
                    let mut chunk: Vec<(SortKey, usize, String)> = Vec::new();
                    let mut held = 0;
                    let write_run = |chunk: &mut Vec<(SortKey, usize, String)>, runs: &mut Vec<PathBuf>| -> Result<(), String> {
                        chunk.sort_by(|a, b| a.0.cmp(&b.0).then(a.1.cmp(&b.1)));
                        let run = path.with_extension(format!("run{}", runs.len()));
                        runs.push(run.clone());
                        let mut file = std::io::BufWriter::new(std::fs::File::create(&run).map_err(|e| e.to_string())?);
                        for (_, place, text) in chunk.drain(..) {
                            file.write_all(&(place as u64).to_le_bytes()).map_err(|e| e.to_string())?;
                            file.write_all(&(text.len() as u64).to_le_bytes()).map_err(|e| e.to_string())?;
                            file.write_all(text.as_bytes()).map_err(|e| e.to_string())?;
                        }
                        file.flush().map_err(|e| e.to_string())
                    };
                    for (i, record) in records.iter().enumerate().skip(skip) {
                        if i % LINES_AT_A_TIME == 0 {
                            if cancel.load(Ordering::Relaxed) {
                                return stopped();
                            }
                            done.store(i as u64, Ordering::Relaxed);
                        }
                        let text = change(&record_text(&store, *record));
                        held += text.len() + 64;
                        chunk.push((key_of(&text, column), i, text));
                        if held >= SORTED_AT_A_TIME {
                            write_run(&mut chunk, &mut runs)?;
                            held = 0;
                        }
                    }
                    if !chunk.is_empty() {
                        write_run(&mut chunk, &mut runs)?;
                    }
                    // Merged: always the smallest of what's at the front of each run, the earliest place first.
                    let mut readers = Vec::new();
                    for run in &runs {
                        readers.push(std::io::BufReader::with_capacity(1 << 20, std::fs::File::open(run).map_err(|e| e.to_string())?));
                    }
                    let next = |reader: &mut std::io::BufReader<std::fs::File>| -> Result<Option<(usize, String)>, String> {
                        use std::io::Read;
                        let mut number = [0u8; 8];
                        match reader.read_exact(&mut number) {
                            Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => return Ok(None),
                            other => other.map_err(|e| e.to_string())?,
                        }
                        let place = u64::from_le_bytes(number) as usize;
                        reader.read_exact(&mut number).map_err(|e| e.to_string())?;
                        let mut text = vec![0u8; u64::from_le_bytes(number) as usize];
                        reader.read_exact(&mut text).map_err(|e| e.to_string())?;
                        Ok(Some((place, String::from_utf8(text).map_err(|e| e.to_string())?)))
                    };
                    let mut heap = std::collections::BinaryHeap::new();
                    for (run, reader) in readers.iter_mut().enumerate() {
                        if let Some((place, text)) = next(reader)? {
                            heap.push(std::cmp::Reverse((key_of(&text, column), place, run, text)));
                        }
                    }
                    let mut merged = records.len() as u64;
                    while let Some(std::cmp::Reverse((_, _, run, text))) = heap.pop() {
                        merged += 1;
                        if merged.is_multiple_of(LINES_AT_A_TIME as u64) {
                            if cancel.load(Ordering::Relaxed) {
                                return stopped();
                            }
                            done.store(merged, Ordering::Relaxed);
                        }
                        if let Some(problems) = out.lines(lines_of(&text))? {
                            return Ok(Some(problems));
                        }
                        if let Some((place, text)) = next(&mut readers[run])? {
                            heap.push(std::cmp::Reverse((key_of(&text, column), place, run, text)));
                        }
                    }
                }
            }
            // Whatever follows the last record, then the file's own last ending.
            if let Some(problems) = out.lines((tail..store.len()).map(|i| store.get(i).to_string()))? {
                return Ok(Some(problems));
            }
            out.finish(last_eol)
        })();
        for run in runs {
            let _ = std::fs::remove_file(run);
        }
        match written {
            Ok(None) => Outcome::Rewritten { path, format },
            Ok(Some(problems)) => {
                let _ = std::fs::remove_file(&path);
                Outcome::Problems(problems, format)
            }
            Err(problem) => {
                let _ = std::fs::remove_file(&path);
                Outcome::Failed(problem)
            }
        }
    })
}

/// One side of a comparison: lines already there, or a file on disk still being scanned.
pub enum Side {
    Lines(LineStore),
    Scanning(Arc<crate::line_store::Source>),
}

/// Compares two sides too large to hold in memory twice over. Each line is boiled down to a hash, the hashes are
/// compared, and the comparison reads the lines it shows from the files as it's drawn.
pub fn compare(left_name: String, right_name: String, left: LineStore, right: Side, doc_id: u64) -> BigJob {
    // Progress in thousandths: the left side's lines, then the right's (scanning it first if need be).
    BigJob::start("Compare", doc_id, 1000, move |done, cancel| {
        let hash_all = |store: &LineStore, from: u64, share: u64| -> Option<Vec<u64>> {
            let mut hashes = Vec::with_capacity(store.len());
            let mut at = 0;
            while at < store.len() {
                if cancel.load(Ordering::Relaxed) {
                    return None;
                }
                let end = (at + LINES_AT_A_TIME).min(store.len());
                hashes.extend(store.range_vec(at, end).iter().map(|line| crate::compare::line_hash(line)));
                at = end;
                done.store(from + share * at as u64 / store.len().max(1) as u64, Ordering::Relaxed);
            }
            Some(hashes)
        };
        let stopped = || Outcome::Failed("It was stopped.".to_string());
        let Some(left_hashes) = hash_all(&left, 0, 450) else {
            return stopped();
        };
        let right = match right {
            Side::Lines(store) => store,
            Side::Scanning(source) => {
                while !source.is_finished() {
                    if cancel.load(Ordering::Relaxed) {
                        source.stop();
                        return stopped();
                    }
                    done.store(450 + (225.0 * source.progress()) as u64, Ordering::Relaxed);
                    std::thread::sleep(std::time::Duration::from_millis(50));
                }
                if let Some(problem) = source.failure() {
                    return Outcome::Failed(problem);
                }
                LineStore::from_source(source)
            }
        };
        let (from, share) = if done.load(Ordering::Relaxed) > 450 { (675, 225) } else { (450, 450) };
        let Some(right_hashes) = hash_all(&right, from, share) else {
            return stopped();
        };
        let runs = crate::compare::runs(&left_hashes, &right_hashes);
        drop((left_hashes, right_hashes));
        Outcome::Compared(Box::new(crate::compare::Comparison::from_runs(left_name, right_name, left, right, runs)))
    })
}

#[cfg(test)]
mod tests {
    #[test]
    fn a_large_table_is_sorted_through_merged_runs_as_a_stable_sort_would() {
        let mut rows = vec!["key,n".to_string()];
        for i in 0..500 {
            rows.push(format!("{},{i}", (i * 37) % 23));
        }
        rows.push("\"multi\nline\",x".to_string());
        rows.push(String::new());
        let lines: Vec<Arc<str>> = rows.iter().flat_map(|row| row.split('\n').map(Arc::from).collect::<Vec<_>>()).collect();
        let store = LineStore::from_vec(lines.clone());
        let records = Arc::new(crate::table::records(&store, ','));
        let dir = std::env::temp_dir().join(format!("limoniter-sort-runs-{}", std::process::id()));
        let path = dir.join("sorted.csv");
        let job = rewrite_records("Sort", store, TextFormat::default(), ',', records.clone(), Arrange::SortedBy { column: 0, headings: true }, Box::new(str::to_string), path.clone(), 1);
        let outcome = loop {
            if let Some(outcome) = job.take() {
                break outcome;
            }
            std::thread::sleep(std::time::Duration::from_millis(2));
        };
        assert!(matches!(outcome, Outcome::Rewritten { .. }));
        // As the table sorts in memory: stably, by the column's values.
        let mut expected: Vec<String> = records[1..].iter().map(|r| crate::table::record_text(&lines, *r)).collect();
        expected.sort_by(|a, b| crate::table::compare_values(a.split(',').next().unwrap(), b.split(',').next().unwrap()));
        let written = std::fs::read_to_string(&path).unwrap().replace("\r\n", "\n");
        assert_eq!(written, format!("key,n\n{}\n", expected.join("\n")));
        let left: Vec<_> = std::fs::read_dir(&dir).unwrap().flatten().map(|e| e.file_name()).collect();
        assert_eq!(left.len(), 1, "the runs are gone: {left:?}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    use super::*;
    use crate::line_store::Source;

    fn wait(job: &BigJob) -> Outcome {
        loop {
            if let Some(outcome) = job.take() {
                return outcome;
            }
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
    }

    /// A file kept on disk, counted and converted a stretch at a time, comes out as it would held in memory whole.
    #[test]
    fn stretches_add_up_to_the_whole() {
        let dir = std::env::temp_dir().join(format!("limoniter-big-jobs-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("lines.txt");
        let mut text = String::new();
        for i in 0..(LINES_AT_A_TIME * 2 + 1234) {
            text += &format!("line {i} caf\u{e9} {}\r\n", "x".repeat(i % 37));
        }
        text += "no ending at the end";
        std::fs::write(&path, &text).unwrap();
        let source = Source::open_as(&path, None).unwrap();
        while !source.is_finished() {
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        let store = LineStore::from_source(source);
        let lines = store.range_vec(0, store.len());
        let whole = count_lines(&lines, 2);
        let Outcome::Done(said) = wait(&count(store.clone(), 2, 1)) else { panic!("it counted") };
        assert_eq!(said, format!("Number of Words: {}\nNumber of Unwrapped Lines: {}\nNumber of characters (non-space): {}", whole.words, whole.lines, whole.non_space));
        let from = store.source().unwrap().format();
        let to = TextFormat { charset: crate::charset::by_id("iso-8859-1").unwrap(), ..from.clone() };
        let copy = dir.join("copy.txt");
        let Outcome::Done(_) = wait(&save_converted(store.clone(), from.clone(), to.clone(), Lossy::Refuse, copy.clone(), 1)) else { panic!("it saved") };
        let expected = codec::export(&lines, &|i| store.terminator(i), &from, &to, Lossy::Refuse).unwrap();
        assert_eq!(std::fs::read(&copy).unwrap(), expected);
        // A character the copy's encoding can't hold stops it before anything is written.
        let ascii = TextFormat { charset: crate::charset::by_id("zx-spectrum").unwrap(), ..from.clone() };
        std::fs::remove_file(&copy).unwrap();
        let Outcome::Problems(problems, _) = wait(&save_converted(store, from, ascii, Lossy::Refuse, copy.clone(), 1)) else { panic!("it refused") };
        assert_eq!(problems.listed[0].line, 0);
        assert!(!copy.exists());
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
