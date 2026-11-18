//! Talking to debuggers through the Debug Adapter Protocol, as VS Code does: the adapter is a program started
//! with its standard input and output as the line, messages framed as the Language Server Protocol frames them.
//! Requests carry a number and the adapter answers each; it says what happens (stopped, output, ended) as events,
//! and may ask the editor things of its own, answered here as not done. Python is debugged with debugpy, and C,
//! C++ and Rust with gdb's own adapter (gdb -i dap, from gdb 14) or with LLVM's lldb-dap.

use serde_json::{Value, json};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{Receiver, channel};

/// Which debugger an adapter is, which says how a program is started with it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Debugpy,
    Gdb,
    Lldb,
}

impl Kind {
    /// The debugger's name, which is also the adapter's id the protocol is told.
    pub fn adapter_name(self) -> &'static str {
        match self {
            Kind::Debugpy => "debugpy",
            Kind::Gdb => "gdb",
            Kind::Lldb => "lldb-dap",
        }
    }

    /// Whether what's debugged is the file itself (a script), rather than a program built from it.
    pub fn runs_the_file(self) -> bool {
        self == Kind::Debugpy
    }
}

/// How a file's program is debugged: the adapter's command, and what's run.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Config {
    pub kind: Kind,
    pub adapter: String,
    /// The script or program run, its arguments (as typed), and the folder it runs in.
    pub program: String,
    pub arguments: String,
    pub folder: String,
}

/// The commands that start each debug adapter.
const DEBUGPY: &str = if cfg!(windows) { "py -m debugpy.adapter" } else { "python3 -m debugpy.adapter" };
const GDB: &str = "gdb -i dap";
const LLDB_DAP: &str = "lldb-dap";
const LLDB_VSCODE: &str = "lldb-vscode";

/// The debugger for a language, as the highlighter names it: the first of those that can be found.
pub fn adapter_for(language: &str) -> Option<(Kind, String)> {
    let (kind, command) = match language {
        "Python" => (Kind::Debugpy, DEBUGPY),
        "C" | "C++" | "Rust" => {
            if crate::lsp::on_path("gdb") {
                (Kind::Gdb, GDB)
            } else if crate::lsp::on_path(LLDB_DAP) {
                (Kind::Lldb, LLDB_DAP)
            } else if crate::lsp::on_path(LLDB_VSCODE) {
                (Kind::Lldb, LLDB_VSCODE)
            } else {
                // gdb is the one to install, where none is.
                (Kind::Gdb, GDB)
            }
        }
        _ => return None,
    };
    Some((kind, command.to_string()))
}

/// What's debugged for a file, as a first guess: a script itself; for Rust, the program Cargo builds for its
/// package; for C, the program named after the file beside it.
pub fn guess(kind: Kind, adapter: String, file: &Path) -> Config {
    let folder = crate::lsp::root_for(file);
    let program = if kind.runs_the_file() {
        file.display().to_string()
    } else if let Some(name) = std::fs::read_to_string(folder.join("Cargo.toml")).ok().and_then(|t| package_name(&t)) {
        folder.join("target").join("debug").join(if cfg!(windows) { format!("{name}.exe") } else { name }).display().to_string()
    } else {
        let stem = file.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
        file.with_file_name(if cfg!(windows) { format!("{stem}.exe") } else { stem }).display().to_string()
    };
    Config { kind, adapter, program, arguments: String::new(), folder: folder.display().to_string() }
}

/// The package's name in a Cargo.toml.
fn package_name(text: &str) -> Option<String> {
    let mut in_package = false;
    for line in text.lines().map(str::trim) {
        if line.starts_with('[') {
            in_package = line == "[package]";
        } else if in_package && let Some(value) = line.strip_prefix("name").map(str::trim_start).and_then(|l| l.strip_prefix('=')) {
            return Some(value.trim().trim_matches('"').to_string());
        }
    }
    None
}

