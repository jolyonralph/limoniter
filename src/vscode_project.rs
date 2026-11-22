//! What a folder's `.vscode` says, read as VS Code reads it: `settings.json`, `tasks.json`, `launch.json` and
//! `extensions.json` (JSON with comments and trailing commas), and a `.code-workspace` file's own settings.
//! Nothing here changes anything: it only works out what each file means in Limoniter's words (a
//! `project_prefs::Prefs`, tasks, debug setups and recommended extensions), so the project can follow the files
//! live, and Project settings can copy them in. Limoniter's own project settings win where both say something.
//!
//! Variables are filled in as VS Code fills them: `${workspaceFolder}`, `${env:NAME}`, `${userHome}`, and for a
//! debug setup the file in front (`${file}` and its relatives). A task's are filled in when it's run, by the same
//! `%FILE%` blanks the Run box has.

use crate::dap::{Config, Kind as Debugger};
use crate::project_prefs::Prefs;
use crate::settings::{AutosaveChoice, AutosaveWhen};
use crate::tasks::{Kind, Task};
use serde_json::Value;
use std::path::{Path, PathBuf};

/// Where the files are, inside a folder.
pub fn settings_file(folder: &Path) -> PathBuf {
    folder.join(".vscode").join("settings.json")
}

/// The words each task of VS Code's is marked with in Tools, Tasks.
pub const MARK: &str = "VS Code";

/// A debug setup out of `launch.json`, as written: its words are filled in when it's started, as the file in
/// front is needed for `${file}`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Launch {
    pub name: String,
    pub debugger: Debugger,
    /// What VS Code's `type` said, to show beside the name.
    pub type_name: String,
    pub program: String,
    pub args: Vec<String>,
    pub cwd: String,
    pub env: Vec<(String, String)>,
    pub env_file: String,
    pub stop_on_entry: bool,
}

/// Everything read from a folder's `.vscode`.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Read {
    /// What `settings.json` says, as a project's settings (the project's own are laid over these).
    pub prefs: Prefs,
    pub autosave: Option<AutosaveChoice>,
    pub autosave_seconds: Option<u32>,
    /// `cSpell.words`: the project's word list.
    pub words: Vec<String>,
    pub tasks: Vec<Task>,
    pub launches: Vec<Launch>,
    /// The extensions `extensions.json` recommends, by id (publisher.name), as written but in lower case.
    pub recommendations: Vec<String>,
    /// Each setting that was taken, by the words Project settings knows it by, and what it was.
    pub shown: Vec<(&'static str, String)>,
}

impl Read {
    /// Whether there was nothing in the folder's files for Limoniter to use.
    pub fn is_empty(&self) -> bool {
        *self == Read::default()
    }

    /// Whether `settings.json` or `tasks.json` said anything: what Import into Limoniter has something to copy.
    pub fn has_something_to_import(&self) -> bool {
        !self.shown.is_empty() || !self.tasks.is_empty() || !self.words.is_empty()
    }
}

// ---------- Variables ----------

/// What the file variables are filled in with.
#[derive(Clone, Copy)]
pub enum Files<'a> {
    /// The file in front, if there is one; with none, `${file}` and the rest are left as written.
    Now(Option<&'a Path>),
    /// The Run box's blanks (`%FILE%`), for a task, which is given its file when it's run.
    Blanks,
}

/// A text with VS Code's variables filled in. What isn't known is left as written.
pub fn expand(text: &str, workspace: &Path, files: Files) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(start) = rest.find("${") {
        out.push_str(&rest[..start]);
        let after = &rest[start + 2..];
        let Some(end) = after.find('}') else {
            out.push_str(&rest[start..]);
            return out;
        };
        let name = &after[..end];
        match variable(name, workspace, files) {
            Some(value) => out.push_str(&value),
            None => {
                out.push_str("${");
                out.push_str(name);
                out.push('}');
            }
        }
        rest = &after[end + 1..];
    }
    out.push_str(rest);
    out
}

fn variable(name: &str, workspace: &Path, files: Files) -> Option<String> {
    let shown = |path: &Path| path.display().to_string();
    if let Some(var) = name.strip_prefix("env:") {
        return Some(std::env::var(var).unwrap_or_default());
    }
    let text = |part: Option<&std::ffi::OsStr>| part.map_or_else(String::new, |p| p.to_string_lossy().into_owned());
    match name {
        "workspaceFolder" | "workspaceRoot" | "cwd" => Some(shown(workspace)),
        "workspaceFolderBasename" => Some(text(workspace.file_name())),
        "userHome" => std::env::var_os("USERPROFILE").or_else(|| std::env::var_os("HOME")).map(|home| shown(Path::new(&home))),
        "pathSeparator" | "/" => Some(std::path::MAIN_SEPARATOR.to_string()),
        "file" | "fileDirname" | "fileBasename" | "fileBasenameNoExtension" | "fileExtname" | "relativeFile" | "relativeFileDirname" => match files {
            Files::Blanks => match name {
                "file" => Some("%FILE%".into()),
                "fileDirname" => Some("%DIR%".into()),
                "fileBasename" => Some("%NAME%".into()),
                "fileBasenameNoExtension" => Some("%STEM%".into()),
                _ => None,
            },
            Files::Now(None) => None,
            Files::Now(Some(file)) => match name {
                "file" => Some(shown(file)),
                "fileDirname" => Some(file.parent().map(shown).unwrap_or_default()),
                "fileBasename" => Some(text(file.file_name())),
                "fileBasenameNoExtension" => Some(text(file.file_stem())),
                "fileExtname" => Some(file.extension().map(|e| format!(".{}", e.to_string_lossy())).unwrap_or_default()),
                "relativeFile" => Some(file.strip_prefix(workspace).unwrap_or(file).display().to_string()),
                _ => Some(file.strip_prefix(workspace).unwrap_or(file).parent().map(shown).unwrap_or_default()),
            },
        },
        _ => None,
    }
}

// ---------- Reading the files ----------

fn json_of(path: &Path) -> Option<Value> {
    let text = std::fs::read_to_string(path).ok()?;
    crate::vsix_grammar::parse_jsonc(&text).ok()
}

