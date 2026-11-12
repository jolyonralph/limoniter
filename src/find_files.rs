//! Find in Files: searches a folder on a background thread, so the window stays responsive.

use crate::codec::{self, TextFormat};
use crate::text_buffer::{SearchOptions, TextBuffer};
use regex::Regex;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

/// Stops at this many results, because more can't usefully be read.
pub const MAX_HITS: usize = 10_000;
const MAX_SHOWN_LINE_CHARS: usize = 300;

#[derive(Clone, Debug, PartialEq)]
pub struct Hit {
    pub path: PathBuf,
    pub line: usize,
    pub start: usize,
    pub end: usize,
    pub text: String,
}

#[derive(Default)]
pub struct SearchJob {
    pub hits: Mutex<Vec<Hit>>,
    pub files_searched: AtomicUsize,
    pub files_unreadable: AtomicUsize,
    pub finished: AtomicBool,
    pub cancel: AtomicBool,
    pub truncated: AtomicBool,
}

/// Matches a name against patterns like "*.rs;*.txt"; an empty list matches everything.
pub fn name_matches(name: &str, patterns: &str) -> bool {
    let name = name.to_lowercase();
    let mut any = false;
    for pattern in patterns.split([';', ',']).map(str::trim).filter(|p| !p.is_empty()) {
        any = true;
        if wildcard(&pattern.to_lowercase(), &name) {
            return true;
        }
    }
    !any
}

fn wildcard(pattern: &str, name: &str) -> bool {
    let p: Vec<char> = pattern.chars().collect();
    let n: Vec<char> = name.chars().collect();
    let (mut pi, mut ni) = (0, 0);
    let mut back: Option<(usize, usize)> = None;
    while ni < n.len() {
        if pi < p.len() && (p[pi] == '?' || p[pi] == n[ni]) {
            pi += 1;
            ni += 1;
        } else if pi < p.len() && p[pi] == '*' {
            back = Some((pi, ni));
            pi += 1;
        } else if let Some((star, matched)) = back {
            pi = star + 1;
            ni = matched + 1;
            back = Some((star, matched + 1));
        } else {
            return false;
        }
    }
    p[pi..].iter().all(|c| *c == '*')
}

/// Starts searching a folder; the job fills in as files are read.
pub fn start(folder: PathBuf, patterns: String, recursive: bool, options: &SearchOptions) -> Result<Arc<SearchJob>, String> {
    let re = TextBuffer::build_regex(options)?;
    let job = Arc::new(SearchJob::default());
    let worker = job.clone();
    std::thread::spawn(move || {
        // If this fails, the search ends rather than appearing to run for ever.
        let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| walk(&folder, &patterns, recursive, &re, &worker)));
        worker.finished.store(true, Ordering::SeqCst);
    });
    Ok(job)
}

/// Starts searching a list of files, such as the favorites.
pub fn start_list(files: Vec<PathBuf>, options: &SearchOptions) -> Result<Arc<SearchJob>, String> {
    let re = TextBuffer::build_regex(options)?;
    let job = Arc::new(SearchJob::default());
    let worker = job.clone();
    std::thread::spawn(move || {
        let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            for path in &files {
                if worker.cancel.load(Ordering::Relaxed) || worker.truncated.load(Ordering::Relaxed) {
                    break;
                }
                search_file(path, &re, &worker);
            }
        }));
        worker.finished.store(true, Ordering::SeqCst);
    });
    Ok(job)
}

fn walk(folder: &Path, patterns: &str, recursive: bool, re: &Regex, job: &SearchJob) {
    let Ok(entries) = std::fs::read_dir(folder) else {
        job.files_unreadable.fetch_add(1, Ordering::Relaxed);
        return;
    };
    for entry in entries.flatten() {
        if job.cancel.load(Ordering::Relaxed) || job.truncated.load(Ordering::Relaxed) {
            return;
        }
        // Links are not followed, so a link back up the tree can't make the search go round forever.
        let Ok(kind) = entry.file_type() else { continue };
        let path = entry.path();
        if kind.is_dir() {
            if recursive {
                walk(&path, patterns, recursive, re, job);
            }
        } else if kind.is_file() && name_matches(&entry.file_name().to_string_lossy(), patterns) {
            search_file(&path, re, job);
        }
    }
}

