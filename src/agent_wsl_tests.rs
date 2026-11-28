//! Running the tools in WSL: paths translated both ways, the command that starts a tool there, finding the tools,
//! and whole sessions run through a stand-in for `wsl.exe` (src/agent_testdata/fake_wsl.py).

use super::*;
use crate::agent_session::{Answer, Item, PermissionKind, Session, Tool};
use serde_json::json;
use std::path::PathBuf;
use std::time::Duration;

fn ubuntu() -> Paths {
    Paths::new("Ubuntu", Path::new("C:\\Users\\Ann Lee\\code\\proj"))
}

// ---------- Paths ----------

#[test]
fn one_path_is_translated_each_way_for_drives_spaces_and_wsl_folders() {
    let paths = ubuntu();
    assert_eq!(paths.linux_folder(), "/mnt/c/Users/Ann Lee/code/proj");
    assert_eq!(paths.path_to_linux("C:\\Users\\Ann Lee\\code\\proj\\src\\a.rs"), "/mnt/c/Users/Ann Lee/code/proj/src/a.rs");
    assert_eq!(paths.path_to_linux("c:\\users\\ann lee\\code\\proj\\a.rs"), "/mnt/c/Users/Ann Lee/code/proj/a.rs", "the folder is known whatever its case, as Windows has it");
    assert_eq!(paths.path_to_linux("D:/data/x.txt"), "/mnt/d/data/x.txt");
    assert_eq!(paths.path_to_linux("\\\\wsl.localhost\\Ubuntu\\home\\ann\\x"), "/home/ann/x");
    assert_eq!(paths.path_to_linux("/home/ann/x"), "/home/ann/x", "already WSL's");
    assert_eq!(paths.path_to_windows("/mnt/c/Users/Ann Lee/code/proj/src/a.rs"), "C:\\Users\\Ann Lee\\code\\proj\\src\\a.rs");
    assert_eq!(paths.path_to_windows("/mnt/d/data/x.txt"), "D:\\data\\x.txt");
    assert_eq!(paths.path_to_windows("/home/ann/x"), "\\\\wsl.localhost\\Ubuntu\\home\\ann\\x");
    assert_eq!(paths.path_to_windows("C:\\already"), "C:\\already");
    assert_eq!(paths.path_to_windows("/mnt/c/Users/Ann Lee/code/proj"), "C:\\Users\\Ann Lee\\code\\proj", "the folder itself");
}

#[test]
fn a_project_inside_wsl_is_its_own_path_and_the_other_files_are_found_by_unc() {
    let paths = Paths::new("Ubuntu", Path::new("\\\\wsl.localhost\\Ubuntu\\home\\ann\\proj"));
    assert_eq!(paths.linux_folder(), "/home/ann/proj");
    assert_eq!(paths.path_to_linux("\\\\wsl.localhost\\Ubuntu\\home\\ann\\proj\\src\\a.rs"), "/home/ann/proj/src/a.rs");
    assert_eq!(paths.path_to_windows("/home/ann/proj/src/a.rs"), "\\\\wsl.localhost\\Ubuntu\\home\\ann\\proj\\src\\a.rs");
    assert_eq!(paths.path_to_windows("/home/ann/other.txt"), "\\\\wsl.localhost\\Ubuntu\\home\\ann\\other.txt");
    assert_eq!(paths.path_to_linux("C:\\x\\y"), "/mnt/c/x/y");
    // The old spelling of the name, and a folder that only starts the same, are not the project.
    assert_eq!(paths.path_to_windows("/home/ann/project2/a"), "\\\\wsl.localhost\\Ubuntu\\home\\ann\\project2\\a");
}

#[test]
fn a_folder_that_is_already_wsls_or_linuxs_is_left_alone() {
    let paths = Paths::new("Ubuntu", Path::new("/home/ann/proj"));
    assert_eq!(paths.outgoing("{\"cwd\":\"/home/ann/proj\"}"), "{\"cwd\":\"/home/ann/proj\"}");
    assert_eq!(paths.incoming("{\"path\":\"/mnt/c/x\"}"), "{\"path\":\"/mnt/c/x\"}");
}

