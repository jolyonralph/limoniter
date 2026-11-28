//! Claude Code as a driver: the lines it sends and takes, checked against recorded ones, and a whole session run
//! against a stand-in program (src/agent_testdata/fake_claude.py) that answers as Claude Code 2.1.332 did.

use super::*;
use crate::agent_session::{Decided, Item, Session, Tool};
use serde_json::json;

fn state() -> State {
    State::new(Path::new("/p"))
}

fn events(state: &mut State, line: Value) -> Vec<Event> {
    state.line(&line.to_string()).0
}

// ---------- The lines, one at a time ----------

#[test]
fn arguments_ask_for_json_lines_and_permissions_through_the_host() {
    let options = Options { program: "claude".into(), prefix: Vec::new(), folder: "/p".into(), mode: Mode::AcceptEdits, model: Some("haiku".into()), effort: Some("low".into()), resume: Some("abc".into()), ide_port: Some(1234), mcp: None, wsl: None, wake: Arc::new(|| {}) };
    let args = arguments(&options);
    let has = |pair: &[&str]| args.windows(pair.len()).any(|w| w == pair);
    assert!(has(&["-p", "--output-format", "stream-json", "--input-format", "stream-json"]), "{args:?}");
    assert!(has(&["--permission-prompt-tool", "stdio"]));
    assert!(has(&["--permission-mode", "acceptEdits"]));
    assert!(has(&["--model", "haiku"]) && has(&["--effort", "low"]) && has(&["--resume", "abc"]));
    assert!(args.contains(&"--include-partial-messages".to_string()) && args.contains(&"--ide".to_string()));
    let plain = Options { model: Some("default".into()), effort: None, resume: None, ide_port: None, mcp: None, ..options };
    let args = arguments(&plain);
    assert!(!args.contains(&"--model".to_string()) && !args.contains(&"--effort".to_string()) && !args.contains(&"--ide".to_string()), "Default is no --model: {args:?}");
}

#[test]
fn the_modes_have_claude_codes_names() {
    for mode in Mode::ALL {
        assert_eq!(mode_from(mode_name(mode)), mode);
    }
    assert_eq!(mode_from("dontAsk"), Mode::Ask);
}

#[test]
fn the_initialize_answer_gives_the_models_and_efforts() {
    let mut s = state();
    let line = s.initialize();
    let id = serde_json::from_str::<Value>(&line).unwrap()["request_id"].as_str().unwrap().to_string();
    let answer = json!({"type": "control_response", "response": {"subtype": "success", "request_id": id, "response": {
        "models": [{"value": "default", "displayName": "Default (recommended)", "supportedEffortLevels": ["low", "max"]}, {"value": "claude-haiku-4-5-20251001", "displayName": "Haiku 4.5"}],
        "current_permission_mode": "plan"}}});
    assert_eq!(
        events(&mut s, answer),
        vec![Event::Ready {
            models: vec![ModelChoice { value: "default".into(), name: "Default (recommended)".into(), efforts: vec!["low".into(), "max".into()] }, ModelChoice { value: "claude-haiku-4-5-20251001".into(), name: "Haiku 4.5".into(), efforts: vec![] }],
            mode: Mode::Plan
        }]
    );
}

#[test]
fn streamed_and_finished_text_share_an_id_so_the_message_isnt_shown_twice() {
    let mut s = state();
    let mut all = Vec::new();
    all.extend(events(&mut s, json!({"type": "stream_event", "event": {"type": "message_start", "message": {"id": "m1"}}, "parent_tool_use_id": null})));
    all.extend(events(&mut s, json!({"type": "stream_event", "event": {"type": "content_block_start", "index": 0, "content_block": {"type": "thinking"}}, "parent_tool_use_id": null})));
    all.extend(events(&mut s, json!({"type": "stream_event", "event": {"type": "content_block_delta", "index": 0, "delta": {"type": "thinking_delta", "thinking": "Hm"}}, "parent_tool_use_id": null})));
    all.extend(events(&mut s, json!({"type": "stream_event", "event": {"type": "content_block_start", "index": 1, "content_block": {"type": "text"}}, "parent_tool_use_id": null})));
    all.extend(events(&mut s, json!({"type": "stream_event", "event": {"type": "content_block_delta", "index": 1, "delta": {"type": "text_delta", "text": "Hi"}}, "parent_tool_use_id": null})));
    all.extend(events(&mut s, json!({"type": "assistant", "message": {"id": "m1", "content": [{"type": "thinking", "thinking": ""}]}, "parent_tool_use_id": null})));
    all.extend(events(&mut s, json!({"type": "assistant", "message": {"id": "m1", "content": [{"type": "text", "text": "Hi there"}]}, "parent_tool_use_id": null})));
    assert_eq!(
        all,
        vec![
            Event::Thinking { id: "m1:k1".into(), text: "Hm".into() },
            Event::TextDelta { id: "m1:t1".into(), text: "Hi".into() },
            Event::Text { id: "m1:t1".into(), text: "Hi there".into(), done: true },
        ],
        "an empty finished thinking block leaves what streamed alone"
    );
}

