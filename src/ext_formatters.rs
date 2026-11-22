//! External formatters for Format document, where no language server formats the file: Prettier, Black, Ruff,
//! isort, clang-format, shfmt, gofmt, rustfmt and others, found installed, one chosen for each language. Each is
//! given the text on its input and gives it back formatted (php-cs-fixer, which only works on files, is given a
//! copy of the text in a file instead).
//!
//! A project's own copy of a formatter, in `node_modules/.bin`, `vendor/bin` or a virtual environment's `bin`,
//! comes before the one on the PATH. Each formatter follows its own settings files, which it finds from where it's
//! run (the file's folder) and the file's name: `.prettierrc`, `pyproject.toml`'s `[tool.ruff]` and `[tool.black]`,
//! `.clang-format`, `rustfmt.toml` and the like. Those that don't read them for themselves are told: php-cs-fixer
//! is given the project's `.php-cs-fixer.php`, and styler the indent `.editorconfig` asks for.

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
    /// The exit codes that mean it worked (phpcbf says 1 when it changed something).
    pub ok: &'static [i32],
    /// Whether it only works on a file, so is given a copy of the text in one: `{temp}` is its path.
    pub temp: bool,
}

const WEB: &[&str] = &["JavaScript", "JavaScript (Babel)", "TypeScript", "TypeScriptReact", "CSS", "SCSS", "Sass", "LESS", "HTML", "JSON", "YAML", "Markdown", "MultiMarkdown", "GraphQL", "Vue Component"];

pub const ALL: [Formatter; 15] = [
    Formatter { name: "Prettier", program: "prettier", args: &["--stdin-filepath", "{file}"], languages: WEB, ok: &[0], temp: false },
    Formatter { name: "Ruff", program: "ruff", args: &["format", "--stdin-filename", "{file}", "-"], languages: &["Python"], ok: &[0], temp: false },
    Formatter { name: "Black", program: "black", args: &["--quiet", "--stdin-filename", "{file}", "-"], languages: &["Python"], ok: &[0], temp: false },
    Formatter { name: "isort (imports only)", program: "isort", args: &["--filename", "{file}", "-"], languages: &["Python"], ok: &[0], temp: false },
    Formatter { name: "clang-format", program: "clang-format", args: &["--assume-filename={file}"], languages: &["C", "C++", "Objective-C", "Objective-C++", "Java", "C#", "JavaScript", "Protocol Buffer"], ok: &[0], temp: false },
    Formatter { name: "shfmt", program: "shfmt", args: &["-filename", "{file}"], languages: &["Bourne Again Shell (bash)", "Shell-Unix-Generic"], ok: &[0], temp: false },
    Formatter { name: "gofmt", program: "gofmt", args: &[], languages: &["Go"], ok: &[0], temp: false },
    Formatter { name: "rustfmt", program: "rustfmt", args: &["--emit", "stdout", "--edition", "2021"], languages: &["Rust"], ok: &[0], temp: false },
    Formatter { name: "StyLua", program: "stylua", args: &["--stdin-filepath", "{file}", "-"], languages: &["Lua"], ok: &[0], temp: false },
    Formatter { name: "Taplo", program: "taplo", args: &["fmt", "-"], languages: &["TOML"], ok: &[0], temp: false },
    Formatter { name: "xmllint", program: "xmllint", args: &["--format", "-"], languages: &["XML"], ok: &[0], temp: false },
    Formatter { name: "jq", program: "jq", args: &["."], languages: &["JSON"], ok: &[0], temp: false },
    // PHP: php-cs-fixer works on files only; phpcbf reads standard input, and says 1 when it changed something.
    Formatter { name: "PHP CS Fixer", program: "php-cs-fixer", args: &["fix", "--quiet", "--using-cache=no", "{config}", "{temp}"], languages: &["PHP"], ok: &[0], temp: true },
    Formatter { name: "PHP_CodeSniffer (phpcbf)", program: "phpcbf", args: &["--stdin-path={file}", "{standard}", "-"], languages: &["PHP"], ok: &[0, 1], temp: false },
    // styler, for R: told the indent .editorconfig asks for.
    Formatter { name: "styler", program: "Rscript", args: &["-e", "txt <- readLines(file('stdin'), warn = FALSE); cat(as.character(styler::style_text(txt, indent_by = {indent})), sep = '\\n')"], languages: &["R"], ok: &[0], temp: false },
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
    chosen_near(language, choices, None)
}