#[test]
fn text_has_its_paths_translated_and_only_its_paths() {
    let paths = ubuntu();
    assert_eq!(paths.text_to_linux("Please edit C:\\Users\\Ann Lee\\code\\proj\\src\\a.rs, then D:\\x\\y.txt."), "Please edit /mnt/c/Users/Ann Lee/code/proj/src/a.rs, then /mnt/d/x/y.txt.");
    assert_eq!(paths.text_to_linux("(see \"C:\\x\\a b.txt\")"), "(see \"/mnt/c/x/a b.txt\")", "a quotation mark ends a path with a space in it");
    assert_eq!(paths.text_to_linux("a mistake: XC:\\not and abc:/nor, 12:30 and C: alone"), "a mistake: XC:\\not and abc:/nor, 12:30 and C: alone");
    assert_eq!(paths.text_to_windows("Read /mnt/c/Users/Ann Lee/code/proj/src/a.rs and /mnt/d/x/y.txt."), "Read C:\\Users\\Ann Lee\\code\\proj\\src\\a.rs and D:\\x\\y.txt.");
    assert_eq!(paths.text_to_windows("cd /mnt/c/Users/Ann Lee/code/proj && ls /usr/bin /mnt/wsl/x a/mnt/c/x"), "cd C:\\Users\\Ann Lee\\code\\proj && ls /usr/bin /mnt/wsl/x a/mnt/c/x");
    assert_eq!(paths.text_to_windows("héllo /mnt/c/é ünï"), "héllo C:\\é ünï");
}

#[test]
fn json_lines_have_their_values_and_keys_translated_and_whole_paths_keep_their_spaces() {
    let paths = ubuntu();
    let line = json!({"cwd": "C:\\Users\\Ann Lee\\code\\proj", "text": "edit C:\\Users\\Ann Lee\\code\\proj\\a.rs", "n": 3, "list": ["D:\\x"], "file_path": "E:\\my files\\a b.txt"}).to_string();
    let sent: Value = serde_json::from_str(&paths.outgoing(&line)).unwrap();
    assert_eq!(sent["cwd"], "/mnt/c/Users/Ann Lee/code/proj");
    assert_eq!(sent["text"], "edit /mnt/c/Users/Ann Lee/code/proj/a.rs");
    assert_eq!(sent["list"][0], "/mnt/d/x");
    assert_eq!(sent["file_path"], "/mnt/e/my files/a b.txt");
    assert_eq!(sent["n"], 3);
    let back: Value = serde_json::from_str(&paths.incoming(&json!({"changes": [{"path": "/mnt/e/my files/a b.txt"}, {"path": "/home/ann/x"}], "/mnt/d/k": 1, "file_path": "/mnt/c/Users/Ann Lee/code/proj/a.rs"}).to_string())).unwrap();
    assert_eq!(back["changes"][0]["path"], "E:\\my files\\a b.txt");
    assert_eq!(back["changes"][1]["path"], "\\\\wsl.localhost\\Ubuntu\\home\\ann\\x");
    assert_eq!(back["D:\\k"], 1);
    assert_eq!(back["file_path"], "C:\\Users\\Ann Lee\\code\\proj\\a.rs");
    assert_eq!(paths.incoming("not json /mnt/c/x"), "not json /mnt/c/x");
}

#[test]
fn what_is_sent_comes_back_the_same() {
    let paths = ubuntu();
    for path in ["C:\\Users\\Ann Lee\\code\\proj\\src\\a.rs", "D:\\x\\y.txt", "\\\\wsl.localhost\\Ubuntu\\home\\ann\\x"] {
        assert_eq!(paths.path_to_windows(&paths.path_to_linux(path)), path);
    }
}

// ---------- The command ----------

