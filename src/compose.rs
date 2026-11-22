//! Docker Compose: finding a project's compose files, picking the tool that runs them (`docker compose`, the old
//! `docker-compose`, or `podman compose`), reading what `config` and `ps` print, the commands each button runs,
//! a poller that asks for the state off the window's thread, and the logs of a service as they stream in.
//! The window's side is in `compose_host.rs`.

use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Child, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

/// The file names Compose looks for, in the order it prefers them.
pub const MAIN_NAMES: [&str; 4] = ["compose.yaml", "compose.yml", "docker-compose.yml", "docker-compose.yaml"];

/// The most log lines kept for a service, so a chatty one can't fill the memory.
const LOG_LINES: usize = 5000;

/// Whether a file name is one of a project's compose files: the usual names, `compose.override.yaml`, and others
/// such as `compose.prod.yml` or `docker-compose.dev.yaml`.
pub fn is_compose_name(name: &str) -> bool {
    let name = name.to_lowercase();
    if MAIN_NAMES.contains(&name.as_str()) {
        return true;
    }
    ["compose.", "docker-compose."].iter().any(|start| name.strip_prefix(start).is_some_and(|rest| (rest.ends_with(".yaml") || rest.ends_with(".yml")) && rest.len() > 5))
}

/// A project's compose files: the main one (or ones) and the others beside it, such as overrides.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Files {
    pub main: Vec<PathBuf>,
    pub others: Vec<PathBuf>,
}

/// The compose files in a folder, or None when it has no main one.
pub fn find(folder: &Path) -> Option<Files> {
    let main: Vec<PathBuf> = MAIN_NAMES.iter().map(|n| folder.join(n)).filter(|p| p.is_file()).collect();
    if main.is_empty() {
        return None;
    }
    let mut others: Vec<PathBuf> = std::fs::read_dir(folder)
        .ok()?
        .flatten()
        .filter(|e| e.file_name().to_str().is_some_and(|n| is_compose_name(n) && !MAIN_NAMES.contains(&n.to_lowercase().as_str())) && e.path().is_file())
        .map(|e| e.path())
        .collect();
    others.sort();
    Some(Files { main, others })
}

/// The tool that runs Compose: a program and the words that go before the command.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Tool {
    pub program: String,
    pub prefix: Vec<String>,
}

impl Tool {
    fn new(program: &str, prefix: &[&str]) -> Tool {
        Tool { program: program.into(), prefix: prefix.iter().map(|s| s.to_string()).collect() }
    }

    /// What it's called, as the person would type it.
    pub fn name(&self) -> String {
        std::iter::once(self.program.as_str()).chain(self.prefix.iter().map(String::as_str)).collect::<Vec<&str>>().join(" ")
    }

    /// A command to run with these arguments, with no window of its own.
    pub fn command(&self, args: &[&str], folder: &Path) -> std::process::Command {
        let mut command = crate::platform::command(&self.program);
        command.args(&self.prefix).args(args).current_dir(folder);
        command
    }

    /// The command as one line for a shell, as a task keeps it.
    pub fn line(&self, args: &[&str]) -> String {
        let quote = |word: &str| if word.contains(' ') { format!("\"{word}\"") } else { word.to_string() };
        std::iter::once(self.program.as_str()).chain(self.prefix.iter().map(String::as_str)).chain(args.iter().copied()).map(quote).collect::<Vec<String>>().join(" ")
    }

    /// Whether it runs: `version` answers.
    pub fn works(&self, folder: &Path) -> bool {
        self.command(&["version"], folder).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null()).status().is_ok_and(|s| s.success())
    }
}

fn on_path(dirs: &[PathBuf], program: &str) -> bool {
    let names = if cfg!(windows) { vec![format!("{program}.exe"), format!("{program}.cmd"), format!("{program}.bat")] } else { vec![program.to_string()] };
    dirs.iter().any(|dir| names.iter().any(|n| dir.join(n).is_file()))
}

/// The tools that could run Compose, in the order to try them: `docker compose` (version 2), `docker-compose`
/// (version 1), then `podman compose`.
pub fn candidates_in(dirs: &[PathBuf]) -> Vec<Tool> {
    let mut found = Vec::new();
    if on_path(dirs, "docker") {
        found.push(Tool::new("docker", &["compose"]));
    }
    if on_path(dirs, "docker-compose") {
        found.push(Tool::new("docker-compose", &[]));
    }
    if on_path(dirs, "podman") {
        found.push(Tool::new("podman", &["compose"]));
    }
    found
}

fn path_dirs() -> Vec<PathBuf> {
    std::env::var_os("PATH").map(|p| std::env::split_paths(&p).collect()).unwrap_or_default()
}