/// The same for a file: a formatter the file's project keeps for itself counts as installed.
pub fn chosen_near<'a>(language: &str, choices: &std::collections::HashMap<String, String>, file: Option<&Path>) -> Option<&'a Formatter> {
    let options = for_language(language);
    let there = |f: &Formatter| installed(f) || file.is_some_and(|file| located(f, file).is_some());
    if let Some(name) = choices.get(language) {
        if name == "none" {
            return None;
        }
        if let Some(found) = options.iter().find(|f| f.name == name.as_str() && there(f)) {
            return Some(found);
        }
    }
    options.into_iter().find(|f| there(f))
}

/// The formatter's program for a file: the project's own copy, if there is one, and otherwise the one on the PATH.
fn located(formatter: &Formatter, file: &Path) -> Option<std::path::PathBuf> {
    let folder = file.parent().filter(|f| !f.as_os_str().is_empty()).map(Path::to_path_buf).or_else(|| std::env::current_dir().ok())?;
    crate::test_runners::project_program(&folder, formatter.program)
}

/// The project file in the file's folder or above it that has one of these names.
fn project_file(file: &Path, names: &[&str]) -> Option<std::path::PathBuf> {
    file.parent()?.ancestors().take(10).find_map(|folder| names.iter().map(|n| folder.join(n)).find(|p| p.is_file()))
}

/// The words a formatter is given in place of `{config}`, `{standard}` and `{indent}`, from the project's own
/// files: the settings of those that can't find them for themselves.
fn settings_words(formatter: &Formatter, file: &Path) -> (String, String, String) {
    let config = match project_file(file, &[".php-cs-fixer.php", ".php-cs-fixer.dist.php", ".php_cs", ".php_cs.dist"]) {
        Some(path) => format!("--config={}", path.display()),
        None => "--rules=@PSR12".to_string(),
    };
    // phpcbf finds phpcs.xml for itself; without one it's told the usual standard.
    let standard = match project_file(file, &["phpcs.xml", ".phpcs.xml", "phpcs.xml.dist", ".phpcs.xml.dist"]) {
        Some(_) => String::new(),
        None => "--standard=PSR12".to_string(),
    };
    let indent = crate::editorconfig::for_file(file).indent_size.filter(|n| (1..=16).contains(n)).unwrap_or(2).to_string();
    let _ = formatter;
    (config, standard, indent)
}

/// The arguments for a file: its name and the project's settings put in, and the blanks left over taken out.
fn arguments(formatter: &Formatter, file: &Path, temp: &Path) -> Vec<String> {
    let (config, standard, indent) = settings_words(formatter, file);
    formatter
        .args
        .iter()
        .map(|a| a.replace("{file}", &file.display().to_string()).replace("{temp}", &temp.display().to_string()).replace("{config}", &config).replace("{standard}", &standard).replace("{indent}", &indent))
        .filter(|a| !a.is_empty())
        .collect()
}

/// The text run through the formatter; an error says what it printed when it failed. It's given ten seconds.
pub fn run(formatter: &Formatter, text: &str, file: &Path) -> Result<String, String> {
    let program = located(formatter, file).or_else(|| program_path(formatter.program)).ok_or_else(|| crate::i18n::tf!("{} isn't installed.", formatter.name))?;
    // One that only works on files is given a copy, kept in the system's folder for temporary files.
    let temp = std::env::temp_dir().join(format!("limoniter-format-{}-{}.{}", std::process::id(), TEMPS.fetch_add(1, std::sync::atomic::Ordering::Relaxed), file.extension().map_or_else(|| "txt".into(), |e| e.to_string_lossy())));
    let args = arguments(formatter, file, &temp);
    if formatter.temp {
        std::fs::write(&temp, text).map_err(|e| e.to_string())?;
    }
    let result = run_program(formatter, &program, &args, text, file);
    let result = match result {
        Ok(_) if formatter.temp => std::fs::read(&temp).map_err(|e| e.to_string()).and_then(|bytes| String::from_utf8(bytes).map_err(|_| crate::i18n::tf!("{} gave back something that isn't text.", formatter.name))),
        other => other,
    };
    if formatter.temp {
        let _ = std::fs::remove_file(&temp);
    }
    result
}

