//! Starting a kernel's program as jupyter_client does: free ports chosen, a connection file written for it, the
//! command from its kernelspec filled in, and the means to interrupt it (SIGINT to its process group on Unix,
//! an event it waits on in Windows) and to end it.

use super::link::KernelSpec;
use std::collections::VecDeque;
use std::io::{BufRead, BufReader};
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::process::{Child, Stdio};
use std::sync::{Arc, Mutex};

/// How many of the kernel's last lines of error output are kept, to say why it stopped.
const LAST_LINES: usize = 12;

/// The five ports and the key, as the connection file holds them.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ConnectionInfo {
    pub shell_port: u16,
    pub iopub_port: u16,
    pub stdin_port: u16,
    pub control_port: u16,
    pub hb_port: u16,
    pub key: String,
    pub kernel_name: String,
}

impl ConnectionInfo {
    /// Five ports nobody is using now, found by asking the system for them all at once (so they differ). They're
    /// held (shell, iopub, stdin, control, heartbeat) until the launcher lets them go for the kernel, the moment
    /// before starting it, so nothing else takes one meanwhile.
    pub fn new(key: &str, kernel_name: &str) -> Result<(ConnectionInfo, Vec<TcpListener>), String> {
        // Ports given to other kernels lately are passed over, since one of those may not have bound its own yet.
        static RECENT: Mutex<VecDeque<u16>> = Mutex::new(VecDeque::new());
        let mut recent = RECENT.lock().unwrap_or_else(|e| e.into_inner());
        let mut listeners = Vec::new();
        let mut ports = Vec::new();
        let mut tries = 0;
        while listeners.len() < 5 {
            let listener = TcpListener::bind("127.0.0.1:0").map_err(|e| e.to_string())?;
            let port = listener.local_addr().map_err(|e| e.to_string())?.port();
            tries += 1;
            if recent.contains(&port) && tries < 100 {
                continue;
            }
            ports.push(port);
            listeners.push(listener);
        }
        recent.extend(&ports);
        while recent.len() > 500 {
            recent.pop_front();
        }
        let info = ConnectionInfo { shell_port: ports[0], iopub_port: ports[1], stdin_port: ports[2], control_port: ports[3], hb_port: ports[4], key: key.to_string(), kernel_name: kernel_name.to_string() };
        Ok((info, listeners))
    }

    pub fn to_json(&self) -> serde_json::Value {
        serde_json::json!({
            "shell_port": self.shell_port,
            "iopub_port": self.iopub_port,
            "stdin_port": self.stdin_port,
            "control_port": self.control_port,
            "hb_port": self.hb_port,
            "ip": "127.0.0.1",
            "key": self.key,
            "transport": "tcp",
            "signature_scheme": "hmac-sha256",
            "kernel_name": self.kernel_name,
        })
    }

    pub fn address(port: u16) -> std::net::SocketAddr {
        std::net::SocketAddr::from(([127, 0, 0, 1], port))
    }
}

/// Writes the connection file where only this user can read it, since it holds the key.
pub fn write_connection_file(info: &ConnectionInfo, file: &Path) -> Result<(), String> {
    use std::io::Write;
    let folder = file.parent().unwrap_or(Path::new("."));
    let mut builder = std::fs::DirBuilder::new();
    builder.recursive(true);
    #[cfg(unix)]
    std::os::unix::fs::DirBuilderExt::mode(&mut builder, 0o700);
    builder.create(folder).map_err(|e| e.to_string())?;
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create(true).truncate(true);
    #[cfg(unix)]
    std::os::unix::fs::OpenOptionsExt::mode(&mut options, 0o600);
    let mut out = options.open(file).map_err(|e| e.to_string())?;
    out.write_all(serde_json::to_string_pretty(&info.to_json()).unwrap_or_default().as_bytes()).map_err(|e| e.to_string())
}

/// Where a kernel's connection file goes: a folder of Limoniter's own in the temporary folder.
pub fn connection_file(session: &str, generation: u64) -> PathBuf {
    std::env::temp_dir().join("limoniter-jupyter").join(format!("kernel-{session}-{generation}.json"))
}

