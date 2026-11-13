//! Talking to language servers: the Language Server Protocol, JSON-RPC messages with a Content-Length header,
//! over a server's standard input and output. A server is started for a language and a project folder, told about
//! the files open in it and their changes, and asked for completion, hover, definitions, uses and renames; what it
//! says about problems comes as it likes.
//!
//! Positions are lines and UTF-16 code units, as the protocol has them by default; the editor works in characters,
//! so they're turned one into the other on the way.

use serde_json::{Value, json};
use std::io::{BufRead, BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{Receiver, Sender, channel};
use std::time::{Duration, Instant};

/// A language and the command that starts its server, as Settings has them.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ServerSetting {
    pub language: String,
    pub command: String,
}

/// The servers Limoniter knows to look for: the language as the highlighter names it, and the command.
pub fn usual_servers() -> Vec<ServerSetting> {
    [
        ("Rust", "rust-analyzer"),
        ("C", "clangd"),
        ("C++", "clangd"),
        ("Python", "pylsp"),
        ("JavaScript", "typescript-language-server --stdio"),
        ("TypeScript", "typescript-language-server --stdio"),
        ("Go", "gopls"),
        ("Java", "jdtls"),
        ("Lua", "lua-language-server"),
        ("Bourne Again Shell (bash)", "bash-language-server start"),
    ]
    .iter()
    .map(|(language, command)| ServerSetting { language: language.to_string(), command: command.to_string() })
    .collect()
}

/// Whether a program can be found on the PATH, as a server's command would be.
pub fn on_path(program: &str) -> bool {
    if program.is_empty() {
        return false;
    }
    if Path::new(program).is_file() {
        return true;
    }
    let endings: &[&str] = if cfg!(windows) { &[".exe", ".cmd", ".bat", ""] } else { &[""] };
    std::env::var_os("PATH").is_some_and(|paths| std::env::split_paths(&paths).any(|dir| endings.iter().any(|e| dir.join(format!("{program}{e}")).is_file())))
}

/// A file's address as the protocol names it.
pub fn uri(path: &Path) -> String {
    let full = std::path::absolute(path).unwrap_or_else(|_| path.to_path_buf());
    let text = full.to_string_lossy().replace('\\', "/");
    let encoded: String = text.chars().map(|c| match c {
        ' ' => "%20".to_string(),
        '#' => "%23".to_string(),
        '%' => "%25".to_string(),
        c => c.to_string(),
    }).collect();
    if encoded.starts_with('/') { format!("file://{encoded}") } else { format!("file:///{encoded}") }
}

/// A file's path from its address.
pub fn path_of(uri: &str) -> Option<PathBuf> {
    let rest = uri.strip_prefix("file://")?;
    let mut decoded = String::new();
    let bytes = rest.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() + 1 && let Ok(b) = u8::from_str_radix(std::str::from_utf8(bytes.get(i + 1..i + 3)?).ok()?, 16) {
            decoded.push(char::from(b));
            i += 3;
        } else {
            decoded.push(char::from(bytes[i]));
            i += 1;
        }
    }
    // file:///C:/x is C:\x on Windows.
    if decoded.len() > 3 && decoded.as_bytes()[0] == b'/' && decoded.as_bytes()[2] == b':' {
        decoded.remove(0);
    }
    if cfg!(windows) {
        decoded = decoded.replace('/', "\\");
    }
    Some(PathBuf::from(decoded))
}

/// A column in characters as UTF-16 code units, on a line.
pub fn to_utf16(line: &str, col: usize) -> usize {
    line.chars().take(col).map(char::len_utf16).sum()
}

/// A column in UTF-16 code units as characters.
pub fn from_utf16(line: &str, units: usize) -> usize {
    let mut count = 0;
    for (i, c) in line.chars().enumerate() {
        if count >= units {
            return i;
        }
        count += c.len_utf16();
    }
    line.chars().count()
}

