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
    // package.json's scripts, run with the package manager the project's lock file names.
    if folder.join("package.json").is_file() {
        let manager = crate::node_tools::manager_for(folder);
        for name in crate::node_tools::scripts(folder) {
            let kind = match name.as_str() {
                "build" => Kind::Build,
                "test" => Kind::Test,
                "start" | "dev" | "serve" => Kind::Run,
                _ => Kind::Other,
            };
            let command = manager.run_script(&name);
            found.push(task(&command, &command, kind));
        }
        found.push(task(&manager.install(), &manager.install(), Kind::Other));
    }
    // Composer, for PHP: its three everyday commands, and each of composer.json's own scripts.
    if let Ok(text) = std::fs::read_to_string(folder.join("composer.json")) {
        found.extend([task("composer install", "composer install", Kind::Other), task("composer update", "composer update", Kind::Other), task("composer dump-autoload", "composer dump-autoload", Kind::Other)]);
        if let Some(scripts) = serde_json::from_str::<serde_json::Value>(&text).ok().and_then(|v| v.get("scripts").and_then(|s| s.as_object()).cloned()) {
            for name in scripts.keys() {
                // Composer's own events (pre-install-cmd and the rest) run by themselves.
                if name.starts_with("pre-") || name.starts_with("post-") {
                    continue;
                }
                let kind = match name.as_str() {
                    "test" | "tests" | "phpunit" => Kind::Test,
                    "start" | "serve" | "dev" => Kind::Run,
                    "build" => Kind::Build,
                    _ => Kind::Other,
                };
                found.push(task(&format!("composer {name}"), &format!("composer run-script {name}"), kind));
            }
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
    // PlatformIO, for Arduino, AVR, ESP32 and the rest: building, uploading to the board, its tests and its monitor.
    if folder.join("platformio.ini").is_file() {
        found.extend([
            task("PlatformIO: build", "pio run", Kind::Build),
            task("PlatformIO: upload to the board", "pio run -t upload", Kind::Run),
            task("PlatformIO: test", "pio test", Kind::Test),
            task("PlatformIO: serial monitor", "pio device monitor", Kind::Other),
        ]);
    }
    // Docker Compose: the whole project up, down, or its images pulled.
    for (name, command) in crate::compose::tasks(folder) {
        found.push(task(&name, &command, Kind::Other));
    }
    found
}

/// How a microcontroller is flashed: the programmer, the chip, and the port it's on, for avrdude (AVR) and MPLAB
/// IPE's ipecmd (PIC). Kept in the settings file.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Flashing {
    pub avr_programmer: String,
    pub avr_part: String,
    pub avr_port: String,
    pub pic_tool: String,
    pub pic_part: String,
}

impl Default for Flashing {
    fn default() -> Self {
        Flashing { avr_programmer: "arduino".into(), avr_part: "atmega328p".into(), avr_port: String::new(), pic_tool: "PK4".into(), pic_part: "16F877A".into() }
    }
}

/// avrdude's programmers most often used, and what each is.
pub const AVR_PROGRAMMERS: [(&str, &str); 9] = [
    ("arduino", "An Arduino's own bootloader (Uno, Nano)"),
    ("wiring", "An Arduino Mega's bootloader"),
    ("usbasp", "USBasp"),
    ("usbtiny", "USBtinyISP"),
    ("avrisp2", "AVRISP mkII"),
    ("stk500v2", "STK500 version 2"),
    ("atmelice_isp", "Atmel-ICE"),
    ("serialupdi", "UPDI over a serial adapter (newer ATtiny and ATmega)"),
    ("jtag2updi", "jtag2updi"),
];

/// MPLAB IPE's tools, as ipecmd names them.
pub const PIC_TOOLS: [(&str, &str); 5] = [("PK3", "PICkit 3"), ("PK4", "PICkit 4"), ("PK5", "PICkit 5"), ("PPK4", "MPLAB PICkit 4 (in PIC mode)"), ("ICD4", "MPLAB ICD 4")];

impl Flashing {
    pub fn to_settings(&self) -> String {
        format!(
            "avr_programmer={}\navr_part={}\navr_port={}\npic_tool={}\npic_part={}\n",
            self.avr_programmer, self.avr_part, self.avr_port, self.pic_tool, self.pic_part
        )
    }

    pub fn read_setting(&mut self, name: &str, value: &str) {
        let value = value.trim().to_string();
        match name {
            "avr_programmer" => self.avr_programmer = value,
            "avr_part" => self.avr_part = value,
            "avr_port" => self.avr_port = value,
            "pic_tool" => self.pic_tool = value,
            "pic_part" => self.pic_part = value,
            _ => {}
        }
    }

