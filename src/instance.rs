//! One Limoniter window for files opened from Explorer. A window listens on this computer only, and a second
//! Limoniter started with files hands them to it and closes, so they open as tabs in the window already there.
//!
//! The window writes where it's listening, with a secret, into a file beside the settings. Only something that can
//! read that file, which is this Windows user, can hand it files. Anything wrong at all, and the second Limoniter
//! simply opens its own window, as it always used to.

use sha2::{Digest, Sha256};
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{IpAddr, Ipv4Addr, SocketAddr, TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::sync::mpsc::{self, Receiver};
use std::time::Duration;

const FILE_NAME: &str = "window.txt";
const GREETING: &str = "LIMONITER-OPEN-1";
const GREETING_2: &str = "LIMONITER-OPEN-2";
const MOST_REQUEST_BYTES: u64 = 256 * 1024;
const WAIT: Duration = Duration::from_secs(3);

/// One file to open, and where in it to start (counting from 1).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Opening {
    pub path: PathBuf,
    pub line: Option<usize>,
    pub column: Option<usize>,
}

/// What a second Limoniter asks of the window: these files, and perhaps to be told when they've been closed.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Request {
    pub files: Vec<Opening>,
    pub wait: bool,
}

/// What the window is given: the files, and the means of saying they've all been closed.
pub struct Handed {
    pub files: Vec<Opening>,
    pub waiter: Option<Waiter>,
}

/// The other end of a `--wait`. Telling it, or letting it drop, lets the waiting program go.
pub struct Waiter {
    stream: Option<TcpStream>,
}

impl Waiter {
    /// For a window that was started to wait for its own files: nothing to tell, it just closes.
    pub fn own() -> Self {
        Self { stream: None }
    }

    pub fn is_own(&self) -> bool {
        self.stream.is_none()
    }

    pub fn finish(self) {
        if let Some(mut stream) = self.stream {
            let _ = stream.write_all(b"CLOSED\n");
        }
    }
}

/// The text of a request. Plain ones use the first greeting, which every Limoniter since files were first handed
/// over understands; only a request that wants more (a place in the file, waiting) says it's the second, so an
/// older window answers "no" to it and the new program simply opens its own window.
fn request(token: &str, request: &Request) -> String {
    let newer = request.wait || request.files.iter().any(|file| file.line.is_some() || file.column.is_some());
    let mut text = format!("{}\n{token}\n", if newer { GREETING_2 } else { GREETING });
    if request.wait {
        text += "@wait\n";
    }
    for file in &request.files {
        if let Some(line) = file.line {
            text += &format!("@line {line}\n");
        }
        if let Some(column) = file.column {
            text += &format!("@column {column}\n");
        }
        text += &file.path.display().to_string();
        text.push('\n');
    }
    text.push('\n');
    text
}

/// What's asked in a request, if it's a proper one with the right secret.
fn request_in(text: &str, token: &str) -> Option<Request> {
    let mut lines = text.split('\n').map(|line| line.trim_end_matches('\r'));
    let newer = match lines.next()? {
        GREETING => false,
        GREETING_2 => true,
        _ => return None,
    };
    if lines.next()? != token {
        return None;
    }
    let mut found = Request::default();
    let (mut line, mut column) = (None, None);
    for text in lines.take_while(|line| !line.is_empty()) {
        // Options come before the file they're for. Only the second greeting has them.
        if let Some(option) = text.strip_prefix('@').filter(|_| newer) {
            let (name, value) = option.split_once(' ').unwrap_or((option, ""));
            match name {
                "wait" => found.wait = true,
                "line" => line = value.trim().parse().ok().filter(|n| *n >= 1),
                "column" => column = value.trim().parse().ok().filter(|n| *n >= 1),
                _ => {}
            }
            continue;
        }
        let path = PathBuf::from(text);
        if path.is_absolute() || is_address(&path) {
            found.files.push(Opening { path, line: line.take(), column: column.take() });
        }
    }
    (!found.files.is_empty()).then_some(found)
}

