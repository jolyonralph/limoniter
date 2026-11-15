//! Applying a unified diff, as diff -u, git diff and Limoniter's own Save as a patch write them, to a folder or
//! to one file. Each change ("hunk") is looked for where the patch says, or the nearest place its lines are,
//! and shown before anything's written. A file any of whose changes can't be found is left as it is, and one
//! that's written is backed up first.

use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};

/// One change: lines taken out and put in, with lines around them to find the place by.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Hunk {
    /// Where the patch says it goes in the old file, counting from 1.
    pub old_start: usize,
    /// Each line, marked ' ' for one kept, '-' for one taken out and '+' for one put in.
    pub lines: Vec<(char, String)>,
    /// The new file has no line ending after the last line this puts in.
    pub new_no_newline: bool,
}

impl Hunk {
    fn old_lines(&self) -> Vec<&str> {
        self.lines.iter().filter(|(mark, _)| *mark != '+').map(|(_, line)| line.as_str()).collect()
    }

    fn new_lines(&self) -> Vec<&str> {
        self.lines.iter().filter(|(mark, _)| *mark != '-').map(|(_, line)| line.as_str()).collect()
    }
}

/// The changes to one file.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct FilePatch {
    /// The file's name before, or None for a file the patch makes.
    pub old: Option<String>,
    /// Its name after, or None for a file the patch takes away.
    pub new: Option<String>,
    pub hunks: Vec<Hunk>,
    /// Only "Binary files differ", which can't be applied.
    pub binary: bool,
}

impl FilePatch {
    /// The name it goes by.
    pub fn name(&self) -> &str {
        self.new.as_deref().or(self.old.as_deref()).unwrap_or("?")
    }
}

/// A name from a --- or +++ line: without the date some tools put after a tab, or None for /dev/null.
fn file_name(text: &str) -> Option<String> {
    let name = text.split('\t').next().unwrap_or(text).trim_end();
    let name = name.strip_prefix('"').and_then(|n| n.strip_suffix('"')).unwrap_or(name);
    (name != "/dev/null" && !name.is_empty()).then(|| name.to_string())
}

/// "-12,3" or "+4" into where it starts and how many lines.
fn range(text: &str) -> Option<(usize, usize)> {
    let text = &text[1..];
    match text.split_once(',') {
        Some((start, count)) => Some((start.parse().ok()?, count.parse().ok()?)),
        None => Some((text.parse().ok()?, 1)),
    }
}

/// Reads a patch into its files and changes.
pub fn parse(text: &str) -> Result<Vec<FilePatch>, String> {
    let lines: Vec<&str> = text.split('\n').map(|line| line.strip_suffix('\r').unwrap_or(line)).collect();
    let mut files: Vec<FilePatch> = Vec::new();
    let mut i = 0;
    while i < lines.len() {
        let line = lines[i];
        if let Some(rest) = line.strip_prefix("--- ")
            && let Some(next) = lines.get(i + 1).and_then(|next| next.strip_prefix("+++ "))
        {
            files.push(FilePatch { old: file_name(rest), new: file_name(next), ..FilePatch::default() });
            i += 2;
            continue;
        }
        if line.starts_with("Binary files ") && line.ends_with(" differ") {
            let names = &line["Binary files ".len()..line.len() - " differ".len()];
            let (old, new) = names.split_once(" and ").unwrap_or((names, names));
            files.push(FilePatch { old: file_name(old), new: file_name(new), binary: true, ..FilePatch::default() });
            i += 1;
            continue;
        }
        if line.starts_with("@@ ") {
            let file = files.last_mut().ok_or("The patch has a change before any file's name (a --- and +++ line).")?;
            let mut parts = line.split_whitespace().skip(1);
            let (Some((old_start, mut old_left)), Some((_, mut new_left))) = (parts.next().and_then(range), parts.next().and_then(range)) else {
                return Err(format!("Line {} of the patch isn't a change's heading that can be read: {line}", i + 1));
            };
            let mut hunk = Hunk { old_start, ..Hunk::default() };
            i += 1;
            while (old_left > 0 || new_left > 0) && i < lines.len() {
                let body = lines[i];
                // Some editors take the space off an empty line kept the same.
                let (mark, rest) = if body.is_empty() { (' ', "") } else { (body.chars().next().unwrap_or(' '), &body[1..]) };
                match mark {
                    ' ' if old_left > 0 && new_left > 0 => {
                        old_left -= 1;
                        new_left -= 1;
                    }
                    '-' if old_left > 0 => old_left -= 1,
                    '+' if new_left > 0 => new_left -= 1,
                    '\\' => {
                        i += 1;
                        continue;
                    }
                    _ => return Err(format!("Line {} of the patch doesn't fit the change it's in: {body}", i + 1)),
                }
                hunk.lines.push((mark, rest.to_string()));
                i += 1;
            }
            if old_left > 0 || new_left > 0 {
                return Err(format!("The patch ends part way through a change to {}.", file.name()));
            }
            // "\ No newline at end of file" after the last line put in, or kept.
            while let Some(marker) = lines.get(i).filter(|line| line.starts_with('\\')) {
                let _ = marker;
                if hunk.lines.last().is_some_and(|(mark, _)| *mark != '-') {
                    hunk.new_no_newline = true;
                }
                i += 1;
            }
            file.hunks.push(hunk);
            continue;
        }
        i += 1;
    }
    if files.is_empty() {
        return Err("That isn't a unified diff: it names no files to change.".into());
    }
    Ok(files)
}

