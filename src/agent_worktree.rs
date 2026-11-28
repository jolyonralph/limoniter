//! A session's copy of the project, made with `git worktree`: its own folder on its own branch, so that an agent can
//! work while the person (or another agent) carries on in the project. This part does the Git and the merging and
//! draws nothing; the windows are in `agent_worktree_host.rs`.
//!
//! The copy is made *beside* the project (`<project>.limoniter/<name>`), not under `.git`: Claude Code and others
//! keep their hands off anything inside `.git`, and a folder in the project would turn up in its Explorer and searches.

use crate::git;
use crate::merge3::{Kind, Merge};
use std::path::{Path, PathBuf};

/// A copy made for a session.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Worktree {
    /// The project's top folder, where the copy is merged back to.
    pub project: PathBuf,
    /// The copy's top folder.
    pub path: PathBuf,
    pub branch: String,
    /// The commit the copy started from: the original both sides are merged from.
    pub base: String,
    /// The folder the session works in: the copy's top, or the same subfolder of it as the project's.
    pub folder: PathBuf,
}

/// Whether a session may be given a copy of its own, and if not why.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Offer {
    /// It may.
    Yes,
    /// The folder isn't in a Git repository (or Git isn't here).
    NotGit,
    /// The repository has no commit yet, so there's nothing to copy.
    NoCommit,
    /// The project has this many changes not yet committed, and the person hasn't said what to do with them.
    Dirty(usize),
    /// A merge or rebase is half done.
    Underway,
}

/// What to do about changes not yet committed in the project.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Carry {
    /// Not chosen yet.
    #[default]
    Ask,
    /// The copy starts with the same changes.
    Carry,
    /// The copy starts from the last commit.
    LastCommit,
}

/// Whether `folder` can have a copy made, and how many changes are in the way.
pub fn offer(folder: &Path) -> Offer {
    if git::program().is_none() {
        return Offer::NotGit;
    }
    let Some(root) = git::repository_of(folder) else { return Offer::NotGit };
    match git::status(&root) {
        Err(_) => Offer::NotGit,
        Ok(status) if !status.born => Offer::NoCommit,
        Ok(status) if status.underway.is_some() => Offer::Underway,
        Ok(status) if status.entries.is_empty() => Offer::Yes,
        Ok(status) => Offer::Dirty(status.entries.len()),
    }
}

/// The reason a copy isn't offered, in words for the person.
pub fn why_not(offer: &Offer) -> Option<String> {
    match offer {
        Offer::Yes => None,
        Offer::NotGit => Some(crate::i18n::tr("A separate copy needs the project to be in a Git repository.").to_string()),
        Offer::NoCommit => Some(crate::i18n::tr("The project has no commit yet, so there is nothing to copy. Make a first commit, then try again.").to_string()),
        Offer::Underway => Some(crate::i18n::tr("A merge or rebase is half done in the project. Finish or abort it first.").to_string()),
        Offer::Dirty(_) => None,
    }
}

/// A name for a folder and branch from a title: lower-case letters, digits and single dashes.
pub fn slug(title: &str) -> String {
    let mut out = String::new();
    for c in title.chars() {
        if c.is_ascii_alphanumeric() {
            out.push(c.to_ascii_lowercase());
        } else if !out.ends_with('-') && !out.is_empty() {
            out.push('-');
        }
        if out.len() >= 40 {
            break;
        }
    }
    let out = out.trim_matches('-').to_string();
    if out.is_empty() { "session".to_string() } else { out }
}

