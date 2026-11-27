//! A terminal in the window: the person's shell on a pseudo-terminal (ConPTY on Windows), its output read into a
//! VT100 screen as an xterm would show it (colours, the cursor, full-screen programs), and keys turned into what
//! a terminal sends.

use eframe::egui::{self, Color32, FontId, Key, Modifiers, Rect, Sense, Ui, pos2, vec2};
use portable_pty::{CommandBuilder, PtySize, native_pty_system};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, Sender};
use std::sync::{Arc, Mutex};
use crate::remote::{self, KnownHosts, Refused, Secret, Site};

pub struct Terminal {
    parser: Arc<Mutex<vt100::Parser>>,
    writer: Arc<Mutex<Box<dyn Write + Send>>>,
    backend: Backend,
    size: (u16, u16),
    /// Whether the shell has ended.
    ended: Arc<AtomicBool>,
    /// Set when something new is on the screen, to draw it.
    pub changed: Arc<AtomicBool>,
    /// The folder it started in.
    pub folder: std::path::PathBuf,
    /// The shell it runs, to start the same one again.
    pub shell: Shell,
    /// The server and folder, when it's a session on a server rather than a shell here.
    pub ssh: Option<(Site, Option<String>)>,
    /// Why the server refused the connection, for the window to deal with (to ask about a new key, say).
    refusal: Arc<Mutex<Option<Refused>>>,
    /// A password or passphrase the person typed that the server accepted, to be kept for this session.
    learned: Arc<Mutex<Option<Secret>>>,
}

/// What a terminal's shell is on the other end of: a pseudo-terminal here, or an SSH channel, which one thread
/// looks after (the library's channels aren't to be read and written from two threads at once).
enum Backend {
    Pty { master: Box<dyn portable_pty::MasterPty + Send>, child: Box<dyn portable_pty::Child + Send + Sync> },
    Ssh { to_server: Sender<ToServer> },
}

/// What the window asks of an SSH session's thread.
enum ToServer {
    Bytes(Vec<u8>),
    Size(u16, u16),
}

/// A `Write` that hands what's written to an SSH session's thread, so the reading thread can answer a program's
/// questions (where the cursor is) in the same way it does for a shell here.
struct Sending(Sender<ToServer>);

impl Write for Sending {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        self.0.send(ToServer::Bytes(bytes.to_vec())).map_err(|_| std::io::ErrorKind::BrokenPipe)?;
        Ok(bytes.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

/// A shell a terminal can run. On Windows there's a choice; elsewhere, the person's own.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum Shell {
    /// PowerShell on Windows, `$SHELL` elsewhere.
    #[default]
    Usual,
    Cmd,
    GitBash(std::path::PathBuf),
    /// A WSL distribution, by its name.
    Wsl(String),
}

impl Shell {
    /// How the choice is kept in the settings: "", "cmd", "gitbash" or "wsl:NAME".
    pub fn id(&self) -> String {
        match self {
            Shell::Usual => String::new(),
            Shell::Cmd => "cmd".into(),
            Shell::GitBash(_) => "gitbash".into(),
            Shell::Wsl(name) => format!("wsl:{name}"),
        }
    }

    pub fn from_id(id: &str) -> Shell {
        match id {
            "cmd" => Shell::Cmd,
            "gitbash" => git_bash().map_or(Shell::Usual, Shell::GitBash),
            _ => id.strip_prefix("wsl:").filter(|n| !n.is_empty()).map_or(Shell::Usual, |name| Shell::Wsl(name.to_string())),
        }
    }

    pub fn label(&self) -> String {
        match self {
            Shell::Usual if cfg!(windows) => "PowerShell".into(),
            Shell::Usual => crate::i18n::tr("The usual shell").to_string(),
            Shell::Cmd => crate::i18n::tr("Command Prompt").to_string(),
            Shell::GitBash(_) => "Git Bash".into(),
            Shell::Wsl(name) => crate::i18n::tf!("WSL: {name}"),
        }
    }

