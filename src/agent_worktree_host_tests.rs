//! A session in a copy of its own, in the window: started there, merged back (clean, and with a clash settled in the
//! merge view), removed, refused outside Git, several at once; and a review handed to another session and passed back.
//! The agents are the stand-in programs.

use super::super::tests::{app_with, draw};
use super::*;
use crate::agent_claude::agent_claude_tests::fake_options;
use crate::agent_session::Item;

const POEM: &str = "one\ntwo\nthree\nfour\nfive\nsix\nseven\n";

/// A repository with a commit, and an app whose Claude Code is the stand-in.
fn app_in_repository(tag: &str) -> Option<(Limoniter, Context, PathBuf)> {
    let top = std::env::temp_dir().join(format!("limoniter-wth-{}-{tag}", std::process::id()));
    let _ = std::fs::remove_dir_all(&top);
    let dir = top.join("project");
    std::fs::create_dir_all(&dir).unwrap();
    crate::git::init(&dir).unwrap();
    for (key, value) in [("user.name", "Test Person"), ("user.email", "test@example.com"), ("commit.gpgsign", "false"), ("core.autocrlf", "false")] {
        crate::git::run(&dir, &["config", key, value]).unwrap();
    }
    std::fs::write(dir.join("a.txt"), POEM).unwrap();
    std::fs::write(dir.join("b.txt"), POEM).unwrap();
    crate::git::run(&dir, &["add", "-A"]).unwrap();
    crate::git::commit(&dir, "first", false).unwrap();
    let dir = crate::platform::canonical(&dir).unwrap();
    let options = fake_options(&dir, &[])?;
    let (mut app, ctx) = app_with("");
    app.agents.program = (options.program, options.prefix);
    Some((app, ctx, dir))
}

fn separate(task: &str) -> Choice {
    Choice { separate: true, task: task.into(), ..Choice::default() }
}

fn until(app: &mut Limoniter, ctx: &Context, what: &str, done: impl Fn(&Limoniter) -> bool) {
    let began = std::time::Instant::now();
    while !done(app) {
        app.agents_tick(ctx);
        app.worktree_tick();
        assert!(began.elapsed() < Duration::from_secs(20), "waited for {what}");
        std::thread::sleep(Duration::from_millis(10));
    }
}

#[test]
fn a_session_in_a_copy_works_there_and_its_tab_says_so() {
    let Some((mut app, ctx, dir)) = app_in_repository("start") else { return };
    app.agent_start(&ctx, &dir, &separate("Try a thing"), None).unwrap();
    let tab = &app.agents.tabs[0];
    let copy = tab.worktree.clone().expect("a copy was made");
    assert_eq!(copy.branch, "limoniter/try-a-thing");
    assert_eq!(tab.session.folder, copy.folder);
    assert!(copy.path.join("a.txt").is_file() && copy.path != dir);
    assert_eq!(app.agents.last.task, "", "the next session isn't named the same");
    // The bar and the tab draw.
    app.panels.agents = true;
    draw(&mut app, &ctx, Vec::new());
    // A session not in a copy has none.
    app.agent_start(&ctx, &dir, &Choice::default(), None).unwrap();
    assert!(app.agents.tabs[1].worktree.is_none());
    assert_eq!(app.agents.tabs[1].session.folder, dir);
}

#[test]
fn a_copy_is_refused_outside_git_and_asks_about_changes() {
    let Some((mut app, ctx, dir)) = app_in_repository("refuse") else { return };
    let plain = dir.parent().unwrap().join("plain");
    std::fs::create_dir_all(&plain).unwrap();
    let why = app.agent_start(&ctx, &plain, &separate("x"), None).unwrap_err();
    assert!(why.contains("Git repository"), "{why}");
    assert!(app.agents.tabs.is_empty());
    // Uncommitted changes and no choice made.
    std::fs::write(dir.join("a.txt"), "edited\n").unwrap();
    assert_eq!(crate::agent_worktree::offer(&dir), Offer::Dirty(1));
    assert!(app.agent_start(&ctx, &dir, &separate("x"), None).is_err());
    // Carried over, it starts.
    app.agent_start(&ctx, &dir, &Choice { carry: Carry::Carry, ..separate("x") }, None).unwrap();
    let copy = app.agents.tabs[0].worktree.clone().unwrap();
    assert_eq!(std::fs::read_to_string(copy.path.join("a.txt")).unwrap(), "edited\n");
}

