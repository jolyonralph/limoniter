//! Build, run and test tasks: the ones a project's own files say (Cargo, Make, npm, Go, pytest, CMake), and any
//! kept with a Limoniter project; where compilers say the problems are in what they print; and test runners'
//! results, test by test.

use std::path::{Path, PathBuf};

/// What a task is for: Ctrl+Shift+B runs the first build task, and a test task's results are read test by test.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Build,
    Test,
    Run,
    Other,
}

impl Kind {
    pub const ALL: [Kind; 4] = [Kind::Build, Kind::Test, Kind::Run, Kind::Other];

    pub fn id(self) -> &'static str {
        match self {
            Kind::Build => "build",
            Kind::Test => "test",
            Kind::Run => "run",
            Kind::Other => "other",
        }
    }

    pub fn from_id(id: &str) -> Kind {
        Kind::ALL.into_iter().find(|k| k.id() == id).unwrap_or(Kind::Other)
    }

    pub fn label(self) -> &'static str {
        match self {
            Kind::Build => "Build",
            Kind::Test => "Test",
            Kind::Run => "Run",
            Kind::Other => "Other",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Task {
    pub name: String,
    pub command: String,
    pub kind: Kind,
}

fn task(name: &str, command: &str, kind: Kind) -> Task {
    Task { name: name.into(), command: command.into(), kind }
}

/// The folder a file's tasks run in: the nearest above it with a project's own file, or its own.
pub fn root_for(file: &Path) -> PathBuf {
    crate::lsp::root_for(file)
}

/// The tasks a folder's own files say: Cargo.toml, a Makefile, package.json's scripts, go.mod, pytest's files,
/// CMakeLists.txt.
pub fn detect(folder: &Path) -> Vec<Task> {
    let mut found = Vec::new();
    if folder.join("Cargo.toml").is_file() {
        found.extend([task("cargo build", "cargo build", Kind::Build), task("cargo test", "cargo test", Kind::Test), task("cargo run", "cargo run", Kind::Run), task("cargo clippy", "cargo clippy", Kind::Build)]);
    }
    if let Ok(text) = std::fs::read_to_string(folder.join("Makefile")) {
        found.push(task("make", "make", Kind::Build));
        for target in ["test", "check", "run", "clean"] {
            if text.lines().any(|l| l.starts_with(&format!("{target}:"))) {
                let kind = match target {
                    "test" | "check" => Kind::Test,
                    "run" => Kind::Run,
                    _ => Kind::Other,
                };
                found.push(task(&format!("make {target}"), &format!("make {target}"), kind));
            }
        }
    }
    if let Ok(text) = std::fs::read_to_string(folder.join("package.json"))
        && let Ok(value) = serde_json::from_str::<serde_json::Value>(&text)
        && let Some(scripts) = value.get("scripts").and_then(|s| s.as_object())
    {
        for name in scripts.keys() {
            let kind = match name.as_str() {
                "build" => Kind::Build,
                "test" => Kind::Test,
                "start" | "dev" | "serve" => Kind::Run,
                _ => Kind::Other,
            };
            found.push(task(&format!("npm run {name}"), &format!("npm run {name}"), kind));
        }
    }
    if folder.join("go.mod").is_file() {
        found.extend([task("go build", "go build ./...", Kind::Build), task("go test", "go test -v ./...", Kind::Test), task("go vet", "go vet ./...", Kind::Build)]);
    }
    let python = ["pyproject.toml", "pytest.ini", "setup.py", "setup.cfg", "tox.ini"].iter().any(|f| folder.join(f).is_file()) || std::fs::read_dir(folder).is_ok_and(|entries| entries.flatten().any(|e| e.file_name().to_string_lossy().starts_with("test_") && e.file_name().to_string_lossy().ends_with(".py")));
    if python {
        found.push(task("pytest", "python -m pytest -v", Kind::Test));
    }
    if folder.join("CMakeLists.txt").is_file() {
        found.extend([task("cmake configure", "cmake -S . -B build", Kind::Other), task("cmake build", "cmake --build build", Kind::Build)]);
    }
    found
}

/// A place a compiler or tool printed: the file, the line (from 1), and what it said.
pub fn place_in(line: &str) -> Option<(String, usize, String)> {
    let trimmed = line.trim_start();
    // rustc: "  --> src/main.rs:12:5".
    if let Some(rest) = trimmed.strip_prefix("--> ").or_else(|| trimmed.strip_prefix("::: ")) {
        let mut parts = rest.rsplitn(3, ':');
        let _col = parts.next()?;
        let number = parts.next()?.parse().ok()?;
        return Some((parts.next()?.to_string(), number, String::new()));
    }
    // Python: '  File "x.py", line 12, in f'.
    if let Some(rest) = trimmed.strip_prefix("File \"") {
        let (file, rest) = rest.split_once("\", line ")?;
        let number = rest.split(|c: char| !c.is_ascii_digit()).next()?.parse().ok()?;
        return Some((file.to_string(), number, String::new()));
    }
    // A Rust panic: "thread 'x' panicked at src/lib.rs:12:5:".
    if let Some(at) = line.find("panicked at ") {
        let rest = line[at + 12..].trim_end_matches(':');
        let mut parts = rest.rsplitn(3, ':');
        let _col = parts.next()?;
        let number = parts.next()?.parse().ok()?;
        return Some((parts.next()?.to_string(), number, "panicked".into()));
    }
    // gcc, clang, Go and the rest: "file:12:5: error: words" or "file:12: words". The file has a dot or a slash,
    // and on Windows may start with a drive letter.
    let (drive, rest) = if trimmed.len() > 2 && trimmed.as_bytes()[1] == b':' && trimmed.as_bytes()[0].is_ascii_alphabetic() { trimmed.split_at(2) } else { ("", trimmed) };
    let mut parts = rest.splitn(4, ':');
    let file = parts.next()?;
    let number: usize = parts.next()?.trim().parse().ok()?;
    if file.is_empty() || file.contains(' ') && !file.contains('/') && !file.contains('\\') || !(file.contains('.') || file.contains('/')) {
        return None;
    }
    let after: Vec<&str> = parts.collect();
    let words = match after.as_slice() {
        [col, words] if col.trim().parse::<usize>().is_ok() => words.trim(),
        [words, more] => return Some((format!("{drive}{file}"), number, format!("{}:{}", words.trim(), more))),
        [words] => words.trim(),
        _ => "",
    };
    if words.starts_with("warning") || words.starts_with("note") {
        return None;
    }
    Some((format!("{drive}{file}"), number, words.trim_start_matches("error:").trim().to_string()))
}

/// One test's result.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TestResult {
    pub name: String,
    pub passed: bool,
    /// Where it failed, if the output says.
    pub place: Option<(String, usize)>,
}

