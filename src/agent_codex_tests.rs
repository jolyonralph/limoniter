//! Codex as a driver: the lines it sends and takes, checked against ones recorded from codex-cli 0.172.0, and whole
//! sessions run against a stand-in program (src/agent_testdata/fake_codex.py) that answers as it did.

use super::*;
use crate::agent_session::{Decided, Item, Session, TaskState, Tool};

/// The editor's context is one for the whole program, so the tests that set it take turns.
static CONTEXT: Mutex<()> = Mutex::new(());

fn options(folder: &str) -> Options {
    Options { program: "codex".into(), prefix: Vec::new(), folder: folder.into(), mode: Mode::Ask, model: None, effort: None, resume: None, mcp: None, wsl: None, wake: Arc::new(|| {}) }
}

fn events(state: &mut State, line: Value) -> Vec<Event> {
    state.line(&line.to_string()).0
}

/// A state past its start: the thread is going (`thread-1`) and the lines it sent are looked at.
fn started(folder: &str) -> State {
    let mut s = State::new(&options(folder));
    let first = s.initialize();
    assert!(first.contains("\"initialize\"") && first.contains("experimentalApi"), "{first}");
    let (_, lines) = s.line(r#"{"id":1,"result":{"userAgent":"x","codexHome":"/h","platformFamily":"unix","platformOs":"linux"}}"#);
    assert!(lines[0].contains("\"initialized\"") && lines[1].contains("model/list") && lines[2].contains("thread/start") && lines[3].contains("account/rateLimits/read"), "{lines:?}");
    let (_, _) = s.line(r#"{"id":3,"result":{"thread":{"id":"thread-1"},"model":"gpt-5.6-luna","approvalPolicy":"untrusted","sandbox":{"type":"workspaceWrite"}}}"#);
    s
}

fn prompt_lines(state: &mut State, text: &str) -> Vec<Value> {
    state.command(Command::Prompt(text.into())).1.iter().map(|l| serde_json::from_str(l).unwrap()).collect()
}

fn permissions(events: &[Event]) -> Vec<Permission> {
    events.iter().filter_map(|e| if let Event::Permission(p) = e { Some(p.clone()) } else { None }).collect()
}

fn temp(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("limoniter-agent-codex-{}-{tag}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

// ---------- Naming things as Codex does ----------

#[test]
fn modes_are_codexs_own_policies_and_come_back_from_what_it_reports() {
    assert_eq!(policy(Mode::Plan).1, "read-only");
    assert_eq!(policy(Mode::Ask), ("untrusted", "workspace-write", json!({"type": "workspaceWrite"})));
    assert_eq!(policy(Mode::AcceptEdits).0, "on-request");
    assert_eq!(policy(Mode::Bypass), ("never", "danger-full-access", json!({"type": "dangerFullAccess"})));
    for mode in Mode::ALL {
        let (approval, sandbox, object) = policy(mode);
        assert_eq!(mode_from(&json!(approval), &json!(sandbox)), Some(mode), "{mode:?} by name");
        assert_eq!(mode_from(&json!(approval), &object), Some(mode), "{mode:?} by object");
    }
    assert_eq!(mode_from(&json!("never"), &json!("something new")), None);
    // Codex's own words, not Claude Code's.
    assert_eq!(Mode::Plan.label_for(Tool::Codex), "Read only");
    assert_eq!(Mode::Bypass.label_for(Tool::Codex), "Full access");
    assert_eq!(Mode::Plan.label_for(Tool::Claude), Mode::Plan.label());
}

#[test]
fn the_program_is_run_as_the_app_server_and_limoniters_tools_go_in_the_threads_own_settings() {
    let mut o = options("/p");
    o.prefix = vec!["x.py".into()];
    assert_eq!(arguments(&o), vec!["x.py", "app-server"]);
    assert_eq!(thread_config(None), None);
    let mcp = Mcp { url: "http://127.0.0.1:5000/mcp".into(), token: "tok".into(), who: "Codex 1".into() };
    let config = thread_config(Some(&mcp)).unwrap();
    assert_eq!(config["mcp_servers"]["limoniter"]["url"], "http://127.0.0.1:5000/mcp");
    assert_eq!(config["mcp_servers"]["limoniter"]["http_headers"]["Authorization"], "Bearer tok");
    assert_eq!(config["mcp_servers"]["limoniter"]["http_headers"]["x-limoniter-session"], "Codex 1");
    // It goes in thread/start, not into config.toml.
    o.mcp = Some(mcp);
    let mut s = State::new(&o);
    s.initialize();
    let (_, lines) = s.line(r#"{"id":1,"result":{}}"#);
    let start: Value = serde_json::from_str(&lines[2]).unwrap();
    assert_eq!(start["params"]["config"]["mcp_servers"]["limoniter"]["url"], "http://127.0.0.1:5000/mcp");
    assert_eq!(start["params"]["approvalPolicy"], "untrusted");
    assert_eq!(start["params"]["sandbox"], "workspace-write");
}

#[test]
fn the_model_list_gives_default_first_then_the_shown_models_with_their_efforts() {
    // As codex-cli 0.172.0 gave it (cut down).
    let answer = json!({"data": [
        {"id": "gpt-6.2-sol", "model": "gpt-6.2-sol", "displayName": "GPT-6.2-Sol", "hidden": false, "isDefault": true, "supportedReasoningEfforts": [{"reasoningEffort": "low", "description": "x"}, {"reasoningEffort": "ultra", "description": "y"}], "defaultReasoningEffort": "low"},
        {"id": "gpt-reserve", "model": "gpt-reserve", "displayName": "GPT-Reserve", "hidden": true, "isDefault": false, "supportedReasoningEfforts": []},
        {"id": "gpt-5.6-luna", "model": "gpt-5.6-luna", "displayName": "GPT-5.6-Luna", "hidden": false, "isDefault": false, "supportedReasoningEfforts": [{"reasoningEffort": "low", "description": "x"}]}]});
    let models = models_from(&answer);
    let names: Vec<_> = models.iter().map(|m| (m.value.as_str(), m.name.as_str())).collect();
    assert_eq!(names, vec![("default", "Default (GPT-6.2-Sol)"), ("gpt-6.2-sol", "GPT-6.2-Sol"), ("gpt-5.6-luna", "GPT-5.6-Luna")]);
    assert_eq!(models[0].efforts, vec!["low", "ultra"], "the default's efforts");
    let mut s = State::new(&options("/p"));
    s.initialize();
    s.probe = true;
    let (_, lines) = s.line(r#"{"id":1,"result":{}}"#);
    assert_eq!(lines.len(), 2, "a probe only asks for the models: {lines:?}");
    let ready = events(&mut s, json!({"id": 2, "result": answer}));
    assert!(matches!(&ready[0], Event::Ready { models, mode: Mode::Ask } if models.len() == 3));
}

#[test]
fn starting_gives_the_thread_model_and_mode_codex_reports() {
    let mut s = State::new(&options("/p"));
    s.initialize();
    s.line(r#"{"id":1,"result":{}}"#);
    let started = events(&mut s, json!({"id": 3, "result": {"thread": {"id": "t9"}, "model": "gpt-5.6-luna", "approvalPolicy": "on-request", "sandbox": {"type": "readOnly"}, "reasoningEffort": "low"}}));
    assert_eq!(started[0], Event::Started { session_id: "t9".into(), model: "gpt-5.6-luna".into(), mode: Mode::Plan });
    assert_eq!(started[1], Event::EffortChanged("low".into()));
    // A thread that couldn't be started ends the session with the reason.
    let mut bad = State::new(&options("/p"));
    bad.initialize();
    bad.line(r#"{"id":1,"result":{}}"#);
    let failed = events(&mut bad, json!({"id": 3, "error": {"code": -1, "message": "not signed in"}}));
    assert_eq!(failed[0], Event::Error("not signed in".into()));
    assert!(matches!(&failed[1], Event::Finished(_)));
}

#[test]
fn resuming_asks_for_the_thread_without_its_turns() {
    let mut o = options("/p");
    o.resume = Some("old-thread".into());
    let mut s = State::new(&o);
    s.initialize();
    let (_, lines) = s.line(r#"{"id":1,"result":{}}"#);
    let resume: Value = serde_json::from_str(&lines[2]).unwrap();
    assert_eq!(resume["method"], "thread/resume");
    assert_eq!(resume["params"]["threadId"], "old-thread");
    assert_eq!(resume["params"]["excludeTurns"], true);
}

// ---------- The prompt ----------

#[test]
fn a_prompt_before_the_thread_is_ready_waits_and_goes_with_the_modes_policy() {
    let mut s = State::new(&options("/p"));
    s.initialize();
    s.line(r#"{"id":1,"result":{}}"#);
    assert!(s.command(Command::Prompt("hello".into())).1.is_empty(), "no thread yet");
    let (_, lines) = s.line(r#"{"id":3,"result":{"thread":{"id":"t"},"model":"m","approvalPolicy":"untrusted","sandbox":{"type":"workspaceWrite"}}}"#);
    let turn: Value = serde_json::from_str(&lines[0]).unwrap();
    assert_eq!(turn["method"], "turn/start");
    assert_eq!(turn["params"]["threadId"], "t");
    assert_eq!(turn["params"]["input"][0]["text"], "hello");
    assert_eq!(turn["params"]["approvalPolicy"], "untrusted");
    assert_eq!(turn["params"]["sandboxPolicy"]["type"], "workspaceWrite");
    assert_eq!(turn["params"]["summary"], "concise");
}

#[test]
fn model_effort_and_mode_changes_apply_to_the_next_turn() {
    let mut s = started("/p");
    let (e, _) = s.command(Command::SetModel(Some("gpt-6.2-sol".into())));
    assert_eq!(e, vec![Event::ModelChanged("gpt-6.2-sol".into())]);
    assert_eq!(s.command(Command::SetEffort("high".into())).0, vec![Event::EffortChanged("high".into())]);
    assert_eq!(s.command(Command::SetMode(Mode::Bypass)).0, vec![Event::ModeChanged(Mode::Bypass)]);
    let turn = &prompt_lines(&mut s, "go")[0];
    assert_eq!(turn["params"]["model"], "gpt-6.2-sol");
    assert_eq!(turn["params"]["effort"], "high");
    assert_eq!(turn["params"]["approvalPolicy"], "never");
    assert_eq!(turn["params"]["sandboxPolicy"]["type"], "dangerFullAccess");
}

#[test]
fn the_editors_context_goes_with_the_turn_and_falls_back_to_the_text_if_refused() {
    let _turn = CONTEXT.lock().unwrap_or_else(|e| e.into_inner());
    let mut s = started("/p");
    set_ide_context("# Context from my IDE setup:\n\n## Active file: a.rs\n".into());
    let turn = &prompt_lines(&mut s, "go")[0];
    assert_eq!(turn["params"]["additionalContext"]["ide_context"]["kind"], "untrusted");
    assert!(turn["params"]["additionalContext"]["ide_context"]["value"].as_str().unwrap().contains("Active file: a.rs"));
    // Refused: tried again with the context as text before the prompt.
    let (e, lines) = s.line(r#"{"id":5,"error":{"code":-32602,"message":"unknown field `additionalContext`"}}"#);
    assert!(e.is_empty(), "{e:?}");
    let again: Value = serde_json::from_str(&lines[0]).unwrap();
    assert!(again["params"].get("additionalContext").is_none());
    assert_eq!(again["params"]["input"].as_array().unwrap().len(), 2);
    assert!(again["params"]["input"][0]["text"].as_str().unwrap().contains("Active file: a.rs"));
    assert_eq!(again["params"]["input"][1]["text"], "go");
    set_ide_context(String::new());
}

#[test]
fn a_turn_that_cant_start_is_an_error_and_the_end_of_the_turn() {
    let mut s = started("/p");
    prompt_lines(&mut s, "go");
    let e = events(&mut s, json!({"id": 5, "error": {"code": -1, "message": "usage limit"}}));
    assert_eq!(e, vec![Event::Error("usage limit".into()), Event::TurnFinished]);
}

#[test]
fn the_ide_context_is_written_as_codexs_own_extension_writes_it() {
    let tabs = vec![("a.rs".to_string(), PathBuf::from("/p/src/a.rs")), ("b.rs".to_string(), PathBuf::from("/p/b.rs"))];
    let text = ide_context_text(Path::new("/p"), Some(Path::new("/p/src/a.rs")), "let x = 1;", &tabs);
    assert_eq!(text, "# Context from my IDE setup:\n\n## Active file: src/a.rs\n\n## Active selection of the file:\nlet x = 1;\n\n## Open tabs:\n- a.rs: src/a.rs\n- b.rs: b.rs\n");
    assert_eq!(ide_context_text(Path::new("/p"), None, "", &[]), "", "nothing open says nothing");
    let many: Vec<_> = (0..9).map(|n| (format!("f{n}"), PathBuf::from(format!("/p/f{n}")))).collect();
    assert_eq!(ide_context_text(Path::new("/p"), Some(Path::new("/p/f0")), "", &many).matches("\n- ").count(), 5, "five tabs at most");
}

// ---------- What Codex says ----------

#[test]
fn streamed_and_finished_text_and_thinking() {
    let mut s = started("/p");
    let delta = |text: &str| json!({"method": "item/agentMessage/delta", "params": {"threadId": "thread-1", "turnId": "t", "itemId": "msg1", "delta": text}});
    assert_eq!(events(&mut s, delta("Hel")), vec![Event::TextDelta { id: "msg1".into(), text: "Hel".into() }]);
    let done = events(&mut s, json!({"method": "item/completed", "params": {"threadId": "thread-1", "item": {"type": "agentMessage", "id": "msg1", "text": "Hello", "phase": "final_answer"}}}));
    assert_eq!(done, vec![Event::Text { id: "msg1".into(), text: "Hello".into(), done: true }]);
    let part = |index: u64, text: &str| json!({"method": "item/reasoning/summaryTextDelta", "params": {"threadId": "thread-1", "turnId": "t", "itemId": "rs1", "delta": text, "summaryIndex": index}});
    events(&mut s, part(0, "**Look"));
    events(&mut s, part(0, "ing**"));
    assert_eq!(events(&mut s, part(1, "Then act")), vec![Event::Thinking { id: "rs1".into(), text: "**Looking**\n\nThen act".into() }]);
    let finished = events(&mut s, json!({"method": "item/completed", "params": {"threadId": "thread-1", "item": {"type": "reasoning", "id": "rs1", "summary": ["**Looking**", "Then act"], "content": []}}}));
    assert_eq!(finished, vec![Event::Thinking { id: "rs1".into(), text: "**Looking**\n\nThen act".into() }]);
    // At low effort Codex gives no summary: nothing to show.
    assert!(events(&mut s, json!({"method": "item/completed", "params": {"threadId": "thread-1", "item": {"type": "reasoning", "id": "rs2", "summary": [], "content": []}}})).is_empty());
}

#[test]
fn a_command_item_is_a_tool_use_with_its_result() {
    let mut s = started("/p");
    let item = |status: &str, output: Value, code: Value| json!({"type": "commandExecution", "id": "c1", "command": "/bin/bash -lc 'cargo test'", "cwd": "/p", "status": status, "commandActions": [], "aggregatedOutput": output, "exitCode": code});
    let began = events(&mut s, json!({"method": "item/started", "params": {"threadId": "thread-1", "item": item("inProgress", Value::Null, Value::Null)}}));
    assert_eq!(began, vec![Event::ToolUse { id: "c1".into(), name: "Shell".into(), summary: "Ran cargo test".into(), kind: ToolKind::Command("cargo test".into()), parent: None }]);
    let ended = events(&mut s, json!({"method": "item/completed", "params": {"threadId": "thread-1", "item": item("completed", json!("ok"), json!(0))}}));
    assert_eq!(ended, vec![Event::ToolResult { id: "c1".into(), text: "ok".into(), is_error: false }]);
    let failed = events(&mut s, json!({"method": "item/completed", "params": {"threadId": "thread-1", "item": item("failed", json!(""), json!(2))}}));
    assert!(matches!(&failed[0], Event::ToolResult { text, is_error: true, .. } if text.contains("exit code 2")));
}

#[test]
fn a_shell_wrapper_is_taken_off_a_command() {
    assert_eq!(unwrap_shell("/bin/bash -lc 'touch b.txt'"), "touch b.txt");
    assert_eq!(unwrap_shell("bash -lc 'echo '\\''hi'\\'''"), "echo 'hi'");
    assert_eq!(unwrap_shell("powershell.exe -Command \"Get-ChildItem\""), "Get-ChildItem");
    assert_eq!(unwrap_shell("cargo test"), "cargo test");
    assert_eq!(unwrap_shell("python3 -c 'print(1)'"), "python3 -c 'print(1)'", "not a shell's -c");
}

#[test]
fn a_command_approval_is_asked_and_each_answer_has_its_decision() {
    // As recorded: no acceptForSession on offer, so "for this session" isn't either.
    let request = r#"{"method":"item/commandExecution/requestApproval","id":0,"params":{"kind":"command","threadId":"thread-1","turnId":"t","itemId":"c1","command":"/bin/bash -lc 'touch b.txt'","cwd":"/p","commandActions":[{"type":"unknown","command":"touch b.txt"}],"proposedExecpolicyAmendment":["touch","b.txt"],"availableDecisions":["accept",{"acceptWithExecpolicyAmendment":{"execpolicy_amendment":["touch","b.txt"]}},"cancel"]}}"#;
    let mut s = started("/p");
    let (e, replies) = s.line(request);
    assert!(replies.is_empty());
    let p = &permissions(&e)[0];
    assert_eq!(p.kind, PermissionKind::Command("touch b.txt".into()));
    assert_eq!(p.title, "Run touch b.txt");
    assert!(!p.can_remember, "Codex didn't offer it");
    let decision = |s: &mut State, answer| -> Value { serde_json::from_str(&s.command(Command::Answer { id: p.id.clone(), answer }).1[0]).unwrap() };
    assert_eq!(decision(&mut s, Answer::Allow { text: None, for_session: false }), json!({"id": 0, "result": {"decision": "accept"}}));
    s.line(request);
    assert_eq!(decision(&mut s, Answer::Refuse("no".into())), json!({"id": 0, "result": {"decision": "decline"}}));
    s.line(request);
    assert_eq!(decision(&mut s, Answer::Allow { text: None, for_session: true })["result"]["decision"], "acceptForSession");
    // Offered: it can be remembered.
    let (e, _) = s.line(&request.replace("\"accept\",{", "\"accept\",\"acceptForSession\",{"));
    assert!(permissions(&e)[0].can_remember);
}

#[test]
fn a_file_change_is_put_to_the_person_as_the_whole_file_and_accepting_is_one_decision() {
    let dir = temp("one-file");
    let file = dir.join("a.txt");
    std::fs::write(&file, "one\ntwo\nthree\n").unwrap();
    let mut s = started(dir.to_str().unwrap());
    let change = json!({"path": file.display().to_string(), "kind": {"type": "update", "move_path": null}, "diff": "@@ -1,3 +1,3 @@\n one\n-two\n+TWO\n three\n"});
    let began = events(&mut s, json!({"method": "item/started", "params": {"threadId": "thread-1", "item": {"type": "fileChange", "id": "f1", "changes": [change], "status": "inProgress"}}}));
    assert_eq!(began, vec![Event::ToolUse { id: "f1".into(), name: "Edit".into(), summary: "Edit a.txt".into(), kind: ToolKind::FileChange(file.clone()), parent: None }]);
    let (e, _) = s.line(r#"{"method":"item/fileChange/requestApproval","id":7,"params":{"threadId":"thread-1","turnId":"t","itemId":"f1","reason":null,"grantRoot":null}}"#);
    let p = &permissions(&e)[0];
    assert_eq!(p.kind, PermissionKind::Edit { path: file.clone(), proposed: "one\nTWO\nthree\n".into() });
    assert_eq!(p.title, "Edit a.txt");
    let lines = s.command(Command::Answer { id: p.id.clone(), answer: Answer::Allow { text: None, for_session: false } }).1;
    assert_eq!(serde_json::from_str::<Value>(&lines[0]).unwrap(), json!({"id": 7, "result": {"decision": "accept"}}));
    // The change made: its result, and nothing written over it.
    let done = events(&mut s, json!({"method": "item/completed", "params": {"threadId": "thread-1", "item": {"type": "fileChange", "id": "f1", "changes": [change], "status": "completed"}}}));
    assert!(matches!(&done[0], Event::ToolResult { id, is_error: false, .. } if id == "f1"));
    assert_eq!(std::fs::read_to_string(&file).unwrap(), "one\ntwo\nthree\n", "the stand-in didn't write; Codex does");
}

#[test]
fn what_the_person_edited_is_written_over_the_change_and_codex_is_told_next_time() {
    let dir = temp("edited");
    let file = dir.join("a.txt");
    std::fs::write(&file, "one\ntwo\nthree\n").unwrap();
    let mut s = started(dir.to_str().unwrap());
    let change = json!({"path": file.display().to_string(), "kind": {"type": "update", "move_path": null}, "diff": "@@ -1,3 +1,3 @@\n one\n-two\n+TWO\n three\n"});
    events(&mut s, json!({"method": "item/started", "params": {"threadId": "thread-1", "item": {"type": "fileChange", "id": "f1", "changes": [change], "status": "inProgress"}}}));
    let (e, _) = s.line(r#"{"method":"item/fileChange/requestApproval","id":7,"params":{"threadId":"thread-1","turnId":"t","itemId":"f1"}}"#);
    let id = permissions(&e)[0].id.clone();
    let lines = s.command(Command::Answer { id, answer: Answer::Allow { text: Some("one\nMINE\nthree\n".into()), for_session: false } }).1;
    assert!(lines[0].contains("\"accept\""), "Codex takes no edited text, so it's accepted as it is");
    // Codex makes its change (here, by hand), and says so.
    std::fs::write(&file, "one\nTWO\nthree\n").unwrap();
    let done = events(&mut s, json!({"method": "item/completed", "params": {"threadId": "thread-1", "item": {"type": "fileChange", "id": "f1", "changes": [change], "status": "completed"}}}));
    assert_eq!(std::fs::read_to_string(&file).unwrap(), "one\nMINE\nthree\n", "what was reviewed is what's left");
    assert!(done.iter().any(|e| matches!(e, Event::Notice(text) if text.contains("a.txt"))), "{done:?}");
    // The next prompt carries the note before the person's own words.
    let turn = &prompt_lines(&mut s, "carry on")[0];
    let input = turn["params"]["input"].as_array().unwrap();
    assert_eq!(input.len(), 2);
    assert!(input[0]["text"].as_str().unwrap().contains("edited a.txt"));
    assert_eq!(input[1]["text"], "carry on");
    let again = &prompt_lines(&mut s, "and again")[0];
    assert_eq!(again["params"]["input"].as_array().unwrap().len(), 1, "told once");
}

#[test]
fn a_change_that_was_declined_isnt_written_over() {
    let dir = temp("declined");
    let file = dir.join("a.txt");
    std::fs::write(&file, "one\ntwo\nthree\n").unwrap();
    let mut s = started(dir.to_str().unwrap());
    let change = json!({"path": file.display().to_string(), "kind": {"type": "update", "move_path": null}, "diff": "@@ -1,3 +1,3 @@\n one\n-two\n+TWO\n three\n"});
    events(&mut s, json!({"method": "item/started", "params": {"threadId": "thread-1", "item": {"type": "fileChange", "id": "f1", "changes": [change], "status": "inProgress"}}}));
    let (e, _) = s.line(r#"{"method":"item/fileChange/requestApproval","id":7,"params":{"threadId":"thread-1","turnId":"t","itemId":"f1"}}"#);
    let id = permissions(&e)[0].id.clone();
    let lines = s.command(Command::Answer { id, answer: Answer::Refuse("no".into()) }).1;
    assert!(lines[0].contains("decline"));
    let done = events(&mut s, json!({"method": "item/completed", "params": {"threadId": "thread-1", "item": {"type": "fileChange", "id": "f1", "changes": [change], "status": "declined"}}}));
    assert!(matches!(&done[0], Event::ToolResult { is_error: true, .. }));
    assert_eq!(std::fs::read_to_string(&file).unwrap(), "one\ntwo\nthree\n");
}

#[test]
fn a_patch_of_several_files_is_asked_file_by_file_and_allowed_only_if_every_file_is() {
    let dir = temp("many");
    let (a, b) = (dir.join("a.txt"), dir.join("new.txt"));
    std::fs::write(&a, "one\ntwo\nthree\n").unwrap();
    let mut s = started(dir.to_str().unwrap());
    let changes = json!([
        {"path": a.display().to_string(), "kind": {"type": "update", "move_path": null}, "diff": "@@ -1,3 +1,3 @@\n one\n-two\n+TWO\n three\n"},
        {"path": b.display().to_string(), "kind": {"type": "add"}, "diff": "hello\n"}]);
    events(&mut s, json!({"method": "item/started", "params": {"threadId": "thread-1", "item": {"type": "fileChange", "id": "f1", "changes": changes, "status": "inProgress"}}}));
    let request = r#"{"method":"item/fileChange/requestApproval","id":3,"params":{"threadId":"thread-1","turnId":"t","itemId":"f1"}}"#;
    let (e, _) = s.line(request);
    let asked = permissions(&e);
    assert_eq!(asked.len(), 2);
    assert_eq!(asked[1].kind, PermissionKind::Edit { path: b.clone(), proposed: "hello\n".into() });
    assert!(s.command(Command::Answer { id: asked[0].id.clone(), answer: Answer::Allow { text: None, for_session: false } }).1.is_empty(), "Codex is told when all are answered");
    let lines = s.command(Command::Answer { id: asked[1].id.clone(), answer: Answer::Refuse("no".into()) }).1;
    assert_eq!(serde_json::from_str::<Value>(&lines[0]).unwrap(), json!({"id": 3, "result": {"decision": "decline"}}));
    // All allowed: accepted.
    s.line(request);
    for p in &asked {
        let lines = s.command(Command::Answer { id: p.id.clone(), answer: Answer::Allow { text: None, for_session: false } }).1;
        if p.id == asked[1].id {
            assert!(lines[0].contains("\"accept\""), "{lines:?}");
        }
    }
}

#[test]
fn a_change_that_cant_be_shown_as_a_file_is_asked_about_in_words() {
    let dir = temp("words");
    let file = dir.join("gone.txt");
    let mut s = started(dir.to_str().unwrap());
    let changes = json!([{"path": file.display().to_string(), "kind": {"type": "delete"}, "diff": "bye\n"}]);
    events(&mut s, json!({"method": "item/started", "params": {"threadId": "thread-1", "item": {"type": "fileChange", "id": "f1", "changes": changes, "status": "inProgress"}}}));
    let (e, _) = s.line(r#"{"method":"item/fileChange/requestApproval","id":3,"params":{"threadId":"thread-1","turnId":"t","itemId":"f1"}}"#);
    let p = &permissions(&e)[0];
    assert_eq!(p.kind, PermissionKind::Other);
    assert_eq!(p.title, "Delete gone.txt");
}

#[test]
fn other_requests_are_answered_so_codex_isnt_left_waiting() {
    let mut s = started("/p");
    let (e, replies) = s.line(r#"{"method":"item/tool/requestUserInput","id":4,"params":{"questions":[]}}"#);
    assert!(matches!(&e[0], Event::Notice(_)));
    assert_eq!(serde_json::from_str::<Value>(&replies[0]).unwrap(), json!({"id": 4, "result": {"answers": {}}}));
    let (_, replies) = s.line(r#"{"method":"mcpServer/elicitation/request","id":5,"params":{"serverName":"x","mode":"form","message":"Your name?","requestedSchema":{"type":"object","properties":{}}}}"#);
    assert_eq!(serde_json::from_str::<Value>(&replies[0]).unwrap()["result"]["action"], "decline", "a form isn't something this panel can show");
    let (_, replies) = s.line(r#"{"method":"something/new","id":6,"params":{}}"#);
    assert_eq!(serde_json::from_str::<Value>(&replies[0]).unwrap()["error"]["code"], -32601);
    // More access: shown, and granted or not as the person says.
    let (e, _) = s.line(r#"{"method":"item/permissions/requestApproval","id":8,"params":{"threadId":"thread-1","turnId":"t","itemId":"p","cwd":"/p","startedAtMs":1,"reason":"the network","permissions":{"network":{"enabled":true}}}}"#);
    let p = &permissions(&e)[0];
    let allowed = s.command(Command::Answer { id: p.id.clone(), answer: Answer::Allow { text: None, for_session: true } }).1;
    assert_eq!(serde_json::from_str::<Value>(&allowed[0]).unwrap(), json!({"id": 8, "result": {"permissions": {"network": {"enabled": true}}, "scope": "session"}}));
}

#[test]
fn an_mcp_tool_is_allowed_if_limoniters_own_and_otherwise_put_to_the_person() {
    // As recorded from codex-cli 0.172.0 (cut down).
    let ask = |server: &str| format!(r#"{{"method":"mcpServer/elicitation/request","id":0,"params":{{"threadId":"thread-1","turnId":"t","serverName":"{server}","mode":"form","_meta":{{"codex_approval_kind":"mcp_tool_call","persist":["session","always"],"tool_name":"ping","tool_description":"Returns pong.","tool_params":{{"n":1}},"tool_params_display":[]}},"message":"Allow the {server} MCP server to run tool \"ping\"?","requestedSchema":{{"type":"object","properties":{{}}}}}}}}"#);
    let mut s = started("/p");
    let (e, replies) = s.line(&ask("limoniter"));
    assert!(e.is_empty(), "no question for its own tools");
    assert_eq!(serde_json::from_str::<Value>(&replies[0]).unwrap(), json!({"id": 0, "result": {"action": "accept", "content": null}}));
    let (e, replies) = s.line(&ask("weather"));
    assert!(replies.is_empty());
    let p = &permissions(&e)[0];
    assert_eq!(p.title, "Use weather: ping");
    assert!(p.can_remember && p.detail.contains("\"n\":1"), "{p:?}");
    let once = s.command(Command::Answer { id: p.id.clone(), answer: Answer::Allow { text: None, for_session: false } }).1;
    assert_eq!(serde_json::from_str::<Value>(&once[0]).unwrap()["result"], json!({"action": "accept", "content": null}));
    s.line(&ask("weather"));
    let session_wide = s.command(Command::Answer { id: p.id.clone(), answer: Answer::Allow { text: None, for_session: true } }).1;
    assert_eq!(serde_json::from_str::<Value>(&session_wide[0]).unwrap()["result"]["_meta"]["persist"], "session");
    s.line(&ask("weather"));
    let no = s.command(Command::Answer { id: p.id.clone(), answer: Answer::Refuse("no".into()) }).1;
    assert_eq!(serde_json::from_str::<Value>(&no[0]).unwrap()["result"]["action"], "decline");
}

#[test]
fn stopping_cancels_what_was_waiting_and_interrupts_the_turn() {
    let mut s = started("/p");
    events(&mut s, json!({"method": "turn/started", "params": {"threadId": "thread-1", "turn": {"id": "turn-9", "items": [], "status": "inProgress"}}}));
    let (e, _) = s.line(r#"{"method":"item/commandExecution/requestApproval","id":0,"params":{"threadId":"thread-1","turnId":"turn-9","itemId":"c1","command":"rm x"}}"#);
    let id = permissions(&e)[0].id.clone();
    let (events_, lines) = s.command(Command::Interrupt);
    assert_eq!(events_, vec![Event::PermissionGone { id }]);
    assert_eq!(serde_json::from_str::<Value>(&lines[0]).unwrap(), json!({"id": 0, "result": {"decision": "cancel"}}));
    let interrupt: Value = serde_json::from_str(&lines[1]).unwrap();
    assert_eq!(interrupt["method"], "turn/interrupt");
    assert_eq!(interrupt["params"], json!({"threadId": "thread-1", "turnId": "turn-9"}));
}

#[test]
fn a_request_taken_back_is_gone_from_the_panel() {
    let mut s = started("/p");
    let (e, _) = s.line(r#"{"method":"item/commandExecution/requestApproval","id":2,"params":{"threadId":"thread-1","turnId":"t","itemId":"c1","command":"ls"}}"#);
    let id = permissions(&e)[0].id.clone();
    assert_eq!(events(&mut s, json!({"method": "serverRequest/resolved", "params": {"threadId": "thread-1", "requestId": 2}})), vec![Event::PermissionGone { id }]);
}

#[test]
fn tokens_context_and_limits_come_from_its_notifications() {
    let mut s = started("/p");
    let usage = events(&mut s, json!({"method": "thread/tokenUsage/updated", "params": {"threadId": "thread-1", "turnId": "t", "tokenUsage": {
        "total": {"totalTokens": 28344, "inputTokens": 28212, "cachedInputTokens": 23936, "cacheWriteInputTokens": 0, "outputTokens": 132, "reasoningOutputTokens": 11},
        "last": {"totalTokens": 14186, "inputTokens": 14181, "cachedInputTokens": 13824, "cacheWriteInputTokens": 0, "outputTokens": 5, "reasoningOutputTokens": 0}, "modelContextWindow": 258400}}}));
    assert_eq!(usage, vec![Event::Usage { tokens: Tokens { input: 28212, output: 132, cached: 23936 }, cost: None }, Event::Context(Context { used: 14186, max: 258_400 })]);
    assert_eq!(s.command(Command::AskContext).0, vec![Event::Context(Context { used: 14186, max: 258_400 })], "asked again, it says what it last heard");
    let limits = events(&mut s, json!({"method": "account/rateLimits/updated", "params": {"rateLimits": {"limitId": "codex", "primary": {"usedPercent": 11, "windowDurationMins": 300, "resetsAt": 1795839145}, "secondary": {"usedPercent": 59, "windowDurationMins": 10080, "resetsAt": 1796252461}}}}));
    let Event::Limits(l) = &limits[0] else { panic!("{limits:?}") };
    assert_eq!(l.five_hour, Some(Limit { percent: 11.0, resets: Some(1_795_839_145) }));
    assert_eq!(l.week, Some(Limit { percent: 59.0, resets: Some(1_796_252_461) }));
    assert!(s.command(Command::AskUsage).1.is_empty(), "already known from its notifications");
    // A plan with a week only.
    let week = limits_from(&json!({"primary": {"usedPercent": 5, "windowDurationMins": 10080, "resetsAt": 1}}));
    assert_eq!((week.five_hour, week.week.map(|w| w.percent)), (None, Some(5.0)));
}

#[test]
fn a_helper_codex_starts_is_a_task_with_its_own_tokens_and_tool_uses() {
    let mut s = started("/p");
    let spawn = |status: &str, tool: &str, state: &str| json!({"method": "item/started", "params": {"threadId": "thread-1", "item": {"type": "collabAgentToolCall", "id": "k1", "tool": tool, "status": status, "senderThreadId": "thread-1", "receiverThreadIds": ["child-1"], "prompt": "Look through the tests\nand report", "agentsStates": {"child-1": {"status": state, "message": null}}}}});
    let e = events(&mut s, spawn("inProgress", "spawnAgent", "running"));
    assert!(matches!(&e[0], Event::ToolUse { summary, .. } if summary == "Subagent: Look through the tests"));
    let Event::Task(task) = &e[1] else { panic!("{e:?}") };
    assert_eq!((task.id.as_str(), task.description.as_str(), task.state), ("child-1", "Look through the tests", TaskState::Running));
    // What the helper does is counted, not shown in the transcript.
    let used = events(&mut s, json!({"method": "item/started", "params": {"threadId": "child-1", "item": {"type": "commandExecution", "id": "x", "command": "ls", "status": "inProgress"}}}));
    let Event::Task(task) = &used[0] else { panic!("{used:?}") };
    assert_eq!((task.tool_uses, task.last_tool.as_deref()), (1, Some("Shell")));
    let tokens = events(&mut s, json!({"method": "thread/tokenUsage/updated", "params": {"threadId": "child-1", "turnId": "ct", "tokenUsage": {"total": {"totalTokens": 700, "inputTokens": 600, "cachedInputTokens": 0, "outputTokens": 100, "reasoningOutputTokens": 0}, "last": {"totalTokens": 700, "inputTokens": 600, "cachedInputTokens": 0, "outputTokens": 100, "reasoningOutputTokens": 0}}}}));
    assert!(matches!(&tokens[0], Event::Task(t) if t.tokens == 700));
    assert!(events(&mut s, json!({"method": "item/agentMessage/delta", "params": {"threadId": "child-1", "turnId": "ct", "itemId": "m", "delta": "hi"}})).is_empty(), "a helper's words aren't the session's");
    let done = events(&mut s, json!({"method": "item/completed", "params": {"threadId": "thread-1", "item": {"type": "collabAgentToolCall", "id": "k2", "tool": "wait", "status": "completed", "senderThreadId": "thread-1", "receiverThreadIds": ["child-1"], "agentsStates": {"child-1": {"status": "completed", "message": "ok"}}}}}));
    assert!(matches!(&done[0], Event::Task(t) if t.state == TaskState::Done && t.description == "Look through the tests"));
}

#[test]
fn a_turn_ending_a_failed_turn_a_title_and_trouble() {
    let mut s = started("/p");
    assert_eq!(events(&mut s, json!({"method": "turn/completed", "params": {"threadId": "thread-1", "turn": {"id": "t", "status": "completed", "items": []}}})), vec![Event::TurnFinished]);
    let failed = events(&mut s, json!({"method": "turn/completed", "params": {"threadId": "thread-1", "turn": {"id": "t", "status": "failed", "items": [], "error": {"message": "the service said no"}}}}));
    assert_eq!(failed, vec![Event::Error("the service said no".into()), Event::TurnFinished]);
    assert_eq!(events(&mut s, json!({"method": "thread/name/updated", "params": {"threadId": "thread-1", "threadName": "Parser fix"}})), vec![Event::Title("Parser fix".into())]);
    let retry = events(&mut s, json!({"method": "error", "params": {"error": {"message": "overloaded"}, "willRetry": true, "threadId": "thread-1", "turnId": "t"}}));
    assert!(matches!(&retry[0], Event::Notice(text) if text.contains("overloaded")));
    assert!(matches!(&events(&mut s, json!({"method": "error", "params": {"error": {"message": "gone"}, "willRetry": false}}))[0], Event::Error(text) if text == "gone"));
}

#[test]
fn a_line_that_isnt_json_is_said_and_the_rest_carry_on() {
    let mut s = started("/p");
    let (e, replies) = s.line("this is not json {");
    assert!(matches!(&e[0], Event::Notice(text) if text.contains("this is not json")));
    assert!(replies.is_empty());
    assert!(s.line("").0.is_empty());
    assert_eq!(events(&mut s, json!({"method": "item/agentMessage/delta", "params": {"threadId": "thread-1", "itemId": "a", "delta": "x"}})).len(), 1);
}

// ---------- The patch ----------

#[test]
fn a_patch_is_applied_where_it_says_or_where_its_lines_are() {
    let now = "a\nb\nc\nd\ne\nf\n";
    assert_eq!(apply_diff(now, "@@ -2,3 +2,3 @@\n b\n-c\n+C\n d\n").unwrap(), "a\nb\nC\nd\ne\nf\n");
    // The file has grown above it since: found a line down.
    assert_eq!(apply_diff("x\na\nb\nc\nd\n", "@@ -1,3 +1,3 @@\n a\n-b\n+B\n c\n").unwrap(), "x\na\nB\nc\nd\n");
    // Two hunks, one adding and one taking away.
    assert_eq!(apply_diff(now, "@@ -1,2 +1,3 @@\n a\n+a2\n b\n@@ -5,2 +6,1 @@\n e\n-f\n").unwrap(), "a\na2\nb\nc\nd\ne\n");
    // Lines that aren't there.
    assert_eq!(apply_diff(now, "@@ -1,2 +1,2 @@\n a\n-zzz\n+y\n"), None);
    assert_eq!(apply_diff(now, "nothing like a diff"), None);
}

#[test]
fn a_patch_keeps_the_files_own_line_ends_and_missing_final_newline() {
    assert_eq!(apply_diff("one\r\ntwo\r\nthree\r\n", "@@ -1,3 +1,3 @@\n one\n-two\n+TWO\n three\n").unwrap(), "one\r\nTWO\r\nthree\r\n");
    assert_eq!(apply_diff("a\nb", "@@ -1,2 +1,2 @@\n a\n-b\n\\ No newline at end of file\n+c\n\\ No newline at end of file\n").unwrap(), "a\nc");
    assert_eq!(apply_diff("", "@@ -0,0 +1,2 @@\n+x\n+y\n").unwrap(), "x\ny\n");
}

// ---------- A whole session against the stand-in ----------

fn python() -> Option<String> {
    ["python3", "python"].iter().find(|name| crate::platform::command(name).arg("--version").output().is_ok_and(|out| out.status.success())).map(|name| name.to_string())
}

pub(crate) fn fake_options(dir: &Path, extra: &[&str]) -> Option<Options> {
    let Some(program) = python() else {
        eprintln!("No Python here, so the stand-in for Codex can't run: this test did nothing.");
        return None;
    };
    let script = Path::new(env!("CARGO_MANIFEST_DIR")).join("src/agent_testdata/fake_codex.py");
    let mut prefix = vec![script.display().to_string(), "--log".to_string(), dir.join("log.txt").display().to_string()];
    prefix.extend(extra.iter().map(|e| e.to_string()));
    Some(Options { program, prefix, folder: dir.to_path_buf(), mode: Mode::Ask, model: None, effort: None, resume: None, mcp: None, wsl: None, wake: Arc::new(|| {}) })
}

fn fake_session(dir: &Path, extra: &[&str]) -> Option<Session> {
    let options = fake_options(dir, extra)?;
    let folder = options.folder.clone();
    Some(Session::new(Tool::Codex, &folder, start(options).unwrap()))
}

fn wait_for(session: &mut Session, what: &str, done: impl Fn(&Session) -> bool) {
    let began = std::time::Instant::now();
    while !done(session) {
        session.pump();
        assert!(began.elapsed() < Duration::from_secs(20), "waited for {what}; the transcript is {:?}", session.items);
        std::thread::sleep(Duration::from_millis(10));
    }
}

fn log_lines(dir: &Path) -> Vec<Value> {
    std::fs::read_to_string(dir.join("log.txt")).unwrap_or_default().lines().filter_map(|l| serde_json::from_str(l).ok()).collect()
}

#[test]
fn a_session_starts_with_models_and_streams_text_thinking_tokens_and_limits() {
    let dir = temp("stream");
    let Some(mut s) = fake_session(&dir, &[]) else { return };
    wait_for(&mut s, "the start", |s| s.session_id.is_some() && !s.models.is_empty());
    assert_eq!(s.model.as_deref(), Some("gpt-6.2-sol"));
    assert_eq!(s.models.iter().map(|m| m.value.as_str()).collect::<Vec<_>>(), vec!["default", "gpt-6.2-sol", "gpt-5.6-luna"]);
    assert!(s.efforts().contains(&"ultra".to_string()), "the default model's efforts");
    s.say("say hello");
    wait_for(&mut s, "the end", |s| !s.busy && s.context.is_some() && s.limits.any());
    assert!(s.items.iter().any(|i| matches!(i, Item::Text { text, done: true, .. } if text == "Hello there")), "{:?}", s.items);
    assert!(s.items.iter().any(|i| matches!(i, Item::Thinking { text, .. } if text == "**Thinking** it over")));
    assert_eq!(s.tokens, Tokens { input: 1400, output: 100, cached: 400 });
    assert_eq!(s.context, Some(Context { used: 900, max: 258_400 }));
    assert_eq!(s.limits.five_hour.unwrap().percent, 11.0);
    assert_eq!(s.limits.week.unwrap().percent, 59.0);
    // The first message names the tab, and the request was made as Codex's extension makes it.
    assert_eq!(s.title, "say hello");
    let log = log_lines(&dir);
    let start_request = log.iter().find(|l| l["method"] == "thread/start").unwrap();
    assert_eq!(start_request["params"]["serviceName"], "limoniter");
    assert_eq!(log.iter().find(|l| l["method"] == "initialize").unwrap()["params"]["clientInfo"]["name"], "limoniter");
}

#[test]
fn a_command_waits_to_be_allowed_and_each_answer_reaches_codex() {
    let dir = temp("command");
    let Some(mut s) = fake_session(&dir, &[]) else { return };
    s.say("command one");
    wait_for(&mut s, "the request", |s| !s.waiting().is_empty());
    let p = s.waiting()[0].clone();
    assert_eq!(p.kind, PermissionKind::Command("touch b.txt".into()));
    assert!(p.can_remember);
    s.answer(&p.id, Answer::Allow { text: None, for_session: true });
    wait_for(&mut s, "the end", |s| !s.busy && s.items.iter().any(|i| matches!(i, Item::Text { text, .. } if text == "done")));
    assert!(s.items.iter().any(|i| matches!(i, Item::Permission { decided: Decided::Allowed { for_session: true }, .. })));
    assert!(s.items.iter().any(|i| matches!(i, Item::Tool { result: Some(r), .. } if !r.is_error)));
    // Refused: Codex goes on, told no.
    s.say("command two");
    wait_for(&mut s, "the second request", |s| !s.waiting().is_empty());
    let p = s.waiting()[0].clone();
    s.answer(&p.id, Answer::Refuse("no".into()));
    wait_for(&mut s, "the end", |s| !s.busy && s.items.iter().any(|i| matches!(i, Item::Text { text, .. } if text == "not allowed")));
    assert!(s.items.iter().any(|i| matches!(i, Item::Tool { result: Some(r), .. } if r.is_error)), "the declined command is shown as one");
}

#[test]
fn a_file_change_asks_for_the_whole_file_and_the_edit_the_person_made_is_what_is_left() {
    let dir = temp("edit");
    let file = dir.join("a.txt");
    std::fs::write(&file, "one\ntwo\nthree\n").unwrap();
    let Some(mut s) = fake_session(&dir, &[]) else { return };
    s.say(&format!("edit {}", file.display()));
    wait_for(&mut s, "the request", |s| !s.waiting().is_empty());
    let p = s.waiting()[0].clone();
    assert_eq!(p.kind, PermissionKind::Edit { path: file.clone(), proposed: "one\nTWO\nthree\n".into() });
    s.answer(&p.id, Answer::Allow { text: Some("one\nMINE\nthree\n".into()), for_session: false });
    wait_for(&mut s, "the end", |s| !s.busy && s.items.iter().any(|i| matches!(i, Item::Text { text, .. } if text == "changed")));
    assert_eq!(std::fs::read_to_string(&file).unwrap(), "one\nMINE\nthree\n");
    assert!(s.items.iter().any(|i| matches!(i, Item::Notice(text) if text.contains("a.txt"))));
    // Codex hears of it with the next message.
    s.say("thanks");
    wait_for(&mut s, "the next turn", |s| !s.busy && s.items.iter().any(|i| matches!(i, Item::Text { text, .. } if text == "ok")));
    let turns: Vec<Value> = log_lines(&dir).into_iter().filter(|l| l["method"] == "turn/start").collect();
    assert_eq!(turns.len(), 2);
    assert!(turns[1]["params"]["input"][0]["text"].as_str().unwrap().contains("edited a.txt"), "{:?}", turns[1]);
    // Refused, a change is not made.
    std::fs::write(&file, "one\ntwo\nthree\n").unwrap();
    s.say(&format!("edit {}", file.display()));
    wait_for(&mut s, "the second request", |s| !s.waiting().is_empty());
    let p = s.waiting()[0].clone();
    s.answer(&p.id, Answer::Refuse("no".into()));
    wait_for(&mut s, "the end", |s| !s.busy && s.items.iter().any(|i| matches!(i, Item::Text { text, .. } if text == "refused")));
    assert_eq!(std::fs::read_to_string(&file).unwrap(), "one\ntwo\nthree\n");
}

#[test]
fn a_new_file_is_a_whole_file_to_review() {
    let dir = temp("add");
    let file = dir.join("c.txt");
    let Some(mut s) = fake_session(&dir, &[]) else { return };
    s.say(&format!("add {}", file.display()));
    wait_for(&mut s, "the request", |s| !s.waiting().is_empty());
    let p = s.waiting()[0].clone();
    assert_eq!(p.kind, PermissionKind::Edit { path: file.clone(), proposed: "hello\n".into() });
    assert_eq!(p.title, "Create c.txt");
    s.answer(&p.id, Answer::Allow { text: None, for_session: false });
    wait_for(&mut s, "the end", |s| !s.busy && file.exists());
    assert_eq!(std::fs::read_to_string(&file).unwrap(), "hello\n");
}

#[test]
fn helpers_are_tasks_while_the_session_goes_on() {
    let dir = temp("agents");
    let Some(mut s) = fake_session(&dir, &[]) else { return };
    s.say("agents now");
    wait_for(&mut s, "the end", |s| !s.busy && s.items.iter().any(|i| matches!(i, Item::Text { text, .. } if text == "all reported")));
    assert_eq!(s.tasks.len(), 1);
    let t = &s.tasks[0];
    assert_eq!((t.description.as_str(), t.state, t.tool_uses, t.tokens), ("Look through the tests", TaskState::Done, 1, 700));
}

#[test]
fn stopping_interrupts_the_turn_and_the_queued_message_goes_after() {
    let dir = temp("interrupt");
    let Some(mut s) = fake_session(&dir, &[]) else { return };
    s.say("slow please");
    s.say("then this");
    assert_eq!(s.queue, vec!["then this".to_string()]);
    wait_for(&mut s, "it working", |s| s.items.iter().any(|i| matches!(i, Item::Text { text, .. } if text == "working")));
    s.stop();
    wait_for(&mut s, "the queued message", |s| s.items.iter().any(|i| matches!(i, Item::You(t) if t == "then this")) && s.items.iter().any(|i| matches!(i, Item::Text { text, .. } if text == "ok")) && !s.busy);
    let log = log_lines(&dir);
    let interrupt = log.iter().find(|l| l["method"] == "turn/interrupt").expect("it was told to stop");
    assert_eq!(interrupt["params"]["threadId"], "thread-fake-1");
    assert!(interrupt["params"]["turnId"].as_str().unwrap().starts_with("turn-"));
}

#[test]
fn a_malformed_line_is_said_and_the_session_goes_on() {
    let dir = temp("malformed");
    let Some(mut s) = fake_session(&dir, &[]) else { return };
    s.say("bad line");
    wait_for(&mut s, "the end", |s| !s.busy && s.items.iter().any(|i| matches!(i, Item::Text { text, .. } if text == "still here")));
    assert!(s.items.iter().any(|i| matches!(i, Item::Notice(text) if text.contains("this is not json"))), "{:?}", s.items);
}

#[test]
fn a_failed_turn_is_an_error_and_not_the_end_of_the_session() {
    let dir = temp("fail");
    let Some(mut s) = fake_session(&dir, &[]) else { return };
    s.say("fail now");
    wait_for(&mut s, "the end", |s| !s.busy && s.items.iter().any(|i| matches!(i, Item::Error(_))));
    assert!(s.finished.is_none());
    s.say("say again");
    wait_for(&mut s, "the next reply", |s| !s.busy && s.items.iter().any(|i| matches!(i, Item::Text { text, .. } if text == "Hello there")));
}

#[test]
fn the_context_is_sent_in_the_text_to_a_codex_that_refuses_the_field() {
    let _turn = CONTEXT.lock().unwrap_or_else(|e| e.into_inner());
    let dir = temp("refuse-context");
    let Some(mut s) = fake_session(&dir, &["--refuse-context"]) else { return };
    set_ide_context("# Context from my IDE setup:\n\n## Active file: a.rs\n".into());
    s.say("say hello");
    wait_for(&mut s, "the reply", |s| !s.busy && s.items.iter().any(|i| matches!(i, Item::Text { text, .. } if text == "Hello there")));
    set_ide_context(String::new());
    let turns: Vec<Value> = log_lines(&dir).into_iter().filter(|l| l["method"] == "turn/start").collect();
    assert_eq!(turns.len(), 2, "tried again");
    assert!(turns[0]["params"].get("additionalContext").is_some());
    assert!(turns[1]["params"].get("additionalContext").is_none());
    assert!(turns[1]["params"]["input"][0]["text"].as_str().unwrap().contains("Active file: a.rs"));
}

#[test]
fn a_thread_is_resumed_by_its_name_and_the_mode_is_applied() {
    let dir = temp("resume");
    let Some(mut options) = fake_options(&dir, &[]) else { return };
    options.resume = Some("old-thread".into());
    options.mode = Mode::Plan;
    let mut s = Session::new(Tool::Codex, &dir, start(options).unwrap());
    wait_for(&mut s, "the start", |s| s.session_id.is_some());
    assert_eq!(s.session_id.as_deref(), Some("old-thread"));
    assert_eq!(s.mode, Mode::Plan);
    let log = log_lines(&dir);
    let resume = log.iter().find(|l| l["method"] == "thread/resume").unwrap();
    assert_eq!(resume["params"]["threadId"], "old-thread");
    assert_eq!(resume["params"]["sandbox"], "read-only");
    assert!(log.iter().all(|l| l["method"] != "thread/start"));
}

#[test]
fn the_models_can_be_asked_for_without_starting_a_thread() {
    let dir = temp("probe");
    let Some(options) = fake_options(&dir, &[]) else { return };
    let models = probe_models(&options.program, &options.prefix, &dir).unwrap();
    assert_eq!(models[0].value, "default");
    assert_eq!(models.len(), 3);
    assert!(log_lines(&dir).iter().all(|l| l["method"] != "thread/start" && l["method"] != "turn/start"));
}

#[test]
fn a_program_that_isnt_there_says_so() {
    let mut o = options("/tmp");
    o.program = "definitely-not-codex-here".into();
    let Err(why) = start(o) else { panic!("it started") };
    assert!(why.contains("definitely-not-codex-here"), "{why}");
}