/// `${NAME}` in a kernelspec's environment value, filled in from the environment as jupyter_client does;
/// a name that isn't set is left as written.
pub fn fill_environment(value: &str, env: &dyn Fn(&str) -> Option<String>) -> String {
    let mut out = String::new();
    let mut rest = value;
    while let Some(start) = rest.find("${") {
        out.push_str(&rest[..start]);
        let after = &rest[start + 2..];
        match after.find('}') {
            Some(end) if after[..end].chars().all(|c| c.is_ascii_alphanumeric() || c == '_') && !after[..end].is_empty() => {
                match env(&after[..end]) {
                    Some(v) => out.push_str(&v),
                    None => out.push_str(&rest[start..start + 3 + end]),
                }
                rest = &after[end + 1..];
            }
            _ => {
                out.push_str("${");
                rest = after;
            }
        }
    }
    out.push_str(rest);
    out
}

/// The command line from a kernelspec, with `{connection_file}` and `{resource_dir}` filled in. A bare
/// `python` (as ipykernel writes into a virtual environment's own kernelspec) is that environment's Python,
/// as jupyter_client takes it to be its own.
pub fn command_line(spec: &KernelSpec, file: &Path) -> Vec<String> {
    let mut argv: Vec<String> = spec.argv.iter().map(|a| a.replace("{connection_file}", &file.to_string_lossy()).replace("{resource_dir}", &spec.dir.to_string_lossy())).collect();
    let bare_python = argv.first().is_some_and(|p| p == "python" || p == "python3" || (p.starts_with("python3.") && !p.contains(['/', '\\'])));
    if bare_python {
        // spec.dir is <prefix>/share/jupyter/kernels/<name>.
        let prefix = spec.dir.ancestors().nth(4).filter(|_| spec.dir.ancestors().nth(2).is_some_and(|j| j.ends_with(Path::new("share").join("jupyter"))));
        let candidates: Vec<PathBuf> = prefix.map(|p| if cfg!(windows) { vec![p.join("python.exe"), p.join("Scripts").join("python.exe")] } else { vec![p.join("bin").join("python"), p.join("bin").join("python3")] }).unwrap_or_default();
        if let Some(python) = candidates.into_iter().find(|c| c.is_file()) {
            argv[0] = python.to_string_lossy().into_owned();
        }
    }
    argv
}

/// A running kernel's program, as the link needs it. The tests give one that's a thread.
pub trait Process: Send {
    /// None while it runs; once it has stopped, how, in words.
    fn ended(&mut self) -> Option<String>;
    /// Interrupts what it's doing, as a signal would.
    fn interrupt(&mut self);
    /// Ends it at once.
    fn kill(&mut self);
    /// The last lines it wrote to its error output.
    fn last_words(&self) -> String;
}

/// Starts kernels: the program from a kernelspec, or a stand-in for the tests. `held` are the connection file's
/// ports, still bound (see `ConnectionInfo::new`).
pub trait Launcher: Send + Sync {
    fn launch(&self, info: &ConnectionInfo, file: &Path, held: Vec<TcpListener>) -> Result<Box<dyn Process>, String>;
}

/// Starts a kernelspec's program.
pub struct SpawnLauncher {
    pub spec: KernelSpec,
    pub cwd: PathBuf,
}