/// The first of the tools that runs, trying each in turn. Starts programs, so not for the window's thread.
pub fn detect(folder: &Path) -> Option<Tool> {
    candidates_in(&path_dirs()).into_iter().find(|t| t.works(folder))
}

/// The tool a task's command line should use, chosen from what's installed without running anything.
pub fn guess() -> Option<Tool> {
    candidates_in(&path_dirs()).into_iter().next()
}

/// Something to do to the project or to one service.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Action {
    Up,
    Down,
    Pull,
    ServiceUp(String),
    ServiceDown(String),
    Restart(String),
    Rebuild(String),
}

impl Action {
    /// The arguments after the tool's own words.
    pub fn args(&self) -> Vec<&str> {
        match self {
            Action::Up => vec!["up", "-d"],
            Action::Down => vec!["down"],
            Action::Pull => vec!["pull"],
            Action::ServiceUp(s) => vec!["up", "-d", s],
            // `down` takes the whole project, so one service is stopped.
            Action::ServiceDown(s) => vec!["stop", s],
            Action::Restart(s) => vec!["restart", s],
            Action::Rebuild(s) => vec!["up", "-d", "--build", s],
        }
    }
}

/// The arguments that follow a service's logs.
pub fn logs_args(service: &str) -> Vec<&str> {
    vec!["logs", "-f", "--no-color", "--tail", "200", service]
}

/// The arguments for a shell in a service: bash when the container has it, and sh when it doesn't.
pub fn shell_args(service: &str) -> Vec<&str> {
    vec!["exec", service, "sh", "-c", "if command -v bash >/dev/null 2>&1; then exec bash; else exec sh; fi"]
}

/// A published port.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Port {
    pub host_ip: String,
    pub published: Option<u16>,
    pub target: u16,
    pub protocol: String,
}

impl Port {
    /// Where a browser would open it: only for a TCP port that's published.
    pub fn url(&self) -> Option<String> {
        let published = self.published?;
        if self.protocol != "tcp" {
            return None;
        }
        let scheme = if self.target == 443 || published == 443 { "https" } else { "http" };
        Some(format!("{scheme}://localhost:{published}"))
    }

    /// How it's written: `8080:80/tcp`, or `80/tcp` when it isn't published.
    pub fn label(&self) -> String {
        match self.published {
            Some(p) => format!("{p}\u{2192}{}/{}", self.target, self.protocol),
            None => format!("{}/{}", self.target, self.protocol),
        }
    }
}

/// A service the project defines.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Service {
    pub name: String,
    pub image: Option<String>,
    pub builds: bool,
    pub ports: Vec<Port>,
}

fn number(value: Option<&serde_json::Value>) -> Option<u16> {
    match value? {
        serde_json::Value::Number(n) => n.as_u64().and_then(|n| u16::try_from(n).ok()),
        serde_json::Value::String(s) => s.parse().ok(),
        _ => None,
    }
}

/// The services in what `config --format json` printed.
pub fn parse_config(text: &str) -> Result<Vec<Service>, String> {
    let value: serde_json::Value = serde_json::from_str(text.trim()).map_err(|e| e.to_string())?;
    let services = value.get("services").and_then(|s| s.as_object()).ok_or_else(|| "no services".to_string())?;
    let mut found = Vec::new();
    for (name, body) in services {
        let mut service = Service { name: name.clone(), image: body.get("image").and_then(|i| i.as_str()).map(String::from), builds: body.get("build").is_some_and(|b| !b.is_null()), ports: Vec::new() };
        for port in body.get("ports").and_then(|p| p.as_array()).into_iter().flatten() {
            // Newer versions print each as an object; a plain string is read as it's written in the file.
            if let Some(text) = port.as_str() {
                service.ports.extend(parse_port(text));
            } else if let Some(target) = number(port.get("target")) {
                service.ports.push(Port { host_ip: port.get("host_ip").and_then(|i| i.as_str()).unwrap_or("").into(), published: number(port.get("published")), target, protocol: port.get("protocol").and_then(|p| p.as_str()).unwrap_or("tcp").into() });
            }
        }
        found.push(service);
    }
    Ok(found)
}

/// One of a service's containers, as `ps --format json` lists it.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Container {
    pub service: String,
    pub name: String,
    pub state: String,
    pub health: String,
    pub status: String,
    pub ports: Vec<Port>,
}

