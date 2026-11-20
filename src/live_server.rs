//! A live server: a folder served on this computer only (127.0.0.1), as Live Server does, so a web page can be
//! seen as a browser shows it, and seen again each time a file's saved: every HTML page gets a few lines of
//! script that ask, once a second, whether anything's been saved, and reload when it has.

use std::io::{BufRead, BufReader, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

/// The address the pages ask whether anything's been saved.
const ASK: &str = "/__limoniter_saved";

/// The script put at the end of each HTML page.
fn reload_script(saved: u64) -> String {
    format!(
        "<script>/* Added by Limoniter's live server: reloads when a file's saved. */(function(){{var seen={saved};setInterval(function(){{fetch('{ASK}').then(function(r){{return r.text();}}).then(function(t){{if(+t!==seen){{location.reload();}}}}).catch(function(){{}});}},1000);}})();</script>"
    )
}

/// A folder being served, and how to stop it.
pub struct Server {
    pub folder: PathBuf,
    pub port: u16,
    saved: Arc<AtomicU64>,
    stop: Arc<AtomicBool>,
}

impl Server {
    /// Serves a folder on the first free port from 5500, as Live Server does.
    pub fn start(folder: &Path) -> Result<Server, String> {
        let listener = (5500..5600).find_map(|port| TcpListener::bind(("127.0.0.1", port)).ok()).ok_or_else(|| crate::i18n::tr("No port from 5500 to 5599 is free to serve on.").to_string())?;
        let port = listener.local_addr().map_err(|e| e.to_string())?.port();
        listener.set_nonblocking(true).map_err(|e| e.to_string())?;
        let saved = Arc::new(AtomicU64::new(1));
        let stop = Arc::new(AtomicBool::new(false));
        let (root, counter, stopping) = (folder.to_path_buf(), saved.clone(), stop.clone());
        std::thread::spawn(move || {
            while !stopping.load(Ordering::Relaxed) {
                match listener.accept() {
                    Ok((stream, _)) => {
                        let (root, counter) = (root.clone(), counter.clone());
                        std::thread::spawn(move || {
                            let _ = answer(stream, &root, &counter);
                        });
                    }
                    Err(_) => std::thread::sleep(std::time::Duration::from_millis(50)),
                }
            }
        });
        Ok(Server { folder: folder.to_path_buf(), port, saved, stop })
    }

    /// A file was saved: pages reload.
    pub fn saved(&self) {
        self.saved.fetch_add(1, Ordering::Relaxed);
    }

    /// The address of a file in the folder.
    pub fn address_of(&self, file: Option<&Path>) -> String {
        let relative = file.and_then(|f| f.strip_prefix(&self.folder).ok()).map(|p| p.to_string_lossy().replace('\\', "/")).unwrap_or_default();
        format!("http://127.0.0.1:{}/{}", self.port, relative.split('/').map(encode).collect::<Vec<_>>().join("/"))
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}

fn encode(part: &str) -> String {
    part.bytes().map(|b| if b.is_ascii_alphanumeric() || b"-._~".contains(&b) { (b as char).to_string() } else { format!("%{b:02X}") }).collect()
}

fn decode(part: &str) -> String {
    let bytes = part.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%'
            && let Some(byte) = part.get(i + 1..i + 3).and_then(|h| u8::from_str_radix(h, 16).ok())
        {
            out.push(byte);
            i += 3;
            continue;
        }
        out.push(if bytes[i] == b'+' { b' ' } else { bytes[i] });
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// The file a request asks for, if it's inside the folder: no climbing out with .., and a folder's index.html.
pub fn file_for(root: &Path, request_path: &str) -> Option<PathBuf> {
    let path = decode(request_path.split(['?', '#']).next().unwrap_or(""));
    let mut file = root.to_path_buf();
    for part in path.split('/').filter(|p| !p.is_empty() && *p != ".") {
        if part == ".." || part.contains(['\\', ':']) {
            return None;
        }
        file.push(part);
    }
    if file.is_dir() {
        file.push("index.html");
    }
    file.is_file().then_some(file)
}

fn kind_of(file: &Path) -> &'static str {
    match file.extension().and_then(|e| e.to_str()).map(str::to_lowercase).as_deref() {
        Some("html" | "htm") => "text/html; charset=utf-8",
        Some("css") => "text/css; charset=utf-8",
        Some("js" | "mjs") => "text/javascript; charset=utf-8",
        Some("json") => "application/json",
        Some("svg") => "image/svg+xml",
        Some("png") => "image/png",
        Some("jpg" | "jpeg") => "image/jpeg",
        Some("gif") => "image/gif",
        Some("webp") => "image/webp",
        Some("ico") => "image/x-icon",
        Some("wasm") => "application/wasm",
        Some("woff2") => "font/woff2",
        Some("txt" | "md") => "text/plain; charset=utf-8",
        _ => "application/octet-stream",
    }
}

/// The page with the reloading script put before its </body>, or at its end.
pub fn with_reload(page: &[u8], saved: u64) -> Vec<u8> {
    let text = String::from_utf8_lossy(page);
    let script = reload_script(saved);
    let lower = text.to_lowercase();
    match lower.rfind("</body>") {
        Some(at) => format!("{}{script}{}", &text[..at], &text[at..]).into_bytes(),
        None => format!("{text}{script}").into_bytes(),
    }
}

fn answer(mut stream: TcpStream, root: &Path, saved: &AtomicU64) -> std::io::Result<()> {
    stream.set_nonblocking(false)?;
    stream.set_read_timeout(Some(std::time::Duration::from_secs(5)))?;
    let mut reader = BufReader::new(stream.try_clone()?);
    let mut first = String::new();
    reader.read_line(&mut first)?;
    // The rest of the request's head isn't needed.
    loop {
        let mut line = String::new();
        if reader.read_line(&mut line)? == 0 || line.trim().is_empty() {
            break;
        }
    }
    let mut parts = first.split_whitespace();
    let (method, path) = (parts.next().unwrap_or(""), parts.next().unwrap_or("/"));
    let reply = |stream: &mut TcpStream, status: &str, kind: &str, body: &[u8]| {
        let head = format!("HTTP/1.1 {status}\r\nContent-Type: {kind}\r\nContent-Length: {}\r\nCache-Control: no-store\r\nConnection: close\r\n\r\n", body.len());
        stream.write_all(head.as_bytes())?;
        if method != "HEAD" {
            stream.write_all(body)?;
        }
        stream.flush()
    };
    if method != "GET" && method != "HEAD" {
        return reply(&mut stream, "405 Method Not Allowed", "text/plain", b"Only GET is served.");
    }
    if path == ASK {
        return reply(&mut stream, "200 OK", "text/plain", saved.load(Ordering::Relaxed).to_string().as_bytes());
    }
    match file_for(root, path) {
        Some(file) => {
            let bytes = std::fs::read(&file).unwrap_or_default();
            let kind = kind_of(&file);
            let body = if kind.starts_with("text/html") { with_reload(&bytes, saved.load(Ordering::Relaxed)) } else { bytes };
            reply(&mut stream, "200 OK", kind, &body)
        }
        None => reply(&mut stream, "404 Not Found", "text/plain", b"Not found."),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Read;

    #[test]
    fn only_the_folders_files_are_served() {
        let root = std::env::temp_dir().join(format!("limoniter-serve-{}", std::process::id()));
        std::fs::create_dir_all(root.join("sub dir")).unwrap();
        std::fs::write(root.join("index.html"), "<html><body>hi</body></html>").unwrap();
        std::fs::write(root.join("sub dir/a.css"), "p{}").unwrap();
        assert_eq!(file_for(&root, "/"), Some(root.join("index.html")));
        assert_eq!(file_for(&root, "/sub%20dir/a.css?v=2"), Some(root.join("sub dir").join("a.css")));
        assert_eq!(file_for(&root, "/../secret"), None);
        assert_eq!(file_for(&root, "/sub%20dir/..%2F..%2Fx"), None);
        assert!(String::from_utf8(with_reload(b"<html><BODY>x</BODY></html>", 3)).unwrap().contains("var seen=3;"));

        // And over the network, the page with its script, and the count of saves.
        let server = Server::start(&root).unwrap();
        let fetch = |path: &str| {
            let mut stream = TcpStream::connect(("127.0.0.1", server.port)).unwrap();
            stream.write_all(format!("GET {path} HTTP/1.1\r\nHost: x\r\n\r\n").as_bytes()).unwrap();
            let mut got = String::new();
            stream.read_to_string(&mut got).unwrap();
            got
        };
        let page = fetch("/");
        assert!(page.starts_with("HTTP/1.1 200") && page.contains("hi") && page.contains("location.reload()"), "{page}");
        server.saved();
        assert!(fetch(ASK).ends_with("\r\n\r\n2"));
        assert!(fetch("/nothing").starts_with("HTTP/1.1 404"));
        assert_eq!(server.address_of(Some(&root.join("sub dir").join("a.css"))), format!("http://127.0.0.1:{}/sub%20dir/a.css", server.port));
        drop(server);
        let _ = std::fs::remove_dir_all(&root);
    }
}
