//! Git itself, in repositories made for each test; a bare repository stands in for GitHub.

use super::*;

/// A new repository in a folder of its own, with a name and email to commit with.
fn repository(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("limoniter-git-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    init(&dir).unwrap();
    run(&dir, &["config", "user.name", "Test Person"]).unwrap();
    run(&dir, &["config", "user.email", "test@example.com"]).unwrap();
    run(&dir, &["config", "commit.gpgsign", "false"]).unwrap();
    dir
}

fn write(dir: &Path, name: &str, text: &str) {
    if let Some(parent) = dir.join(name).parent() {
        std::fs::create_dir_all(parent).unwrap();
    }
    std::fs::write(dir.join(name), text).unwrap();
}

#[test]
fn a_file_is_found_in_its_repository() {
    let dir = repository("found");
    write(&dir, "src/deep/main.rs", "fn main() {}\n");
    let file = dir.join("src/deep/main.rs");
    assert_eq!(repository_of(&file).map(|p| std::fs::canonicalize(p).unwrap()), Some(std::fs::canonicalize(&dir).unwrap()));
    assert_eq!(relative(&dir, &file).as_deref(), Some("src/deep/main.rs"));
    assert!(version().is_some());
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn the_status_says_what_has_changed_and_commits_take_what_is_staged() {
    let dir = repository("status");
    let fresh = status(&dir).unwrap();
    assert!(!fresh.born, "no commits yet");
    assert_eq!(fresh.branch.as_deref(), Some("main"));
    write(&dir, "a.txt", "one\n");
    write(&dir, "b file.txt", "two\n");
    let found = status(&dir).unwrap();
    assert_eq!(found.unstaged().map(|e| (e.path.as_str(), e.unstaged)).collect::<Vec<_>>(), [("a.txt", Some(Change::Untracked)), ("b file.txt", Some(Change::Untracked))]);
    stage(&dir, &["a.txt".into(), "b file.txt".into()]).unwrap();
    let staged = status(&dir).unwrap();
    assert_eq!(staged.staged().count(), 2);
    assert_eq!(staged.of("a.txt").and_then(|e| e.staged), Some(Change::Added));
    // Unstaged before there's a commit, it goes back to untracked.
    unstage(&dir, &["b file.txt".into()], staged.born).unwrap();
    assert_eq!(status(&dir).unwrap().of("b file.txt").and_then(|e| e.unstaged), Some(Change::Untracked));
    commit(&dir, "First snapshot\n\nWith a body.", false).unwrap();
    let after = status(&dir).unwrap();
    assert!(after.born);
    assert_eq!(after.entries.len(), 1, "only the untracked file is left");
    // A change, discarded, goes back to the commit's.
    write(&dir, "a.txt", "one, changed\n");
    assert_eq!(status(&dir).unwrap().of("a.txt").and_then(|e| e.unstaged), Some(Change::Modified));
    discard(&dir, &["a.txt".into()]).unwrap();
    assert_eq!(std::fs::read_to_string(dir.join("a.txt")).unwrap().replace("\r\n", "\n"), "one\n", "Git for Windows may check it out with CRLF");
    let history = log(&dir, Some("a.txt"), 10).unwrap();
    assert_eq!((history.len(), history[0].subject.as_str(), history[0].author.as_str()), (1, "First snapshot", "Test Person"));
    assert_eq!(file_at(&dir, "HEAD", "a.txt").as_deref(), Some(b"one\n".as_slice()));
    assert_eq!(file_at(&dir, "HEAD", "missing.txt"), None);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_rename_and_a_deletion_are_told_apart() {
    let dir = repository("rename");
    write(&dir, "old.txt", "the same text, long enough to be seen as a rename\n");
    write(&dir, "gone.txt", "x\n");
    stage(&dir, &["old.txt".into(), "gone.txt".into()]).unwrap();
    commit(&dir, "Two files", false).unwrap();
    run(&dir, &["mv", "old.txt", "new.txt"]).unwrap();
    std::fs::remove_file(dir.join("gone.txt")).unwrap();
    let found = status(&dir).unwrap();
    let renamed = found.of("new.txt").unwrap();
    assert_eq!((renamed.staged, renamed.from.as_deref()), (Some(Change::Renamed), Some("old.txt")));
    assert_eq!(found.of("gone.txt").and_then(|e| e.unstaged), Some(Change::Deleted));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn branches_are_made_switched_and_pushed_to_a_remote_and_pulled_back() {
    let dir = repository("branches");
    write(&dir, "a.txt", "one\n");
    stage(&dir, &["a.txt".into()]).unwrap();
    commit(&dir, "One", false).unwrap();
    create_branch(&dir, "idea").unwrap();
    assert_eq!(status(&dir).unwrap().branch.as_deref(), Some("idea"));
    switch(&dir, "main").unwrap();
    let names: Vec<(String, bool)> = branches(&dir).unwrap().into_iter().map(|b| (b.name, b.current)).collect();
    assert_eq!(names, [("idea".to_string(), false), ("main".to_string(), true)]);
    // A bare repository stands in for GitHub.
    let remote = dir.with_extension("remote.git");
    let _ = std::fs::remove_dir_all(&remote);
    std::fs::create_dir_all(&remote).unwrap();
    run(&remote, &["init", "--bare", "--quiet", "--initial-branch=main"]).unwrap();
    add_remote(&dir, "origin", &remote.display().to_string()).unwrap();
    assert_eq!(remotes(&dir).first().map(|(name, _)| name.as_str()), Some("origin"));
    let first = status(&dir).unwrap();
    assert!(first.upstream.is_none());
    push(&dir, &first).unwrap();
    let pushed = status(&dir).unwrap();
    assert_eq!((pushed.upstream.as_deref(), pushed.ahead, pushed.behind), (Some("origin/main"), 0, 0));
    // Someone else's clone pushes a change; it's fetched, seen as behind, and pulled.
    let other = dir.with_extension("other");
    let _ = std::fs::remove_dir_all(&other);
    clone(&remote.display().to_string(), &other).unwrap();
    run(&other, &["config", "user.name", "Someone Else"]).unwrap();
    run(&other, &["config", "user.email", "else@example.com"]).unwrap();
    write(&other, "a.txt", "one\ntwo\n");
    stage(&other, &["a.txt".into()]).unwrap();
    commit(&other, "Two", false).unwrap();
    push(&other, &status(&other).unwrap()).unwrap();
    fetch(&dir).unwrap();
    assert_eq!(status(&dir).unwrap().behind, 1);
    pull(&dir).unwrap();
    assert_eq!(std::fs::read_to_string(dir.join("a.txt")).unwrap().replace("\r\n", "\n"), "one\ntwo\n");
    for folder in [&dir, &remote, &other] {
        let _ = std::fs::remove_dir_all(folder);
    }
}

#[test]
fn lines_are_marked_against_the_last_commit() {
    let old = ["a", "b", "c", "d"];
    let new = ["a", "B", "c", "new", "d"];
    assert_eq!(marks(&old, &new), [None, Some(Mark::Changed), None, Some(Mark::Added), None]);
    assert_eq!(marks(&["a", "gone", "b"], &["a", "b"]), [None, Some(Mark::DeletedAbove)]);
    let found = hunks(&old, &new);
    assert_eq!(found.len(), 2);
    assert_eq!((found[0].old_start, found[0].old_lines.clone(), found[0].new_lines.clone()), (1, vec!["b".to_string()], vec!["B".to_string()]));
}

#[test]
fn one_stretch_of_changes_is_staged_on_its_own() {
    let dir = repository("hunk");
    write(&dir, "a.txt", "1\n2\n3\n4\n5\n6\n7\n8\n");
    stage(&dir, &["a.txt".into()]).unwrap();
    commit(&dir, "Numbers", false).unwrap();
    write(&dir, "a.txt", "ONE\n2\n3\n4\n5\n6\n7\nEIGHT\n");
    let staged: Vec<String> = String::from_utf8(file_at(&dir, "", "a.txt").unwrap()).unwrap().lines().map(str::to_string).collect();
    let now: Vec<String> = std::fs::read_to_string(dir.join("a.txt")).unwrap().lines().map(str::to_string).collect();
    let found = hunks(&staged, &now);
    assert_eq!(found.len(), 2);
    stage_hunk(&dir, "a.txt", &staged, &found[1], "\n", true).unwrap();
    assert_eq!(file_at(&dir, "", "a.txt").as_deref(), Some(b"1\n2\n3\n4\n5\n6\n7\nEIGHT\n".as_slice()), "only the second change is staged");
    let both = status(&dir).unwrap();
    let entry = both.of("a.txt").unwrap();
    assert_eq!((entry.staged, entry.unstaged), (Some(Change::Modified), Some(Change::Modified)));
    let blamed = blame(&dir, "a.txt").unwrap();
    assert_eq!(blamed.len(), 8);
    assert_eq!(blamed[1].author, "Test Person");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_real_merge_conflict_is_found_and_settled() {
    let dir = repository("conflict");
    write(&dir, "a.txt", "top\nmiddle\nbottom\n");
    stage(&dir, &["a.txt".into()]).unwrap();
    commit(&dir, "Base", false).unwrap();
    create_branch(&dir, "other").unwrap();
    write(&dir, "a.txt", "top\ntheirs\nbottom\n");
    stage(&dir, &["a.txt".into()]).unwrap();
    commit(&dir, "Theirs", false).unwrap();
    switch(&dir, "main").unwrap();
    write(&dir, "a.txt", "top\nmine\nbottom\n");
    stage(&dir, &["a.txt".into()]).unwrap();
    commit(&dir, "Mine", false).unwrap();
    assert!(run(&dir, &["merge", "other"]).is_err(), "the merge stops");
    let stopped = status(&dir).unwrap();
    assert_eq!(stopped.conflicted().map(|e| e.path.as_str()).collect::<Vec<_>>(), ["a.txt"]);
    let lines: Vec<String> = std::fs::read_to_string(dir.join("a.txt")).unwrap().lines().map(str::to_string).collect();
    let found = conflicts(&lines);
    assert_eq!(found.len(), 1);
    assert_eq!(settle(&lines, &found[0], Keep::Mine), ["mine"]);
    assert_eq!(settle(&lines, &found[0], Keep::Theirs), ["theirs"]);
    assert_eq!(settle(&lines, &found[0], Keep::Both), ["mine", "theirs"]);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn status_is_read_from_porcelain_version_two() {
    let said = b"# branch.oid abc\x00# branch.head main\x00# branch.upstream origin/main\x00# branch.ab +2 -1\x001 .M N... 100644 100644 100644 a a src/x.rs\x002 R. N... 100644 100644 100644 a a R100 new.rs\x00old.rs\x00u UU N... 1 2 3 4 h1 h2 h3 c.rs\x00? spaced name.txt\x00";
    let status = parse_status(said);
    assert_eq!((status.branch.as_deref(), status.upstream.as_deref(), status.ahead, status.behind), (Some("main"), Some("origin/main"), 2, 1));
    assert_eq!(status.entries.len(), 4);
    assert_eq!(status.entries[1].from.as_deref(), Some("old.rs"));
    assert_eq!(status.entries[2].unstaged, Some(Change::Conflicted));
    assert_eq!(status.entries[3].path, "spaced name.txt");
}

#[test]
fn a_starting_gitignore_suits_the_folder() {
    let dir = std::env::temp_dir().join(format!("limoniter-ignore-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("Cargo.toml"), "").unwrap();
    let ignore = suggested_ignore(&dir);
    assert!(ignore.contains("/target/") && ignore.contains("*.bak"));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn changes_are_put_aside_and_brought_back() {
    let dir = repository("stash");
    write(&dir, "a.txt", "one\n");
    stage(&dir, &["a.txt".into()]).unwrap();
    commit(&dir, "One", false).unwrap();
    write(&dir, "a.txt", "one, changed\n");
    write(&dir, "new.txt", "new\n");
    stash(&dir, "half done").unwrap();
    assert!(status(&dir).unwrap().entries.is_empty(), "the files are as at the last commit");
    let put = stashes(&dir);
    assert_eq!(put.len(), 1);
    assert!(put[0].message.contains("half done"));
    unstash(&dir, &put[0].name, false).unwrap();
    assert!(stashes(&dir).is_empty());
    assert_eq!(std::fs::read_to_string(dir.join("new.txt")).unwrap().replace("\r\n", "\n"), "new\n", "new files come back too");
    stash(&dir, "").unwrap();
    drop_stash(&dir, &stashes(&dir)[0].name).unwrap();
    assert!(stashes(&dir).is_empty());
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_release_is_tagged_and_the_tag_pushed() {
    let dir = repository("tags");
    write(&dir, "a.txt", "one\n");
    stage(&dir, &["a.txt".into()]).unwrap();
    commit(&dir, "One", false).unwrap();
    let remote = dir.with_extension("tags.git");
    let _ = std::fs::remove_dir_all(&remote);
    std::fs::create_dir_all(&remote).unwrap();
    run(&remote, &["init", "--bare", "--quiet", "--initial-branch=main"]).unwrap();
    add_remote(&dir, "origin", &remote.display().to_string()).unwrap();
    create_tag(&dir, "v1.0.0", "The first release").unwrap();
    let found = tags(&dir);
    assert_eq!((found[0].name.as_str(), found[0].subject.as_str()), ("v1.0.0", "The first release"));
    push_tag(&dir, "v1.0.0").unwrap();
    assert!(run(&remote, &["tag"]).unwrap().contains("v1.0.0"), "the tag is online");
    for folder in [&dir, &remote] {
        let _ = std::fs::remove_dir_all(folder);
    }
}

#[test]
fn diverged_branches_are_rebased_and_a_conflict_partway_can_be_aborted_or_settled() {
    let dir = repository("rebase");
    write(&dir, "a.txt", "1\n2\n3\n");
    stage(&dir, &["a.txt".into()]).unwrap();
    commit(&dir, "Base", false).unwrap();
    let remote = dir.with_extension("rebase.git");
    let _ = std::fs::remove_dir_all(&remote);
    std::fs::create_dir_all(&remote).unwrap();
    run(&remote, &["init", "--bare", "--quiet", "--initial-branch=main"]).unwrap();
    add_remote(&dir, "origin", &remote.display().to_string()).unwrap();
    push(&dir, &status(&dir).unwrap()).unwrap();
    let other = dir.with_extension("rebase-other");
    let _ = std::fs::remove_dir_all(&other);
    clone(&remote.display().to_string(), &other).unwrap();
    for (key, value) in [("user.name", "Else"), ("user.email", "else@example.com"), ("commit.gpgsign", "false")] {
        run(&other, &["config", key, value]).unwrap();
    }
    // Both change the first line: online first.
    write(&other, "a.txt", "ONE\n2\n3\n");
    stage(&other, &["a.txt".into()]).unwrap();
    commit(&other, "Theirs", false).unwrap();
    push(&other, &status(&other).unwrap()).unwrap();
    write(&dir, "a.txt", "uno\n2\n3\n");
    stage(&dir, &["a.txt".into()]).unwrap();
    commit(&dir, "Mine", false).unwrap();
    fetch(&dir).unwrap();
    let both = status(&dir).unwrap();
    assert_eq!((both.ahead, both.behind), (1, 1), "both have moved on");
    assert!(pull(&dir).is_err(), "a plain pull won't merge by surprise");
    assert!(pull_rebase(&dir).is_err(), "the rebase stops at the conflict");
    assert_eq!(underway(&dir), Some(Underway::Rebase));
    abort(&dir, Underway::Rebase).unwrap();
    assert_eq!(underway(&dir), None);
    assert_eq!(std::fs::read_to_string(dir.join("a.txt")).unwrap().replace("\r\n", "\n"), "uno\n2\n3\n", "aborted, it's as it was");
    // Again, settled this time, and continued.
    assert!(pull_rebase(&dir).is_err());
    write(&dir, "a.txt", "ONE and uno\n2\n3\n");
    stage(&dir, &["a.txt".into()]).unwrap();
    rebase_continue(&dir).unwrap();
    assert_eq!(underway(&dir), None);
    let after = status(&dir).unwrap();
    assert_eq!((after.ahead, after.behind), (1, 0), "mine now comes after theirs");
    let subjects: Vec<String> = log(&dir, None, 5).unwrap().into_iter().map(|c| c.subject).collect();
    assert_eq!(subjects, ["Mine", "Theirs", "Base"]);
    for folder in [&dir, &remote, &other] {
        let _ = std::fs::remove_dir_all(folder);
    }
}