/// The containers in what `ps --format json` printed: one object a line (newer versions) or an array (older).
pub fn parse_ps(text: &str) -> Result<Vec<Container>, String> {
    let text = text.trim();
    if text.is_empty() {
        return Ok(Vec::new());
    }
    let objects: Vec<serde_json::Value> = if text.starts_with('[') {
        serde_json::from_str(text).map_err(|e| e.to_string())?
    } else {
        text.lines().filter(|l| !l.trim().is_empty()).map(serde_json::from_str).collect::<Result<_, _>>().map_err(|e: serde_json::Error| e.to_string())?
    };
    let word = |o: &serde_json::Value, key: &str| o.get(key).and_then(|v| v.as_str()).unwrap_or("").to_string();
    Ok(objects
        .iter()
        .map(|o| {
            let mut ports: Vec<Port> = Vec::new();
            for p in o.get("Publishers").and_then(|p| p.as_array()).into_iter().flatten() {
                let Some(target) = number(p.get("TargetPort")) else { continue };
                let published = number(p.get("PublishedPort")).filter(|p| *p != 0);
                let port = Port { host_ip: word(p, "URL"), published, target, protocol: p.get("Protocol").and_then(|v| v.as_str()).unwrap_or("tcp").into() };
                // Compose lists a port once for IPv4 and once for IPv6.
                if !ports.iter().any(|q| q.published == port.published && q.target == port.target && q.protocol == port.protocol) {
                    ports.push(port);
                }
            }
            Container { service: word(o, "Service"), name: word(o, "Name"), state: word(o, "State"), health: word(o, "Health"), status: word(o, "Status"), ports }
        })
        .collect())
}

/// A service's state as one word, from its containers: running, restarting, exited and so on; `down` for none.
pub fn state_of(service: &str, containers: &[Container]) -> String {
    let mine: Vec<&Container> = containers.iter().filter(|c| c.service == service).collect();
    if mine.is_empty() {
        return "down".into();
    }
    // A container that isn't running is what to see first.
    mine.iter().find(|c| c.state != "running").unwrap_or(&mine[0]).state.clone()
}

/// The services as the compose files say, for when `config` can't be run: a plain reading of the `services:` block.
pub fn read_services(files: &[PathBuf]) -> Vec<Service> {
    let Some(text) = files.iter().find_map(|f| std::fs::read_to_string(f).ok()) else { return Vec::new() };
    services_in_yaml(&text)
}

/// The services named in a compose file's text: the keys one level under `services:`, with their image and ports.
pub fn services_in_yaml(text: &str) -> Vec<Service> {
    let mut found: Vec<Service> = Vec::new();
    let mut in_services = false;
    let mut child_indent = None;
    let mut in_ports = false;
    for raw in text.lines() {
        let line = raw.split(" #").next().unwrap_or(raw).trim_end();
        let trimmed = line.trim_start();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        let indent = line.len() - trimmed.len();
        if indent == 0 {
            in_services = trimmed == "services:";
            child_indent = None;
            continue;
        }
        if !in_services {
            continue;
        }
        let level = *child_indent.get_or_insert(indent);
        if indent == level {
            if let Some(name) = trimmed.strip_suffix(':') {
                found.push(Service { name: name.trim_matches(['"', '\'']).into(), ..Service::default() });
            }
            in_ports = false;
        } else if indent > level && let Some(service) = found.last_mut() {
            if let Some(image) = trimmed.strip_prefix("image:") {
                service.image = Some(image.trim().trim_matches(['"', '\'']).into());
                in_ports = false;
            } else if trimmed.starts_with("build:") {
                service.builds = true;
                in_ports = false;
            } else if let Some(rest) = trimmed.strip_prefix("ports:") {
                in_ports = true;
                // The inline form: ports: ["80:80", "443:443"].
                for item in rest.trim().trim_start_matches('[').trim_end_matches(']').split(',') {
                    service.ports.extend(parse_port(item.trim().trim_matches(['"', '\''])));
                }
            } else if in_ports && let Some(item) = trimmed.strip_prefix('-') {
                service.ports.extend(parse_port(item.trim().trim_matches(['"', '\''])));
            } else if !trimmed.starts_with('-') {
                in_ports = false;
            }
        }
    }
    found
}

/// A port as a compose file writes it: `80`, `8080:80`, `127.0.0.1:8080:80/udp`. A range or a variable gives None.
pub fn parse_port(text: &str) -> Option<Port> {
    let (spec, protocol) = match text.split_once('/') {
        Some((s, p)) => (s, p.to_string()),
        None => (text, "tcp".to_string()),
    };
    let parts: Vec<&str> = spec.split(':').collect();
    let num = |s: &str| s.trim().parse::<u16>().ok();
    match parts.as_slice() {
        [target] => Some(Port { host_ip: String::new(), published: None, target: num(target)?, protocol }),
        [published, target] => Some(Port { host_ip: String::new(), published: Some(num(published)?), target: num(target)?, protocol }),
        [ip, published, target] => Some(Port { host_ip: (*ip).into(), published: Some(num(published)?), target: num(target)?, protocol }),
        _ => None,
    }
}