    /// The program and its arguments, starting in `folder` (WSL is told it in Windows' own form, which it turns
    /// into /mnt/c/…).
    fn command(&self, folder: &Path) -> CommandBuilder {
        match self {
            Shell::Usual => shell(),
            Shell::Cmd => CommandBuilder::new("cmd.exe"),
            Shell::GitBash(bash) => {
                let mut command = CommandBuilder::new(bash);
                command.args(["--login", "-i"]);
                command
            }
            Shell::Wsl(name) => {
                let mut command = CommandBuilder::new("wsl.exe");
                command.args(["-d", name.as_str(), "--cd", &folder.display().to_string()]);
                command
            }
        }
    }
}

/// The shells to offer: on Windows, PowerShell, Command Prompt, Git Bash when it's installed, and each WSL
/// distribution (found once, then remembered); elsewhere, the usual one only.
pub fn shells() -> Vec<Shell> {
    if !cfg!(windows) {
        return vec![Shell::Usual];
    }
    static WSL: std::sync::OnceLock<Vec<String>> = std::sync::OnceLock::new();
    let mut shells = vec![Shell::Usual, Shell::Cmd];
    shells.extend(git_bash().map(Shell::GitBash));
    shells.extend(WSL.get_or_init(wsl_distributions).iter().cloned().map(Shell::Wsl));
    shells
}

/// Git for Windows' bash, where it's usually installed.
fn git_bash() -> Option<std::path::PathBuf> {
    ["ProgramFiles", "ProgramW6432", "LOCALAPPDATA"]
        .iter()
        .filter_map(std::env::var_os)
        .map(std::path::PathBuf::from)
        .flat_map(|base| [base.join("Git").join("bin").join("bash.exe"), base.join("Programs").join("Git").join("bin").join("bash.exe")])
        .find(|path| path.is_file())
}

/// The WSL distributions installed, as `wsl -l -q` lists them.
fn wsl_distributions() -> Vec<String> {
    let output = crate::platform::command("wsl.exe").args(["-l", "-q"]).stdin(std::process::Stdio::null()).stderr(std::process::Stdio::null()).output();
    output.ok().filter(|o| o.status.success()).map(|o| distributions_in(&o.stdout)).unwrap_or_default()
}

/// The names in `wsl -l -q`'s output, which is UTF-16 (little-endian), one to a line; Docker Desktop's own aren't
/// shells to open.
fn distributions_in(bytes: &[u8]) -> Vec<String> {
    let units: Vec<u16> = bytes.as_chunks::<2>().0.iter().map(|pair| u16::from_le_bytes(*pair)).collect();
    String::from_utf16_lossy(&units)
        .lines()
        .map(|line| line.trim_matches(|c: char| c.is_whitespace() || c == '\u{feff}' || c == '\0').to_string())
        .filter(|name| !name.is_empty() && !name.starts_with("docker-desktop"))
        .collect()
}

/// The shell to start: PowerShell on Windows, and the person's own elsewhere.
fn shell() -> CommandBuilder {
    if cfg!(windows) {
        let mut command = CommandBuilder::new("powershell.exe");
        command.arg("-NoLogo");
        command
    } else {
        let shell = std::env::var("SHELL").ok().filter(|s| !s.is_empty()).unwrap_or_else(|| "/bin/sh".into());
        CommandBuilder::new(shell)
    }
}

impl Terminal {
    /// Starts a shell of the kind chosen, in a folder, on a screen of so many rows and columns.
    pub fn start_shell(shell: &Shell, folder: &Path, rows: u16, cols: u16) -> Result<Terminal, String> {
        let mut command = shell.command(folder);
        let wsl = match shell {
            Shell::Wsl(name) => Some(name.as_str()),
            _ => None,
        };
        crate::claude_ide::note_terminal(wsl);
        // A `claude` run in it finds Limoniter by itself.
        for (name, value) in crate::claude_ide::terminal_env(wsl.is_some(), std::env::var("WSLENV").ok().as_deref()) {
            command.env(name, value);
        }
        let mut terminal = Self::start_with(command, folder, rows, cols)?;
        terminal.shell = shell.clone();
        Ok(terminal)
    }

    pub fn start_with(mut command: CommandBuilder, folder: &Path, rows: u16, cols: u16) -> Result<Terminal, String> {
        let pty = native_pty_system();
        let pair = pty.openpty(PtySize { rows, cols, pixel_width: 0, pixel_height: 0 }).map_err(|e| crate::i18n::tf!("A terminal couldn't be made: {e}"))?;
        command.cwd(folder);
        command.env("TERM", "xterm-256color");
        let child = pair.slave.spawn_command(command).map_err(|e| crate::i18n::tf!("The shell couldn't be started: {e}"))?;
        drop(pair.slave);
        let mut reader = pair.master.try_clone_reader().map_err(|e| e.to_string())?;
        let writer: Arc<Mutex<Box<dyn Write + Send>>> = Arc::new(Mutex::new(pair.master.take_writer().map_err(|e| e.to_string())?));
        let parser = Arc::new(Mutex::new(vt100::Parser::new(rows, cols, 2000)));
        let (ended, changed) = (Arc::new(AtomicBool::new(false)), Arc::new(AtomicBool::new(true)));
        let (screen, done, fresh, answer) = (parser.clone(), ended.clone(), changed.clone(), writer.clone());
        std::thread::spawn(move || {
            let mut buffer = [0u8; 8192];
            loop {
                match reader.read(&mut buffer) {
                    Ok(0) | Err(_) => break,
                    Ok(n) => {
                        take_in(&screen, &answer, &buffer[..n]);
                        fresh.store(true, Ordering::Relaxed);
                    }
                }
            }
            done.store(true, Ordering::Relaxed);
            fresh.store(true, Ordering::Relaxed);
        });
        Ok(Terminal { parser, writer, backend: Backend::Pty { master: pair.master, child }, size: (rows, cols), ended, changed, folder: folder.to_path_buf(), shell: Shell::Usual, ssh: None, refusal: Arc::default(), learned: Arc::default() })
    }

