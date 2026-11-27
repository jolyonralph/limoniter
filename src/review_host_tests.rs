//! Reviews of proposed changes, and Claude Code's connection to them: a `claude` (a WebSocket client here) asks
//! for a diff, the person decides in the window, and the answer goes back.

use super::super::super::tests::{app_with, draw};
use super::*;
use crate::claude_ide::{Options, Server};
use serde_json::{Value, json};
use std::sync::mpsc::channel;
use tungstenite::Message;
use tungstenite::client::IntoClientRequest;
use tungstenite::stream::MaybeTlsStream;

fn folder(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("limoniter-review-{}-{tag}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn side_lines(app: &mut Limoniter, tab: u64, side: usize) -> Vec<String> {
    app.with_compare_tab(tab, |app, tab| {
        let CompareTab::Files(files) = tab else { panic!("not a file comparison") };
        let pair = two_docs(&mut app.docs, &mut files.sources).unwrap();
        pair[side].text.all_lines().iter().map(|line| line.to_string()).collect()
    })
    .unwrap()
}

/// Edits the right side of a review as the person would, by replacing all its text.
fn edit_right(app: &mut Limoniter, tab: u64, text: &str) {
    app.with_compare_tab(tab, |app, tab| {
        let CompareTab::Files(files) = tab else { panic!("not a file comparison") };
        let pair = two_docs(&mut app.docs, &mut files.sources).unwrap();
        pair[1].text.replace_selection_or_all(text);
    });
}

fn tab_index(app: &Limoniter, tab: u64) -> Option<usize> {
    app.docs.iter().position(|doc| doc.id == tab)
}

#[test]
fn a_review_shows_the_file_beside_the_proposal_and_tells_the_decision() {
    let dir = folder("api");
    let file = dir.join("a.txt");
    std::fs::write(&file, "one\ntwo\nthree\n").unwrap();
    let (mut app, ctx) = app_with("");
    let (tab, decisions) = app.open_review(&file, "one\nTWO\nthree\n", "Edit a.txt").unwrap();
    assert_eq!(app.docs[app.active].id, tab, "in front");
    assert_eq!(app.docs[app.active].name(), "Edit a.txt");
    assert_eq!(side_lines(&mut app, tab, 0), ["one", "two", "three", ""]);
    assert_eq!(side_lines(&mut app, tab, 1), ["one", "TWO", "three", ""]);
    assert!(!app.docs[app.active].is_modified(), "closing a review isn't asked about");
    assert_eq!(app.review_tabs(), vec![tab]);
    // The left side is the file as it is, and can't be changed; the right can.
    app.with_compare_tab(tab, |app, tab| {
        let CompareTab::Files(files) = tab else { panic!() };
        let pair = two_docs(&mut app.docs, &mut files.sources).unwrap();
        assert!(pair[0].text.read_only && !pair[1].text.read_only);
        assert_eq!(files.diff.summary(), "1 difference");
    });
    draw(&mut app, &ctx, Vec::new());
    edit_right(&mut app, tab, "one\nTWO!\nthree\n");
    app.decide_review(tab, true);
    assert_eq!(decisions.try_recv(), Ok(Decision::Accepted("one\nTWO!\nthree\n".into())), "the text as it was edited");
    assert!(tab_index(&app, tab).is_none(), "the tab goes");
    assert_eq!(std::fs::read_to_string(&file).unwrap(), "one\ntwo\nthree\n", "Limoniter doesn't write the file");
    // Rejected.
    let (tab, decisions) = app.open_review(&file, "x\n", "Edit again").unwrap();
    app.decide_review(tab, false);
    assert_eq!(decisions.try_recv(), Ok(Decision::Rejected));
    // Closed with the tab's own close.
    let (tab, decisions) = app.open_review(&file, "y\n", "And again").unwrap();
    let index = tab_index(&app, tab).unwrap();
    app.close_tab(index);
    assert_eq!(decisions.try_recv(), Ok(Decision::Closed));
    assert!(tab_index(&app, tab).is_none());
    // Closed by whoever asked.
    let (tab, decisions) = app.open_review(&file, "z\n", "Last").unwrap();
    app.close_review(tab);
    assert_eq!(decisions.try_recv(), Ok(Decision::Closed));
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn the_buttons_and_keys_decide() {
    let dir = folder("keys");
    let file = dir.join("a.txt");
    std::fs::write(&file, "one\n").unwrap();
    let (mut app, ctx) = app_with("");
    for (key, expected) in [(egui::Key::A, Decision::Accepted("two\n".into())), (egui::Key::R, Decision::Rejected)] {
        let (tab, decisions) = app.open_review(&file, "two\n", "Edit a.txt").unwrap();
        let alt = egui::Modifiers::ALT;
        let events = vec![egui::Event::Key { key, physical_key: None, pressed: true, repeat: false, modifiers: alt }];
        draw(&mut app, &ctx, events);
        draw(&mut app, &ctx, Vec::new());
        assert_eq!(decisions.try_recv(), Ok(expected), "Alt+{key:?}");
        assert!(tab_index(&app, tab).is_none());
    }
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn a_new_file_is_reviewed_against_nothing_and_line_endings_are_kept() {
    let dir = folder("new");
    let (mut app, _ctx) = app_with("");
    let (tab, decisions) = app.open_review(&dir.join("new.txt"), "hello\r\nthere\r\n", "Write new.txt").unwrap();
    assert_eq!(side_lines(&mut app, tab, 0), [""], "nothing on the left");
    app.decide_review(tab, true);
    assert_eq!(decisions.try_recv(), Ok(Decision::Accepted("hello\r\nthere\r\n".into())), "as it came, down to its line endings");
    assert!(!dir.join("new.txt").exists());
    // Characters beyond ASCII come back as they went in.
    let (tab, decisions) = app.open_review(&dir.join("new.txt"), "caf\u{e9} \u{1F34B}\n", "Write").unwrap();
    app.decide_review(tab, true);
    assert_eq!(decisions.try_recv(), Ok(Decision::Accepted("caf\u{e9} \u{1F34B}\n".into())));
    // Something that isn't text can't be reviewed as text.
    let binary = dir.join("b.bin");
    std::fs::write(&binary, [0u8, 1, 2, 0, 0, 0, 0, 255, 0, 0]).unwrap();
    assert!(app.open_review(&binary, "text", "x").is_err());
    let _ = std::fs::remove_dir_all(dir);
}

// ---------- With a client ----------

type Client = tungstenite::WebSocket<MaybeTlsStream<std::net::TcpStream>>;

fn connect(server: &Server) -> Client {
    let mut request = format!("ws://127.0.0.1:{}", server.port).into_client_request().unwrap();
    request.headers_mut().insert("sec-websocket-protocol", "mcp".parse().unwrap());
    request.headers_mut().insert("x-claude-code-ide-authorization", server.token.parse().unwrap());
    let (socket, _) = tungstenite::connect(request).unwrap();
    if let MaybeTlsStream::Plain(stream) = socket.get_ref() {
        stream.set_read_timeout(Some(Duration::from_secs(20))).unwrap();
    }
    socket
}

fn send(socket: &mut Client, id: u64, method: &str, params: Value) {
    socket.send(Message::text(json!({ "jsonrpc": "2.0", "id": id, "method": method, "params": params }).to_string())).unwrap();
}

fn read(socket: &mut Client) -> Value {
    loop {
        if let Message::Text(text) = socket.read().unwrap() {
            return serde_json::from_str(&text).unwrap();
        }
    }
}

/// Reads what the client is sent, drawing the window meanwhile so that it can be answered.
fn read_drawing(app: &mut Limoniter, ctx: &Context, socket: &mut Client) -> Value {
    if let MaybeTlsStream::Plain(stream) = socket.get_ref() {
        stream.set_read_timeout(Some(Duration::from_millis(30))).unwrap();
    }
    let began = std::time::Instant::now();
    loop {
        draw(app, ctx, Vec::new());
        match socket.read() {
            Ok(Message::Text(text)) => return serde_json::from_str(&text).unwrap(),
            Ok(_) => {}
            Err(tungstenite::Error::Io(e)) if matches!(e.kind(), std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut) => {}
            Err(e) => panic!("{e}"),
        }
        assert!(began.elapsed() < Duration::from_secs(20), "nothing came");
    }
}

fn serve(dir: &Path) -> Server {
    Server::start(Options { lock_dir: Some(dir.join("ide")), folders: vec![dir.display().to_string()], running_in_windows: false, also_on: Vec::new(), wake: Arc::new(|| {}) }).unwrap()
}

/// Draws the window until `done` says so.
fn draw_until(app: &mut Limoniter, ctx: &Context, what: &str, mut done: impl FnMut(&mut Limoniter) -> bool) {
    let began = std::time::Instant::now();
    while !done(app) {
        draw(app, ctx, Vec::new());
        assert!(began.elapsed() < Duration::from_secs(20), "never: {what}");
        std::thread::sleep(Duration::from_millis(5));
    }
}

/// A client on a thread of its own, since the window has to be drawn for it to be answered.
fn on_a_thread(mut socket: Client, id: u64, method: &str, params: Value) -> std::sync::mpsc::Receiver<Value> {
    let (tx, rx) = channel();
    let method = method.to_string();
    std::thread::spawn(move || {
        send(&mut socket, id, &method, params);
        loop {
            let message = read(&mut socket);
            if message.get("id") == Some(&json!(id)) {
                let _ = tx.send(message);
                return;
            }
        }
    });
    rx
}

fn open_diff(file: &Path, contents: &str, tab_name: &str) -> Value {
    json!({ "name": "openDiff", "arguments": { "old_file_path": file.display().to_string(), "new_file_path": file.display().to_string(), "new_file_contents": contents, "tab_name": tab_name } })
}

#[test]
fn claude_code_asks_for_a_diff_and_the_person_accepts_it_edited() {
    let dir = folder("accept");
    let file = dir.join("a.txt");
    std::fs::write(&file, "one\ntwo\n").unwrap();
    let (mut app, ctx) = app_with("");
    let server = serve(&dir);
    let lock = server.lock_path().unwrap().to_path_buf();
    let socket = connect(&server);
    app.claude_ide_use(server);
    let reply = on_a_thread(socket, 5, "tools/call", open_diff(&file, "one\nTWO\n", "\u{273B} [Claude Code] a.txt (ab12cd) \u{29C9}"));
    draw_until(&mut app, &ctx, "the diff to open", |app| !app.review_tabs().is_empty());
    let tab = app.review_tabs()[0];
    assert_eq!(app.docs[app.active].id, tab);
    assert_eq!(app.docs[app.active].name(), "\u{273B} [Claude Code] a.txt (ab12cd) \u{29C9}", "named as Claude Code names it");
    assert!(reply.try_recv().is_err(), "nothing is said while the person decides");
    assert!(app.claude_connected());
    assert!(app.claude_ide_status().is_some(), "the status bar says so");
    edit_right(&mut app, tab, "one\nTWO (as I like it)\n");
    app.decide_review(tab, true);
    draw_until(&mut app, &ctx, "the answer", |_| reply.try_recv().map(|answer| { assert_eq!(answer["result"]["content"], json!([{ "type": "text", "text": "FILE_SAVED" }, { "type": "text", "text": "one\nTWO (as I like it)\n" }])); }).is_ok());
    assert!(app.review_tabs().is_empty());
    assert_eq!(std::fs::read_to_string(&file).unwrap(), "one\ntwo\n", "the CLI writes the file, not Limoniter");
    app.claude_ide_stop();
    assert!(!lock.exists(), "the lock file goes when Limoniter stops it");
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn rejecting_and_closing_are_told_and_a_client_going_closes_its_diffs() {
    let dir = folder("reject");
    let file = dir.join("a.txt");
    std::fs::write(&file, "one\n").unwrap();
    let (mut app, ctx) = app_with("");
    let server = serve(&dir);
    let (first, second) = (connect(&server), connect(&server));
    app.claude_ide_use(server);
    // Rejected.
    let reply = on_a_thread(first, 1, "tools/call", open_diff(&file, "two\n", "T1"));
    draw_until(&mut app, &ctx, "the diff to open", |app| !app.review_tabs().is_empty());
    let tab = app.review_tabs()[0];
    app.decide_review(tab, false);
    draw_until(&mut app, &ctx, "the answer", |_| reply.try_recv().map(|a| assert_eq!(a["result"]["content"], json!([{ "type": "text", "text": "DIFF_REJECTED" }, { "type": "text", "text": "T1" }]))).is_ok());
    // The tab closed by the person without a decision.
    let mut socket = second;
    send(&mut socket, 2, "tools/call", open_diff(&file, "three\n", "T2"));
    draw_until(&mut app, &ctx, "the diff to open", |app| !app.review_tabs().is_empty());
    let index = tab_index(&app, app.review_tabs()[0]).unwrap();
    app.close_tab(index);
    let answer = read_drawing(&mut app, &ctx, &mut socket);
    assert_eq!(answer["result"]["content"][0]["text"], "TAB_CLOSED");
    // A diff asked for, then the client's tool close_tab for it, then closeAllDiffTabs for two.
    send(&mut socket, 3, "tools/call", open_diff(&file, "four\n", "T3"));
    draw_until(&mut app, &ctx, "the diff to open", |app| !app.review_tabs().is_empty());
    send(&mut socket, 4, "tools/call", json!({ "name": "close_tab", "arguments": { "tab_name": "T3" } }));
    let mut told = Vec::new();
    for _ in 0..2 {
        told.push(read_drawing(&mut app, &ctx, &mut socket)["id"].as_u64().unwrap());
    }
    told.sort_unstable();
    assert_eq!(told, [3, 4], "the diff's own answer, and the close's");
    assert!(app.review_tabs().is_empty());
    for (id, name) in [(6, "A"), (7, "B")] {
        send(&mut socket, id, "tools/call", open_diff(&file, "x\n", name));
        draw_until(&mut app, &ctx, "the diff to open", |app| app.review_tabs().len() == (id - 5) as usize);
    }
    send(&mut socket, 8, "tools/call", json!({ "name": "closeAllDiffTabs", "arguments": {} }));
    let mut texts = std::collections::HashMap::new();
    for _ in 0..3 {
        let message = read_drawing(&mut app, &ctx, &mut socket);
        texts.insert(message["id"].as_u64().unwrap(), message["result"]["content"][0]["text"].as_str().unwrap().to_string());
    }
    assert_eq!(texts[&8], "CLOSED_2_DIFF_TABS");
    assert!(app.review_tabs().is_empty());
    // A client that goes away takes its diffs with it.
    send(&mut socket, 9, "tools/call", open_diff(&file, "y\n", "C"));
    draw_until(&mut app, &ctx, "the diff to open", |app| !app.review_tabs().is_empty());
    drop(socket);
    draw_until(&mut app, &ctx, "its diffs to close", |app| app.review_tabs().is_empty());
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn problems_come_from_the_language_servers_as_they_gave_them() {
    let dir = folder("problems");
    let (a, b) = (dir.join("a.rs"), dir.join("b.rs"));
    std::fs::write(&a, "fn main() {\n    let x: u32 = \"s\";\n}\n").unwrap();
    std::fs::write(&b, "fn ok() {}\n").unwrap();
    let (mut app, ctx) = app_with("");
    app.open_path(&a);
    app.open_path(&b);
    let doc = app.docs.iter_mut().find(|doc| doc.path.as_deref() == Some(a.as_path())).unwrap();
    let intel = doc.text.intel.get_or_insert_with(Box::default);
    intel.lsp_problems = vec![
        json!({ "range": { "start": { "line": 1, "character": 17 }, "end": { "line": 1, "character": 20 } }, "severity": 1, "code": "E0308", "source": "rustc", "message": "mismatched types" }),
        json!({ "range": { "start": { "line": 0, "character": 3 }, "end": { "line": 0, "character": 7 } }, "severity": 2, "source": "clippy", "message": "unused" }),
    ];
    let server = serve(&dir);
    let mut socket = connect(&server);
    app.claude_ide_use(server);
    let text_of = |message: Value| -> Value { serde_json::from_str(message["result"]["content"][0]["text"].as_str().unwrap()).unwrap() };
    // For one file, by its address.
    let uri = format!("file://{}", a.display());
    let reply = on_a_thread(socket, 1, "tools/call", json!({ "name": "getDiagnostics", "arguments": { "uri": uri } }));
    draw_until(&mut app, &ctx, "the problems", |_| reply.try_recv().map(|message| {
        let list = text_of(message);
        assert_eq!(list.as_array().unwrap().len(), 1);
        assert_eq!(list[0]["uri"], uri.as_str(), "the address as it was asked for");
        assert_eq!(list[0]["linesInFile"], 4);
        assert_eq!(list[0]["diagnostics"][0], json!({ "message": "mismatched types", "severity": "Error", "range": { "start": { "line": 1, "character": 17 }, "end": { "line": 1, "character": 20 } }, "source": "rustc", "code": "E0308" }));
        assert_eq!(list[0]["diagnostics"][1]["severity"], "Warning");
    }).is_ok());
    // For every file: only those with problems.
    socket = connect(app.claude_ide.server.as_ref().unwrap());
    let reply = on_a_thread(socket, 2, "tools/call", json!({ "name": "getDiagnostics", "arguments": {} }));
    draw_until(&mut app, &ctx, "all the problems", |_| reply.try_recv().map(|message| {
        let list = text_of(message);
        assert_eq!(list.as_array().unwrap().len(), 1, "b.rs has none");
        assert!(list[0]["uri"].as_str().unwrap().ends_with("/a.rs"));
    }).is_ok());
    // A file that has none, or isn't open, has an empty list.
    socket = connect(app.claude_ide.server.as_ref().unwrap());
    let reply = on_a_thread(socket, 3, "tools/call", json!({ "name": "getDiagnostics", "arguments": { "uri": format!("file://{}", b.display()) } }));
    draw_until(&mut app, &ctx, "b's problems", |_| reply.try_recv().map(|message| assert_eq!(text_of(message)[0]["diagnostics"], json!([]))).is_ok());
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn the_selection_is_told_as_it_changes_and_a_mention_is_sent() {
    let dir = folder("selection");
    let file = dir.join("s.txt");
    std::fs::write(&file, "caf\u{e9} \u{1F34B} one\ntwo\nthree\nfour\n").unwrap();
    let (mut app, ctx) = app_with("");
    app.open_path(&file);
    let server = serve(&dir);
    let mut socket = connect(&server);
    app.claude_ide_use(server);
    // Told soon after connecting, with nothing selected.
    let began = std::time::Instant::now();
    send(&mut socket, 1, "initialize", json!({ "protocolVersion": "2025-06-18" }));
    assert_eq!(read(&mut socket)["id"], 1);
    draw_until(&mut app, &ctx, "the first selection", |_| began.elapsed() > Duration::from_millis(600));
    let told = read(&mut socket);
    assert_eq!(told["method"], "selection_changed");
    assert_eq!(told["params"]["filePath"], file.display().to_string().as_str());
    assert_eq!(told["params"]["selection"]["isEmpty"], true);
    // Selecting the second and third lines, all in a moment, is told once, after the wait.
    app.doc().text.select_chars((1, 0), (2, 5));
    let moved = std::time::Instant::now();
    draw_until(&mut app, &ctx, "the new selection", |_| moved.elapsed() > Duration::from_millis(100));
    app.doc().text.select_chars((1, 0), (3, 2));
    let moved = std::time::Instant::now();
    draw_until(&mut app, &ctx, "the new selection to be sent", |_| moved.elapsed() > Duration::from_millis(500));
    let told = read(&mut socket);
    assert_eq!(told["params"]["text"], "two\nthree\nfo");
    assert_eq!(told["params"]["selection"], json!({ "start": { "line": 1, "character": 0 }, "end": { "line": 3, "character": 2 }, "isEmpty": false }));
    assert!(moved.elapsed() >= Duration::from_millis(300), "not before the wait");
    // Characters are counted in UTF-16, as the client counts them: the lemon takes two.
    app.doc().text.select_chars((0, 0), (0, 6));
    let moved = std::time::Instant::now();
    draw_until(&mut app, &ctx, "the new selection to be sent", |_| moved.elapsed() > Duration::from_millis(500));
    assert_eq!(read(&mut socket)["params"]["selection"]["end"]["character"], 7);
    // The mention: the file and its selected lines, from 0. A selection ending at a line's start stops before it.
    app.doc().text.select_chars((1, 0), (3, 0));
    command_mention(&mut app, &ctx);
    let told = read(&mut socket);
    assert_eq!(told["method"], "at_mentioned");
    assert_eq!(told["params"], json!({ "filePath": file.display().to_string(), "lineStart": 1, "lineEnd": 2 }));
    app.doc().text.select_chars((0, 0), (0, 0));
    command_mention(&mut app, &ctx);
    assert_eq!(read(&mut socket)["params"], json!({ "filePath": file.display().to_string() }), "no lines when none are selected");
    // Nothing is told of a file shown in hex.
    app.set_view(Some(true));
    draw(&mut app, &ctx, Vec::new());
    assert!(app.claude_current_selection().is_none());
    let _ = std::fs::remove_dir_all(dir);
}

fn command_mention(app: &mut Limoniter, ctx: &Context) {
    super::super::super::tests::command(app, ctx, Command::MentionInClaude);
}

#[test]
fn a_file_changed_by_claude_is_reloaded_without_asking_while_it_is_connected() {
    let dir = folder("reload");
    let file = dir.join("r.txt");
    std::fs::write(&file, "one\n").unwrap();
    let (mut app, _ctx) = app_with("");
    app.open_path(&file);
    let server = serve(&dir);
    let _socket = connect(&server);
    app.claude_ide_use(server);
    let began = std::time::Instant::now();
    while !app.claude_connected() {
        app.claude_ide_tick(&Context::default());
        assert!(began.elapsed() < Duration::from_secs(10));
        std::thread::sleep(Duration::from_millis(5));
    }
    std::fs::write(&file, "one\nwritten by the CLI after Accept\n").unwrap();
    let index = app.active;
    app.docs[index].disk_stamp = None;
    app.screenshot = None;
    app.check_for_changed_files();
    assert_eq!(app.docs[index].text.line(1).as_ref(), "written by the CLI after Accept", "reloaded with no question");
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn the_switch_in_settings_keeps_the_server_off() {
    let (mut app, ctx) = app_with("claude_ide=false\n");
    assert!(!app.prefs.claude_ide);
    app.claude_ide_tick(&ctx);
    assert!(app.claude_ide.server.is_none() && app.claude_ide.starting.is_none());
    let dir = folder("off");
    let server = serve(&dir);
    let lock = server.lock_path().unwrap().to_path_buf();
    app.claude_ide_use(server);
    app.claude_ide_tick(&ctx);
    assert!(app.claude_ide.server.is_none(), "turned off, it stops");
    assert!(!lock.exists());
    let _ = std::fs::remove_dir_all(dir);
}

/// By hand, with the real Claude Code (it costs a little of the person's plan, so it isn't run with the others):
///   cargo test --release --offline -- --ignored by_hand_with_the_real_claude_code --nocapture
/// Claude Code is started in a Limoniter terminal and asked, on the cheapest model, to change a file twice.
/// Its proposed changes open here as reviews; the first is accepted and the second rejected.
#[cfg(unix)]
#[test]
#[ignore = "runs the real claude, which costs tokens"]
fn by_hand_with_the_real_claude_code() {
    let dir = folder("real");
    let file = dir.join("hello.txt");
    std::fs::write(&file, "one\n").unwrap();
    let (mut app, ctx) = app_with("");
    let server = Server::start(Options { lock_dir: crate::claude_ide::lock_dir(), folders: vec![dir.display().to_string()], running_in_windows: false, also_on: Vec::new(), wake: Arc::new(|| {}) }).unwrap();
    app.claude_ide_use(server);
    let mut terminal = crate::terminal::Terminal::start_shell(&crate::terminal::Shell::Usual, &dir, 40, 120).unwrap();
    terminal.send(b"claude --model haiku --permission-mode default\r");
    let began = std::time::Instant::now();
    let mut trusted = false;
    // Until it's up and Limoniter is connected to it, saying yes to what it asks about the folder.
    while !app.claude_connected() {
        draw(&mut app, &ctx, Vec::new());
        let screen = terminal.text();
        if !trusted && screen.contains("Yes, I trust") {
            // The choice starts on "No, exit".
            std::thread::sleep(Duration::from_millis(700));
            terminal.send(b"\x1b[B");
            std::thread::sleep(Duration::from_millis(500));
            terminal.send(b"\r");
            trusted = true;
        }
        assert!(began.elapsed() < Duration::from_secs(90), "Claude Code never connected. Its screen:\n{}", terminal.text());
        std::thread::sleep(Duration::from_millis(100));
    }
    println!("connected after {:?}; the screen:\n{}", began.elapsed(), terminal.text());
    for (prompt, accept, expected) in [("Change the text in hello.txt from one to two. Nothing else.", true, "two\n"), ("Change the text in hello.txt from two to three. Nothing else.", false, "two\n")] {
        std::thread::sleep(Duration::from_secs(2));
        terminal.send(prompt.as_bytes());
        std::thread::sleep(Duration::from_millis(500));
        terminal.send(b"\r");
        let asked = std::time::Instant::now();
        while app.review_tabs().is_empty() {
            draw(&mut app, &ctx, Vec::new());
            assert!(asked.elapsed() < Duration::from_secs(120), "no review opened. The screen:\n{}", terminal.text());
            std::thread::sleep(Duration::from_millis(100));
        }
        let tab = app.review_tabs()[0];
        println!("a review opened after {:?}: {} / left {:?} right {:?}", asked.elapsed(), app.docs.iter().find(|d| d.id == tab).unwrap().name(), side_lines(&mut app, tab, 0), side_lines(&mut app, tab, 1));
        app.decide_review(tab, accept);
        let decided = std::time::Instant::now();
        while decided.elapsed() < Duration::from_secs(40) {
            draw(&mut app, &ctx, Vec::new());
            if accept && std::fs::read_to_string(&file).is_ok_and(|text| text == expected) {
                break;
            }
            std::thread::sleep(Duration::from_millis(100));
        }
        std::thread::sleep(Duration::from_secs(3));
        draw(&mut app, &ctx, Vec::new());
        println!("after {}: the file holds {:?}; the screen:\n{}", if accept { "Accept" } else { "Reject" }, std::fs::read_to_string(&file).unwrap(), terminal.text());
        assert_eq!(std::fs::read_to_string(&file).unwrap(), expected);
    }
    terminal.send(b"\x03\x03");
    app.claude_ide_stop();
    let _ = std::fs::remove_dir_all(dir);
}