/// Files bigger than this are searched a line at a time, rather than read in whole.
const STREAM_ABOVE: u64 = 16 * 1024 * 1024;

/// Searches a big file as it is read, however large it is.
fn search_big_file(path: &Path, re: &Regex, job: &SearchJob) -> std::io::Result<()> {
    use std::io::BufRead;
    let mut reader = std::io::BufReader::with_capacity(1024 * 1024, std::fs::File::open(path)?);
    let mut line = Vec::new();
    let mut number = 0usize;
    loop {
        line.clear();
        if reader.read_until(b'\n', &mut line)? == 0 {
            return Ok(());
        }
        if number == 0 && codec::looks_binary(&line) {
            return Ok(());
        }
        if job.cancel.load(Ordering::Relaxed) || job.truncated.load(Ordering::Relaxed) {
            return Ok(());
        }
        let text = String::from_utf8_lossy(&line);
        let body = text.trim_end_matches(['\r', '\n']);
        if re.is_match(body) {
            let mut found = Vec::new();
            for m in re.find_iter(body).filter(|m| !m.is_empty()) {
                found.push(Hit { path: path.to_path_buf(), line: number, start: m.start(), end: m.end(), text: body.chars().take(MAX_SHOWN_LINE_CHARS).collect() });
            }
            add_hits(found, job);
        }
        number += 1;
    }
}

/// Searches one file on disk.
pub fn search_file(path: &Path, re: &Regex, job: &SearchJob) {
    if std::fs::metadata(path).is_ok_and(|m| m.len() > STREAM_ABOVE) {
        job.files_searched.fetch_add(1, Ordering::Relaxed);
        if search_big_file(path, re, job).is_err() {
            job.files_unreadable.fetch_add(1, Ordering::Relaxed);
        }
        return;
    }
    let bytes = std::fs::read(path).ok();
    let Some(bytes) = bytes else {
        job.files_unreadable.fetch_add(1, Ordering::Relaxed);
        return;
    };
    job.files_searched.fetch_add(1, Ordering::Relaxed);
    if codec::looks_binary(&bytes) {
        return;
    }
    let found = codec::detect(&bytes);
    let mut format = TextFormat { charset: found.charset, ..TextFormat::default() };
    let lines = codec::decode(&bytes, &mut format).lines;
    search_lines(path, &lines, re, job);
}

/// Adds every match in some lines to the job's results.
pub fn search_lines<S: AsRef<str>>(path: &Path, lines: &[S], re: &Regex, job: &SearchJob) {
    let mut found = Vec::new();
    for (i, line) in lines.iter().enumerate() {
        let line = line.as_ref();
        for m in re.find_iter(line).filter(|m| !m.is_empty()) {
            found.push(Hit {
                path: path.to_path_buf(),
                line: i,
                start: m.start(),
                end: m.end(),
                text: line.chars().take(MAX_SHOWN_LINE_CHARS).collect(),
            });
        }
    }
    add_hits(found, job);
}

fn add_hits(mut found: Vec<Hit>, job: &SearchJob) {
    if found.is_empty() {
        return;
    }
    let mut hits = job.hits.lock().unwrap();
    let room = MAX_HITS.saturating_sub(hits.len());
    if found.len() > room {
        found.truncate(room);
        job.truncated.store(true, Ordering::Relaxed);
    }
    hits.extend(found);
}

/// The most Replace in Files will read in to change. It has to hold the whole file twice over for a moment.
const MOST_REPLACED_BYTES: u64 = 256 * 1024 * 1024;

/// How a Replace in Files came out.
#[derive(Default)]
pub struct ReplaceReport {
    pub files_changed: usize,
    pub open_files_changed: usize,
    pub replaced: usize,
    pub backups: usize,
    /// Files left as they were, and why.
    pub skipped: Vec<(PathBuf, String)>,
}

#[derive(Default)]
pub struct ReplaceJob {
    pub done: AtomicUsize,
    pub total: AtomicUsize,
    pub finished: AtomicBool,
    pub report: Mutex<ReplaceReport>,
}