/// How a window is reached: its port and its secret.
fn parse_target(text: &str) -> Option<(u16, String)> {
    let mut parts = text.split_whitespace();
    let port = parts.next()?.parse::<u16>().ok()?;
    Some((port, parts.next()?.to_string()))
}

/// The windows to try, best first: the one this program was started from, if it was run in a Limoniter terminal,
/// then the one that last wrote its note.
fn targets(folder: &Path, from_terminal: Option<&str>) -> Vec<(u16, String)> {
    let mut found = Vec::new();
    // The terminal gives "port:secret".
    if let Some(target) = from_terminal.and_then(|text| parse_target(&text.replace(':', " "))) {
        found.push(target);
    }
    if let Some(target) = std::fs::read_to_string(folder.join(FILE_NAME)).ok().and_then(|note| parse_target(&note))
        && !found.contains(&target)
    {
        found.push(target);
    }
    found
}

/// A window that took the files, which a `--wait` can go on listening to until they're closed.
pub struct Taken(BufReader<TcpStream>);

impl Taken {
    /// Holds until the window says every file asked for has been closed (or the window itself has gone).
    pub fn wait_until_closed(mut self) {
        let _ = self.0.get_ref().set_read_timeout(None);
        let mut line = String::new();
        // Anything at all, or the end of the conversation, means it's over.
        let _ = self.0.read_line(&mut line);
    }
}

/// Tries to give the files to a window that's already open: this program's own terminal's if it has one, else the
/// latest. Some means it took them, and this one can close (or wait).
pub fn hand_off(folder: &Path, asked: &Request) -> Option<Taken> {
    let from_terminal = std::env::var(WINDOW_VARIABLE).ok();
    hand_off_to(&targets(folder, from_terminal.as_deref()), asked)
}

fn hand_off_to(windows: &[(u16, String)], asked: &Request) -> Option<Taken> {
    // A file on a server goes as its address; anything else, as its whole path.
    let files = asked.files.iter().map(|file| Opening { path: if is_address(&file.path) { file.path.clone() } else { std::path::absolute(&file.path).unwrap_or_else(|_| file.path.clone()) }, ..file.clone() }).collect();
    let asked = Request { files, wait: asked.wait };
    // Windows only lets the program in front bring another to the front. This one was just started from Explorer,
    // so it's in front, and it lets the window it's handing the files to come forward.
    #[cfg(windows)]
    // SAFETY: a plain call with no pointers.
    unsafe {
        let _ = windows::Win32::UI::WindowsAndMessaging::AllowSetForegroundWindow(windows::Win32::UI::WindowsAndMessaging::ASFW_ANY);
    }
    for (port, token) in windows {
        let address = SocketAddr::from((Ipv4Addr::LOCALHOST, *port));
        let Ok(mut stream) = TcpStream::connect_timeout(&address, Duration::from_millis(600)) else {
            continue;
        };
        let _ = stream.set_read_timeout(Some(WAIT));
        let _ = stream.set_write_timeout(Some(WAIT));
        if stream.write_all(request(token, &asked).as_bytes()).is_err() {
            continue;
        }
        let mut reader = BufReader::new(stream);
        let mut answer = String::new();
        if reader.read_line(&mut answer).is_ok() && answer.trim() == "OK" {
            return Some(Taken(reader));
        }
    }
    None
}

/// What a Limoniter terminal gives its programs, so that `limoniter` there reaches that window: "port:secret".
pub const WINDOW_VARIABLE: &str = "LIMONITER_WINDOW";

static OWN_ADDRESS: Mutex<Option<String>> = Mutex::new(None);

/// This window's own "port:secret", once it's listening.
pub fn own_address() -> Option<String> {
    OWN_ADDRESS.lock().ok().and_then(|own| own.clone())
}