/// Everything a folder's `.vscode` says. A workspace file's own settings (see `.code-workspace`) are read after
/// the folder's, so they say more. With `settings` false, `settings.json` and `tasks.json` are left unread (as
/// after Import into Limoniter, when the project holds their values itself).
pub fn read(folder: &Path, workspace_file: Option<&Path>, settings: bool) -> Read {
    let mut read = Read::default();
    let dir = folder.join(".vscode");
    if settings {
        if let Some(json) = json_of(&settings_file(folder)) {
            read_settings(&json, folder, &mut read);
        }
        if let Some(json) = workspace_file.and_then(json_of).and_then(|w| w.get("settings").cloned()) {
            read_settings(&json, folder, &mut read);
        }
        if let Some(json) = json_of(&dir.join("tasks.json")) {
            read.tasks = tasks_from(&json, folder);
        }
        if let Some(json) = workspace_file.and_then(json_of).and_then(|w| w.get("tasks").cloned()) {
            read.tasks.extend(tasks_from(&json, folder));
        }
    }
    if let Some(json) = json_of(&dir.join("launch.json")) {
        read.launches = launches_from(&json);
    }
    if let Some(json) = workspace_file.and_then(json_of).and_then(|w| w.get("launch").cloned()) {
        read.launches.extend(launches_from(&json));
    }
    if let Some(json) = json_of(&dir.join("extensions.json")) {
        read.recommendations = recommendations_from(&json);
    }
    read
}

fn flag(json: &Value, key: &str) -> Option<bool> {
    json.get(key).and_then(Value::as_bool)
}

fn text<'a>(json: &'a Value, key: &str) -> Option<&'a str> {
    json.get(key).and_then(Value::as_str)
}

/// The patterns in an object of glob to true, as `files.exclude` writes them. A glob with a condition (`"when"`)
/// is left out, as it can't be a plain pattern.
fn globs(json: &Value, key: &str) -> Vec<String> {
    json.get(key).and_then(Value::as_object).map_or_else(Vec::new, |map| map.iter().filter(|(_, on)| on.as_bool() == Some(true)).map(|(glob, _)| glob.trim().to_string()).filter(|g| !g.is_empty()).collect())
}

/// The highlighter's name for a VS Code language id.
fn language_name(id: &str) -> Option<&'static str> {
    Some(match id {
        "python" => "Python",
        "javascript" => "JavaScript",
        "javascriptreact" => "JavaScript (Babel)",
        "typescript" => "TypeScript",
        "typescriptreact" => "TypeScriptReact",
        "json" | "jsonc" => "JSON",
        "css" => "CSS",
        "scss" => "SCSS",
        "less" => "LESS",
        "html" => "HTML",
        "markdown" => "Markdown",
        "yaml" => "YAML",
        "graphql" => "GraphQL",
        "vue" => "Vue Component",
        "go" => "Go",
        "rust" => "Rust",
        "c" => "C",
        "cpp" => "C++",
        "objective-c" => "Objective-C",
        "java" => "Java",
        "csharp" => "C#",
        "php" => "PHP",
        "lua" => "Lua",
        "shellscript" => "Bourne Again Shell (bash)",
        "toml" => "TOML",
        "xml" => "XML",
        "r" => "R",
        _ => return None,
    })
}

/// The formatter (by `ext_formatters`' name) an extension's id means, or "none" for no formatter.
fn formatter_for(extension: &str) -> Option<&'static str> {
    Some(match extension.to_ascii_lowercase().as_str() {
        "esbenp.prettier-vscode" => "Prettier",
        "charliermarsh.ruff" => "Ruff",
        "ms-python.black-formatter" => "Black",
        "ms-python.isort" => "isort (imports only)",
        "xaver.clang-format" | "ms-vscode.cpptools" => "clang-format",
        "foxundermoon.shell-format" => "shfmt",
        "golang.go" => "gofmt",
        "rust-lang.rust-analyzer" | "rust-lang.rust" => "rustfmt",
        "johnnymorganz.stylua" => "StyLua",
        "tamasfe.even-better-toml" | "be5invis.toml" => "Taplo",
        "junstyle.php-cs-fixer" => "PHP CS Fixer",
        "dotjoshjohnson.xml" | "redhat.vscode-xml" => "xmllint",
        _ => return None,
    })
}

/// The preset new files start with, for the line ending and encoding VS Code was told. Only what a preset
/// really is: with neither said, or a pair no preset has, nothing is chosen.
fn preset_for(eol: Option<&str>, encoding: Option<&str>) -> Option<&'static str> {
    let encoding = encoding.map(|e| e.to_ascii_lowercase().replace(['-', '_'], ""));
    let lf = match eol? {
        "\n" => true,
        "\r\n" => false,
        _ => return None,
    };
    Some(match (encoding.as_deref(), lf) {
        (None | Some("utf8") | Some("utf8bom"), true) => "linux",
        (None | Some("utf8") | Some("utf8bom"), false) => "modern-windows",
        (Some("windows1252"), false) => "legacy-windows",
        (Some("iso88591"), true) => "amiga-plain",
        (Some("cp437" | "ibm437"), false) => "dos-us",
        _ => return None,
    })
}