    pub fn send(&mut self, bytes: &[u8]) {
        if let Ok(mut writer) = self.writer.lock() {
            let _ = writer.write_all(bytes);
            let _ = writer.flush();
        }
    }

    pub fn resize(&mut self, rows: u16, cols: u16) {
        if (rows, cols) == self.size || rows == 0 || cols == 0 {
            return;
        }
        self.size = (rows, cols);
        match &self.backend {
            Backend::Pty { master, .. } => {
                let _ = master.resize(PtySize { rows, cols, pixel_width: 0, pixel_height: 0 });
            }
            Backend::Ssh { to_server } => {
                let _ = to_server.send(ToServer::Size(rows, cols));
            }
        }
        if let Ok(mut parser) = self.parser.lock() {
            parser.screen_mut().set_size(rows, cols);
        }
    }

    pub fn ended(&self) -> bool {
        self.ended.load(Ordering::Relaxed)
    }

    /// What's on the screen, as text.
    #[cfg(all(test, unix))]
    pub fn text(&self) -> String {
        self.parser.lock().map(|p| p.screen().contents()).unwrap_or_default()
    }

    /// Whether the program running wants the cursor keys as an application's (vim and the like).
    fn application_cursor(&self) -> bool {
        self.parser.lock().is_ok_and(|p| p.screen().application_cursor())
    }
}

/// Puts what a program sent into the screen, and answers what it asked: where the cursor is, and what kind of
/// terminal this is. Windows' ConPTY asks as it starts, and shows nothing until it's answered.
fn take_in(screen: &Mutex<vt100::Parser>, answer: &Mutex<Box<dyn Write + Send>>, got: &[u8]) {
    let mut reply = Vec::new();
    if let Ok(mut parser) = screen.lock() {
        parser.process(got);
        if got.windows(4).any(|w| w == b"\x1b[6n") {
            let (row, col) = parser.screen().cursor_position();
            reply.extend(format!("\x1b[{};{}R", row + 1, col + 1).into_bytes());
        }
        if got.windows(3).any(|w| w == b"\x1b[c") || got.windows(4).any(|w| w == b"\x1b[0c") {
            reply.extend(b"\x1b[?1;2c");
        }
    }
    if !reply.is_empty()
        && let Ok(mut writer) = answer.lock()
    {
        let _ = writer.write_all(&reply);
        let _ = writer.flush();
    }
}

/// A word for a shell to take as one: in single quotes, with any quote in it closed, escaped and opened again.
pub fn shell_quote(text: &str) -> String {
    format!("'{}'", text.replace('\'', "'\\''"))
}

/// What to type to a server's shell to go to a folder. The space in front keeps it out of the history in the
/// shells that are asked to leave such lines out.
pub fn cd_command(folder: &str) -> String {
    format!(" cd -- {}\r", shell_quote(folder))
}

/// How an SSH session's thread is told to start.
struct SshStart {
    site: Site,
    secret: Secret,
    known: KnownHosts,
    folder: Option<String>,
    size: (u16, u16),
}

/// What the thread shares with its terminal.
struct SshShared {
    parser: Arc<Mutex<vt100::Parser>>,
    writer: Arc<Mutex<Box<dyn Write + Send>>>,
    ended: Arc<AtomicBool>,
    changed: Arc<AtomicBool>,
    refusal: Arc<Mutex<Option<Refused>>>,
    learned: Arc<Mutex<Option<Secret>>>,
}

impl SshShared {
    /// Says something on the screen, as a line of its own.
    fn say(&self, text: &str) {
        if let Ok(mut parser) = self.parser.lock() {
            parser.process(format!("{}\r\n", text.replace('\n', "\r\n")).as_bytes());
        }
        self.changed.store(true, Ordering::Relaxed);
    }