/// Adds what a shell in Limoniter's terminal needs to the environment it's being given: the way to reach this
/// window, named in WSLENV too for a WSL shell so that WSL passes it on; and, outside WSL, the folder the program
/// is in on the front of PATH if it isn't on it yet, so `limoniter` runs there.
pub fn terminal_environment(env: &mut Vec<(String, String)>, wsl: bool, wslenv: Option<&str>, path: Option<&std::ffi::OsStr>, program_folder: Option<&Path>) {
    if let Some(address) = own_address() {
        env.push((WINDOW_VARIABLE.to_string(), address));
        if wsl {
            // WSLENV may already have been set for something else in this list.
            let at = env.iter().position(|(name, _)| name == "WSLENV");
            let existing = at.map_or_else(|| wslenv.unwrap_or("").to_string(), |at| env[at].1.clone());
            if !existing.split(':').any(|name| name.split('/').next() == Some(WINDOW_VARIABLE)) {
                let all = if existing.is_empty() { WINDOW_VARIABLE.to_string() } else { format!("{existing}:{WINDOW_VARIABLE}") };
                match at {
                    Some(at) => env[at].1 = all,
                    None => env.push(("WSLENV".to_string(), all)),
                }
            }
        }
    }
    if !wsl
        && let (Some(folder), Some(path)) = (program_folder, path)
    {
        let mut folders: Vec<PathBuf> = std::env::split_paths(path).collect();
        if !folders.iter().any(|one| one == folder) {
            folders.insert(0, folder.to_path_buf());
            if let Ok(joined) = std::env::join_paths(folders) {
                env.push(("PATH".to_string(), joined.to_string_lossy().into_owned()));
            }
        }
    }
}

/// A secret that can't be guessed from outside: when, where in memory, and which process.
fn new_token() -> String {
    let marker = 0u8;
    let seed = format!("{:?} {} {:p}", std::time::SystemTime::now(), std::process::id(), &raw const marker);
    Sha256::digest(seed.as_bytes()).iter().take(16).map(|byte| format!("{byte:02x}")).collect()
}

/// A window listening for files.
pub struct Listening {
    pub files: Receiver<Handed>,
    /// Where this window is reached, as "port:secret".
    #[cfg_attr(not(test), allow(dead_code))] // Tests reach the window with it.
    pub address: String,
    note_path: PathBuf,
    note: String,
}

impl Listening {
    /// Takes away the note, as the window closes, unless a newer window has written its own.
    pub fn stop(&self) {
        if std::fs::read_to_string(&self.note_path).is_ok_and(|now| now == self.note) {
            let _ = std::fs::remove_file(&self.note_path);
        }
    }
}

/// Starts listening, and says where. Files handed over arrive on the receiver, and the window is asked to draw.
/// It listens on this computer only, and also on `also_on` at the same port: on Windows, the address WSL reaches
/// it by, for the `limoniter` command in a distribution where it can't run the program itself.
pub fn listen(folder: &Path, wake: impl Fn() + Send + Sync + 'static, also_on: &[IpAddr]) -> Option<Listening> {
    let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).ok()?;
    let port = listener.local_addr().ok()?.port();
    let token = new_token();
    std::fs::create_dir_all(folder).ok()?;
    let note = format!("{port} {token}\n");
    if let Ok(mut own) = OWN_ADDRESS.lock() {
        *own = Some(format!("{port}:{token}"));
    }
    let note_path = folder.join(FILE_NAME);
    crate::codec::write_file(&note_path, note.as_bytes()).ok()?;
    let (sender, receiver) = mpsc::channel();
    let wake: Arc<dyn Fn() + Send + Sync> = Arc::new(wake);
    let mut listeners = vec![listener];
    // Another address that can't be had at that port is simply not offered.
    listeners.extend(also_on.iter().filter_map(|address| TcpListener::bind((*address, port)).ok()));
    for listener in listeners {
        let (token, sender, wake) = (token.clone(), sender.clone(), wake.clone());
        std::thread::Builder::new().name("limoniter-window".into()).spawn(move || serve(&listener, &token, &sender, &*wake)).ok()?;
    }
    Some(Listening { files: receiver, address: format!("{port}:{token}"), note_path, note })
}