/// A language server running.
pub struct Server {
    pub name: String,
    pub root: PathBuf,
    writer: Box<dyn Write + Send>,
    incoming: Receiver<Value>,
    next_id: i64,
    /// What the server said it can do, once it's said.
    pub capabilities: Option<Value>,
    initialize_id: i64,
    child: Option<std::process::Child>,
    /// Messages that came while waiting for a particular answer, to be dealt with after.
    pub waiting: Vec<Value>,
}

impl Server {
    /// Starts a server's program in a folder, and says hello.
    pub fn start(command: &str, root: &Path) -> Result<Server, String> {
        let parts = crate::emulators::split_options(command);
        let (program, args) = parts.split_first().ok_or("No command for the language server.")?;
        let mut child = std::process::Command::new(program)
            .args(args)
            .current_dir(root)
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::null())
            .spawn()
            .map_err(|e| format!("The language server {program} couldn't be started: {e}"))?;
        let stdin = child.stdin.take().ok_or("No way to talk to the language server.")?;
        let stdout = child.stdout.take().ok_or("No way to hear the language server.")?;
        let mut server = Server::over(program.clone(), root.to_path_buf(), Box::new(stdin), stdout);
        server.child = Some(child);
        Ok(server)
    }

    /// Talks to a server over any two streams: its input and its output.
    pub fn over(name: String, root: PathBuf, writer: Box<dyn Write + Send>, reader: impl Read + Send + 'static) -> Server {
        let (send, incoming) = channel();
        std::thread::spawn(move || read_messages(reader, &send));
        let mut server = Server { name, root, writer, incoming, next_id: 1, capabilities: None, initialize_id: 0, child: None, waiting: Vec::new() };
        let root_uri = uri(&server.root);
        let capabilities = json!({
            "textDocument": {
                "synchronization": { "didSave": false },
                "completion": { "completionItem": { "snippetSupport": false } },
                "hover": { "contentFormat": ["plaintext", "markdown"] },
                "definition": {},
                "references": {},
                "rename": {},
                "publishDiagnostics": {}
            },
            "general": { "positionEncodings": ["utf-16"] }
        });
        server.initialize_id = server.request("initialize", json!({ "processId": std::process::id(), "rootUri": root_uri, "capabilities": capabilities, "clientInfo": { "name": "Limoniter" } }));
        server
    }

    fn send(&mut self, message: &Value) {
        let body = message.to_string();
        let _ = self.writer.write_all(format!("Content-Length: {}\r\n\r\n{body}", body.len()).as_bytes());
        let _ = self.writer.flush();
    }

    pub fn request(&mut self, method: &str, params: Value) -> i64 {
        let id = self.next_id;
        self.next_id += 1;
        self.send(&json!({ "jsonrpc": "2.0", "id": id, "method": method, "params": params }));
        id
    }

    pub fn notify(&mut self, method: &str, params: Value) {
        self.send(&json!({ "jsonrpc": "2.0", "method": method, "params": params }));
    }

    /// What's come in since last time, initialising on the way when the server answers hello.
    pub fn poll(&mut self) -> Vec<Value> {
        let mut out = std::mem::take(&mut self.waiting);
        while let Ok(message) = self.incoming.try_recv() {
            out.push(message);
        }
        out.retain(|message| self.handle_own(message));
        out
    }

    /// Deals with what's the client's own business: the answer to initialize, and requests from the server, which
    /// are answered with nothing. False if it's dealt with.
    fn handle_own(&mut self, message: &Value) -> bool {
        if message.get("id").and_then(Value::as_i64) == Some(self.initialize_id) && message.get("method").is_none() {
            self.capabilities = Some(message.pointer("/result/capabilities").cloned().unwrap_or(Value::Null));
            self.notify("initialized", json!({}));
            return false;
        }
        if let (Some(id), Some(_)) = (message.get("id"), message.get("method")) {
            let id = id.clone();
            self.send(&json!({ "jsonrpc": "2.0", "id": id, "result": Value::Null }));
            return false;
        }
        true
    }

    pub fn ready(&self) -> bool {
        self.capabilities.is_some()
    }

    /// Waits for the answer to a request, up to `wait`, keeping everything else that comes for later.
    pub fn wait_for(&mut self, id: i64, wait: Duration) -> Option<Value> {
        let began = Instant::now();
        loop {
            for message in self.poll() {
                if message.get("id").and_then(Value::as_i64) == Some(id) && message.get("method").is_none() {
                    // Whatever else came stays for the next poll.
                    return Some(message.get("result").cloned().unwrap_or(Value::Null));
                }
                self.waiting.push(message);
            }
            if began.elapsed() > wait {
                return None;
            }
            match self.incoming.recv_timeout(Duration::from_millis(20)) {
                Ok(message) => self.waiting.push(message),
                Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
                Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => return None,
            }
        }
    }

    /// Waits until the server has said hello back.
    #[cfg(test)]
    pub fn wait_ready(&mut self, wait: Duration) -> bool {
        let began = Instant::now();
        while !self.ready() && began.elapsed() < wait {
            let came = self.poll();
            self.waiting.extend(came);
            if !self.ready() {
                std::thread::sleep(Duration::from_millis(20));
            }
        }
        self.ready()
    }

    pub fn did_open(&mut self, path: &Path, language: &str, version: u64, text: &str) {
        self.notify("textDocument/didOpen", json!({ "textDocument": { "uri": uri(path), "languageId": language_id(language), "version": version, "text": text } }));
    }

    pub fn did_change(&mut self, path: &Path, version: u64, text: &str) {
        self.notify("textDocument/didChange", json!({ "textDocument": { "uri": uri(path), "version": version }, "contentChanges": [{ "text": text }] }));
    }

    pub fn did_close(&mut self, path: &Path) {
        self.notify("textDocument/didClose", json!({ "textDocument": { "uri": uri(path) } }));
    }

    /// Asks about a place in a file: completion, hover, definition or references.
    pub fn at(&mut self, method: &str, path: &Path, line: usize, units: usize) -> i64 {
        let mut params = json!({ "textDocument": { "uri": uri(path) }, "position": { "line": line, "character": units } });
        if method == "textDocument/references" {
            params["context"] = json!({ "includeDeclaration": true });
        }
        self.request(method, params)
    }

    pub fn rename(&mut self, path: &Path, line: usize, units: usize, new_name: &str) -> i64 {
        self.request("textDocument/rename", json!({ "textDocument": { "uri": uri(path) }, "position": { "line": line, "character": units }, "newName": new_name }))
    }

    /// Says goodbye and stops it.
    pub fn stop(&mut self) {
        let id = self.request("shutdown", Value::Null);
        let _ = self.wait_for(id, Duration::from_millis(500));
        self.notify("exit", Value::Null);
        if let Some(child) = self.child.as_mut() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        if let Some(child) = self.child.as_mut() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

/// The protocol's name for a language.
pub fn language_id(language: &str) -> String {
    match language {
        "C++" => "cpp".into(),
        "C#" => "csharp".into(),
        "Bourne Again Shell (bash)" => "shellscript".into(),
        other => other.to_ascii_lowercase(),
    }
}

/// Reads messages until the stream ends: headers, a blank line, and Content-Length bytes of JSON.
fn read_messages(reader: impl Read, send: &Sender<Value>) {
    let mut reader = BufReader::new(reader);
    loop {
        let mut length = None;
        loop {
            let mut header = String::new();
            match reader.read_line(&mut header) {
                Ok(0) | Err(_) => return,
                Ok(_) => {}
            }
            let header = header.trim();
            if header.is_empty() {
                break;
            }
            if let Some(value) = header.strip_prefix("Content-Length:") {
                length = value.trim().parse::<usize>().ok();
            }
        }
        let Some(length) = length else {
            continue;
        };
        let mut body = vec![0; length];
        if reader.read_exact(&mut body).is_err() {
            return;
        }
        if let Ok(message) = serde_json::from_slice::<Value>(&body)
            && send.send(message).is_err()
        {
            return;
        }
    }
}

/// A place a server named: a file, and a line and character column.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Location {
    pub path: PathBuf,
    pub line: usize,
    /// In UTF-16 units, to be turned into characters against the line.
    pub units: usize,
}

