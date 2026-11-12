//! The few things done differently on Windows, macOS and Linux: running a command line, stopping it and what it
//! started, and handing a file or folder to the system's own file manager or usual program.

use std::path::Path;
use std::process::Command;

/// A command line, run as the system's shell runs one: cmd on Windows, sh elsewhere. No window of its own.
pub fn shell_command(line: &str) -> Command {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        let mut command = Command::new("cmd");
        // /S keeps the line exactly as typed, quotes and all.
        command.raw_arg(format!("/D /S /C \"{line}\"")).creation_flags(0x0800_0000);
        command
    }
    #[cfg(not(windows))]
    {
        use std::os::unix::process::CommandExt;
        let mut command = Command::new("sh");
        command.arg("-c").arg(line);
        // A group of its own, so that stopping it stops whatever it started too.
        command.process_group(0);
        command
    }
}

/// Ends a process and everything it started.
pub fn kill_tree(process: u32) {
    #[cfg(windows)]
    let mut command = {
        use std::os::windows::process::CommandExt;
        let mut command = Command::new("taskkill");
        command.args(["/T", "/F", "/PID", &process.to_string()]).creation_flags(0x0800_0000);
        command
    };
    #[cfg(not(windows))]
    let mut command = {
        let mut command = Command::new("kill");
        command.args(["-KILL", "--", &format!("-{process}")]);
        command
    };
    let _ = command.stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null()).status();
}

/// What the system's file manager is called, for menus and messages.
pub const FILE_MANAGER: &str = if cfg!(windows) {
    "Explorer"
} else if cfg!(target_os = "macos") {
    "Finder"
} else {
    "the file manager"
};

/// The menu item that shows the file in the file manager.
pub const SHOW_IN_FILE_MANAGER: &str = if cfg!(windows) {
    "Show in Explorer"
} else if cfg!(target_os = "macos") {
    "Show in Finder"
} else {
    "Show in the file manager"
};

/// Where a kept password goes.
pub const CREDENTIAL_STORE: &str = if cfg!(windows) {
    "Windows Credential Manager"
} else if cfg!(target_os = "macos") {
    "the macOS keychain"
} else {
    "the system's keyring"
};

/// What runs a command line, in words.
pub const SHELL_NAME: &str = if cfg!(windows) { "the Command Prompt" } else { "the shell (sh)" };

/// Opens a folder in the file manager.
pub fn open_folder(folder: &Path) -> Result<(), String> {
    let program = if cfg!(windows) {
        "explorer"
    } else if cfg!(target_os = "macos") {
        "open"
    } else {
        "xdg-open"
    };
    Command::new(program).arg(folder).spawn().map(|_| ()).map_err(|e| format!("Couldn't open {FILE_MANAGER}.\n\n{e}"))
}

/// Shows a file in the file manager, selected where it can be.
pub fn reveal(file: &Path) -> Result<(), String> {
    #[cfg(windows)]
    let mut command = {
        use std::os::windows::process::CommandExt;
        let mut command = Command::new("explorer");
        command.raw_arg(format!("/select,\"{}\"", file.display()));
        command
    };
    #[cfg(target_os = "macos")]
    let mut command = {
        let mut command = Command::new("open");
        command.arg("-R").arg(file);
        command
    };
    #[cfg(not(any(windows, target_os = "macos")))]
    let mut command = {
        // Most Linux file managers can't be asked to select a file, so its folder is opened.
        let mut command = Command::new("xdg-open");
        command.arg(file.parent().unwrap_or(file));
        command
    };
    command.spawn().map(|_| ()).map_err(|e| format!("Couldn't open {FILE_MANAGER}.\n\n{e}"))
}

/// Opens a web page in the system's browser.
pub fn open_web_page(address: &str) -> Result<(), String> {
    // A test never opens a browser.
    if cfg!(test) {
        return Ok(());
    }
    #[cfg(windows)]
    let mut command = {
        use std::os::windows::process::CommandExt;
        let mut command = Command::new("rundll32");
        command.args(["url.dll,FileProtocolHandler", address]).creation_flags(0x0800_0000);
        command
    };
    #[cfg(target_os = "macos")]
    let mut command = {
        let mut command = Command::new("open");
        command.arg(address);
        command
    };
    #[cfg(not(any(windows, target_os = "macos")))]
    let mut command = {
        let mut command = Command::new("xdg-open");
        command.arg(address);
        command
    };
    command.spawn().map(|_| ()).map_err(|e| format!("Couldn't open the browser.\n\n{e}"))
}

/// Opens a file in the program the system uses for its kind.
#[cfg(not(windows))]
pub fn open_with_default(file: &Path) -> Result<(), String> {
    let program = if cfg!(target_os = "macos") { "open" } else { "xdg-open" };
    let status = Command::new(program).arg(file).status().map_err(|e| format!("Couldn't ask the system to open it.\n\n{e}"))?;
    if status.success() { Ok(()) } else { Err("The system has no program set to open this kind of file.".into()) }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_command_line_runs_in_the_systems_shell() {
        let output = shell_command("echo one && echo two").output().unwrap();
        let text = String::from_utf8_lossy(&output.stdout).replace('\r', "");
        assert_eq!(text.split_whitespace().collect::<Vec<_>>(), ["one", "two"]);
    }

    #[test]
    fn stopping_a_command_stops_what_it_started() {
        let line = if cfg!(windows) { "ping -n 30 127.0.0.1" } else { "sleep 30; sleep 30" };
        let mut child = shell_command(line).stdout(std::process::Stdio::null()).spawn().unwrap();
        let began = std::time::Instant::now();
        kill_tree(child.id());
        child.wait().unwrap();
        assert!(began.elapsed() < std::time::Duration::from_secs(10));
    }
}
