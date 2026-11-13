//! Git, through the git program itself, so that sign-ins, SSH keys, signing and hooks all work as they do at the
//! command line, and Limoniter never holds a password. This part finds git and the repository a file is in, and
//! asks git things; nothing here draws anything. Git 2.23 or later is needed (for restore and switch).

use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

/// Where git is: on the path, or where Git for Windows puts it.
pub fn program() -> Option<PathBuf> {
    let name = if cfg!(windows) { "git.exe" } else { "git" };
    if let Some(paths) = std::env::var_os("PATH") {
        for folder in std::env::split_paths(&paths) {
            let candidate = folder.join(name);
            if candidate.is_file() {
                return Some(candidate);
            }
        }
    }
    let usual: &[&str] = if cfg!(windows) {
        &["C:\\Program Files\\Git\\cmd\\git.exe", "C:\\Program Files (x86)\\Git\\cmd\\git.exe"]
    } else {
        &["/usr/bin/git", "/usr/local/bin/git", "/opt/homebrew/bin/git"]
    };
    usual.iter().map(PathBuf::from).find(|path| path.is_file())
}

/// The version git says it is, such as "2.43.0".
pub fn version() -> Option<String> {
    let output = command(None).arg("--version").output().ok()?;
    let said = String::from_utf8_lossy(&output.stdout);
    said.split_whitespace().nth(2).map(str::to_string)
}

/// The top folder of the repository a file or folder is in, if it's in one.
pub fn repository_of(path: &Path) -> Option<PathBuf> {
    let mut at = if path.is_dir() { path.to_path_buf() } else { path.parent()?.to_path_buf() };
    loop {
        if at.join(".git").exists() {
            return Some(at);
        }
        at = at.parent()?.to_path_buf();
    }
}

/// A git command, run in a repository if there's one. It never waits for a typed answer (there's nowhere to type
/// one), never opens a console window, and says file names as they are.
fn command(root: Option<&Path>) -> Command {
    let mut command = Command::new(program().unwrap_or_else(|| PathBuf::from("git")));
    if let Some(root) = root {
        command.current_dir(root);
    }
    command.args(["-c", "core.quotepath=off", "-c", "color.ui=false"]);
    command.env("GIT_TERMINAL_PROMPT", "0").env("GIT_OPTIONAL_LOCKS", "0").stdin(Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x0800_0000);
    }
    command
}

/// Runs git and returns what it printed, or what it said went wrong.
pub fn run(root: &Path, args: &[&str]) -> Result<String, String> {
    let output = run_bytes(root, args)?;
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

fn run_bytes(root: &Path, args: &[&str]) -> Result<Output, String> {
    let output = command(Some(root)).args(args).output().map_err(|e| format!("Git couldn't be started: {e}"))?;
    if output.status.success() {
        Ok(output)
    } else {
        let said = String::from_utf8_lossy(&output.stderr).trim().to_string();
        let said = if said.is_empty() { String::from_utf8_lossy(&output.stdout).trim().to_string() } else { said };
        Err(if said.is_empty() { format!("git {} failed.", args.first().unwrap_or(&"")) } else { said })
    }
}

/// Runs git with something on its standard input, such as a patch.
fn run_with_input(root: &Path, args: &[&str], input: &[u8]) -> Result<String, String> {
    use std::io::Write;
    let mut child = command(Some(root)).args(args).stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn().map_err(|e| format!("Git couldn't be started: {e}"))?;
    child.stdin.take().map(|mut stdin| stdin.write_all(input)).transpose().map_err(|e| e.to_string())?;
    let output = child.wait_with_output().map_err(|e| e.to_string())?;
    if output.status.success() { Ok(String::from_utf8_lossy(&output.stdout).into_owned()) } else { Err(String::from_utf8_lossy(&output.stderr).trim().to_string()) }
}

/// A file's path in the repository, with forward slashes, as git names it.
pub fn relative(root: &Path, path: &Path) -> Option<String> {
    let root = std::fs::canonicalize(root).unwrap_or_else(|_| root.to_path_buf());
    let path = std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    let inside = path.strip_prefix(&root).ok()?;
    Some(inside.components().map(|c| c.as_os_str().to_string_lossy().into_owned()).collect::<Vec<_>>().join("/"))
}

// ---------- Status ----------

/// What's happened to a file since the last commit.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Change {
    Added,
    Modified,
    Deleted,
    Renamed,
    /// Its type changed, such as a file becoming a link.
    TypeChanged,
    Untracked,
    /// Both sides changed it, and the merge stopped for the person to choose.
    Conflicted,
}