/// The places in a definition or references answer: a Location, a list of them, or LocationLinks.
pub fn locations(result: &Value) -> Vec<Location> {
    let one = |v: &Value| -> Option<Location> {
        let uri = v.get("uri").or_else(|| v.get("targetUri"))?.as_str()?;
        let range = v.get("range").or_else(|| v.get("targetSelectionRange"))?;
        Some(Location { path: path_of(uri)?, line: range.pointer("/start/line")?.as_u64()? as usize, units: range.pointer("/start/character")?.as_u64()? as usize })
    };
    match result {
        Value::Array(items) => items.iter().filter_map(one).collect(),
        Value::Object(_) => one(result).into_iter().collect(),
        _ => Vec::new(),
    }
}

/// The words a completion answer offers: each item's text to put in, best first as the server sorted them.
pub fn completion_words(result: &Value) -> Vec<String> {
    let items = result.get("items").unwrap_or(result).as_array().cloned().unwrap_or_default();
    let mut sorted: Vec<(String, String)> = items
        .iter()
        .filter_map(|item| {
            let text = item.pointer("/textEdit/newText").or_else(|| item.get("insertText")).or_else(|| item.get("label"))?.as_str()?.to_string();
            let sort = item.get("sortText").and_then(Value::as_str).unwrap_or(&text).to_string();
            Some((sort, text))
        })
        .collect();
    sorted.sort();
    let mut words: Vec<String> = sorted.into_iter().map(|(_, text)| text).collect();
    words.dedup();
    words.truncate(60);
    words
}

