//! Past Claude Code sessions in a project, read from the logs Claude Code keeps: `projects/<folder>/<id>.jsonl` in
//! its settings folder. They're listed (title, when, first message) so one can be taken up again with `--resume`,
//! and a session's transcript is built from its log for showing straight away.
//!
//! The log's format is Claude Code's own and not promised to stay; whatever can't be read is left out.

use crate::agent_claude::describe;
use crate::agent_session::{Item, Tool, ToolResult};
use serde_json::Value;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::time::SystemTime;

/// The longest a folder's name for a project is, before Claude Code cuts it short and adds a hash.
const LONGEST_NAME: usize = 200;
/// The most sessions listed.
const MOST_LISTED: usize = 50;
/// The most of one log read to list it.
const MOST_READ: usize = 8 * 1024 * 1024;
/// The most items a transcript built from a log has.
const MOST_ITEMS: usize = 3000;

#[derive(Clone, Debug, PartialEq)]
pub struct Past {
    pub id: String,
    pub title: String,
    pub first_message: String,
    pub when: SystemTime,
    pub path: PathBuf,
    /// Whose log it is.
    pub tool: Tool,
    /// The WSL distribution whose log it is, if it was found there.
    pub wsl: Option<String>,
}

/// What Claude Code calls a project's folder in its logs: the path with each character that isn't a letter or a
/// digit made a dash.
pub fn folder_name(folder: &Path) -> String {
    folder.display().to_string().chars().map(|c| if c.is_ascii_alphanumeric() { c } else { '-' }).collect()
}

/// Where a project's logs are, under the settings folder given (CLAUDE_CONFIG_DIR, or `.claude` at home).
pub fn log_folder(config: &Path, folder: &Path) -> Option<PathBuf> {
    let projects = config.join("projects");
    let name = folder_name(folder);
    let exact = projects.join(&name);
    if exact.is_dir() {
        return Some(exact);
    }
    // A long path is cut and given a hash, which isn't worked out here: the folder that starts the same is it.
    if name.len() > LONGEST_NAME {
        let start = &name[..LONGEST_NAME];
        return std::fs::read_dir(&projects).ok()?.flatten().find(|entry| entry.file_name().to_string_lossy().starts_with(start)).map(|entry| entry.path());
    }
    None
}

/// The text of a message the person wrote, from a user line; None for a tool's result or something Claude Code wrote.
fn person_said(line: &Value) -> Option<String> {
    if line.get("isMeta").and_then(Value::as_bool).unwrap_or(false) || line.get("isSidechain").and_then(Value::as_bool).unwrap_or(false) {
        return None;
    }
    let content = line.get("message")?.get("content")?;
    let text = match content {
        Value::String(text) => text.clone(),
        Value::Array(parts) => {
            if parts.iter().any(|p| p.get("type").and_then(Value::as_str) == Some("tool_result")) {
                return None;
            }
            parts.iter().filter_map(|p| p.get("text").and_then(Value::as_str)).collect::<Vec<_>>().join("\n")
        }
        _ => return None,
    };
    let text = text.trim();
    // What Claude Code puts in for a command or an interruption isn't what was said.
    (!text.is_empty() && !text.starts_with('<') && !text.starts_with("[Request interrupted")).then(|| text.to_string())
}

/// One log, listed: its title (Claude Code's own, or the first words said), when it was last used, and the first message.
fn read_past(path: &Path) -> Option<Past> {
    let id = path.file_stem()?.to_string_lossy().into_owned();
    let when = std::fs::metadata(path).and_then(|m| m.modified()).ok()?;
    let file = std::fs::File::open(path).ok()?;
    let mut title = String::new();
    let mut first = String::new();
    let mut read = 0usize;
    for line in BufReader::new(file).lines().map_while(Result::ok) {
        read += line.len() + 1;
        if read > MOST_READ {
            break;
        }
        // Only lines that can matter are parsed: a log is mostly other things.
        let wanted_title = line.contains("\"ai-title\"") || line.contains("\"custom-title\"");
        let wanted_first = first.is_empty() && line.contains("\"type\":\"user\"");
        if !wanted_title && !wanted_first {
            continue;
        }
        let Ok(value) = serde_json::from_str::<Value>(&line) else { continue };
        match value.get("type").and_then(Value::as_str) {
            Some("ai-title") => title = value.get("aiTitle").and_then(Value::as_str).unwrap_or("").to_string(),
            Some("custom-title") => title = value.get("customTitle").and_then(Value::as_str).unwrap_or("").to_string(),
            Some("user") if first.is_empty() => first = person_said(&value).unwrap_or_default(),
            _ => {}
        }
    }
    if first.is_empty() && title.is_empty() {
        return None;
    }
    let title = if title.is_empty() { crate::agent_session::title_from(&first) } else { title };
    Some(Past { id, title, first_message: first, when, path: path.to_path_buf(), tool: Tool::Claude, wsl: None })
}