/// Answers what comes in on one address.
fn serve(listener: &TcpListener, token: &str, sender: &mpsc::Sender<Handed>, wake: &(dyn Fn() + Send + Sync)) {
    for stream in listener.incoming().flatten() {
        let _ = stream.set_read_timeout(Some(WAIT));
        let mut text = String::new();
        let mut reader = BufReader::new((&stream).take(MOST_REQUEST_BYTES));
        // The request ends with a blank line.
        loop {
            let mut line = String::new();
            match reader.read_line(&mut line) {
                Ok(0) | Err(_) => break,
                Ok(_) => {
                    let blank = line.trim().is_empty();
                    text += &line;
                    if blank {
                        break;
                    }
                }
            }
        }
        let Some(asked) = request_in(&text, token) else {
            let _ = (&stream).write_all(b"NO\n");
            continue;
        };
        let waiter = if asked.wait { stream.try_clone().ok().map(|stream| Waiter { stream: Some(stream) }) } else { None };
        if asked.wait && waiter.is_none() {
            let _ = (&stream).write_all(b"NO\n");
            continue;
        }
        // The answer goes before the window hears of it, so a quick "closed" can't arrive ahead of it.
        let _ = (&stream).write_all(b"OK\n");
        if sender.send(Handed { files: asked.files, waiter }).is_err() {
            return;
        }
        wake();
    }
}

