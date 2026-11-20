//! Talking to debuggers through the Debug Adapter Protocol, as VS Code does: the adapter is a program started
//! with its standard input and output as the line, messages framed as the Language Server Protocol frames them.
//! Requests carry a number and the adapter answers each; it says what happens (stopped, output, ended) as events,
//! and may ask the editor things of its own, answered here as not done. Python is debugged with debugpy, and C,
//! C++ and Rust with gdb's own adapter (gdb -i dap, from gdb 14) or with LLVM's lldb-dap. Amiga programs
//! assembled here are debugged in Copperline through copperline-ctl --dap, which starts the emulator, maps the
//! program's source lines from the LINE debug blocks Limoniter writes into it, and can step backwards (its
//! snapshots replayed), which it says with supportsStepBack. How it behaves was learnt from its documentation
//! (Copperline 1.1.0, docs/debugger/dap.md) and checked against the real thing.

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
    /// Copperline's copperline-ctl, which starts the emulator itself, with the program.
    Copperline,
}

impl Kind {
    /// The debugger's name, which is also the adapter's id the protocol is told.
    pub fn adapter_name(self) -> &'static str {
        match self {
            Kind::Debugpy => "debugpy",
            Kind::Gdb => "gdb",
            Kind::Lldb => "lldb-dap",
            Kind::Copperline => "copperline",
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
    /// For an adapter that starts an emulator (Copperline), the emulator and the person's own options for it.
    pub emulator: Option<Emulated>,
}

/// The emulator an adapter starts: its program, and options for its command line.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Emulated {
    pub program: String,
    pub options: Vec<String>,
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
    Config { kind, adapter, program, arguments: String::new(), folder: folder.display().to_string(), emulator: None }
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
        // The machine is in the configuration made beside the program for the run (see `amiga_run`). Without a
        // display to open a window on, it runs without one.
        Kind::Copperline => {
            let mut launch = json!({ "program": config.program, "cwd": config.folder, "stopOnEntry": false, "headless": !has_display() });
            let made = Path::new(&config.program).with_file_name(crate::amiga_run::COPPERLINE_CONFIG);
            if made.is_file() {
                launch["config"] = json!(made.display().to_string());
            }
            if !args.is_empty() {
                launch["args"] = json!(args);
            }
            if let Some(emulator) = &config.emulator {
                launch["copperline"] = json!(emulator.program);
                if !emulator.options.is_empty() {
                    launch["extraArgs"] = json!(emulator.options);
                }
            }
            launch
        }
    }
}

/// Whether there's a display for a window: always on Windows and a Mac, and on Linux when X or Wayland is there.
pub fn has_display() -> bool {
    !cfg!(target_os = "linux") || std::env::var_os("DISPLAY").is_some_and(|d| !d.is_empty()) || std::env::var_os("WAYLAND_DISPLAY").is_some_and(|d| !d.is_empty())
}

/// Copperline's debug adapter, copperline-ctl, for the emulator found at `emulator`: beside it (as Windows, a Mac's
/// app, Homebrew and Limoniter's own download put it), on the path, or in a folder beside the emulator's that's
/// named for Copperline (as the tools for its AppImage unpack, Copperline-1.1.0-x86_64-tools).
pub fn copperline_ctl(emulator: &Path) -> Option<PathBuf> {
    let name = if cfg!(windows) { "copperline-ctl.exe" } else { "copperline-ctl" };
    let beside = emulator.with_file_name(name);
    if beside.is_file() {
        return Some(beside);
    }
    if let Some(found) = std::env::var_os("PATH").and_then(|paths| std::env::split_paths(&paths).map(|dir| dir.join(name)).find(|p| p.is_file())) {
        return Some(found);
    }
    let home = emulator.parent()?;
    let mut folders: Vec<PathBuf> = std::fs::read_dir(home).ok()?.flatten().map(|e| e.path()).filter(|p| p.is_dir() && p.file_name().is_some_and(|n| n.to_string_lossy().to_lowercase().contains("copperline"))).collect();
    folders.sort();
    // The newest release's tools, where there are several.
    folders.into_iter().rev().map(|f| f.join(name)).find(|p| p.is_file())
}

/// The command that starts copperline-ctl as a debug adapter, its path quoted for one with spaces.
pub fn copperline_adapter(ctl: &Path) -> String {
    format!("\"{}\" --dap", ctl.display())
}