/// The tests in a test runner's output, as cargo test, pytest -v and go test -v print them.
pub fn test_results(lines: &[String]) -> Vec<TestResult> {
    let mut results: Vec<TestResult> = Vec::new();
    for line in lines {
        let t = line.trim();
        // cargo: "test module::name ... ok" / "... FAILED".
        if let Some(rest) = t.strip_prefix("test ")
            && let Some((name, outcome)) = rest.rsplit_once(" ... ")
        {
            match outcome.trim() {
                "ok" => results.push(TestResult { name: name.into(), passed: true, place: None }),
                "FAILED" => results.push(TestResult { name: name.into(), passed: false, place: None }),
                _ => {}
            }
            continue;
        }
        // go: "--- PASS: TestName (0.00s)".
        if let Some(rest) = t.strip_prefix("--- PASS: ").or_else(|| t.strip_prefix("--- FAIL: ")) {
            let name = rest.split_whitespace().next().unwrap_or("").to_string();
            results.push(TestResult { name, passed: t.starts_with("--- PASS"), place: None });
            continue;
        }
        // pytest -v: "tests/test_x.py::test_name PASSED [ 50%]".
        if let Some(at) = t.find(" PASSED").or_else(|| t.find(" FAILED")).or_else(|| t.find(" ERROR"))
            && t[..at].contains("::")
            && !t.starts_with("FAILED ")
        {
            results.push(TestResult { name: t[..at].to_string(), passed: t[at..].starts_with(" PASSED"), place: None });
        }
    }
    // Where failures happened: cargo's panics name the thread (the test), pytest's "file:line: Error" follows
    // the test's name in its failures section, go's "    x_test.go:12: words" follows "=== RUN".
    for (i, line) in lines.iter().enumerate() {
        if let Some(rest) = line.trim_start().strip_prefix("thread '")
            && let Some((name, _)) = rest.split_once("' ")
            && let Some((file, number, _)) = place_in(line)
            && let Some(result) = results.iter_mut().find(|r| r.name == name)
        {
            result.place = Some((file, number));
            continue;
        }
        if line.trim_start().starts_with("--- FAIL: ") {
            let name = line.trim_start()[10..].split_whitespace().next().unwrap_or("");
            // go prints the test's own lines before its --- FAIL.
            let place = lines[..i].iter().rev().take(20).find_map(|l| place_in(l).filter(|(f, _, _)| f.ends_with("_test.go")));
            if let (Some((file, number, _)), Some(result)) = (place, results.iter_mut().find(|r| r.name == name)) {
                result.place = Some((file, number));
            }
        }
        if let Some(name) = line.trim().strip_prefix("____").map(|s| s.trim_matches(|c| c == '_' || c == ' ')) {
            let place = lines[i..].iter().take(60).find_map(|l| place_in(l).filter(|(f, _, _)| f.ends_with(".py")));
            if let Some((file, number, _)) = place
                && let Some(result) = results.iter_mut().find(|r| !r.passed && r.name.ends_with(&format!("::{name}")))
            {
                result.place = Some((file, number));
            }
        }
    }
    results
}