impl Change {
    fn from_letter(letter: u8) -> Option<Change> {
        match letter {
            b'A' => Some(Change::Added),
            b'M' => Some(Change::Modified),
            b'D' => Some(Change::Deleted),
            b'R' | b'C' => Some(Change::Renamed),
            b'T' => Some(Change::TypeChanged),
            _ => None,
        }
    }

    pub fn word(self) -> &'static str {
        match self {
            Change::Added => "new",
            Change::Modified => "changed",
            Change::Deleted => "deleted",
            Change::Renamed => "renamed",
            Change::TypeChanged => "type changed",
            Change::Untracked => "not tracked yet",
            Change::Conflicted => "in conflict",
        }
    }
}

/// A file git says something about.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Entry {
    pub path: String,
    /// What was it called before, if renamed.
    pub from: Option<String>,
    /// The change ready to commit (staged), and the change not yet staged.
    pub staged: Option<Change>,
    pub unstaged: Option<Change>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Status {
    /// The branch, or None when it's on no branch (a detached HEAD).
    pub branch: Option<String>,
    /// Whether there's a commit at all yet, and which one is current.
    pub born: bool,
    pub head: String,
    pub upstream: Option<String>,
    pub ahead: usize,
    pub behind: usize,
    pub entries: Vec<Entry>,
    /// A rebase or merge partway through, waiting on the person.
    pub underway: Option<Underway>,
}

impl Status {
    pub fn staged(&self) -> impl Iterator<Item = &Entry> {
        self.entries.iter().filter(|e| e.staged.is_some() && e.unstaged != Some(Change::Conflicted))
    }

    pub fn unstaged(&self) -> impl Iterator<Item = &Entry> {
        self.entries.iter().filter(|e| e.unstaged.is_some() && e.unstaged != Some(Change::Conflicted))
    }

    pub fn conflicted(&self) -> impl Iterator<Item = &Entry> {
        self.entries.iter().filter(|e| e.unstaged == Some(Change::Conflicted))
    }

    /// What's changed for one file, if anything.
    pub fn of(&self, relative: &str) -> Option<&Entry> {
        self.entries.iter().find(|e| e.path == relative)
    }
}

/// The repository's status: its branch, how it stands with its upstream, and every changed file.
pub fn status(root: &Path) -> Result<Status, String> {
    let output = run_bytes(root, &["status", "--porcelain=v2", "--branch", "-z", "--untracked-files=all"])?;
    let mut status = parse_status(&output.stdout);
    status.underway = underway(root);
    Ok(status)
}

/// Reads `git status --porcelain=v2 --branch -z`.
pub fn parse_status(bytes: &[u8]) -> Status {
    let mut status = Status { born: true, ..Status::default() };
    let mut fields = bytes.split(|b| *b == 0).map(|f| String::from_utf8_lossy(f).into_owned());
    while let Some(field) = fields.next() {
        if field.is_empty() {
            continue;
        }
        if let Some(header) = field.strip_prefix("# ") {
            let (key, value) = header.split_once(' ').unwrap_or((header, ""));
            match key {
                "branch.oid" => {
                    status.born = value != "(initial)";
                    status.head = value.to_string();
                }
                "branch.head" => status.branch = (value != "(detached)").then(|| value.to_string()),
                "branch.upstream" => status.upstream = Some(value.to_string()),
                "branch.ab" => {
                    for part in value.split_whitespace() {
                        if let Some(n) = part.strip_prefix('+') {
                            status.ahead = n.parse().unwrap_or(0);
                        } else if let Some(n) = part.strip_prefix('-') {
                            status.behind = n.parse().unwrap_or(0);
                        }
                    }
                }
                _ => {}
            }
            continue;
        }
        let kind = field.as_bytes()[0];
        match kind {
            b'1' | b'2' => {
                // "1 XY sub mH mI mW hH hI path", and for a rename "2 ... Xscore path" then the old path.
                let parts: Vec<&str> = field.splitn(if kind == b'1' { 9 } else { 10 }, ' ').collect();
                let xy = parts.get(1).map_or(b"..".as_slice(), |xy| xy.as_bytes());
                let path = parts.last().map_or_else(String::new, |p| (*p).to_string());
                let from = if kind == b'2' { fields.next() } else { None };
                status.entries.push(Entry { path, from, staged: Change::from_letter(xy[0]), unstaged: Change::from_letter(xy[1]) });
            }
            b'u' => {
                let path = field.splitn(11, ' ').last().unwrap_or_default().to_string();
                status.entries.push(Entry { path, from: None, staged: None, unstaged: Some(Change::Conflicted) });
            }
            b'?' => status.entries.push(Entry { path: field[2..].to_string(), from: None, staged: None, unstaged: Some(Change::Untracked) }),
            _ => {}
        }
    }
    status
}