/// A name for a backup that isn't already in use, so that an earlier backup is never overwritten.
fn free_backup_name(path: &Path) -> PathBuf {
    let with = |tail: &str| {
        let mut name = path.as_os_str().to_owned();
        name.push(tail);
        PathBuf::from(name)
    };
    let first = with(".bak");
    if !first.exists() {
        return first;
    }
    (2..10_000).map(|number| with(&format!(".bak{number}"))).find(|name| !name.exists()).unwrap_or(first)
}

/// Writes a line that had matches, in the file's own encoding. Lines without any are copied exactly as they were.
fn write_replaced(out: &mut impl std::io::Write, re: &Regex, options: &SearchOptions, format: &TextFormat, set: &'static crate::charset::Charset, body: &[u8], count: &mut usize) -> Result<(), String> {
    let text = codec::decode_line(body, set);
    match crate::text_buffer::replaced_line(re, options, &text) {
        Some((replaced, here)) => {
            let bytes = codec::encode_with(&[replaced], &|_| codec::Eol::None, format, codec::Lossy::Refuse).map_err(|_| format!("{} can't hold the replacement text", format.encoding_name()))?;
            out.write_all(&bytes).map_err(|e| e.to_string())?;
            *count += here;
        }
        None => out.write_all(body).map_err(|e| e.to_string())?,
    }
    Ok(())
}

/// Replaces in a file too big to read in, a line at a time, writing the result beside it as it goes. So it holds no
/// more than one line at a time, however big the file is. Every line without a match is copied byte for byte.
fn replace_in_big_file(path: &Path, options: &SearchOptions, backup: bool) -> Result<(usize, bool), String> {
    use std::io::{BufRead, Read, Write};
    let stamp = |meta: &std::fs::Metadata| (meta.len(), meta.modified().ok());
    let before = stamp(&std::fs::metadata(path).map_err(|e| e.to_string())?);
    let mut head = vec![0u8; 65536];
    let got = std::fs::File::open(path).and_then(|mut file| file.read(&mut head)).map_err(|e| e.to_string())?;
    head.truncate(got);
    if codec::looks_binary(&head) {
        return Err("it looks like a binary file".to_string());
    }
    let found = codec::detect(&head);
    if (found.charset.holds_characters() && found.charset.id != "utf-8") || found.charset.ebcdic {
        return Err(format!("it's in {}, which can't be changed a line at a time at this size", found.charset.name));
    }
    let format = TextFormat { charset: found.charset, ..TextFormat::default() };
    let by_bytes = format.charset.keep_bytes;
    let here = SearchOptions {
        pattern: if by_bytes { format.prepare_input(&options.pattern, format.state) } else { options.pattern.clone() },
        replacement: if by_bytes { format.prepare_input(&options.replacement, format.state) } else { options.replacement.clone() },
        whole_word: !by_bytes && options.whole_word,
        regex: !by_bytes && options.regex,
        ..options.clone()
    };
    let re = TextBuffer::build_regex(&here)?;
    let mut temp_name = path.as_os_str().to_owned();
    temp_name.push(".limoniter-tmp");
    let temp = PathBuf::from(temp_name);
    let write_all = || -> Result<usize, String> {
        let mut reader = std::io::BufReader::with_capacity(1 << 20, std::fs::File::open(path).map_err(|e| e.to_string())?);
        let file = std::fs::File::create(&temp).map_err(|e| e.to_string())?;
        let mut out = std::io::BufWriter::with_capacity(1 << 20, file);
        let mut line = Vec::new();
        let mut count = 0;
        let mut first = true;
        loop {
            line.clear();
            if reader.read_until(b'\n', &mut line).map_err(|e| e.to_string())? == 0 {
                break;
            }
            let mut rest = line.as_slice();
            if first && rest.starts_with(&[0xEF, 0xBB, 0xBF]) {
                out.write_all(&rest[..3]).map_err(|e| e.to_string())?;
                rest = &rest[3..];
            }
            first = false;
            // A lone carriage return ends a line too, as it does when the file's opened.
            while let Some(at) = memchr::memchr2(b'\n', b'\r', rest) {
                let size = if rest[at] == b'\r' && rest.get(at + 1) == Some(&b'\n') { 2 } else { 1 };
                write_replaced(&mut out, &re, &here, &format, format.charset, &rest[..at], &mut count)?;
                out.write_all(&rest[at..at + size]).map_err(|e| e.to_string())?;
                rest = &rest[at + size..];
            }
            write_replaced(&mut out, &re, &here, &format, format.charset, rest, &mut count)?;
        }
        out.flush().map_err(|e| e.to_string())?;
        out.get_ref().sync_all().map_err(|e| e.to_string())?;
        Ok(count)
    };
    let finished = write_all().and_then(|count| {
        if count == 0 {
            return Ok((0, false));
        }
        if stamp(&std::fs::metadata(path).map_err(|e| e.to_string())?) != before {
            return Err("another program changed it while it was being read".to_string());
        }
        if backup {
            std::fs::copy(path, free_backup_name(path)).map_err(|e| format!("its backup couldn't be written: {e}"))?;
        }
        std::fs::rename(&temp, path).map_err(|e| e.to_string())?;
        Ok((count, backup))
    });
    let _ = std::fs::remove_file(&temp);
    finished
}