/// The command that runs only one test, from the command that runs them all.
pub fn only_this(command: &str, test: &str) -> String {
    let base = command.trim();
    if base.starts_with("cargo test") {
        format!("{base} {test} -- --exact")
    } else if base.starts_with("go test") {
        format!("{base} -run '^{test}$'")
    } else if base.contains("pytest") {
        format!("{base} \"{test}\"")
    } else {
        format!("{base} {test}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn places_are_found_in_what_compilers_print() {
        assert_eq!(place_in("src/main.c:12:5: error: expected ';'"), Some(("src/main.c".into(), 12, "expected ';'".into())));
        assert_eq!(place_in("   --> src/lib.rs:40:9"), Some(("src/lib.rs".into(), 40, String::new())));
        assert_eq!(place_in("  File \"tool.py\", line 7, in main"), Some(("tool.py".into(), 7, String::new())));
        assert_eq!(place_in("./x_test.go:21: wrong answer"), Some(("./x_test.go".into(), 21, "wrong answer".into())));
        assert_eq!(place_in("C:\\work\\main.c:3:1: error: oops").map(|p| (p.0, p.1)), Some(("C:\\work\\main.c".into(), 3)));
        assert_eq!(place_in("thread 'tests::adds' (123) panicked at src/lib.rs:12:5:").map(|p| (p.0, p.1)), Some(("src/lib.rs".into(), 12)));
        assert_eq!(place_in("src/main.c:12:5: warning: unused"), None, "warnings aren't problems");
        assert_eq!(place_in("Compiling foo v0.1.0"), None);
        assert_eq!(place_in("real 0m1.2s"), None);
    }

    #[test]
    fn test_results_are_read_from_each_runner() {
        let lines = |text: &str| text.lines().map(str::to_string).collect::<Vec<_>>();
        let cargo = test_results(&lines("running 2 tests\ntest tests::adds ... ok\ntest tests::subtracts ... FAILED\n\nfailures:\n\n---- tests::subtracts stdout ----\nthread 'tests::subtracts' (99) panicked at src/lib.rs:20:9:\nassertion failed\n"));
        assert_eq!(cargo, [TestResult { name: "tests::adds".into(), passed: true, place: None }, TestResult { name: "tests::subtracts".into(), passed: false, place: Some(("src/lib.rs".into(), 20)) }]);
        let pytest = test_results(&lines("test_maths.py::test_add PASSED  [ 50%]\ntest_maths.py::test_sub FAILED  [100%]\n=== FAILURES ===\n____ test_sub ____\n    def test_sub():\n>       assert 1 - 1 == 1\nE       assert 0 == 1\n\ntest_maths.py:6: AssertionError\n"));
        assert_eq!(pytest.len(), 2);
        assert_eq!(pytest[1].place, Some(("test_maths.py".into(), 6)));
        let go = test_results(&lines("=== RUN   TestAdd\n--- PASS: TestAdd (0.00s)\n=== RUN   TestSub\n    maths_test.go:14: got 0, want 1\n--- FAIL: TestSub (0.00s)\nFAIL\n"));
        assert_eq!(go[1], TestResult { name: "TestSub".into(), passed: false, place: Some(("maths_test.go".into(), 14)) });
        assert_eq!(only_this("cargo test", "tests::adds"), "cargo test tests::adds -- --exact");
        assert_eq!(only_this("go test -v ./...", "TestSub"), "go test -v ./... -run '^TestSub$'");
    }

    #[test]
    fn a_folder_s_tasks_are_found() {
        let folder = std::env::temp_dir().join(format!("limoniter-tasks-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&folder);
        std::fs::create_dir_all(&folder).unwrap();
        std::fs::write(folder.join("Cargo.toml"), "[package]\nname = \"x\"\n").unwrap();
        std::fs::write(folder.join("Makefile"), "all:\n\tcc x.c\ntest:\n\t./x\n").unwrap();
        std::fs::write(folder.join("package.json"), "{\"scripts\": {\"build\": \"tsc\", \"test\": \"jest\"}}").unwrap();
        let tasks = detect(&folder);
        let names: Vec<&str> = tasks.iter().map(|t| t.name.as_str()).collect();
        assert!(names.contains(&"cargo test") && names.contains(&"make test") && names.contains(&"npm run build"), "{names:?}");
        assert_eq!(tasks.iter().find(|t| t.name == "npm run test").map(|t| t.kind), Some(Kind::Test));
        let _ = std::fs::remove_dir_all(folder);
    }
}