#[test]
fn the_command_runs_the_tool_through_a_login_shell_in_the_folder() {
    let (program, prefix) = runner(&("wsl.exe".to_string(), Vec::new()), "Ubuntu", "/mnt/c/a b", "/home/ann/.nvm/bin/codex");
    assert_eq!(program, "wsl.exe");
    assert_eq!(prefix, ["-d", "Ubuntu", "--cd", "/mnt/c/a b", "--exec", "sh", "-lc", "exec \"$@\"", "sh", "/home/ann/.nvm/bin/codex"]);
    let (program, prefix) = runner(&("python".to_string(), vec!["fake.py".to_string()]), "D", "/x", "/t");
    assert_eq!((program.as_str(), prefix[0].as_str(), prefix[1].as_str()), ("python", "fake.py", "-d"));
    // With the tool's own arguments after, as `arguments` puts them, it is what the tool was given.
    let args = crate::agent_codex::arguments(&crate::agent_codex::Options { program, prefix, folder: "/x".into(), mode: crate::agent_session::Mode::Ask, model: None, effort: None, resume: None, mcp: None, wsl: None, wake: std::sync::Arc::new(|| {}) });
    assert_eq!(args.last().map(String::as_str), Some("app-server"));
    assert_eq!(args[args.len() - 2], "/t");
}

#[test]
fn the_place_chosen_is_the_remembered_one_if_it_is_still_there() {
    let here = Place { distribution: None, program: "codex".into() };
    let ubuntu = Place { distribution: Some("Ubuntu".into()), program: "/u/codex".into() };
    let debian = Place { distribution: Some("Debian".into()), program: "/d/codex".into() };
    let places = [here.clone(), ubuntu.clone(), debian.clone()];
    assert_eq!(choose(&places, Some("Debian")), Some(&debian));
    assert_eq!(choose(&places, Some("")), Some(&here), "empty is this computer's own");
    assert_eq!(choose(&places, Some("Gone")), Some(&here));
    assert_eq!(choose(&places[1..], None), Some(&ubuntu));
    assert_eq!(choose(&[], None), None);
    assert_eq!(ubuntu.describe("Codex"), "Codex, in WSL: Ubuntu");
    assert_eq!(here.describe("Codex"), "Codex, on Windows");
}

#[test]
fn what_is_remembered_is_kept_as_text() {
    let remembered = Remembered { claude: Some("Ubuntu".into()), codex: Some(String::new()) };
    assert_eq!(Remembered::from_text(&remembered.to_text()), remembered);
    assert_eq!(Remembered::from_text("junk\ncodex = Debian\n"), Remembered { claude: None, codex: Some("Debian".into()) });
}

#[test]
fn the_path_a_distribution_prints_is_taken_and_a_message_is_not() {
    assert_eq!(found_path("/home/ann/.nvm/versions/node/v22/bin/codex\n"), Some("/home/ann/.nvm/versions/node/v22/bin/codex".to_string()));
    assert_eq!(found_path("Welcome to nvm\r\n/usr/bin/claude\r\n"), Some("/usr/bin/claude".to_string()));
    assert_eq!(found_path("alias codex='x'\n"), None);
    assert_eq!(found_path(""), None);
}

// ---------- Against a stand-in for wsl.exe ----------

fn python() -> Option<String> {
    ["python3", "python"].iter().find(|name| crate::platform::command(name).arg("--version").output().is_ok_and(|out| out.status.success())).map(|name| name.to_string())
}