    fn finish(&self) {
        self.ended.store(true, Ordering::Relaxed);
        self.changed.store(true, Ordering::Relaxed);
    }
}

/// Reads what's typed, without showing it, up to Enter: for a password. None if it's given up with Ctrl+C, or
/// the terminal has gone. Size changes meanwhile are noted in `size`.
fn read_secret(from: &Receiver<ToServer>, size: &mut (u16, u16)) -> Option<String> {
    let mut typed = Vec::new();
    loop {
        match from.recv_timeout(std::time::Duration::from_millis(200)) {
            Ok(ToServer::Size(rows, cols)) => *size = (rows, cols),
            Ok(ToServer::Bytes(bytes)) => {
                // An escape sequence (an arrow key) isn't part of a password.
                for byte in bytes.into_iter().take_while(|b| *b != 0x1b) {
                    match byte {
                        b'\r' | b'\n' => return Some(String::from_utf8_lossy(&typed).into_owned()),
                        3 => return None,
                        8 | 0x7f => {
                            while typed.last().is_some_and(|b| b & 0xC0 == 0x80) {
                                typed.pop();
                            }
                            typed.pop();
                        }
                        b if b >= 0x20 => typed.push(b),
                        _ => {}
                    }
                }
            }
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => return None,
        }
    }
}

/// Tries again after the library says it would have to wait: it's been told not to.
fn waiting<T>(mut attempt: impl FnMut() -> std::io::Result<T>) -> std::io::Result<T> {
    loop {
        match attempt() {
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => std::thread::sleep(std::time::Duration::from_millis(2)),
            other => return other,
        }
    }
}

/// The whole life of an SSH session: connecting and signing in (asking for a password if what's kept isn't
/// accepted), then passing bytes between the server's shell and the screen until either end closes.
fn run_ssh(start: SshStart, from: &Receiver<ToServer>, shared: &SshShared) {
    let SshStart { site, mut secret, known, folder, mut size } = start;
    shared.say(&crate::i18n::tf!("Connecting to {}\u{2026}", site.host));
    let mut tries = 0;
    let session = loop {
        match remote::open_session(&site, &secret, &known) {
            Ok(session) => {
                if tries > 0 {
                    *shared.learned.lock().unwrap_or_else(std::sync::PoisonError::into_inner) = Some(secret.clone());
                }
                break session;
            }
            Err(Refused::SignIn(why)) if tries < 3 => {
                tries += 1;
                // What was tried first wasn't accepted, or there was nothing to try: ask.
                shared.say(&Refused::SignIn(why).describe(&site));
                let key = site.key_file.is_some();
                shared.say(&if key { crate::i18n::tf!("Passphrase for {}:", site.key_file.as_ref().map_or_else(String::new, |k| k.display().to_string())) } else { crate::i18n::tf!("Password for {}@{}:", site.user, site.host) });
                let Some(typed) = read_secret(from, &mut size) else {
                    shared.say(crate::i18n::tr("Not connected."));
                    return shared.finish();
                };
                secret = if key { Secret::KeyPassphrase(Some(typed)) } else { Secret::Password(typed) };
                shared.say(&crate::i18n::tf!("Connecting to {}\u{2026}", site.host));
            }
            Err(refused) => {
                shared.say(&refused.describe(&site));
                *shared.refusal.lock().unwrap_or_else(std::sync::PoisonError::into_inner) = Some(refused);
                return shared.finish();
            }
        }
    };
    let opened = (|| {
        let mut channel = session.channel_session()?;
        channel.request_pty("xterm-256color", None, Some((u32::from(size.1), u32::from(size.0), 0, 0)))?;
        channel.shell()?;
        Ok::<_, ssh2::Error>(channel)
    })();
    let mut channel = match opened {
        Ok(channel) => channel,
        Err(e) => {
            shared.say(&crate::i18n::tf!("{} wouldn't start a shell: {e}", site.host));
            return shared.finish();
        }
    };
    session.set_blocking(false);
    // What was typed while connecting is sent once there's a shell, after the move to the folder asked for.
    let mut waiting_to_go: Vec<u8> = folder.map(|folder| cd_command(&folder).into_bytes()).unwrap_or_default();
    let mut buffer = [0u8; 8192];
    'session: loop {
        loop {
            match from.try_recv() {
                Ok(ToServer::Bytes(bytes)) => waiting_to_go.extend(bytes),
                Ok(ToServer::Size(rows, cols)) => {
                    if waiting(|| channel.request_pty_size(u32::from(cols), u32::from(rows), None, None).map_err(std::io::Error::from)).is_err() {
                        break 'session;
                    }
                }
                Err(std::sync::mpsc::TryRecvError::Empty) => break,
                Err(std::sync::mpsc::TryRecvError::Disconnected) => return,
            }
        }
        let mut sent = &waiting_to_go[..];
        while !sent.is_empty() {
            match waiting(|| channel.write(sent)) {
                Ok(n) => sent = &sent[n..],
                Err(_) => break 'session,
            }
        }
        waiting_to_go.clear();
        match channel.read(&mut buffer) {
            Ok(0) => break,
            Ok(n) => {
                take_in(&shared.parser, &shared.writer, &buffer[..n]);
                shared.changed.store(true, Ordering::Relaxed);
            }
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => std::thread::sleep(std::time::Duration::from_millis(5)),
            Err(_) => break,
        }
    }
    shared.say(&crate::i18n::tf!("\nThe connection to {} has closed.", site.host));
    shared.finish();
}