// ---------- Changing what's staged, and committing ----------

/// Stages files: their changes will be in the next commit.
pub fn stage(root: &Path, paths: &[String]) -> Result<(), String> {
    let mut args = vec!["add", "--all", "--"];
    args.extend(paths.iter().map(String::as_str));
    run(root, &args).map(|_| ())
}

/// Takes files' changes out of the next commit, leaving the files as they are.
pub fn unstage(root: &Path, paths: &[String], born: bool) -> Result<(), String> {
    let mut args = if born { vec!["restore", "--staged", "--"] } else { vec!["rm", "--cached", "--quiet", "--"] };
    args.extend(paths.iter().map(String::as_str));
    run(root, &args).map(|_| ())
}

/// Throws away changes to files that git has, putting them back as they were at the last commit (or as staged).
pub fn discard(root: &Path, paths: &[String]) -> Result<(), String> {
    let mut args = vec!["restore", "--worktree", "--"];
    args.extend(paths.iter().map(String::as_str));
    run(root, &args).map(|_| ())
}

/// Commits what's staged, or with `amend`, folds it into the last commit.
pub fn commit(root: &Path, message: &str, amend: bool) -> Result<String, String> {
    let mut args = vec!["commit", "--cleanup=strip", "-F", "-"];
    if amend {
        args.push("--amend");
    }
    run_with_input(root, &args, message.as_bytes())
}

/// The name and email commits are made with, if git has been told them.
pub fn identity(root: Option<&Path>) -> (Option<String>, Option<String>) {
    let get = |key: &str| {
        let output = command(root).args(["config", "--get", key]).output().ok()?;
        let value = String::from_utf8_lossy(&output.stdout).trim().to_string();
        (!value.is_empty()).then_some(value)
    };
    (get("user.name"), get("user.email"))
}

/// Tells git, for every repository, the name and email to make commits with.
pub fn set_identity(name: &str, email: &str) -> Result<(), String> {
    for (key, value) in [("user.name", name), ("user.email", email)] {
        let output = command(None).args(["config", "--global", key, value]).output().map_err(|e| e.to_string())?;
        if !output.status.success() {
            return Err(String::from_utf8_lossy(&output.stderr).trim().to_string());
        }
    }
    Ok(())
}

/// Starts a repository in a folder, on a branch called main.
pub fn init(folder: &Path) -> Result<(), String> {
    run(folder, &["init", "--quiet", "--initial-branch=main"]).or_else(|_| run(folder, &["init", "--quiet"])).map(|_| ())
}

// ---------- Branches and remotes ----------

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Branch {
    pub name: String,
    pub current: bool,
    /// A branch on a remote, such as origin/main.
    pub remote: bool,
}

pub fn branches(root: &Path) -> Result<Vec<Branch>, String> {
    let said = run(root, &["branch", "--all", "--format=%(HEAD)%00%(refname)%00%(refname:short)"])?;
    Ok(said
        .lines()
        .filter_map(|line| {
            let mut parts = line.split('\0');
            let (head, full, short) = (parts.next()?, parts.next()?, parts.next()?);
            // origin/HEAD only says which branch the remote thinks is its main one.
            (!full.ends_with("/HEAD")).then(|| Branch { name: short.to_string(), current: head == "*", remote: full.starts_with("refs/remotes/") })
        })
        .collect())
}

/// Switches to a branch; one only on the remote is made here, following it.
pub fn switch(root: &Path, branch: &str) -> Result<(), String> {
    run(root, &["switch", branch]).map(|_| ())
}