#[test]
fn what_a_subagent_says_is_not_streamed_into_the_transcript_but_its_tools_are_tagged() {
    let mut s = state();
    assert!(events(&mut s, json!({"type": "stream_event", "event": {"type": "message_start", "message": {"id": "m2"}}, "parent_tool_use_id": "toolu_parent"})).is_empty());
    let found = events(&mut s, json!({"type": "assistant", "parent_tool_use_id": "toolu_parent", "message": {"id": "m2", "content": [{"type": "text", "text": "inner"}, {"type": "tool_use", "id": "c1", "name": "Read", "input": {"file_path": "/p/a.rs"}}]}}));
    assert_eq!(found.len(), 1, "{found:?}");
    assert!(matches!(&found[0], Event::ToolUse { parent: Some(p), summary, .. } if p == "toolu_parent" && summary == "Read a.rs"));
}

#[test]
fn tools_are_described_in_one_line_with_what_they_do() {
    let (summary, kind) = describe("Bash", &json!({"command": "cargo test --release\nsecond line"}), Path::new("/p"));
    assert_eq!(summary, "Ran cargo test --release");
    assert_eq!(kind, ToolKind::Command("cargo test --release\nsecond line".into()));
    let (summary, kind) = describe("Edit", &json!({"file_path": "/p/src/main.rs"}), Path::new("/p"));
    assert_eq!(summary, "Edit src/main.rs");
    assert_eq!(kind, ToolKind::FileChange("/p/src/main.rs".into()));
    assert_eq!(describe("Agent", &json!({"description": "Look around"}), Path::new("/p")).0, "Subagent: Look around");
    assert_eq!(describe("mcp__ide__getDiagnostics", &json!({}), Path::new("/p")).0, "mcp__ide__getDiagnostics");
}

#[test]
fn a_tool_result_comes_with_its_text_and_whether_it_failed() {
    let mut s = state();
    let got = events(&mut s, json!({"type": "user", "message": {"role": "user", "content": [{"tool_use_id": "t", "type": "tool_result", "content": [{"type": "text", "text": "a"}, {"type": "text", "text": "b"}], "is_error": true}]}}));
    assert_eq!(got, vec![Event::ToolResult { id: "t".into(), text: "a\nb".into(), is_error: true }]);
}

#[test]
fn a_result_gives_the_sessions_tokens_and_cost_then_the_turn_is_over() {
    let mut s = state();
    let got = events(&mut s, json!({"type": "result", "subtype": "success", "is_error": false, "total_cost_usd": 0.5, "usage": {"input_tokens": 1}, "modelUsage": {"a": {"inputTokens": 10, "outputTokens": 5, "cacheReadInputTokens": 100, "cacheCreationInputTokens": 20}, "b": {"inputTokens": 1, "outputTokens": 1}}}));
    assert_eq!(got, vec![Event::Usage { tokens: Tokens { input: 131, output: 6, cached: 100 }, cost: Some(0.5) }, Event::TurnFinished]);
    // Without the running total, each turn's own count is added up, and no cost is made up.
    let mut s = state();
    let turn = json!({"type": "result", "subtype": "success", "usage": {"input_tokens": 3, "output_tokens": 4, "cache_read_input_tokens": 7}});
    events(&mut s, turn.clone());
    let got = events(&mut s, turn);
    assert_eq!(got[0], Event::Usage { tokens: Tokens { input: 20, output: 8, cached: 14 }, cost: None });
}

#[test]
fn an_error_result_is_told_and_a_stopped_turn_is_only_said_to_be_stopped() {
    let mut s = state();
    let got = events(&mut s, json!({"type": "result", "subtype": "error_max_turns", "is_error": true, "result": "Too many turns"}));
    assert!(got.contains(&Event::Error("Too many turns".into())));
    s.command(Command::Interrupt);
    let got = events(&mut s, json!({"type": "result", "subtype": "error_during_execution", "is_error": true}));
    assert!(!got.iter().any(|e| matches!(e, Event::Error(_))), "{got:?}");
    assert!(got.contains(&Event::Notice("Stopped.".into())) && got.contains(&Event::TurnFinished));
}