/// Replaces every match in one file on disk, and says how many. Nothing's written unless something changed.
/// The file is read as it is now, and has to come back byte for byte before it's touched, so the only
/// differences in what's written are the replacements. The new file replaces the old only once it's safely written.
pub fn replace_in_file(path: &Path, options: &SearchOptions, backup: bool) -> Result<(usize, bool), String> {
    let meta = std::fs::metadata(path).map_err(|e| e.to_string())?;
    if meta.permissions().readonly() {
        return Err("it's marked read-only".to_string());
    }
    if meta.len() > MOST_REPLACED_BYTES {
        return replace_in_big_file(path, options, backup);
    }
    let bytes = std::fs::read(path).map_err(|e| e.to_string())?;
    if codec::looks_binary(&bytes) {
        return Err("it looks like a binary file".to_string());
    }
    let found = codec::detect(&bytes);
    let mut format = TextFormat { charset: found.charset, has_bom: found.has_bom, certainty: found.certainty, ..TextFormat::default() };
    let decoded = codec::decode(&bytes, &mut format);
    let mut buffer = TextBuffer::default();
    buffer.format = format.clone();
    buffer.set_decoded(decoded);
    if buffer.encode(&format, codec::Lossy::Refuse).ok().as_deref() != Some(&bytes[..]) {
        return Err("its encoding couldn't be kept exactly as it is".to_string());
    }
    // Where every byte stays a byte in the text, a search has to look for those bytes.
    let by_bytes = format.charset.keep_bytes;
    let here = SearchOptions {
        pattern: if by_bytes { format.prepare_input(&options.pattern, format.state) } else { options.pattern.clone() },
        replacement: if by_bytes { format.prepare_input(&options.replacement, format.state) } else { options.replacement.clone() },
        whole_word: !by_bytes && options.whole_word,
        regex: !by_bytes && options.regex,
        ..options.clone()
    };
    let count = buffer.replace_all(&here)?;
    if count == 0 {
        return Ok((0, false));
    }
    let Ok(new_bytes) = buffer.encode(&format, codec::Lossy::Refuse) else {
        return Err(format!("{} can't hold the replacement text", format.encoding_name()));
    };
    if backup {
        codec::write_file(&free_backup_name(path), &bytes).map_err(|e| format!("its backup couldn't be written: {e}"))?;
    }
    codec::write_file(path, &new_bytes).map_err(|e| e.to_string())?;
    Ok((count, backup))
}