fn read_settings(json: &Value, folder: &Path, read: &mut Read) {
    let mut shown = |label: &'static str, value: String| {
        read.shown.retain(|(l, _)| *l != label);
        read.shown.push((label, value));
    };
    let mut prefs = std::mem::take(&mut read.prefs);
    let mut exclude = prefs.search_exclude.clone();
    for pattern in globs(json, "files.exclude").into_iter().chain(globs(json, "search.exclude")) {
        if !exclude.contains(&pattern) {
            exclude.push(pattern);
        }
    }
    if exclude != prefs.search_exclude {
        shown("Left out of search", exclude.join(", "));
        prefs.search_exclude = exclude;
    }
    if let Some(size) = json.get("editor.tabSize").and_then(Value::as_u64).map(|n| n as usize).filter(|n| (crate::text_buffer::MIN_TAB_SIZE..=crate::text_buffer::MAX_TAB_SIZE).contains(n)) {
        prefs.tab_width = Some(size);
        shown("Tab width", size.to_string());
    }
    if let Some(spaces) = flag(json, "editor.insertSpaces") {
        prefs.spaces = Some(spaces);
        shown("Tab key", if spaces { "spaces" } else { "tab characters" }.to_string());
    }
    let (eol, encoding) = (text(json, "files.eol"), text(json, "files.encoding"));
    if let Some(preset) = preset_for(eol, encoding) {
        prefs.new_file_preset = Some(preset.to_string());
        shown("New files start as", preset.to_string());
    }
    if let Some(on) = flag(json, "files.trimTrailingWhitespace") {
        prefs.trim_trailing = Some(on);
        shown("Trim trailing spaces on save", on.to_string());
    }
    if let Some(on) = flag(json, "files.insertFinalNewline") {
        prefs.final_newline = Some(on);
        shown("Final new line on save", on.to_string());
    }
    if let Some(on) = flag(json, "editor.formatOnSave") {
        prefs.format_on_save = Some(on);
        shown("Format on save", on.to_string());
    }
    // The formatter for every language it can do, then each `[language]`'s own choice over that.
    if let Some(name) = text(json, "editor.defaultFormatter").and_then(formatter_for) {
        for formatter in crate::ext_formatters::ALL.iter().filter(|f| f.name == name) {
            for language in formatter.languages {
                prefs.formatters.insert(language.to_string(), name.to_string());
            }
        }
        shown("Formatter", name.to_string());
    }
    if let Some(map) = json.as_object() {
        for (key, block) in map {
            let Some(id) = key.strip_prefix('[').and_then(|k| k.strip_suffix(']')) else { continue };
            let Some(language) = id.split(',').next().and_then(|first| language_name(first.trim())) else { continue };
            match block.get("editor.defaultFormatter") {
                Some(Value::Null) => {
                    prefs.formatters.insert(language.to_string(), "none".into());
                    shown("Formatters by language", format!("{language}: none"));
                }
                Some(Value::String(extension)) => {
                    if let Some(name) = formatter_for(extension) {
                        prefs.formatters.insert(language.to_string(), name.to_string());
                        shown("Formatters by language", format!("{language}: {name}"));
                    }
                }
                _ => {}
            }
        }
    }
    if let Some(mode) = text(json, "files.autoSave") {
        let choice = match mode {
            "off" => Some(AutosaveChoice::Off),
            "afterDelay" => Some(AutosaveChoice::When(AutosaveWhen::AfterDelay)),
            "onFocusChange" => Some(AutosaveChoice::When(AutosaveWhen::OnFocusChange)),
            "onWindowChange" => Some(AutosaveChoice::When(AutosaveWhen::OnWindowChange)),
            _ => None,
        };
        if let Some(choice) = choice {
            read.autosave = Some(choice);
            shown("Autosave", mode.to_string());
        }
    }
    if let Some(ms) = json.get("files.autoSaveDelay").and_then(Value::as_u64) {
        read.autosave_seconds = Some(ms.div_ceil(1000).clamp(1, 3600) as u32);
    }
    if let Some(path) = text(json, "python.defaultInterpreterPath").filter(|p| !p.trim().is_empty()) {
        let path = expand(path, folder, Files::Now(None));
        shown("Python", path.clone());
        prefs.python = Some(path);
    }
    if let Some(path) = text(json, "python.envFile").filter(|p| !p.trim().is_empty()) {
        let path = expand(path, folder, Files::Now(None));
        // Written relative to the folder where it's inside it, as the project's own setting is.
        let written = Path::new(&path).strip_prefix(folder).map_or(path.clone(), |inside| inside.to_string_lossy().replace('\\', "/"));
        shown("The .env file", written.clone());
        prefs.env_file = Some(written);
    }
    if let Some(on) = flag(json, "eslint.enable") {
        prefs.eslint = Some(on);
        shown("ESLint", on.to_string());
    }
    if let Some(words) = json.get("cSpell.words").and_then(Value::as_array) {
        for word in words.iter().filter_map(Value::as_str).map(str::trim).filter(|w| !w.is_empty()) {
            if !read.words.iter().any(|w| w == word) {
                read.words.push(word.to_string());
            }
        }
        if !read.words.is_empty() {
            shown("Spelling words", read.words.len().to_string());
        }
    }
    read.prefs = prefs;
}

// ---------- tasks.json ----------

/// A word on a command line, in double quotes where it has spaces (or other things a shell would split).
fn quoted(word: &str) -> String {
    let plain = !word.is_empty() && !word.contains(|c: char| c.is_whitespace() || "\"'&|<>;()".contains(c));
    let already = word.len() > 1 && ((word.starts_with('"') && word.ends_with('"')) || (word.starts_with('\'') && word.ends_with('\'')));
    if plain || already { word.to_string() } else { format!("\"{}\"", word.replace('"', "\\\"")) }
}

/// A command written for this computer's shell: where to run (`cd`), what to set, and the command itself.
fn shell_line(command: &str, args: &[String], cwd: Option<&str>, env: &[(String, String)]) -> String {
    let mut parts: Vec<String> = Vec::new();
    if let Some(cwd) = cwd.filter(|c| !c.is_empty()) {
        parts.push(if cfg!(windows) { format!("cd /d {}", quoted(cwd)) } else { format!("cd {}", quoted(cwd)) });
    }
    let mut line = String::new();
    for (name, value) in env {
        if cfg!(windows) {
            parts.push(format!("set \"{name}={value}\""));
        } else {
            line.push_str(&format!("{name}={} ", quoted(value)));
        }
    }
    line.push_str(command);
    for arg in args {
        line.push(' ');
        line.push_str(&quoted(arg));
    }
    parts.push(line);
    parts.join(" && ")
}

/// A task's command, as the person's operating system names it: a `windows`, `linux` or `osx` block over the rest.
fn per_system(task: &Value, key: &str) -> Option<Value> {
    let system = if cfg!(windows) {
        "windows"
    } else if cfg!(target_os = "macos") {
        "osx"
    } else {
        "linux"
    };
    task.get(system).and_then(|block| block.get(key)).or_else(|| task.get(key)).cloned()
}