pub fn create_branch(root: &Path, name: &str) -> Result<(), String> {
    run(root, &["switch", "--create", name]).map(|_| ())
}

pub fn remotes(root: &Path) -> Vec<(String, String)> {
    run(root, &["remote", "--verbose"])
        .unwrap_or_default()
        .lines()
        .filter(|line| line.ends_with("(push)"))
        .filter_map(|line| {
            let mut parts = line.split_whitespace();
            Some((parts.next()?.to_string(), parts.next()?.to_string()))
        })
        .collect()
}

pub fn add_remote(root: &Path, name: &str, url: &str) -> Result<(), String> {
    run(root, &["remote", "add", name, url]).map(|_| ())
}

/// Pushes the current branch, setting where it goes the first time.
pub fn push(root: &Path, status: &Status) -> Result<String, String> {
    let output = match (&status.upstream, &status.branch) {
        (Some(_), _) => run_bytes(root, &["push"])?,
        (None, Some(branch)) => run_bytes(root, &["push", "--set-upstream", "origin", branch])?,
        (None, None) => return Err("There's no branch to push: switch to one first.".into()),
    };
    Ok(String::from_utf8_lossy(&output.stderr).trim().to_string())
}

/// Brings in what's been pushed elsewhere: only if it can simply follow on, so nothing is merged by surprise.
pub fn pull(root: &Path) -> Result<String, String> {
    run(root, &["pull", "--ff-only"])
}

pub fn fetch(root: &Path) -> Result<String, String> {
    run(root, &["fetch", "--prune"])
}

/// Clones a repository into a new folder.
pub fn clone(url: &str, into: &Path) -> Result<(), String> {
    let parent = into.parent().ok_or("Choose a folder to put it in.")?;
    std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    let output = command(Some(parent)).args(["clone", "--", url]).arg(into).output().map_err(|e| format!("Git couldn't be started: {e}"))?;
    if output.status.success() { Ok(()) } else { Err(String::from_utf8_lossy(&output.stderr).trim().to_string()) }
}

/// Brings in what's been pushed elsewhere, putting this branch's own new commits after them (a rebase), for when
/// both have moved on. A conflict stops it partway, for the person to settle and continue, or abort.
pub fn pull_rebase(root: &Path) -> Result<String, String> {
    run(root, &["pull", "--rebase"])
}

/// Something git is partway through, and waiting on the person for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Underway {
    Rebase,
    Merge,
}

/// Whether a rebase or merge is partway through.
pub fn underway(root: &Path) -> Option<Underway> {
    let git_dir = run(root, &["rev-parse", "--git-dir"]).ok()?;
    let git_dir = root.join(git_dir.trim());
    if git_dir.join("rebase-merge").exists() || git_dir.join("rebase-apply").exists() {
        Some(Underway::Rebase)
    } else if git_dir.join("MERGE_HEAD").exists() {
        Some(Underway::Merge)
    } else {
        None
    }
}

/// The message git has ready for a merge's commit, if one's partway through.
pub fn merge_message(root: &Path) -> Option<String> {
    let git_dir = run(root, &["rev-parse", "--git-dir"]).ok()?;
    let text = std::fs::read_to_string(root.join(git_dir.trim()).join("MERGE_MSG")).ok()?;
    Some(text.lines().filter(|line| !line.starts_with('#')).collect::<Vec<_>>().join("\n").trim().to_string())
}

/// Carries on with a rebase once its conflicts are settled and staged, keeping each commit's own message.
pub fn rebase_continue(root: &Path) -> Result<String, String> {
    let output = command(Some(root)).args(["-c", "core.editor=true", "rebase", "--continue"]).env("GIT_EDITOR", "true").output().map_err(|e| e.to_string())?;
    if output.status.success() { Ok(String::from_utf8_lossy(&output.stdout).into_owned()) } else { Err(String::from_utf8_lossy(&output.stderr).trim().to_string()) }
}

/// Puts everything back as it was before the rebase or merge began.
pub fn abort(root: &Path, what: Underway) -> Result<(), String> {
    let args: &[&str] = match what {
        Underway::Rebase => &["rebase", "--abort"],
        Underway::Merge => &["merge", "--abort"],
    };
    run(root, args).map(|_| ())
}

// ---------- Stashes ----------

