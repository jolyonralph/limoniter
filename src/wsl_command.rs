//! The `limoniter` command for a WSL distribution. Windows Limoniter can't be started by name from inside WSL, so
//! a small shell script is put in the distribution's `~/.local/bin` (no administrator rights needed). It turns paths
//! into Windows' form and runs `limoniter.exe`, which hands the files to the window that's open.
//!
//! Everything is done by running `sh` in the distribution through `wsl.exe`, with the commands sent on its standard
//! input so that no quoting has to survive Windows and WSL splitting the command line.

use std::io::Write;
use std::sync::mpsc::Sender;

/// The script, with `__EXE__` and `__COMMENT__` to be filled in with where Limoniter is.
const TEMPLATE: &str = include_str!("../data/limoniter-wsl.sh");

/// The line that ends the script when it's sent to the shell, which the script itself never contains.
const END: &str = "LIMONITER_SCRIPT_END";

/// What a distribution has for `limoniter`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Status {
    /// Nothing called that.
    None,
    /// Something that isn't this program's script: left alone.
    Other,
    /// Limoniter's own script, written for the program at this path (as Windows writes it).
    Mine(String),
}

/// What asking a distribution found out.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Found {
    pub status: Status,
    /// Whether `~/.local/bin` is on the distribution's PATH, where commands are found.
    pub on_path: bool,
}

/// The script for the program at `exe`, as Windows writes its path.
pub fn script(exe: &str) -> String {
    let exe: String = exe.chars().filter(|c| *c != '\n' && *c != '\r').collect();
    TEMPLATE.replace("__COMMENT__", &exe).replace("__EXE__", &exe.replace('\'', "'\\''"))
}

/// The commands that say what's there, and print `mine` (then the program's path), `other` or `none`, and `onpath`
/// if `~/.local/bin` is on the PATH a login shell has.
pub fn check_commands() -> String {
    include_str!("../data/wsl-check.sh").to_string()
}

/// What the check printed, read.
pub fn found_in(output: &str) -> Found {
    let mut lines = output.lines().map(str::trim_end);
    let status = match lines.next() {
        Some("mine") => Status::Mine(lines.next().filter(|line| *line != "onpath").unwrap_or_default().to_string()),
        Some("other") => Status::Other,
        _ => Status::None,
    };
    Found { status, on_path: output.lines().any(|line| line.trim_end() == "onpath") }
}

/// The commands that put the script in place, for the program at `exe`.
pub fn install_commands(exe: &str) -> String {
    format!("mkdir -p \"$HOME/.local/bin\" && cat > \"$HOME/.local/bin/limoniter\" <<'{END}'\n{}{END}\nchmod +x \"$HOME/.local/bin/limoniter\" && echo done\n", with_final_newline(&script(exe)))
}

/// The commands that take it away again, only if it's Limoniter's own.
pub fn remove_commands() -> String {
    include_str!("../data/wsl-remove.sh").to_string()
}

fn with_final_newline(text: &str) -> String {
    if text.ends_with('\n') { text.to_string() } else { format!("{text}\n") }
}

/// The arguments to `wsl.exe` that run a login shell in a distribution, reading its commands from standard input.
pub fn wsl_arguments(distribution: &str) -> Vec<String> {
    ["-d", distribution, "--", "sh", "-l"].map(String::from).to_vec()
}

