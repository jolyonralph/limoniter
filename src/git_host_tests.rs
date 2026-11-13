//! Git in the window, with real repositories.

use super::*;
use crate::app::tests::app_with;
use crate::git;

fn repository(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("limoniter-githost-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    git::init(&dir).unwrap();
    for (key, value) in [("user.name", "Test Person"), ("user.email", "test@example.com"), ("commit.gpgsign", "false")] {
        git::run(&dir, &["config", key, value]).unwrap();
    }
    dir
}

fn committed(dir: &Path, name: &str, text: &str) -> PathBuf {
    std::fs::write(dir.join(name), text).unwrap();
    git::stage(dir, &[name.to_string()]).unwrap();
    git::commit(dir, &format!("Add {name}"), false).unwrap();
    dir.join(name)
}

/// Runs the window's Git upkeep until git has finished whatever it was doing and the status is read.
fn settle(app: &mut Limoniter, ctx: &Context) {
    let began = std::time::Instant::now();
    app.git.refresh();
    loop {
        let busy = app.git_tick(ctx);
        if !busy && app.git.status.is_some() && app.git.job.is_none() {
            return;
        }
        assert!(began.elapsed() < Duration::from_secs(20), "git never finished");
        std::thread::sleep(Duration::from_millis(20));
    }
}

#[test]
fn an_open_file_shows_its_changes_and_is_committed_from_the_panel() {
    let dir = repository("commit");
    let file = committed(&dir, "notes.txt", "one\ntwo\nthree\n");
    let (mut app, ctx) = app_with("");
    assert!(app.open_path(&file));
    settle(&mut app, &ctx);
    assert!(app.git.repo.is_some());
    assert!(app.git.status.as_ref().is_some_and(|s| s.entries.is_empty()));
    // An edit is marked in the margin at once, saved or not.
    app.doc().text.move_caret(1, 0, false);
    app.doc().text.type_text("2 ");
    app.git_tick(&ctx);
    let marks = app.docs[app.active].text.git_marks.clone().unwrap();
    assert_eq!(marks.as_slice(), [None, Some(git::Mark::Changed), None, None]);
    assert!(app.save(app.active));
    settle(&mut app, &ctx);
    assert_eq!(app.git.status.as_ref().unwrap().unstaged().count(), 1);
    // Commit all changes, with nothing staged first.
    app.git.message = "Number two".into();
    app.git_commit(&dir);
    settle(&mut app, &ctx);
    assert!(app.git.status.as_ref().unwrap().entries.is_empty(), "{:?}", app.git.said);
    assert_eq!(git::log(&dir, None, 5).unwrap()[0].subject, "Number two");
    // Once committed, the line isn't marked any more.
    app.git_tick(&ctx);
    assert!(app.docs[app.active].text.git_marks.as_ref().unwrap().iter().all(Option::is_none));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn one_change_is_staged_and_another_undone_at_the_cursor() {
    let dir = repository("hunks");
    let file = committed(&dir, "a.txt", "1\n2\n3\n4\n5\n6\n7\n8\n");
    let (mut app, ctx) = app_with("");
    assert!(app.open_path(&file));
    settle(&mut app, &ctx);
    app.doc().text.set_content(["ONE", "2", "3", "4", "5", "6", "7", "EIGHT"].map(String::from).to_vec());
    assert!(app.save(app.active));
    // The cursor in the last change: just that goes in the next commit.
    app.doc().text.move_caret(7, 0, false);
    app.stage_change_at_cursor();
    assert_eq!(git::file_at(&dir, "", "a.txt").as_deref(), Some(b"1\n2\n3\n4\n5\n6\n7\nEIGHT\n".as_slice()));
    // And the first change is undone, as an edit.
    app.doc().text.move_caret(0, 0, false);
    app.undo_change_at_cursor();
    assert_eq!(app.docs[app.active].text.line(0).as_ref(), "1");
    assert_eq!(app.docs[app.active].text.line(7).as_ref(), "EIGHT");
    assert!(app.docs[app.active].is_modified());
    // Compared with the last commit, in the Compare window.
    app.compare_with_commit("HEAD");
    assert!(matches!(app.dialog, Dialog::Compare(_)));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn the_wizard_starts_a_folders_history_with_a_first_commit() {
    let dir = std::env::temp_dir().join(format!("limoniter-gitwizard-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("story.txt"), "Once upon a time\n").unwrap();
    std::fs::write(dir.join("story.txt.bak"), "Once\n").unwrap();
    // A name and email for this repository only, as the wizard's own step would set them for every one.
    let ignore = git::suggested_ignore(&dir);
    git::init(&dir).unwrap();
    for (key, value) in [("user.name", "Test Person"), ("user.email", "test@example.com"), ("commit.gpgsign", "false")] {
        git::run(&dir, &["config", key, value]).unwrap();
    }
    super::git_wizard::start_repository(&dir, &ignore).unwrap();
    let status = git::status(&dir).unwrap();
    assert!(status.born && status.entries.is_empty(), "everything is committed: {status:?}");
    let files = git::run(&dir, &["ls-files"]).unwrap();
    assert!(files.contains("story.txt") && files.contains(".gitignore") && !files.contains(".bak"), "{files}");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn git_is_offered_for_a_project_folder_unless_asked_never_to() {
    let dir = std::env::temp_dir().join(format!("limoniter-gitoffer-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    for name in ["a.txt", "b.txt", "c.txt"] {
        std::fs::write(dir.join(name), "x\n").unwrap();
    }
    let (mut app, ctx) = app_with("");
    assert!(app.open_path(&dir.join("a.txt")));
    app.git_tick(&ctx);
    assert_eq!(app.git.offer.as_deref().map(|p| std::fs::canonicalize(p).unwrap()), Some(std::fs::canonicalize(&dir).unwrap()));
    // Never for this folder, and it's remembered.
    let (mut app, ctx) = app_with(&format!("git_never={}\n", dir.display()));
    assert!(app.open_path(&dir.join("b.txt")));
    app.git_tick(&ctx);
    assert!(app.git.offer.is_none());
    assert!(app.git.to_settings().contains("git_never="));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn what_git_says_comes_with_what_to_do() {
    assert!(git_host::explain("! [rejected] main -> main (fetch first)").contains("Pull first"));
    assert!(git_host::explain("Author identity unknown\n*** Please tell me who you are.").contains("wizard"));
}