/// What hovering over an `image:` line says: the image and its tag.
pub fn describe_image(reference: &str) -> String {
    let reference = reference.trim().trim_matches(['"', '\'']);
    if let Some((name, digest)) = reference.split_once('@') {
        return crate::i18n::tf!("{name}\nPinned to the exact image {digest}");
    }
    // A colon after the last slash starts the tag; one before it is a registry's port.
    let after_slash = reference.rfind('/').map_or(0, |i| i + 1);
    match reference[after_slash..].rfind(':') {
        Some(i) => {
            let (name, tag) = reference.split_at(after_slash + i);
            {
                let tag = &tag[1..];
                crate::i18n::tf!("{name}\nTag {tag}")
            }
        }
        None => crate::i18n::tf!("{reference}\nNo tag, so the tag latest is used"),
    }
}

/// What hovering over a port line says, in words: what's mapped to what.
pub fn describe_port(port: &Port) -> String {
    let side = if port.host_ip.is_empty() { String::new() } else { crate::i18n::tf!(" (only on {})", port.host_ip) };
    let (target, protocol) = (port.target, &port.protocol);
    match port.published {
        Some(p) => crate::i18n::tf!("Port {p} on this computer{side} goes to port {target} in the container ({protocol})"),
        None => crate::i18n::tf!("Port {target} in the container, with a port on this computer chosen when it starts ({protocol})"),
    }
}

/// The words for a hover over a column of a compose file's line, or None: `image:` lines and port mappings.
pub fn describe_line(line: &str) -> Option<String> {
    let trimmed = line.trim();
    if let Some(rest) = trimmed.strip_prefix("image:") {
        let rest = rest.split(" #").next().unwrap_or(rest).trim();
        return (!rest.is_empty()).then(|| describe_image(rest));
    }
    let item = trimmed.strip_prefix("- ").unwrap_or(trimmed);
    let item = item.split(" #").next().unwrap_or(item).trim().trim_matches(['"', '\'']);
    // Only things that look like a port mapping: digits with colons, a slash and a protocol at most.
    let looks_like_port = !item.is_empty() && item.chars().all(|c| c.is_ascii_digit() || matches!(c, ':' | '.' | '/' | 'u' | 'd' | 'p' | 't' | 'c')) && item.chars().next().is_some_and(|c| c.is_ascii_digit());
    if looks_like_port && trimmed.starts_with("- ") {
        return parse_port(item).map(|p| describe_port(&p));
    }
    // The inline form: ports: ["80:80", "443:443"].
    if let Some(rest) = trimmed.strip_prefix("ports:") {
        let ports: Vec<String> = rest.trim().trim_start_matches('[').trim_end_matches(']').split(',').filter_map(|i| parse_port(i.trim().trim_matches(['"', '\''])).map(|p| describe_port(&p))).collect();
        return (!ports.is_empty()).then(|| ports.join("\n"));
    }
    None
}

/// The tasks for the whole project: Up, Down and Pull, when the folder has compose files.
pub fn tasks(folder: &Path) -> Vec<(String, String)> {
    if find(folder).is_none() {
        return Vec::new();
    }
    let Some(tool) = guess() else { return Vec::new() };
    [("compose up", Action::Up), ("compose down", Action::Down), ("compose pull", Action::Pull)].into_iter().map(|(name, action)| (name.to_string(), tool.line(&action.args()))).collect()
}

/// What the poller found the last time it looked.
#[derive(Clone, Debug, Default)]
pub struct Snapshot {
    pub tool: Option<Tool>,
    pub services: Vec<Service>,
    pub containers: Vec<Container>,
    /// What's wrong, when something is: no tool installed, `ps` failing because the daemon isn't running.
    pub problem: Option<String>,
    /// How many times it has looked, so the window can tell when there's news.
    pub rounds: u64,
}

/// Looks at a project every few seconds on a thread of its own, until it's dropped.
pub struct Poller {
    shared: Arc<Mutex<Snapshot>>,
    stop: Arc<AtomicBool>,
    wake: Arc<AtomicBool>,
}

fn output_of(tool: &Tool, args: &[&str], folder: &Path) -> Result<String, String> {
    let output = tool.command(args, folder).stdin(Stdio::null()).output().map_err(|e| e.to_string())?;
    if output.status.success() {
        Ok(String::from_utf8_lossy(&output.stdout).into_owned())
    } else {
        let said = String::from_utf8_lossy(&output.stderr).trim().to_string();
        Err(if said.is_empty() { crate::i18n::tf!("{} stopped with an error.", tool.name()) } else { said })
    }
}