/// Runs commands in a distribution, with no window, and gives what they printed.
fn run_in(distribution: &str, commands: &str) -> Result<String, String> {
    let mut child = crate::platform::command("wsl.exe")
        .args(wsl_arguments(distribution))
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .spawn()
        .map_err(|problem| crate::i18n::tf!("WSL couldn't be started: {problem}"))?;
    if let Some(mut input) = child.stdin.take() {
        let _ = input.write_all(commands.as_bytes());
    }
    let output = child.wait_with_output().map_err(|problem| crate::i18n::tf!("WSL didn't answer: {problem}"))?;
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

/// What happened in the background.
pub enum Done {
    Checked(String, Found),
    Installed(String, Result<(), String>),
    Removed(String, Result<(), String>),
}

/// Looks in a distribution, on a thread of its own. A script written for a program that has since moved is put
/// right quietly. `exe` is where this program is, as Windows writes it.
pub fn check_in_background(distribution: String, exe: String, tell: Sender<Done>, wake: impl Fn() + Send + 'static) {
    std::thread::spawn(move || {
        if let Ok(output) = run_in(&distribution, &check_commands()) {
            let mut found = found_in(&output);
            if let Status::Mine(old) = &found.status
                && *old != exe
                && run_in(&distribution, &install_commands(&exe)).is_ok_and(|said| said.trim() == "done")
            {
                found.status = Status::Mine(exe);
            }
            let _ = tell.send(Done::Checked(distribution, found));
            wake();
        }
    });
}

pub fn install_in_background(distribution: String, exe: String, tell: Sender<Done>, wake: impl Fn() + Send + 'static) {
    std::thread::spawn(move || {
        let result = run_in(&distribution, &install_commands(&exe)).and_then(|said| if said.trim() == "done" { Ok(()) } else { Err(crate::i18n::tr("The command couldn't be put in place.").to_string()) });
        let _ = tell.send(Done::Installed(distribution, result));
        wake();
    });
}

pub fn remove_in_background(distribution: String, tell: Sender<Done>, wake: impl Fn() + Send + 'static) {
    std::thread::spawn(move || {
        let result = run_in(&distribution, &remove_commands()).map(|_| ());
        let _ = tell.send(Done::Removed(distribution, result));
        wake();
    });
}

#[cfg(unix)]
#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;
    use std::path::{Path, PathBuf};
    use std::process::Stdio;

    fn folder(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("limoniter-wsl-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn executable(path: &Path, text: &str) {
        std::fs::write(path, text).unwrap();
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755)).unwrap();
    }

    /// Runs commands under `sh -l` the way `wsl.exe -d X -- sh -l` would, with a home folder of its own.
    fn run_commands(home: &Path, commands: &str) -> String {
        #[allow(clippy::disallowed_methods)]
        let mut child = std::process::Command::new("sh").arg("-l").env("HOME", home).stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::null()).spawn().unwrap();
        child.stdin.take().unwrap().write_all(commands.as_bytes()).unwrap();
        String::from_utf8_lossy(&child.wait_with_output().unwrap().stdout).into_owned()
    }

    #[test]
    fn the_script_has_the_programs_path_written_in() {
        let text = script(r"C:\Users\Jo's PC\limoniter.exe");
        assert!(text.contains(r"exe_windows='C:\Users\Jo'\''s PC\limoniter.exe'"), "a quote in the path can't break out");
        assert!(text.contains(r"# exe: C:\Users\Jo's PC\limoniter.exe"));
        assert!(!text.contains("__EXE__") && !text.contains("__COMMENT__"));
        assert!(!text.contains(END));
    }

    #[test]
    fn the_command_line_for_a_distribution_has_no_quoting_to_survive() {
        assert_eq!(wsl_arguments("Ubuntu 22.04"), ["-d", "Ubuntu 22.04", "--", "sh", "-l"]);
    }

    #[test]
    fn installing_checking_and_removing_work_in_a_home_folder() {
        let home = folder("home");
        let before = found_in(&run_commands(&home, &check_commands()));
        assert_eq!(before.status, Status::None);
        assert!(!before.on_path, "nothing says it's on the PATH yet");
        assert_eq!(run_commands(&home, &install_commands(r"C:\a\limoniter.exe")).trim(), "done");
        let made = home.join(".local/bin/limoniter");
        assert!(made.is_file() && std::fs::metadata(&made).unwrap().permissions().mode() & 0o111 != 0, "it can be run");
        assert_eq!(std::fs::read_to_string(&made).unwrap(), with_final_newline(&script(r"C:\a\limoniter.exe")), "as written, nothing lost");
        let after = found_in(&run_commands(&home, &check_commands()));
        assert_eq!(after.status, Status::Mine(r"C:\a\limoniter.exe".into()));
        assert_eq!(run_commands(&home, &install_commands(r"D:\b\limoniter.exe")).trim(), "done");
        assert_eq!(found_in(&run_commands(&home, &check_commands())).status, Status::Mine(r"D:\b\limoniter.exe".into()), "a moved program is put right");
        run_commands(&home, &remove_commands());
        assert!(!made.exists());
        // Somebody else's command of that name is never taken away.
        executable(&made, "#!/bin/sh\necho theirs\n");
        run_commands(&home, &remove_commands());
        assert!(made.exists());
        let _ = std::fs::remove_dir_all(home);
    }

    #[test]
    fn what_the_check_printed_is_read() {
        assert_eq!(found_in("none\n"), Found { status: Status::None, on_path: false });
        assert_eq!(found_in("other\nonpath\n"), Found { status: Status::Other, on_path: true });
        assert_eq!(found_in("mine\nC:\\x\\limoniter.exe\nonpath\n"), Found { status: Status::Mine("C:\\x\\limoniter.exe".into()), on_path: true });
        assert_eq!(found_in(""), Found { status: Status::None, on_path: false });
    }

    /// A folder with a fake `wslpath` (as WSL's would turn paths: /home/a/x into \\wsl.localhost\Ubuntu\home\a\x)
    /// and a fake limoniter.exe that writes down what it was given.
    fn fakes(tag: &str) -> (PathBuf, PathBuf) {
        let dir = folder(tag);
        executable(
            &dir.join("wslpath"),
            r#"#!/bin/sh
mode=$1; shift
case $mode in
  -aw) p=$1; case $p in /*) ;; *) p=/home/a/$p;; esac
       if [ -n "$FAKE_PLAIN" ]; then printf '%s' "$p"; exit 0; fi
       printf '%s' "\\\\wsl.localhost\\Ubuntu$p" | sed 's|/|\\|g' ;;
  -u) printf '%s' "$FAKE_EXE" ;;
esac
"#,
        );
        let log = dir.join("log.txt");
        executable(&dir.join("limoniter.exe"), &format!("#!/bin/sh\nfor a in \"$@\"; do printf '%s\\n' \"$a\"; done > '{}'\nexit ${{FAKE_STATUS:-0}}\n", log.display()));
        (dir, log)
    }

    fn run_script(dir: &Path, args: &[&str], more: &[(&str, &str)]) -> (i32, String) {
        let script_path = dir.join("limoniter");
        std::fs::write(&script_path, script(r"C:\Program Files\Limoniter\limoniter.exe")).unwrap();
        #[allow(clippy::disallowed_methods)]
        let mut command = std::process::Command::new("sh");
        command.arg(&script_path).args(args).env("PATH", format!("{}:{}", dir.display(), std::env::var("PATH").unwrap_or_default())).env("FAKE_EXE", dir.join("limoniter.exe")).env_remove("LIMONITER_WINDOW");
        for (name, value) in more {
            command.env(name, value);
        }
        let output = command.stdin(Stdio::null()).output().unwrap();
        (output.status.code().unwrap_or(-1), String::from_utf8_lossy(&output.stderr).into_owned())
    }

    #[test]
    fn the_script_turns_paths_into_windows_and_keeps_options_and_places() {
        let (dir, log) = fakes("convert");
        let existing = dir.join("real:7");
        std::fs::write(&existing, "x").unwrap();
        let (code, _) = run_script(&dir, &["--wait", "--line", "9", "notes.txt", "/home/a/x.rs:12:3", "sftp://me@host/f", existing.to_str().unwrap(), "--", "-odd"], &[("LIMONITER_WSL_INTEROP", "yes")]);
        assert_eq!(code, 0);
        let given: Vec<String> = std::fs::read_to_string(&log).unwrap().lines().map(String::from).collect();
        assert_eq!(given[..3], ["--wait", "--line", "9"], "options are passed through, and --line's number with it");
        assert_eq!(given[3], r"\\wsl.localhost\Ubuntu\home\a\notes.txt");
        assert_eq!(given[4], r"\\wsl.localhost\Ubuntu\home\a\x.rs:12:3", "the place stays on the end");
        assert_eq!(given[5], "sftp://me@host/f", "a server's address isn't a path");
        assert!(given[6].ends_with(r"real:7"), "a file really called that isn't cut: {}", given[6]);
        assert_eq!(given[7], "--");
        assert_eq!(given[8], r"\\wsl.localhost\Ubuntu\home\a\.\-odd", "after -- a dash starts a name");
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn the_programs_exit_status_comes_back_and_a_missing_program_is_said() {
        let (dir, _) = fakes("status");
        let (code, _) = run_script(&dir, &["a.txt"], &[("LIMONITER_WSL_INTEROP", "yes"), ("FAKE_STATUS", "3")]);
        assert_eq!(code, 3);
        let (code, said) = run_script(&dir, &["a.txt"], &[("LIMONITER_WSL_INTEROP", "yes"), ("FAKE_EXE", "/no/such/limoniter.exe")]);
        assert_eq!(code, 1);
        assert!(said.contains("isn't where this command expects it"), "{said}");
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn with_interop_off_the_window_is_asked_over_the_network() {
        let (dir, _) = fakes("network");
        let window_folder = folder("network-window");
        let listening = crate::instance::listen(&window_folder, || {}, &[]).expect("it listens");
        let address = listening.address.clone();
        let (code, said) = run_script(&dir, &["--line", "4", "notes.txt", "/home/a/x.rs:12:3"], &[("LIMONITER_WSL_INTEROP", "no"), ("LIMONITER_WINDOW", &address), ("FAKE_PLAIN", "1")]);
        assert_eq!(code, 0, "{said}");
        let handed = listening.files.recv_timeout(std::time::Duration::from_secs(5)).unwrap();
        let places: Vec<_> = handed.files.iter().map(|f| (f.path.display().to_string(), f.line, f.column)).collect();
        assert_eq!(places, [("/home/a/notes.txt".to_string(), Some(4), None), ("/home/a/x.rs".to_string(), Some(12), Some(3))]);
        let (code, said) = run_script(&dir, &["--new-window", "a.txt"], &[("LIMONITER_WSL_INTEROP", "no")]);
        assert_eq!(code, 1);
        assert!(said.contains("interoperability"), "{said}");
        let (code, said) = run_script(&dir, &["a.txt"], &[("LIMONITER_WSL_INTEROP", "no"), ("LIMONITER_WINDOW", "1:nothing")]);
        assert_eq!(code, 1);
        assert!(said.contains("didn't answer"), "{said}");
        listening.stop();
        let _ = std::fs::remove_dir_all(dir);
        let _ = std::fs::remove_dir_all(window_folder);
    }
}
