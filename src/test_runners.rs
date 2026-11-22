//! The test runners a project has (pytest, Jest, Vitest, PHPUnit, R's testthat, `cargo test` and `go test`): found
//! from the project's own files, asked to list their tests, and told which to run. What they print when they run is
//! read by `tasks::test_results`. The commands are plain text, run by the Tasks machinery the way any task is, so
//! they show in the Output panel as they go.

use std::path::{Path, PathBuf};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Runner {
    Pytest,
    Jest,
    Vitest,
    Phpunit,
    Testthat,
    Cargo,
    Go,
}

impl Runner {
    pub fn label(self) -> &'static str {
        match self {
            Runner::Pytest => "pytest",
            Runner::Jest => "Jest",
            Runner::Vitest => "Vitest",
            Runner::Phpunit => "PHPUnit",
            Runner::Testthat => "testthat",
            Runner::Cargo => "cargo test",
            Runner::Go => "go test",
        }
    }
}

/// One test: the group it's listed under (a file, a class or a package), its name there, and the whole name the
/// runner knows it by.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Item {
    pub group: String,
    pub name: String,
    pub id: String,
}

/// What to run.
#[derive(Clone, Copy, Debug)]
pub enum Scope<'a> {
    All,
    /// Every test in a group.
    Group(&'a str),
    One(&'a Item),
    /// Several tests: the ones that failed last time, say.
    Some(&'a [Item]),
}

/// A program the project keeps for itself, which comes before the one on the PATH: in `node_modules/.bin`,
/// `vendor/bin` or a virtual environment's `bin` (`Scripts` on Windows), in the folder or above it.
pub fn project_program(start: &Path, name: &str) -> Option<PathBuf> {
    let places: &[&str] = if cfg!(windows) { &["node_modules/.bin", "vendor/bin", ".venv/Scripts", "venv/Scripts", "env/Scripts"] } else { &["node_modules/.bin", "vendor/bin", ".venv/bin", "venv/bin", "env/bin"] };
    let endings: &[&str] = if cfg!(windows) { &[".exe", ".cmd", ".bat", ".phar", ""] } else { &[""] };
    start.ancestors().take(8).find_map(|folder| places.iter().find_map(|place| endings.iter().map(|e| folder.join(place).join(format!("{name}{e}"))).find(|p| p.is_file())))
}

/// The program to run: the project's own, else the name for the PATH to find. A path is quoted for the shell.
fn program(folder: &Path, name: &str) -> String {
    project_program(folder, name).map_or_else(|| name.to_string(), |p| quote(&p.display().to_string()))
}

/// The Python that runs pytest: the project's environment's, else the usual one.
fn python(folder: &Path) -> String {
    project_program(folder, "python").map_or_else(|| (if cfg!(windows) { "python" } else { "python3" }).to_string(), |p| quote(&p.display().to_string()))
}

/// A word quoted for the shell, as it's only when it needs to be.
pub fn quote(word: &str) -> String {
    if !word.is_empty() && word.chars().all(|c| c.is_alphanumeric() || "-_./:=@+,".contains(c)) {
        word.to_string()
    } else if cfg!(windows) {
        format!("\"{}\"", word.replace('"', "\\\""))
    } else {
        format!("'{}'", word.replace('\'', "'\\''"))
    }
}

/// A name with the characters a regular expression would take as its own made plain.
fn regex_escape(name: &str) -> String {
    let mut out = String::new();
    for c in name.chars() {
        if "\\.+*?()|[]{}^$#&-~".contains(c) {
            out.push('\\');
        }
        out.push(c);
    }
    out
}

fn mentions(folder: &Path, file: &str, word: &str) -> bool {
    std::fs::read_to_string(folder.join(file)).is_ok_and(|t| t.contains(word))
}

/// The runners a project has, found from its files.
pub fn find(folder: &Path) -> Vec<Runner> {
    let mut found = Vec::new();
    let has = |f: &str| folder.join(f).is_file();
    let python_tests = std::fs::read_dir(folder).is_ok_and(|e| e.flatten().any(|e| e.file_name().to_string_lossy().starts_with("test_") && e.file_name().to_string_lossy().ends_with(".py"))) || folder.join("tests").is_dir() && std::fs::read_dir(folder.join("tests")).is_ok_and(|e| e.flatten().any(|e| e.file_name().to_string_lossy().ends_with(".py")));
    if has("pytest.ini") || mentions(folder, "pyproject.toml", "pytest") || mentions(folder, "tox.ini", "pytest") || mentions(folder, "setup.cfg", "pytest") || python_tests {
        found.push(Runner::Pytest);
    }
    let package = std::fs::read_to_string(folder.join("package.json")).unwrap_or_default();
    let config = |name: &str| std::fs::read_dir(folder).is_ok_and(|e| e.flatten().any(|e| e.file_name().to_string_lossy().starts_with(name)));
    if package.contains("\"jest\"") || config("jest.config") {
        found.push(Runner::Jest);
    }
    if package.contains("\"vitest\"") || config("vitest.config") {
        found.push(Runner::Vitest);
    }
    if has("phpunit.xml") || has("phpunit.xml.dist") || folder.join("vendor/bin/phpunit").is_file() {
        found.push(Runner::Phpunit);
    }
    if folder.join("tests/testthat").is_dir() {
        found.push(Runner::Testthat);
    }
    if has("Cargo.toml") {
        found.push(Runner::Cargo);
    }
    if has("go.mod") {
        found.push(Runner::Go);
    }
    found
}

/// The command that lists a runner's tests, if it needs one (testthat's are found by reading the files).
pub fn collect_command(runner: Runner, folder: &Path) -> Option<String> {
    Some(match runner {
        Runner::Pytest => format!("{} -m pytest --collect-only -q", python(folder)),
        Runner::Jest => format!("{} --listTests", node_tool(folder, "jest")),
        Runner::Vitest => format!("{} list", node_tool(folder, "vitest")),
        Runner::Phpunit => format!("{} --list-tests", program(folder, "phpunit")),
        Runner::Testthat => return None,
        Runner::Cargo => "cargo test -- --list".to_string(),
        Runner::Go => "go test -list . ./...".to_string(),
    })
}

/// A tool Node projects have: the project's own, else the one `npx` finds without downloading anything.
fn node_tool(folder: &Path, name: &str) -> String {
    project_program(folder, name).map_or_else(|| format!("npx --no-install {name}"), |p| quote(&p.display().to_string()))
}

/// The tests in what the listing command printed (and, for those that list only files, in the files).
pub fn parse_collected(runner: Runner, folder: &Path, output: &str) -> Vec<Item> {
    match runner {
        Runner::Pytest => output
            .lines()
            .map(str::trim)
            .filter_map(|l| {
                let (file, rest) = l.split_once("::")?;
                (file.ends_with(".py") && !l.contains(' ')).then(|| Item { group: file.to_string(), name: rest.to_string(), id: l.to_string() })
            })
            .collect(),
        Runner::Vitest => output
            .lines()
            .map(str::trim)
            .filter_map(|l| {
                let (file, rest) = l.split_once(" > ")?;
                Some(Item { group: file.to_string(), name: rest.to_string(), id: l.to_string() })
            })
            .collect(),
        Runner::Jest => {
            let files: Vec<&str> = output.lines().map(str::trim).filter(|l| !l.is_empty() && Path::new(l).is_file()).collect();
            files.iter().flat_map(|file| scan_js(folder, file)).collect()
        }
        Runner::Phpunit => {
            let classes: Vec<(String, String)> = php_classes(folder);
            output
                .lines()
                .filter_map(|l| l.trim().strip_prefix("- "))
                .filter_map(|l| {
                    let (class, method) = l.trim().split_once("::")?;
                    let short = class.rsplit('\\').next().unwrap_or(class);
                    let group = classes.iter().find(|(c, _)| c == short).map_or_else(|| class.to_string(), |(_, file)| file.clone());
                    Some(Item { group, name: method.to_string(), id: l.trim().to_string() })
                })
                .collect()
        }
        Runner::Testthat => scan_r(folder),
        Runner::Cargo => output
            .lines()
            .filter_map(|l| l.trim().strip_suffix(": test"))
            .map(|id| {
                let group = id.rsplit_once("::").map_or_else(|| "(top level)".to_string(), |(module, _)| module.to_string());
                Item { name: id.rsplit("::").next().unwrap_or(id).to_string(), group, id: id.to_string() }
            })
            .collect(),
        Runner::Go => {
            // Each package's tests are listed, then "ok  package  0.01s" closes them.
            let mut items = Vec::new();
            let mut waiting: Vec<String> = Vec::new();
            for line in output.lines().map(str::trim) {
                if let Some(rest) = line.strip_prefix("ok ").or_else(|| line.strip_prefix("FAIL ")) {
                    let package = rest.split_whitespace().next().unwrap_or("").to_string();
                    items.extend(waiting.drain(..).map(|name| Item { group: package.clone(), id: name.clone(), name }));
                } else if line.starts_with("Test") || line.starts_with("Example") || line.starts_with("Benchmark") {
                    waiting.push(line.to_string());
                }
            }
            items
        }
    }
}

/// The names given to `it(` and `test(` in a JavaScript or TypeScript test file, as the file's tests.
fn scan_js(folder: &Path, file: &str) -> Vec<Item> {
    let Ok(text) = std::fs::read_to_string(file) else { return Vec::new() };
    let shown = Path::new(file).strip_prefix(folder).unwrap_or(Path::new(file)).display().to_string().replace('\\', "/");
    let Ok(pattern) = regex::Regex::new(r#"(?m)^\s*(?:it|test)(?:\.only|\.skip|\.concurrent)?\(\s*(?:'([^']+)'|"([^"]+)"|`([^`]+)`)"#) else { return Vec::new() };
    pattern
        .captures_iter(&text)
        .filter_map(|c| c.get(1).or_else(|| c.get(2)).or_else(|| c.get(3)))
        .map(|m| Item { group: shown.clone(), name: m.as_str().to_string(), id: m.as_str().to_string() })
        .collect()
}

/// The names given to `test_that(` in a project's `tests/testthat` files.
fn scan_r(folder: &Path) -> Vec<Item> {
    let dir = folder.join("tests").join("testthat");
    let Ok(pattern) = regex::Regex::new(r#"test_that\(\s*(?:"([^"]+)"|'([^']+)')"#) else { return Vec::new() };
    let mut files: Vec<PathBuf> = std::fs::read_dir(&dir).into_iter().flatten().flatten().map(|e| e.path()).filter(|p| p.extension().is_some_and(|e| e.eq_ignore_ascii_case("r")) && p.file_name().is_some_and(|n| n.to_string_lossy().starts_with("test"))).collect();
    files.sort();
    files
        .iter()
        .flat_map(|file| {
            let text = std::fs::read_to_string(file).unwrap_or_default();
            let shown = format!("tests/testthat/{}", file.file_name().map(|n| n.to_string_lossy()).unwrap_or_default());
            pattern.captures_iter(&text).filter_map(|c| c.get(1).or_else(|| c.get(2))).map(|m| Item { group: shown.clone(), name: m.as_str().to_string(), id: m.as_str().to_string() }).collect::<Vec<_>>()
        })
        .collect()
}

/// The classes in a project's PHP test files, each with its file, so PHPUnit's class names can be listed by file.
fn php_classes(folder: &Path) -> Vec<(String, String)> {
    let Ok(pattern) = regex::Regex::new(r"(?m)^\s*(?:final\s+|abstract\s+)?class\s+(\w+)") else { return Vec::new() };
    let mut found = Vec::new();
    let mut stack = vec![folder.to_path_buf()];
    let mut seen = 0;
    while let Some(dir) = stack.pop() {
        for entry in std::fs::read_dir(&dir).into_iter().flatten().flatten() {
            let path = entry.path();
            let name = entry.file_name().to_string_lossy().into_owned();
            if path.is_dir() {
                if !matches!(name.as_str(), "vendor" | "node_modules" | ".git") {
                    stack.push(path);
                }
            } else if name.ends_with(".php") && seen < 2000 {
                seen += 1;
                if let Ok(text) = std::fs::read_to_string(&path) {
                    let shown = path.strip_prefix(folder).unwrap_or(&path).display().to_string().replace('\\', "/");
                    found.extend(pattern.captures_iter(&text).map(|c| (c[1].to_string(), shown.clone())));
                }
            }
        }
    }
    found
}

/// The command that runs the tests asked for.
pub fn run_command(runner: Runner, folder: &Path, scope: Scope) -> String {
    let words = |items: &[Item], join: &str, f: fn(&Item) -> String| items.iter().map(f).collect::<Vec<_>>().join(join);
    match runner {
        Runner::Pytest => {
            let base = format!("{} -m pytest -v --durations=0 --durations-min=0", python(folder));
            match scope {
                Scope::All => base,
                Scope::Group(group) => format!("{base} {}", quote(group)),
                Scope::One(item) => format!("{base} {}", quote(&item.id)),
                Scope::Some(items) => format!("{base} {}", words(items, " ", |i| quote(&i.id))),
            }
        }
        Runner::Jest | Runner::Vitest => {
            let base = if runner == Runner::Jest { format!("{} --verbose", node_tool(folder, "jest")) } else { format!("{} run --reporter=verbose", node_tool(folder, "vitest")) };
            // Vitest's names start with their file, Jest's are the test's own, listed under their file.
            let leaf = |i: &Item| regex_escape(if runner == Runner::Vitest { i.name.rsplit(" > ").next().unwrap_or(&i.name) } else { &i.name });
            match scope {
                Scope::All => base,
                Scope::Group(group) => format!("{base} {}", quote(group)),
                Scope::One(item) => format!("{base} {} -t {}", quote(&item.group), quote(&format!("^{}$", leaf(item)))),
                Scope::Some(items) => {
                    let files: std::collections::BTreeSet<String> = items.iter().map(|i| quote(&i.group)).collect();
                    format!("{base} {} -t {}", files.into_iter().collect::<Vec<_>>().join(" "), quote(&format!("^({})$", items.iter().map(leaf).collect::<Vec<_>>().join("|"))))
                }
            }
        }
        Runner::Phpunit => {
            let base = format!("{} --teamcity", program(folder, "phpunit"));
            match scope {
                Scope::All => base,
                Scope::Group(group) if group.ends_with(".php") => format!("{base} {}", quote(group)),
                Scope::Group(group) => format!("{base} --filter {}", quote(&format!("/^{}::/", regex_escape(group)))),
                Scope::One(item) => format!("{base} --filter {}", quote(&format!("/^{}$/", regex_escape(&item.id)))),
                Scope::Some(items) => format!("{base} --filter {}", quote(&format!("/^({})$/", items.iter().map(|i| regex_escape(&i.id)).collect::<Vec<_>>().join("|")))),
            }
        }
        Runner::Testthat => {
            // testthat runs a file at a time; a test is run by running its file.
            let run = |dir: &str| format!("Rscript -e \"testthat::test_{dir}, reporter='location', stop_on_failure=FALSE)\"");
            let files: std::collections::BTreeSet<String> = match scope {
                Scope::All => std::collections::BTreeSet::new(),
                Scope::Group(group) => [group.to_string()].into(),
                Scope::One(item) => [item.group.clone()].into(),
                Scope::Some(items) => items.iter().map(|i| i.group.clone()).collect(),
            };
            if files.is_empty() {
                run(if folder.join("DESCRIPTION").is_file() { "local('.'" } else { "dir('tests/testthat'" })
            } else {
                files.iter().map(|f| run(&format!("file('{f}'"))).collect::<Vec<_>>().join(" && ")
            }
        }
        Runner::Cargo => match scope {
            Scope::All => "cargo test".to_string(),
            Scope::Group(group) => format!("cargo test {}::", quote(group)),
            Scope::One(item) => format!("cargo test -- --exact {}", quote(&item.id)),
            Scope::Some(items) => format!("cargo test -- --exact {}", words(items, " ", |i| quote(&i.id))),
        },
        Runner::Go => match scope {
            Scope::All => "go test -v ./...".to_string(),
            Scope::Group(group) => format!("go test -v {}", quote(group)),
            Scope::One(item) => format!("go test -v {} -run {}", quote(&item.group), quote(&format!("^{}$", item.id))),
            Scope::Some(items) => format!("go test -v ./... -run {}", quote(&format!("^({})$", words(items, "|", |i| i.id.clone())))),
        },
    }
}

/// Whether a result's name is this test's: the whole name, or (for Jest, which names only the test) its end.
pub fn same_test(item: &Item, result: &str) -> bool {
    result == item.id || result == item.name || result.ends_with(&format!("::{}", item.id)) || (result.ends_with(&format!(" > {}", item.name)) && result.starts_with(&item.group))
}

/// Runs a command line in a folder and gives back what it printed, with at most two minutes to finish.
pub fn capture(line: &str, folder: &Path) -> Result<String, String> {
    let mut command = crate::platform::shell_command(line);
    command.current_dir(folder).stdin(std::process::Stdio::null()).stdout(std::process::Stdio::piped()).stderr(std::process::Stdio::piped());
    let child = command.spawn().map_err(|e| crate::i18n::tf!("Couldn't run it.\n\n{e}"))?;
    let (send, receive) = std::sync::mpsc::channel();
    let id = child.id();
    std::thread::spawn(move || {
        let _ = send.send(child.wait_with_output());
    });
    match receive.recv_timeout(std::time::Duration::from_secs(120)) {
        Ok(output) => {
            let output = output.map_err(|e| e.to_string())?;
            let mut text = String::from_utf8_lossy(&output.stdout).into_owned();
            text.push_str(&String::from_utf8_lossy(&output.stderr));
            Ok(text)
        }
        Err(_) => {
            #[cfg(unix)]
            let _ = crate::platform::command("kill").arg("-9").arg(id.to_string()).status();
            #[cfg(windows)]
            let _ = crate::platform::command("taskkill").args(["/F", "/PID", &id.to_string()]).status();
            Err(crate::i18n::tr("It took too long, and was stopped.").to_string())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn folder(name: &str) -> PathBuf {
        let folder = std::env::temp_dir().join(format!("limoniter-runners-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&folder);
        std::fs::create_dir_all(&folder).unwrap();
        folder
    }

    /// A stand-in for a runner: a script that prints what the real one prints. Unix only, as it's a shell script.
    #[cfg(unix)]
    fn fake(folder: &Path, relative: &str, script: &str) {
        use std::os::unix::fs::PermissionsExt;
        let path = folder.join(relative);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, format!("#!/bin/sh\n{script}\n")).unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
    }

    /// Lists, then runs everything, as the panel does, and gives back the items and the results.
    #[cfg(unix)]
    fn list_and_run(runner: Runner, dir: &Path) -> (Vec<Item>, Vec<crate::tasks::TestResult>) {
        let listing = capture(&collect_command(runner, dir).unwrap(), dir).unwrap();
        let items = parse_collected(runner, dir, &listing);
        let output = capture(&run_command(runner, dir, Scope::All), dir).unwrap();
        let lines: Vec<String> = output.lines().map(str::to_string).collect();
        (items, crate::tasks::test_results(&lines))
    }

    #[test]
    fn runners_are_found_from_a_project_s_files() {
        let dir = folder("find");
        assert!(find(&dir).is_empty());
        std::fs::write(dir.join("package.json"), "{\"devDependencies\": {\"jest\": \"^29\", \"vitest\": \"^1\"}}").unwrap();
        std::fs::write(dir.join("phpunit.xml.dist"), "<phpunit/>").unwrap();
        std::fs::write(dir.join("go.mod"), "module x\n").unwrap();
        std::fs::write(dir.join("Cargo.toml"), "[package]\n").unwrap();
        std::fs::create_dir_all(dir.join("tests/testthat")).unwrap();
        std::fs::write(dir.join("pytest.ini"), "[pytest]\n").unwrap();
        assert_eq!(find(&dir), [Runner::Pytest, Runner::Jest, Runner::Vitest, Runner::Phpunit, Runner::Testthat, Runner::Cargo, Runner::Go]);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn each_runner_s_listing_is_read_by_file_and_name() {
        let dir = folder("parse");
        let pytest = parse_collected(Runner::Pytest, &dir, "tests/test_a.py::test_add\ntests/test_a.py::TestMaths::test_sub\n\n2 tests collected in 0.01s\n");
        assert_eq!(pytest.len(), 2);
        assert_eq!((pytest[1].group.as_str(), pytest[1].name.as_str(), pytest[1].id.as_str()), ("tests/test_a.py", "TestMaths::test_sub", "tests/test_a.py::TestMaths::test_sub"));
        let vitest = parse_collected(Runner::Vitest, &dir, "tests/sum.test.ts > sum > adds\ntests/sum.test.ts > sum > subtracts\n");
        assert_eq!((vitest[0].group.as_str(), vitest[0].name.as_str()), ("tests/sum.test.ts", "sum > adds"));
        let cargo = parse_collected(Runner::Cargo, &dir, "tests::adds: test\ntests::inner::subtracts: test\ntop: test\nbench_x: benchmark\n\n3 tests, 1 benchmarks\n");
        assert_eq!(cargo.iter().map(|i| (i.group.as_str(), i.name.as_str())).collect::<Vec<_>>(), [("tests", "adds"), ("tests::inner", "subtracts"), ("(top level)", "top")]);
        let go = parse_collected(Runner::Go, &dir, "TestAdd\nTestSub\nok  \texample.com/maths\t0.003s\n?   \texample.com/empty\t[no test files]\n");
        assert_eq!(go.iter().map(|i| (i.group.as_str(), i.id.as_str())).collect::<Vec<_>>(), [("example.com/maths", "TestAdd"), ("example.com/maths", "TestSub")]);
        // PHPUnit's classes are put under the files they're in.
        std::fs::create_dir_all(dir.join("tests")).unwrap();
        std::fs::write(dir.join("tests/MathTest.php"), "<?php\nfinal class MathTest extends TestCase {}\n").unwrap();
        let php = parse_collected(Runner::Phpunit, &dir, "PHPUnit 10.5\n\nAvailable test(s):\n - Tests\\MathTest::testAdd\n - Tests\\MathTest::testSub with data set #0\n - Tests\\Other::testX\n");
        assert_eq!(php.iter().map(|i| (i.group.as_str(), i.name.as_str())).collect::<Vec<_>>(), [("tests/MathTest.php", "testAdd"), ("tests/MathTest.php", "testSub with data set #0"), ("Tests\\Other", "testX")]);
        // Jest lists files; the names are read from them.
        std::fs::write(dir.join("sum.test.js"), "describe('sum', () => {\n  it('adds', () => {});\n  test(\"subtracts\", () => {});\n  it.skip(`skipped one`, () => {});\n});\n").unwrap();
        let jest = parse_collected(Runner::Jest, &dir, &format!("{}\n", dir.join("sum.test.js").display()));
        assert_eq!(jest.iter().map(|i| (i.group.as_str(), i.name.as_str())).collect::<Vec<_>>(), [("sum.test.js", "adds"), ("sum.test.js", "subtracts"), ("sum.test.js", "skipped one")]);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn each_runner_is_told_what_to_run() {
        let dir = folder("commands");
        let item = |group: &str, name: &str, id: &str| Item { group: group.into(), name: name.into(), id: id.into() };
        let py = item("tests/test_a.py", "test_add", "tests/test_a.py::test_add");
        assert!(run_command(Runner::Pytest, &dir, Scope::All).ends_with("-m pytest -v --durations=0 --durations-min=0"));
        assert!(run_command(Runner::Pytest, &dir, Scope::One(&py)).ends_with("tests/test_a.py::test_add"));
        assert!(run_command(Runner::Pytest, &dir, Scope::Group("tests/test_a.py")).ends_with(" tests/test_a.py"));
        let cargo = item("tests", "adds", "tests::adds");
        assert_eq!(run_command(Runner::Cargo, &dir, Scope::One(&cargo)), "cargo test -- --exact tests::adds");
        assert_eq!(run_command(Runner::Cargo, &dir, Scope::Some(&[cargo.clone(), item("tests", "subs", "tests::subs")])), "cargo test -- --exact tests::adds tests::subs");
        let go = item("example.com/m", "TestAdd", "TestAdd");
        assert_eq!(run_command(Runner::Go, &dir, Scope::One(&go)), run_command(Runner::Go, &dir, Scope::All).replace("./...", "example.com/m -run").replace(" -run", &format!(" -run {}", quote("^TestAdd$"))));
        let php = item("tests/MathTest.php", "testAdd", "Tests\\MathTest::testAdd");
        assert!(run_command(Runner::Phpunit, &dir, Scope::All).ends_with("phpunit --teamcity"));
        assert!(run_command(Runner::Phpunit, &dir, Scope::Group("tests/MathTest.php")).ends_with("--teamcity tests/MathTest.php"));
        assert!(run_command(Runner::Phpunit, &dir, Scope::One(&php)).contains("--filter") && run_command(Runner::Phpunit, &dir, Scope::One(&php)).contains("Tests\\\\MathTest::testAdd"));
        let js = item("sum.test.js", "adds (1+1)", "adds (1+1)");
        let one = run_command(Runner::Jest, &dir, Scope::One(&js));
        assert!(one.contains("sum.test.js") && one.contains("-t") && one.contains("adds \\(1\\+1\\)"), "{one}");
        let vi = item("sum.test.ts", "sum > adds", "sum.test.ts > sum > adds");
        assert!(run_command(Runner::Vitest, &dir, Scope::One(&vi)).contains("--reporter=verbose sum.test.ts -t"));
        let r = item("tests/testthat/test-a.R", "adds", "adds");
        assert!(run_command(Runner::Testthat, &dir, Scope::All).contains("test_dir('tests/testthat'"));
        assert!(run_command(Runner::Testthat, &dir, Scope::One(&r)).contains("test_file('tests/testthat/test-a.R'"));
        // A name with spaces or quotes is one word to the shell.
        assert_eq!(quote("a b"), if cfg!(windows) { "\"a b\"" } else { "'a b'" });
        assert_eq!(quote("tests/a.py::t"), "tests/a.py::t");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_result_is_matched_to_its_test() {
        let item = Item { group: "tests/sum.test.ts".into(), name: "sum > adds".into(), id: "tests/sum.test.ts > sum > adds".into() };
        assert!(same_test(&item, "tests/sum.test.ts > sum > adds"));
        assert!(!same_test(&item, "tests/other.test.ts > sum > adds"));
        let rust = Item { group: "tests".into(), name: "adds".into(), id: "tests::adds".into() };
        assert!(same_test(&rust, "tests::adds") && !same_test(&rust, "tests::adds_more"));
    }

    /// The project's own copy of a program comes before the one on the PATH.
    #[cfg(unix)]
    #[test]
    fn the_project_s_own_programs_come_first() {
        let dir = folder("own");
        assert_eq!(project_program(&dir, "phpunit"), None);
        fake(&dir, "vendor/bin/phpunit", "echo hi");
        fake(&dir, "node_modules/.bin/jest", "echo hi");
        fake(&dir, ".venv/bin/python", "echo hi");
        std::fs::create_dir_all(dir.join("src/deeper")).unwrap();
        assert_eq!(project_program(&dir.join("src/deeper"), "phpunit"), Some(dir.join("vendor/bin/phpunit")), "found from a folder below");
        assert!(run_command(Runner::Phpunit, &dir, Scope::All).starts_with(&dir.display().to_string()));
        assert!(run_command(Runner::Jest, &dir, Scope::All).starts_with(&dir.join("node_modules/.bin/jest").display().to_string()));
        assert!(run_command(Runner::Pytest, &dir, Scope::All).starts_with(&dir.join(".venv/bin/python").display().to_string()));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[cfg(unix)]
    #[test]
    fn phpunit_is_listed_and_run_through_a_fake() {
        let dir = folder("phpunit");
        std::fs::write(dir.join("phpunit.xml"), "<phpunit/>").unwrap();
        std::fs::create_dir_all(dir.join("tests")).unwrap();
        std::fs::write(dir.join("tests/MathTest.php"), "<?php\nclass MathTest {}\n").unwrap();
        let teamcity = "##teamcity[testStarted name='testAdd' locationHint='php_qn:///w/tests/MathTest.php::\\\\MathTest::testAdd' flowId='1']\n##teamcity[testFinished name='testAdd' duration='3' flowId='1']\n##teamcity[testStarted name='testSub' locationHint='php_qn:///w/tests/MathTest.php::\\\\MathTest::testSub' flowId='1']\n##teamcity[testFailed name='testSub' message='Failed asserting that 0 matches expected 1.' details=' |n/w/tests/MathTest.php:12|n ' duration='1' type='comparisonFailure' actual='0' expected='1' flowId='1']\n##teamcity[testFinished name='testSub' duration='1' flowId='1']";
        fake(&dir, "vendor/bin/phpunit", &format!("if [ \"$1\" = \"--list-tests\" ]; then printf 'Available test(s):\\n - MathTest::testAdd\\n - MathTest::testSub\\n'; else cat <<'EOT'\n{teamcity}\nEOT\nexit 1; fi"));
        assert_eq!(find(&dir), [Runner::Phpunit]);
        let (items, results) = list_and_run(Runner::Phpunit, &dir);
        assert_eq!(items.len(), 2);
        assert!(items.iter().all(|i| i.group == "tests/MathTest.php"));
        assert_eq!(results.len(), 2);
        assert!(results[0].passed && !results[1].passed);
        assert_eq!(results[1].detail, ["Expected: 1", "Got: 0"]);
        assert!(items.iter().zip(&results).all(|(i, r)| same_test(i, &r.name)), "{items:?} {results:?}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[cfg(unix)]
    #[test]
    fn jest_and_vitest_are_listed_and_run_through_fakes() {
        let dir = folder("js");
        std::fs::write(dir.join("package.json"), "{\"devDependencies\": {\"jest\": \"29\", \"vitest\": \"1\"}}").unwrap();
        std::fs::write(dir.join("sum.test.js"), "it('adds', () => {});\nit('subtracts', () => {});\n").unwrap();
        let file = dir.join("sum.test.js");
        fake(&dir, "node_modules/.bin/jest", &format!("if [ \"$1\" = \"--listTests\" ]; then echo '{}'; else cat <<'EOT'\nFAIL ./sum.test.js\n  \u{2713} adds (2 ms)\n  \u{2715} subtracts (3 ms)\n\n  \u{25CF} subtracts\n\n    Expected: 1\n    Received: 0\n\n      at Object.<anonymous> (sum.test.js:2:19)\nEOT\nexit 1; fi", file.display()));
        fake(&dir, "node_modules/.bin/vitest", "if [ \"$1\" = \"list\" ]; then printf 'sum.test.ts > sum > adds\\nsum.test.ts > sum > subtracts\\n'; else cat <<'EOT'\n \u{2713} sum.test.ts > sum > adds 3ms\n \u{00D7} sum.test.ts > sum > subtracts 2ms\n\n FAIL  sum.test.ts > sum > subtracts\nAssertionError: expected 0 to be 1\n\n \u{276F} sum.test.ts:5:23\nEOT\nexit 1; fi");
        let (items, results) = list_and_run(Runner::Jest, &dir);
        assert_eq!(items.iter().map(|i| i.name.as_str()).collect::<Vec<_>>(), ["adds", "subtracts"]);
        assert!(items.iter().zip(&results).all(|(i, r)| same_test(i, &r.name)));
        assert_eq!(results[1].detail, ["Expected: 1", "Received: 0"]);
        assert_eq!(results[1].place, Some(("sum.test.js".into(), 2)));
        let (items, results) = list_and_run(Runner::Vitest, &dir);
        assert_eq!(items.len(), 2);
        assert!(items.iter().zip(&results).all(|(i, r)| same_test(i, &r.name)));
        assert_eq!(results[1].place, Some(("sum.test.ts".into(), 5)));
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// pytest itself, if it's here: listed, run all, and run one.
    #[test]
    fn real_pytest_is_listed_and_run() {
        let dir = folder("pytest");
        let python = if cfg!(windows) { "python" } else { "python3" };
        // It answers "pytest 8.3.4"; "No module named pytest" means it isn't there.
        if !capture(&format!("{python} -m pytest --version"), &dir).is_ok_and(|v| v.trim_start().to_lowercase().starts_with("pytest ")) {
            eprintln!("skipped: pytest isn't installed here");
            return;
        }
        std::fs::write(dir.join("pytest.ini"), "[pytest]\n").unwrap();
        std::fs::write(dir.join("test_maths.py"), "def test_add():\n    assert 1 + 1 == 2\n\n\ndef test_sub():\n    assert 1 - 1 == 1\n").unwrap();
        assert_eq!(find(&dir), [Runner::Pytest]);
        let items = parse_collected(Runner::Pytest, &dir, &capture(&collect_command(Runner::Pytest, &dir).unwrap(), &dir).unwrap());
        assert_eq!(items.iter().map(|i| i.id.as_str()).collect::<Vec<_>>(), ["test_maths.py::test_add", "test_maths.py::test_sub"]);
        let read = |scope: Scope| {
            let output = capture(&run_command(Runner::Pytest, &dir, scope), &dir).unwrap();
            crate::tasks::test_results(&output.lines().map(str::to_string).collect::<Vec<_>>())
        };
        let all = read(Scope::All);
        assert_eq!(all.len(), 2, "{all:?}");
        assert!(all[0].passed && !all[1].passed);
        assert_eq!(all[1].place, Some(("test_maths.py".into(), 6)));
        assert!(all[1].detail.iter().any(|l| l.contains("assert 0 == 1")), "{:?}", all[1].detail);
        assert!(all[0].note.is_some(), "its time: {:?}", all[0]);
        let one = read(Scope::One(&items[0]));
        assert_eq!(one.len(), 1);
        assert!(one[0].passed && same_test(&items[0], &one[0].name));
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// testthat itself, if R has it.
    #[test]
    fn real_testthat_is_listed_and_run() {
        let dir = folder("testthat");
        if !capture("Rscript -e \"cat(requireNamespace('testthat', quietly = TRUE))\"", &dir).is_ok_and(|o| o.trim_end().ends_with("TRUE")) {
            eprintln!("skipped: R's testthat isn't installed here");
            return;
        }
        std::fs::create_dir_all(dir.join("tests/testthat")).unwrap();
        std::fs::write(dir.join("tests/testthat/test-a.R"), "test_that(\"adds\", { expect_equal(1 + 1, 2) })\ntest_that(\"subtracts\", { expect_equal(1 - 1, 1) })\n").unwrap();
        assert_eq!(find(&dir), [Runner::Testthat]);
        let items = parse_collected(Runner::Testthat, &dir, "");
        assert_eq!(items.iter().map(|i| (i.group.as_str(), i.name.as_str())).collect::<Vec<_>>(), [("tests/testthat/test-a.R", "adds"), ("tests/testthat/test-a.R", "subtracts")]);
        for scope in [Scope::All, Scope::One(&items[1])] {
            let output = capture(&run_command(Runner::Testthat, &dir, scope), &dir).unwrap();
            let results = crate::tasks::test_results(&output.lines().map(str::to_string).collect::<Vec<_>>());
            assert_eq!(results.len(), 2, "{output}");
            assert!(results[0].passed && !results[1].passed);
            assert_eq!(results[1].place, Some(("tests/testthat/test-a.R".into(), 2)));
            assert!(dir.join(&results[1].place.as_ref().unwrap().0).is_file(), "the place is a real file from the project's folder");
        }
        let _ = std::fs::remove_dir_all(&dir);
    }
}
