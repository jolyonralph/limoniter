//! Claude's limits for `claude` run in a terminal. Claude Code tells a "status line" command what the session is
//! doing (its model, context, cost and the plan's five-hour and weekly limits) each time it redraws. With the switch
//! in Settings on, Limoniter adds such a command to Claude's user settings: it keeps what it was told for the Usage
//! window to read, and prints a short line. A status line the person already has is wrapped, not replaced: it is
//! run with what Claude passed and its output is given back, and switching off puts the settings back as they were.

use crate::agent_session::{Limit, Limits};
use serde_json::{Map, Value, json};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

/// The flag that makes Limoniter act as the status line command, with no window.
pub const FLAG: &str = "--claude-status-line";
/// Where, in Claude's settings folder, the original status line and the exact file are kept while ours is in.
const BACKUP: &str = "limoniter-statusline.json";
/// Where, in Claude's settings folder, what Claude passed is kept, a file for each session.
const RECORDS: &str = "limoniter-status";
/// Records older than this many seconds are removed as new ones are written.
const KEEP_RECORDS: i64 = 3 * 86400;

// ---------- Putting the command in, and taking it out ----------

/// Somewhere a `claude` has its settings, and how that `claude` is to be told where Limoniter and its records are.
#[derive(Clone, Debug, PartialEq)]
pub struct Target {
    /// Claude's settings folder, as Limoniter reaches it.
    pub config: PathBuf,
    /// Limoniter's program, and the settings folder, as the `claude` there writes them in a command.
    pub exe: String,
    pub config_arg: String,
    /// Whether that `claude` runs the command with a Unix shell (it does, apart from Windows' own).
    pub posix: bool,
}

/// A word quoted for the shell the command is run by.
pub fn quote(word: &str, posix: bool) -> String {
    if posix { format!("'{}'", word.replace('\'', "'\\''")) } else { format!("\"{}\"", word.replace('"', "")) }
}

impl Target {
    /// The command Claude is to run.
    pub fn command(&self) -> String {
        format!("{} {FLAG} --config {}", quote(&self.exe, self.posix), quote(&self.config_arg, self.posix))
    }

    /// For the `claude` on this computer, with its own settings folder.
    pub fn here(config: &Path, exe: &Path) -> Target {
        let slashes = |p: &Path| if cfg!(windows) { p.display().to_string().replace('\\', "/") } else { p.display().to_string() };
        Target { config: config.to_path_buf(), exe: slashes(exe), config_arg: slashes(config), posix: !cfg!(windows) }
    }

    /// For a `claude` in a WSL distribution, run by Windows' Limoniter: its program through `/mnt/c/...`, and the
    /// settings folder as Windows reaches it (`//wsl.localhost/...`, which Windows programs take).
    pub fn in_wsl(config: &Path, exe: &Path) -> Target {
        let exe = crate::claude_ide::to_wsl(&exe.display().to_string(), None);
        Target { config: config.to_path_buf(), exe, config_arg: config.display().to_string().replace('\\', "/"), posix: true }
    }
}

/// What is kept of the settings as they were.
struct Backup {
    /// The settings file's text before, or None if there was no file.
    original_text: Option<String>,
    /// The text as it was written, to tell whether anything has changed it since.
    written_text: String,
    /// The `statusLine` setting before, or None if there was none.
    original: Option<Value>,
}

impl Backup {
    fn read(config: &Path) -> Option<Backup> {
        let value: Value = serde_json::from_str(&std::fs::read_to_string(config.join(BACKUP)).ok()?).ok()?;
        Some(Backup {
            original_text: value.get("original_text").and_then(Value::as_str).map(str::to_string),
            written_text: value.get("written_text")?.as_str()?.to_string(),
            original: value.get("original_status_line").filter(|v| !v.is_null()).cloned(),
        })
    }

    fn write(&self, config: &Path) -> Result<(), String> {
        let value = json!({ "original_text": self.original_text, "written_text": self.written_text, "original_status_line": self.original });
        std::fs::write(config.join(BACKUP), serde_json::to_string_pretty(&value).unwrap_or_default()).map_err(|e| e.to_string())
    }