/// One look, for the poller and for tests: the services (from `config` when `refresh_config`, else kept) and
/// the containers.
pub fn look(tool: Option<&Tool>, folder: &Path, previous: &Snapshot, refresh_config: bool) -> Snapshot {
    let mut next = Snapshot { rounds: previous.rounds + 1, ..previous.clone() };
    let Some(tool) = tool else {
        next.tool = None;
        next.services = read_services(&find(folder).map(|f| f.main).unwrap_or_default());
        next.containers.clear();
        next.problem = Some(crate::i18n::tr("Neither docker compose, docker-compose nor podman compose could be started: is Docker installed?").into());
        return next;
    };
    next.tool = Some(tool.clone());
    next.problem = None;
    if refresh_config || next.services.is_empty() {
        match output_of(tool, &["config", "--format", "json"], folder).and_then(|t| parse_config(&t)) {
            Ok(services) => next.services = services,
            Err(problem) => {
                next.services = read_services(&find(folder).map(|f| f.main).unwrap_or_default());
                next.problem = Some(crate::i18n::tf!("The project's configuration couldn't be read, so its files are read instead: {problem}"));
            }
        }
    }
    match output_of(tool, &["ps", "--all", "--format", "json"], folder).and_then(|t| parse_ps(&t)) {
        Ok(containers) => next.containers = containers,
        Err(problem) => {
            next.containers.clear();
            next.problem.get_or_insert(problem);
        }
    }
    next
}

impl Poller {
    /// Starts looking at the project in a folder. `tool` is what to use, or None to find one. `ctx` is woken
    /// when there's news.
    pub fn start(folder: PathBuf, tool: Option<Tool>, every: Duration, ctx: Option<eframe::egui::Context>) -> Poller {
        let shared = Arc::new(Mutex::new(Snapshot::default()));
        let (stop, wake) = (Arc::new(AtomicBool::new(false)), Arc::new(AtomicBool::new(true)));
        let (s, st, w) = (shared.clone(), stop.clone(), wake.clone());
        std::thread::spawn(move || {
            let tool = tool.or_else(|| detect(&folder));
            let mut config_due = true;
            while !st.load(Ordering::Relaxed) {
                let previous = s.lock().map(|g| g.clone()).unwrap_or_default();
                let next = look(tool.as_ref(), &folder, &previous, config_due);
                config_due = false;
                if st.load(Ordering::Relaxed) {
                    break;
                }
                if let Ok(mut guard) = s.lock() {
                    *guard = next;
                }
                if let Some(ctx) = &ctx {
                    ctx.request_repaint();
                }
                // Waits in short steps, so closing the panel or asking for a look is quick.
                w.store(false, Ordering::Relaxed);
                let mut waited = Duration::ZERO;
                while waited < every && !st.load(Ordering::Relaxed) && !w.load(Ordering::Relaxed) {
                    std::thread::sleep(Duration::from_millis(50));
                    waited += Duration::from_millis(50);
                }
                if w.load(Ordering::Relaxed) {
                    config_due = true;
                }
            }
        });
        Poller { shared, stop, wake }
    }

    pub fn snapshot(&self) -> Snapshot {
        self.shared.lock().map(|g| g.clone()).unwrap_or_default()
    }

    /// Asks for a look now, services included.
    pub fn refresh(&self) {
        self.wake.store(true, Ordering::Relaxed);
    }
}

impl Drop for Poller {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}

/// A service's logs, following as they come, until it's dropped (which stops them).
pub struct LogStream {
    pub service: String,
    lines: Arc<Mutex<Vec<String>>>,
    child: Child,
    ended: Arc<AtomicBool>,
}

impl LogStream {
    pub fn start(tool: &Tool, folder: &Path, service: &str, ctx: Option<eframe::egui::Context>) -> Result<LogStream, String> {
        let mut command = tool.command(&logs_args(service), folder);
        command.stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped());
        let mut child = command.spawn().map_err(|e| crate::i18n::tf!("{} couldn't be started: {e}", tool.name()))?;
        let lines = Arc::new(Mutex::new(Vec::new()));
        let ended = Arc::new(AtomicBool::new(false));
        let out: Option<Box<dyn std::io::Read + Send>> = child.stdout.take().map(|s| Box::new(s) as Box<dyn std::io::Read + Send>);
        let err: Option<Box<dyn std::io::Read + Send>> = child.stderr.take().map(|s| Box::new(s) as Box<dyn std::io::Read + Send>);
        let open = Arc::new(std::sync::atomic::AtomicUsize::new(usize::from(out.is_some()) + usize::from(err.is_some())));
        for stream in [out, err].into_iter().flatten() {
            let (lines, ended, open, ctx) = (lines.clone(), ended.clone(), open.clone(), ctx.clone());
            std::thread::spawn(move || {
                for line in BufReader::new(stream).lines().map_while(Result::ok) {
                    if let Ok(mut kept) = lines.lock() {
                        kept.push(line);
                        if kept.len() > LOG_LINES {
                            let extra = kept.len() - LOG_LINES;
                            kept.drain(..extra);
                        }
                    }
                    if let Some(ctx) = &ctx {
                        ctx.request_repaint();
                    }
                }
                if open.fetch_sub(1, Ordering::Relaxed) == 1 {
                    ended.store(true, Ordering::Relaxed);
                    if let Some(ctx) = &ctx {
                        ctx.request_repaint();
                    }
                }
            });
        }
        Ok(LogStream { service: service.into(), lines, child, ended })
    }

    pub fn lines(&self) -> Vec<String> {
        self.lines.lock().map(|l| l.clone()).unwrap_or_default()
    }

    /// Whether the command has finished and printed everything.
    pub fn ended(&self) -> bool {
        self.ended.load(Ordering::Relaxed)
    }
}

