//! External formatters for Format document, where no language server formats the file: Prettier, Black, Ruff,
//! isort, clang-format, shfmt, gofmt, rustfmt and others, found installed, one chosen for each language. Each is
//! given the text on its input and gives it back formatted.

use std::io::Write;
use std::path::Path;
use std::time::Duration;

/// A formatter: its name, the program, how it's run ({file} is the file's path, for those that go by its ending),
/// and the languages it formats, by the highlighter's names.
#[derive(Debug)]
pub struct Formatter {
    pub name: &'static str,
    pub program: &'static str,
    pub args: &'static [&'static str],
    pub languages: &'static [&'static str],
}

const WEB: &[&str] = &["JavaScript", "JavaScript (Babel)", "TypeScript", "TypeScriptReact", "CSS", "SCSS", "Sass", "LESS", "HTML", "JSON", "YAML", "Markdown", "MultiMarkdown", "GraphQL", "Vue Component"];

pub const ALL: [Formatter; 12] = [
    Formatter { name: "Prettier", program: "prettier", args: &["--stdin-filepath", "{file}"], languages: WEB },
    Formatter { name: "Ruff", program: "ruff", args: &["format", "--stdin-filename", "{file}", "-"], languages: &["Python"] },
    Formatter { name: "Black", program: "black", args: &["--quiet", "--stdin-filename", "{file}", "-"], languages: &["Python"] },
    Formatter { name: "isort (imports only)", program: "isort", args: &["--filename", "{file}", "-"], languages: &["Python"] },
    Formatter { name: "clang-format", program: "clang-format", args: &["--assume-filename={file}"], languages: &["C", "C++", "Objective-C", "Objective-C++", "Java", "C#", "JavaScript", "Protocol Buffer"] },
    Formatter { name: "shfmt", program: "shfmt", args: &["-filename", "{file}"], languages: &["Bourne Again Shell (bash)", "Shell-Unix-Generic"] },
    Formatter { name: "gofmt", program: "gofmt", args: &[], languages: &["Go"] },
    Formatter { name: "rustfmt", program: "rustfmt", args: &["--emit", "stdout", "--edition", "2021"], languages: &["Rust"] },
    Formatter { name: "StyLua", program: "stylua", args: &["--stdin-filepath", "{file}", "-"], languages: &["Lua"] },
    Formatter { name: "Taplo", program: "taplo", args: &["fmt", "-"], languages: &["TOML"] },
    Formatter { name: "xmllint", program: "xmllint", args: &["--format", "-"], languages: &["XML"] },
    Formatter { name: "jq", program: "jq", args: &["."], languages: &["JSON"] },
];

/// The formatters for a language, in the order they're tried.
pub fn for_language(language: &str) -> Vec<&'static Formatter> {
    ALL.iter().filter(|f| f.languages.contains(&language)).collect()
}

/// Whether a formatter's program is installed: on the PATH, as is or as a .cmd or .exe on Windows (npm's
/// Prettier is a .cmd there).
pub fn installed(formatter: &Formatter) -> bool {
    let mut known = FOUND.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    *known.entry(formatter.program).or_insert_with(|| program_path(formatter.program).is_some())
}

/// Whether each program was found, so the menus needn't look along the PATH each time they're drawn.
static FOUND: std::sync::Mutex<std::collections::BTreeMap<&'static str, bool>> = std::sync::Mutex::new(std::collections::BTreeMap::new());

/// Looks for the programs afresh, as the Formatters box opens, in case one's been installed since.
pub fn look_again() {
    FOUND.lock().unwrap_or_else(std::sync::PoisonError::into_inner).clear();
}

fn program_path(program: &str) -> Option<std::path::PathBuf> {
    let path = std::env::var_os("PATH")?;
    let endings: &[&str] = if cfg!(windows) { &[".exe", ".cmd", ".bat", ""] } else { &[""] };
    std::env::split_paths(&path).find_map(|folder| endings.iter().map(|e| folder.join(format!("{program}{e}"))).find(|p| p.is_file()))
}

/// The one to use for a language: the one chosen, if it's installed, else the first installed.
pub fn chosen<'a>(language: &str, choices: &std::collections::HashMap<String, String>) -> Option<&'a Formatter> {
    let options = for_language(language);
    if let Some(name) = choices.get(language) {
        if name == "none" {
            return None;
        }
        if let Some(found) = options.iter().find(|f| f.name == name.as_str() && installed(f)) {
            return Some(found);
        }
    }
    options.into_iter().find(|f| installed(f))
}