/// How one change fits the file.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Fit {
    /// Where the patch says.
    Exact,
    /// This many lines further down (or up, if negative).
    Moved(isize),
    /// Its lines aren't in the file.
    Fails,
}

/// What applying a patch would do to one file.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FileCheck {
    /// Its name as the patch has it.
    pub name: String,
    /// The file it's applied to.
    pub target: PathBuf,
    /// Its path for the backup folder.
    pub backup_name: String,
    pub fits: Vec<Fit>,
    /// Why it can't be applied at all, if it can't.
    pub problem: Option<String>,
    /// What the file becomes: its bytes, or None when the patch takes it away.
    pub result: Option<Vec<u8>>,
    /// A fingerprint of the file as it was checked, or None if it wasn't there.
    original: Option<[u8; 32]>,
}

impl FileCheck {
    /// Whether it will be written: every change found.
    pub fn ready(&self) -> bool {
        self.problem.is_none() && self.fits.iter().all(|fit| *fit != Fit::Fails)
    }

    pub fn makes(&self) -> bool {
        self.original.is_none()
    }

    pub fn deletes(&self) -> bool {
        self.result.is_none()
    }

    /// What will happen to it, in words.
    pub fn describe(&self) -> String {
        if let Some(problem) = &self.problem {
            return problem.clone();
        }
        let failed = self.fits.iter().filter(|fit| **fit == Fit::Fails).count();
        if failed > 0 {
            return format!("{failed} of {} change{} can't be found, so it will be left as it is", self.fits.len(), if self.fits.len() == 1 { "" } else { "s" });
        }
        let moved = self.fits.iter().filter(|fit| matches!(fit, Fit::Moved(_))).count();
        let what = if self.makes() { "Made" } else if self.deletes() { "Deleted" } else { "Changed" };
        let mut said = format!("{what}: {} change{}", self.fits.len(), if self.fits.len() == 1 { "" } else { "s" });
        if moved > 0 {
            said.push_str(&format!(", {moved} found a little way from where the patch says"));
        }
        said
    }
}

/// A file's text as lines without their endings, whether its lines end CR LF, and whether it ends with an ending.
fn split_text(bytes: &[u8]) -> Option<(Vec<String>, bool, bool)> {
    if crate::codec::looks_binary(bytes) {
        return None;
    }
    let text = String::from_utf8_lossy(bytes);
    let final_newline = text.ends_with('\n');
    let mut lines: Vec<String> = text.split('\n').map(str::to_string).collect();
    if final_newline || text.is_empty() {
        lines.pop();
    }
    let crlf = lines.iter().filter(|line| line.ends_with('\r')).count() * 2 > lines.len();
    for line in &mut lines {
        if line.ends_with('\r') {
            line.pop();
        }
    }
    Some((lines, crlf, final_newline))
}