impl Terminal {
    /// A shell on a server, over SSH: a terminal at once, which says it's connecting while a thread of its own
    /// does so, and signs in as `remote::open_session` does. `folder` is a folder on the server to go to.
    pub fn start_ssh(site: Site, secret: Secret, known: KnownHosts, folder: Option<String>, rows: u16, cols: u16) -> Terminal {
        let (to_server, from) = std::sync::mpsc::channel();
        let writer: Arc<Mutex<Box<dyn Write + Send>>> = Arc::new(Mutex::new(Box::new(Sending(to_server.clone()))));
        let shared = SshShared { parser: Arc::new(Mutex::new(vt100::Parser::new(rows, cols, 2000))), writer: writer.clone(), ended: Arc::default(), changed: Arc::new(AtomicBool::new(true)), refusal: Arc::default(), learned: Arc::default() };
        let terminal = Terminal {
            parser: shared.parser.clone(),
            writer,
            backend: Backend::Ssh { to_server },
            size: (rows, cols),
            ended: shared.ended.clone(),
            changed: shared.changed.clone(),
            folder: PathBuf::from(folder.clone().unwrap_or_default()),
            shell: Shell::Usual,
            ssh: Some((site.clone(), folder.clone())),
            refusal: shared.refusal.clone(),
            learned: shared.learned.clone(),
        };
        let start = SshStart { site, secret, known, folder, size: (rows, cols) };
        std::thread::spawn(move || {
            let caught = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| run_ssh(start, &from, &shared)));
            if caught.is_err() {
                shared.say(crate::i18n::tr("It failed unexpectedly."));
                shared.finish();
            }
        });
        terminal
    }

    /// What the tab is called: the server's address for a session on one, otherwise the folder's name.
    pub fn title(&self) -> String {
        match &self.ssh {
            Some((site, _)) => format!("{}@{}", site.user, site.host),
            None => self.folder.file_name().map_or_else(|| self.folder.display().to_string(), |n| n.to_string_lossy().into_owned()),
        }
    }

    /// The server's refusal of a connection, once, if there was one.
    pub fn take_refusal(&self) -> Option<Refused> {
        self.refusal.lock().unwrap_or_else(std::sync::PoisonError::into_inner).take()
    }

    /// A password the server accepted after it was typed here, once.
    pub fn take_learned(&self) -> Option<Secret> {
        self.learned.lock().unwrap_or_else(std::sync::PoisonError::into_inner).take()
    }
}

impl Drop for Terminal {
    fn drop(&mut self) {
        // An SSH session's thread ends when its sender is dropped, with the terminal.
        if let Backend::Pty { child, .. } = &mut self.backend {
            let _ = child.kill();
        }
    }
}

/// What a key sends to the terminal, if anything: Enter a carriage return, the arrows their escape sequences
/// (the application forms when asked for), Ctrl and a letter its control code.
pub fn key_bytes(key: Key, modifiers: Modifiers, application_cursor: bool) -> Option<Vec<u8>> {
    let arrow = |c: char| if application_cursor { format!("\x1bO{c}") } else { format!("\x1b[{c}") };
    let bytes = match key {
        Key::Enter => "\r".to_string(),
        Key::Backspace => "\x7f".to_string(),
        Key::Tab if modifiers.shift => "\x1b[Z".to_string(),
        Key::Tab => "\t".to_string(),
        Key::Escape => "\x1b".to_string(),
        Key::ArrowUp => arrow('A'),
        Key::ArrowDown => arrow('B'),
        Key::ArrowRight => arrow('C'),
        Key::ArrowLeft => arrow('D'),
        Key::Home => "\x1b[H".to_string(),
        Key::End => "\x1b[F".to_string(),
        Key::PageUp => "\x1b[5~".to_string(),
        Key::PageDown => "\x1b[6~".to_string(),
        Key::Delete => "\x1b[3~".to_string(),
        Key::Insert => "\x1b[2~".to_string(),
        _ if modifiers.ctrl && !modifiers.alt => {
            let name = key.name();
            let c = name.chars().next().filter(|_| name.len() == 1)?.to_ascii_lowercase();
            if c.is_ascii_lowercase() {
                return Some(vec![c as u8 - b'a' + 1]);
            }
            return None;
        }
        _ => return None,
    };
    Some(bytes.into_bytes())
}

/// An xterm colour as it's drawn: the 16 named ones, the 6x6x6 cube, the greys, or red, green and blue.
pub(crate) fn colour(c: vt100::Color, default: Color32) -> Color32 {
    const NAMED: [u32; 16] = [0x1E1E1E, 0xCD3131, 0x0DBC79, 0xE5E510, 0x2472C8, 0xBC3FBC, 0x11A8CD, 0xE5E5E5, 0x666666, 0xF14C4C, 0x23D18B, 0xF5F543, 0x3B8EEA, 0xD670D6, 0x29B8DB, 0xFFFFFF];
    match c {
        vt100::Color::Default => default,
        vt100::Color::Idx(i) if i < 16 => {
            let v = NAMED[usize::from(i)];
            Color32::from_rgb((v >> 16) as u8, (v >> 8) as u8, v as u8)
        }
        vt100::Color::Idx(i) if i < 232 => {
            let n = i - 16;
            let level = |v: u8| if v == 0 { 0 } else { 55 + v * 40 };
            Color32::from_rgb(level(n / 36), level((n / 6) % 6), level(n % 6))
        }
        vt100::Color::Idx(i) => {
            let g = 8 + (i - 232) * 10;
            Color32::from_rgb(g, g, g)
        }
        vt100::Color::Rgb(r, g, b) => Color32::from_rgb(r, g, b),
    }
}

