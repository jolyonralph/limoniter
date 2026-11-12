//! One Limoniter window for files opened from Explorer. A window listens on this computer only, and a second
//! Limoniter started with files hands them to it and closes, so they open as tabs in the window already there.
//!
//! The window writes where it's listening, with a secret, into a file beside the settings. Only something that can
//! read that file, which is this Windows user, can hand it files. Anything wrong at all, and the second Limoniter
//! simply opens its own window, as it always used to.

use sha2::{Digest, Sha256};
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{Ipv4Addr, SocketAddr, TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver};
use std::time::Duration;

const FILE_NAME: &str = "window.txt";
const GREETING: &str = "LIMONITER-OPEN-1";
const MOST_REQUEST_BYTES: u64 = 256 * 1024;
const WAIT: Duration = Duration::from_secs(3);

/// The text of a request to open these files.
fn request(token: &str, files: &[PathBuf]) -> String {
    let mut text = format!("{GREETING}\n{token}\n");
    for file in files {
        text += &file.display().to_string();
        text.push('\n');
    }
    text.push('\n');
    text
}

/// The files in a request, if it's a proper one with the right secret.
fn files_in(text: &str, token: &str) -> Option<Vec<PathBuf>> {
    let mut lines = text.split('\n').map(|line| line.trim_end_matches('\r'));
    if lines.next()? != GREETING || lines.next()? != token {
        return None;
    }
    let files: Vec<PathBuf> = lines.take_while(|line| !line.is_empty()).map(PathBuf::from).filter(|path| path.is_absolute()).collect();
    (!files.is_empty()).then_some(files)
}

/// Tries to give the files to a window that's already open. True means it took them, and this one can close.
pub fn hand_off(folder: &Path, files: &[PathBuf]) -> bool {
    let Ok(note) = std::fs::read_to_string(folder.join(FILE_NAME)) else {
        return false;
    };
    let mut parts = note.split_whitespace();
    let (Some(port), Some(token)) = (parts.next().and_then(|port| port.parse::<u16>().ok()), parts.next()) else {
        return false;
    };
    let files: Vec<PathBuf> = files.iter().map(|file| std::path::absolute(file).unwrap_or_else(|_| file.clone())).collect();
    // Windows only lets the program in front bring another to the front. This one was just started from Explorer,
    // so it's in front, and it lets the window it's handing the files to come forward.
    #[cfg(windows)]
    // SAFETY: a plain call with no pointers.
    unsafe {
        let _ = windows::Win32::UI::WindowsAndMessaging::AllowSetForegroundWindow(windows::Win32::UI::WindowsAndMessaging::ASFW_ANY);
    }
    let address = SocketAddr::from((Ipv4Addr::LOCALHOST, port));
    let Ok(mut stream) = TcpStream::connect_timeout(&address, Duration::from_millis(600)) else {
        return false;
    };
    let _ = stream.set_read_timeout(Some(WAIT));
    let _ = stream.set_write_timeout(Some(WAIT));
    if stream.write_all(request(token, &files).as_bytes()).is_err() {
        return false;
    }
    let mut answer = String::new();
    BufReader::new(stream).read_line(&mut answer).is_ok() && answer.trim() == "OK"
}

/// A secret that can't be guessed from outside: when, where in memory, and which process.
fn new_token() -> String {
    let marker = 0u8;
    let seed = format!("{:?} {} {:p}", std::time::SystemTime::now(), std::process::id(), &raw const marker);
    Sha256::digest(seed.as_bytes()).iter().take(16).map(|byte| format!("{byte:02x}")).collect()
}

/// A window listening for files.
pub struct Listening {
    pub files: Receiver<Vec<PathBuf>>,
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
pub fn listen(folder: &Path, wake: impl Fn() + Send + 'static) -> Option<Listening> {
    let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).ok()?;
    let port = listener.local_addr().ok()?.port();
    let token = new_token();
    std::fs::create_dir_all(folder).ok()?;
    let note = format!("{port} {token}\n");
    let note_path = folder.join(FILE_NAME);
    crate::codec::write_file(&note_path, note.as_bytes()).ok()?;
    let (sender, receiver) = mpsc::channel();
    std::thread::Builder::new()
        .name("limoniter-window".into())
        .spawn(move || {
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
                let reply = match files_in(&text, &token) {
                    Some(files) => {
                        if sender.send(files).is_err() {
                            return;
                        }
                        wake();
                        "OK\n"
                    }
                    None => "NO\n",
                };
                let _ = (&stream).write_all(reply.as_bytes());
            }
        })
        .ok()?;
    Some(Listening { files: receiver, note_path, note })
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

    #[test]
    fn a_request_reads_back_and_needs_the_secret() {
        let files = if cfg!(windows) { vec![PathBuf::from(r"C:\a b\one.txt"), PathBuf::from(r"D:\two.md")] } else { vec![PathBuf::from("/a b/one.txt"), PathBuf::from("/two.md")] };
        let text = request("secret", &files);
        assert_eq!(files_in(&text, "secret"), Some(files));
        assert_eq!(files_in(&text, "wrong"), None);
        assert_eq!(files_in("nonsense\n", "secret"), None);
        assert_eq!(files_in(&request("secret", &[PathBuf::from("relative.txt")]), "secret"), None, "only full paths");
    }

    #[test]
    fn files_handed_over_arrive_in_the_window_already_open() {
        let dir = folder("hand");
        let listening = listen(&dir, || {}).expect("it listens");
        let file = dir.join("opened.txt");
        assert!(hand_off(&dir, std::slice::from_ref(&file)));
        assert_eq!(listening.files.recv_timeout(Duration::from_secs(5)).unwrap(), [file]);
        listening.stop();
        assert!(!dir.join(FILE_NAME).exists(), "the note goes when the window closes");
        assert!(!hand_off(&dir, &[dir.join("x.txt")]));
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn with_no_window_or_a_stale_note_it_opens_its_own() {
        let dir = folder("none");
        assert!(!hand_off(&dir, &[dir.join("x.txt")]), "no note at all");
        std::fs::write(dir.join(FILE_NAME), "1 secret\n").unwrap();
        assert!(!hand_off(&dir, &[dir.join("x.txt")]), "nobody's listening there");
        std::fs::write(dir.join(FILE_NAME), "garbage").unwrap();
        assert!(!hand_off(&dir, &[dir.join("x.txt")]));
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn a_wrong_secret_is_turned_away() {
        let dir = folder("secret");
        let listening = listen(&dir, || {}).unwrap();
        let note = std::fs::read_to_string(dir.join(FILE_NAME)).unwrap();
        let port = note.split_whitespace().next().unwrap();
        std::fs::write(dir.join(FILE_NAME), format!("{port} not-the-secret\n")).unwrap();
        assert!(!hand_off(&dir, &[dir.join("x.txt")]));
        assert!(listening.files.try_recv().is_err());
        listening.stop();
        assert!(dir.join(FILE_NAME).exists(), "a note that isn't this window's own is left alone");
        let _ = std::fs::remove_dir_all(dir);
    }
}