#[test]
fn context_and_limits_come_from_their_answers_and_only_when_there_are_some() {
    let mut s = state();
    let line = s.command(Command::AskContext).remove(0);
    let id = serde_json::from_str::<Value>(&line).unwrap()["request_id"].as_str().unwrap().to_string();
    let got = events(&mut s, json!({"type": "control_response", "response": {"subtype": "success", "request_id": id, "response": {"totalTokens": 27566, "maxTokens": 1000000, "percentage": 3}}}));
    assert_eq!(got, vec![Event::Context(Context { used: 27566, max: 1_000_000 })]);
    let line = s.command(Command::AskUsage).remove(0);
    let id = serde_json::from_str::<Value>(&line).unwrap()["request_id"].as_str().unwrap().to_string();
    let got = events(&mut s, json!({"type": "control_response", "response": {"subtype": "success", "request_id": id, "response": {"rate_limits_available": true, "rate_limits": {"five_hour": {"utilization": 42, "resets_at": "2026-11-27T20:19:59.621803+00:00"}, "seven_day": {"utilization": 19, "resets_at": null}, "seven_day_opus": null}}}}));
    let Event::Limits(limits) = &got[0] else { panic!("{got:?}") };
    assert_eq!(limits.five_hour.unwrap().percent, 42.0);
    assert_eq!(limits.five_hour.unwrap().resets, Some(1_795_810_799));
    assert_eq!(limits.week.unwrap().resets, None);
    // A plan without limits (or an API key) says nothing, so nothing is shown.
    let line = s.command(Command::AskUsage).remove(0);
    let id = serde_json::from_str::<Value>(&line).unwrap()["request_id"].as_str().unwrap().to_string();
    assert!(events(&mut s, json!({"type": "control_response", "response": {"subtype": "success", "request_id": id, "response": {"rate_limits_available": false, "rate_limits": null}}})).is_empty());
}

#[test]
fn a_rate_limit_event_gives_percent_from_a_fraction() {
    let mut s = state();
    let got = events(&mut s, json!({"type": "rate_limit_event", "rate_limit_info": {"status": "allowed", "unifiedWindows": {"five_hour": {"utilization": 0.41, "resetsAt": 1795810800}, "seven_day": {"utilization": 0.19, "resetsAt": 1796320800}}}}));
    let Event::Limits(limits) = &got[0] else { panic!("{got:?}") };
    assert!((limits.five_hour.unwrap().percent - 41.0).abs() < 0.01);
    assert_eq!(limits.week.unwrap().resets, Some(1_796_320_800));
    assert!(events(&mut s, json!({"type": "rate_limit_event", "rate_limit_info": {"status": "allowed"}})).is_empty());
}

#[test]
fn subagent_lines_become_tasks_that_keep_what_an_update_leaves_out() {
    let mut s = state();
    let started = events(&mut s, json!({"type": "system", "subtype": "task_started", "task_id": "a", "description": "Read the files", "subagent_type": "general-purpose", "task_type": "local_agent"}));
    assert!(matches!(&started[0], Event::Task(t) if t.description == "Read the files" && t.kind == "general-purpose" && t.state == TaskState::Running));
    let progress = events(&mut s, json!({"type": "system", "subtype": "task_progress", "task_id": "a", "description": "Reading a.txt", "usage": {"total_tokens": 15026, "tool_uses": 1, "duration_ms": 5}, "last_tool_name": "Read"}));
    assert!(matches!(&progress[0], Event::Task(t) if t.tokens == 15026 && t.tool_uses == 1 && t.last_tool.as_deref() == Some("Read")));
    let killed = events(&mut s, json!({"type": "system", "subtype": "task_updated", "task_id": "a", "patch": {"status": "killed"}}));
    assert!(matches!(&killed[0], Event::Task(t) if t.state == TaskState::Stopped));
    assert!(events(&mut s, json!({"type": "system", "subtype": "task_updated", "task_id": "a", "patch": {"description": "x"}})).is_empty());
}