/// What the adapter can do, from its answer to initialize.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Capabilities {
    /// Step back and reverse continue, by its snapshots.
    pub step_back: bool,
    pub read_memory: bool,
    /// The exceptions it stops at unless told otherwise.
    pub exception_defaults: Vec<String>,
}

pub fn capabilities(body: &Value) -> Capabilities {
    let yes = |name: &str| body.get(name).and_then(Value::as_bool).unwrap_or(false);
    let exception_defaults = body.get("exceptionBreakpointFilters").and_then(Value::as_array).map(|list| list.iter().filter(|f| f.get("default").and_then(Value::as_bool) == Some(true)).filter_map(|f| Some(f.get("filter")?.as_str()?.to_string())).collect()).unwrap_or_default();
    Capabilities { step_back: yes("supportsStepBack"), read_memory: yes("supportsReadMemoryRequest"), exception_defaults }
}

/// Memory read with readMemory: where it starts, and the bytes (base64 in the answer).
pub fn memory(body: &Value) -> Option<(u64, Vec<u8>)> {
    let address = number(body.get("address")?.as_str()?)?;
    let data = body.get("data").and_then(Value::as_str).unwrap_or("");
    Some((address, crate::conversions::base64_decode(data).ok()?))
}

/// A number as an address is written: $DFF000, 0xDFF000, or in decimal.
pub fn number(text: &str) -> Option<u64> {
    let text = text.trim();
    if let Some(hex) = text.strip_prefix('$').or_else(|| text.strip_prefix("0x")).or_else(|| text.strip_prefix("0X")) {
        return u64::from_str_radix(hex, 16).ok();
    }
    text.parse().ok()
}

/// The address in what an expression was worked out to: the first number in it, in hexadecimal as Copperline
/// writes a register or a pointer ("0x00C05564 (12604772)"), or with a $.
pub fn address_in(result: &str) -> Option<u64> {
    result.split(|c: char| c.is_whitespace() || matches!(c, '(' | ')' | ',' | '=' | ':')).find_map(number)
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

/// Where a program's time went, from a CPU profile in Chrome's form (.cpuprofile), as Copperline's adapter
/// writes one when asked with copperline/profile: each sample is the innermost frame the program was in, with its
/// routine and source line, and the time until the next sample is its weight.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct CpuProfile {
    pub total: u64,
    /// Each routine's share, most first: its name without the offset into it ("WaitFrame+0xc" is WaitFrame's).
    pub routines: Vec<(String, u64)>,
    /// Each source line's share, most first: its file, and the line from 0.
    pub lines: Vec<(PathBuf, usize, u64)>,
}

pub fn cpu_profile(text: &str) -> Result<CpuProfile, String> {
    let json: Value = serde_json::from_str(text).map_err(|e| crate::i18n::tf!("The profile couldn't be read: {e}"))?;
    let nodes: std::collections::HashMap<i64, &Value> = json.get("nodes").and_then(Value::as_array).map(|n| n.iter().filter_map(|n| Some((n.get("id")?.as_i64()?, n))).collect()).unwrap_or_default();
    let samples: Vec<i64> = json.get("samples").and_then(Value::as_array).map(|s| s.iter().filter_map(Value::as_i64).collect()).unwrap_or_default();
    let deltas: Vec<u64> = json.get("timeDeltas").and_then(Value::as_array).map(|s| s.iter().map(|d| d.as_i64().unwrap_or(0).max(0) as u64).collect()).unwrap_or_default();
    let mut routines: std::collections::HashMap<String, u64> = Default::default();
    let mut lines: std::collections::HashMap<(PathBuf, usize), u64> = Default::default();
    let mut total = 0;
    for (n, node) in samples.iter().enumerate() {
        // A sample lasts until the next one; the last, one step as long as the one before.
        let weight = deltas.get(n + 1).or(deltas.get(n)).copied().unwrap_or(1).max(1);
        total += weight;
        let Some(frame) = nodes.get(node).and_then(|n| n.get("callFrame")) else { continue };
        let name = frame.get("functionName").and_then(Value::as_str).unwrap_or("");
        let routine = name.split('+').next().unwrap_or(name).trim();
        *routines.entry(if routine.is_empty() { "?".to_string() } else { routine.to_string() }).or_default() += weight;
        let url = frame.get("url").and_then(Value::as_str).unwrap_or("");
        if let (false, Some(line)) = (url.is_empty(), frame.get("lineNumber").and_then(Value::as_i64).filter(|l| *l >= 0)) {
            let path = url.strip_prefix("file://").unwrap_or(url);
            *lines.entry((PathBuf::from(path), line as usize)).or_default() += weight;
        }
    }
    let mut routines: Vec<(String, u64)> = routines.into_iter().collect();
    routines.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
    let mut lines: Vec<(PathBuf, usize, u64)> = lines.into_iter().map(|((p, l), t)| (p, l, t)).collect();
    lines.sort_by(|a, b| b.2.cmp(&a.2).then(a.1.cmp(&b.1)));
    Ok(CpuProfile { total, routines, lines })
}