fn join_text(lines: &[String], crlf: bool, final_newline: bool) -> Vec<u8> {
    let ending = if crlf { "\r\n" } else { "\n" };
    let mut text = lines.join(ending);
    if final_newline && !lines.is_empty() {
        text.push_str(ending);
    }
    text.into_bytes()
}

/// Applies the changes to a file's lines, as far as they go: the new lines and whether they end with an ending,
/// and how each change fitted.
fn apply_hunks(lines: &[String], final_newline: bool, hunks: &[Hunk]) -> (Vec<String>, bool, Vec<Fit>) {
    let mut out: Vec<String> = Vec::new();
    let mut fits = Vec::new();
    let mut cursor = 0;
    let mut offset: isize = 0;
    let mut final_newline = final_newline;
    for hunk in hunks {
        let old = hunk.old_lines();
        // With no old lines, old_start is the line it goes after.
        let expected = if old.is_empty() { hunk.old_start } else { hunk.old_start.saturating_sub(1) };
        let wanted = (expected as isize + offset).max(cursor as isize) as usize;
        let fits_at = |at: usize| at + old.len() <= lines.len() && old.iter().enumerate().all(|(n, line)| lines[at + n] == *line);
        // The nearest place its lines are, not before the last change.
        let found = (cursor..=lines.len()).filter(|&at| fits_at(at)).min_by_key(|&at| at.abs_diff(wanted));
        let Some(at) = found else {
            fits.push(Fit::Fails);
            continue;
        };
        fits.push(if at == expected { Fit::Exact } else { Fit::Moved(at as isize - expected as isize) });
        offset = at as isize - expected as isize;
        out.extend(lines[cursor..at].iter().cloned());
        out.extend(hunk.new_lines().into_iter().map(str::to_string));
        cursor = at + old.len();
        if cursor == lines.len() {
            final_newline = !hunk.new_no_newline;
        }
    }
    out.extend(lines[cursor.min(lines.len())..].iter().cloned());
    (out, final_newline, fits)
}

/// Where a file a patch names is under a folder: its name as given, or without the a/ or b/ that git puts in
/// front. A name that would lead outside the folder is refused.
fn place_in(folder: &Path, name: &str) -> Result<(PathBuf, String), String> {
    let name = name.replace('\\', "/");
    let stripped = name.strip_prefix("a/").or_else(|| name.strip_prefix("b/")).map(str::to_string);
    let candidates: Vec<String> = stripped.into_iter().chain(std::iter::once(name.clone())).collect();
    for candidate in &candidates {
        crate::diff_engine::safe_path(candidate).map_err(|_| format!("{name} would be outside the folder, so it's left alone."))?;
    }
    let at = |relative: &str| relative.split('/').fold(folder.to_path_buf(), |at, part| at.join(part));
    let chosen = candidates.iter().find(|candidate| at(candidate).exists()).unwrap_or(&candidates[0]);
    Ok((at(chosen), chosen.clone()))
}