impl Drop for LogStream {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const CONFIG: &str = include_str!("compose_testdata/config.json");
    const PS: &str = include_str!("compose_testdata/ps.ndjson");
    const PS_ARRAY: &str = include_str!("compose_testdata/ps-array.json");
    const YAML: &str = include_str!("compose_testdata/compose.yaml");
    // The fake docker is a shell script, used only by the Unix-only tests.
    #[cfg(unix)]
    const FAKE: &str = include_str!("compose_testdata/docker.sh");

    #[test]
    fn compose_file_names_are_recognised() {
        for name in ["compose.yaml", "compose.yml", "docker-compose.yml", "docker-compose.yaml", "compose.override.yaml", "docker-compose.prod.yml", "Compose.YAML"] {
            assert!(is_compose_name(name), "{name}");
        }
        for name in ["compose.txt", "docker-compose", "compose.yaml.bak", "other.yaml", "compose..yml"] {
            assert!(!is_compose_name(name), "{name}");
        }
    }

    #[test]
    fn a_folder_s_compose_files_are_found_with_their_overrides() {
        let folder = fresh_folder("find");
        assert!(find(&folder).is_none());
        std::fs::write(folder.join("docker-compose.yml"), "services: {}").unwrap();
        std::fs::write(folder.join("docker-compose.override.yml"), "services: {}").unwrap();
        std::fs::write(folder.join("compose.prod.yaml"), "services: {}").unwrap();
        std::fs::write(folder.join("notes.yaml"), "").unwrap();
        let found = find(&folder).unwrap();
        assert_eq!(found.main, vec![folder.join("docker-compose.yml")]);
        assert_eq!(found.others, vec![folder.join("compose.prod.yaml"), folder.join("docker-compose.override.yml")]);
    }

    #[test]
    fn docker_compose_is_preferred_then_the_old_one_then_podman() {
        let folder = fresh_folder("path");
        let dirs = vec![folder.clone()];
        assert!(candidates_in(&dirs).is_empty());
        std::fs::write(folder.join(if cfg!(windows) { "podman.exe" } else { "podman" }), "").unwrap();
        assert_eq!(candidates_in(&dirs)[0].name(), "podman compose");
        std::fs::write(folder.join(if cfg!(windows) { "docker-compose.exe" } else { "docker-compose" }), "").unwrap();
        assert_eq!(candidates_in(&dirs)[0].name(), "docker-compose");
        std::fs::write(folder.join(if cfg!(windows) { "docker.exe" } else { "docker" }), "").unwrap();
        let names: Vec<String> = candidates_in(&dirs).iter().map(Tool::name).collect();
        assert_eq!(names, ["docker compose", "docker-compose", "podman compose"]);
    }

    #[test]
    fn config_lists_the_services_with_their_images_and_ports() {
        let services = parse_config(CONFIG).unwrap();
        let names: Vec<&str> = services.iter().map(|s| s.name.as_str()).collect();
        assert_eq!(names, ["db", "web", "worker"]);
        assert_eq!(services[0].image.as_deref(), Some("postgres:16-alpine"));
        assert_eq!(services[0].ports, vec![Port { host_ip: "127.0.0.1".into(), published: Some(5432), target: 5432, protocol: "tcp".into() }]);
        assert!(services[1].builds && services[1].image.is_none());
        assert_eq!(services[1].ports.len(), 2);
        assert_eq!(services[1].ports[0].url().as_deref(), Some("http://localhost:8080"));
        assert_eq!(services[1].ports[1].url(), None, "an unpublished port has nowhere to open");
        assert!(parse_config("not json").is_err());
        assert!(parse_config("{}").is_err());
    }