/// One of Copperline's warnings about the program: what it says (its own words, with the register and the beam
/// position, or the code written and by what), the address that did it (the instruction, or the Copper's place in
/// its list), and how many times.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Warning {
    pub text: String,
    pub by: u32,
    pub count: u64,
}

/// The warnings in Copperline's chipset.report (the custom chips used as the hardware quietly ignores: a byte
/// written to a word register, a read-only register written, a DMA pointer past chip memory, a blit started over
/// another) or smc.report (code written after it ran: self-modifying code, and how near the prefetch), as the
/// adapter gives a raw control request's reply in an evaluate: JSON, pretty printed.
pub fn warnings(reply: &str) -> Result<Vec<Warning>, String> {
    let json: Value = serde_json::from_str(reply).map_err(|_| reply.trim().to_string())?;
    let (list, by) = match (json.get("findings"), json.get("writes")) {
        (Some(list), _) => (list, "addr"),
        (None, Some(list)) => (list, "writer_pc"),
        _ => return Err(reply.trim().to_string()),
    };
    Ok(list
        .as_array()
        .map(|list| {
            list.iter()
                .map(|w| Warning {
                    text: w.get("detail").and_then(Value::as_str).unwrap_or("").to_string(),
                    by: w.get(by).and_then(Value::as_u64).unwrap_or(0) as u32,
                    count: w.get("count").and_then(Value::as_u64).unwrap_or(1),
                })
                .collect()
        })
        .unwrap_or_default())
}

/// Where Copperline's adapter says the program's hunks were put, from its console line "demo loaded at $C0F4F8,
/// $C0F920".
pub fn loaded_at(said: &str) -> Option<Vec<u32>> {
    let (_, places) = said.trim().split_once(" loaded at ")?;
    places.split(',').map(|p| p.trim().strip_prefix('$').and_then(|h| u32::from_str_radix(h, 16).ok())).collect()
}

/// A program's source lines by where they are in its hunks, and where its hunks were loaded: to say which line
/// an address Copperline names (a warning's) is.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ProgramMap {
    /// For each hunk, its size, and each line with code or data in it: where in the hunk, how long, its file and
    /// its line from 0.
    pub hunks: Vec<(u32, Vec<(u32, u32, PathBuf, usize)>)>,
    pub bases: Vec<u32>,
}

impl ProgramMap {
    pub fn of(assembly: &crate::asm::Assembly) -> ProgramMap {
        let hunks = assembly
            .sections
            .iter()
            .filter(|s| s.size() > 0)
            .map(|section| {
                let lines = assembly.lines.iter().filter(|p| !p.bytes.is_empty() && p.address >= section.start && p.address < section.end).map(|p| ((p.address - section.start) as u32, p.bytes.len() as u32, assembly.files.get(p.file).cloned().unwrap_or_default(), p.line)).collect();
                (section.size() as u32, lines)
            })
            .collect();
        ProgramMap { hunks, bases: Vec::new() }
    }

    /// Whether an address is in the program, once it's loaded.
    pub fn contains(&self, address: u32) -> bool {
        self.bases.iter().zip(&self.hunks).any(|(base, (size, _))| address.wrapping_sub(*base) < *size)
    }

    /// The source line at an address in the program: its file, and the line from 0.
    pub fn place(&self, address: u32) -> Option<(PathBuf, usize)> {
        self.bases.iter().zip(&self.hunks).find_map(|(base, (size, lines))| {
            let at = address.wrapping_sub(*base);
            (at < *size).then(|| lines.iter().find(|(start, length, ..)| at >= *start && at < start + length).map(|(_, _, file, line)| (file.clone(), *line)))?
        })
    }
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