/// The launch request's arguments for a configuration.
pub fn launch_arguments(config: &Config) -> Value {
    let args: Vec<String> = crate::emulators::split_options(&config.arguments);
    match config.kind {
        Kind::Debugpy => json!({ "name": "Limoniter", "type": "python", "request": "launch", "program": config.program, "args": args, "cwd": config.folder, "console": "internalConsole", "justMyCode": true, "redirectOutput": true }),
        Kind::Gdb => json!({ "program": config.program, "args": args, "cwd": config.folder, "stopAtBeginningOfMainSubprogram": false }),
        Kind::Lldb => json!({ "program": config.program, "args": args, "cwd": config.folder }),
    }
}

/// One frame of the stack where the program's stopped.
#[derive(Clone, Debug, PartialEq)]
pub struct Frame {
    pub id: i64,
    pub name: String,
    pub path: Option<PathBuf>,
    /// From 0.
    pub line: usize,
}

pub fn frames(body: &Value) -> Vec<Frame> {
    body.get("stackFrames")
        .and_then(Value::as_array)
        .map(|list| {
            list.iter()
                .filter_map(|f| {
                    Some(Frame {
                        id: f.get("id")?.as_i64()?,
                        name: f.get("name").and_then(Value::as_str).unwrap_or("").to_string(),
                        path: f.pointer("/source/path").and_then(Value::as_str).map(PathBuf::from),
                        line: (f.get("line").and_then(Value::as_u64).unwrap_or(1) as usize).saturating_sub(1),
                    })
                })
                .collect()
        })
        .unwrap_or_default()
}

/// A variable, or a scope (Locals, Globals and the like): its name, value and type, and if it has parts, the number
/// to ask for them by.
#[derive(Clone, Debug, PartialEq)]
pub struct Variable {
    pub name: String,
    pub value: String,
    pub kind: String,
    pub parts: i64,
}

/// The scopes of a frame, as variables whose parts are what's in them.
pub fn scopes(body: &Value) -> Vec<Variable> {
    body.get("scopes")
        .and_then(Value::as_array)
        .map(|list| list.iter().filter_map(|s| Some(Variable { name: s.get("name")?.as_str()?.to_string(), value: String::new(), kind: String::new(), parts: s.get("variablesReference").and_then(Value::as_i64).unwrap_or(0) })).collect())
        .unwrap_or_default()
}

pub fn variables(body: &Value) -> Vec<Variable> {
    body.get("variables")
        .and_then(Value::as_array)
        .map(|list| {
            list.iter()
                .filter_map(|v| {
                    Some(Variable {
                        name: v.get("name")?.as_str()?.to_string(),
                        value: v.get("value").and_then(Value::as_str).unwrap_or("").to_string(),
                        kind: v.get("type").and_then(Value::as_str).unwrap_or("").to_string(),
                        parts: v.get("variablesReference").and_then(Value::as_i64).unwrap_or(0),
                    })
                })
                .collect()
        })
        .unwrap_or_default()
}

/// A debug adapter running, and the line to it.
pub struct Session {
    writer: Box<dyn Write + Send>,
    incoming: Receiver<Value>,
    next: i64,
    child: Option<std::process::Child>,
    /// What it said of its troubles, to say why if it ends.
    complaints: std::sync::Arc<std::sync::Mutex<Vec<String>>>,
}

impl Session {
    /// Starts an adapter's command in a folder.
    pub fn start(command: &str, folder: &Path) -> Result<Session, String> {
        let parts = crate::emulators::split_options(command);
        let (program, args) = parts.split_first().ok_or(crate::i18n::tr("No command for the debugger."))?;
        let mut child = std::process::Command::new(program)
            .args(args)
            .current_dir(folder)
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()
            .map_err(|e| crate::i18n::tf!("The debugger {program} couldn't be started: {e}"))?;
        let stdin = child.stdin.take().ok_or(crate::i18n::tr("No way to talk to the debugger."))?;
        let stdout = child.stdout.take().ok_or(crate::i18n::tr("No way to hear the debugger."))?;
        let mut session = Session::over(Box::new(stdin), stdout);
        if let Some(stderr) = child.stderr.take() {
            let kept = session.complaints.clone();
            std::thread::spawn(move || {
                use std::io::BufRead;
                for line in std::io::BufReader::new(stderr).lines().map_while(Result::ok) {
                    if let Ok(mut kept) = kept.lock() {
                        kept.push(line);
                        let extra = kept.len().saturating_sub(20);
                        kept.drain(..extra);
                    }
                }
            });
        }
        session.child = Some(child);
        Ok(session)
    }