fn words(value: Option<&Value>, folder: &Path) -> Vec<String> {
    let Some(Value::Array(items)) = value else { return Vec::new() };
    items
        .iter()
        .filter_map(|item| match item {
            Value::String(s) => Some(s.clone()),
            // `{ "value": "...", "quoting": "strong" }`
            Value::Object(_) => item.get("value").and_then(Value::as_str).map(str::to_string),
            Value::Number(n) => Some(n.to_string()),
            _ => None,
        })
        .map(|word| expand(&word, folder, Files::Blanks))
        .collect()
}

/// The tasks `tasks.json` says: `shell` and `process` tasks (npm's are package.json's scripts, which are listed
/// anyway), with the tasks they depend on run first, in order.
pub fn tasks_from(json: &Value, folder: &Path) -> Vec<Task> {
    let Some(list) = json.get("tasks").and_then(Value::as_array) else { return Vec::new() };
    // Each task's own line, by its label; a task with nothing but `dependsOn` has no line of its own.
    let labelled: Vec<(String, &Value)> = list.iter().filter_map(|t| Some((t.get("label").and_then(Value::as_str).or_else(|| t.get("taskName").and_then(Value::as_str))?.to_string(), t))).collect();
    let mut found: Vec<(bool, Task)> = Vec::new();
    for (label, task) in &labelled {
        if !matches!(task.get("type").and_then(Value::as_str).unwrap_or("shell"), "shell" | "process") {
            continue;
        }
        let mut lines = Vec::new();
        let mut seen = vec![label.clone()];
        chain(task, &labelled, folder, &mut seen, &mut lines);
        if lines.is_empty() {
            continue;
        }
        let (kind, default) = match task.get("group") {
            Some(Value::String(g)) => (group_kind(g), false),
            Some(group @ Value::Object(_)) => (group_kind(text(group, "kind").unwrap_or("")), flag(group, "isDefault").unwrap_or(false)),
            _ => (Kind::Other, false),
        };
        found.push((default, Task { name: format!("{label} ({MARK})"), command: lines.join(" && "), kind }));
    }
    // The default ones first, so Ctrl+Shift+B runs what VS Code would.
    found.sort_by_key(|(default, _)| !default);
    found.into_iter().map(|(_, task)| task).collect()
}

fn group_kind(group: &str) -> Kind {
    match group {
        "build" => Kind::Build,
        "test" => Kind::Test,
        _ => Kind::Other,
    }
}

/// Adds a task's lines to `lines`: those of the tasks it depends on, in the order written, then its own.
fn chain(task: &Value, all: &[(String, &Value)], folder: &Path, seen: &mut Vec<String>, lines: &mut Vec<String>) {
    let depends: Vec<String> = match task.get("dependsOn") {
        Some(Value::String(one)) => vec![one.clone()],
        Some(Value::Array(many)) => many.iter().filter_map(|d| d.as_str().map(str::to_string)).collect(),
        _ => Vec::new(),
    };
    for name in depends {
        // A task that depends on itself, round a loop, is run once.
        if seen.contains(&name) {
            continue;
        }
        if let Some((_, other)) = all.iter().find(|(l, _)| *l == name) {
            seen.push(name.clone());
            chain(other, all, folder, seen, lines);
        }
    }
    let Some(command) = per_system(task, "command").and_then(|c| c.as_str().map(str::to_string)) else { return };
    let mut args = words(per_system(task, "args").as_ref(), folder);
    let command = expand(&command, folder, Files::Blanks);
    // A task's `command` can be written with its arguments in it; it's run as it's written, then.
    let options = per_system(task, "options");
    let cwd = options.as_ref().and_then(|o| text(o, "cwd")).map(|c| expand(c, folder, Files::Blanks));
    let env: Vec<(String, String)> = options.as_ref().and_then(|o| o.get("env")).and_then(Value::as_object).map_or_else(Vec::new, |map| map.iter().filter_map(|(k, v)| Some((k.clone(), expand(&env_text(v)?, folder, Files::Blanks)))).collect());
    // The `shell` type runs a command line; `process` runs a program, whose arguments are all quoted.
    let command = if task.get("type").and_then(Value::as_str) == Some("process") { quoted(&command) } else { command };
    if command.is_empty() {
        args.clear();
    }
    lines.push(shell_line(&command, &args, cwd.as_deref(), &env));
}

fn env_text(value: &Value) -> Option<String> {
    match value {
        Value::String(s) => Some(s.clone()),
        Value::Number(n) => Some(n.to_string()),
        Value::Bool(b) => Some(b.to_string()),
        _ => None,
    }
}

// ---------- launch.json ----------

/// The debug setups `launch.json` says that Limoniter's debugger can start: Python, Node, PHP, C and C++ with
/// gdb or lldb. Attaching to a running program isn't done.
pub fn launches_from(json: &Value) -> Vec<Launch> {
    let Some(list) = json.get("configurations").and_then(Value::as_array) else { return Vec::new() };
    list.iter().filter_map(launch_from).collect()
}

