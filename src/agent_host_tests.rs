//! The agents panel in the window: an edit an agent asks to make goes through the diff review, the files it
//! changes are marked as its own, and the panel and its box draw. The agent is a stand-in program.

use super::super::tests::{app_with, draw};
use super::*;
use crate::agent_claude::agent_claude_tests::fake_options;
use crate::agent_session::{Item, Tool};
use serde_json::Value;

fn folder(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("limoniter-agent-host-{}-{tag}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// An app with a session running on the stand-in, in `dir`.
fn app_with_session(dir: &Path, extra: &[&str]) -> Option<(Limoniter, Context)> {
    let options = fake_options(dir, extra)?;
    let (mut app, ctx) = app_with("");
    app.agents.program = (options.program, options.prefix);
    app.agent_start(&ctx, dir, &Choice::default(), None).unwrap();
    Some((app, ctx))
}

/// Runs the window's round until `done` holds.
fn until(app: &mut Limoniter, ctx: &Context, what: &str, done: impl Fn(&Limoniter) -> bool) {
    let began = std::time::Instant::now();
    while !done(app) {
        app.agents_tick(ctx);
        assert!(began.elapsed() < Duration::from_secs(20), "waited for {what}; the transcript is {:?}", app.agents.tabs[0].session.items);
        std::thread::sleep(Duration::from_millis(10));
    }
}

fn session(app: &Limoniter) -> &Session {
    &app.agents.tabs[0].session
}

fn review_tab(app: &Limoniter) -> u64 {
    app.review_tabs()[0]
}

#[test]
fn an_edit_it_asks_to_make_opens_a_review_and_accepting_it_makes_the_change() {
    let dir = folder("accept");
    let file = dir.join("a.txt");
    std::fs::write(&file, "one\ntwo\n").unwrap();
    let Some((mut app, ctx)) = app_with_session(&dir, &[]) else { return };
    app.agents.tabs[0].session.say(&format!("edit {}", file.display()));
    until(&mut app, &ctx, "the review", |app| !app.review_tabs().is_empty());
    let tab = review_tab(&app);
    assert_eq!(app.review_proposed_text(tab).unwrap(), "one\nthree\n", "the proposed file");
    assert_eq!(std::fs::read_to_string(&file).unwrap(), "one\ntwo\n", "nothing is written before it's accepted");
    assert!(!session(&app).waiting().is_empty());
    // Draws, with the request waiting on the review.
    app.panels.agents = true;
    draw(&mut app, &ctx, Vec::new());
    app.decide_review(tab, true);
    until(&mut app, &ctx, "the edit", |app| !session(app).busy && !session(app).items.is_empty() && std::fs::read_to_string(&file).unwrap() == "one\nthree\n");
    assert!(app.review_tabs().is_empty(), "the tab went");
    assert!(session(&app).items.iter().any(|i| matches!(i, Item::Permission { decided: crate::agent_session::Decided::Allowed { .. }, .. })));
    assert!(crate::files_changing::claimed_by(&file).is_some_and(|who| who.starts_with("Claude Code")), "the file is marked as the agent's");
}

#[test]
fn what_the_person_changed_in_the_review_is_what_gets_written() {
    let dir = folder("changed");
    let file = dir.join("a.txt");
    std::fs::write(&file, "one\ntwo\n").unwrap();
    let Some((mut app, ctx)) = app_with_session(&dir, &[]) else { return };
    app.agents.tabs[0].session.say(&format!("edit {}", file.display()));
    until(&mut app, &ctx, "the review", |app| !app.review_tabs().is_empty());
    let tab = review_tab(&app);
    app.review_change_proposed(tab, "one\nTHREE!\n");
    app.decide_review(tab, true);
    until(&mut app, &ctx, "the edit", |app| !session(app).busy && std::fs::read_to_string(&file).unwrap() == "one\nTHREE!\n");
}

#[test]
fn rejecting_or_closing_the_review_refuses_the_change_and_tells_the_agent() {
    let dir = folder("reject");
    let file = dir.join("a.txt");
    std::fs::write(&file, "one\ntwo\n").unwrap();
    let Some((mut app, ctx)) = app_with_session(&dir, &[]) else { return };
    app.agents.tabs[0].session.say(&format!("edit {}", file.display()));
    until(&mut app, &ctx, "the review", |app| !app.review_tabs().is_empty());
    let tab = review_tab(&app);
    app.decide_review(tab, false);
    until(&mut app, &ctx, "the refusal", |app| !session(app).busy && !session(app).items.is_empty() && session(app).waiting().is_empty());
    assert_eq!(std::fs::read_to_string(&file).unwrap(), "one\ntwo\n");
    let log: Vec<Value> = std::fs::read_to_string(dir.join("log.txt")).unwrap().lines().filter_map(|l| serde_json::from_str(l).ok()).collect();
    let answer = log.iter().find(|l| l["type"] == "control_response").unwrap();
    assert_eq!(answer["response"]["response"]["behavior"], "deny");
    assert!(answer["response"]["response"]["message"].as_str().unwrap().contains("rejected"));

    // Closing the review's tab is a refusal too.
    app.agents.tabs[0].session.say(&format!("edit {}", file.display()));
    until(&mut app, &ctx, "the second review", |app| !app.review_tabs().is_empty());
    let tab = review_tab(&app);
    let index = app.docs.iter().position(|d| d.id == tab).unwrap();
    app.close_tab(index);
    until(&mut app, &ctx, "the second refusal", |app| !session(app).busy && session(app).waiting().is_empty());
    assert_eq!(std::fs::read_to_string(&file).unwrap(), "one\ntwo\n");
}

#[test]
fn refusing_in_the_panel_closes_the_review_that_was_open() {
    let dir = folder("panel-refuse");
    let file = dir.join("a.txt");
    std::fs::write(&file, "one\ntwo\n").unwrap();
    let Some((mut app, ctx)) = app_with_session(&dir, &[]) else { return };
    app.agents.tabs[0].session.say(&format!("edit {}", file.display()));
    until(&mut app, &ctx, "the review", |app| !app.review_tabs().is_empty());
    let id = session(&app).waiting()[0].id.clone();
    app.agents.tabs[0].session.answer(&id, Answer::Refuse("no".into()));
    until(&mut app, &ctx, "the review to close", |app| app.review_tabs().is_empty());
    until(&mut app, &ctx, "the turn to end", |app| !session(app).busy);
}

#[test]
fn a_command_waits_for_the_person_and_the_folder_is_marked_while_it_runs() {
    let dir = folder("command");
    let Some((mut app, ctx)) = app_with_session(&dir, &[]) else { return };
    app.agents.tabs[0].session.say("bash");
    until(&mut app, &ctx, "the request", |app| !session(app).waiting().is_empty());
    assert!(app.review_tabs().is_empty(), "a command has no diff to review");
    assert!(crate::files_changing::claimed_by(&dir.join("made-by-the-command.txt")).is_some(), "a file the command makes is put down to the agent");
    let id = session(&app).waiting()[0].id.clone();
    app.agents.tabs[0].session.answer(&id, Answer::Allow { text: None, for_session: false });
    until(&mut app, &ctx, "the end", |app| !session(app).busy);
}

#[test]
fn closing_a_session_closes_its_open_review_and_stops_the_program() {
    let dir = folder("close");
    let file = dir.join("a.txt");
    std::fs::write(&file, "one\ntwo\n").unwrap();
    let Some((mut app, ctx)) = app_with_session(&dir, &[]) else { return };
    app.agents.tabs[0].session.say(&format!("edit {}", file.display()));
    until(&mut app, &ctx, "the review", |app| !app.review_tabs().is_empty());
    app.agent_close(0);
    assert!(app.agents.tabs.is_empty());
    assert!(app.review_tabs().is_empty());
    assert_eq!(std::fs::read_to_string(&file).unwrap(), "one\ntwo\n");
}

#[test]
fn a_past_session_is_taken_up_again_with_its_transcript_and_resume() {
    let dir = folder("resume");
    let Some(options) = fake_options(&dir, &[]) else { return };
    let config = folder("resume-config");
    let logs = config.join("projects").join(crate::agent_logs::folder_name(&dir));
    std::fs::create_dir_all(&logs).unwrap();
    std::fs::write(logs.join("old-session.jsonl"), concat!(r#"{"parentUuid":null,"type":"user","message":{"role":"user","content":"Fix the parser"}}"#, "\n", r#"{"type":"assistant","message":{"id":"m1","content":[{"type":"text","text":"Fixed."}]}}"#, "\n")).unwrap();
    let past = crate::agent_logs::list(&config, &dir);
    assert_eq!(past.len(), 1);
    let (mut app, ctx) = app_with("");
    app.agents.program = (options.program, options.prefix);
    app.agent_start(&ctx, &dir, &Choice::default(), Some(&past[0])).unwrap();
    assert_eq!(session(&app).title, "Fix the parser");
    assert_eq!(session(&app).items.len(), 2, "what was said is shown at once");
    app.agents.tabs[0].session.say("hello");
    until(&mut app, &ctx, "a reply", |app| !session(app).busy && session(app).items.len() > 3);
    let argv = std::fs::read_to_string(dir.join("log.txt")).unwrap();
    assert!(argv.contains("--resume") && argv.contains("old-session"), "{argv}");
}

/// An app with a Codex session running on its stand-in, in `dir`.
fn app_with_codex(dir: &Path, extra: &[&str]) -> Option<(Limoniter, Context)> {
    let options = crate::agent_codex::agent_codex_tests::fake_options(dir, extra)?;
    let (mut app, ctx) = app_with("");
    app.agents.codex_program = (options.program, options.prefix);
    app.agent_start(&ctx, dir, &Choice { tool: Tool::Codex, ..Choice::default() }, None).unwrap();
    Some((app, ctx))
}

#[test]
fn a_codex_edit_opens_the_review_and_what_the_person_changed_is_what_is_left() {
    let dir = folder("codex-edit");
    let file = dir.join("a.txt");
    std::fs::write(&file, "one\ntwo\nthree\n").unwrap();
    let Some((mut app, ctx)) = app_with_codex(&dir, &[]) else { return };
    assert_eq!(session(&app).tool, Tool::Codex);
    app.agents.tabs[0].session.say(&format!("edit {}", file.display()));
    until(&mut app, &ctx, "the review", |app| !app.review_tabs().is_empty());
    let tab = review_tab(&app);
    assert_eq!(app.review_proposed_text(tab).unwrap(), "one\nTWO\nthree\n", "the whole file, worked out from Codex's patch");
    assert!(crate::files_changing::claimed_by(&file).is_some_and(|who| who.starts_with("Codex")), "the file is put down to Codex");
    // Draws, with the request waiting on the review and Codex's own words for the mode.
    app.panels.agents = true;
    draw(&mut app, &ctx, Vec::new());
    app.review_change_proposed(tab, "one\nMINE\nthree\n");
    app.decide_review(tab, true);
    until(&mut app, &ctx, "the edit", |app| !session(app).busy && std::fs::read_to_string(&file).unwrap() == "one\nMINE\nthree\n" && session(app).items.iter().any(|i| matches!(i, Item::Notice(_))));
    assert!(app.review_tabs().is_empty());
}

#[test]
fn rejecting_a_codex_edit_declines_it() {
    let dir = folder("codex-reject");
    let file = dir.join("a.txt");
    std::fs::write(&file, "one\ntwo\nthree\n").unwrap();
    let Some((mut app, ctx)) = app_with_codex(&dir, &[]) else { return };
    app.agents.tabs[0].session.say(&format!("edit {}", file.display()));
    until(&mut app, &ctx, "the review", |app| !app.review_tabs().is_empty());
    let tab = review_tab(&app);
    app.decide_review(tab, false);
    until(&mut app, &ctx, "the refusal", |app| !session(app).busy && session(app).items.iter().any(|i| matches!(i, Item::Text { text, .. } if text == "refused")));
    assert_eq!(std::fs::read_to_string(&file).unwrap(), "one\ntwo\nthree\n");
}

#[test]
fn a_codex_command_is_asked_in_the_panel_and_marks_the_folder() {
    let dir = folder("codex-command");
    let Some((mut app, ctx)) = app_with_codex(&dir, &[]) else { return };
    app.agents.tabs[0].session.say("command one");
    until(&mut app, &ctx, "the request", |app| !session(app).waiting().is_empty());
    assert!(app.review_tabs().is_empty());
    assert!(crate::files_changing::claimed_by(&dir.join("b.txt")).is_some_and(|who| who.starts_with("Codex")));
    let id = session(&app).waiting()[0].id.clone();
    app.agents.tabs[0].session.answer(&id, Answer::Allow { text: None, for_session: false });
    until(&mut app, &ctx, "the end", |app| !session(app).busy);
}

#[test]
fn past_codex_sessions_are_listed_beside_claudes_and_taken_up_with_thread_resume() {
    let dir = folder("codex-resume");
    let Some(options) = crate::agent_codex::agent_codex_tests::fake_options(&dir, &[]) else { return };
    let home = folder("codex-resume-home");
    let day = home.join("sessions/2026/11/27");
    std::fs::create_dir_all(&day).unwrap();
    let meta = format!(r#"{{"type":"session_meta","payload":{{"id":"old-thread","cwd":"{}","originator":"codex_cli_rs"}}}}"#, dir.display().to_string().replace('\\', "\\\\"));
    let said = r#"{"type":"event_msg","payload":{"type":"item_completed","item":{"type":"UserMessage","id":"u","content":[{"type":"text","text":"Fix the parser"}]}}}"#;
    let answered = r#"{"type":"event_msg","payload":{"type":"item_completed","item":{"type":"AgentMessage","id":"a","content":[{"type":"Text","text":"Fixed."}]}}}"#;
    std::fs::write(day.join("rollout-2026-11-27T10-00-00-old-thread.jsonl"), format!("{meta}\n{said}\n{answered}\n")).unwrap();
    let past = crate::agent_codex_logs::list(&home, &dir);
    assert_eq!(past.len(), 1);
    assert_eq!(past[0].tool, Tool::Codex);
    let (mut app, ctx) = app_with("");
    app.agents.codex_program = (options.program, options.prefix);
    app.agent_start(&ctx, &dir, &Choice { tool: Tool::Codex, ..Choice::default() }, Some(&past[0])).unwrap();
    assert_eq!(session(&app).title, "Fix the parser");
    assert_eq!(session(&app).items.len(), 2, "what was said is shown at once, from the log");
    until(&mut app, &ctx, "the thread", |app| session(app).session_id.as_deref() == Some("old-thread") && !session(app).models.is_empty());
    let log = std::fs::read_to_string(dir.join("log.txt")).unwrap();
    assert!(log.contains("thread/resume") && log.contains("old-thread"), "{log}");
}

#[test]
fn the_box_for_a_new_session_offers_both_tools_and_codexs_own_modes() {
    let (mut app, ctx) = app_with("");
    // The real programs aren't started to ask them for their models.
    app.agents.probed = true;
    app.agents.codex_probed = true;
    app.agent_new_session_box();
    draw(&mut app, &ctx, Vec::new());
    app.agents.dialog.as_mut().unwrap().tool = Tool::Codex;
    app.agents.dialog.as_mut().unwrap().mode = Mode::Bypass;
    draw(&mut app, &ctx, Vec::new());
    assert_eq!(Choice::default().tool, Tool::Claude);
}

#[test]
fn the_panel_and_the_box_for_a_new_session_draw_with_a_made_up_conversation() {
    let (mut app, ctx) = app_with("");
    app.agents_screenshot_step("");
    assert_eq!(session(&app).waiting().len(), 1);
    assert_eq!(session(&app).running_tasks().len(), 1);
    draw(&mut app, &ctx, Vec::new());
    draw(&mut app, &ctx, Vec::new());
    app.agents_screenshot_step("top");
    assert!(session(&app).waiting().is_empty());
    draw(&mut app, &ctx, Vec::new());
    app.agent_new_session_box();
    assert!(app.agents.dialog.is_some());
    draw(&mut app, &ctx, Vec::new());
    app.agents.dialog.as_mut().unwrap().mode = Mode::Bypass;
    draw(&mut app, &ctx, Vec::new());
}

#[test]
fn the_files_an_at_can_name_skip_what_nobody_would_mention() {
    let dir = folder("files");
    std::fs::create_dir_all(dir.join("src")).unwrap();
    std::fs::create_dir_all(dir.join("target/debug")).unwrap();
    std::fs::create_dir_all(dir.join(".git")).unwrap();
    std::fs::write(dir.join("src/main.rs"), "").unwrap();
    std::fs::write(dir.join("README.md"), "").unwrap();
    std::fs::write(dir.join("target/debug/x"), "").unwrap();
    std::fs::write(dir.join(".git/config"), "").unwrap();
    let mut files = project_files(&dir);
    files.sort();
    assert_eq!(files, vec!["README.md", "src/main.rs"]);
}

#[test]
fn the_panel_is_remembered_in_a_saved_layout() {
    let mut panels = crate::panels::Panels::default();
    assert!(!panels.agents);
    panels.agents = true;
    let text = panels.open_ones();
    assert!(text.contains("agents_panel=true"));
}

/// By hand only (it uses the person's tokens): the real `claude` on the cheapest model, one tiny edit through the
/// review. Run with `LIMONITER_REAL_CLAUDE=FOLDER cargo test --release --offline real_claude -- --ignored --nocapture`,
/// FOLDER being a scratch folder with an `a.txt` in it holding the lines one and two.
#[test]
#[ignore]
fn real_claude_edits_a_file_through_the_review() {
    let dir = PathBuf::from(std::env::var("LIMONITER_REAL_CLAUDE").expect("a scratch folder"));
    let file = dir.join("a.txt");
    let (mut app, ctx) = app_with("");
    let choice = Choice { model: "haiku".into(), ..Choice::default() };
    app.agent_start(&ctx, &dir, &choice, None).unwrap();
    app.agents.tabs[0].session.say("Use the Edit tool (not Bash) to change 'two' to 'three' in a.txt. Then say done.");
    until(&mut app, &ctx, "the review", |app| !app.review_tabs().is_empty());
    let tab = review_tab(&app);
    eprintln!("PROPOSED {:?}", app.review_proposed_text(tab));
    app.decide_review(tab, true);
    until(&mut app, &ctx, "the end", |app| !session(app).busy && session(app).context.is_some());
    std::thread::sleep(Duration::from_millis(1500));
    app.agents_tick(&ctx);
    let s = session(&app);
    eprintln!("FILE {:?}", std::fs::read_to_string(&file));
    eprintln!("CLAIM {:?}", crate::files_changing::claimed_by(&file));
    eprintln!("TOKENS {:?} COST {:?} CONTEXT {:?} LIMITS {:?} MODEL {:?} TITLE {:?}", s.tokens, s.cost, s.context, s.limits, s.model, s.title);
    for item in &s.items {
        eprintln!("ITEM {item:?}");
    }
}

/// By hand only (it uses the person's tokens): the real `codex` on its cheapest model, one tiny edit through the
/// review, with Limoniter's MCP server offered to it. Run with `LIMONITER_REAL_CODEX=FOLDER cargo test --release
/// --offline real_codex -- --ignored --nocapture`, FOLDER being a scratch folder with an `a.txt` in it holding the
/// lines one, two and three.
#[test]
#[ignore]
fn real_codex_edits_a_file_through_the_review_and_asks_limoniters_tools() {
    let dir = PathBuf::from(std::env::var("LIMONITER_REAL_CODEX").expect("a scratch folder"));
    let file = dir.join("a.txt");
    let (mut app, ctx) = app_with("");
    app.prefs.claude_ide = true;
    let options = crate::claude_ide::Options { lock_dir: None, folders: vec![dir.display().to_string()], running_in_windows: false, also_on: Vec::new(), wake: Arc::new(|| {}) };
    app.claude_ide_use(crate::claude_ide::Server::start(options).unwrap());
    app.open_path(&file);
    let choice = Choice { tool: Tool::Codex, model: "gpt-5.6-luna".into(), effort: "low".into(), ..Choice::default() };
    app.agent_start(&ctx, &dir, &choice, None).unwrap();
    app.agents.tabs[0].session.say("First call the getOpenEditors tool of the limoniter MCP server and tell me the label of the active file. Then use apply_patch (not the shell) to change the line two to TWO in a.txt.");
    let began = std::time::Instant::now();
    while app.review_tabs().is_empty() {
        app.claude_ide_tick(&ctx);
        app.agents_tick(&ctx);
        // A command it chooses to run first (to look at the file) is allowed.
        let commands: Vec<String> = session(&app).waiting().iter().filter(|p| matches!(p.kind, PermissionKind::Command(_))).map(|p| p.id.clone()).collect();
        for id in commands {
            app.agents.tabs[0].session.answer(&id, Answer::Allow { text: None, for_session: false });
        }
        assert!(began.elapsed() < Duration::from_secs(120), "no review; transcript {:?}", session(&app).items);
        std::thread::sleep(Duration::from_millis(20));
    }
    let tab = review_tab(&app);
    eprintln!("PROPOSED {:?}", app.review_proposed_text(tab));
    app.decide_review(tab, true);
    while session(&app).busy || session(&app).context.is_none() {
        app.claude_ide_tick(&ctx);
        app.agents_tick(&ctx);
        assert!(began.elapsed() < Duration::from_secs(240), "not finished; transcript {:?}", session(&app).items);
        std::thread::sleep(Duration::from_millis(20));
    }
    let s = session(&app);
    eprintln!("FILE {:?}", std::fs::read_to_string(&file));
    eprintln!("CLAIM {:?}", crate::files_changing::claimed_by(&file));
    eprintln!("TOKENS {:?} CONTEXT {:?} LIMITS {:?} MODEL {:?} TITLE {:?} MODE {:?}", s.tokens, s.context, s.limits, s.model, s.title, s.mode);
    for item in &s.items {
        eprintln!("ITEM {item:?}");
    }
}
