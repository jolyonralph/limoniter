//! Spell checking with Windows' own spell checker, in whatever language Windows is set to. Nothing has to be shipped
//! or licensed: the dictionaries are Windows' own, and a word added goes into the person's Windows dictionary.
//!
//! The checker lives on a thread of its own, and lines are sent to it, so the window never waits. What it finds is
//! kept by each line's text, so a line is only checked again when it changes.

use std::cell::{Cell, RefCell};
use std::collections::hash_map::DefaultHasher;
use std::collections::{HashMap, HashSet};
use std::hash::{Hash, Hasher};
use std::sync::mpsc::{self, Receiver, Sender};

/// Where a mistake is in a line, by character.
pub type Mark = (usize, usize);

/// The most lines a search for the next mistake looks through.
pub const MOST_LINES_SEARCHED: usize = 20_000;
const MOST_REMEMBERED: usize = 50_000;

pub enum Ask {
    Check(Vec<(u64, String)>),
    /// The first mistake in these lines, each with its line number, and suggestions for it.
    FindNext(Vec<(usize, String)>),
    Ignore(String),
    Add(String),
}

pub enum Told {
    Checked(Vec<(u64, Vec<Mark>)>),
    Found(Option<Found>),
    Unavailable(String),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Found {
    pub line: usize,
    pub start: usize,
    pub end: usize,
    pub word: String,
    pub suggestions: Vec<String>,
}

pub struct Speller {
    asks: Sender<Ask>,
    pub told: Receiver<Told>,
}

impl Speller {
    pub fn start(wake: impl Fn() + Send + 'static) -> Option<Self> {
        let (asks, from_window) = mpsc::channel();
        let (to_window, told) = mpsc::channel();
        std::thread::Builder::new().name("limoniter-spelling".into()).spawn(move || work(&from_window, &to_window, &wake)).ok()?;
        Some(Self { asks, told })
    }

    pub fn ask(&self, ask: Ask) {
        let _ = self.asks.send(ask);
    }
}

#[cfg(windows)]
fn work(asks: &Receiver<Ask>, told: &Sender<Told>, wake: &dyn Fn()) {
    let checker = match windows_checker::Checker::new() {
        Ok(checker) => checker,
        Err(problem) => {
            let _ = told.send(Told::Unavailable(problem));
            wake();
            return;
        }
    };
    while let Ok(ask) = asks.recv() {
        let reply = match ask {
            Ask::Check(lines) => Told::Checked(lines.into_iter().map(|(key, text)| (key, checker.mistakes(&text))).collect()),
            Ask::FindNext(lines) => Told::Found(lines.into_iter().find_map(|(line, text)| {
                let (start, end) = *checker.mistakes(&text).first()?;
                let word: String = text.chars().skip(start).take(end - start).collect();
                let suggestions = checker.suggestions(&word);
                Some(Found { line, start, end, word, suggestions })
            })),
            Ask::Ignore(word) => {
                checker.ignore(&word);
                continue;
            }
            Ask::Add(word) => {
                checker.add(&word);
                continue;
            }
        };
        if told.send(reply).is_err() {
            return;
        }
        wake();
    }
}

/// On macOS and Linux, hunspell or aspell checks the words, whichever is installed, through the pipe both share.
#[cfg(not(windows))]
fn work(asks: &Receiver<Ask>, told: &Sender<Told>, wake: &dyn Fn()) {
    let mut checker = match pipe_checker::Checker::new() {
        Ok(checker) => checker,
        Err(problem) => {
            let _ = told.send(Told::Unavailable(problem));
            wake();
            return;
        }
    };
    while let Ok(ask) = asks.recv() {
        let reply = match ask {
            Ask::Check(lines) => Told::Checked(lines.into_iter().map(|(key, text)| (key, checker.misses(&text).into_iter().map(|(mark, _)| mark).collect())).collect()),
            Ask::FindNext(lines) => Told::Found(lines.into_iter().find_map(|(line, text)| {
                let ((start, end), suggestions) = checker.misses(&text).into_iter().next()?;
                let word: String = text.chars().skip(start).take(end - start).collect();
                Some(Found { line, start, end, word, suggestions })
            })),
            Ask::Ignore(word) => {
                checker.ignore(&word);
                continue;
            }
            Ask::Add(word) => {
                checker.add(&word);
                continue;
            }
        };
        if told.send(reply).is_err() {
            return;
        }
        wake();
    }
}

/// The ispell pipe that hunspell and aspell both speak: a line in, and back a line for each word it doesn't know,
/// with suggestions, then an empty line.
#[cfg_attr(windows, allow(dead_code))]
mod pipe_checker {
    use super::Mark;
    use std::io::{BufRead, BufReader, Write};
    use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};