impl Launcher for SpawnLauncher {
    fn launch(&self, _info: &ConnectionInfo, file: &Path, held: Vec<TcpListener>) -> Result<Box<dyn Process>, String> {
        let argv = command_line(&self.spec, file);
        let mut command = crate::platform::command(&argv[0]);
        command.args(&argv[1..]).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::piped());
        if self.cwd.is_dir() {
            command.current_dir(&self.cwd);
        }
        let env = |name: &str| std::env::var(name).ok();
        for (name, value) in &self.spec.env {
            command.env(name, fill_environment(value, &env));
        }
        #[cfg(unix)]
        {
            use std::os::unix::process::CommandExt;
            // A process group of its own, as jupyter_client gives it, so SIGINT reaches it and whatever it
            // started, and Ctrl+C in a terminal Limoniter was started from doesn't.
            command.process_group(0);
            // The kernel watches for this process going away, and then ends itself.
            command.env("JPY_PARENT_PID", std::process::id().to_string());
        }
        #[cfg(windows)]
        let events = {
            let events = windows_events::Events::new();
            if let Some(events) = &events {
                command.env("JPY_INTERRUPT_EVENT", events.interrupt.to_string());
                command.env("IPY_INTERRUPT_EVENT", events.interrupt.to_string());
                command.env("JPY_PARENT_PID", events.parent.to_string());
            }
            events
        };
        // The ports are the kernel's from here.
        drop(held);
        let mut child = command.spawn().map_err(|e| crate::i18n::tf!("Couldn't start {program}: {e}", program = argv[0]))?;
        let lines = Arc::new(Mutex::new(VecDeque::new()));
        if let Some(stderr) = child.stderr.take() {
            let lines = lines.clone();
            let _ = std::thread::Builder::new().name("jupyter-stderr".into()).spawn(move || {
                for line in BufReader::new(stderr).lines().map_while(Result::ok) {
                    let mut lines = lines.lock().unwrap_or_else(|e| e.into_inner());
                    lines.push_back(line);
                    if lines.len() > LAST_LINES {
                        lines.pop_front();
                    }
                }
            });
        }
        Ok(Box::new(SpawnedProcess {
            child,
            lines,
            ended: None,
            #[cfg(windows)]
            events,
        }))
    }
}

struct SpawnedProcess {
    child: Child,
    lines: Arc<Mutex<VecDeque<String>>>,
    ended: Option<String>,
    #[cfg(windows)]
    events: Option<windows_events::Events>,
}

impl Process for SpawnedProcess {
    fn ended(&mut self) -> Option<String> {
        if self.ended.is_none()
            && let Ok(Some(status)) = self.child.try_wait()
        {
            self.ended = Some(status.to_string());
        }
        self.ended.clone()
    }

    fn interrupt(&mut self) {
        if self.ended().is_some() {
            return;
        }
        #[cfg(unix)]
        {
            unsafe extern "C" {
                fn kill(pid: i32, signal: i32) -> i32;
            }
            const SIGINT: i32 = 2;
            // The whole group, as jupyter_client's os.killpg does. The group is the child's own process id.
            // SAFETY: kill() only sends a signal; a wrong id fails with an error code and harms nothing.
            unsafe {
                kill(-(self.child.id() as i32), SIGINT);
            }
        }
        #[cfg(windows)]
        if let Some(events) = &self.events {
            events.interrupt();
        }
    }

    fn kill(&mut self) {
        if self.ended().is_none() {
            crate::platform::kill_tree(self.child.id());
            let _ = self.child.kill();
            let _ = self.child.wait();
        }
    }

    fn last_words(&self) -> String {
        let lines = self.lines.lock().unwrap_or_else(|e| e.into_inner());
        lines.iter().cloned().collect::<Vec<_>>().join("\n")
    }
}

#[cfg(windows)]
mod windows_events {
    //! The two handles a Windows kernel is given, as jupyter_client gives them: an event it waits on, set to
    //! interrupt it (Windows has no SIGINT to send another program), and a handle to this process, which it
    //! watches so that it ends itself if Limoniter goes away.

    use std::ffi::c_void;