/// Makes a copy of the repository `folder` is in, on a new branch `limoniter/<name>`, started from the last commit,
/// with the project's uncommitted changes copied over if `carry` says so.
pub fn create(folder: &Path, name: &str, carry: Carry) -> Result<Worktree, String> {
    let project = git::repository_of(folder).ok_or_else(|| crate::i18n::tr("A separate copy needs the project to be in a Git repository.").to_string())?;
    let project = crate::platform::canonical(&project).unwrap_or(project);
    let status = git::status(&project)?;
    if !status.born {
        return Err(crate::i18n::tr("The project has no commit yet, so there is nothing to copy. Make a first commit, then try again.").to_string());
    }
    if !status.entries.is_empty() && carry == Carry::Ask {
        return Err(crate::i18n::tr("Choose what to do with the changes not yet committed.").to_string());
    }
    let base = git::run(&project, &["rev-parse", "HEAD"])?.trim().to_string();
    let slug = slug(name);
    let home = project.parent().ok_or_else(|| crate::i18n::tr("The project has no folder beside it to put a copy in.").to_string())?.join(format!("{}.limoniter", project.file_name().map_or_else(|| "project".to_string(), |n| n.to_string_lossy().into_owned())));
    std::fs::create_dir_all(&home).map_err(|e| crate::i18n::tf!("Couldn't make the folder {} for the copy: {e}", home.display()))?;
    // A name not used by a branch or folder already.
    let branches: Vec<String> = git::branches(&project)?.into_iter().map(|b| b.name).collect();
    let mut n = 1;
    let (leaf, branch) = loop {
        let leaf = if n == 1 { slug.clone() } else { format!("{slug}-{n}") };
        let branch = format!("limoniter/{leaf}");
        if !home.join(&leaf).exists() && !branches.contains(&branch) {
            break (leaf, branch);
        }
        n += 1;
    };
    let path = home.join(&leaf);
    git::run(&project, &["worktree", "add", "-b", &branch, &path.display().to_string(), &base])?;
    let path = crate::platform::canonical(&path).unwrap_or(path);
    if carry == Carry::Carry {
        for entry in &status.entries {
            for file in std::iter::once(&entry.path).chain(entry.from.iter()) {
                let (from, to) = (project.join(file), path.join(file));
                if from.is_file() {
                    if let Some(parent) = to.parent() {
                        let _ = std::fs::create_dir_all(parent);
                    }
                    std::fs::copy(&from, &to).map_err(|e| crate::i18n::tf!("Couldn't copy {} into the new copy: {e}", file))?;
                } else if !from.exists() {
                    let _ = std::fs::remove_file(&to);
                }
            }
        }
    }
    let inside = git::relative(&project, folder).filter(|r| !r.is_empty());
    let folder = inside.map(|r| path.join(r)).filter(|p| p.is_dir()).unwrap_or_else(|| path.clone());
    Ok(Worktree { project, path, branch, base, folder })
}

/// What happened to a file in the copy since it started.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Happened {
    New,
    Changed,
    Deleted,
}

impl Happened {
    pub fn word(self) -> &'static str {
        match self {
            Happened::New => "new",
            Happened::Changed => "changed",
            Happened::Deleted => "deleted",
        }
    }
}

/// A file the copy has changed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Changed {
    pub path: String,
    pub what: Happened,
}

/// The files the copy changed since it started, whether or not they were committed. (Everything is staged first,
/// which only touches the copy's own index.)
pub fn changed(wt: &Worktree) -> Result<Vec<Changed>, String> {
    git::run(&wt.path, &["add", "-A"])?;
    let said = git::run(&wt.path, &["diff", "--cached", "--name-status", "--no-renames", "-z", &wt.base])?;
    let mut fields = said.split('\0').filter(|f| !f.is_empty());
    let mut found = Vec::new();
    while let (Some(letter), Some(path)) = (fields.next(), fields.next()) {
        let what = match letter.chars().next() {
            Some('A') => Happened::New,
            Some('D') => Happened::Deleted,
            _ => Happened::Changed,
        };
        found.push(Changed { path: path.to_string(), what });
    }
    found.sort_by(|a, b| a.path.cmp(&b.path));
    Ok(found)
}

/// The copy's changes as one unified diff, from where it started.
pub fn diff_text(wt: &Worktree) -> Result<String, String> {
    git::run(&wt.path, &["add", "-A"])?;
    git::run(&wt.path, &["diff", "--cached", "--no-renames", &wt.base])
}

/// The changes of a folder that has no copy of its own, as a diff from the last commit (for a review).
pub fn diff_of_project(folder: &Path) -> Result<String, String> {
    let root = git::repository_of(folder).ok_or_else(|| crate::i18n::tr("A separate copy needs the project to be in a Git repository.").to_string())?;
    // New files are named too, so a review sees them.
    git::run(&root, &["add", "-A", "-N"])?;
    git::run(&root, &["diff", "HEAD", "--no-renames"])
}