    pub struct Checker {
        _child: Child,
        input: ChildStdin,
        output: BufReader<ChildStdout>,
    }

    impl Checker {
        pub fn new() -> Result<Self, String> {
            let tries: [(&str, &[&str]); 2] = [("hunspell", &["-a", "-i", "utf-8"]), ("aspell", &["-a", "--encoding=utf-8"])];
            for (program, args) in tries {
                let Ok(mut child) = Command::new(program).args(args).stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::null()).spawn() else {
                    continue;
                };
                let (Some(mut input), Some(output)) = (child.stdin.take(), child.stdout.take()) else {
                    continue;
                };
                let mut output = BufReader::new(output);
                let mut banner = String::new();
                if output.read_line(&mut banner).is_err() || !banner.starts_with("@(#)") {
                    let _ = child.kill();
                    continue;
                }
                // Terse: only the words it doesn't know are answered.
                if input.write_all(b"!\n").and_then(|()| input.flush()).is_err() {
                    continue;
                }
                return Ok(Self { _child: child, input, output });
            }
            Err("Spell checking here uses hunspell or aspell, and neither is installed. Install either one, with an English dictionary, and start Limoniter again.".into())
        }

        fn send(&mut self, line: &str) -> std::io::Result<()> {
            self.input.write_all(line.as_bytes())?;
            self.input.write_all(b"\n")?;
            self.input.flush()
        }

        /// Each word it doesn't know in a line, by character, with what it suggests.
        pub fn misses(&mut self, text: &str) -> Vec<(Mark, Vec<String>)> {
            // ^ keeps a line that starts with one of the pipe's own commands from being taken as one.
            let line: String = text.chars().map(|c| if c == '\n' || c == '\r' { ' ' } else { c }).collect();
            if self.send(&format!("^{line}")).is_err() {
                return Vec::new();
            }
            let chars: Vec<char> = line.chars().collect();
            let mut found = Vec::new();
            let mut from = 0;
            let mut reply = String::new();
            loop {
                reply.clear();
                if self.output.read_line(&mut reply).unwrap_or(0) == 0 {
                    break;
                }
                let reply = reply.trim_end_matches(['\r', '\n']);
                if reply.is_empty() {
                    break;
                }
                let (head, suggestions) = reply.split_once(": ").map_or((reply, Vec::new()), |(head, list)| (head, list.split(", ").map(str::to_string).collect()));
                let mut parts = head.split(' ');
                let (Some(kind), Some(word)) = (parts.next(), parts.next()) else {
                    continue;
                };
                if kind != "&" && kind != "#" {
                    continue;
                }
                let word: Vec<char> = word.chars().collect();
                let said = parts.next_back().and_then(|n| n.parse::<usize>().ok()).map(|n| n.saturating_sub(1));
                // The place it gives is in characters for aspell and may be in bytes for hunspell, so it's checked
                // against the line, and the word looked for if it doesn't fit.
                let fits = |at: usize| chars.get(at..at + word.len()).is_some_and(|here| here == word.as_slice());
                let at = said.filter(|at| fits(*at)).or_else(|| (from..chars.len()).find(|at| fits(*at)));
                if let Some(at) = at {
                    found.push(((at, at + word.len()), suggestions));
                    from = at + word.len();
                }
            }
            found
        }

        pub fn ignore(&mut self, word: &str) {
            let _ = self.send(&format!("@{word}"));
        }