    /// Talks to an adapter over any two streams.
    pub fn over(writer: Box<dyn Write + Send>, reader: impl std::io::Read + Send + 'static) -> Session {
        let (send, incoming) = channel();
        std::thread::spawn(move || crate::lsp::read_messages(reader, &send));
        Session { writer, incoming, next: 1, child: None, complaints: Default::default() }
    }

    fn send(&mut self, message: &Value) {
        let body = message.to_string();
        let _ = self.writer.write_all(format!("Content-Length: {}\r\n\r\n{body}", body.len()).as_bytes());
        let _ = self.writer.flush();
    }

    /// Asks something; the number its answer will carry.
    pub fn request(&mut self, command: &str, arguments: Value) -> i64 {
        let seq = self.next;
        self.next += 1;
        self.send(&json!({ "seq": seq, "type": "request", "command": command, "arguments": arguments }));
        seq
    }

    /// Answers a request the adapter made, as not done: Limoniter runs programs itself, with no terminal of its own.
    pub fn decline(&mut self, message: &Value) {
        let seq = self.next;
        self.next += 1;
        let request_seq = message.get("seq").cloned().unwrap_or(Value::Null);
        let command = message.get("command").cloned().unwrap_or(Value::Null);
        self.send(&json!({ "seq": seq, "type": "response", "request_seq": request_seq, "success": false, "command": command, "message": "not supported" }));
    }

    /// Everything said since last asked.
    pub fn poll(&mut self) -> Vec<Value> {
        self.incoming.try_iter().collect()
    }

    /// Whether the adapter has ended, and if so, the last thing it said of why.
    pub fn ended(&mut self) -> Option<String> {
        let child = self.child.as_mut()?;
        child.try_wait().ok().flatten()?;
        std::thread::sleep(std::time::Duration::from_millis(50));
        Some(self.complaints.lock().ok().and_then(|k| k.iter().rev().find(|l| !l.trim().is_empty()).cloned()).unwrap_or_default())
    }

    pub fn stop(&mut self) {
        if let Some(child) = self.child.as_mut() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        self.stop();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn programs_are_guessed_and_launched_as_each_debugger_wants() {
        assert_eq!(package_name("[package]\nname = \"tryit\"\nversion = \"0.1.0\"\n[dependencies]\nname = \"no\"\n").as_deref(), Some("tryit"));
        let script = guess(Kind::Debugpy, "python3 -m debugpy.adapter".into(), Path::new("/w/tool.py"));
        assert_eq!(script.program, "/w/tool.py");
        let launched = launch_arguments(&Config { arguments: "one \"two words\"".into(), ..script });
        assert_eq!(launched["args"], json!(["one", "two words"]));
        assert_eq!(launched["console"], "internalConsole");
        let c = guess(Kind::Gdb, "gdb -i dap".into(), Path::new("/nowhere-at-all/prog.c"));
        assert!(c.program.ends_with("prog") || c.program.ends_with("prog.exe"), "{}", c.program);
        assert_eq!(launch_arguments(&c)["stopAtBeginningOfMainSubprogram"], false);
    }

    #[test]
    fn frames_scopes_and_variables_are_read() {
        let stack = json!({ "stackFrames": [{ "id": 7, "name": "double", "line": 2, "column": 1, "source": { "path": "/w/tool.py" } }, { "id": 8, "name": "<module>", "line": 4 }] });
        assert_eq!(frames(&stack), [Frame { id: 7, name: "double".into(), path: Some("/w/tool.py".into()), line: 1 }, Frame { id: 8, name: "<module>".into(), path: None, line: 3 }]);
        assert_eq!(scopes(&json!({ "scopes": [{ "name": "Locals", "variablesReference": 3 }] }))[0].parts, 3);
        let vars = variables(&json!({ "variables": [{ "name": "n", "value": "4", "type": "int", "variablesReference": 0 }] }));
        assert_eq!(vars[0], Variable { name: "n".into(), value: "4".into(), kind: "int".into(), parts: 0 });
    }
}