/// Draws the terminal to fill the space, and passes it the keyboard while it has the focus. Clicking it gives it
/// the focus.
pub fn draw(ui: &mut Ui, terminal: &mut Terminal, font_size: f32) {
    let font = FontId::monospace(font_size);
    let cell = vec2(ui.fonts_mut(|f| f.glyph_width(&font, 'M')), ui.fonts_mut(|f| f.row_height(&font)));
    let area = ui.available_rect_before_wrap();
    ui.allocate_rect(area, Sense::hover());
    let rows = ((area.height() / cell.y).floor() as u16).max(2);
    let cols = ((area.width() / cell.x).floor() as u16).max(10);
    terminal.resize(rows, cols);
    let id = ui.scope_id().with("terminal");
    let response = ui.interact(area, id, Sense::click());
    if response.clicked() {
        response.request_focus();
    }
    let focused = response.has_focus();
    if focused {
        // The terminal takes Tab, the arrows and Escape, as a terminal does.
        ui.memory_mut(|m| m.set_focus_lock_filter(id, egui::EventFilter { tab: true, horizontal_arrows: true, vertical_arrows: true, escape: true }));
        let events = ui.input(|i| i.events.clone());
        let application = terminal.application_cursor();
        for event in events {
            match event {
                egui::Event::Text(text) => terminal.send(text.as_bytes()),
                egui::Event::Paste(text) => terminal.send(text.replace("\r\n", "\r").replace('\n', "\r").as_bytes()),
                egui::Event::Key { key, pressed: true, modifiers, .. } => {
                    if let Some(bytes) = key_bytes(key, modifiers, application) {
                        terminal.send(&bytes);
                    }
                }
                _ => {}
            }
        }
    }
    let painter = ui.painter_at(area);
    let background = Color32::from_rgb(0x16, 0x16, 0x16);
    painter.rect_filled(area, 0.0, background);
    let foreground = Color32::from_rgb(0xD4, 0xD4, 0xD4);
    let Ok(parser) = terminal.parser.lock() else {
        return;
    };
    let screen = parser.screen();
    for row in 0..rows {
        for col in 0..cols {
            let Some(c) = screen.cell(row, col) else {
                continue;
            };
            let (mut fg, mut bg) = (colour(c.fgcolor(), foreground), colour(c.bgcolor(), background));
            if c.inverse() {
                std::mem::swap(&mut fg, &mut bg);
            }
            let at = pos2(area.min.x + f32::from(col) * cell.x, area.min.y + f32::from(row) * cell.y);
            if bg != background {
                painter.rect_filled(Rect::from_min_size(at, cell), 0.0, bg);
            }
            if c.has_contents() {
                painter.text(at, egui::Align2::LEFT_TOP, c.contents(), font.clone(), fg);
            }
        }
    }
    if !screen.hide_cursor() {
        let (row, col) = screen.cursor_position();
        let at = pos2(area.min.x + f32::from(col) * cell.x, area.min.y + f32::from(row) * cell.y);
        let shape = Rect::from_min_size(at, cell);
        if focused {
            painter.rect_filled(shape, 0.0, foreground.gamma_multiply(0.7));
        } else {
            painter.rect_stroke(shape, 0.0, egui::Stroke::new(1.0, foreground.gamma_multiply(0.6)), egui::StrokeKind::Inside);
        }
    }
    if terminal.ended() {
        painter.text(pos2(area.min.x + 4.0, area.max.y - cell.y - 2.0), egui::Align2::LEFT_TOP, "The shell has ended. Close the terminal and open it again for another.", font, Color32::from_rgb(0xE5, 0xE5, 0x10));
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn wsl_s_list_is_read_and_shells_are_kept_by_id() {
        // UTF-16, as wsl -l -q writes it, with Docker Desktop's own left out.
        let text: Vec<u8> = "Ubuntu\r\ndocker-desktop\r\nDebian\r\n".encode_utf16().flat_map(u16::to_le_bytes).collect();
        assert_eq!(distributions_in(&text), ["Ubuntu", "Debian"]);
        for shell in [Shell::Usual, Shell::Cmd, Shell::Wsl("Ubuntu".into())] {
            assert_eq!(Shell::from_id(&shell.id()), shell);
        }
        assert_eq!(Shell::from_id("wsl:"), Shell::Usual);
        assert!(!shells().is_empty());
    }

    use super::*;

    #[test]
    fn keys_send_what_a_terminal_sends() {
        assert_eq!(key_bytes(Key::Enter, Modifiers::NONE, false), Some(b"\r".to_vec()));
        assert_eq!(key_bytes(Key::ArrowUp, Modifiers::NONE, false), Some(b"\x1b[A".to_vec()));
        assert_eq!(key_bytes(Key::ArrowUp, Modifiers::NONE, true), Some(b"\x1bOA".to_vec()), "vim's way");
        assert_eq!(key_bytes(Key::C, Modifiers::CTRL, false), Some(vec![3]), "Ctrl+C");
        assert_eq!(key_bytes(Key::A, Modifiers::NONE, false), None, "letters come as text");
        assert_eq!(colour(vt100::Color::Idx(196), Color32::BLACK), Color32::from_rgb(255, 0, 0));
    }

    /// A real shell on a pseudo-terminal: what's typed runs, and its output, colours and all, is on the screen.
    #[cfg(unix)]
    #[test]
    fn a_shell_runs_in_it() {
        let mut command = CommandBuilder::new("/bin/sh");
        command.env("PS1", "$ ");
        let mut terminal = Terminal::start_with(command, &std::env::temp_dir(), 24, 80).unwrap();
        terminal.send(b"printf '\\033[31mred\\033[0m %s\\n' $((6*7))\r");
        let began = std::time::Instant::now();
        while !terminal.text().contains("red 42") && began.elapsed() < std::time::Duration::from_secs(10) {
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        assert!(terminal.text().contains("red 42"), "{}", terminal.text());
        let red = {
            let parser = terminal.parser.lock().unwrap();
            let screen = parser.screen();
            (0..24).find_map(|row| (0..80).find(|col| screen.cell(row, *col).is_some_and(|c| c.contents() == "r" && c.fgcolor() == vt100::Color::Idx(1))))
        };
        assert!(red.is_some(), "red is red");
        terminal.resize(30, 100);
        terminal.send(b"exit\r");
        let began = std::time::Instant::now();
        while !terminal.ended() && began.elapsed() < std::time::Duration::from_secs(10) {
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        assert!(terminal.ended());
    }

    #[test]
    fn folders_are_quoted_for_the_servers_shell() {
        assert_eq!(shell_quote("/srv/site"), "'/srv/site'");
        assert_eq!(shell_quote("/tmp/it's here"), "'/tmp/it'\\''s here'");
        assert_eq!(cd_command("/srv/my site"), " cd -- '/srv/my site'\r");
        // A folder that looks like a command, or an option, or two, stays a folder.
        assert_eq!(cd_command("-x; rm -rf ~"), " cd -- '-x; rm -rf ~'\r");
        assert_eq!(cd_command("$(whoami)`id`"), " cd -- '$(whoami)`id`'\r");
    }

    #[test]
    fn a_typed_password_is_read_without_being_shown() {
        let (to, from) = std::sync::mpsc::channel();
        to.send(ToServer::Bytes(b"pas".to_vec())).unwrap();
        to.send(ToServer::Size(30, 100)).unwrap();
        to.send(ToServer::Bytes(b"x\x7fs\r".to_vec())).unwrap();
        let mut size = (12, 80);
        assert_eq!(read_secret(&from, &mut size), Some("pass".to_string()));
        assert_eq!(size, (30, 100));
        to.send(ToServer::Bytes(vec![3])).unwrap();
        assert_eq!(read_secret(&from, &mut size), None, "Ctrl+C gives up");
    }

    /// A server of our own for the SSH tests, which need `sshd`: run only when LIMONITER_SSHD_TEST is set, and
    /// on Unix. It listens on this computer only, with its own keys and configuration in a folder of its own, and
    /// is stopped when this is dropped.
    #[cfg(unix)]
    struct TestServer {
        site: Site,
        known: KnownHosts,
        process: std::process::Child,
        folder: PathBuf,
    }

    #[cfg(unix)]
    impl TestServer {
        fn start() -> Option<TestServer> {
            std::env::var_os("LIMONITER_SSHD_TEST")?;
            let folder = std::env::temp_dir().join(format!("limoniter-sshd-{}-{:?}", std::process::id(), std::thread::current().id()).replace(['(', ')'], ""));
            let _ = std::fs::remove_dir_all(&folder);
            std::fs::create_dir_all(&folder).unwrap();
            let keygen = |name: &str| {
                let key = folder.join(name);
                let made = crate::platform::command("ssh-keygen").args(["-q", "-t", "ed25519", "-N", "", "-f"]).arg(&key).status().unwrap();
                assert!(made.success());
                key
            };
            let (host, user_key) = (keygen("host"), keygen("user"));
            std::fs::copy(folder.join("user.pub"), folder.join("authorized")).unwrap();
            let port = std::net::TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port();
            let config = format!("Port {port}\nListenAddress 127.0.0.1\nHostKey {}\nPidFile {}\nAuthorizedKeysFile {}\nPasswordAuthentication yes\nKbdInteractiveAuthentication no\nUsePAM no\nPermitRootLogin yes\nStrictModes no\n", host.display(), folder.join("pid").display(), folder.join("authorized").display());
            std::fs::write(folder.join("config"), config).unwrap();
            let process = crate::platform::command("/usr/sbin/sshd").arg("-D").arg("-f").arg(folder.join("config")).stdin(std::process::Stdio::null()).stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null()).spawn().unwrap();
            // The server's key, as the person would be shown it.
            let listed = crate::platform::command("ssh-keygen").arg("-l").arg("-f").arg(folder.join("host.pub")).output().unwrap();
            let print = String::from_utf8_lossy(&listed.stdout).split_whitespace().nth(1).unwrap().to_string();
            let user = String::from_utf8(crate::platform::command("id").arg("-un").output().unwrap().stdout).unwrap().trim().to_string();
            let site = Site { host: "127.0.0.1".into(), port, user, key_file: Some(user_key), ..Site::default() };
            let mut known = KnownHosts::default();
            known.trust(&site, &print);
            // Ready when it takes connections.
            let began = std::time::Instant::now();
            while std::net::TcpStream::connect(("127.0.0.1", port)).is_err() && began.elapsed() < std::time::Duration::from_secs(10) {
                std::thread::sleep(std::time::Duration::from_millis(50));
            }
            Some(TestServer { site, known, process, folder })
        }
    }

    #[cfg(unix)]
    impl Drop for TestServer {
        fn drop(&mut self) {
            let _ = self.process.kill();
            let _ = self.process.wait();
            let _ = std::fs::remove_dir_all(&self.folder);
        }
    }

    #[cfg(unix)]
    fn wait_for(terminal: &Terminal, wanted: &str) -> bool {
        let began = std::time::Instant::now();
        while !terminal.text().contains(wanted) && began.elapsed() < std::time::Duration::from_secs(15) {
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        terminal.text().contains(wanted)
    }

    /// A session on a server: it connects, goes to the folder asked for (quotes and all), runs what's typed,
    /// tells the server when the terminal is resized, and says so when the shell ends.
    #[cfg(unix)]
    #[test]
    fn an_ssh_session_runs_commands_and_is_resized() {
        let Some(server) = TestServer::start() else { return };
        let odd = std::env::temp_dir().join(format!("it's here {}", std::process::id()));
        std::fs::create_dir_all(&odd).unwrap();
        let mut terminal = Terminal::start_ssh(server.site.clone(), Secret::KeyPassphrase(None), server.known.clone(), Some(odd.display().to_string()), 24, 80);
        assert!(wait_for(&terminal, "Connecting to 127.0.0.1"), "{}", terminal.text());
        terminal.send(b"echo ab$((6*7))cd; pwd\r");
        assert!(wait_for(&terminal, "ab42cd"), "{}", terminal.text());
        assert!(terminal.text().contains(&format!("{}\n", odd.display())), "it started in the folder: {}", terminal.text());
        terminal.resize(30, 100);
        terminal.send(b"stty size\r");
        assert!(wait_for(&terminal, "30 100"), "{}", terminal.text());
        assert!(!terminal.ended());
        terminal.send(b"exit\r");
        assert!(wait_for(&terminal, "has closed"), "{}", terminal.text());
        assert!(terminal.ended());
        let _ = std::fs::remove_dir_all(&odd);
    }

    #[cfg(unix)]
    #[test]
    fn an_ssh_session_refuses_a_changed_key_and_asks_about_a_new_one() {
        let Some(server) = TestServer::start() else { return };
        let mut changed = KnownHosts::default();
        changed.trust(&server.site, "SHA256:notTheKeyTheServerHas");
        let terminal = Terminal::start_ssh(server.site.clone(), Secret::KeyPassphrase(None), changed, None, 24, 80);
        assert!(wait_for(&terminal, "isn't the one it had before"), "{}", terminal.text());
        assert!(matches!(terminal.take_refusal(), Some(Refused::HostKeyChanged { .. })));
        assert!(terminal.ended(), "nothing is signed in to");
        let terminal = Terminal::start_ssh(server.site.clone(), Secret::KeyPassphrase(None), KnownHosts::default(), None, 24, 80);
        assert!(wait_for(&terminal, "first time"), "{}", terminal.text());
        assert!(matches!(terminal.take_refusal(), Some(Refused::UnknownHost(print)) if print.starts_with("SHA256:")));
    }

    /// A password the server doesn't accept is said so, and asked for again, without showing what's typed.
    #[cfg(unix)]
    #[test]
    fn an_ssh_session_says_a_wrong_password_is_wrong() {
        let Some(mut server) = TestServer::start() else { return };
        server.site.key_file = None;
        let mut terminal = Terminal::start_ssh(server.site.clone(), Secret::Password("not the password".into()), server.known.clone(), None, 24, 80);
        assert!(wait_for(&terminal, "didn't accept the sign-in"), "{}", terminal.text());
        assert!(wait_for(&terminal, "Password for"), "{}", terminal.text());
        terminal.send(b"still wrong\r");
        let began = std::time::Instant::now();
        while terminal.text().matches("Password for").count() < 2 && began.elapsed() < std::time::Duration::from_secs(15) {
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        assert_eq!(terminal.text().matches("Password for").count(), 2, "asked again: {}", terminal.text());
        assert!(!terminal.text().contains("still wrong"), "what's typed isn't shown");
        terminal.send(&[3]);
        assert!(wait_for(&terminal, "Not connected"), "{}", terminal.text());
        assert!(terminal.ended());
    }
}