        /// Adds a word to the person's own dictionary, and saves it.
        pub fn add(&mut self, word: &str) {
            let _ = self.send(&format!("*{word}"));
            let _ = self.send("#");
        }
    }

    #[cfg(test)]
    mod tests {
        #[test]
        #[ignore = "needs hunspell or aspell"]
        fn the_pipe_finds_mistakes_by_character() {
            let mut checker = super::Checker::new().unwrap();
            let misses = checker.misses("Helo wrold, café teh end");
            let marks: Vec<_> = misses.iter().map(|(mark, _)| *mark).collect();
            assert!(marks.contains(&(0, 4)) && marks.contains(&(5, 10)) && marks.contains(&(17, 20)), "{marks:?}");
            assert!(misses[0].1.iter().any(|s| s == "Hello"));
            checker.ignore("wrold");
            assert!(!checker.misses("wrold").iter().any(|(mark, _)| *mark == (0, 5)));
        }
    }
}

#[cfg(windows)]
mod windows_checker {
    use super::Mark;
    use windows::Win32::Globalization::{GetUserDefaultLocaleName, ISpellChecker, ISpellCheckerFactory, SpellCheckerFactory};
    use windows::Win32::System::Com::{CLSCTX_INPROC_SERVER, COINIT_MULTITHREADED, CoCreateInstance, CoInitializeEx, CoTaskMemFree};
    use windows::core::{HSTRING, PWSTR};

    pub struct Checker {
        inner: ISpellChecker,
    }

    impl Checker {
        pub fn new() -> Result<Self, String> {
            // SAFETY: this thread is the checker's own, and it's set up for COM once, before anything else uses it.
            unsafe {
                let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
                let factory: ISpellCheckerFactory = CoCreateInstance(&SpellCheckerFactory, None, CLSCTX_INPROC_SERVER).map_err(|e| format!("Windows' spell checker isn't available: {e}"))?;
                let mut name = [0u16; 85];
                let length = GetUserDefaultLocaleName(&mut name);
                let mut wanted = vec![String::from_utf16_lossy(&name[..(length.max(1) as usize - 1)])];
                wanted.push("en-US".into());
                for language in wanted {
                    let tag = HSTRING::from(language.as_str());
                    if factory.IsSupported(&tag).is_ok_and(|yes| yes.as_bool())
                        && let Ok(inner) = factory.CreateSpellChecker(&tag)
                    {
                        return Ok(Self { inner });
                    }
                }
                Err("Windows has no spelling dictionary for its language. One can be added in Windows Settings, under Time and language.".into())
            }
        }

        /// Each mistake, by character, where Windows counts in 16-bit units.
        pub fn mistakes(&self, text: &str) -> Vec<Mark> {
            if text.trim().is_empty() {
                return Vec::new();
            }
            let mut unit_to_char = Vec::with_capacity(text.len() + 1);
            for (index, c) in text.chars().enumerate() {
                for _ in 0..c.len_utf16() {
                    unit_to_char.push(index);
                }
            }
            unit_to_char.push(text.chars().count());
            let mut found = Vec::new();
            // SAFETY: the checker belongs to this thread, and every pointer handed back is used and let go of here.
            unsafe {
                let Ok(errors) = self.inner.Check(&HSTRING::from(text)) else {
                    return found;
                };
                loop {
                    let mut error = None;
                    if errors.Next(&mut error).is_err() {
                        break;
                    }
                    let Some(error) = error else {
                        break;
                    };
                    let (Ok(start), Ok(length)) = (error.StartIndex(), error.Length()) else {
                        continue;
                    };
                    let (start, end) = (start as usize, (start + length) as usize);
                    if let (Some(a), Some(z)) = (unit_to_char.get(start), unit_to_char.get(end)) {
                        found.push((*a, *z));
                    }
                }
            }
            found
        }

        pub fn suggestions(&self, word: &str) -> Vec<String> {
            let mut out = Vec::new();
            // SAFETY: as above; each string Windows hands over is copied and then freed.
            unsafe {
                let Ok(list) = self.inner.Suggest(&HSTRING::from(word)) else {
                    return out;
                };
                for _ in 0..8 {
                    let mut item = [PWSTR::null()];
                    let mut fetched = 0u32;
                    if list.Next(&mut item, Some(&raw mut fetched)).is_err() || fetched == 0 {
                        break;
                    }
                    out.push(item[0].to_string().unwrap_or_default());
                    CoTaskMemFree(Some(item[0].0.cast()));
                }
            }
            out
        }