/// The text run through the formatter; an error says what it printed when it failed. It's given ten seconds.
pub fn run(formatter: &Formatter, text: &str, file: &Path) -> Result<String, String> {
    let program = program_path(formatter.program).ok_or_else(|| crate::i18n::tf!("{} isn't installed.", formatter.name))?;
    let args: Vec<String> = formatter.args.iter().map(|a| a.replace("{file}", &file.display().to_string())).collect();
    let mut command = crate::platform::command(program);
    command.args(&args).stdin(std::process::Stdio::piped()).stdout(std::process::Stdio::piped()).stderr(std::process::Stdio::piped());
    if let Some(folder) = file.parent().filter(|f| f.is_dir()) {
        command.current_dir(folder);
    }
    let mut child = command.spawn().map_err(|e| e.to_string())?;
    let mut input = child.stdin.take().ok_or_else(|| crate::i18n::tr("The formatter couldn't be given the text.").to_string())?;
    let owned = text.to_string();
    let writer = std::thread::spawn(move || input.write_all(owned.as_bytes()));
    let (send, receive) = std::sync::mpsc::channel();
    let id = child.id();
    std::thread::spawn(move || {
        let _ = send.send(child.wait_with_output());
    });
    let output = match receive.recv_timeout(Duration::from_secs(10)) {
        Ok(output) => output.map_err(|e| e.to_string())?,
        Err(_) => {
            kill(id);
            return Err(crate::i18n::tf!("{} took too long, and was stopped.", formatter.name));
        }
    };
    let _ = writer.join();
    if !output.status.success() {
        let said = String::from_utf8_lossy(&output.stderr).trim().lines().take(6).collect::<Vec<_>>().join("\n");
        return Err(crate::i18n::tf!("{} couldn't format it:\n\n{said}", formatter.name));
    }
    String::from_utf8(output.stdout).map_err(|_| crate::i18n::tf!("{} gave back something that isn't text.", formatter.name))
}

fn kill(id: u32) {
    #[cfg(unix)]
    let _ = crate::platform::command("kill").arg("-9").arg(id.to_string()).status();
    #[cfg(windows)]
    let _ = crate::platform::command("taskkill").args(["/F", "/PID", &id.to_string()]).status();
}

/// The choices kept in the settings file, one line a language.
pub fn to_settings(choices: &std::collections::HashMap<String, String>) -> String {
    let mut lines: Vec<String> = choices.iter().map(|(language, name)| format!("formatter={language}\u{1}{name}\n")).collect();
    lines.sort();
    lines.concat()
}

pub fn read_setting(value: &str, choices: &mut std::collections::HashMap<String, String>) {
    if let Some((language, name)) = value.split_once('\u{1}') {
        choices.insert(language.to_string(), name.trim().to_string());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_language_has_its_formatters_and_the_choice_is_kept() {
        let python: Vec<&str> = for_language("Python").iter().map(|f| f.name).collect();
        assert_eq!(python, ["Ruff", "Black", "isort (imports only)"]);
        assert!(for_language("COBOL").is_empty());
        let mut choices = std::collections::HashMap::new();
        choices.insert("Python".to_string(), "Black".to_string());
        let mut read = std::collections::HashMap::new();
        for line in to_settings(&choices).lines() {
            read_setting(line.strip_prefix("formatter=").unwrap(), &mut read);
        }
        assert_eq!(read, choices);
        choices.insert("Go".into(), "none".into());
        assert!(chosen("Go", &choices).is_none(), "none chosen");
    }

    #[cfg(unix)]
    #[test]
    fn a_formatter_is_given_the_text_and_gives_it_back() {
        // cat stands in for a formatter that changes nothing; sort for one that does.
        let cat = Formatter { name: "cat", program: "cat", args: &[], languages: &[] };
        assert_eq!(run(&cat, "b\na\n", Path::new("x.txt")).unwrap(), "b\na\n");
        let sort = Formatter { name: "sort", program: "sort", args: &[], languages: &[] };
        assert_eq!(run(&sort, "b\na\n", Path::new("x.txt")).unwrap(), "a\nb\n");
        let failing = Formatter { name: "false", program: "false", args: &[], languages: &[] };
        assert!(run(&failing, "x", Path::new("x.txt")).is_err());
    }
}