fn scratch(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("limoniter-agent-wsl-{}-{tag}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// The stand-in for `wsl.exe`, as the program and what goes before its arguments, with a distribution (Ubuntu) that
/// has `claude` and `codex` at made-up places, which run `runs`. Every command line it's given is kept in
/// `wsl.log` in `dir`.
pub(crate) fn fake_wsl(dir: &Path, runs: &[(&str, Vec<String>)]) -> Option<(String, Vec<String>)> {
    let Some(program) = python() else {
        eprintln!("No Python here, so the stand-in for wsl.exe can't run: this test did nothing.");
        return None;
    };
    let script = Path::new(env!("CARGO_MANIFEST_DIR")).join("src/agent_testdata/fake_wsl.py");
    let run: serde_json::Map<String, Value> = runs.iter().map(|(path, argv)| (path.to_string(), json!(argv))).collect();
    let config = json!({"distributions": ["Ubuntu", "Debian"], "tools": {"Ubuntu": {"claude": "/home/ann/.local/bin/claude", "codex": "/home/ann/.nvm/bin/codex"}, "Debian": {"claude": "/usr/bin/claude"}}, "run": run, "log": dir.join("wsl.log").display().to_string()});
    let file = dir.join("wsl.json");
    std::fs::write(&file, config.to_string()).unwrap();
    Some((program, vec![script.display().to_string(), "--config".to_string(), file.display().to_string()]))
}

/// The paths of a session in `dir`: Windows' own on Windows, and a made-up Windows folder that stands for `dir`
/// anywhere else.
pub(crate) fn paths_for(dir: &Path) -> (Paths, PathBuf) {
    if cfg!(windows) {
        (Paths::new("Ubuntu", dir), dir.to_path_buf())
    } else {
        (Paths::with_folders("Ubuntu", "C:\\ws\\proj", &dir.display().to_string()), PathBuf::from("C:\\ws\\proj"))
    }
}

fn wait(session: &mut Session, what: &str, done: impl Fn(&Session) -> bool) {
    let began = std::time::Instant::now();
    while !done(session) {
        session.pump();
        assert!(began.elapsed() < Duration::from_secs(20), "waited for {what}; the transcript is {:?}", session.items);
        std::thread::sleep(Duration::from_millis(10));
    }
}

fn logged(dir: &Path, name: &str) -> Vec<Value> {
    std::fs::read_to_string(dir.join(name)).unwrap_or_default().lines().filter_map(|l| serde_json::from_str(l).ok()).collect()
}

#[test]
fn the_tools_are_found_in_each_distribution_that_has_them() {
    let dir = scratch("find");
    let Some(wsl) = fake_wsl(&dir, &[]) else { return };
    let names = ["Ubuntu".to_string(), "Debian".to_string(), "Gone".to_string()];
    let codex = find_places(&wsl, &names, "codex");
    assert_eq!(codex, vec![Place { distribution: Some("Ubuntu".into()), program: "/home/ann/.nvm/bin/codex".into() }]);
    let claude = find_places(&wsl, &names, "claude");
    assert_eq!(claude.iter().map(|p| (p.distribution.clone().unwrap(), p.program.clone())).collect::<Vec<_>>(), vec![("Ubuntu".to_string(), "/home/ann/.local/bin/claude".to_string()), ("Debian".to_string(), "/usr/bin/claude".to_string())]);
    assert_eq!(find_in(&wsl, "Ubuntu", "nothing-like-this"), None);
    // It was asked through a login shell, with no window of its own.
    let asked = logged(&dir, "wsl.log");
    assert!(asked.iter().any(|line| line == &json!(["-d", "Ubuntu", "--exec", "sh", "-lc", "command -v codex"])), "{asked:?}");
}

#[test]
fn claude_in_wsl_is_told_wsl_paths_and_the_person_sees_windows_ones() {
    let dir = scratch("claude");
    let Some(options) = crate::agent_claude::agent_claude_tests::fake_options(&dir, &[]) else { return };
    let Some(wsl) = fake_wsl(&dir, &[("/home/ann/.local/bin/claude", std::iter::once(options.program.clone()).chain(options.prefix.clone()).collect())]) else { return };
    let (paths, windows_folder) = paths_for(&dir);
    let (program, prefix) = runner(&wsl, "Ubuntu", paths.linux_folder(), "/home/ann/.local/bin/claude");
    let options = crate::agent_claude::Options { program, prefix, folder: windows_folder.clone(), wsl: Some(paths.clone()), ..options };
    let mut session = Session::new(Tool::Claude, &windows_folder, crate::agent_claude::start(options).unwrap());
    wait(&mut session, "the models", |s| !s.models.is_empty());
    let file = windows_folder.join("new.txt");
    let shown = if cfg!(windows) { file.display().to_string() } else { "C:\\ws\\proj\\new.txt".to_string() };
    session.say(&format!("write {shown}"));
    wait(&mut session, "the request", |s| !s.waiting().is_empty());
    let request = session.waiting()[0].clone();
    let PermissionKind::Edit { path, proposed } = &request.kind else { panic!("{request:?}") };
    assert_eq!(path.display().to_string(), shown, "the person is shown the Windows path");
    assert_eq!(proposed, "written\n");
    assert!(!dir.join("new.txt").exists());
    session.answer(&request.id, Answer::Allow { text: None, for_session: false });
    wait(&mut session, "the file", |s| !s.busy && dir.join("new.txt").exists());
    assert_eq!(std::fs::read_to_string(dir.join("new.txt")).unwrap(), "written\n");
    // What the tool was sent had WSL's paths, and the stand-in was started in the folder in WSL's form.
    let sent = std::fs::read_to_string(dir.join("log.txt")).unwrap();
    assert!(sent.contains(&format!("write {}", paths.path_to_linux(&shown))), "{sent}");
    let started = logged(&dir, "wsl.log");
    let line = started.last().unwrap().as_array().unwrap().iter().map(|v| v.as_str().unwrap().to_string()).collect::<Vec<_>>();
    assert_eq!(&line[..4], ["-d", "Ubuntu", "--cd", paths.linux_folder()]);
    assert_eq!(&line[4..10], ["--exec", "sh", "-lc", "exec \"$@\"", "sh", "/home/ann/.local/bin/claude"]);
    assert!(line.contains(&"stream-json".to_string()));
}

#[test]
fn codex_in_wsl_asks_about_a_change_in_windows_paths_and_makes_it() {
    let dir = scratch("codex");
    let Some(options) = crate::agent_codex::agent_codex_tests::fake_options(&dir, &[]) else { return };
    let Some(wsl) = fake_wsl(&dir, &[("/home/ann/.nvm/bin/codex", std::iter::once(options.program.clone()).chain(options.prefix.clone()).collect())]) else { return };
    let (paths, windows_folder) = paths_for(&dir);
    let (program, prefix) = runner(&wsl, "Ubuntu", paths.linux_folder(), "/home/ann/.nvm/bin/codex");
    let options = crate::agent_codex::Options { program, prefix, folder: windows_folder.clone(), wsl: Some(paths.clone()), ..options };
    let mut session = Session::new(Tool::Codex, &windows_folder, crate::agent_codex::start(options).unwrap());
    wait(&mut session, "the models", |s| !s.models.is_empty());
    let shown = if cfg!(windows) { dir.join("new.txt").display().to_string() } else { "C:\\ws\\proj\\new.txt".to_string() };
    session.say(&format!("add {shown}"));
    wait(&mut session, "the request", |s| !s.waiting().is_empty());
    let request = session.waiting()[0].clone();
    let PermissionKind::Edit { path, proposed } = &request.kind else { panic!("{request:?}") };
    assert_eq!(path.display().to_string(), shown, "the person is shown the Windows path");
    assert_eq!(proposed, "hello\n");
    session.answer(&request.id, Answer::Allow { text: None, for_session: false });
    wait(&mut session, "the file", |s| !s.busy && dir.join("new.txt").exists());
    assert_eq!(std::fs::read_to_string(dir.join("new.txt")).unwrap().replace("\r\n", "\n"), "hello\n");
    // The thread was started in the folder in WSL's form, and the stand-in was told paths in WSL's form.
    let said = std::fs::read_to_string(dir.join("log.txt")).unwrap();
    assert!(said.contains(&serde_json::to_string(paths.linux_folder()).unwrap()), "{said}");
    assert!(!said.contains("ws\\\\proj"), "no Windows path went to the tool: {said}");
    assert!(session.items.iter().any(|item| matches!(item, Item::Permission { .. })));
}

#[test]
fn earlier_sessions_kept_in_a_distribution_are_listed_and_marked() {
    let root = scratch("past");
    let (paths, _) = (Paths::with_folders("Ubuntu", "C:\\ws\\proj", "/mnt/c/ws/proj"), ());
    let logs = root.join("home/ann/.claude/projects/-mnt-c-ws-proj");
    std::fs::create_dir_all(&logs).unwrap();
    std::fs::write(logs.join("s1.jsonl"), "{\"type\":\"user\",\"message\":{\"role\":\"user\",\"content\":\"tidy the readme\"}}\n").unwrap();
    let other = root.join("home/ann/.claude/projects/-mnt-c-else");
    std::fs::create_dir_all(&other).unwrap();
    std::fs::write(other.join("s2.jsonl"), "{\"type\":\"user\",\"message\":{\"role\":\"user\",\"content\":\"elsewhere\"}}\n").unwrap();
    let found = past_in(&root, "Ubuntu", &paths);
    assert_eq!(found.len(), 1, "{found:?}");
    assert_eq!((found[0].id.as_str(), found[0].wsl.as_deref(), found[0].tool), ("s1", Some("Ubuntu"), Tool::Claude));
    assert!(past_in(&root.join("nothing"), "Ubuntu", &paths).is_empty());
}