fn launch_from(config: &Value) -> Option<Launch> {
    let type_name = text(config, "type")?.to_string();
    if text(config, "request").unwrap_or("launch") != "launch" {
        return None;
    }
    let debugger = match type_name.as_str() {
        "python" | "debugpy" => Debugger::Debugpy,
        "node" | "pwa-node" => Debugger::JsDebug,
        "php" => Debugger::Xdebug,
        "cppdbg" if text(config, "MIMode") == Some("lldb") => Debugger::Lldb,
        "cppdbg" | "gdb" => Debugger::Gdb,
        "lldb" => Debugger::Lldb,
        _ => return None,
    };
    let name = text(config, "name").unwrap_or(&type_name).to_string();
    let string_of = |key: &str| text(config, key).unwrap_or_default().to_string();
    let args: Vec<String> = match config.get("args") {
        Some(Value::Array(items)) => items.iter().filter_map(|a| a.as_str().map(str::to_string).or_else(|| a.as_u64().map(|n| n.to_string()))).collect(),
        // Written as one string, it's split at its spaces, words in double quotes kept whole.
        Some(Value::String(line)) => crate::emulators::split_options(line),
        _ => Vec::new(),
    };
    let mut program = if debugger == Debugger::Gdb && type_name == "gdb" { string_of("target") } else { string_of("program") };
    // Node's `npm run`, or the same with yarn or pnpm, is a package.json script run under the debugger.
    if debugger == Debugger::JsDebug
        && matches!(text(config, "runtimeExecutable"), Some("npm" | "yarn" | "pnpm"))
        && let Some(script) = config.get("runtimeArgs").and_then(Value::as_array).and_then(|a| match (a.first()?.as_str()?, a.get(1)?.as_str()?) {
            ("run" | "run-script", script) => Some(script.to_string()),
            _ => None,
        })
    {
        program = format!("script:{script}");
    }
    // Something has to be run, except for PHP, which can wait for the browser to connect.
    if program.is_empty() && debugger != Debugger::Xdebug {
        return None;
    }
    let env: Vec<(String, String)> = match (config.get("env"), config.get("environment")) {
        (Some(Value::Object(map)), _) => map.iter().filter_map(|(k, v)| Some((k.clone(), env_text(v)?))).collect(),
        // cppdbg's: a list of { name, value }.
        (_, Some(Value::Array(items))) => items.iter().filter_map(|i| Some((text(i, "name")?.to_string(), text(i, "value")?.to_string()))).collect(),
        _ => Vec::new(),
    };
    let stop_on_entry = flag(config, "stopOnEntry").or_else(|| flag(config, "stopAtEntry")).or_else(|| flag(config, "stopAtBeginningOfMainSubprogram")).unwrap_or(false);
    Some(Launch { name, debugger, type_name, program, args, cwd: string_of("cwd"), env, env_file: string_of("envFile"), stop_on_entry })
}

impl Launch {
    /// The debugger's configuration, with VS Code's variables filled in for this folder and the file in front.
    pub fn to_config(&self, workspace: &Path, file: Option<&Path>) -> Config {
        let fill = |text: &str| expand(text, workspace, Files::Now(file));
        let quoted_args: Vec<String> = self.args.iter().map(|a| quoted(&fill(a))).collect();
        let folder = if self.cwd.is_empty() { workspace.display().to_string() } else { fill(&self.cwd) };
        let mut env: Vec<(String, String)> = if self.env_file.is_empty() { Vec::new() } else { crate::env_file::environment_from(&crate::project::resolved(workspace, &fill(&self.env_file))) };
        for (name, value) in &self.env {
            env.retain(|(n, _)| n != name);
            env.push((name.clone(), fill(value)));
        }
        // A script run by a package manager keeps its `script:` marker; any other program is a path.
        let program = fill(&self.program);
        let program = if self.debugger == Debugger::Xdebug || program.starts_with("script:") || Path::new(&program).is_absolute() || program.is_empty() { program } else { crate::project::resolved(workspace, &program).display().to_string() };
        Config { kind: self.debugger, adapter: crate::dap::adapter_command(self.debugger), program, arguments: quoted_args.join(" "), folder, emulator: None, env, stop_on_entry: self.stop_on_entry }
    }

    /// Whether `${file}` is in it, so a file has to be in front to start it.
    pub fn needs_file(&self) -> bool {
        std::iter::once(&self.program).chain(&self.args).chain(std::iter::once(&self.cwd)).any(|t| t.contains("${file") || t.contains("${relativeFile"))
    }
}

// ---------- extensions.json ----------

pub fn recommendations_from(json: &Value) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for id in json.get("recommendations").and_then(Value::as_array).into_iter().flatten().filter_map(Value::as_str) {
        let id = id.trim().to_ascii_lowercase();
        if id.contains('.') && !out.contains(&id) {
            out.push(id);
        }
    }
    out
}

/// The language server Limoniter knows for an extension, by the language and the server's command.
pub fn server_for(extension: &str) -> Option<(&'static str, &'static str)> {
    Some(match extension {
        "rust-lang.rust-analyzer" => ("Rust", "rust-analyzer"),
        "ms-python.python" | "ms-python.vscode-pylance" | "ms-python.pylance" => ("Python", "pylsp"),
        "dbaeumer.vscode-eslint" => ("JavaScript", "vscode-eslint-language-server"),
        "golang.go" => ("Go", "gopls"),
        "llvm-vs-code-extensions.vscode-clangd" | "ms-vscode.cpptools" => ("C++", "clangd"),
        "bmewburn.vscode-intelephense-client" => ("PHP", "intelephense"),
        "sumneko.lua" => ("Lua", "lua-language-server"),
        "mads-hartmann.bash-ide-vscode" => ("Bourne Again Shell (bash)", "bash-language-server"),
        "redhat.java" => ("Java", "jdtls"),
        "ms-vscode.vscode-typescript-next" => ("TypeScript", "typescript-language-server"),
        _ => return None,
    })
}

/// Extensions that only Visual Studio Code itself can use (they run its own debugger, its notebooks or its
/// online services), which an offer to install them from Open VSX would only disappoint.
pub fn only_for_vs_code(extension: &str) -> bool {
    ["ms-vscode-remote.", "github.copilot", "ms-toolsai.", "ms-vsliveshare.", "ms-azuretools.", "github.vscode-pull-request-github"].iter().any(|p| extension.starts_with(p))
}

// ---------- The project's words ----------

/// Where a folder project keeps its own spelling words, one a line, so they go into Git with it.
pub fn words_file(folder: &Path) -> PathBuf {
    folder.join(".limoniter").join("words.txt")
}

/// Adds words (VS Code's `cSpell.words`) to the project's word list, leaving out those already there.
pub fn keep_words(folder: &Path, words: &[String]) -> std::io::Result<()> {
    let file = words_file(folder);
    let mut kept: Vec<String> = std::fs::read_to_string(&file).map(|t| t.lines().map(str::trim).filter(|l| !l.is_empty()).map(str::to_string).collect()).unwrap_or_default();
    let before = kept.len();
    for word in words {
        if !kept.iter().any(|k| k.eq_ignore_ascii_case(word)) {
            kept.push(word.clone());
        }
    }
    if kept.len() == before {
        return Ok(());
    }
    if let Some(dir) = file.parent() {
        std::fs::create_dir_all(dir)?;
    }
    std::fs::write(&file, kept.join("\n") + "\n")
}