/// A hover answer as plain text.
pub fn hover_text(result: &Value) -> Option<String> {
    let contents = result.get("contents")?;
    let text = match contents {
        Value::String(s) => s.clone(),
        Value::Array(parts) => parts.iter().filter_map(|p| p.as_str().map(str::to_string).or_else(|| p.get("value").and_then(Value::as_str).map(str::to_string))).collect::<Vec<_>>().join("\n"),
        Value::Object(_) => contents.get("value")?.as_str()?.to_string(),
        _ => return None,
    };
    // Markdown's code fences go; the rest reads well enough as it is.
    let text: String = text.lines().filter(|l| !l.trim_start().starts_with("```")).collect::<Vec<_>>().join("\n");
    let text = text.trim().to_string();
    (!text.is_empty()).then(|| text.chars().take(1500).collect())
}

/// The problems in a publishDiagnostics notification: the file, and each problem's line and words.
pub fn diagnostics(message: &Value) -> Option<(PathBuf, Vec<(usize, String)>)> {
    let params = message.get("params")?;
    let path = path_of(params.get("uri")?.as_str()?)?;
    let list = params.get("diagnostics")?.as_array()?;
    let problems = list
        .iter()
        .filter(|d| d.get("severity").and_then(Value::as_u64).is_none_or(|s| s <= 2))
        .filter_map(|d| {
            let line = d.pointer("/range/start/line")?.as_u64()? as usize;
            let words = d.get("message")?.as_str()?.lines().next().unwrap_or("").to_string();
            let source = d.get("source").and_then(Value::as_str).map(|s| format!(" ({s})")).unwrap_or_default();
            Some((line, format!("{words}{source}")))
        })
        .collect();
    Some((path, problems))
}