    #[test]
    fn ps_is_read_as_lines_or_as_an_array() {
        let containers = parse_ps(PS).unwrap();
        assert_eq!(containers.len(), 3);
        assert_eq!((containers[0].service.as_str(), containers[0].state.as_str(), containers[0].health.as_str()), ("db", "running", "healthy"));
        // The IPv6 twin of web's port is dropped, and the unpublished one has no published number.
        assert_eq!(containers[1].ports.len(), 2);
        assert_eq!(containers[1].ports[0].label(), "8080\u{2192}80/tcp");
        assert_eq!(containers[1].ports[1].label(), "443/tcp");
        assert_eq!(containers[2].state, "exited");
        let array = parse_ps(PS_ARRAY).unwrap();
        assert_eq!(array.len(), 2);
        assert_eq!(array[1].state, "restarting");
        assert!(array[1].ports.is_empty(), "null publishers");
        assert!(parse_ps("").unwrap().is_empty());
        assert!(parse_ps("{broken").is_err());
    }

    #[test]
    fn a_service_s_state_is_the_one_to_look_at() {
        let containers = parse_ps(PS).unwrap();
        assert_eq!(state_of("web", &containers), "running");
        assert_eq!(state_of("worker", &containers), "exited");
        assert_eq!(state_of("cache", &containers), "down");
    }

    #[test]
    fn the_files_are_read_for_the_list_when_the_command_isn_t_there() {
        let services = services_in_yaml(YAML);
        let names: Vec<&str> = services.iter().map(|s| s.name.as_str()).collect();
        assert_eq!(names, ["web", "db", "worker"]);
        assert!(services[0].builds);
        assert_eq!(services[0].image.as_deref(), Some("shop-web:latest"));
        assert_eq!(services[0].ports.iter().map(Port::label).collect::<Vec<_>>(), ["8080\u{2192}80/tcp", "443/tcp"]);
        assert_eq!(services[1].ports[0].host_ip, "127.0.0.1");
        assert_eq!(services[2].image.as_deref(), Some("shop-worker"));
    }

    #[test]
    fn ports_are_read_as_compose_writes_them() {
        assert_eq!(parse_port("80").unwrap().published, None);
        let p = parse_port("127.0.0.1:8080:80/udp").unwrap();
        assert_eq!((p.host_ip.as_str(), p.published, p.target, p.protocol.as_str()), ("127.0.0.1", Some(8080), 80, "udp"));
        assert_eq!(p.url(), None, "UDP isn't a web page");
        assert!(parse_port("8000-8010:80").is_none());
        assert!(parse_port("${PORT}:80").is_none());
        assert_eq!(parse_port("8443:443").unwrap().url().as_deref(), Some("https://localhost:8443"));
    }

    #[test]
    fn images_say_their_tag() {
        assert_eq!(describe_image("postgres:16-alpine"), "postgres\nTag 16-alpine");
        assert_eq!(describe_image("\"nginx\""), "nginx\nNo tag, so the tag latest is used");
        assert_eq!(describe_image("registry.local:5000/team/app"), "registry.local:5000/team/app\nNo tag, so the tag latest is used");
        assert_eq!(describe_image("registry.local:5000/team/app:2.1"), "registry.local:5000/team/app\nTag 2.1");
        assert!(describe_image("alpine@sha256:abc").contains("Pinned"));
    }

    #[test]
    fn hovers_come_from_image_and_port_lines_only() {
        assert_eq!(describe_line("    image: redis:7 # cache").as_deref(), Some("redis\nTag 7"));
        assert_eq!(describe_line("      - \"8080:80\"").as_deref(), Some("Port 8080 on this computer goes to port 80 in the container (tcp)"));
        assert!(describe_line("    ports: [\"80:80\", \"443:443\"]").unwrap().lines().count() == 2);
        assert!(describe_line("      - 5432").unwrap().contains("chosen when it starts"));
        assert_eq!(describe_line("    restart: always"), None);
        assert_eq!(describe_line("      - ./data:/var/lib"), None, "a volume isn't a port");
        assert_eq!(describe_line("      - redis"), None);
    }

    #[test]
    fn the_buttons_run_the_right_commands() {
        assert_eq!(Action::Up.args(), ["up", "-d"]);
        assert_eq!(Action::Rebuild("web".into()).args(), ["up", "-d", "--build", "web"]);
        assert_eq!(Action::ServiceDown("web".into()).args(), ["stop", "web"]);
        assert_eq!(Action::Restart("db".into()).args(), ["restart", "db"]);
        assert_eq!(logs_args("web")[..3], ["logs", "-f", "--no-color"]);
        assert_eq!(shell_args("web")[..3], ["exec", "web", "sh"]);
        let tool = Tool::new("docker", &["compose"]);
        assert_eq!(tool.line(&Action::Pull.args()), "docker compose pull");
        assert_eq!(Tool::new("/a b/docker-compose", &[]).line(&["down"]), "\"/a b/docker-compose\" down");
    }

