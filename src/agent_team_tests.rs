//! The team's rules as sessions meet them: claims refusing an edit, a command's changes reported, builds in a
//! queue, and the task board reached through MCP by Claude Code and by Codex (the stand-ins for both).

use super::*;
use crate::agent_session::{Answer, Command, Decided, Event, Handle, Item, Mode, Permission, PermissionKind, Session, Tool, ToolKind};
use std::sync::Arc;
use std::sync::mpsc::{Receiver, Sender, channel};

/// Tests that use the one build queue take turns, since the queue is the program's own and there is only one.
pub(crate) static BUILD_LOCK: Mutex<()> = Mutex::new(());

fn dir(tag: &str) -> PathBuf {
    let path = std::env::temp_dir().join(format!("limoniter-team-it-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&path);
    std::fs::create_dir_all(&path).unwrap();
    path
}

/// A session with no program behind it, on the team under this name.
fn member(key: &str, tool: Tool) -> (Session, Sender<Event>, Receiver<Command>) {
    let (commands, command_rx) = channel();
    let (event_tx, events) = channel();
    let mut session = Session::new(tool, Path::new("/p"), Handle { commands, events });
    session.key = key.to_string();
    team().add_member(key, tool.name(), "");
    (session, event_tx, command_rx)
}

fn permission(id: &str, kind: PermissionKind) -> Event {
    Event::Permission(Permission { id: id.into(), tool: "x".into(), title: "t".into(), detail: String::new(), kind, can_remember: true })
}

fn commands(rx: &Receiver<Command>) -> Vec<Command> {
    rx.try_iter().filter(|c| !matches!(c, Command::AskContext | Command::AskUsage)).collect()
}

fn notices(session: &Session) -> Vec<String> {
    session.items.iter().filter_map(|i| if let Item::Notice(t) = i { Some(t.clone()) } else { None }).collect()
}

#[test]
fn an_edit_of_a_claimed_file_is_refused_saying_who_has_it_and_the_person_is_not_asked() {
    let d = dir("refuse");
    team().add_member("Holder 1", "Codex", "Parser");
    team().claim_paths("Holder 1", &[d.join("src").display().to_string()]).unwrap();
    let (mut s, tx, rx) = member("Runner 1", Tool::Claude);
    tx.send(permission("p1", PermissionKind::Edit { path: d.join("src/a.rs"), proposed: "x".into() })).unwrap();
    tx.send(permission("p2", PermissionKind::Edit { path: d.join("other.rs"), proposed: "x".into() })).unwrap();
    let seen = s.pump();
    assert_eq!(seen.len(), 1, "only the unclaimed file's request reaches the window");
    assert!(matches!(&seen[0], Event::Permission(p) if p.id == "p2"));
    let sent = commands(&rx);
    assert!(matches!(&sent[..], [Command::Answer { id, answer: Answer::Refuse(why) }] if id == "p1" && why.contains("Holder 1 (Parser)") && why.contains("leave_note")), "{sent:?}");
    assert!(s.items.iter().any(|i| matches!(i, Item::Permission { permission, decided: Decided::Refused } if permission.id == "p1")));
    assert!(notices(&s).iter().any(|n| n.contains("Holder 1") && n.contains("a.rs")), "{:?}", notices(&s));
    assert_eq!(s.waiting().len(), 1);
    // What it holds itself it may change.
    team().claim_paths("Runner 1", &[d.join("mine").display().to_string()]).unwrap();
    tx.send(permission("p3", PermissionKind::Edit { path: d.join("mine/b.rs"), proposed: "x".into() })).unwrap();
    assert_eq!(s.pump().len(), 1);
    team().remove_member("Holder 1");
}

#[test]
fn what_a_command_changes_in_another_ones_files_is_reported_to_both() {
    let d = dir("command");
    std::fs::create_dir_all(d.join("lib")).unwrap();
    std::fs::write(d.join("lib/a.txt"), "one").unwrap();
    team().add_member("Holder 2", "Claude Code", "Library");
    team().claim_paths("Holder 2", &[d.join("lib").display().to_string()]).unwrap();
    let (mut holder, _tx, _rx) = member("Holder 2", Tool::Claude);
    let (mut s, tx, _rx2) = member("Runner 2", Tool::Codex);
    tx.send(Event::ToolUse { id: "c1".into(), name: "Bash".into(), summary: "Ran make".into(), kind: ToolKind::Command("make".into()), parent: None }).unwrap();
    s.pump();
    std::fs::write(d.join("lib/a.txt"), "one and more").unwrap();
    tx.send(Event::ToolResult { id: "c1".into(), text: "ok".into(), is_error: false }).unwrap();
    s.pump();
    assert!(notices(&s).iter().any(|n| n.contains("Runner 2") && n.contains("a.txt") && n.contains("Holder 2")), "{:?}", notices(&s));
    holder.pump();
    assert!(notices(&holder).iter().any(|n| n.contains("Runner 2") && n.contains("which you have claimed")), "{:?}", notices(&holder));
    // A file change made where nothing asked (a mode that doesn't) is said afterwards, unless it failed.
    tx.send(Event::ToolUse { id: "f1".into(), name: "Edit".into(), summary: "Edit".into(), kind: ToolKind::FileChange(d.join("lib/a.txt")), parent: None }).unwrap();
    tx.send(Event::ToolResult { id: "f1".into(), text: "no".into(), is_error: true }).unwrap();
    let before = notices(&s).len();
    s.pump();
    assert_eq!(notices(&s).len(), before, "a refused change isn't reported as made");
    tx.send(Event::ToolUse { id: "f2".into(), name: "Edit".into(), summary: "Edit".into(), kind: ToolKind::FileChange(d.join("lib/a.txt")), parent: None }).unwrap();
    tx.send(Event::ToolResult { id: "f2".into(), text: "ok".into(), is_error: false }).unwrap();
    s.pump();
    assert_eq!(notices(&s).len(), before + 1);
}

#[test]
fn a_build_waits_for_another_sessions_and_both_transcripts_say_so() {
    let _only = BUILD_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let (mut a, atx, arx) = member("Builder A", Tool::Claude);
    let (mut b, btx, brx) = member("Builder B", Tool::Codex);
    atx.send(permission("pa", PermissionKind::Command("CARGO_BUILD_JOBS=4 cargo test --release".into()))).unwrap();
    btx.send(permission("pb", PermissionKind::Command("cargo build".into()))).unwrap();
    btx.send(permission("pc", PermissionKind::Command("ls -la".into()))).unwrap();
    a.pump();
    b.pump();
    a.answer("pa", Answer::Allow { text: None, for_session: false });
    b.answer("pb", Answer::Allow { text: None, for_session: false });
    b.answer("pc", Answer::Allow { text: None, for_session: false });
    assert_eq!(commands(&arx).len(), 1, "the first goes at once");
    let sent = commands(&brx);
    assert_eq!(sent.len(), 1, "the second waits, and a command that isn't a build doesn't: {sent:?}");
    assert!(matches!(&sent[0], Command::Answer { id, .. } if id == "pc"));
    a.pump();
    b.pump();
    assert!(notices(&b).iter().any(|n| n == "Waiting for Builder A's cargo test"), "{:?}", notices(&b));
    assert!(notices(&a).iter().any(|n| n.contains("Builder B is waiting to run cargo build")), "{:?}", notices(&a));
    assert!(matches!(&b.items[..], [Item::Permission { decided: Decided::Allowed { .. }, .. }, ..]), "it shows as allowed while it waits");
    // The first finishes: the second's answer goes.
    atx.send(Event::ToolUse { id: "t1".into(), name: "Bash".into(), summary: "Ran".into(), kind: ToolKind::Command("cargo test".into()), parent: None }).unwrap();
    atx.send(Event::ToolResult { id: "t1".into(), text: "ok".into(), is_error: false }).unwrap();
    a.pump();
    b.pump();
    let sent = commands(&brx);
    assert!(matches!(&sent[..], [Command::Answer { id, answer: Answer::Allow { .. } }] if id == "pb"), "{sent:?}");
    // And it holds the queue until it has finished too.
    assert!(team().queue_rows().iter().any(|r| r.running && r.who.starts_with("Builder B")));
    btx.send(Event::TurnFinished).unwrap();
    b.pump();
    assert!(team().queue_rows().is_empty());
}

#[test]
fn stopping_a_session_drops_the_builds_it_was_waiting_to_run() {
    let _only = BUILD_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let (mut a, atx, _arx) = member("Stopper A", Tool::Claude);
    let (mut b, btx, brx) = member("Stopper B", Tool::Claude);
    atx.send(permission("pa", PermissionKind::Command("npm test".into()))).unwrap();
    btx.send(permission("pb", PermissionKind::Command("pytest".into()))).unwrap();
    a.pump();
    b.pump();
    a.answer("pa", Answer::Allow { text: None, for_session: false });
    b.answer("pb", Answer::Allow { text: None, for_session: false });
    assert!(commands(&brx).is_empty());
    b.stop();
    let sent = commands(&brx);
    assert!(matches!(&sent[..], [Command::Answer { answer: Answer::Refuse(_), .. }]), "{sent:?}");
    assert_eq!(team().queue_rows().iter().filter(|r| r.who.starts_with("Stopper B")).count(), 0);
    drop(a);
    assert!(team().queue_rows().is_empty(), "a session that ends frees the queue");
}

// ---------- The stand-ins, and Limoniter's own server ----------

fn server() -> crate::claude_ide::Server {
    crate::claude_ide::Server::start(crate::claude_ide::Options { lock_dir: None, folders: Vec::new(), running_in_windows: false, also_on: Vec::new(), wake: Arc::new(|| {}) }).unwrap()
}

fn mcp_for(server: &crate::claude_ide::Server, who: &str) -> crate::agent_codex::Mcp {
    crate::agent_codex::Mcp { url: format!("http://127.0.0.1:{}/mcp", server.port), token: server.token.clone(), who: who.to_string() }
}

fn texts(session: &Session) -> Vec<String> {
    session.items.iter().filter_map(|i| if let Item::Text { text, .. } = i { Some(text.clone()) } else { None }).collect()
}

fn wait(session: &mut Session, what: &str, done: impl Fn(&Session) -> bool) {
    let began = std::time::Instant::now();
    while !done(session) {
        session.pump();
        assert!(began.elapsed() < Duration::from_secs(30), "waited for {what}; the transcript is {:?}", session.items);
        std::thread::sleep(Duration::from_millis(10));
    }
}

/// Says something and waits for the whole of the answer.
fn say(session: &mut Session, text: &str) -> String {
    let before = texts(session).len();
    session.say(text);
    wait(session, text, |s| !s.busy && texts(s).len() > before);
    texts(session).last().cloned().unwrap()
}

fn claude(dir: &Path, key: &str, mcp: Option<crate::agent_codex::Mcp>) -> Option<Session> {
    let mut options = crate::agent_claude::agent_claude_tests::fake_options(dir, &[])?;
    options.mcp = mcp;
    let mut session = Session::new(Tool::Claude, dir, crate::agent_claude::start(options).unwrap());
    session.key = key.to_string();
    team().add_member(key, "Claude Code", "");
    Some(session)
}

fn codex(dir: &Path, key: &str, mcp: Option<crate::agent_codex::Mcp>) -> Option<Session> {
    let mut options = crate::agent_codex::agent_codex_tests::fake_options(dir, &[])?;
    options.mcp = mcp;
    let mut session = Session::new(Tool::Codex, dir, crate::agent_codex::start(options).unwrap());
    session.key = key.to_string();
    team().add_member(key, "Codex", "");
    Some(session)
}

/// Every tool of the board, called over MCP by a session, and what it left on the board.
fn use_the_board(mut s: Session, key: &str, tag: &str, d: &Path) {
    let title = format!("Lex it {tag}");
    let added = say(&mut s, &format!("board add_task {}", json!({ "title": title, "details": "in src", "assignee": key })));
    let id: u32 = added.trim_start_matches("Added task ").trim_end_matches('.').parse().unwrap_or_else(|_| panic!("{added}"));
    let listed = say(&mut s, "board list_tasks {}");
    assert!(listed.contains(&title) && listed.contains(&format!("\"you_are\": \"{key}\"")), "the board knows who asks: {listed}");
    assert!(say(&mut s, &format!("board claim_task {}", json!({ "id": id }))).contains("is yours"));
    let path = d.join("src").display().to_string();
    assert!(say(&mut s, &format!("board claim_paths {}", json!({ "paths": [path.clone()] }))).starts_with("Claimed."));
    assert!(say(&mut s, &format!("board leave_note {}", json!({ "to": "you", "text": format!("note from {tag}") }))).starts_with("Note left"));
    assert!(say(&mut s, "board read_notes {}").contains("notes"));
    assert!(say(&mut s, &format!("board finish_task {}", json!({ "id": id, "notes": "done it" }))).contains("done"));
    assert!(say(&mut s, &format!("board release_paths {}", json!({ "paths": [path] }))).contains("Released"));
    // What it left on the board (read back through the tool, since other tests share the program's one board).
    let listed = say(&mut s, "board list_tasks {}");
    assert!(listed.contains(&title) && listed.contains("\"status\": \"done\"") && listed.contains("done it") && listed.contains(&format!("\"assignee\": \"{key}\"")), "{listed}");
}

#[test]
fn claude_code_uses_every_tool_of_the_board_through_mcp() {
    let d = dir("mcp-claude");
    let server = server();
    let Some(s) = claude(&d, "Claude Code 91", Some(mcp_for(&server, "Claude Code 91"))) else { return };
    use_the_board(s, "Claude Code 91", "claude", &d);
}

#[test]
fn codex_uses_every_tool_of_the_board_through_mcp() {
    let d = dir("mcp-codex");
    let server = server();
    let Some(s) = codex(&d, "Codex 91", Some(mcp_for(&server, "Codex 91"))) else { return };
    use_the_board(s, "Codex 91", "codex", &d);
}

#[test]
fn a_planner_splits_a_goal_into_tasks_on_the_board() {
    let d = dir("planner");
    let server = server();
    for (tool, key) in [(Tool::Claude, "Claude Code 92"), (Tool::Codex, "Codex 92")] {
        let mcp = Some(mcp_for(&server, key));
        let Some(mut s) = (if tool == Tool::Claude { claude(&d, key, mcp) } else { codex(&d, key, mcp) }) else { return };
        assert_eq!(say(&mut s, &planner_prompt("Make the parser faster")), "Planned.");
        let listed = say(&mut s, "board list_tasks {}");
        assert!(listed.contains("Plan step one") && listed.contains("Plan step two") && listed.contains(&format!("\"by\": \"{key}\"")), "{key}: {listed}");
    }
}

#[test]
fn a_session_the_board_does_not_know_is_still_answered_by_name() {
    let (text, failed) = call(Some("Somebody 1"), "list_tasks", &json!({}));
    assert!(!failed && text.contains("Somebody 1"));
    assert!(call(None, "list_tasks", &json!({})).0.contains("Claude Code (terminal)"), "the terminal's Claude Code isn't told apart");
    assert!(call(Some("x"), "no_such_tool", &json!({})).1);
}

#[test]
fn claims_refuse_an_edit_for_claude_code_and_for_codex_and_let_the_holder_through() {
    team().add_member("Holder 3", "Codex", "Docs");
    for (tool, key) in [(Tool::Claude, "Claude Code 93"), (Tool::Codex, "Codex 93")] {
        let d = dir(&format!("edit-{key}").replace(' ', "-"));
        std::fs::create_dir_all(d.join("docs")).unwrap();
        let file = d.join("docs/a.txt");
        std::fs::write(&file, "one\ntwo\nthree\n").unwrap();
        team().claim_paths("Holder 3", &[d.join("docs").display().to_string()]).unwrap();
        let Some(mut s) = (if tool == Tool::Claude { claude(&d, key, None) } else { codex(&d, key, None) }) else { team().remove_member("Holder 3"); return };
        s.say(&format!("edit {}", file.display()));
        wait(&mut s, "the end of the turn", |s| !s.busy && s.items.iter().any(|i| matches!(i, Item::Text { done: true, .. })));
        assert_eq!(std::fs::read_to_string(&file).unwrap(), "one\ntwo\nthree\n", "{key}: the file wasn't changed");
        assert!(s.items.iter().any(|i| matches!(i, Item::Permission { decided: Decided::Refused, .. })), "{key}");
        assert!(notices(&s).iter().any(|n| n.contains("Holder 3") && n.contains("a.txt")), "{key}: {:?}", notices(&s));
        let log = std::fs::read_to_string(d.join("log.txt")).unwrap();
        assert!(log.contains("claimed by Holder 3") || log.contains("decline"), "{key}: the tool was told: {log}");
        if tool == Tool::Claude {
            assert!(log.contains("claimed by Holder 3"), "Claude's deny message says who: {log}");
        }
        // Handed over, the holder's own edit goes ahead (answered here as the window would).
        team().release_paths("Holder 3", &[d.join("docs").display().to_string()]);
        team().claim_paths(key, &[d.join("docs").display().to_string()]).unwrap();
        s.say(&format!("edit {}", file.display()));
        wait(&mut s, "the request", |s| !s.waiting().is_empty());
        let id = s.waiting()[0].id.clone();
        s.answer(&id, Answer::Allow { text: None, for_session: false });
        wait(&mut s, "the end of the turn", |s| !s.busy && std::fs::read_to_string(&file).unwrap_or_default() != "one\ntwo\nthree\n");
        assert_ne!(std::fs::read_to_string(&file).unwrap(), "one\ntwo\nthree\n", "{key}: its own claim lets it change the file");
    }
    team().remove_member("Holder 3");
}

/// Run by hand, with real programs and the cheapest models: Claude Code and Codex in two sessions on one scratch
/// folder, each holding half of it, with one tiny task each through the board.
/// `LIMONITER_REAL_TEAM=<empty folder> cargo test --release --offline real_claude_and_codex -- --ignored --nocapture`
#[test]
#[ignore = "runs the real programs, which costs a little"]
fn real_claude_and_codex_share_one_folder_through_the_board() {
    let Ok(folder) = std::env::var("LIMONITER_REAL_TEAM") else { return };
    let d = PathBuf::from(folder);
    std::fs::create_dir_all(d.join("a")).unwrap();
    std::fs::create_dir_all(d.join("b")).unwrap();
    let server = server();
    let (ck, xk) = ("Claude Code 1", "Codex 1");
    {
        let mut team = team();
        team.set_folder(&d);
        team.tasks.clear();
    }
    let claude_options = crate::agent_claude::Options { program: "claude".into(), prefix: Vec::new(), folder: d.clone(), mode: Mode::Ask, model: Some("haiku".into()), effort: Some("low".into()), resume: None, ide_port: None, mcp: Some(mcp_for(&server, ck)), wsl: None, wake: Arc::new(|| {}) };
    let codex_options = crate::agent_codex::Options { program: "codex".into(), prefix: Vec::new(), folder: d.clone(), mode: Mode::Ask, model: std::env::var("LIMONITER_REAL_CODEX_MODEL").ok(), effort: Some("low".into()), resume: None, mcp: Some(mcp_for(&server, xk)), wsl: None, wake: Arc::new(|| {}) };
    let mut c = Session::new(Tool::Claude, &d, crate::agent_claude::start(claude_options).unwrap());
    let mut x = Session::new(Tool::Codex, &d, crate::agent_codex::start(codex_options).unwrap());
    c.key = ck.into();
    x.key = xk.into();
    team().add_member(ck, "Claude Code", "");
    team().add_member(xk, "Codex", "");
    team().claim_paths(ck, &["a/".into()]).unwrap();
    team().claim_paths(xk, &["b/".into()]).unwrap();
    team().add_task(PERSON, "Create a/one.txt containing the word one", "", Some(ck.into()));
    team().add_task(PERSON, "Create b/two.txt containing the word two", "", Some(xk.into()));
    let rules = "Use the list_tasks, claim_task and finish_task tools of the limoniter server for the task given to you, doing exactly what it says and nothing more. ";
    c.say(&format!("{rules}You are {ck}, working in this folder beside {xk}, who holds b/. When your task is finished, try to create b/trick.txt containing x with your Write tool (not a shell command); if that is refused, use leave_note to {xk} asking for it, and stop."));
    x.say(&format!("{rules}You are {xk}, working in this folder beside {ck}, who holds a/. When your task is finished, stop."));
    let began = std::time::Instant::now();
    while (c.busy || x.busy || began.elapsed() < Duration::from_secs(5)) && began.elapsed() < Duration::from_secs(300) {
        for s in [&mut c, &mut x] {
            s.pump();
            // The person's part: everything is allowed that wasn't refused for them.
            let waiting: Vec<String> = s.waiting().iter().map(|p| p.id.clone()).collect();
            for id in waiting {
                s.answer(&id, Answer::Allow { text: None, for_session: false });
            }
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    for s in [&c, &x] {
        println!("---- {}", s.tool.name());
        for item in &s.items {
            println!("{item:?}");
        }
    }
    {
        let team = team();
        println!("tasks: {:?}\nnotes: {:?}", team.tasks, team.notes);
    }
    assert_eq!(std::fs::read_to_string(d.join("a/one.txt")).unwrap_or_default().trim(), "one");
    assert_eq!(std::fs::read_to_string(d.join("b/two.txt")).unwrap_or_default().trim(), "two");
    assert!(!d.join("b/trick.txt").exists(), "the claim kept Claude Code out of Codex's half");
}