/// The edits in a rename's answer: for each file, its changes as (line, from, to, text), the columns in UTF-16.
pub fn workspace_edits(result: &Value) -> Vec<(PathBuf, Vec<(usize, usize, usize, usize, String)>)> {
    let mut out: Vec<(PathBuf, Vec<(usize, usize, usize, usize, String)>)> = Vec::new();
    let edit_of = |e: &Value| -> Option<(usize, usize, usize, usize, String)> {
        Some((
            e.pointer("/range/start/line")?.as_u64()? as usize,
            e.pointer("/range/start/character")?.as_u64()? as usize,
            e.pointer("/range/end/line")?.as_u64()? as usize,
            e.pointer("/range/end/character")?.as_u64()? as usize,
            e.get("newText")?.as_str()?.to_string(),
        ))
    };
    if let Some(changes) = result.get("changes").and_then(Value::as_object) {
        for (uri, edits) in changes {
            if let Some(path) = path_of(uri) {
                out.push((path, edits.as_array().map(|list| list.iter().filter_map(edit_of).collect()).unwrap_or_default()));
            }
        }
    }
    if let Some(documents) = result.get("documentChanges").and_then(Value::as_array) {
        for document in documents {
            if let (Some(uri), Some(edits)) = (document.pointer("/textDocument/uri").and_then(Value::as_str), document.get("edits").and_then(Value::as_array))
                && let Some(path) = path_of(uri)
            {
                out.push((path, edits.iter().filter_map(edit_of).collect()));
            }
        }
    }
    out
}

/// The folder a server starts in: the nearest above the file with a project's own file (Cargo.toml, .git and
/// the like), or the file's own.
pub fn root_for(path: &Path) -> PathBuf {
    let start = path.parent().map(Path::to_path_buf).unwrap_or_default();
    let marks = ["Cargo.toml", ".git", "package.json", "go.mod", "pyproject.toml", "setup.py", "compile_commands.json", "CMakeLists.txt", "Makefile"];
    let mut folder = Some(start.as_path());
    while let Some(here) = folder {
        if marks.iter().any(|m| here.join(m).exists()) {
            return here.to_path_buf();
        }
        folder = here.parent();
    }
    start
}

#[cfg(test)]
pub mod tests {
    use super::*;

    /// A stand-in language server in a thread: it answers initialize, sends a problem when a file is opened,
    /// and answers completion, hover, definition, references and rename with fixed answers.
    pub fn fake_server(root: &Path) -> Server {
        let (to_server_read, to_server_write) = std::io::pipe().unwrap();
        let (from_server_read, from_server_write) = std::io::pipe().unwrap();
        std::thread::spawn(move || {
            let (send, incoming) = channel();
            std::thread::spawn(move || read_messages(to_server_read, &send));
            let mut out = from_server_write;
            let mut say = |message: Value| {
                let body = message.to_string();
                let _ = out.write_all(format!("Content-Length: {}\r\n\r\n{body}", body.len()).as_bytes());
                let _ = out.flush();
            };
            while let Ok(message) = incoming.recv() {
                let method = message.get("method").and_then(Value::as_str).unwrap_or("").to_string();
                let id = message.get("id").cloned();
                let uri = message.pointer("/params/textDocument/uri").and_then(Value::as_str).unwrap_or("").to_string();
                let answer = match method.as_str() {
                    "initialize" => json!({ "capabilities": { "completionProvider": {}, "hoverProvider": true } }),
                    "textDocument/didOpen" | "textDocument/didChange" => {
                        let text = message.pointer("/params/textDocument/text").or_else(|| message.pointer("/params/contentChanges/0/text")).and_then(Value::as_str).unwrap_or("");
                        let bad: Vec<Value> = text.lines().enumerate().filter(|(_, l)| l.contains("oops")).map(|(n, _)| json!({ "range": { "start": { "line": n, "character": 0 }, "end": { "line": n, "character": 4 } }, "severity": 1, "message": "oops isn't allowed", "source": "fake" })).collect();
                        say(json!({ "jsonrpc": "2.0", "method": "textDocument/publishDiagnostics", "params": { "uri": uri, "diagnostics": bad } }));
                        continue;
                    }
                    "textDocument/completion" => json!({ "isIncomplete": false, "items": [{ "label": "counter", "sortText": "1" }, { "label": "count_up", "sortText": "2" }] }),
                    "textDocument/hover" => json!({ "contents": { "kind": "markdown", "value": "```rust\nfn count_up(n: u32) -> u32\n```" } }),
                    "textDocument/definition" => json!([{ "uri": uri, "range": { "start": { "line": 0, "character": 3 }, "end": { "line": 0, "character": 11 } } }]),
                    "textDocument/references" => json!([{ "uri": uri, "range": { "start": { "line": 0, "character": 3 }, "end": { "line": 0, "character": 11 } } }, { "uri": uri, "range": { "start": { "line": 2, "character": 8 }, "end": { "line": 2, "character": 16 } } }]),
                    "textDocument/rename" => json!({ "changes": { uri.clone(): [
                        { "range": { "start": { "line": 0, "character": 3 }, "end": { "line": 0, "character": 11 } }, "newText": message.pointer("/params/newName").cloned().unwrap_or(Value::Null) },
                        { "range": { "start": { "line": 2, "character": 8 }, "end": { "line": 2, "character": 16 } }, "newText": message.pointer("/params/newName").cloned().unwrap_or(Value::Null) }
                    ] } }),
                    "shutdown" => Value::Null,
                    _ => continue,
                };
                if let Some(id) = id {
                    say(json!({ "jsonrpc": "2.0", "id": id, "result": answer }));
                }
            }
        });
        Server::over("fake".into(), root.to_path_buf(), Box::new(to_server_write), from_server_read)
    }