// ---------- .code-workspace ----------

/// The folders a `.code-workspace` file names, each written relative to the file, whole: those that exist.
pub fn workspace_folders(text: &str, file: &Path) -> Vec<(String, PathBuf)> {
    let Some(json) = crate::vsix_grammar::parse_jsonc(text).ok() else { return Vec::new() };
    let base = file.parent().map(Path::to_path_buf).unwrap_or_default();
    json.get("folders")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|f| {
            let path = crate::project::resolved(&base, f.get("path").and_then(Value::as_str)?);
            let name = f.get("name").and_then(Value::as_str).map(str::to_string).or_else(|| path.file_name().map(|n| n.to_string_lossy().into_owned()))?;
            path.is_dir().then_some((name, path))
        })
        .collect()
}

/// Which formatters can be told apart by a language, for a test: the names `formatter_for` gives all exist.
#[cfg(test)]
fn every_formatter_name() -> Vec<&'static str> {
    ["esbenp.prettier-vscode", "charliermarsh.ruff", "ms-python.black-formatter", "ms-python.isort", "xaver.clang-format", "foxundermoon.shell-format", "golang.go", "rust-lang.rust-analyzer", "johnnymorganz.stylua", "tamasfe.even-better-toml", "junstyle.php-cs-fixer", "dotjoshjohnson.xml"].iter().filter_map(|id| formatter_for(id)).collect()
}