/// Changes put aside.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Stash {
    /// Its name to git, such as stash@{0}.
    pub name: String,
    pub message: String,
}

/// Puts every change aside (new files too), leaving the files as at the last commit.
pub fn stash(root: &Path, message: &str) -> Result<(), String> {
    let mut args = vec!["stash", "push", "--include-untracked"];
    if !message.trim().is_empty() {
        args.extend(["--message", message.trim()]);
    }
    run(root, &args).map(|_| ())
}

pub fn stashes(root: &Path) -> Vec<Stash> {
    run(root, &["stash", "list", "--format=%gd%x00%s"])
        .unwrap_or_default()
        .lines()
        .filter_map(|line| line.split_once('\0').map(|(name, message)| Stash { name: name.to_string(), message: message.to_string() }))
        .collect()
}

/// Brings changes put aside back; with `keep`, they stay put aside too.
pub fn unstash(root: &Path, name: &str, keep: bool) -> Result<(), String> {
    run(root, &["stash", if keep { "apply" } else { "pop" }, name]).map(|_| ())
}

pub fn drop_stash(root: &Path, name: &str) -> Result<(), String> {
    run(root, &["stash", "drop", name]).map(|_| ())
}

// ---------- Tags ----------

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Tag {
    pub name: String,
    pub when: String,
    pub subject: String,
}

/// The tags, newest first.
pub fn tags(root: &Path) -> Vec<Tag> {
    run(root, &["for-each-ref", "refs/tags", "--sort=-creatordate", "--format=%(refname:short)%00%(creatordate:format:%Y-%m-%d)%00%(subject)"])
        .unwrap_or_default()
        .lines()
        .filter_map(|line| {
            let mut parts = line.split('\0');
            Some(Tag { name: parts.next()?.to_string(), when: parts.next().unwrap_or_default().to_string(), subject: parts.next().unwrap_or_default().to_string() })
        })
        .collect()
}

/// Tags the current commit, with a message (an annotated tag), as releases are.
pub fn create_tag(root: &Path, name: &str, message: &str) -> Result<(), String> {
    let message = if message.trim().is_empty() { name } else { message.trim() };
    run(root, &["tag", "--annotate", name, "--message", message]).map(|_| ())
}

pub fn push_tag(root: &Path, name: &str) -> Result<String, String> {
    let output = run_bytes(root, &["push", "origin", &format!("refs/tags/{name}")])?;
    Ok(String::from_utf8_lossy(&output.stderr).trim().to_string())
}

/// What's staged, as a diff: for writing a commit's message about.
pub fn staged_diff(root: &Path) -> Result<String, String> {
    run(root, &["diff", "--cached", "--no-ext-diff", "--stat", "--patch"])
}

// ---------- History ----------

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Commit {
    pub hash: String,
    pub author: String,
    /// When, as "2026-11-13 14:02".
    pub when: String,
    pub subject: String,
}

/// The commits that changed a file (following it through renames), or the whole repository with None; newest first.
pub fn log(root: &Path, relative: Option<&str>, most: usize) -> Result<Vec<Commit>, String> {
    let most = format!("--max-count={most}");
    let mut args = vec!["log", &most, "--date=format:%Y-%m-%d %H:%M", "--format=%H%x00%an%x00%ad%x00%s"];
    if let Some(relative) = relative {
        args.extend(["--follow", "--", relative]);
    }
    let said = match run(root, &args) {
        Ok(said) => said,
        // A repository with no commits yet has no history.
        Err(problem) if problem.contains("does not have any commits") => return Ok(Vec::new()),
        Err(problem) => return Err(problem),
    };
    Ok(said
        .lines()
        .filter_map(|line| {
            let mut parts = line.split('\0');
            Some(Commit { hash: parts.next()?.to_string(), author: parts.next()?.to_string(), when: parts.next()?.to_string(), subject: parts.next().unwrap_or_default().to_string() })
        })
        .collect())
}

/// A file's bytes at a commit ("HEAD", a hash), or as staged with "". None if it wasn't there.
pub fn file_at(root: &Path, revision: &str, relative: &str) -> Option<Vec<u8>> {
    run_bytes(root, &["show", &format!("{revision}:{relative}")]).ok().map(|output| output.stdout)
}

/// Who last changed each line, and in which commit: one for each line of the file as it's saved.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Blame {
    pub hash: String,
    pub author: String,
    pub when: String,
    pub subject: String,
}