    /// The command that was there before, which is still to be run.
    fn original_command(&self) -> Option<String> {
        self.original.as_ref()?.get("command")?.as_str().map(str::to_string).filter(|c| !c.trim().is_empty())
    }
}

/// Whether a `statusLine` setting is ours.
fn is_ours(status_line: &Value) -> bool {
    status_line.get("command").and_then(Value::as_str).is_some_and(|c| c.contains(FLAG))
}

/// The settings file, written the way Claude Code writes it (two spaces), ending as the one it replaces did.
fn render(settings: &Value, trailing_newline: bool) -> String {
    let mut text = serde_json::to_string_pretty(settings).unwrap_or_default();
    if trailing_newline {
        text.push('\n');
    }
    text
}

/// Adds the status line command to the settings in `target`. Whatever status line is there is kept to be run.
/// Settings that can't be read are left alone, and the reason given.
pub fn install(target: &Target) -> Result<(), String> {
    let path = target.config.join("settings.json");
    let existing = match std::fs::read(&path) {
        Ok(bytes) => Some(String::from_utf8(bytes).map_err(|_| crate::i18n::tf!("{} isn't text Limoniter can read, so it was left as it is.", path.display()))?),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
        Err(e) => return Err(format!("{}: {e}", path.display())),
    };
    let mut settings: Value = match &existing {
        Some(text) if !text.trim().is_empty() => serde_json::from_str(text).map_err(|_| crate::i18n::tf!("{} isn't in the form Claude Code writes, so it was left as it is.", path.display()))?,
        _ => Value::Object(Map::new()),
    };
    let Some(object) = settings.as_object_mut() else {
        return Err(crate::i18n::tf!("{} isn't in the form Claude Code writes, so it was left as it is.", path.display()));
    };
    let command = target.command();
    let current = object.get("statusLine").cloned();
    let mut backup = match (Backup::read(&target.config), &current) {
        // Ours already (an earlier run, perhaps of another copy of Limoniter): keep what that kept.
        (Some(backup), Some(now)) if is_ours(now) => backup,
        _ => Backup { original_text: existing.clone(), written_text: String::new(), original: current.clone() },
    };
    // Ours takes over what is there: its other settings (padding) stay, and what it ran is run from ours.
    let mut line = match current.filter(|now| !is_ours(now)).or_else(|| backup.original.clone()) {
        Some(Value::Object(old)) if old.get("type").and_then(Value::as_str).is_none_or(|t| t == "command") => old,
        _ => Map::new(),
    };
    line.insert("type".into(), json!("command"));
    line.insert("command".into(), json!(command));
    object.insert("statusLine".into(), Value::Object(line));
    let text = render(&settings, existing.as_deref().is_none_or(|t| t.ends_with('\n')));
    backup.written_text.clone_from(&text);
    std::fs::create_dir_all(&target.config).map_err(|e| e.to_string())?;
    // The backup first: with it kept, whatever happens next can be undone.
    backup.write(&target.config)?;
    std::fs::write(&path, text).map_err(|e| format!("{}: {e}", path.display()))
}

/// Puts the settings in `config` back as they were before `install`: byte for byte if nothing else has changed
/// them since, and otherwise only the status line setting, if it is still ours. Nothing to undo is not a failure.
pub fn remove(config: &Path) -> Result<(), String> {
    let Some(backup) = Backup::read(config) else { return Ok(()) };
    let path = config.join("settings.json");
    let current = std::fs::read_to_string(&path).ok();
    if current.as_deref() == Some(backup.written_text.as_str()) {
        match &backup.original_text {
            Some(text) => std::fs::write(&path, text).map_err(|e| format!("{}: {e}", path.display()))?,
            None => std::fs::remove_file(&path).map_err(|e| format!("{}: {e}", path.display()))?,
        }
    } else if let Some(text) = current {
        // Changed since: only the one setting, and only if it's still ours, is put back.
        if let Ok(Value::Object(mut settings)) = serde_json::from_str::<Value>(&text)
            && settings.get("statusLine").is_some_and(is_ours)
        {
            match backup.original {
                Some(original) => settings.insert("statusLine".into(), original),
                None => settings.shift_remove("statusLine"),
            };
            std::fs::write(&path, render(&Value::Object(settings), text.ends_with('\n'))).map_err(|e| format!("{}: {e}", path.display()))?;
        }
    }
    let _ = std::fs::remove_file(config.join(BACKUP));
    let _ = std::fs::remove_dir_all(config.join(RECORDS));
    Ok(())
}