/// What merging one file back does.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Plan {
    /// The project already has what the copy has, or the copy didn't change it.
    Nothing,
    /// The project's file hasn't changed since the copy started, so the copy's is taken as it is.
    Write(Vec<u8>),
    /// The same, for a file the copy deleted.
    Remove,
    /// Both changed it and the changes fit together.
    Merged(String),
    /// Both changed it, in the same place: for the merge view.
    Conflict { base: String, mine: String, theirs: String },
    /// Both changed a file that isn't text, so there is no telling how to put them together.
    Unmergeable,
}

fn text_of(bytes: Option<&[u8]>) -> Option<String> {
    match bytes {
        None => Some(String::new()),
        Some(bytes) if crate::codec::looks_binary(bytes) => None,
        Some(bytes) => String::from_utf8(bytes.to_vec()).ok(),
    }
}

/// Works out what merging a file back does, from the file as the copy started (`base`), as the project has it now
/// (`mine`) and as the copy has it (`theirs`). None means the file isn't there.
pub fn plan(base: Option<&[u8]>, mine: Option<&[u8]>, theirs: Option<&[u8]>) -> Plan {
    if theirs == base || theirs == mine {
        return Plan::Nothing;
    }
    if mine == base {
        return theirs.map_or(Plan::Remove, |t| Plan::Write(t.to_vec()));
    }
    let (Some(b), Some(m), Some(t)) = (text_of(base), text_of(mine), text_of(theirs)) else { return Plan::Unmergeable };
    // One side deleted a file the other changed: there is nothing to line up, so the person chooses.
    if mine.is_none() || theirs.is_none() {
        return Plan::Conflict { base: b, mine: m, theirs: t };
    }
    let merge = Merge::of(&b, &m, &t);
    if merge.chunks.iter().any(|c| c.kind == Kind::Conflict) { Plan::Conflict { base: b, mine: m, theirs: t } } else { Plan::Merged(merge.text(&crate::merge3::Labels::default())) }
}

/// A file that needs the person in the merge view.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Conflicted {
    pub path: String,
    pub base: String,
    pub mine: String,
    pub theirs: String,
}

/// What merging the copy back did.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Report {
    /// Files written as the copy has them.
    pub taken: Vec<String>,
    /// Files both changed, which fitted together.
    pub merged: Vec<String>,
    /// Files the project had already.
    pub same: Vec<String>,
    /// Files both changed in the same place.
    pub conflicts: Vec<Conflicted>,
    /// Files both changed that aren't text, and were left as the project has them.
    pub left: Vec<String>,
}

/// Reads a file of the project or the copy: None if it isn't there.
fn read(path: &Path) -> Option<Vec<u8>> {
    std::fs::read(path).ok()
}

/// What each file would do, without doing it.
pub fn preview(wt: &Worktree) -> Result<Vec<(Changed, Plan)>, String> {
    let mut found = Vec::new();
    for change in changed(wt)? {
        let base = git::file_at(&wt.project, &wt.base, &change.path);
        let plan = plan(base.as_deref(), read(&wt.project.join(&change.path)).as_deref(), read(&wt.path.join(&change.path)).as_deref());
        found.push((change, plan));
    }
    Ok(found)
}

/// Puts the copy's changes into the project's working tree: files the project hasn't touched are taken, files both
/// changed are merged three ways (the copy's start is the original), and the ones that clash are handed back to be
/// settled in the merge view. Nothing is committed.
pub fn merge_back(wt: &Worktree) -> Result<Report, String> {
    let mut report = Report::default();
    for (change, plan) in preview(wt)? {
        let target = wt.project.join(&change.path);
        let write = |bytes: &[u8]| -> Result<(), String> {
            if let Some(parent) = target.parent() {
                let _ = std::fs::create_dir_all(parent);
            }
            std::fs::write(&target, bytes).map_err(|e| crate::i18n::tf!("Couldn't write {}: {e}", target.display()))
        };
        match plan {
            Plan::Nothing => report.same.push(change.path),
            Plan::Write(bytes) => {
                write(&bytes)?;
                report.taken.push(change.path);
            }
            Plan::Remove => {
                std::fs::remove_file(&target).map_err(|e| crate::i18n::tf!("Couldn't delete {}: {e}", target.display()))?;
                report.taken.push(change.path);
            }
            Plan::Merged(text) => {
                write(text.as_bytes())?;
                report.merged.push(change.path);
            }
            Plan::Conflict { base, mine, theirs } => report.conflicts.push(Conflicted { path: change.path, base, mine, theirs }),
            Plan::Unmergeable => report.left.push(change.path),
        }
    }
    Ok(report)
}