#[test]
fn merging_a_copy_back_then_removing_it_closes_its_session() {
    let Some((mut app, ctx, dir)) = app_in_repository("merge") else { return };
    app.agent_start(&ctx, &dir, &separate("merge"), None).unwrap();
    let (number, copy) = (app.agents.tabs[0].number, app.agents.tabs[0].worktree.clone().unwrap());
    std::fs::write(copy.path.join("a.txt"), POEM.replace("two", "TWO")).unwrap();
    std::fs::write(copy.path.join("new.txt"), "new\n").unwrap();
    // The box lists the files and what each would do.
    app.worktree_open_merge_box(number);
    let rows = &app.agents.wt.merge.as_ref().unwrap().rows;
    assert_eq!(rows.iter().map(|(c, _)| c.path.as_str()).collect::<Vec<_>>(), vec!["a.txt", "new.txt"]);
    draw(&mut app, &ctx, Vec::new());
    std::fs::write(dir.join("a.txt"), POEM.replace("seven", "SEVEN")).unwrap();
    let said = app.worktree_merge_back(number).unwrap();
    assert!(said.iter().any(|l| l.contains("fitted together")), "{said:?}");
    assert_eq!(std::fs::read_to_string(dir.join("a.txt")).unwrap(), POEM.replace("two", "TWO").replace("seven", "SEVEN"));
    assert_eq!(std::fs::read_to_string(dir.join("new.txt")).unwrap(), "new\n");
    app.worktree_remove(number, true).unwrap();
    assert!(app.agents.tabs.is_empty(), "the session in the folder that went is closed");
    assert!(!copy.path.exists());
}

#[test]
fn a_clash_opens_in_the_merge_view_and_settling_it_writes_the_project_file() {
    let Some((mut app, ctx, dir)) = app_in_repository("clash") else { return };
    app.agent_start(&ctx, &dir, &separate("clash"), None).unwrap();
    let (number, copy) = (app.agents.tabs[0].number, app.agents.tabs[0].worktree.clone().unwrap());
    std::fs::write(copy.path.join("a.txt"), POEM.replace("three", "THREE")).unwrap();
    std::fs::write(dir.join("a.txt"), POEM.replace("three", "3")).unwrap();
    let said = app.worktree_merge_back(number).unwrap();
    assert!(said.iter().any(|l| l.contains("merge view")), "{said:?}");
    let index = app.docs.iter().position(|d| d.merge.is_some()).expect("a merge tab");
    assert_eq!(app.docs[index].path.as_deref().map(|p| p.file_name().unwrap().to_owned()), Some("a.txt".into()));
    assert_eq!(app.unresolved(index), 1);
    app.active = index;
    app.merge_take(crate::merge3::Take::Theirs);
    assert_eq!(app.unresolved(index), 0);
    assert!(app.save_merge(index));
    assert_eq!(std::fs::read_to_string(dir.join("a.txt")).unwrap(), POEM.replace("three", "THREE"));
}

#[test]
fn several_copies_at_once_are_merged_in_turn() {
    let Some((mut app, ctx, dir)) = app_in_repository("several") else { return };
    app.agent_start(&ctx, &dir, &separate("first"), None).unwrap();
    app.agent_start(&ctx, &dir, &separate("second"), None).unwrap();
    let copies: Vec<(u64, Worktree)> = app.agents.tabs.iter().map(|t| (t.number, t.worktree.clone().unwrap())).collect();
    assert_ne!(copies[0].1.path, copies[1].1.path);
    std::fs::write(copies[0].1.path.join("a.txt"), POEM.replace("one", "ONE")).unwrap();
    std::fs::write(copies[1].1.path.join("a.txt"), POEM.replace("seven", "SEVEN")).unwrap();
    app.worktree_merge_back(copies[0].0).unwrap();
    app.worktree_merge_back(copies[1].0).unwrap();
    assert_eq!(std::fs::read_to_string(dir.join("a.txt")).unwrap(), POEM.replace("one", "ONE").replace("seven", "SEVEN"));
    app.worktree_remove(copies[0].0, true).unwrap();
    assert_eq!(app.agents.tabs.len(), 1, "only the first session closed");
    app.worktree_remove(copies[1].0, true).unwrap();
}