    #[test]
    fn a_server_is_talked_to() {
        let root = std::env::temp_dir();
        let mut server = fake_server(&root);
        assert!(server.wait_ready(Duration::from_secs(5)), "it says hello");
        let file = root.join("lib.rs");
        server.did_open(&file, "Rust", 1, "fn count_up(n: u32) -> u32 { n }\nlet x = oops;\nlet y = count_up(1);\n");
        let began = Instant::now();
        let mut problems = None;
        while problems.is_none() && began.elapsed() < Duration::from_secs(5) {
            problems = server.poll().iter().find_map(diagnostics);
            std::thread::sleep(Duration::from_millis(10));
        }
        let (path, problems) = problems.expect("problems come");
        assert_eq!(path, std::path::absolute(&file).unwrap());
        assert_eq!(problems, [(1, "oops isn't allowed (fake)".to_string())]);
        let id = server.at("textDocument/completion", &file, 2, 10);
        assert_eq!(server.wait_for(id, Duration::from_secs(5)).map(|r| completion_words(&r)), Some(vec!["counter".to_string(), "count_up".into()]));
        let id = server.at("textDocument/hover", &file, 2, 10);
        assert_eq!(server.wait_for(id, Duration::from_secs(5)).and_then(|r| hover_text(&r)).as_deref(), Some("fn count_up(n: u32) -> u32"));
        let id = server.at("textDocument/references", &file, 2, 10);
        let places = locations(&server.wait_for(id, Duration::from_secs(5)).unwrap());
        assert_eq!(places.iter().map(|p| (p.line, p.units)).collect::<Vec<_>>(), [(0, 3), (2, 8)]);
        let id = server.rename(&file, 2, 10, "count_more");
        let edits = workspace_edits(&server.wait_for(id, Duration::from_secs(5)).unwrap());
        assert_eq!(edits[0].1.len(), 2);
        server.stop();
    }

    #[test]
    fn places_and_columns_are_turned_both_ways() {
        let file = if cfg!(windows) { PathBuf::from("C:\\work\\my file.rs") } else { PathBuf::from("/work/my file.rs") };
        let address = uri(&file);
        assert!(address.starts_with("file:///") && address.ends_with("my%20file.rs"), "{address}");
        assert_eq!(path_of(&address).map(|p| p.display().to_string()), Some(file.display().to_string()), "with the system's own separators");
        // An emoji is two UTF-16 units and one character.
        assert_eq!(to_utf16("a\u{1F600}b", 2), 3);
        assert_eq!(from_utf16("a\u{1F600}b", 3), 2);
        assert_eq!(language_id("C++"), "cpp");
    }
}