    // The fake Docker is a shell script run through sh, so nothing has to be made executable.
    #[cfg(unix)]
    fn fake(name: &str) -> (Tool, PathBuf) {
        let folder = fresh_folder(name);
        for (file, text) in [("docker.sh", FAKE), ("config.json", CONFIG), ("ps.ndjson", PS), ("compose.yaml", YAML)] {
            std::fs::write(folder.join(file), text).unwrap();
        }
        let tool = Tool { program: "sh".into(), prefix: vec![folder.join("docker.sh").display().to_string(), "compose".into()] };
        (tool, folder)
    }

    fn fresh_folder(name: &str) -> PathBuf {
        let folder = std::env::temp_dir().join(format!("limoniter-compose-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&folder);
        std::fs::create_dir_all(&folder).unwrap();
        folder
    }

    #[cfg(unix)]
    #[test]
    fn a_look_reads_config_and_ps_from_the_tool() {
        let (tool, folder) = fake("look");
        assert!(tool.works(&folder));
        let snapshot = look(Some(&tool), &folder, &Snapshot::default(), true);
        assert!(snapshot.problem.is_none(), "{:?}", snapshot.problem);
        assert_eq!(snapshot.services.len(), 3);
        assert_eq!(snapshot.containers.len(), 3);
        assert_eq!(state_of("worker", &snapshot.containers), "exited");
        assert_eq!(snapshot.rounds, 1);
    }

    #[cfg(unix)]
    #[test]
    fn a_failing_ps_is_said_and_the_services_are_still_listed() {
        let (tool, folder) = fake("broken");
        std::fs::write(folder.join("ps-broken"), "").unwrap();
        let snapshot = look(Some(&tool), &folder, &Snapshot::default(), true);
        assert!(snapshot.problem.as_deref().is_some_and(|p| p.contains("Docker daemon")), "{:?}", snapshot.problem);
        assert_eq!(snapshot.services.len(), 3);
        assert!(snapshot.containers.is_empty());
    }

    #[cfg(unix)]
    #[test]
    fn when_config_can_t_be_read_the_file_is() {
        let (tool, folder) = fake("noconfig");
        std::fs::write(folder.join("config.json"), "oops").unwrap();
        let snapshot = look(Some(&tool), &folder, &Snapshot::default(), true);
        assert_eq!(snapshot.services.iter().map(|s| s.name.as_str()).collect::<Vec<_>>(), ["web", "db", "worker"]);
        assert!(snapshot.problem.is_some());
        assert_eq!(snapshot.containers.len(), 3, "ps still worked");
    }

    #[test]
    fn with_no_tool_the_files_are_read_and_the_problem_said() {
        let folder = fresh_folder("notool");
        std::fs::write(folder.join("compose.yaml"), YAML).unwrap();
        let snapshot = look(None, &folder, &Snapshot::default(), true);
        assert_eq!(snapshot.services.len(), 3);
        assert!(snapshot.problem.is_some() && snapshot.tool.is_none());
    }

    #[cfg(unix)]
    #[test]
    fn the_poller_looks_again_when_asked_and_stops_when_dropped() {
        let (tool, folder) = fake("poller");
        let poller = Poller::start(folder, Some(tool), Duration::from_secs(60), None);
        let wait = |n: u64| {
            for _ in 0..200 {
                if poller.snapshot().rounds >= n {
                    return true;
                }
                std::thread::sleep(Duration::from_millis(50));
            }
            false
        };
        assert!(wait(1), "the first look");
        assert_eq!(poller.snapshot().containers.len(), 3);
        poller.refresh();
        assert!(wait(2), "a look on request");
        let stop = poller.stop.clone();
        drop(poller);
        assert!(stop.load(Ordering::Relaxed));
    }

    #[cfg(unix)]
    #[test]
    fn logs_stream_in_and_stop_when_dropped() {
        let (tool, folder) = fake("logs");
        let stream = LogStream::start(&tool, &folder, "web", None).unwrap();
        for _ in 0..200 {
            if stream.ended() {
                break;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        assert_eq!(stream.lines(), ["web-1  | listening on 80", "web-1  | GET / 200"]);
        drop(stream);
    }

    #[cfg(unix)]
    #[test]
    fn a_shell_runs_in_the_service() {
        let (tool, folder) = fake("shell");
        let output = tool.command(&shell_args("web"), &folder).output().unwrap();
        assert!(String::from_utf8_lossy(&output.stdout).starts_with("ran exec web sh -c"));
    }
}
