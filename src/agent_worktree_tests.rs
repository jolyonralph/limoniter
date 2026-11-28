//! A session's own copy: made, its changes listed, merged back three ways, removed. Real Git in scratch repositories.

use super::*;

/// A repository with one commit holding `files`, in a folder of its own (and the copies go beside it).
fn repository(tag: &str, files: &[(&str, &str)]) -> PathBuf {
    let top = std::env::temp_dir().join(format!("limoniter-wt-{}-{tag}", std::process::id()));
    let _ = std::fs::remove_dir_all(&top);
    let dir = top.join("project");
    std::fs::create_dir_all(&dir).unwrap();
    git::init(&dir).unwrap();
    for (key, value) in [("user.name", "Test Person"), ("user.email", "test@example.com"), ("commit.gpgsign", "false"), ("core.autocrlf", "false")] {
        git::run(&dir, &["config", key, value]).unwrap();
    }
    for (name, text) in files {
        write(&dir, name, text);
    }
    git::run(&dir, &["add", "-A"]).unwrap();
    git::commit(&dir, "first", false).unwrap();
    crate::platform::canonical(&dir).unwrap()
}

pub(crate) fn write(dir: &Path, name: &str, text: &str) {
    let path = dir.join(name);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, text).unwrap();
}

fn read(dir: &Path, name: &str) -> String {
    std::fs::read_to_string(dir.join(name)).unwrap_or_else(|_| "<missing>".into())
}

const POEM: &str = "one\ntwo\nthree\nfour\nfive\nsix\nseven\n";

#[test]
fn a_copy_is_offered_only_in_a_clean_repository_or_with_a_choice_about_changes() {
    let plain = std::env::temp_dir().join(format!("limoniter-wt-{}-plain", std::process::id()));
    std::fs::create_dir_all(&plain).unwrap();
    // Not in a repository (unless the temp folder itself is in one, which it isn't).
    assert_eq!(offer(&plain), Offer::NotGit);
    assert!(why_not(&Offer::NotGit).is_some());
    let dir = repository("offer", &[("a.txt", POEM)]);
    assert_eq!(offer(&dir), Offer::Yes);
    write(&dir, "a.txt", "changed\n");
    write(&dir, "new.txt", "new\n");
    assert_eq!(offer(&dir), Offer::Dirty(2));
    assert_eq!(why_not(&Offer::Dirty(2)), None, "changes are asked about, not a reason to refuse");
    let empty = std::env::temp_dir().join(format!("limoniter-wt-{}-empty", std::process::id()));
    let _ = std::fs::remove_dir_all(&empty);
    std::fs::create_dir_all(&empty).unwrap();
    git::init(&empty).unwrap();
    assert_eq!(offer(&empty), Offer::NoCommit);
}

#[test]
fn the_copy_is_made_beside_the_project_on_a_branch_of_its_own() {
    let dir = repository("make", &[("a.txt", POEM), ("src/b.txt", "b\n")]);
    let copy = create(&dir, "Fix the parser!", Carry::Ask).unwrap();
    assert_eq!(copy.branch, "limoniter/fix-the-parser");
    assert_eq!(copy.path, crate::platform::canonical(&dir.parent().unwrap().join("project.limoniter/fix-the-parser")).unwrap());
    assert_eq!(read(&copy.path, "a.txt"), POEM);
    assert_eq!(copy.base, git::run(&dir, &["rev-parse", "HEAD"]).unwrap().trim());
    assert_eq!(git::status(&copy.path).unwrap().branch.as_deref(), Some("limoniter/fix-the-parser"));
    assert_eq!(copy.folder, copy.path);
    // The same name again gets another.
    let second = create(&dir, "Fix the parser!", Carry::Ask).unwrap();
    assert_eq!(second.branch, "limoniter/fix-the-parser-2");
    assert_ne!(second.path, copy.path);
    // A session started in a subfolder works in the same place in the copy.
    let deep = create(&dir.join("src"), "", Carry::Ask).unwrap();
    assert_eq!(deep.folder, deep.path.join("src"));
    assert!(deep.branch.starts_with("limoniter/session"));
}

