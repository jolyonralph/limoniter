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

/// Rewrites a table too large to hold, a record at a time, into a new file: each record changed, and the records
/// in the order asked for. Lines after the last record (most often the empty line a file ends with) stay last.
#[allow(clippy::too_many_arguments, reason = "everything a rewrite needs, each of it different")]
pub fn rewrite_records(title: &'static str, store: LineStore, format: TextFormat, delimiter: char, records: Arc<Vec<crate::table::Record>>, arrange: Arrange, mut change: RecordChange, path: PathBuf, doc_id: u64) -> BigJob {
    let sorting = matches!(arrange, Arrange::SortedBy { .. });
    let total = records.len() as u64 * if sorting { 2 } else { 1 };
    BigJob::start(title, doc_id, total, move |done, cancel| {
        let written = (|| -> Result<Option<Problems>, String> {
            let stopped = || Err("It was stopped.".to_string());
            let order: Vec<usize> = match arrange {
                Arrange::AsTheyAre => (0..records.len()).collect(),
                Arrange::SortedBy { column, headings } => {
                    let skip = usize::from(headings && !records.is_empty());
                    let mut keyed: Vec<(crate::table::SortKey, usize)> = Vec::with_capacity(records.len());
                    for (i, record) in records.iter().enumerate().skip(skip) {
                        if i % LINES_AT_A_TIME == 0 {
                            if cancel.load(Ordering::Relaxed) {
                                return stopped();
                            }
                            done.store(i as u64, Ordering::Relaxed);
                        }
                        let text = crate::table::record_text(&store, *record);
                        keyed.push((crate::table::SortKey::new(crate::table::fields(&text, delimiter).into_iter().nth(column).map(|f| f.value).unwrap_or_default()), i));
                    }
                    keyed.sort_by(|a, b| a.0.cmp(&b.0));
                    (0..skip).chain(keyed.into_iter().map(|(_, i)| i)).collect()
                }
            };
            let base = if sorting { records.len() as u64 } else { 0 };
            if let Some(folder) = path.parent() {
                std::fs::create_dir_all(folder).map_err(|e| e.to_string())?;
            }
            let mut out = std::io::BufWriter::with_capacity(1 << 20, std::fs::File::create(&path).map_err(|e| e.to_string())?);
            let tail = records.last().map_or(0, |last| last.last + 1);
            let (eol, last_eol) = (store.insert_eol(), store.terminator(store.len().saturating_sub(1)));
            let mut at = 0;
            let mut first = true;
            // A stretch of records at a time, then whatever follows the last.
            while at <= order.len() {
                if cancel.load(Ordering::Relaxed) {
                    return stopped();
                }
                let end = (at + LINES_AT_A_TIME).min(order.len());
                let mut lines: Vec<String> = Vec::new();
                for &r in &order[at..end] {
                    lines.extend(crate::table::lines_of(&change(&crate::table::record_text(&store, records[r]))));
                }
                let finished = end == order.len();
                if finished {
                    lines.extend((tail..store.len()).map(|i| store.get(i).to_string()));
                }
                let count = lines.len();
                // Every stretch but the last ends with a line ending, so a line follows it.
                if !finished {
                    lines.push(String::new());
                }
                let mut target = format.clone();
                target.has_bom &= first;
                let ending = |i: usize| if finished && i + 1 == count { last_eol } else { eol };
                match codec::export(&lines, &ending, &format, &target, Lossy::Refuse) {
                    Ok(bytes) => out.write_all(&bytes).map_err(|e| e.to_string())?,
                    Err(problems) => return Ok(Some(problems)),
                }
                first = false;
                done.store(base + end as u64, Ordering::Relaxed);
                if finished {
                    break;
                }
                at = end;
            }
            out.into_inner().map_err(|e| e.to_string())?.sync_all().map_err(|e| e.to_string())?;
            Ok(None)
        })();
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