    /// Gives the adapter a while to end by itself, as copperline-ctl does once it has closed the emulator it
    /// started, and then ends it.
    pub fn end_within(&mut self, wait: std::time::Duration) {
        let began = std::time::Instant::now();
        while let Some(child) = self.child.as_mut()
            && began.elapsed() < wait
            && child.try_wait().ok().flatten().is_none()
        {
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        self.stop();
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

    /// Copperline's launch, as its documentation lists the arguments: the program, the configuration made beside
    /// it, the emulator to start, and the person's options for it.
    #[test]
    fn copperline_is_launched_with_the_program_and_its_machine() {
        let folder = std::env::temp_dir().join(format!("limoniter-dap-copperline-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&folder);
        std::fs::create_dir_all(&folder).unwrap();
        let program = folder.join("demo");
        let mut config = Config { kind: Kind::Copperline, adapter: copperline_adapter(Path::new("/opt/Copperline tools/copperline-ctl")), program: program.display().to_string(), arguments: String::new(), folder: folder.display().to_string(), emulator: Some(Emulated { program: "/opt/copperline".into(), options: vec!["--noaudio".into()] }) };
        assert_eq!(crate::emulators::split_options(&config.adapter), ["/opt/Copperline tools/copperline-ctl", "--dap"]);
        let launch = launch_arguments(&config);
        assert_eq!((launch["program"].as_str(), launch["copperline"].as_str(), launch["stopOnEntry"].as_bool()), (Some(program.to_str().unwrap()), Some("/opt/copperline"), Some(false)));
        assert_eq!(launch["extraArgs"], json!(["--noaudio"]));
        assert!(launch.get("config").is_none() && launch.get("args").is_none(), "{launch}");
        std::fs::write(folder.join("copperline.toml"), "").unwrap();
        config.arguments = "-level 2".into();
        let launch = launch_arguments(&config);
        assert_eq!(launch["config"], json!(folder.join("copperline.toml").display().to_string()));
        assert_eq!(launch["args"], json!(["-level", "2"]));
        let _ = std::fs::remove_dir_all(&folder);
    }

    #[test]
    fn copperline_ctl_is_found_beside_copperline_or_its_tools() {
        let folder = std::env::temp_dir().join(format!("limoniter-ctl-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&folder);
        let name = if cfg!(windows) { "copperline-ctl.exe" } else { "copperline-ctl" };
        let tools = folder.join("Copperline-1.1.0-x86_64-tools");
        std::fs::create_dir_all(&tools).unwrap();
        std::fs::write(tools.join(name), "").unwrap();
        let image = folder.join("Copperline-1.1.0-x86_64.AppImage");
        // Unless one's on this computer's path, the AppImage's tools are found in their own folder beside it.
        if std::env::var_os("PATH").and_then(|p| std::env::split_paths(&p).map(|d| d.join(name)).find(|p| p.is_file())).is_none() {
            assert_eq!(copperline_ctl(&image), Some(tools.join(name)));
        }
        std::fs::write(folder.join(name), "").unwrap();
        assert_eq!(copperline_ctl(&image), Some(folder.join(name)), "beside it first");
        let _ = std::fs::remove_dir_all(&folder);
    }

    /// A profile as Copperline writes one (its nodes, the samples naming them, and the time between): the time
    /// is shared out by routine and by source line, lines counted from 0 as Chrome's form counts them.
    #[test]
    fn a_cpu_profile_is_shared_out_by_routine_and_line() {
        let profile = json!({ "nodes": [
            { "id": 1, "callFrame": { "functionName": "(root)", "url": "", "lineNumber": -1 }, "children": [2, 3, 4] },
            { "id": 2, "callFrame": { "functionName": "WaitFrame+0xc", "url": "/w/demo.s", "lineNumber": 40 } },
            { "id": 3, "callFrame": { "functionName": "WaitFrame+0x6", "url": "/w/demo.s", "lineNumber": 39 } },
            { "id": 4, "callFrame": { "functionName": "[exec] Wait+$12", "url": "", "lineNumber": 0 } } ],
            "samples": [2, 3, 2, 4], "timeDeltas": [0, 10, 30, 20], "startTime": 0, "endTime": 80 });
        let read = cpu_profile(&profile.to_string()).unwrap();
        assert_eq!(read.total, 10 + 30 + 20 + 20);
        assert_eq!(read.routines, [("WaitFrame".to_string(), 60), ("[exec] Wait".to_string(), 20)]);
        assert_eq!(read.lines, [(PathBuf::from("/w/demo.s"), 39, 30), (PathBuf::from("/w/demo.s"), 40, 30)]);
        assert!(cpu_profile("not json").is_err());
    }

    /// Copperline's reports, as its adapter gave them for a program writing COLOR00 a byte at a time and patching
    /// the instruction after the one writing (Kickstart 1.3's own findings among them), and where it said the
    /// program was loaded.
    #[test]
    fn copperlines_warnings_are_read_and_put_at_their_lines() {
        let chipset = r#"{ "armed": true, "dropped": 0, "findings": [
            { "addr": 16543084, "by": "cpu", "count": 118, "detail": "COP2LCH aimed at 0xC00000, past the chip RAM Agnus can address; the pointer wraps (by cpu at 0xFC6D6C, v1 h2, 118 times)", "kind": "pointer-outside-chip-ram", "reg": "COP2LCH" },
            { "addr": 12645596, "by": "cpu", "count": 21, "detail": "COLOR00 accessed a byte at a time; the custom chips have no byte lanes, so a byte write latches into both halves of the register (by cpu at 0xC0F4DC, v297 h124, 21 times)", "kind": "byte-access", "reg": "COLOR00" } ] }"#;
        let found = warnings(chipset).unwrap();
        assert_eq!((found.len(), found[1].by, found[1].count), (2, 0xC0F4DC, 21));
        assert!(found[1].text.starts_with("COLOR00 accessed a byte at a time"));
        let smc = r#"{ "armed": true, "dropped": 0, "writes": [ { "addr": 12645616, "count": 20, "detail": "code at $C0F4F0 written by $C0F4EC (4 bytes ahead, inside the prefetch), 20 times", "distance": 4, "writer_pc": 12645612 } ] }"#;
        assert_eq!(warnings(smc).unwrap()[0].by, 0xC0F4EC);
        assert_eq!(warnings("error -32601: unknown method"), Err("error -32601: unknown method".to_string()));
        assert_eq!(loaded_at("demo loaded at $C0F4F8, $C0F920, $010690\n"), Some(vec![0xC0F4F8, 0xC0F920, 0x010690]));
        assert_eq!(loaded_at("warp on"), None);
        // The program those came from, assembled here: line 5 writes COLOR00, line 8 patches line 9.
        let source = "\tsection code,code\nstart:\tmove.l\t4.w,a6\n\tlea\t$dff000,a5\n\tmoveq\t#20,d7\nloop:\tmove.b\t#$0f,$180(a5)\n\tmove.w\t#$1234,$004(a5)\n\tlea\tpatch(pc),a0\n\tmove.w\t#$7001,(a0)\npatch:\tmoveq\t#0,d0\n\tdbf\td7,loop\n\tmoveq\t#0,d0\n\trts\n";
        let (assembly, _) = crate::asm::hunk::assemble_executable(Path::new("/w/bad.s"), source, &|_| Err("no".into()), false, &crate::asm::Options::default());
        let mut map = ProgramMap::of(&assembly);
        assert_eq!(map.place(0xC0F4DC), None, "not loaded yet");
        map.bases = vec![0xC0F4D0];
        assert_eq!(map.place(0xC0F4DC), Some((PathBuf::from("/w/bad.s"), 4)));
        assert_eq!(map.place(0xC0F4EC), Some((PathBuf::from("/w/bad.s"), 7)));
        assert!(map.contains(0xC0F4D0) && !map.contains(0xFC6D6C));
    }

    #[test]
    fn what_copperline_can_do_and_its_memory_are_read() {
        let said = json!({ "supportsStepBack": true, "supportsReadMemoryRequest": true, "exceptionBreakpointFilters": [{ "filter": "bus-error", "default": false }, { "filter": "address-error", "default": true }, { "filter": "illegal", "default": true }] });
        assert_eq!(capabilities(&said), Capabilities { step_back: true, read_memory: true, exception_defaults: vec!["address-error".into(), "illegal".into()] });
        assert_eq!(capabilities(&json!({})), Capabilities::default());
        // As copperline-ctl answered readMemory at address 0 on a Kickstart 1.3 A500.
        let (at, bytes) = memory(&json!({ "address": "0x0", "data": "AAAAAADAAnYA/AgYAPwIGg==" })).unwrap();
        assert_eq!((at, bytes.len(), &bytes[4..8]), (0, 16, &[0x00, 0xC0, 0x02, 0x76][..]));
        assert_eq!((number("$DFF180"), number("0x10"), number("42"), number("label")), (Some(0xDFF180), Some(16), Some(42), None));
        assert_eq!(address_in("0x00C05564 (12604772)"), Some(0xC05564));
        assert_eq!(address_in("not a number"), None);
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