/// The past sessions of a project, the newest first.
pub fn list(config: &Path, folder: &Path) -> Vec<Past> {
    let Some(dir) = log_folder(config, folder) else { return Vec::new() };
    let mut logs: Vec<(SystemTime, PathBuf)> = std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .filter(|entry| entry.path().extension().is_some_and(|e| e == "jsonl"))
        .filter_map(|entry| Some((entry.metadata().ok()?.modified().ok()?, entry.path())))
        .collect();
    logs.sort_by_key(|log| std::cmp::Reverse(log.0));
    logs.into_iter().filter_map(|(_, path)| read_past(&path)).take(MOST_LISTED).collect()
}

/// A session's transcript from its log: what was said, the tools used with their results.
pub fn transcript(path: &Path, folder: &Path) -> Vec<Item> {
    let Ok(file) = std::fs::File::open(path) else { return Vec::new() };
    let mut items: Vec<Item> = Vec::new();
    for line in BufReader::new(file).lines().map_while(Result::ok) {
        if !(line.contains("\"type\":\"user\"") || line.contains("\"type\":\"assistant\"")) {
            continue;
        }
        let Ok(value) = serde_json::from_str::<Value>(&line) else { continue };
        if value.get("isSidechain").and_then(Value::as_bool).unwrap_or(false) {
            continue;
        }
        match value.get("type").and_then(Value::as_str) {
            Some("user") => {
                if let Some(text) = person_said(&value) {
                    items.push(Item::You(text));
                } else if let Some(parts) = value.get("message").and_then(|m| m.get("content")).and_then(Value::as_array) {
                    for part in parts.iter().filter(|p| p.get("type").and_then(Value::as_str) == Some("tool_result")) {
                        let id = part.get("tool_use_id").and_then(Value::as_str).unwrap_or("");
                        let text = match part.get("content") {
                            Some(Value::String(text)) => text.clone(),
                            Some(Value::Array(blocks)) => blocks.iter().filter_map(|b| b.get("text").and_then(Value::as_str)).collect::<Vec<_>>().join("\n"),
                            _ => String::new(),
                        };
                        let is_error = part.get("is_error").and_then(Value::as_bool).unwrap_or(false);
                        if let Some(Item::Tool { result, .. }) = items.iter_mut().rev().find(|item| matches!(item, Item::Tool { id: i, .. } if i == id)) {
                            *result = Some(ToolResult { text, is_error });
                        }
                    }
                }
            }
            Some("assistant") => {
                let message = value.get("message");
                let message_id = message.and_then(|m| m.get("id")).and_then(Value::as_str).unwrap_or("");
                for (n, block) in message.and_then(|m| m.get("content")).and_then(Value::as_array).into_iter().flatten().enumerate() {
                    match block.get("type").and_then(Value::as_str) {
                        Some("text") => {
                            let text = block.get("text").and_then(Value::as_str).unwrap_or("").trim();
                            if !text.is_empty() {
                                items.push(Item::Text { id: format!("{message_id}:{n}"), text: text.to_string(), done: true });
                            }
                        }
                        Some("thinking") => {
                            let text = block.get("thinking").and_then(Value::as_str).unwrap_or("").trim();
                            if !text.is_empty() {
                                items.push(Item::Thinking { id: format!("{message_id}:{n}"), text: text.to_string() });
                            }
                        }
                        Some("tool_use") => {
                            let name = block.get("name").and_then(Value::as_str).unwrap_or("");
                            let (summary, kind) = describe(name, block.get("input").unwrap_or(&Value::Null), folder);
                            items.push(Item::Tool { id: block.get("id").and_then(Value::as_str).unwrap_or("").to_string(), name: name.to_string(), summary, kind, parent: None, result: None });
                        }
                        _ => {}
                    }
                }
            }
            _ => {}
        }
        if items.len() > MOST_ITEMS {
            items.drain(..items.len() - MOST_ITEMS);
        }
    }
    items
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write(dir: &Path, id: &str, lines: &[&str]) -> PathBuf {
        std::fs::create_dir_all(dir).unwrap();
        let path = dir.join(format!("{id}.jsonl"));
        std::fs::write(&path, lines.join("\n") + "\n").unwrap();
        path
    }

    fn temp(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("limoniter-agent-logs-{}-{tag}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        dir
    }

    const USER: &str = r#"{"parentUuid":null,"isSidechain":false,"type":"user","message":{"role":"user","content":"Fix the parser"},"uuid":"u1"}"#;
    const ASSISTANT: &str = r#"{"parentUuid":"u1","isSidechain":false,"type":"assistant","message":{"id":"m1","role":"assistant","content":[{"type":"thinking","thinking":"Hmm"},{"type":"text","text":"Looking."},{"type":"tool_use","id":"t1","name":"Read","input":{"file_path":"/p/src/a.rs"}}]},"uuid":"a1"}"#;
    const RESULT: &str = r#"{"parentUuid":"a1","isSidechain":false,"type":"user","message":{"role":"user","content":[{"tool_use_id":"t1","type":"tool_result","content":"fn main() {}"}]},"uuid":"u2"}"#;

    #[test]
    fn a_folder_is_named_as_claude_code_names_it() {
        assert_eq!(folder_name(Path::new("/mnt/h/Limoniter")), "-mnt-h-Limoniter");
        assert_eq!(folder_name(Path::new("/tmp/a b.c")), "-tmp-a-b-c");
    }

    #[test]
    fn past_sessions_are_listed_newest_first_with_title_and_first_message() {
        let config = temp("list");
        let folder = Path::new("/work/proj");
        let dir = config.join("projects").join(folder_name(folder));
        write(&dir, "old", &[USER]);
        std::thread::sleep(std::time::Duration::from_millis(30));
        write(&dir, "new", &[USER, r#"{"type":"ai-title","aiTitle":"Parser fix","sessionId":"new"}"#, ASSISTANT]);
        write(&dir, "empty", &[r#"{"type":"queue-operation"}"#]);
        std::fs::write(dir.join("notes.txt"), "x").unwrap();
        let listed = list(&config, folder);
        let names: Vec<_> = listed.iter().map(|p| p.id.as_str()).collect();
        assert_eq!(names.first(), Some(&"new"), "the newest first: {names:?}");
        assert!(!names.contains(&"empty"), "a log with nothing said isn't a session");
        let new = listed.iter().find(|p| p.id == "new").unwrap();
        assert_eq!(new.title, "Parser fix");
        assert_eq!(new.first_message, "Fix the parser");
        let old = listed.iter().find(|p| p.id == "old").unwrap();
        assert_eq!(old.title, "Fix the parser", "without a title, the first words");
    }

    #[test]
    fn a_missing_folder_lists_nothing() {
        assert!(list(&temp("none"), Path::new("/nowhere")).is_empty());
    }

    #[test]
    fn a_transcript_has_what_was_said_and_the_tools_with_results() {
        let config = temp("transcript");
        let path = write(&config, "s", &[USER, r#"{"type":"queue-operation"}"#, ASSISTANT, RESULT, r#"{"isSidechain":true,"type":"assistant","message":{"id":"m9","content":[{"type":"text","text":"a subagent"}]}}"#, "not json at all"]);
        let items = transcript(&path, Path::new("/p"));
        assert_eq!(items.len(), 4, "{items:?}");
        assert_eq!(items[0], Item::You("Fix the parser".into()));
        assert!(matches!(&items[1], Item::Thinking { text, .. } if text == "Hmm"));
        assert!(matches!(&items[2], Item::Text { text, .. } if text == "Looking."));
        match &items[3] {
            Item::Tool { summary, result, .. } => {
                assert_eq!(summary, "Read src/a.rs");
                assert_eq!(result.as_ref().unwrap().text, "fn main() {}");
            }
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn commands_and_meta_lines_are_not_taken_for_what_the_person_said() {
        let command = r#"{"type":"user","message":{"role":"user","content":"<command-name>/clear</command-name>"}}"#;
        let meta = r#"{"type":"user","isMeta":true,"message":{"role":"user","content":"Caveat: ..."}}"#;
        assert_eq!(person_said(&serde_json::from_str(command).unwrap()), None);
        assert_eq!(person_said(&serde_json::from_str(meta).unwrap()), None);
    }

    #[test]
    fn a_long_folders_name_is_found_by_its_start() {
        let config = temp("long");
        let folder = PathBuf::from(format!("/{}", "a".repeat(250)));
        let name = folder_name(&folder);
        let dir = config.join("projects").join(format!("{}-hash123", &name[..200]));
        std::fs::create_dir_all(&dir).unwrap();
        assert_eq!(log_folder(&config, &folder), Some(dir));
    }
}
