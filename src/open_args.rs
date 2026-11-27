//! What a command line says about opening files: `FILE:LINE[:COLUMN]`, `--line N`, `--new-window` and `--wait`.
//! The rest of the command line (the tools, the screenshot) is read where it always was.

use crate::instance::Opening;
use std::ffi::OsString;
use std::path::{Path, PathBuf};

/// The options for opening files, taken out of the command line.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Options {
    /// Open in a window of its own, not as tabs in the one already open.
    pub new_window: bool,
    /// Hold until the files have been closed.
    pub wait: bool,
    /// `--line N`: where to start in the files that don't say for themselves.
    pub line: Option<usize>,
}

/// Takes `--new-window`, `--wait` and `--line N` out of the arguments, giving what's left. Everything after a
/// bare `--` is a file, and the words that belong to other options are left alone.
pub fn extract(args: Vec<OsString>) -> Result<(Options, Vec<OsString>), String> {
    let mut options = Options::default();
    let mut rest = Vec::new();
    let mut args = args.into_iter();
    while let Some(arg) = args.next() {
        let word = arg.to_string_lossy().into_owned();
        let takes = match word.as_str() {
            "--" => {
                rest.push(arg);
                rest.extend(args);
                break;
            }
            "--new-window" => {
                options.new_window = true;
                continue;
            }
            "--wait" => {
                options.wait = true;
                continue;
            }
            "--line" => {
                let number = args.next().and_then(|n| n.to_string_lossy().parse::<usize>().ok()).filter(|n| *n >= 1);
                options.line = Some(number.ok_or_else(|| crate::i18n::tr("--line needs a line number, counting from 1.").to_string())?);
                continue;
            }
            "--screenshot" => 1,
            "--diff" => 2,
            "--merge" => 4,
            _ => 0,
        };
        rest.push(arg);
        for _ in 0..takes {
            rest.extend(args.next());
        }
    }
    Ok((options, rest))
}

/// A file as written on the command line, perhaps with `:LINE` or `:LINE:COLUMN` after it. A file that exists
/// under its whole name is that file, even if the name ends in `:12`; a server's address and a Windows drive
/// (`C:\\x`, or a bare `C:`) are never cut.
pub fn split_place(written: &Path, line: Option<usize>) -> Opening {
    let whole = || Opening { path: written.to_path_buf(), line, column: None };
    let Some(text) = written.to_str() else {
        return whole();
    };
    if crate::remote::is_address(text) || written.exists() {
        return whole();
    }
    let mut rest = text;
    let mut numbers = Vec::new();
    // At most a line and a column.
    while numbers.len() < 2 {
        let Some((before, after)) = rest.rsplit_once(':') else { break };
        match after.parse::<usize>() {
            Ok(number) if !after.is_empty() && after.bytes().all(|b| b.is_ascii_digit()) && !before.is_empty() => {
                numbers.push(number);
                rest = before;
            }
            _ => break,
        }
    }
    // What's left mustn't be just a drive letter.
    if numbers.is_empty() || (rest.len() == 1 && rest.as_bytes()[0].is_ascii_alphabetic()) {
        return whole();
    }
    numbers.reverse();
    Opening { path: PathBuf::from(rest), line: Some(numbers[0]).filter(|n| *n >= 1), column: numbers.get(1).copied().filter(|n| *n >= 1) }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn words(list: &[&str]) -> Vec<OsString> {
        list.iter().map(OsString::from).collect()
    }

    #[test]
    fn options_come_out_and_the_rest_stays() {
        let (options, rest) = extract(words(&["--wait", "a.txt", "--line", "7", "--new-window", "b.txt"])).unwrap();
        assert_eq!(options, Options { new_window: true, wait: true, line: Some(7) });
        assert_eq!(rest, words(&["a.txt", "b.txt"]));
        let (options, rest) = extract(words(&["--", "--wait", "x"])).unwrap();
        assert_eq!(options, Options::default());
        assert_eq!(rest, words(&["--", "--wait", "x"]), "after -- they're file names");
        let (options, rest) = extract(words(&["--screenshot", "--wait", "f"])).unwrap();
        assert!(!options.wait, "the picture's name is not an option");
        assert_eq!(rest, words(&["--screenshot", "--wait", "f"]));
        assert!(extract(words(&["--line"])).is_err());
        assert!(extract(words(&["--line", "x"])).is_err());
        assert!(extract(words(&["--line", "0"])).is_err());
    }

    #[test]
    fn a_line_and_column_after_the_name() {
        let place = split_place(Path::new("/no/such/a.rs:12"), None);
        assert_eq!((place.path, place.line, place.column), (PathBuf::from("/no/such/a.rs"), Some(12), None));
        let place = split_place(Path::new("/no/such/a.rs:12:5"), Some(99));
        assert_eq!((place.path, place.line, place.column), (PathBuf::from("/no/such/a.rs"), Some(12), Some(5)));
        let place = split_place(Path::new("/no/such/a.rs"), Some(4));
        assert_eq!((place.path, place.line), (PathBuf::from("/no/such/a.rs"), Some(4)), "--line fills in");
        let place = split_place(Path::new("/no/such/a.rs:x"), None);
        assert_eq!(place.path, PathBuf::from("/no/such/a.rs:x"));
        assert_eq!(split_place(Path::new("/no/such/a.rs:1:2:3"), None).path, PathBuf::from("/no/such/a.rs:1"), "only two numbers are taken");
    }

    #[test]
    fn drives_addresses_and_real_names_are_not_cut() {
        assert_eq!(split_place(Path::new(r"C:\x\a.txt"), None).path, PathBuf::from(r"C:\x\a.txt"));
        assert_eq!(split_place(Path::new("C:12"), None).path, PathBuf::from("C:12"), "a drive letter isn't a file");
        assert_eq!(split_place(Path::new(r"C:\x\a.txt:3"), None).line, Some(3));
        assert_eq!(split_place(Path::new("sftp://me@host:22"), None).line, None);
        let dir = std::env::temp_dir().join(format!("limoniter-args-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let real = dir.join("notes:12");
        std::fs::write(&real, "x").unwrap();
        let place = split_place(&real, None);
        assert_eq!((place.path, place.line), (real, None), "a file really named that is opened as it is");
        let _ = std::fs::remove_dir_all(dir);
    }
}