/// Whether the status line command is in the settings in `config`.
#[cfg(test)]
pub fn installed(config: &Path) -> bool {
    std::fs::read_to_string(config.join("settings.json")).ok().and_then(|t| serde_json::from_str::<Value>(&t).ok()).and_then(|s| s.get("statusLine").cloned()).is_some_and(|l| is_ours(&l))
}

// ---------- Being the status line command ----------

/// Whether the command line asks Limoniter to be the status line command.
pub fn wanted(args: &[std::ffi::OsString]) -> bool {
    args.first().is_some_and(|a| a == FLAG)
}

/// The settings folder named after `--config`.
fn config_from(args: &[std::ffi::OsString]) -> Option<PathBuf> {
    args.iter().position(|a| a == "--config").and_then(|at| args.get(at + 1)).map(PathBuf::from)
}

/// Runs as the status line command: reads what Claude passes on its input, keeps it, and prints one line. The
/// exit code is 0 whatever happens, so a failure here never shows in the terminal beyond an empty line.
pub fn run_cli(args: &[std::ffi::OsString]) -> i32 {
    let mut input = String::new();
    let _ = std::io::stdin().read_to_string(&mut input);
    let config = config_from(args).or_else(crate::claude_ide::config_dir);
    let now = chrono::Utc::now().timestamp();
    let line = config.map(|config| respond(&config, &input, now)).unwrap_or_default();
    let mut out = std::io::stdout();
    let _ = out.write_all(line.as_bytes());
    let _ = out.flush();
    0
}

/// What the status line says, having kept what it was passed.
pub fn respond(config: &Path, input: &str, now: i64) -> String {
    if let Ok(value) = serde_json::from_str::<Value>(input) {
        keep(config, &value, now);
    }
    let original = Backup::read(config).and_then(|b| b.original_command());
    if let Some(command) = original
        && let Some(output) = run_original(&command, input)
    {
        return output;
    }
    short_line(input)
}

/// Runs the status line the person had, with what Claude passed it. None if it couldn't be run or said nothing.
fn run_original(command: &str, input: &str) -> Option<String> {
    let mut shell = if cfg!(windows) {
        let mut shell = crate::platform::command("cmd.exe");
        shell.args(["/C", command]);
        shell
    } else {
        let mut shell = crate::platform::command("sh");
        shell.args(["-c", command]);
        shell
    };
    let mut child = shell.stdin(std::process::Stdio::piped()).stdout(std::process::Stdio::piped()).stderr(std::process::Stdio::null()).spawn().ok()?;
    let mut stdin = child.stdin.take()?;
    // From a thread of its own, so that a program that doesn't read its input can't hold this up.
    let text = input.to_string();
    std::thread::spawn(move || {
        let _ = stdin.write_all(text.as_bytes());
    });
    let output = child.wait_with_output().ok()?;
    let text = String::from_utf8_lossy(&output.stdout).to_string();
    (!text.trim().is_empty()).then_some(text)
}

/// A short line of the limits and the context, in figures alone so that it needs no translating.
pub fn short_line(input: &str) -> String {
    let Ok(value) = serde_json::from_str::<Value>(input) else { return String::new() };
    let record = Record::from_input(&value, 0);
    let mut parts = Vec::new();
    if let Some(l) = record.limits.five_hour {
        parts.push(format!("5h {:.0}%", l.percent));
    }
    if let Some(l) = record.limits.week {
        parts.push(format!("7d {:.0}%", l.percent));
    }
    if let Some(p) = record.context_percent {
        parts.push(format!("ctx {p:.0}%"));
    }
    parts.join(" | ")
}