#[test]
fn changes_not_committed_are_carried_over_or_left_behind_as_chosen() {
    let dir = repository("carry", &[("a.txt", POEM), ("gone.txt", "bye\n")]);
    write(&dir, "a.txt", "edited\n");
    write(&dir, "fresh.txt", "fresh\n");
    std::fs::remove_file(dir.join("gone.txt")).unwrap();
    assert!(create(&dir, "x", Carry::Ask).is_err(), "the person hasn't said");
    let carried = create(&dir, "carried", Carry::Carry).unwrap();
    assert_eq!(read(&carried.path, "a.txt"), "edited\n");
    assert_eq!(read(&carried.path, "fresh.txt"), "fresh\n");
    assert!(!carried.path.join("gone.txt").exists());
    let clean = create(&dir, "clean", Carry::LastCommit).unwrap();
    assert_eq!(read(&clean.path, "a.txt"), POEM);
    assert!(!clean.path.join("fresh.txt").exists());
    assert!(clean.path.join("gone.txt").exists());
    // What was carried is not counted as the session's work when merging back: the project already has it.
    let report = merge_back(&carried).unwrap();
    assert!(report.taken.is_empty() && report.conflicts.is_empty() && report.merged.is_empty(), "{report:?}");
}

#[test]
fn the_files_the_copy_changed_are_listed_committed_or_not() {
    let dir = repository("list", &[("a.txt", POEM), ("b.txt", "b\n"), ("c.txt", "c\n")]);
    let copy = create(&dir, "list", Carry::Ask).unwrap();
    write(&copy.path, "a.txt", "A\n");
    git::run(&copy.path, &["add", "-A"]).unwrap();
    git::commit(&copy.path, "agent commit", false).unwrap();
    std::fs::remove_file(copy.path.join("b.txt")).unwrap();
    write(&copy.path, "new/n.txt", "n\n");
    let found = changed(&copy).unwrap();
    assert_eq!(found, vec![Changed { path: "a.txt".into(), what: Happened::Changed }, Changed { path: "b.txt".into(), what: Happened::Deleted }, Changed { path: "new/n.txt".into(), what: Happened::New }]);
    let diff = diff_text(&copy).unwrap();
    assert!(diff.contains("+++ b/new/n.txt") && diff.contains("-b"), "{diff}");
}

#[test]
fn each_case_of_merging_a_file_back() {
    let b: &[u8] = POEM.as_bytes();
    let theirs = POEM.replace("two", "TWO");
    let mine_far = POEM.replace("seven", "SEVEN");
    let mine_near = POEM.replace("two", "2");
    // Nobody changed it, or both changed it the same way.
    assert_eq!(plan(Some(b), Some(b), Some(b)), Plan::Nothing);
    assert_eq!(plan(Some(b), Some(theirs.as_bytes()), Some(theirs.as_bytes())), Plan::Nothing);
    // The project hasn't changed it: the copy's is taken, deletion included, and a new file.
    assert_eq!(plan(Some(b), Some(b), Some(theirs.as_bytes())), Plan::Write(theirs.clone().into_bytes()));
    assert_eq!(plan(Some(b), Some(b), None), Plan::Remove);
    assert_eq!(plan(None, None, Some(b"new\n")), Plan::Write(b"new\n".to_vec()));
    // Both changed it, in different places: three-way merged.
    assert_eq!(plan(Some(b), Some(mine_far.as_bytes()), Some(theirs.as_bytes())), Plan::Merged(POEM.replace("two", "TWO").replace("seven", "SEVEN")));
    // The same place: a conflict, with the three sides for the merge view.
    let Plan::Conflict { base, mine, theirs: t } = plan(Some(b), Some(mine_near.as_bytes()), Some(theirs.as_bytes())) else { panic!("a conflict") };
    assert_eq!((base.as_str(), mine.as_str(), t.as_str()), (POEM, mine_near.as_str(), theirs.as_str()));
    // Deleted on one side, changed on the other: the person chooses.
    assert!(matches!(plan(Some(b), Some(mine_far.as_bytes()), None), Plan::Conflict { .. }));
    // Not text, changed both ways.
    assert_eq!(plan(Some(&[0, 1, 2]), Some(&[0, 1, 3]), Some(&[0, 1, 4])), Plan::Unmergeable);
}