#[test]
fn a_line_that_isnt_json_is_said_not_swallowed_and_changes_nothing() {
    let mut s = state();
    let (got, replies) = s.line("this is not json");
    assert!(matches!(&got[..], [Event::Notice(text)] if text.contains("this is not json")));
    assert!(replies.is_empty());
    assert!(s.line("   ").0.is_empty());
    assert!(s.line(r#"{"type":"assistant","message":{"id":"x","content":["#).0.iter().all(|e| matches!(e, Event::Notice(_))));
    assert!(s.line(r#"{"type":"something_new","x":1}"#).0.is_empty(), "a message from a newer Claude Code is ignored");
}

#[test]
fn what_the_program_asks_of_its_host_that_this_host_doesnt_do_is_answered_with_an_error() {
    let mut s = state();
    let (got, replies) = s.line(&json!({"type": "control_request", "request_id": "r9", "request": {"subtype": "hook_callback"}}).to_string());
    assert!(got.is_empty());
    let reply: Value = serde_json::from_str(&replies[0]).unwrap();
    assert_eq!(reply["response"]["subtype"], "error");
    assert_eq!(reply["response"]["request_id"], "r9");
}

#[test]
fn a_question_with_choices_is_refused_with_a_word_to_ask_in_plain_text() {
    let mut s = state();
    let (got, replies) = s.line(&json!({"type": "control_request", "request_id": "q1", "request": {"subtype": "can_use_tool", "tool_name": "AskUserQuestion", "input": {"questions": []}}}).to_string());
    assert!(matches!(&got[..], [Event::Notice(_)]));
    let reply: Value = serde_json::from_str(&replies[0]).unwrap();
    assert_eq!(reply["response"]["response"]["behavior"], "deny");
    assert!(reply["response"]["response"]["message"].as_str().unwrap().contains("plain text"));
}

// ---------- Permission requests ----------

fn folder(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("limoniter-agent-claude-{}-{tag}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn request(tool: &str, input: Value) -> String {
    json!({"type": "control_request", "request_id": "r1", "request": {"subtype": "can_use_tool", "tool_name": tool, "input": input, "tool_use_id": "tu1", "permission_suggestions": [{"type": "addRules", "rules": [{"toolName": "Bash", "ruleContent": "ls"}], "behavior": "allow", "destination": "localSettings"}]}}).to_string()
}

fn sent(lines: Vec<String>) -> Value {
    assert_eq!(lines.len(), 1, "{lines:?}");
    serde_json::from_str::<Value>(&lines[0]).unwrap()["response"]["response"].clone()
}

#[test]
fn an_edit_request_carries_the_whole_file_as_it_would_be_and_allowing_sends_the_input_on() {
    let dir = folder("edit");
    let file = dir.join("a.txt");
    std::fs::write(&file, "one\ntwo\n").unwrap();
    let mut s = State::new(&dir);
    let input = json!({"file_path": file, "old_string": "two", "new_string": "three", "replace_all": false});
    let (got, _) = s.line(&request("Edit", input.clone()));
    let Event::Permission(permission) = &got[0] else { panic!("{got:?}") };
    assert_eq!(permission.title, "Edit a.txt");
    assert_eq!(permission.kind, PermissionKind::Edit { path: file.clone(), proposed: "one\nthree\n".into() });
    let answer = sent(s.command(Command::Answer { id: "r1".into(), answer: Answer::Allow { text: Some("one\nthree\n".into()), for_session: false } }));
    assert_eq!(answer["behavior"], "allow");
    assert_eq!(answer["updatedInput"], input, "what was proposed, unchanged, is sent as it came");
    assert!(answer.get("updatedPermissions").is_none());
}

#[test]
fn an_edit_the_person_changed_in_the_review_is_sent_as_an_edit_that_makes_the_reviewed_file() {
    let dir = folder("edited");
    let file = dir.join("a.txt");
    std::fs::write(&file, "one\ntwo\n").unwrap();
    let mut s = State::new(&dir);
    s.line(&request("Edit", json!({"file_path": file, "old_string": "two", "new_string": "three"})));
    let answer = sent(s.command(Command::Answer { id: "r1".into(), answer: Answer::Allow { text: Some("one\nTHREE\n".into()), for_session: false } }));
    let input = &answer["updatedInput"];
    assert_eq!(input["old_string"], "one\ntwo\n");
    assert_eq!(input["new_string"], "one\nTHREE\n");
    assert_eq!(input["replace_all"], false);
    // A write takes the reviewed text as its content.
    let mut s = State::new(&dir);
    s.line(&request("Write", json!({"file_path": file, "content": "new\n"})));
    let answer = sent(s.command(Command::Answer { id: "r1".into(), answer: Answer::Allow { text: Some("newer\n".into()), for_session: false } }));
    assert_eq!(answer["updatedInput"]["content"], "newer\n");
}

#[test]
fn a_new_file_is_proposed_from_nothing() {
    let dir = folder("new");
    let file = dir.join("fresh.txt");
    let mut s = State::new(&dir);
    let (got, _) = s.line(&request("Write", json!({"file_path": file, "content": "hello\n"})));
    let Event::Permission(permission) = &got[0] else { panic!() };
    assert_eq!(permission.kind, PermissionKind::Edit { path: file, proposed: "hello\n".into() });
}

#[test]
fn an_edit_that_doesnt_fit_the_file_or_a_file_that_isnt_text_is_asked_about_in_words() {
    let dir = folder("nofit");
    let file = dir.join("a.txt");
    std::fs::write(&file, "one\n").unwrap();
    let binary = dir.join("b.bin");
    std::fs::write(&binary, [0xff, 0xfe, 0x00, 0x80]).unwrap();
    let mut s = State::new(&dir);
    let (got, _) = s.line(&request("Edit", json!({"file_path": file, "old_string": "absent", "new_string": "x"})));
    assert!(matches!(&got[0], Event::Permission(p) if p.kind == PermissionKind::Other));
    let (got, _) = s.line(&request("Write", json!({"file_path": binary, "content": "x"})));
    assert!(matches!(&got[0], Event::Permission(p) if p.kind == PermissionKind::Other));
}

#[test]
fn proposed_text_handles_each_edit_tool_and_the_files_line_ends() {
    let edit = json!({"file_path": "/p/a", "old_string": "b", "new_string": "X", "replace_all": true});
    assert_eq!(proposed_text("Edit", &edit, Some("abcb")).unwrap().1, "aXcX");
    let once = json!({"file_path": "/p/a", "old_string": "b", "new_string": "X"});
    assert_eq!(proposed_text("Edit", &once, Some("abcb")), None, "ambiguous, so the tool itself would refuse it");
    let multi = json!({"file_path": "/p/a", "edits": [{"old_string": "a", "new_string": "1"}, {"old_string": "1b", "new_string": "2"}]});
    assert_eq!(proposed_text("MultiEdit", &multi, Some("abc")).unwrap().1, "2c");
    let crlf = json!({"file_path": "/p/a", "old_string": "one\ntwo", "new_string": "one\n2"});
    assert_eq!(proposed_text("Edit", &crlf, Some("one\r\ntwo\r\n")).unwrap().1, "one\r\n2\r\n");
    let create = json!({"file_path": "/p/a", "old_string": "", "new_string": "hi"});
    assert_eq!(proposed_text("Edit", &create, None).unwrap().1, "hi");
    assert_eq!(proposed_text("Bash", &json!({"command": "ls"}), None), None);
}

#[test]
fn allowing_a_command_for_the_session_sends_the_suggested_rules_made_to_last_only_the_session() {
    let mut s = state();
    let (got, _) = s.line(&request("Bash", json!({"command": "ls -l", "description": "List"})));
    let Event::Permission(permission) = &got[0] else { panic!() };
    assert_eq!(permission.title, "Run ls -l");
    assert_eq!(permission.kind, PermissionKind::Command("ls -l".into()));
    let answer = sent(s.command(Command::Answer { id: "r1".into(), answer: Answer::Allow { text: None, for_session: true } }));
    assert_eq!(answer["updatedPermissions"][0]["destination"], "session", "not localSettings, which would be kept in a file");
    assert_eq!(answer["updatedPermissions"][0]["rules"][0]["toolName"], "Bash");
}

#[test]
fn refusing_says_why_and_a_request_taken_back_is_forgotten() {
    let mut s = state();
    s.line(&request("Bash", json!({"command": "rm -r x"})));
    let answer = sent(s.command(Command::Answer { id: "r1".into(), answer: Answer::Refuse("No.".into()) }));
    assert_eq!(answer, json!({"behavior": "deny", "message": "No."}));
    assert!(s.command(Command::Answer { id: "r1".into(), answer: Answer::Refuse("again".into()) }).is_empty(), "answered once");
    s.line(&request("Bash", json!({"command": "x"})));
    let got = s.line(&json!({"type": "control_cancel_request", "request_id": "r1"}).to_string()).0;
    assert_eq!(got, vec![Event::PermissionGone { id: "r1".into() }]);
}

#[test]
fn commands_become_the_requests_claude_code_takes() {
    let mut s = state();
    let line = |lines: Vec<String>| serde_json::from_str::<Value>(&lines[0]).unwrap();
    assert_eq!(line(s.command(Command::Prompt("hi".into())))["message"]["content"], "hi");
    assert_eq!(line(s.command(Command::SetModel(Some("haiku".into()))))["request"], json!({"subtype": "set_model", "model": "haiku"}));
    assert_eq!(line(s.command(Command::SetModel(None)))["request"]["model"], Value::Null);
    assert_eq!(line(s.command(Command::SetEffort("high".into())))["request"], json!({"subtype": "apply_flag_settings", "settings": {"effortLevel": "high"}}));
    assert_eq!(line(s.command(Command::SetMode(Mode::Plan)))["request"], json!({"subtype": "set_permission_mode", "mode": "plan"}));
    assert_eq!(line(s.command(Command::StopTask("t1".into())))["request"], json!({"subtype": "stop_task", "task_id": "t1"}));
    assert_eq!(line(s.command(Command::Interrupt))["request"]["subtype"], "interrupt");
    let ids: std::collections::HashSet<_> = (0..5).map(|_| line(s.command(Command::AskContext))["request_id"].as_str().unwrap().to_string()).collect();
    assert_eq!(ids.len(), 5, "each request has its own id");
}

#[test]
fn a_refused_change_of_mode_is_told() {
    let mut s = state();
    let line = s.command(Command::SetMode(Mode::Bypass)).remove(0);
    let id = serde_json::from_str::<Value>(&line).unwrap()["request_id"].as_str().unwrap().to_string();
    let got = events(&mut s, json!({"type": "control_response", "response": {"subtype": "error", "request_id": id, "error": "Cannot set permission mode to bypassPermissions"}}));
    assert!(matches!(&got[0], Event::Error(text) if text.contains("bypassPermissions")));
}

// ---------- A whole session, against the stand-in program ----------

/// The stand-in is a Python script, which is run by whichever Python this computer has.
fn python() -> Option<String> {
    ["python3", "python"].iter().find(|name| crate::platform::command(name).arg("--version").output().is_ok_and(|out| out.status.success())).map(|name| name.to_string())
}

pub(crate) fn fake_options(dir: &Path, extra: &[&str]) -> Option<Options> {
    let Some(program) = python() else {
        eprintln!("No Python here, so the stand-in for Claude Code can't run: this test did nothing.");
        return None;
    };
    let script = Path::new(env!("CARGO_MANIFEST_DIR")).join("src/agent_testdata/fake_claude.py");
    let mut prefix = vec![script.display().to_string(), "--log".to_string(), dir.join("log.txt").display().to_string()];
    prefix.extend(extra.iter().map(|e| e.to_string()));
    Some(Options { program, prefix, folder: dir.to_path_buf(), mode: Mode::Ask, model: None, effort: None, resume: None, ide_port: None, mcp: None, wsl: None, wake: Arc::new(|| {}) })
}

fn fake_session(dir: &Path, extra: &[&str]) -> Option<Session> {
    let options = fake_options(dir, extra)?;
    let folder = options.folder.clone();
    Some(Session::new(Tool::Claude, &folder, crate::agent_claude::start(options).unwrap()))
}

/// Takes what the stand-in says until `done` holds.
pub(crate) fn wait_for(session: &mut Session, what: &str, done: impl Fn(&Session) -> bool) {
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

fn texts(session: &Session) -> Vec<String> {
    session.items.iter().filter_map(|i| if let Item::Text { text, .. } = i { Some(text.clone()) } else { None }).collect()
}

#[test]
fn a_session_starts_streams_a_reply_and_reports_what_it_used() {
    let dir = folder("run-hello");
    let Some(mut s) = fake_session(&dir, &[]) else { return };
    wait_for(&mut s, "the models", |s| !s.models.is_empty());
    assert_eq!(s.models[0].value, "default");
    s.say("hello");
    wait_for(&mut s, "the end of the turn", |s| !s.busy && s.context.is_some());
    assert_eq!(texts(&s), vec!["Hello there".to_string()], "streamed and finished text made one message");
    assert_eq!(s.session_id.as_deref(), Some("fake-session-1"));
    assert_eq!(s.model.as_deref(), Some("claude-haiku-5-5"));
    assert_eq!(s.title, "hello");
    assert_eq!(s.tokens, Tokens { input: 110, output: 20, cached: 100 });
    assert_eq!(s.cost, Some(0.0125));
    assert_eq!(s.context, Some(Context { used: 250_000, max: 1_000_000 }));
    wait_for(&mut s, "the limits", |s| s.limits.any());
    assert_eq!(s.limits.five_hour.unwrap().percent, 42.0);
    assert!(s.limits.week.is_some());
}

#[test]
fn without_plan_limits_none_are_shown() {
    let dir = folder("run-nolimits");
    let Some(mut s) = fake_session(&dir, &["--no-limits"]) else { return };
    s.say("hello");
    wait_for(&mut s, "the context", |s| s.context.is_some());
    std::thread::sleep(Duration::from_millis(200));
    s.pump();
    assert!(!s.limits.any());
}

#[test]
fn a_command_permission_is_answered_each_way() {
    let dir = folder("run-bash");
    let Some(mut s) = fake_session(&dir, &[]) else { return };
    s.say("bash");
    wait_for(&mut s, "the request", |s| !s.waiting().is_empty());
    let permission = s.waiting()[0].clone();
    assert_eq!(permission.title, "Run cargo test");
    assert_eq!(permission.kind, PermissionKind::Command("cargo test".into()));
    s.answer(&permission.id, Answer::Allow { text: None, for_session: true });
    wait_for(&mut s, "the end", |s| !s.busy);
    let tool = s.items.iter().find_map(|i| if let Item::Tool { summary, result, .. } = i { Some((summary.clone(), result.clone())) } else { None }).unwrap();
    assert_eq!(tool.0, "Ran cargo test");
    assert!(tool.1.unwrap().text.contains("3 failed"));
    let log = log_lines(&dir);
    let response = log.iter().find(|l| l["type"] == "control_response").unwrap();
    assert_eq!(response["response"]["response"]["behavior"], "allow");
    assert_eq!(response["response"]["response"]["updatedPermissions"][0]["destination"], "session");
    assert!(matches!(&s.items.iter().find(|i| matches!(i, Item::Permission { .. })), Some(Item::Permission { decided: Decided::Allowed { for_session: true }, .. })));

    s.say("bash");
    wait_for(&mut s, "the second request", |s| !s.waiting().is_empty());
    let id = s.waiting()[0].id.clone();
    s.answer(&id, Answer::Refuse("Not now.".into()));
    wait_for(&mut s, "the end", |s| !s.busy);
    assert!(texts(&s).contains(&"Not run.".to_string()));
    let log = log_lines(&dir);
    let refusal = log.iter().rfind(|l| l["type"] == "control_response").unwrap();
    assert_eq!(refusal["response"]["response"], json!({"behavior": "deny", "message": "Not now."}));
}

#[test]
fn an_edit_allowed_makes_the_change_and_one_the_person_changed_makes_theirs() {
    let dir = folder("run-edit");
    let file = dir.join("a.txt");
    std::fs::write(&file, "one\ntwo\n").unwrap();
    let Some(mut s) = fake_session(&dir, &[]) else { return };
    s.say(&format!("edit {}", file.display()));
    wait_for(&mut s, "the request", |s| !s.waiting().is_empty());
    let permission = s.waiting()[0].clone();
    let PermissionKind::Edit { proposed, .. } = &permission.kind else { panic!("{permission:?}") };
    assert_eq!(proposed, "one\nthree\n");
    s.answer(&permission.id, Answer::Allow { text: Some("one\nTHREE\n".into()), for_session: false });
    wait_for(&mut s, "the end", |s| !s.busy);
    assert_eq!(std::fs::read_to_string(&file).unwrap(), "one\nTHREE\n", "the file is what was reviewed");

    std::fs::write(&file, "one\ntwo\n").unwrap();
    s.say(&format!("edit {}", file.display()));
    wait_for(&mut s, "the request", |s| !s.waiting().is_empty());
    let id = s.waiting()[0].id.clone();
    s.answer(&id, Answer::Allow { text: Some("one\nthree\n".into()), for_session: false });
    wait_for(&mut s, "the end", |s| !s.busy);
    assert_eq!(std::fs::read_to_string(&file).unwrap(), "one\nthree\n");

    std::fs::write(&file, "one\ntwo\n").unwrap();
    s.say(&format!("edit {}", file.display()));
    wait_for(&mut s, "the request", |s| !s.waiting().is_empty());
    let id = s.waiting()[0].id.clone();
    s.answer(&id, Answer::Refuse("I'd rather not.".into()));
    wait_for(&mut s, "the end", |s| !s.busy);
    assert_eq!(std::fs::read_to_string(&file).unwrap(), "one\ntwo\n", "refused: not changed");
    assert!(texts(&s).contains(&"Not edited.".to_string()));
}

#[test]
fn subagents_show_their_progress_and_tools_inside_them_stay_out_of_the_transcript() {
    let dir = folder("run-agent");
    let Some(mut s) = fake_session(&dir, &[]) else { return };
    s.say("agent");
    wait_for(&mut s, "the end", |s| !s.busy);
    assert_eq!(s.tasks.len(), 1);
    let task = &s.tasks[0];
    assert_eq!(task.description, "Read the files", "the notification's summary");
    assert_eq!((task.tokens, task.tool_uses), (16100, 2));
    assert_eq!(task.last_tool.as_deref(), Some("Read"));
    assert_eq!(task.state, TaskState::Done);
    let inner = s.items.iter().filter(|i| matches!(i, Item::Tool { parent: Some(_), .. })).count();
    let outer = s.items.iter().filter(|i| matches!(i, Item::Tool { parent: None, .. })).count();
    assert_eq!((inner, outer), (1, 1));
}

#[test]
fn stopping_interrupts_the_turn_and_the_session_carries_on() {
    let dir = folder("run-stop");
    let Some(mut s) = fake_session(&dir, &[]) else { return };
    s.say("slow");
    wait_for(&mut s, "some work", |s| texts(s).iter().any(|t| t.contains("Working")));
    assert!(s.busy);
    s.stop();
    wait_for(&mut s, "the stop", |s| !s.busy);
    assert!(s.items.iter().any(|i| matches!(i, Item::Notice(text) if text == "Stopped.")));
    assert!(!s.items.iter().any(|i| matches!(i, Item::Error(_))), "stopping isn't an error: {:?}", s.items);
    assert!(log_lines(&dir).iter().any(|l| l["request"]["subtype"] == "interrupt"));
    s.say("hello");
    wait_for(&mut s, "a reply after the stop", |s| texts(s).contains(&"Hello there".to_string()));
}

#[test]
fn a_message_typed_while_it_works_waits_and_goes_next() {
    let dir = folder("run-queue");
    let Some(mut s) = fake_session(&dir, &[]) else { return };
    s.say("slow");
    wait_for(&mut s, "some work", |s| texts(s).iter().any(|t| t.contains("Working")));
    s.say("hello");
    assert_eq!(s.queue, vec!["hello".to_string()]);
    assert!(!log_lines(&dir).iter().any(|l| l["message"]["content"] == "hello"), "not sent yet");
    s.stop();
    wait_for(&mut s, "the queued reply", |s| texts(s).contains(&"Hello there".to_string()) && !s.busy);
    assert!(s.queue.is_empty());
    let prompts: Vec<_> = log_lines(&dir).iter().filter(|l| l["type"] == "user").map(|l| l["message"]["content"].as_str().unwrap().to_string()).collect();
    assert_eq!(prompts, vec!["slow", "hello"]);
}

#[test]
fn model_effort_and_mode_can_be_changed_while_it_runs() {
    let dir = folder("run-control");
    let Some(mut s) = fake_session(&dir, &["--no-bypass"]) else { return };
    wait_for(&mut s, "the models", |s| !s.models.is_empty());
    s.set_model(Some("haiku"));
    s.set_effort("low");
    s.set_mode(Mode::Plan);
    wait_for(&mut s, "the changes", |s| s.model.as_deref() == Some("fake-haiku") && s.effort.as_deref() == Some("low") && s.mode == Mode::Plan);
    assert_eq!(s.efforts(), vec!["low", "high"], "haiku takes fewer efforts");
    s.set_mode(Mode::Bypass);
    wait_for(&mut s, "the refusal", |s| s.items.iter().any(|i| matches!(i, Item::Error(text) if text.contains("bypassPermissions"))));
    assert_eq!(s.mode, Mode::Plan, "a mode the program refused isn't shown as chosen");
    let log = log_lines(&dir);
    assert!(log.iter().any(|l| l["request"]["subtype"] == "apply_flag_settings" && l["request"]["settings"]["effortLevel"] == "low"));
}

#[test]
fn a_title_the_program_gives_is_used_and_a_rate_limit_event_is_shown() {
    let dir = folder("run-title");
    let Some(mut s) = fake_session(&dir, &["--no-limits"]) else { return };
    s.say("title");
    wait_for(&mut s, "the title", |s| s.title == "Tidy the parser" && !s.busy);
    s.say("limit");
    wait_for(&mut s, "the limit", |s| s.limits.five_hour.is_some_and(|l| l.percent == 50.0));
}

#[test]
fn lines_that_are_not_messages_do_not_break_the_session() {
    let dir = folder("run-garbage");
    let Some(mut s) = fake_session(&dir, &[]) else { return };
    s.say("garbage");
    wait_for(&mut s, "the reply", |s| !s.busy && texts(s).contains(&"Still here".to_string()));
    assert!(s.items.iter().any(|i| matches!(i, Item::Notice(text) if text.contains("this is not json"))));
    assert!(s.finished.is_none());
    s.say("hello");
    wait_for(&mut s, "a reply after", |s| texts(s).contains(&"Hello there".to_string()));
}

#[test]
fn the_program_dying_ends_the_session_with_what_it_said() {
    let dir = folder("run-die");
    let Some(mut s) = fake_session(&dir, &[]) else { return };
    s.say("die");
    wait_for(&mut s, "the end", |s| s.finished.is_some());
    let why = s.finished.clone().unwrap();
    assert!(why.contains("something went badly wrong"), "{why}");
    assert!(!s.busy);
}

#[test]
fn a_program_that_isnt_there_is_an_error_not_a_crash() {
    let options = Options { program: "/no/such/claude".into(), prefix: Vec::new(), folder: std::env::temp_dir(), mode: Mode::Ask, model: None, effort: None, resume: None, ide_port: None, mcp: None, wsl: None, wake: Arc::new(|| {}) };
    assert!(start(options).is_err());
}

#[test]
fn the_models_can_be_asked_for_without_a_conversation() {
    let dir = folder("probe");
    let Some(options) = fake_options(&dir, &[]) else { return };
    let models = probe_models(&options.program, &options.prefix, &dir).unwrap();
    assert_eq!(models.iter().map(|m| m.value.as_str()).collect::<Vec<_>>(), vec!["default", "haiku"]);
    assert!(log_lines(&dir).iter().all(|l| l["type"] != "user"), "nothing was said to it");
}