/// Keeps what Claude passed, for the session it names; and clears out old ones.
fn keep(config: &Path, input: &Value, now: i64) {
    let Some(session) = input.get("session_id").and_then(Value::as_str).filter(|s| s.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')) else { return };
    let folder = config.join(RECORDS);
    if std::fs::create_dir_all(&folder).is_err() {
        return;
    }
    let text = json!({ "at": now, "input": input }).to_string();
    // Written whole and moved into place, so that a reader never sees half of it.
    let temp = folder.join(format!(".{session}.tmp"));
    if std::fs::write(&temp, text).is_ok() {
        let _ = std::fs::rename(&temp, folder.join(format!("{session}.json")));
    }
    for entry in std::fs::read_dir(&folder).into_iter().flatten().flatten() {
        let old = entry.metadata().and_then(|m| m.modified()).ok().and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok()).is_some_and(|d| now - d.as_secs() as i64 > KEEP_RECORDS);
        if old {
            let _ = std::fs::remove_file(entry.path());
        }
    }
}

// ---------- Reading what was kept ----------

/// What Claude passed for one session, the last time.
#[derive(Clone, Debug, PartialEq)]
pub struct Record {
    pub session: String,
    /// When it was passed, seconds since 1970.
    pub at: i64,
    pub model: Option<String>,
    pub folder: Option<String>,
    pub cost: Option<f64>,
    pub context_percent: Option<f64>,
    pub context_size: Option<u64>,
    pub context_used: Option<u64>,
    pub tokens_in: Option<u64>,
    pub tokens_out: Option<u64>,
    pub limits: Limits,
}

impl Record {
    pub fn from_input(input: &Value, at: i64) -> Record {
        let window = input.get("context_window");
        let number = |value: Option<&Value>, name: &str| value.and_then(|v| v.get(name)).and_then(Value::as_u64);
        let limit = |name: &str| {
            let window = input.get("rate_limits")?.get(name)?;
            Some(Limit { percent: window.get("used_percentage")?.as_f64()? as f32, resets: window.get("resets_at").and_then(Value::as_i64) })
        };
        let usage = window.and_then(|w| w.get("current_usage")).filter(|u| u.is_object());
        Record {
            session: input.get("session_id").and_then(Value::as_str).unwrap_or_default().to_string(),
            at,
            model: input.get("model").and_then(|m| m.get("display_name").or_else(|| m.get("id"))).and_then(Value::as_str).map(str::to_string),
            folder: input.get("cwd").and_then(Value::as_str).map(str::to_string),
            cost: input.get("cost").and_then(|c| c.get("total_cost_usd")).and_then(Value::as_f64),
            context_percent: window.and_then(|w| w.get("used_percentage")).and_then(Value::as_f64),
            context_size: number(window, "context_window_size"),
            context_used: usage.map(|u| ["input_tokens", "cache_creation_input_tokens", "cache_read_input_tokens"].iter().map(|n| u.get(n).and_then(Value::as_u64).unwrap_or(0)).sum()),
            tokens_in: number(window, "total_input_tokens"),
            tokens_out: number(window, "total_output_tokens"),
            limits: Limits { five_hour: limit("five_hour"), week: limit("seven_day") },
        }
    }
}

/// Every record in a settings folder, the newest first.
pub fn read_records(config: &Path) -> Vec<Record> {
    let mut records: Vec<Record> = std::fs::read_dir(config.join(RECORDS))
        .into_iter()
        .flatten()
        .flatten()
        .filter(|e| e.path().extension().is_some_and(|x| x == "json"))
        .filter_map(|e| {
            let value: Value = serde_json::from_str(&std::fs::read_to_string(e.path()).ok()?).ok()?;
            Some(Record::from_input(value.get("input")?, value.get("at")?.as_i64()?))
        })
        .collect();
    records.sort_by_key(|r| std::cmp::Reverse(r.at));
    records
}

#[cfg(test)]
mod tests {
    use super::*;