    #[repr(C)]
    struct SecurityAttributes {
        length: u32,
        descriptor: *mut c_void,
        inherit: i32,
    }

    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn CreateEventW(attributes: *const SecurityAttributes, manual_reset: i32, initial: i32, name: *const u16) -> isize;
        fn SetEvent(event: isize) -> i32;
        fn CloseHandle(handle: isize) -> i32;
        fn GetCurrentProcess() -> isize;
        fn DuplicateHandle(source_process: isize, source: isize, target_process: isize, target: *mut isize, access: u32, inherit: i32, options: u32) -> i32;
    }

    const DUPLICATE_SAME_ACCESS: u32 = 2;

    pub struct Events {
        pub interrupt: isize,
        pub parent: isize,
    }

    impl Events {
        /// Both handles, made so the kernel's process inherits them.
        pub fn new() -> Option<Events> {
            let attributes = SecurityAttributes { length: std::mem::size_of::<SecurityAttributes>() as u32, descriptor: std::ptr::null_mut(), inherit: 1 };
            // SAFETY: plain Win32 calls with valid arguments; each handle is closed in drop.
            unsafe {
                let interrupt = CreateEventW(&attributes, 0, 0, std::ptr::null());
                if interrupt == 0 {
                    return None;
                }
                let me = GetCurrentProcess();
                let mut parent = 0isize;
                if DuplicateHandle(me, me, me, &mut parent, 0, 1, DUPLICATE_SAME_ACCESS) == 0 {
                    CloseHandle(interrupt);
                    return None;
                }
                Some(Events { interrupt, parent })
            }
        }

        pub fn interrupt(&self) {
            // SAFETY: the handle is this struct's own, open until drop.
            unsafe {
                SetEvent(self.interrupt);
            }
        }
    }

    impl Drop for Events {
        fn drop(&mut self) {
            // SAFETY: both handles are this struct's own.
            unsafe {
                CloseHandle(self.interrupt);
                CloseHandle(self.parent);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_connection_file_says_what_jupyter_expects() {
        let (info, held) = ConnectionInfo::new("abc", "python3").unwrap();
        assert_eq!(held.len(), 5);
        let ports = [info.shell_port, info.iopub_port, info.stdin_port, info.control_port, info.hb_port];
        assert!(ports.iter().all(|&p| p != 0));
        assert!(ports.iter().enumerate().all(|(i, p)| !ports[i + 1..].contains(p)));
        let file = std::env::temp_dir().join(format!("limoniter-jupyter-test-{}", std::process::id())).join("kernel.json");
        write_connection_file(&info, &file).unwrap();
        let json: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(&file).unwrap()).unwrap();
        assert_eq!(json["shell_port"], info.shell_port);
        assert_eq!(json["ip"], "127.0.0.1");
        assert_eq!(json["transport"], "tcp");
        assert_eq!(json["signature_scheme"], "hmac-sha256");
        assert_eq!(json["key"], "abc");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(std::fs::metadata(&file).unwrap().permissions().mode() & 0o777, 0o600);
        }
        let _ = std::fs::remove_dir_all(file.parent().unwrap());
    }

    #[test]
    fn environment_values_are_filled_in() {
        let env = |name: &str| (name == "HOME").then(|| "/home/me".to_string());
        assert_eq!(fill_environment("${HOME}/lib:${NOPE}:$HOME:${", &env), "/home/me/lib:${NOPE}:$HOME:${");
        assert_eq!(fill_environment("plain", &env), "plain");
    }

    #[test]
    fn the_command_line_is_filled_in_and_a_bare_python_found() {
        let root = std::env::temp_dir().join(format!("limoniter-jupyter-argv-{}", std::process::id()));
        let dir = root.join("share").join("jupyter").join("kernels").join("python3");
        std::fs::create_dir_all(&dir).unwrap();
        let python = if cfg!(windows) { root.join("python.exe") } else { root.join("bin").join("python") };
        std::fs::create_dir_all(python.parent().unwrap()).unwrap();
        std::fs::write(&python, "").unwrap();
        let spec = KernelSpec { name: "python3".into(), display_name: "Python 3".into(), language: "python".into(), argv: vec!["python".into(), "-f".into(), "{connection_file}".into(), "{resource_dir}/x".into()], env: vec![], dir: dir.clone() };
        let argv = command_line(&spec, Path::new("/tmp/k.json"));
        assert_eq!(argv, vec![python.to_string_lossy().into_owned(), "-f".into(), "/tmp/k.json".into(), format!("{}/x", dir.display())]);
        // Elsewhere a bare python is left for the PATH to find.
        let elsewhere = KernelSpec { dir: root.join("kernels").join("python3"), ..spec };
        assert_eq!(command_line(&elsewhere, Path::new("k.json"))[0], "python");
        let _ = std::fs::remove_dir_all(root);
    }
}