static TEMPS: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

fn run_program(formatter: &Formatter, program: &Path, args: &[String], text: &str, file: &Path) -> Result<String, String> {
    let mut command = crate::platform::command(program);
    command.args(args).stdin(if formatter.temp { std::process::Stdio::null() } else { std::process::Stdio::piped() }).stdout(std::process::Stdio::piped()).stderr(std::process::Stdio::piped());
    if let Some(folder) = file.parent().filter(|f| f.is_dir()) {
        command.current_dir(folder);
    }
    let mut child = command.spawn().map_err(|e| e.to_string())?;
    let writer = child.stdin.take().map(|mut input| {
        let owned = text.to_string();
        std::thread::spawn(move || input.write_all(owned.as_bytes()))
    });
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
    if let Some(writer) = writer {
        let _ = writer.join();
    }
    if !output.status.code().is_some_and(|code| formatter.ok.contains(&code)) {
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
        let cat = Formatter { name: "cat", program: "cat", args: &[], languages: &[], ok: &[0], temp: false };
        assert_eq!(run(&cat, "b\na\n", Path::new("x.txt")).unwrap(), "b\na\n");
        let sort = Formatter { name: "sort", program: "sort", args: &[], languages: &[], ok: &[0], temp: false };
        assert_eq!(run(&sort, "b\na\n", Path::new("x.txt")).unwrap(), "a\nb\n");
        let failing = Formatter { name: "false", program: "false", args: &[], languages: &[], ok: &[0], temp: false };
        assert!(run(&failing, "x", Path::new("x.txt")).is_err());
    }

    fn project(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("limoniter-fmt-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[cfg(unix)]
    fn fake(dir: &Path, relative: &str, script: &str) {
        use std::os::unix::fs::PermissionsExt;
        let path = dir.join(relative);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, format!("#!/bin/sh\n{script}\n")).unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
    }

    /// `run`, tried again for a moment when the stand-in program was written so lately that starting it says it's
    /// busy (another test starting a program at that instant can still hold it open for writing).
    fn ran(formatter: &Formatter, text: &str, file: &Path) -> Result<String, String> {
        for _ in 0..40 {
            match run(formatter, text, file) {
                Err(e) if e.contains("busy") || e.contains("Text file") => std::thread::sleep(std::time::Duration::from_millis(50)),
                other => return other,
            }
        }
        run(formatter, text, file)
    }

    fn named(name: &str) -> &'static Formatter {
        ALL.iter().find(|f| f.name == name).unwrap()
    }

    #[test]
    fn the_projects_own_settings_are_passed_on() {
        let dir = project("settings");
        let file = dir.join("src/a.php");
        std::fs::create_dir_all(dir.join("src")).unwrap();
        let none = std::path::Path::new("/unused.tmp");
        // php-cs-fixer: the project's own configuration, or else PSR-12.
        assert!(arguments(named("PHP CS Fixer"), &file, none).contains(&"--rules=@PSR12".to_string()));
        std::fs::write(dir.join(".php-cs-fixer.dist.php"), "<?php return new PhpCsFixer\\Config();").unwrap();
        let args = arguments(named("PHP CS Fixer"), &file, none);
        assert!(args.contains(&format!("--config={}", dir.join(".php-cs-fixer.dist.php").display())), "{args:?}");
        assert_eq!(args.last().map(String::as_str), Some("/unused.tmp"), "the copy is the last word");
        // phpcbf: told PSR-12 only where the project has no phpcs.xml of its own.
        assert!(arguments(named("PHP_CodeSniffer (phpcbf)"), &file, none).contains(&"--standard=PSR12".to_string()));
        std::fs::write(dir.join("phpcs.xml"), "<ruleset/>").unwrap();
        let args = arguments(named("PHP_CodeSniffer (phpcbf)"), &file, none);
        assert!(!args.iter().any(|a| a.starts_with("--standard")) && args.contains(&format!("--stdin-path={}", file.display())), "{args:?}");
        // styler: the indent .editorconfig asks for, else two spaces.
        let r = dir.join("analysis.R");
        assert!(arguments(named("styler"), &r, none)[1].contains("indent_by = 2"));
        std::fs::write(dir.join(".editorconfig"), "root = true\n[*.R]\nindent_size = 4\n").unwrap();
        assert!(arguments(named("styler"), &r, none)[1].contains("indent_by = 4"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A formatter kept in the project is used before the one on the PATH, and counts as installed.
    #[cfg(unix)]
    #[test]
    fn the_projects_own_formatter_comes_first() {
        let dir = project("own");
        let file = dir.join("src/index.js");
        std::fs::create_dir_all(dir.join("src")).unwrap();
        fake(&dir, "node_modules/.bin/prettier", "cat >/dev/null; echo from-the-project");
        assert_eq!(ran(named("Prettier"), "x = 1\n", &file).unwrap(), "from-the-project\n");
        let mut choices = std::collections::HashMap::new();
        choices.insert("JavaScript".to_string(), "Prettier".to_string());
        assert_eq!(chosen_near("JavaScript", &choices, Some(&file)).map(|f| f.name), Some("Prettier"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// php-cs-fixer works on a file, so it's given a copy and the copy is read back; phpcbf says 1 when it changed
    /// something, which is not a failure.
    #[cfg(unix)]
    #[test]
    fn php_is_formatted_through_a_copy_or_standard_input() {
        let dir = project("php");
        let file = dir.join("a.php");
        // Written for BSD sed as well as GNU's: a Mac's `sed -i` wants a suffix, so the file is written anew.
        fake(&dir, "vendor/bin/php-cs-fixer", "for last; do :; done; sed 's/array()/[]/' \"$last\" > \"$last.new\" && mv \"$last.new\" \"$last\"");
        assert_eq!(ran(named("PHP CS Fixer"), "<?php $a = array();\n", &file).unwrap(), "<?php $a = [];\n");
        fake(&dir, "vendor/bin/phpcbf", "cat; exit 1");
        assert_eq!(ran(named("PHP_CodeSniffer (phpcbf)"), "<?php\n", &file).unwrap(), "<?php\n");
        fake(&dir, "vendor/bin/phpcbf", "echo broken >&2; exit 3");
        assert!(ran(named("PHP_CodeSniffer (phpcbf)"), "<?php\n", &file).unwrap_err().contains("broken"));
        let mut choices = std::collections::HashMap::new();
        assert_eq!(chosen_near("PHP", &choices, Some(&file)).map(|f| f.name), Some("PHP CS Fixer"), "the first of those the project has");
        choices.insert("PHP".to_string(), "PHP_CodeSniffer (phpcbf)".to_string());
        assert_eq!(chosen_near("PHP", &choices, Some(&file)).map(|f| f.name), Some("PHP_CodeSniffer (phpcbf)"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Black itself, if it's here: it finds `[tool.black]` in the project's pyproject.toml from the file's name.
    #[test]
    fn black_follows_the_projects_pyproject() {
        if !installed(named("Black")) {
            eprintln!("skipped: Black isn't installed here");
            return;
        }
        let dir = project("black");
        let file = dir.join("a.py");
        let long = "x = foo(aaaaaaaaaa, bbbbbbbbbb, cccccccccc)\n";
        let roomy = ran(named("Black"), long, &file).unwrap();
        assert_eq!(roomy, long, "88 columns is room enough");
        std::fs::write(dir.join("pyproject.toml"), "[tool.black]\nline-length = 20\n").unwrap();
        let narrow = ran(named("Black"), long, &file).unwrap();
        assert!(narrow.lines().count() > 1, "wrapped to the project's line length: {narrow}");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