    /// The tasks that flash what's been built in a folder: an Intel HEX file (the newest, in it or in build/),
    /// sent with avrdude where there's AVR code and with ipecmd where there's PIC code.
    pub fn tasks(&self, folder: &Path, languages: &[String]) -> Vec<Task> {
        let Some(hex) = newest_hex(folder) else { return Vec::new() };
        let shown = hex.strip_prefix(folder).unwrap_or(&hex).display().to_string();
        let quoted = if shown.contains(' ') { format!("\"{shown}\"") } else { shown.clone() };
        let mut found = Vec::new();
        let has = |words: &[&str]| languages.iter().any(|l| words.iter().any(|w| l.contains(w)));
        if has(&["AVR", "Arduino"]) {
            let port = if self.avr_port.trim().is_empty() { String::new() } else { format!(" -P {}", self.avr_port.trim()) };
            found.push(task(&format!("avrdude: flash {shown}"), &format!("avrdude -c {} -p {}{port} -U flash:w:{quoted}:i", self.avr_programmer, self.avr_part), Kind::Run));
        }
        if has(&["PIC"]) {
            found.push(task(&format!("ipecmd: flash {shown}"), &format!("ipecmd -TP{} -P{} -F{quoted} -M -OL", self.pic_tool, self.pic_part), Kind::Run));
        }
        found
    }
}