/// Replaces in each file on another thread, adding to what the report already holds.
pub fn start_replace(files: Vec<PathBuf>, options: SearchOptions, backup: bool, report: ReplaceReport) -> Arc<ReplaceJob> {
    let job = Arc::new(ReplaceJob { total: AtomicUsize::new(files.len()), report: Mutex::new(report), ..ReplaceJob::default() });
    let worker = job.clone();
    std::thread::spawn(move || {
        for path in files {
            // Trouble with one file doesn't stop the rest.
            let attempt = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| replace_in_file(&path, &options, backup)));
            let mut report = worker.report.lock().unwrap();
            match attempt.unwrap_or_else(|_| Err("something went wrong reading it".to_string())) {
                Ok((0, _)) => {}
                Ok((count, kept)) => {
                    report.files_changed += 1;
                    report.replaced += count;
                    report.backups += usize::from(kept);
                }
                Err(reason) => report.skipped.push((path, reason)),
            }
            drop(report);
            worker.done.fetch_add(1, Ordering::Relaxed);
        }
        worker.finished.store(true, Ordering::SeqCst);
    });
    job
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn name_patterns() {
        assert!(name_matches("main.RS", "*.rs"));
        assert!(name_matches("a.txt", "*.rs; *.txt"));
        assert!(!name_matches("a.txt", "*.rs"));
        assert!(name_matches("anything", ""));
        assert!(name_matches("data1.csv", "data?.csv"));
        assert!(!name_matches("data12.csv", "data?.csv"));
        assert!(name_matches("abcabc", "*abc"));
        assert!(!name_matches("x", "*.*"));
        assert!(name_matches("x.y", "*.*"));
    }

    fn wait(job: &SearchJob) {
        while !job.finished.load(Ordering::SeqCst) {
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
    }

    #[test]
    fn searches_a_folder_and_respects_the_options() {
        let dir = std::env::temp_dir().join(format!("limoniter-findfiles-{}", std::process::id()));
        fs::create_dir_all(dir.join("sub")).unwrap();
        fs::write(dir.join("a.txt"), "first\nneedle here\n").unwrap();
        fs::write(dir.join("b.log"), "needle in a log\n").unwrap();
        fs::write(dir.join("sub").join("c.txt"), "x NEEDLE y needle\n").unwrap();
        fs::write(dir.join("d.bin"), b"needle\x00binary").unwrap();
        let options = SearchOptions { pattern: "needle".into(), ..Default::default() };

        let job = start(dir.clone(), "*.txt".into(), false, &options).unwrap();
        wait(&job);
        let hits = job.hits.lock().unwrap().clone();
        assert_eq!(hits.len(), 1);
        assert_eq!((hits[0].line, hits[0].start, hits[0].end), (1, 0, 6));

        let job = start(dir.clone(), "".into(), true, &options).unwrap();
        wait(&job);
        assert_eq!(job.hits.lock().unwrap().len(), 4, "the binary file is left out");

        let exact = SearchOptions { match_case: true, ..options.clone() };
        let job = start(dir.clone(), "c.txt".into(), true, &exact).unwrap();
        wait(&job);
        assert_eq!(job.hits.lock().unwrap().len(), 1);

        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_bad_pattern_is_reported_before_starting() {
        let options = SearchOptions { pattern: "(".into(), regex: true, ..Default::default() };
        assert!(start(std::env::temp_dir(), "".into(), false, &options).is_err());
    }

    #[test]
    fn big_files_are_searched_as_they_are_read() {
        let dir = std::env::temp_dir().join(format!("limoniter-findbig-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("big.log");
        let line = "x".repeat(1023);
        let mut data = String::with_capacity((STREAM_ABOVE + 4096) as usize);
        while (data.len() as u64) <= STREAM_ABOVE {
            data.push_str(&line);
            data.push('\n');
        }
        data.push_str("the needle\r\nafter");
        fs::write(&path, data.as_bytes()).unwrap();
        let job = SearchJob::default();
        search_file(&path, &Regex::new("needle").unwrap(), &job);
        let hits = job.hits.lock().unwrap().clone();
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].text, "the needle");
        assert_eq!((hits[0].start, hits[0].end), (4, 10));
        assert_eq!(hits[0].line as u64, STREAM_ABOVE / 1024 + 1);
        fs::remove_dir_all(&dir).unwrap();
    }

    fn replace_dir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("limoniter-replace-{tag}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn swap(pattern: &str, replacement: &str) -> SearchOptions {
        SearchOptions { pattern: pattern.into(), replacement: replacement.into(), ..Default::default() }
    }

    #[test]
    fn a_list_of_files_is_searched_and_a_missing_one_is_counted() {
        let dir = replace_dir("list");
        fs::write(dir.join("a.txt"), "needle\n").unwrap();
        let options = SearchOptions { pattern: "needle".into(), ..Default::default() };
        let job = start_list(vec![dir.join("a.txt"), dir.join("gone.txt")], &options).unwrap();
        wait(&job);
        assert_eq!(job.hits.lock().unwrap().len(), 1);
        assert_eq!(job.files_unreadable.load(Ordering::Relaxed), 1);
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn replacing_changes_only_the_matches_and_keeps_every_other_byte() {
        let dir = replace_dir("exact");
        let path = dir.join("mixed.txt");
        // Mixed line endings, a stray byte that isn't UTF-8, and no ending on the last line.
        let original: &[u8] = b"cat one\r\ncat\ttwo\nthree \xE9 cat\rlast cat";
        fs::write(&path, original).unwrap();
        assert_eq!(replace_in_file(&path, &SearchOptions { match_case: true, ..swap("cat", "dog") }, false), Ok((4, false)));
        assert_eq!(fs::read(&path).unwrap(), b"dog one\r\ndog\ttwo\nthree \xE9 dog\rlast dog");
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_byte_order_mark_and_utf_16_survive() {
        let dir = replace_dir("bom");
        let with_mark = dir.join("mark.txt");
        fs::write(&with_mark, b"\xEF\xBB\xBFcaf\xC3\xA9\r\n").unwrap();
        assert_eq!(replace_in_file(&with_mark, &swap("caf", "CAF"), false), Ok((1, false)));
        assert_eq!(fs::read(&with_mark).unwrap(), b"\xEF\xBB\xBFCAF\xC3\xA9\r\n");
        let wide = dir.join("wide.txt");
        let mut data = vec![0xFF, 0xFE];
        data.extend("hi there\r\n".encode_utf16().flat_map(u16::to_le_bytes));
        fs::write(&wide, &data).unwrap();
        assert_eq!(replace_in_file(&wide, &swap("there", "you"), false), Ok((1, false)));
        let mut wanted = vec![0xFF, 0xFE];
        wanted.extend("hi you\r\n".encode_utf16().flat_map(u16::to_le_bytes));
        assert_eq!(fs::read(&wide).unwrap(), wanted);
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn backups_hold_the_original_and_never_overwrite_an_earlier_one() {
        let dir = replace_dir("backup");
        let path = dir.join("a.txt");
        fs::write(&path, "one\n").unwrap();
        assert_eq!(replace_in_file(&path, &swap("one", "two"), true), Ok((1, true)));
        assert_eq!(fs::read(dir.join("a.txt.bak")).unwrap(), b"one\n");
        assert_eq!(replace_in_file(&path, &swap("two", "three"), true), Ok((1, true)));
        assert_eq!(fs::read(dir.join("a.txt.bak")).unwrap(), b"one\n", "the first backup stays");
        assert_eq!(fs::read(dir.join("a.txt.bak2")).unwrap(), b"two\n");
        assert_eq!(fs::read(&path).unwrap(), b"three\n");
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn with_no_match_nothing_is_written_at_all() {
        let dir = replace_dir("none");
        let path = dir.join("a.txt");
        fs::write(&path, "one\n").unwrap();
        let before = fs::metadata(&path).unwrap().modified().unwrap();
        assert_eq!(replace_in_file(&path, &swap("absent", "x"), true), Ok((0, false)));
        assert_eq!(fs::metadata(&path).unwrap().modified().unwrap(), before);
        assert!(!dir.join("a.txt.bak").exists());
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn files_that_cant_be_done_safely_are_left_alone_and_say_why() {
        let dir = replace_dir("refused");
        let binary = dir.join("b.bin");
        fs::write(&binary, b"cat\x00cat").unwrap();
        assert!(replace_in_file(&binary, &swap("cat", "dog"), false).unwrap_err().contains("binary"));
        assert_eq!(fs::read(&binary).unwrap(), b"cat\x00cat");

        let locked = dir.join("locked.txt");
        fs::write(&locked, "cat").unwrap();
        let mut permissions = fs::metadata(&locked).unwrap().permissions();
        permissions.set_readonly(true);
        fs::set_permissions(&locked, permissions.clone()).unwrap();
        assert!(replace_in_file(&locked, &swap("cat", "dog"), false).unwrap_err().contains("read-only"));
        #[allow(clippy::permissions_set_readonly_false)]
        permissions.set_readonly(false);
        fs::set_permissions(&locked, permissions).unwrap();

        assert!(replace_in_file(&dir.join("gone.txt"), &swap("cat", "dog"), false).is_err());
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn replacing_a_line_at_a_time_changes_only_the_matches() {
        let dir = replace_dir("big-way");
        let path = dir.join("log.txt");
        let original: &[u8] = b"\xEF\xBB\xBFcat one\r\nplain\ncat\ttwo\rthree \xE9 cat\r\n\n\nlast cat";
        fs::write(&path, original).unwrap();
        let options = SearchOptions { match_case: true, ..swap("cat", "dog") };
        assert_eq!(replace_in_big_file(&path, &options, false), Ok((4, false)));
        assert_eq!(fs::read(&path).unwrap(), b"\xEF\xBB\xBFdog one\r\nplain\ndog\ttwo\rthree \xE9 dog\r\n\n\nlast dog");
        assert!(!dir.join("log.txt.limoniter-tmp").exists());
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_line_at_a_time_goes_by_the_same_rules_as_opening_the_file() {
        let dir = replace_dir("big-regex");
        let path = dir.join("log.txt");
        fs::write(&path, "a=1\r\nb=2\rc=3\n").unwrap();
        let options = SearchOptions { pattern: "^(.)=(.)$".into(), replacement: "$2=$1".into(), regex: true, ..Default::default() };
        assert_eq!(replace_in_big_file(&path, &options, false), Ok((3, false)), "a lone carriage return ends a line");
        assert_eq!(fs::read(&path).unwrap(), b"1=a\r\n2=b\r3=c\n");
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_line_at_a_time_with_no_match_writes_nothing_and_a_backup_holds_the_original() {
        let dir = replace_dir("big-backup");
        let path = dir.join("log.txt");
        fs::write(&path, "cat\n").unwrap();
        let before = fs::metadata(&path).unwrap().modified().unwrap();
        assert_eq!(replace_in_big_file(&path, &swap("absent", "x"), true), Ok((0, false)));
        assert_eq!(fs::metadata(&path).unwrap().modified().unwrap(), before);
        assert!(!dir.join("log.txt.bak").exists());
        assert!(!dir.join("log.txt.limoniter-tmp").exists());
        assert_eq!(replace_in_big_file(&path, &swap("cat", "dog"), true), Ok((1, true)));
        assert_eq!(fs::read(dir.join("log.txt.bak")).unwrap(), b"cat\n");
        assert_eq!(fs::read(&path).unwrap(), b"dog\n");
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_line_at_a_time_refuses_what_it_cant_keep_exact() {
        let dir = replace_dir("big-refuse");
        let wide = dir.join("wide.txt");
        let mut data = vec![0xFF, 0xFE];
        data.extend("cat\r\n".encode_utf16().flat_map(u16::to_le_bytes));
        fs::write(&wide, &data).unwrap();
        assert!(replace_in_big_file(&wide, &swap("cat", "dog"), false).unwrap_err().contains("UTF-16"));
        assert_eq!(fs::read(&wide).unwrap(), data);
        let binary = dir.join("b.bin");
        fs::write(&binary, b"cat\x00cat").unwrap();
        assert!(replace_in_big_file(&binary, &swap("cat", "dog"), false).unwrap_err().contains("binary"));
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_job_carries_on_past_a_bad_file_and_reports_it_all() {
        let dir = replace_dir("job");
        fs::write(dir.join("a.txt"), "cat cat\n").unwrap();
        fs::write(dir.join("b.txt"), "cat\n").unwrap();
        fs::write(dir.join("c.txt"), "nothing\n").unwrap();
        let files = vec![dir.join("gone.txt"), dir.join("a.txt"), dir.join("b.txt"), dir.join("c.txt")];
        let job = start_replace(files, swap("cat", "dog"), false, ReplaceReport::default());
        while !job.finished.load(Ordering::SeqCst) {
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        let report = job.report.lock().unwrap();
        assert_eq!((report.files_changed, report.replaced, report.skipped.len()), (2, 3, 1));
        assert_eq!(job.done.load(Ordering::Relaxed), 4);
        assert_eq!(fs::read(dir.join("a.txt")).unwrap(), b"dog dog\n");
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn results_stop_at_the_limit() {
        let job = SearchJob::default();
        let re = Regex::new("x").unwrap();
        let lines = vec!["x"; MAX_HITS + 5];
        search_lines(Path::new("big"), &lines, &re, &job);
        assert_eq!(job.hits.lock().unwrap().len(), MAX_HITS);
        assert!(job.truncated.load(Ordering::Relaxed));
    }
}