pub fn blame(root: &Path, relative: &str) -> Result<Vec<Blame>, String> {
    let said = run(root, &["blame", "--line-porcelain", "--", relative])?;
    let mut lines = Vec::new();
    let mut current = Blame { hash: String::new(), author: String::new(), when: String::new(), subject: String::new() };
    for line in said.lines() {
        if line.starts_with('\t') {
            lines.push(current.clone());
            continue;
        }
        let (key, value) = line.split_once(' ').unwrap_or((line, ""));
        match key {
            "author" => current.author = value.to_string(),
            "author-time" => current.when = value.parse::<i64>().ok().and_then(|t| chrono::DateTime::from_timestamp(t, 0)).map(|t| t.format("%Y-%m-%d").to_string()).unwrap_or_default(),
            "summary" => current.subject = value.to_string(),
            hash if hash.len() == 40 && hash.chars().all(|c| c.is_ascii_hexdigit()) => current.hash = hash.to_string(),
            _ => {}
        }
    }
    Ok(lines)
}

// ---------- Changes line by line ----------

/// What's happened to a line since a version of the file.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mark {
    Added,
    Changed,
    /// Lines were taken out just above this one.
    DeletedAbove,
}

/// Each line's mark against an older version: the lines added, the lines changed, and where lines went.
pub fn marks<S: AsRef<str>, T: AsRef<str>>(old: &[S], new: &[T]) -> Vec<Option<Mark>> {
    use similar::{Algorithm, DiffOp, capture_diff_slices_deadline};
    let old: Vec<&str> = old.iter().map(AsRef::as_ref).collect();
    let new: Vec<&str> = new.iter().map(AsRef::as_ref).collect();
    let mut marks = vec![None; new.len()];
    let deadline = Some(std::time::Instant::now() + std::time::Duration::from_secs(1));
    for op in capture_diff_slices_deadline(Algorithm::Myers, &old, &new, deadline) {
        match op {
            DiffOp::Equal { .. } => {}
            DiffOp::Insert { new_index, new_len, .. } => marks[new_index..new_index + new_len].fill(Some(Mark::Added)),
            DiffOp::Replace { new_index, new_len, .. } => marks[new_index..new_index + new_len].fill(Some(Mark::Changed)),
            DiffOp::Delete { new_index, .. } => {
                if let Some(mark) = marks.get_mut(new_index).filter(|m| m.is_none()) {
                    *mark = Some(Mark::DeletedAbove);
                }
            }
        }
    }
    marks
}

/// The stretches where a file differs from an older version: for each, the old lines and the new, by line number,
/// so one stretch can be staged, or put back, on its own.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Hunk {
    pub old_start: usize,
    pub old_lines: Vec<String>,
    pub new_start: usize,
    pub new_lines: Vec<String>,
}

pub fn hunks<S: AsRef<str>, T: AsRef<str>>(old: &[S], new: &[T]) -> Vec<Hunk> {
    use similar::{Algorithm, DiffOp, capture_diff_slices};
    let old: Vec<&str> = old.iter().map(AsRef::as_ref).collect();
    let new: Vec<&str> = new.iter().map(AsRef::as_ref).collect();
    let mut hunks: Vec<Hunk> = Vec::new();
    for op in capture_diff_slices(Algorithm::Myers, &old, &new) {
        let (old_index, old_len, new_index, new_len) = match op {
            DiffOp::Equal { .. } => continue,
            DiffOp::Insert { old_index, new_index, new_len } => (old_index, 0, new_index, new_len),
            DiffOp::Delete { old_index, old_len, new_index } => (old_index, old_len, new_index, 0),
            DiffOp::Replace { old_index, old_len, new_index, new_len } => (old_index, old_len, new_index, new_len),
        };
        // Changes right next to each other are one stretch.
        if let Some(last) = hunks.last_mut()
            && last.old_start + last.old_lines.len() == old_index
            && last.new_start + last.new_lines.len() == new_index
        {
            last.old_lines.extend(old[old_index..old_index + old_len].iter().map(|s| s.to_string()));
            last.new_lines.extend(new[new_index..new_index + new_len].iter().map(|s| s.to_string()));
            continue;
        }
        hunks.push(Hunk { old_start: old_index, old_lines: old[old_index..old_index + old_len].iter().map(|s| s.to_string()).collect(), new_start: new_index, new_lines: new[new_index..new_index + new_len].iter().map(|s| s.to_string()).collect() });
    }
    hunks
}