/// Commits everything in the copy on its branch.
pub fn commit(wt: &Worktree, message: &str) -> Result<(), String> {
    git::run(&wt.path, &["add", "-A"])?;
    if git::status(&wt.path)?.entries.is_empty() {
        return Ok(());
    }
    git::commit(&wt.path, message, false).map(|_| ())
}

/// Removes the copy's folder, and its branch if asked. Anything not committed in it is lost.
pub fn remove(wt: &Worktree, delete_branch: bool) -> Result<(), String> {
    git::run(&wt.project, &["worktree", "remove", "--force", &wt.path.display().to_string()])?;
    if delete_branch {
        git::run(&wt.project, &["branch", "-D", &wt.branch])?;
    }
    // The folder beside the project goes when nothing else is in it.
    if let Some(home) = wt.path.parent() {
        let _ = std::fs::remove_dir(home);
    }
    Ok(())
}

// ---------- Asking the other tool to review ----------

/// What the reviewer is asked, unless the person has said otherwise.
pub fn default_review_prompt() -> String {
    crate::i18n::tr("Please review the changes below, which another AI assistant made. Look for mistakes, missed cases and anything unclear. Don't change any files. Write each comment on a line of its own, as path:line: what you found, and finish with one short sentence on whether the changes are ready.").to_string()
}

/// The most of a diff that is handed over; past it the rest is left out and the reviewer is told so.
const MOST_DIFF: usize = 120_000;

/// The message a reviewer is sent: the prompt, then the diff.
pub fn review_message(prompt: &str, diff: &str) -> String {
    let mut shown = diff.to_string();
    let mut note = String::new();
    if shown.len() > MOST_DIFF {
        let mut cut = MOST_DIFF;
        while !shown.is_char_boundary(cut) {
            cut -= 1;
        }
        shown.truncate(cut);
        note = format!("\n\n{}", crate::i18n::tr("(The diff was too long to hand over whole, so the end of it is left out.)"));
    }
    format!("{}\n\n```diff\n{}\n```{note}", prompt.trim(), shown.trim_end())
}

/// One comment of a review, naming a place.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Comment {
    pub path: String,
    pub line: usize,
    pub text: String,
}

/// The comments in a review's text: lines that begin (after a bullet, number or quote marks) with `path:line`.
pub fn parse_comments(review: &str) -> Vec<Comment> {
    review.lines().filter_map(parse_comment).collect()
}

fn parse_comment(line: &str) -> Option<Comment> {
    let mut rest = line.trim();
    // A bullet or a number, and the marks around a name.
    rest = rest.trim_start_matches(['-', '*', '>', ' ']);
    let digits = rest.chars().take_while(char::is_ascii_digit).count();
    if digits > 0 && rest[digits..].starts_with(". ") {
        rest = &rest[digits + 2..];
    }
    let wrapped = rest.starts_with(['*', '`']);
    let rest = rest.trim_start_matches(['*', '`', ' ']);
    let end = rest.find(':')?;
    let path = &rest[..end];
    // Something like a file: no spaces, and a dot or a slash.
    if path.is_empty() || path.contains(char::is_whitespace) || !(path.contains('.') || path.contains('/')) {
        return None;
    }
    let after = &rest[end + 1..];
    let digits = after.chars().take_while(char::is_ascii_digit).count();
    if digits == 0 {
        return None;
    }
    let line_number: usize = after[..digits].parse().ok()?;
    // The marks that closed a wrapped name are not part of the comment.
    let tail = after[digits..].trim_start_matches(|c| wrapped && (c == '*' || c == '`'));
    let text = tail.trim_start_matches([':', '-', ' ']).trim().to_string();
    Some(Comment { path: path.to_string(), line: line_number, text })
}

/// What a reviewer says, passed back to the session that was reviewed.
pub fn send_back_message(reviewer: &str, review: &str) -> String {
    crate::i18n::tf!("{reviewer} reviewed your changes and says:\n\n{review}\n\nPlease go through it, and change what needs changing.")
}

#[cfg(test)]
#[path = "agent_worktree_tests.rs"]
mod tests;