#[test]
fn a_review_is_handed_to_another_session_its_comments_found_and_sent_back() {
    let Some((mut app, ctx, dir)) = app_in_repository("review") else { return };
    app.agent_start(&ctx, &dir, &separate("review"), None).unwrap();
    let (number, copy) = (app.agents.tabs[0].number, app.agents.tabs[0].worktree.clone().unwrap());
    // Nothing to review yet.
    assert!(app.worktree_ask(&ctx, number, Tool::Claude, None, "Please look.").is_err());
    std::fs::write(copy.path.join("a.txt"), POEM.replace("two", "TWO")).unwrap();
    // The stand-in says back what it was told, so a line of the prompt that names a place comes back as a comment.
    app.worktree_ask(&ctx, number, Tool::Claude, None, "Please look.\na.txt:2: this one is odd\nThanks").unwrap();
    assert_eq!(app.agents.tabs.len(), 2, "a new session reviews");
    assert_eq!(app.agents.active, 0, "the person stays where they were");
    assert_eq!(app.agents.tabs[1].session.folder, copy.folder, "it reads the copy");
    until(&mut app, &ctx, "the review", |app| app.agents.wt.reviews[0].text.is_some());
    let review = &app.agents.wt.reviews[0];
    assert!(review.text.as_deref().unwrap().contains("```diff") && review.text.as_deref().unwrap().contains("+TWO"), "the diff was handed over: {:?}", review.text);
    assert_eq!(review.comments, vec![crate::agent_worktree::Comment { path: "a.txt".into(), line: 2, text: "this one is odd".into() }]);
    // The bar and the box draw.
    app.panels.agents = true;
    app.agents.wt.shown = Some(0);
    draw(&mut app, &ctx, Vec::new());
    // A click on the comment goes to its line in the copy's file.
    app.go_to_place(&copy.path.join("a.txt").display().to_string(), 2);
    assert_eq!(app.docs[app.active].path.as_deref().map(|p| p.starts_with(&copy.path)), Some(true));
    // Send it back: the first session gets it as its next message.
    app.worktree_send_back(0);
    assert!(app.agents.wt.reviews[0].passed_back);
    let first = &app.agents.tabs[0].session;
    assert!(first.items.iter().any(|i| matches!(i, Item::You(t) if t.contains("reviewed your changes and says") && t.contains("a.txt:2: this one is odd"))));
    // The prompt the person set is kept for the next time.
    assert!(app.agents.wt.prompt.is_empty());
}

#[test]
fn the_other_tool_is_asked_when_it_is_installed_and_the_same_one_when_not() {
    let Some((app, _ctx, _dir)) = app_in_repository("other") else { return };
    // Neither a Codex program nor WSL here: the same tool reviews.
    let mut app = app;
    app.agents.codex_program = ("limoniter-no-such-codex".into(), Vec::new());
    assert_eq!(app.other_tool(Tool::Claude), Tool::Claude);
    app.agents.codex_program = (std::env::current_exe().unwrap().display().to_string(), Vec::new());
    assert_eq!(app.other_tool(Tool::Claude), Tool::Codex);
}

/// By hand, with the real Claude Code: `LIMONITER_REAL_CLAUDE=<scratch folder> cargo test --release --offline real_claude_worktree -- --ignored`.
/// A session in a copy makes one small change (with the cheapest model), and it is merged back.
#[test]
#[ignore]
fn real_claude_worktree_session_changes_a_file_and_it_is_merged_back() {
    let Some(scratch) = std::env::var_os("LIMONITER_REAL_CLAUDE").map(PathBuf::from) else { return };
    let top = scratch.join("wt-real");
    let _ = std::fs::remove_dir_all(&top);
    let dir = top.join("project");
    std::fs::create_dir_all(&dir).unwrap();
    crate::git::init(&dir).unwrap();
    for (key, value) in [("user.name", "Test Person"), ("user.email", "test@example.com"), ("commit.gpgsign", "false"), ("core.autocrlf", "false")] {
        crate::git::run(&dir, &["config", key, value]).unwrap();
    }
    std::fs::write(dir.join("a.txt"), "one\ntwo\n").unwrap();
    crate::git::run(&dir, &["add", "-A"]).unwrap();
    crate::git::commit(&dir, "first", false).unwrap();
    let (mut app, ctx) = app_with("");
    let choice = Choice { model: "haiku".into(), mode: Mode::Bypass, ..separate("real") };
    // Root can't run bypass mode, so the edit is accepted through the review instead.
    let choice = Choice { mode: Mode::AcceptEdits, ..choice };
    app.agent_start(&ctx, &dir, &choice, None).unwrap();
    let (number, copy) = (app.agents.tabs[0].number, app.agents.tabs[0].worktree.clone().unwrap());
    app.agents.tabs[0].session.say("In a.txt, change the word two to TWO. Do nothing else, and say only: done.");
    let began = std::time::Instant::now();
    while app.agents.tabs[0].session.busy || app.agents.tabs[0].session.items.is_empty() {
        app.agents_tick(&ctx);
        for tab in app.review_tabs() {
            app.decide_review(tab, true);
        }
        assert!(began.elapsed() < Duration::from_secs(150), "{:?}", app.agents.tabs[0].session.items);
        std::thread::sleep(Duration::from_millis(50));
    }
    assert_eq!(std::fs::read_to_string(copy.path.join("a.txt")).unwrap(), "one\nTWO\n");
    assert_eq!(std::fs::read_to_string(dir.join("a.txt")).unwrap(), "one\ntwo\n", "the project was left alone meanwhile");
    app.worktree_merge_back(number).unwrap();
    assert_eq!(std::fs::read_to_string(dir.join("a.txt")).unwrap(), "one\nTWO\n");
    app.worktree_remove(number, true).unwrap();
}