/// The settings of the keys that Import copies and the words it's said in, as `BTreeMap`, for tests to compare.
#[cfg(test)]
fn labels(read: &Read) -> std::collections::BTreeMap<&'static str, String> {
    read.shown.iter().cloned().collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A settings.json as VS Code writes it: comments, trailing commas, and a block for a language.
    const SETTINGS: &str = r#"{
    // Editor
    "editor.tabSize": 2,
    "editor.insertSpaces": true,
    "editor.formatOnSave": true,
    "editor.defaultFormatter": "esbenp.prettier-vscode",
    "[python]": {
        "editor.defaultFormatter": "charliermarsh.ruff",
        "editor.tabSize": 4,
    },
    "[markdown]": { "editor.defaultFormatter": null },
    /* Files */
    "files.exclude": { "**/.git": true, "**/*.pyc": true, "**/keep": false, "**/x": { "when": "$(basename).ts" } },
    "search.exclude": { "**/node_modules": true, "**/.git": true },
    "files.eol": "\n",
    "files.encoding": "utf8",
    "files.trimTrailingWhitespace": true,
    "files.insertFinalNewline": true,
    "files.autoSave": "onFocusChange",
    "files.autoSaveDelay": 1500,
    "python.defaultInterpreterPath": "${workspaceFolder}/.venv/bin/python",
    "python.envFile": "${workspaceFolder}/.env.dev",
    "eslint.enable": false,
    "cSpell.words": ["limoniter", "amiga", "limoniter"],
}
"#;

    fn folder(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("limoniter-vscode-{}-{tag}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join(".vscode")).unwrap();
        dir
    }

    #[test]
    fn comments_and_trailing_commas_are_read_and_every_key_lands_in_its_field() {
        let dir = folder("settings");
        std::fs::write(settings_file(&dir), SETTINGS).unwrap();
        let read = read(&dir, None, true);
        let p = &read.prefs;
        assert_eq!(p.tab_width, Some(2));
        assert_eq!(p.spaces, Some(true));
        assert_eq!(p.format_on_save, Some(true));
        assert_eq!((p.trim_trailing, p.final_newline), (Some(true), Some(true)));
        assert_eq!(p.new_file_preset.as_deref(), Some("linux"));
        // Both lists, with each glob once and only those switched on.
        assert_eq!(p.search_exclude, vec!["**/.git".to_string(), "**/*.pyc".to_string(), "**/node_modules".to_string()]);
        assert_eq!(p.formatters.get("Python").map(String::as_str), Some("Ruff"));
        assert_eq!(p.formatters.get("CSS").map(String::as_str), Some("Prettier"));
        assert_eq!(p.formatters.get("Markdown").map(String::as_str), Some("none"));
        assert_eq!(read.autosave, Some(AutosaveChoice::When(AutosaveWhen::OnFocusChange)));
        assert_eq!(read.autosave_seconds, Some(2));
        assert_eq!(p.python.as_deref(), Some(format!("{}/.venv/bin/python", dir.display()).as_str()));
        assert_eq!(p.env_file.as_deref(), Some(".env.dev"));
        assert_eq!(p.eslint, Some(false));
        assert_eq!(read.words, vec!["limoniter".to_string(), "amiga".to_string()]);
        assert!(labels(&read).contains_key("Tab width"));
    }

    #[test]
    fn formatter_extensions_map_onto_the_names_the_formatters_have() {
        for name in every_formatter_name() {
            assert!(crate::ext_formatters::ALL.iter().any(|f| f.name == name), "{name}");
        }
        assert_eq!(formatter_for("ms-python.black-formatter"), Some("Black"));
        assert_eq!(formatter_for("Esbenp.Prettier-VSCode"), Some("Prettier"));
        assert_eq!(formatter_for("someone.unknown"), None);
    }

    #[test]
    fn line_ending_and_encoding_choose_a_preset_only_when_one_fits() {
        assert_eq!(preset_for(Some("\r\n"), Some("utf8")), Some("modern-windows"));
        assert_eq!(preset_for(Some("\r\n"), Some("windows1252")), Some("legacy-windows"));
        assert_eq!(preset_for(Some("\n"), None), Some("linux"));
        assert_eq!(preset_for(Some("auto"), Some("utf8")), None);
        assert_eq!(preset_for(None, Some("utf8")), None);
        assert_eq!(preset_for(Some("\n"), Some("shiftjis")), None);
        for id in ["linux", "modern-windows", "legacy-windows", "amiga-plain", "dos-us"] {
            assert!(crate::codec::preset_by_id(id).is_some());
        }
    }

    #[test]
    fn odd_values_are_left_alone() {
        let json = crate::vsix_grammar::parse_jsonc(r#"{ "editor.tabSize": "auto", "editor.insertSpaces": "auto", "files.autoSave": "sometimes", "editor.defaultFormatter": "x.y" }"#).unwrap();
        let mut read = Read::default();
        read_settings(&json, Path::new("/w"), &mut read);
        assert!(read.is_empty(), "{read:?}");
    }

    #[test]
    fn variables_are_filled_in_as_vs_code_fills_them() {
        let w = Path::new("/work/app");
        let file = Path::new("/work/app/src/main.py");
        let now = Files::Now(Some(file));
        assert_eq!(expand("${workspaceFolder}/out", w, now), "/work/app/out");
        assert_eq!(expand("${file}", w, now), "/work/app/src/main.py");
        assert_eq!(expand("${fileDirname}|${fileBasename}|${fileBasenameNoExtension}|${fileExtname}", w, now), "/work/app/src|main.py|main|.py");
        assert_eq!(expand("${relativeFile}", w, now), "src/main.py");
        assert_eq!(expand("${workspaceFolderBasename}", w, now), "app");
        // ${env:NAME}: the variable's value, or nothing where it isn't set.
        // SAFETY: only this test uses these two names.
        unsafe {
            std::env::set_var("LIMONITER_VSCODE_TEST", "yes");
            std::env::remove_var("LIMONITER_VSCODE_NOT_SET");
        }
        assert_eq!(expand("${env:LIMONITER_VSCODE_TEST}!", w, now), "yes!");
        assert_eq!(expand("[${env:LIMONITER_VSCODE_NOT_SET}]", w, now), "[]");
        // With no file in front, ${file} stays as it was written, and so does what isn't a variable.
        assert_eq!(expand("${file} ${nonsense} ${open", w, Files::Now(None)), "${file} ${nonsense} ${open");
        // A task is given the Run box's blanks.
        assert_eq!(expand("${file} ${fileDirname}", w, Files::Blanks), "%FILE% %DIR%");
        if let Some(home) = std::env::var_os("USERPROFILE").or_else(|| std::env::var_os("HOME")) {
            assert_eq!(expand("${userHome}", w, now), Path::new(&home).display().to_string());
        }
    }

    #[test]
    fn shell_and_process_tasks_become_commands_with_their_folder_and_environment() {
        let json = crate::vsix_grammar::parse_jsonc(
            r#"{
  "version": "2.0.0",
  "tasks": [
    {
      "label": "build",
      "type": "shell",
      "command": "make",
      "args": ["-j4", "all", "two words"],
      "group": { "kind": "build", "isDefault": true },
      "options": { "cwd": "${workspaceFolder}/src", "env": { "MODE": "fast" } },
      "presentation": { "reveal": "always" },
    },
    { "label": "lint", "type": "process", "command": "eslint", "args": ["${file}"], "group": "test" },
    { "label": "dev", "type": "npm", "script": "dev" },
    { "label": "check", "dependsOn": ["lint", "build"], "dependsOrder": "sequence" },
  ]
}"#,
        )
        .unwrap();
        let tasks = tasks_from(&json, Path::new("/w"));
        let by = |name: &str| tasks.iter().find(|t| t.name == format!("{name} ({MARK})")).unwrap_or_else(|| panic!("{name} in {tasks:?}"));
        let build = by("build");
        assert_eq!(build.kind, Kind::Build);
        assert!(build.command.contains("make -j4 all \"two words\""), "{}", build.command);
        assert!(build.command.contains("/w/src"), "{}", build.command);
        assert!(build.command.contains("MODE="), "{}", build.command);
        assert_eq!(by("lint").kind, Kind::Test);
        assert_eq!(by("lint").command, "eslint %FILE%");
        // dependsOn: the tasks it names first, in the order written, then its own (none).
        assert_eq!(by("check").command, format!("{} && {}", by("lint").command, by("build").command));
        // npm tasks are the package.json scripts, listed anyway.
        assert!(!tasks.iter().any(|t| t.name.starts_with("dev ")));
        // The default build task is first.
        assert_eq!(tasks[0].name, format!("build ({MARK})"));
    }

    #[test]
    fn tasks_that_depend_on_each_other_in_a_loop_are_still_made() {
        let json = serde_json::json!({ "tasks": [
            { "label": "a", "command": "echo a", "dependsOn": "b" },
            { "label": "b", "command": "echo b", "dependsOn": "a" },
        ] });
        let tasks = tasks_from(&json, Path::new("/w"));
        assert_eq!(tasks.len(), 2);
        assert_eq!(tasks[0].command, "echo b && echo a");
    }

    #[test]
    fn launch_setups_are_read_for_each_debugger_with_their_fields() {
        let json = crate::vsix_grammar::parse_jsonc(
            r#"{
  "version": "0.2.0",
  "configurations": [
    { "name": "Python: current file", "type": "python", "request": "launch", "program": "${file}", "args": ["--fast", "a b"], "cwd": "${workspaceFolder}/src", "env": { "DEBUG": "1" }, "envFile": "${workspaceFolder}/.env", "stopOnEntry": true },
    { "name": "Node", "type": "pwa-node", "request": "launch", "program": "${workspaceFolder}/app.js" },
    { "name": "Dev script", "type": "node", "request": "launch", "runtimeExecutable": "npm", "runtimeArgs": ["run", "dev"] },
    { "name": "Listen for Xdebug", "type": "php", "request": "launch", "port": 9003 },
    { "name": "C++", "type": "cppdbg", "request": "launch", "program": "${workspaceFolder}/build/app", "environment": [{ "name": "A", "value": "b" }], "stopAtEntry": true },
    { "name": "Mac", "type": "cppdbg", "MIMode": "lldb", "request": "launch", "program": "build/app" },
    { "name": "CodeLLDB", "type": "lldb", "request": "launch", "program": "${workspaceFolder}/target/debug/x", "args": "-v \"a b\"" },
    { "name": "Attach", "type": "python", "request": "attach" },
    { "name": "Java", "type": "java", "request": "launch", "mainClass": "A" },
    { "name": "No program", "type": "python", "request": "launch" },
  ],
}"#,
        )
        .unwrap();
        let launches = launches_from(&json);
        let names: Vec<&str> = launches.iter().map(|l| l.name.as_str()).collect();
        assert_eq!(names, ["Python: current file", "Node", "Dev script", "Listen for Xdebug", "C++", "Mac", "CodeLLDB"]);
        let w = Path::new("/w");
        let file = Path::new("/w/src/tool.py");
        let python = launches[0].to_config(w, Some(file));
        // Paths compared with / throughout, as Windows writes some with \.
        let slashed = |s: &str| s.replace('\\', "/");
        assert_eq!((python.kind, slashed(&python.program), slashed(&python.folder)), (Debugger::Debugpy, "/w/src/tool.py".to_string(), "/w/src".to_string()));
        assert_eq!(python.arguments, "--fast \"a b\"");
        assert!(python.env.contains(&("DEBUG".to_string(), "1".to_string())));
        assert!(python.stop_on_entry);
        assert!(launches[0].needs_file() && !launches[1].needs_file());
        let node = launches[1].to_config(w, None);
        assert_eq!((node.kind, slashed(&node.program), slashed(&node.folder)), (Debugger::JsDebug, "/w/app.js".to_string(), "/w".to_string()));
        assert_eq!(launches[2].to_config(w, None).program, "script:dev");
        assert_eq!(launches[3].debugger, Debugger::Xdebug);
        assert!(launches[3].to_config(w, None).program.is_empty());
        let cpp = launches[4].to_config(w, None);
        assert_eq!((cpp.kind, slashed(&cpp.program), cpp.env.clone(), cpp.stop_on_entry), (Debugger::Gdb, "/w/build/app".to_string(), vec![("A".to_string(), "b".to_string())], true));
        // A relative program is in the project's folder.
        assert_eq!((launches[5].debugger, slashed(&launches[5].to_config(w, None).program)), (Debugger::Lldb, "/w/build/app".to_string()));
        assert_eq!(launches[6].to_config(w, None).arguments, "-v \"a b\"");
    }

    #[test]
    fn a_launch_setup_s_env_file_is_read_and_its_own_env_wins() {
        let dir = folder("envfile");
        std::fs::write(dir.join(".env"), "A=from-file\nB=2\n").unwrap();
        let json = serde_json::json!({ "configurations": [{ "name": "x", "type": "python", "program": "a.py", "envFile": "${workspaceFolder}/.env", "env": { "A": "mine" } }] });
        let config = launches_from(&json)[0].to_config(&dir, None);
        assert!(config.env.contains(&("A".to_string(), "mine".to_string())));
        assert!(config.env.contains(&("B".to_string(), "2".to_string())));
        assert_eq!(config.env.iter().filter(|(n, _)| n == "A").count(), 1);
    }

    #[test]
    fn recommendations_are_ids_once_each_in_lower_case() {
        let json = crate::vsix_grammar::parse_jsonc(r#"{ "recommendations": ["dbaeumer.vscode-eslint", "esbenp.prettier-vscode", "DBAEUMER.vscode-eslint", "nonsense", 3], "unwantedRecommendations": ["x.y"] }"#).unwrap();
        assert_eq!(recommendations_from(&json), vec!["dbaeumer.vscode-eslint".to_string(), "esbenp.prettier-vscode".to_string()]);
        assert_eq!(server_for("dbaeumer.vscode-eslint"), Some(("JavaScript", "vscode-eslint-language-server")));
        assert!(only_for_vs_code("ms-vscode-remote.remote-ssh"));
        assert!(!only_for_vs_code("esbenp.prettier-vscode"));
    }

    #[test]
    fn a_workspace_file_names_its_folders_and_adds_its_settings() {
        let dir = folder("workspace");
        std::fs::create_dir_all(dir.join("api")).unwrap();
        std::fs::create_dir_all(dir.join("web")).unwrap();
        let file = dir.join("my.code-workspace");
        let text = r#"{ "folders": [ { "path": "api" }, { "path": "web", "name": "Website" }, { "path": "gone" } ], "settings": { "editor.tabSize": 8 }, }"#;
        std::fs::write(&file, text).unwrap();
        let folders = workspace_folders(text, &file);
        assert_eq!(folders, vec![("api".to_string(), dir.join("api")), ("Website".to_string(), dir.join("web"))]);
        // The workspace's settings are read after the folder's own, so they say more.
        std::fs::write(settings_file(&dir.join("api")), "").ok();
        std::fs::create_dir_all(dir.join("api").join(".vscode")).unwrap();
        std::fs::write(settings_file(&dir.join("api")), r#"{ "editor.tabSize": 2, "editor.insertSpaces": false }"#).unwrap();
        let read = read(&dir.join("api"), Some(&file), true);
        assert_eq!((read.prefs.tab_width, read.prefs.spaces), (Some(8), Some(false)));
    }

    #[test]
    fn reading_settings_can_be_turned_off_but_launch_and_extensions_are_still_read() {
        let dir = folder("off");
        std::fs::write(settings_file(&dir), SETTINGS).unwrap();
        std::fs::write(dir.join(".vscode").join("tasks.json"), r#"{ "tasks": [{ "label": "a", "command": "echo" }] }"#).unwrap();
        std::fs::write(dir.join(".vscode").join("extensions.json"), r#"{ "recommendations": ["a.b"] }"#).unwrap();
        let on = read(&dir, None, true);
        assert!(on.has_something_to_import() && on.tasks.len() == 1);
        let off = read(&dir, None, false);
        assert!(!off.has_something_to_import());
        assert_eq!(off.recommendations, vec!["a.b".to_string()]);
    }

    #[test]
    fn words_are_added_to_the_project_s_list_once_each() {
        let dir = folder("words");
        keep_words(&dir, &["amiga".to_string(), "Limoniter".to_string()]).unwrap();
        keep_words(&dir, &["limoniter".to_string(), "copper".to_string()]).unwrap();
        assert_eq!(std::fs::read_to_string(words_file(&dir)).unwrap(), "amiga\nLimoniter\ncopper\n");
    }

    #[test]
    fn a_folder_with_no_vscode_is_empty() {
        let dir = std::env::temp_dir().join(format!("limoniter-vscode-{}-none", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        assert!(read(&dir, None, true).is_empty());
    }
}