    fn folder(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("limoniter-usage-status-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn target(config: &Path) -> Target {
        Target { config: config.to_path_buf(), exe: "/opt/limoniter".into(), config_arg: config.display().to_string(), posix: true }
    }

    const PASSED: &str = r#"{"session_id":"abc-1","cwd":"/work/app","model":{"id":"claude-opus-5-5","display_name":"Opus 5.5"},"cost":{"total_cost_usd":0.5},"context_window":{"total_input_tokens":1000,"total_output_tokens":200,"context_window_size":1000000,"used_percentage":6.2,"current_usage":{"input_tokens":10,"cache_creation_input_tokens":20,"cache_read_input_tokens":30}},"rate_limits":{"five_hour":{"used_percentage":42.5,"resets_at":1800000000},"seven_day":{"used_percentage":19,"resets_at":1800500000}}}"#;

    #[test]
    fn installing_then_removing_leaves_the_settings_byte_for_byte() {
        // Odd spacing, a key order and no final newline: nothing that a re-written file would keep by chance.
        for original in ["{\n  \"theme\": \"dark\",\n  \"permissions\": {\"allow\": [\"Bash(ls)\"]}\n}", "{\"a\":1}\n", "{}"] {
            let dir = folder("roundtrip");
            std::fs::write(dir.join("settings.json"), original).unwrap();
            install(&target(&dir)).unwrap();
            let text = std::fs::read_to_string(dir.join("settings.json")).unwrap();
            assert!(text.contains(FLAG) && installed(&dir));
            assert!(text.contains("\"theme\"") || original.len() < 10, "the rest of the settings stays");
            remove(&dir).unwrap();
            assert_eq!(std::fs::read_to_string(dir.join("settings.json")).unwrap(), original);
            assert!(!dir.join(BACKUP).exists() && !installed(&dir));
        }
    }

    #[test]
    fn no_settings_file_is_made_and_unmade() {
        let dir = folder("nofile");
        install(&target(&dir)).unwrap();
        assert!(installed(&dir));
        remove(&dir).unwrap();
        assert!(!dir.join("settings.json").exists(), "a file that wasn't there goes again");
    }

    #[test]
    fn a_status_line_already_there_is_wrapped_and_comes_back() {
        let dir = folder("wrap");
        let original = "{\n  \"statusLine\": {\n    \"type\": \"command\",\n    \"command\": \"echo mine\",\n    \"padding\": 2\n  }\n}\n";
        std::fs::write(dir.join("settings.json"), original).unwrap();
        install(&target(&dir)).unwrap();
        // Installing again (as every start does) keeps the first original, not ours.
        install(&target(&dir)).unwrap();
        let settings: Value = serde_json::from_str(&std::fs::read_to_string(dir.join("settings.json")).unwrap()).unwrap();
        assert_eq!(settings["statusLine"]["padding"], 2, "its other settings stay");
        assert!(settings["statusLine"]["command"].as_str().unwrap().contains(FLAG));
        #[cfg(unix)]
        {
            // Their line's output is what shows, and what Claude passed is kept.
            assert_eq!(respond(&dir, PASSED, 1_700_000_000), "mine\n");
            assert_eq!(read_records(&dir).len(), 1);
        }
        remove(&dir).unwrap();
        assert_eq!(std::fs::read_to_string(dir.join("settings.json")).unwrap(), original);
    }

    #[test]
    fn settings_changed_since_keep_the_changes_and_only_ours_goes() {
        let dir = folder("changed");
        std::fs::write(dir.join("settings.json"), "{\"statusLine\":{\"type\":\"command\",\"command\":\"echo mine\"}}").unwrap();
        install(&target(&dir)).unwrap();
        // Something else adds a setting while ours is in.
        let mut settings: Value = serde_json::from_str(&std::fs::read_to_string(dir.join("settings.json")).unwrap()).unwrap();
        settings["model"] = json!("opus");
        std::fs::write(dir.join("settings.json"), serde_json::to_string_pretty(&settings).unwrap()).unwrap();
        remove(&dir).unwrap();
        let after: Value = serde_json::from_str(&std::fs::read_to_string(dir.join("settings.json")).unwrap()).unwrap();
        assert_eq!(after["model"], "opus");
        assert_eq!(after["statusLine"]["command"], "echo mine");
        // If the person has since pointed the status line at something else, it isn't touched.
        install(&target(&dir)).unwrap();
        let mut settings: Value = serde_json::from_str(&std::fs::read_to_string(dir.join("settings.json")).unwrap()).unwrap();
        settings["statusLine"] = json!({"type": "command", "command": "echo newer"});
        std::fs::write(dir.join("settings.json"), serde_json::to_string(&settings).unwrap()).unwrap();
        remove(&dir).unwrap();
        assert!(std::fs::read_to_string(dir.join("settings.json")).unwrap().contains("echo newer"));
        // Removing when nothing was installed is not an error.
        remove(&dir).unwrap();
    }

    #[test]
    fn unreadable_settings_are_left_alone() {
        let dir = folder("broken");
        std::fs::write(dir.join("settings.json"), "{ not json").unwrap();
        assert!(install(&target(&dir)).is_err());
        assert_eq!(std::fs::read_to_string(dir.join("settings.json")).unwrap(), "{ not json");
        assert!(!dir.join(BACKUP).exists());
    }

    #[test]
    fn what_claude_passes_is_kept_and_a_short_line_printed() {
        let dir = folder("record");
        let line = respond(&dir, PASSED, 1_700_000_000);
        assert_eq!(line, "5h 42% | 7d 19% | ctx 6%");
        let records = read_records(&dir);
        assert_eq!(records.len(), 1);
        let record = &records[0];
        assert_eq!((record.session.as_str(), record.at, record.model.as_deref(), record.cost), ("abc-1", 1_700_000_000, Some("Opus 5.5"), Some(0.5)));
        assert_eq!(record.limits.five_hour, Some(Limit { percent: 42.5, resets: Some(1_800_000_000) }));
        assert_eq!((record.context_used, record.context_size), (Some(60), Some(1_000_000)));
        // A session name that could climb out of the folder is not used; nonsense input prints nothing.
        respond(&dir, r#"{"session_id":"../../x"}"#, 1);
        assert_eq!(read_records(&dir).len(), 1);
        assert_eq!(respond(&dir, "not json", 1), "");
        assert_eq!(respond(&dir, "{}", 1), "");
    }

    #[test]
    fn commands_are_quoted_for_the_shell_that_runs_them() {
        let unix = Target { config: PathBuf::new(), exe: "/mnt/c/Users/Ann O'Neil/limoniter.exe".into(), config_arg: "//wsl.localhost/Ubuntu/home/ann/.claude".into(), posix: true };
        assert_eq!(unix.command(), "'/mnt/c/Users/Ann O'\\''Neil/limoniter.exe' --claude-status-line --config '//wsl.localhost/Ubuntu/home/ann/.claude'");
        let windows = Target { config: PathBuf::new(), exe: "C:/Program Files/Limoniter/limoniter.exe".into(), config_arg: "C:/Users/ann/.claude".into(), posix: false };
        assert_eq!(windows.command(), "\"C:/Program Files/Limoniter/limoniter.exe\" --claude-status-line --config \"C:/Users/ann/.claude\"");
        let wsl = Target::in_wsl(Path::new("\\\\wsl.localhost\\Ubuntu\\home\\ann\\.claude"), Path::new("C:\\Program Files\\Limoniter\\limoniter.exe"));
        assert_eq!(wsl.exe, "/mnt/c/Program Files/Limoniter/limoniter.exe");
        assert_eq!(wsl.config_arg, "//wsl.localhost/Ubuntu/home/ann/.claude");
    }

    #[test]
    fn the_flag_is_recognised_and_its_folder_read() {
        let args: Vec<std::ffi::OsString> = ["--claude-status-line", "--config", "/x/.claude"].iter().map(Into::into).collect();
        assert!(wanted(&args));
        assert_eq!(config_from(&args), Some(PathBuf::from("/x/.claude")));
        assert!(!wanted(&["file.txt".into()]));
    }
}