/// Works out what applying a patch would do, without writing anything. `target` is a folder, for a patch of
/// any number of files, or a file, for a patch of one.
pub fn check(patch: &[FilePatch], target: &Path) -> Result<Vec<FileCheck>, String> {
    let to_file = !target.is_dir();
    if to_file && patch.len() != 1 {
        return Err(format!("The patch changes {} files, so choose the folder it's to be applied to, rather than one file.", patch.len()));
    }
    let mut checks = Vec::new();
    for file in patch {
        let (place, backup_name) = if to_file {
            (target.to_path_buf(), target.file_name().map_or_else(|| "file".to_string(), |name| name.to_string_lossy().into_owned()))
        } else {
            match place_in(target, file.name()) {
                Ok(found) => found,
                Err(problem) => {
                    checks.push(FileCheck { name: file.name().to_string(), target: target.to_path_buf(), backup_name: String::new(), fits: Vec::new(), problem: Some(problem), result: None, original: None });
                    continue;
                }
            }
        };
        let mut check = FileCheck { name: file.name().to_string(), target: place.clone(), backup_name, fits: Vec::new(), problem: None, result: None, original: None };
        if file.binary {
            check.problem = Some("It's a change to a binary file, which a patch doesn't hold, so it's left as it is".into());
            checks.push(check);
            continue;
        }
        let bytes = if place.is_file() { std::fs::read(&place).ok() } else { None };
        check.original = bytes.as_ref().map(|bytes| Sha256::digest(bytes).into());
        match (&file.old, bytes) {
            (None, Some(_)) if !to_file => check.problem = Some("The patch makes it, but there's already a file of that name, so it's left as it is".into()),
            (_, None) if file.old.is_some() => check.problem = Some("It isn't there to change".into()),
            (_, bytes) => {
                let Some((lines, crlf, final_newline)) = split_text(bytes.as_deref().unwrap_or_default()) else {
                    check.problem = Some("It isn't text, so it's left as it is".into());
                    checks.push(check);
                    continue;
                };
                let (out, final_newline, fits) = apply_hunks(&lines, final_newline || bytes.is_none(), &file.hunks);
                check.fits = fits;
                if file.new.is_none() {
                    if !out.is_empty() && check.ready() {
                        check.problem = Some("The patch deletes it, but it has more in it than the patch says, so it's left as it is".into());
                    }
                } else {
                    check.result = Some(join_text(&out, crlf, final_newline));
                }
            }
        }
        checks.push(check);
    }
    Ok(checks)
}

/// What applying a patch did.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ApplyReport {
    pub changed: usize,
    pub made: usize,
    pub deleted: usize,
    pub left: Vec<(String, String)>,
    pub backup_folder: Option<PathBuf>,
}

impl ApplyReport {
    pub fn describe(&self) -> String {
        let mut said = format!("The patch changed {} file{}", self.changed, if self.changed == 1 { "" } else { "s" });
        if self.made > 0 {
            said.push_str(&format!(", made {}", self.made));
        }
        if self.deleted > 0 {
            said.push_str(&format!(", deleted {}", self.deleted));
        }
        said.push('.');
        if let Some(folder) = &self.backup_folder {
            said.push_str(&format!("\n\nThe files as they were are kept in {}.", folder.display()));
        }
        if !self.left.is_empty() {
            said.push_str(&format!("\n\n{} left as {} were:", self.left.len(), if self.left.len() == 1 { "it" } else { "they" }));
            for (name, why) in self.left.iter().take(12) {
                said.push_str(&format!("\n{name}: {why}"));
            }
        }
        said
    }
}