/// Whether a "file" is a server's address (sftp://user@host/path, or ftp://), passed on as it is.
fn is_address(path: &Path) -> bool {
    path.to_str().is_some_and(crate::remote::is_address)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn folder(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("limoniter-window-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// A full path in the style of this computer.
    fn full(name: &str) -> PathBuf {
        if cfg!(windows) { PathBuf::from(format!(r"C:\a\{name}")) } else { PathBuf::from(format!("/a/{name}")) }
    }

    fn plain(files: Vec<PathBuf>) -> Request {
        Request { files: files.into_iter().map(|path| Opening { path, line: None, column: None }).collect(), wait: false }
    }

    #[test]
    fn a_request_reads_back_and_needs_the_secret() {
        let files = if cfg!(windows) { vec![PathBuf::from(r"C:\a b\one.txt"), PathBuf::from(r"D:\two.md")] } else { vec![PathBuf::from("/a b/one.txt"), PathBuf::from("/two.md")] };
        let asked = plain(files);
        let text = request("secret", &asked);
        assert!(text.starts_with("LIMONITER-OPEN-1\n"), "a plain request is one every Limoniter understands");
        assert_eq!(request_in(&text, "secret"), Some(asked));
        assert_eq!(request_in(&text, "wrong"), None);
        assert_eq!(request_in("nonsense\n", "secret"), None);
        assert_eq!(request_in(&request("secret", &plain(vec![PathBuf::from("relative.txt")])), "secret"), None, "only full paths");
    }

    #[test]
    fn a_request_from_before_places_and_waiting_is_still_read() {
        // Exactly as the first version wrote it.
        let old = format!("LIMONITER-OPEN-1\nsecret\n{}\n{}\n\n", full("one.txt").display(), full("two.txt").display());
        assert_eq!(request_in(&old, "secret"), Some(plain(vec![full("one.txt"), full("two.txt")])));
        // An option in it is a file name, which isn't a full path, so it's dropped.
        assert_eq!(request_in(&format!("LIMONITER-OPEN-1\nsecret\n@wait\n{}\n\n", full("one.txt").display()), "secret"), Some(plain(vec![full("one.txt")])));
    }

    #[test]
    fn places_and_waiting_ride_along_and_name_the_second_greeting() {
        let asked = Request { files: vec![Opening { path: full("one.txt"), line: Some(12), column: Some(5) }, Opening { path: full("two.txt"), line: None, column: None }, Opening { path: full("three.txt"), line: Some(3), column: None }], wait: true };
        let text = request("secret", &asked);
        assert!(text.starts_with("LIMONITER-OPEN-2\n"), "an older window refuses this, and the new program opens its own");
        assert_eq!(request_in(&text, "secret"), Some(asked));
        // A place belongs to the one file after it only.
        let two = request_in(&format!("LIMONITER-OPEN-2\nsecret\n@line 7\n{}\n{}\n\n", full("x").display(), full("y").display()), "secret").unwrap();
        assert_eq!((two.files[0].line, two.files[1].line, two.wait), (Some(7), None, false));
        assert_eq!(request_in(&format!("LIMONITER-OPEN-2\nsecret\n@line 0\n@line x\n{}\n\n", full("x").display()), "secret").unwrap().files[0].line, None, "nonsense is no place");
    }

    #[test]
    fn files_handed_over_arrive_in_the_window_already_open() {
        let dir = folder("hand");
        let listening = listen(&dir, || {}, &[]).expect("it listens");
        let file = dir.join("opened.txt");
        assert!(hand_off(&dir, &plain(vec![file.clone()])).is_some());
        let handed = listening.files.recv_timeout(Duration::from_secs(5)).unwrap();
        assert_eq!(handed.files, plain(vec![file]).files);
        assert!(handed.waiter.is_none());
        listening.stop();
        assert!(!dir.join(FILE_NAME).exists(), "the note goes when the window closes");
        assert!(hand_off(&dir, &plain(vec![dir.join("x.txt")])).is_none());
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn a_line_and_column_arrive_with_the_file() {
        let dir = folder("place");
        let listening = listen(&dir, || {}, &[]).unwrap();
        let asked = Request { files: vec![Opening { path: dir.join("a.rs"), line: Some(40), column: Some(2) }], wait: false };
        assert!(hand_off(&dir, &asked).is_some());
        assert_eq!(listening.files.recv_timeout(Duration::from_secs(5)).unwrap().files, asked.files);
        listening.stop();
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn waiting_holds_until_the_window_says_the_files_are_closed() {
        let dir = folder("wait");
        let listening = listen(&dir, || {}, &[]).unwrap();
        let asked = Request { files: vec![Opening { path: dir.join("a.rs"), line: None, column: None }], wait: true };
        let taken = hand_off(&dir, &asked).expect("taken");
        let released = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let flag = released.clone();
        let waiting = std::thread::spawn(move || {
            taken.wait_until_closed();
            flag.load(std::sync::atomic::Ordering::SeqCst)
        });
        let handed = listening.files.recv_timeout(Duration::from_secs(5)).unwrap();
        std::thread::sleep(Duration::from_millis(300));
        assert!(!waiting.is_finished(), "still holding while the files are open");
        released.store(true, std::sync::atomic::Ordering::SeqCst);
        handed.waiter.expect("a waiter").finish();
        assert!(waiting.join().unwrap(), "let go once told");
        // A window that goes away lets the waiting program go too.
        let taken = hand_off(&dir, &asked).expect("taken again");
        let handed = listening.files.recv_timeout(Duration::from_secs(5)).unwrap();
        drop(handed);
        taken.wait_until_closed();
        listening.stop();
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn the_terminals_own_window_is_tried_before_the_latest_one() {
        let dir = folder("own");
        std::fs::write(dir.join(FILE_NAME), "1111 latest\n").unwrap();
        assert_eq!(targets(&dir, Some("2222:mine")), [(2222, "mine".to_string()), (1111, "latest".to_string())]);
        assert_eq!(targets(&dir, None), [(1111, "latest".to_string())]);
        assert_eq!(targets(&dir, Some("garbage")), [(1111, "latest".to_string())]);
        // Two windows, the first one's terminal: its files go to it, whatever window.txt says.
        let first = folder("own-first");
        let second = folder("own-second");
        let one = listen(&first, || {}, &[]).unwrap();
        let first_address = one.address.clone();
        let two = listen(&second, || {}, &[]).unwrap();
        let (port, token) = parse_target(&first_address.replace(':', " ")).unwrap();
        let file = first.join("x.txt");
        assert!(hand_off_to(&[(port, token), (1, "no".into())], &plain(vec![file.clone()])).is_some());
        assert_eq!(one.files.recv_timeout(Duration::from_secs(5)).unwrap().files[0].path, file);
        assert!(two.files.try_recv().is_err(), "not the window that wrote its note last");
        // Its address if its window has gone: the other is tried.
        let (port2, token2) = parse_target(&std::fs::read_to_string(second.join(FILE_NAME)).unwrap()).unwrap();
        assert!(hand_off_to(&[(1, "gone".into()), (port2, token2)], &plain(vec![file])).is_some());
        assert!(two.files.recv_timeout(Duration::from_secs(5)).is_ok());
        one.stop();
        two.stop();
        for dir in [dir, first, second] {
            let _ = std::fs::remove_dir_all(dir);
        }
    }

    #[test]
    fn a_terminal_is_given_its_window_the_path_and_wslenv() {
        let _window = listen(&folder("env"), || {}, &[]).unwrap();
        let mut env = vec![("WSLENV".to_string(), "CLAUDE_CODE_SSE_PORT".to_string())];
        let path = std::env::join_paths(["/usr/bin", "/bin"]).unwrap();
        terminal_environment(&mut env, true, None, Some(&path), Some(Path::new("/opt/limoniter")));
        assert!(env.iter().any(|(name, _)| name == WINDOW_VARIABLE));
        assert_eq!(env[0].1, "CLAUDE_CODE_SSE_PORT:LIMONITER_WINDOW", "named in WSLENV beside what's there");
        assert!(!env.iter().any(|(name, _)| name == "PATH"), "a WSL shell has its own PATH");
        let mut env = Vec::new();
        terminal_environment(&mut env, false, None, Some(&path), Some(Path::new("/opt/limoniter")));
        let value = &env.iter().find(|(name, _)| name == "PATH").unwrap().1;
        assert!(value.starts_with("/opt/limoniter") && value.contains("/usr/bin"));
        let mut env = Vec::new();
        terminal_environment(&mut env, false, None, Some(&path), Some(Path::new("/bin")));
        assert!(!env.iter().any(|(name, _)| name == "PATH"), "already there");
    }

    #[test]
    fn with_no_window_or_a_stale_note_it_opens_its_own() {
        let dir = folder("none");
        assert!(hand_off_to(&targets(&dir, None), &plain(vec![dir.join("x.txt")])).is_none(), "no note at all");
        std::fs::write(dir.join(FILE_NAME), "1 secret\n").unwrap();
        assert!(hand_off_to(&targets(&dir, None), &plain(vec![dir.join("x.txt")])).is_none(), "nobody's listening there");
        std::fs::write(dir.join(FILE_NAME), "garbage").unwrap();
        assert!(hand_off_to(&targets(&dir, None), &plain(vec![dir.join("x.txt")])).is_none());
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn a_wrong_secret_is_turned_away() {
        let dir = folder("secret");
        let listening = listen(&dir, || {}, &[]).unwrap();
        let note = std::fs::read_to_string(dir.join(FILE_NAME)).unwrap();
        let port = note.split_whitespace().next().unwrap();
        assert!(hand_off_to(&[(port.parse().unwrap(), "not-the-secret".into())], &plain(vec![dir.join("x.txt")])).is_none());
        assert!(listening.files.try_recv().is_err());
        listening.stop();
        let _ = std::fs::remove_dir_all(dir);
    }
}