/// The newest .hex file in a folder or its build folders.
fn newest_hex(folder: &Path) -> Option<PathBuf> {
    let mut found: Vec<(std::time::SystemTime, PathBuf)> = Vec::new();
    for place in [folder.to_path_buf(), folder.join("build"), folder.join("Debug"), folder.join("Release")] {
        let Ok(entries) = std::fs::read_dir(&place) else { continue };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().is_some_and(|e| e.eq_ignore_ascii_case("hex")) {
                let when = entry.metadata().and_then(|m| m.modified()).unwrap_or(std::time::UNIX_EPOCH);
                found.push((when, path));
            }
        }
    }
    found.into_iter().max_by_key(|(when, _)| *when).map(|(_, path)| path)
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
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct TestResult {
    pub name: String,
    pub passed: bool,
    /// Where it failed, if the output says.
    pub place: Option<(String, usize)>,
    /// Said beside it, such as the cycles an assembly routine took.
    pub note: Option<String>,
    /// What went wrong, a line each, shown under it.
    pub detail: Vec<String>,
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
                "ok" => results.push(TestResult { name: name.into(), passed: true, place: None, ..Default::default() }),
                "FAILED" => results.push(TestResult { name: name.into(), passed: false, place: None, ..Default::default() }),
                _ => {}
            }
            continue;
        }
        // go: "--- PASS: TestName (0.00s)".
        if let Some(rest) = t.strip_prefix("--- PASS: ").or_else(|| t.strip_prefix("--- FAIL: ")) {
            let name = rest.split_whitespace().next().unwrap_or("").to_string();
            results.push(TestResult { name, passed: t.starts_with("--- PASS"), place: None, ..Default::default() });
            continue;
        }
        // pytest -v: "tests/test_x.py::test_name PASSED [ 50%]".
        if let Some(at) = t.find(" PASSED").or_else(|| t.find(" FAILED")).or_else(|| t.find(" ERROR"))
            && t[..at].contains("::")
            && !t.starts_with("FAILED ")
        {
            results.push(TestResult { name: t[..at].to_string(), passed: t[at..].starts_with(" PASSED"), place: None, ..Default::default() });
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
    crate::test_output::more_results(lines, &mut results);
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
    fn microcontrollers_are_flashed_with_what_was_built() {
        let folder = std::env::temp_dir().join(format!("limoniter-flash-{}", std::process::id()));
        std::fs::create_dir_all(folder.join("build")).unwrap();
        std::fs::write(folder.join("platformio.ini"), "[env:uno]\n").unwrap();
        assert!(detect(&folder).iter().any(|t| t.command == "pio run -t upload"));
        let flashing = Flashing { avr_port: "COM3".into(), ..Flashing::default() };
        assert!(flashing.tasks(&folder, &["Assembly AVR".into()]).is_empty(), "nothing built yet");
        std::fs::write(folder.join("build/blink.hex"), ":00000001FF\n").unwrap();
        let tasks = flashing.tasks(&folder, &["Assembly AVR".into()]);
        assert_eq!(tasks[0].command, format!("avrdude -c arduino -p atmega328p -P COM3 -U flash:w:{}:i", std::path::Path::new("build").join("blink.hex").display()));
        let pic = Flashing { pic_part: "16F84A".into(), ..Flashing::default() }.tasks(&folder, &["Assembly PIC".into()]);
        assert!(pic.iter().any(|t| t.command.starts_with("ipecmd -TPPK4 -P16F84A -F")));
        let mut read = Flashing::default();
        for line in flashing.to_settings().lines() {
            let (name, value) = line.split_once('=').unwrap();
            read.read_setting(name, value);
        }
        assert_eq!(read, flashing);
        let _ = std::fs::remove_dir_all(&folder);
    }

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
        assert_eq!(cargo, [TestResult { name: "tests::adds".into(), passed: true, place: None, ..Default::default() }, TestResult { name: "tests::subtracts".into(), passed: false, place: Some(("src/lib.rs".into(), 20)), detail: vec!["assertion failed".into()], ..Default::default() }]);
        let pytest = test_results(&lines("test_maths.py::test_add PASSED  [ 50%]\ntest_maths.py::test_sub FAILED  [100%]\n=== FAILURES ===\n____ test_sub ____\n    def test_sub():\n>       assert 1 - 1 == 1\nE       assert 0 == 1\n\ntest_maths.py:6: AssertionError\n"));
        assert_eq!(pytest.len(), 2);
        assert_eq!(pytest[1].place, Some(("test_maths.py".into(), 6)));
        let go = test_results(&lines("=== RUN   TestAdd\n--- PASS: TestAdd (0.00s)\n=== RUN   TestSub\n    maths_test.go:14: got 0, want 1\n--- FAIL: TestSub (0.00s)\nFAIL\n"));
        assert_eq!(go[1], TestResult { name: "TestSub".into(), passed: false, place: Some(("maths_test.go".into(), 14)), note: Some("0.00 s".into()), detail: vec!["got 0, want 1".into()] });
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
        std::fs::write(folder.join("composer.json"), "{\"scripts\": {\"test\": \"phpunit\", \"lint\": \"phpcs\", \"post-install-cmd\": \"x\"}}").unwrap();
        let tasks = detect(&folder);
        let composer: Vec<(&str, &str)> = tasks.iter().filter(|t| t.name.starts_with("composer")).map(|t| (t.name.as_str(), t.command.as_str())).collect();
        assert_eq!(composer, [("composer install", "composer install"), ("composer update", "composer update"), ("composer dump-autoload", "composer dump-autoload"), ("composer test", "composer run-script test"), ("composer lint", "composer run-script lint")]);
        assert_eq!(tasks.iter().find(|t| t.name == "composer test").map(|t| t.kind), Some(Kind::Test));
        let _ = std::fs::remove_dir_all(folder);
    }

    /// package.json's scripts run with the manager the project's lock file names.
    #[test]
    fn scripts_are_run_with_the_projects_package_manager() {
        let folder = std::env::temp_dir().join(format!("limoniter-tasks-node-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&folder);
        std::fs::create_dir_all(&folder).unwrap();
        std::fs::write(folder.join("package.json"), "{\"scripts\": {\"dev\": \"vite\", \"test\": \"vitest run\"}}").unwrap();
        for (lock, manager) in [("pnpm-lock.yaml", "pnpm"), ("yarn.lock", "yarn"), ("bun.lockb", "bun"), ("package-lock.json", "npm")] {
            for old in ["pnpm-lock.yaml", "yarn.lock", "bun.lockb", "package-lock.json"] {
                let _ = std::fs::remove_file(folder.join(old));
            }
            std::fs::write(folder.join(lock), "").unwrap();
            let tasks = detect(&folder);
            let dev = tasks.iter().find(|t| t.name == format!("{manager} run dev")).unwrap_or_else(|| panic!("{manager}: {tasks:?}"));
            assert_eq!((dev.command.as_str(), dev.kind), (format!("{manager} run dev").as_str(), Kind::Run));
            assert_eq!(tasks.iter().find(|t| t.name == format!("{manager} run test")).map(|t| t.kind), Some(Kind::Test));
            assert!(tasks.iter().any(|t| t.command == format!("{manager} install")), "installing the packages");
            assert!(!tasks.iter().any(|t| t.command.starts_with("npm run")) || manager == "npm");
        }
        let _ = std::fs::remove_dir_all(folder);
    }
}