#[test]
fn merging_back_takes_merges_and_hands_over_the_clashes() {
    let dir = repository("merge", &[("taken.txt", POEM), ("both.txt", POEM), ("clash.txt", POEM), ("same.txt", POEM), ("deleted.txt", "x\n")]);
    let copy = create(&dir, "merge", Carry::Ask).unwrap();
    // The agent's work in the copy.
    write(&copy.path, "taken.txt", &POEM.replace("one", "ONE"));
    write(&copy.path, "both.txt", &POEM.replace("two", "TWO"));
    write(&copy.path, "clash.txt", &POEM.replace("three", "THREE"));
    write(&copy.path, "same.txt", &POEM.replace("four", "FOUR"));
    write(&copy.path, "added.txt", "added\n");
    std::fs::remove_file(copy.path.join("deleted.txt")).unwrap();
    // The person's work in the project meanwhile.
    write(&dir, "both.txt", &POEM.replace("seven", "SEVEN"));
    write(&dir, "clash.txt", &POEM.replace("three", "3"));
    write(&dir, "same.txt", &POEM.replace("four", "FOUR"));
    let report = merge_back(&copy).unwrap();
    assert_eq!(report.taken, vec!["added.txt", "deleted.txt", "taken.txt"]);
    assert_eq!(report.merged, vec!["both.txt"]);
    assert_eq!(report.same, vec!["same.txt"]);
    assert_eq!(report.conflicts.len(), 1);
    assert_eq!(report.conflicts[0].path, "clash.txt");
    assert_eq!(report.conflicts[0].mine, POEM.replace("three", "3"));
    assert_eq!(report.conflicts[0].theirs, POEM.replace("three", "THREE"));
    assert_eq!(read(&dir, "taken.txt"), POEM.replace("one", "ONE"));
    assert_eq!(read(&dir, "both.txt"), POEM.replace("two", "TWO").replace("seven", "SEVEN"));
    assert_eq!(read(&dir, "added.txt"), "added\n");
    assert_eq!(read(&dir, "deleted.txt"), "<missing>");
    assert_eq!(read(&dir, "clash.txt"), POEM.replace("three", "3"), "a clash leaves the project's file alone for the merge view");
    // Nothing was committed in the project.
    assert_eq!(git::run(&dir, &["rev-list", "--count", "HEAD"]).unwrap().trim(), "1");
}

#[test]
fn committing_and_removing_the_copy() {
    let dir = repository("remove", &[("a.txt", POEM)]);
    let copy = create(&dir, "gone", Carry::Ask).unwrap();
    write(&copy.path, "a.txt", "agent\n");
    commit(&copy, "agent work").unwrap();
    assert!(git::status(&copy.path).unwrap().entries.is_empty());
    assert_eq!(git::run(&dir, &["log", "-1", "--format=%s", &copy.branch]).unwrap().trim(), "agent work");
    // The folder goes, the branch stays.
    remove(&copy, false).unwrap();
    assert!(!copy.path.exists());
    assert!(git::branches(&dir).unwrap().iter().any(|b| b.name == copy.branch));
    // The folder and the branch both go.
    let other = create(&dir, "other", Carry::Ask).unwrap();
    write(&other.path, "x.txt", "never committed\n");
    remove(&other, true).unwrap();
    assert!(!other.path.exists());
    assert!(!git::branches(&dir).unwrap().iter().any(|b| b.name == other.branch));
    assert!(git::run(&dir, &["worktree", "list"]).unwrap().lines().count() == 1);
}

#[test]
fn several_copies_are_merged_back_in_turn() {
    let dir = repository("several", &[("a.txt", POEM), ("b.txt", POEM)]);
    let first = create(&dir, "first", Carry::Ask).unwrap();
    let second = create(&dir, "second", Carry::Ask).unwrap();
    write(&first.path, "a.txt", &POEM.replace("one", "ONE"));
    write(&second.path, "a.txt", &POEM.replace("seven", "SEVEN"));
    write(&second.path, "b.txt", &POEM.replace("two", "TWO"));
    let one = merge_back(&first).unwrap();
    assert_eq!(one.taken, vec!["a.txt"]);
    // The second now meets what the first put in, and fits with it.
    let two = merge_back(&second).unwrap();
    assert_eq!(two.merged, vec!["a.txt"]);
    assert_eq!(two.taken, vec!["b.txt"]);
    assert_eq!(read(&dir, "a.txt"), POEM.replace("one", "ONE").replace("seven", "SEVEN"));
    remove(&first, true).unwrap();
    remove(&second, true).unwrap();
}

#[test]
fn the_review_message_and_the_comments_in_the_answer() {
    let message = review_message("Look at this.", "diff --git a/x b/x\n+new\n");
    assert!(message.starts_with("Look at this.\n\n```diff\ndiff --git"), "{message}");
    assert!(message.ends_with("+new\n```"));
    let long = "x".repeat(MOST_DIFF + 50);
    assert!(review_message("p", &long).contains("too long"));
    let said = "Some thoughts.\n- src/parser.rs:11: `last` is never used\n2. **tests/a.rs:4** - no test for an empty file\n`b.txt:9`: odd\nNote: this is fine\nsee a.txt:3 here\nThen v1.2:3 is not a file";
    let found = parse_comments(said);
    let comment = |path: &str, line: usize, text: &str| Comment { path: path.into(), line, text: text.into() };
    assert_eq!(found, vec![comment("src/parser.rs", 11, "`last` is never used"), comment("tests/a.rs", 4, "no test for an empty file"), comment("b.txt", 9, "odd")]);
    assert!(send_back_message("Codex", "fix x").contains("Codex reviewed your changes and says:\n\nfix x"));
}