/// Stages one stretch of a file's changes: the staged version, with that stretch changed as the file has it.
/// `staged` is the file as it's staged now, and the stretch's old lines are from it.
pub fn stage_hunk(root: &Path, relative: &str, staged: &[String], hunk: &Hunk, ending: &str, ends_with_newline: bool) -> Result<(), String> {
    let mut lines: Vec<String> = staged.to_vec();
    lines.splice(hunk.old_start..hunk.old_start + hunk.old_lines.len(), hunk.new_lines.iter().cloned());
    let mut text = lines.join(ending);
    if ends_with_newline && !text.is_empty() {
        text.push_str(ending);
    }
    // The new contents become a blob, and the index points the file at it, keeping its mode.
    let blob = run_with_input(root, &["hash-object", "-w", "--stdin"], text.as_bytes())?.trim().to_string();
    let mode = run(root, &["ls-files", "--stage", "--", relative]).ok().and_then(|line| line.split_whitespace().next().map(str::to_string)).unwrap_or_else(|| "100644".into());
    run(root, &["update-index", "--add", "--cacheinfo", &format!("{mode},{blob},{relative}")]).map(|_| ())
}

// ---------- Conflicts ----------

/// One place where a merge stopped: the lines from each side, between git's markers.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Conflict {
    /// The marker lines: <<<<<<<, =======, >>>>>>> (and ||||||| for the common version, if shown).
    pub start: usize,
    pub middle: usize,
    pub end: usize,
    pub base: Option<usize>,
}

/// The conflicts in a file's lines.
pub fn conflicts<S: AsRef<str>>(lines: &[S]) -> Vec<Conflict> {
    let mut found = Vec::new();
    let mut open: Option<(usize, Option<usize>, Option<usize>)> = None;
    for (i, line) in lines.iter().enumerate() {
        let line = line.as_ref();
        if line.starts_with("<<<<<<< ") || line == "<<<<<<<" {
            open = Some((i, None, None));
        } else if line.starts_with("||||||| ") || line == "|||||||" {
            if let Some((_, base, _)) = &mut open {
                *base = Some(i);
            }
        } else if line == "=======" {
            if let Some((_, _, middle)) = &mut open {
                *middle = Some(i);
            }
        } else if (line.starts_with(">>>>>>> ") || line == ">>>>>>>")
            && let Some((start, base, Some(middle))) = open.take()
        {
            found.push(Conflict { start, middle, end: i, base });
        }
    }
    found
}

/// How a conflict is settled.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Keep {
    /// The lines from this branch, above the =======.
    Mine,
    /// The lines coming in, below it.
    Theirs,
    Both,
}

/// The lines a conflict is replaced with, to settle it.
pub fn settle<S: AsRef<str>>(lines: &[S], conflict: &Conflict, keep: Keep) -> Vec<String> {
    let mine_end = conflict.base.unwrap_or(conflict.middle);
    let mine = lines[conflict.start + 1..mine_end].iter().map(|s| s.as_ref().to_string());
    let theirs = lines[conflict.middle + 1..conflict.end].iter().map(|s| s.as_ref().to_string());
    match keep {
        Keep::Mine => mine.collect(),
        Keep::Theirs => theirs.collect(),
        Keep::Both => mine.chain(theirs).collect(),
    }
}

/// A .gitignore to start a repository with, for what's in the folder: build output and backups left out.
pub fn suggested_ignore(folder: &Path) -> String {
    let mut lines = vec!["# Files Git leaves out. Lines can be added here at any time.".to_string(), "*.bak".into(), "*.tmp".into(), ".DS_Store".into(), "Thumbs.db".into()];
    let has = |name: &str| folder.join(name).exists();
    if has("Cargo.toml") {
        lines.push("/target/".into());
    }
    if has("package.json") {
        lines.push("node_modules/".into());
    }
    if has("pyproject.toml") || has("setup.py") || has("requirements.txt") {
        lines.extend(["__pycache__/".into(), ".venv/".into()]);
    }
    lines.join("\n") + "\n"
}

#[cfg(test)]
#[path = "git_tests.rs"]
mod tests;