/// Writes each file that's ready, keeping each as it was under `backup_root` first. A file that has changed since
/// it was checked, or can't be backed up, is left as it is.
pub fn apply(checks: &[FileCheck], backup_root: &Path) -> ApplyReport {
    let mut report = ApplyReport::default();
    for check in checks {
        if !check.ready() {
            report.left.push((check.name.clone(), check.describe()));
            continue;
        }
        let now: Option<[u8; 32]> = std::fs::read(&check.target).ok().map(|bytes| Sha256::digest(bytes).into());
        if now != check.original {
            report.left.push((check.name.clone(), "It has changed since the patch was checked against it.".into()));
            continue;
        }
        if check.original.is_some() {
            let kept = check.backup_name.split('/').fold(backup_root.to_path_buf(), |at, part| at.join(part));
            let backed = kept.parent().map_or(Ok(()), std::fs::create_dir_all).and_then(|()| std::fs::copy(&check.target, &kept).map(|_| ()));
            if let Err(e) = backed {
                report.left.push((check.name.clone(), format!("It couldn't be backed up first ({e}).")));
                continue;
            }
            report.backup_folder = Some(backup_root.to_path_buf());
        }
        let done = match &check.result {
            Some(bytes) => check.target.parent().map_or(Ok(()), std::fs::create_dir_all).and_then(|()| crate::codec::write_file(&check.target, bytes)),
            None => std::fs::remove_file(&check.target),
        };
        match done {
            Ok(()) if check.result.is_none() => report.deleted += 1,
            Ok(()) if check.original.is_none() => report.made += 1,
            Ok(()) => report.changed += 1,
            Err(e) => report.left.push((check.name.clone(), e.to_string())),
        }
    }
    report
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::diff_engine::{TextOptions, diff_lines, unified_diff};

    fn scratch(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("limoniter-patch-{}-{tag}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn patch_of(name: &str, old: &str, new: &str) -> String {
        let (l, r): (Vec<&str>, Vec<&str>) = (old.lines().collect(), new.lines().collect());
        let chunks = diff_lines(&l, &r, &TextOptions::default());
        unified_diff(&format!("a/{name}"), &format!("b/{name}"), &l, &r, true, true, &chunks)
    }

    #[test]
    fn a_patch_made_here_applies_back() {
        let dir = scratch("roundtrip");
        let old = "one\ntwo\nthree\nfour\nfive\nsix\nseven\neight\nnine\nten\n";
        let new = "one\nTWO\nthree\nfour\nfive\nsix\nseven\neight\nnine\nten\neleven\n";
        std::fs::create_dir_all(dir.join("src")).unwrap();
        std::fs::write(dir.join("src/n.txt"), old).unwrap();
        let patch = parse(&patch_of("src/n.txt", old, new)).unwrap();
        assert_eq!(patch.len(), 1);
        assert_eq!(patch[0].hunks.len(), 2);
        let checks = check(&patch, &dir).unwrap();
        assert!(checks[0].ready(), "{}", checks[0].describe());
        assert_eq!(checks[0].fits, [Fit::Exact, Fit::Exact]);
        assert_eq!(std::fs::read_to_string(dir.join("src/n.txt")).unwrap(), old, "checking writes nothing");
        let backups = dir.join("backups");
        let report = apply(&checks, &backups);
        assert_eq!(report.changed, 1, "{}", report.describe());
        assert_eq!(std::fs::read_to_string(dir.join("src/n.txt")).unwrap(), new);
        assert_eq!(std::fs::read_to_string(backups.join("src/n.txt")).unwrap(), old, "backed up first");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_change_moved_down_is_still_found_and_a_missing_one_fails_cleanly() {
        let dir = scratch("moved");
        let patch = "--- a/f.txt\n+++ b/f.txt\n@@ -1,3 +1,3 @@\n a\n-b\n+B\n c\n@@ -8,2 +8,2 @@\n x\n-y\n+Y\n";
        std::fs::write(dir.join("f.txt"), "new first line\nanother\na\nb\nc\n1\n2\n3\n4\nx\ny\n").unwrap();
        let parsed = parse(patch).unwrap();
        let checks = check(&parsed, &dir).unwrap();
        assert_eq!(checks[0].fits, [Fit::Moved(2), Fit::Moved(2)], "each found two lines further down than the patch says");
        assert!(checks[0].ready());
        assert_eq!(String::from_utf8(checks[0].result.clone().unwrap()).unwrap(), "new first line\nanother\na\nB\nc\n1\n2\n3\n4\nx\nY\n");
        // A file without the lines a change needs is left as it is, whatever else fits.
        std::fs::write(dir.join("f.txt"), "a\nb\nc\nnothing like it\n").unwrap();
        let checks = check(&parsed, &dir).unwrap();
        assert_eq!(checks[0].fits, [Fit::Exact, Fit::Fails]);
        assert!(!checks[0].ready());
        let report = apply(&checks, &dir.join("backups"));
        assert_eq!(report.changed, 0);
        assert_eq!(report.left.len(), 1);
        assert_eq!(std::fs::read_to_string(dir.join("f.txt")).unwrap(), "a\nb\nc\nnothing like it\n");
        assert!(!dir.join("backups").exists(), "nothing to back up when nothing's written");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn files_are_made_and_deleted_and_line_endings_kept() {
        let dir = scratch("make-delete");
        std::fs::write(dir.join("gone.txt"), "bye\n").unwrap();
        std::fs::write(dir.join("dos.txt"), "a\r\nb\r\n").unwrap();
        let patch = "diff --git a/new.txt b/new.txt\n--- /dev/null\n+++ b/new.txt\n@@ -0,0 +1,2 @@\n+hello\n+there\n\\ No newline at end of file\n--- a/gone.txt\n+++ /dev/null\n@@ -1 +0,0 @@\n-bye\n--- a/dos.txt\n+++ b/dos.txt\n@@ -1,2 +1,2 @@\n a\n-b\n+c\nBinary files a/pic.png and b/pic.png differ\n";
        let parsed = parse(patch).unwrap();
        assert_eq!(parsed.len(), 4);
        let checks = check(&parsed, &dir).unwrap();
        assert!(checks[0].makes() && checks[0].ready());
        assert!(checks[1].deletes() && checks[1].ready(), "{}", checks[1].describe());
        assert!(checks[3].problem.is_some(), "a binary change can't be applied");
        let report = apply(&checks, &dir.join("backups"));
        assert_eq!((report.made, report.deleted, report.changed, report.left.len()), (1, 1, 1, 1), "{}", report.describe());
        assert_eq!(std::fs::read_to_string(dir.join("new.txt")).unwrap(), "hello\nthere");
        assert!(!dir.join("gone.txt").exists());
        assert_eq!(std::fs::read_to_string(dir.join("backups/gone.txt")).unwrap(), "bye\n", "a deleted file is kept first");
        assert_eq!(std::fs::read_to_string(dir.join("dos.txt")).unwrap(), "a\r\nc\r\n", "CR LF stays CR LF");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn one_file_and_bad_patches() {
        let dir = scratch("one");
        let file = dir.join("whatever-name.txt");
        std::fs::write(&file, "a\nb\n").unwrap();
        let parsed = parse("--- old.txt\t2024-01-01\n+++ new.txt\t2024-01-02\n@@ -1,2 +1,2 @@\n a\n-b\n+c\n").unwrap();
        assert_eq!(parsed[0].old.as_deref(), Some("old.txt"), "the date after the tab isn't part of the name");
        let checks = check(&parsed, &file).unwrap();
        assert!(checks[0].ready());
        apply(&checks, &dir.join("backups"));
        assert_eq!(std::fs::read_to_string(&file).unwrap(), "a\nc\n");
        // Two files can't go into one.
        let two = parse("--- a\n+++ a\n@@ -1 +1 @@\n-x\n+y\n--- b\n+++ b\n@@ -1 +1 @@\n-x\n+y\n").unwrap();
        assert!(check(&two, &file).is_err());
        assert!(parse("just some text").is_err());
        assert!(parse("--- a\n+++ a\n@@ -1,3 +1,3 @@\n a\n").is_err(), "a change cut short");
        // A name leading out of the folder is refused.
        let out = parse("--- a/../../etc/x\n+++ b/../../etc/x\n@@ -1 +1 @@\n-x\n+y\n").unwrap();
        let checks = check(&out, &dir).unwrap();
        assert!(checks[0].problem.as_deref().is_some_and(|p| p.contains("outside")));
        // A file changed after it was checked is left alone.
        std::fs::write(&file, "a\nb\n").unwrap();
        let checks = check(&parsed, &file).unwrap();
        assert!(checks[0].ready());
        std::fs::write(&file, "a\nb\nmore\n").unwrap();
        let report = apply(&checks, &dir.join("backups"));
        assert_eq!(report.left.len(), 1);
        assert!(report.left[0].1.contains("changed since"));
        assert_eq!(std::fs::read_to_string(&file).unwrap(), "a\nb\nmore\n");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