        pub fn ignore(&self, word: &str) {
            // SAFETY: as above.
            unsafe {
                let _ = self.inner.Ignore(&HSTRING::from(word));
            }
        }

        pub fn add(&self, word: &str) {
            // SAFETY: as above.
            unsafe {
                let _ = self.inner.Add(&HSTRING::from(word));
            }
        }
    }
}

// ---------- What's been found, for drawing ----------

thread_local! {
    static ON: Cell<bool> = const { Cell::new(false) };
    static MARKS: RefCell<HashMap<u64, Vec<Mark>>> = RefCell::new(HashMap::new());
    static WANTED: RefCell<(Vec<(u64, String)>, HashSet<u64>)> = RefCell::new((Vec::new(), HashSet::new()));
}

pub fn set_on(on: bool) {
    ON.set(on);
}

pub fn is_on() -> bool {
    ON.get()
}

fn key_of(text: &str) -> u64 {
    let mut hasher = DefaultHasher::new();
    text.hash(&mut hasher);
    hasher.finish()
}

/// The mistakes in a line, if they're known. If they aren't, the line is asked about.
pub fn marks_for(text: &str) -> Option<Vec<Mark>> {
    let key = key_of(text);
    if let Some(found) = MARKS.with_borrow(|marks| marks.get(&key).cloned()) {
        return Some(found);
    }
    WANTED.with_borrow_mut(|(list, seen)| {
        if seen.insert(key) {
            list.push((key, text.to_string()));
        }
    });
    None
}

/// The lines waiting to be checked.
pub fn take_wanted() -> Vec<(u64, String)> {
    WANTED.with_borrow_mut(|(list, _)| std::mem::take(list))
}

pub fn remember(results: Vec<(u64, Vec<Mark>)>) {
    MARKS.with_borrow_mut(|marks| {
        if marks.len() > MOST_REMEMBERED {
            marks.clear();
        }
        marks.extend(results);
    });
    WANTED.with_borrow_mut(|(_, seen)| seen.clear());
}

/// After a word is added or ignored, everything is checked again.
pub fn forget_all() {
    MARKS.with_borrow_mut(HashMap::clear);
    WANTED.with_borrow_mut(|(list, seen)| {
        list.clear();
        seen.clear();
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn lines_are_asked_about_once_and_then_remembered() {
        forget_all();
        assert_eq!(marks_for("teh cat"), None);
        assert_eq!(marks_for("teh cat"), None, "asked again before an answer comes");
        let wanted = take_wanted();
        assert_eq!(wanted.len(), 1, "only asked about once");
        remember(vec![(wanted[0].0, vec![(0, 3)])]);
        assert_eq!(marks_for("teh cat"), Some(vec![(0, 3)]));
        forget_all();
        assert_eq!(marks_for("teh cat"), None);
    }

    /// The system's checker itself, on this computer: Windows' own, or hunspell or aspell. It only reads, and adds
    /// nothing to any dictionary.
    #[test]
    fn the_checker_finds_a_mistake_and_suggests_a_word() {
        let Some(speller) = Speller::start(|| {}) else {
            return;
        };
        speller.ask(Ask::FindNext(vec![(0, "All fine here.".into()), (1, "This has a mistaek in it.".into())]));
        match speller.told.recv_timeout(Duration::from_secs(30)).unwrap() {
            Told::Found(Some(found)) => {
                assert_eq!((found.line, found.word.as_str()), (1, "mistaek"));
                assert_eq!((found.start, found.end), (11, 18));
                assert!(found.suggestions.iter().any(|word| word == "mistake"), "{:?}", found.suggestions);
            }
            Told::Unavailable(problem) => eprintln!("spell checking isn't set up on this computer: {problem}"),
            _ => panic!("a mistake should have been found"),
        }
        speller.ask(Ask::Check(vec![(7, "\u{1F600} teh".into())]));
        if let Ok(Told::Checked(results)) = speller.told.recv_timeout(Duration::from_secs(30)) {
            assert_eq!(results[0].1, vec![(2, 5)], "counted in characters, past the emoji");
        }
    }
}
