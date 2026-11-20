//! FTP and FTPS against a small FTP server run inside the tests, over real sockets, serving a temporary folder.
//! It speaks enough of FTP for the client (and the remote projects, which use it too), and can be made to offer
//! less, as older servers do: no MLSD or MLST, so LIST has to be read; no EPSV, so PASV is used.

use super::*;
use std::collections::HashMap;
use std::net::TcpListener;
use std::path::PathBuf;

/// A certificate the test server makes its own, as a home server does: no authority signs it.
const CERTIFICATE: &str = "-----BEGIN CERTIFICATE-----
MIIBvDCCAWGgAwIBAgIUC15rSkutUhWeBL7DC/ov54OWoU4wCgYIKoZIzj0EAwIw
JDEiMCAGA1UEAwwZTGltb25pdGVyIHRlc3QgRlRQIHNlcnZlcjAgFw0yNjExMjAw
ODMyMzFaGA8yMTI2MTAyNzA4MzIzMVowJDEiMCAGA1UEAwwZTGltb25pdGVyIHRl
c3QgRlRQIHNlcnZlcjBZMBMGByqGSM49AgEGCCqGSM49AwEHA0IABHHVlhF14TUC
uI7DFZxQnJhmNtB/SY3iaffGfgVPVVq6a0s/JG2Dy7AUN6Os27rnhywWguv9s4Re
k8Bsxj/EVYWjbzBtMB0GA1UdDgQWBBR/gFGPIuH3e0SwVaU9BX4SuEqRyjAfBgNV
HSMEGDAWgBR/gFGPIuH3e0SwVaU9BX4SuEqRyjAPBgNVHRMBAf8EBTADAQH/MBoG
A1UdEQQTMBGCCWxvY2FsaG9zdIcEfwAAATAKBggqhkjOPQQDAgNJADBGAiEA2+78
uq+MDCi1YGz+x3rA7TPA2oFnjDI1RnxBrCJwooICIQDB5qIS9+9MHdqRlfPPWxQS
UV4aaq835KL4Ztnr0r+Xqg==
-----END CERTIFICATE-----
";
const PRIVATE_KEY: &str = "-----BEGIN PRIVATE KEY-----
MIGHAgEAMBMGByqGSM49AgEGCCqGSM49AwEHBG0wawIBAQQg1bECyUHLlo4DxNnB
y9T+0ebPfkr+UVP1G/lx7PRwjM2hRANCAARx1ZYRdeE1AriOwxWcUJyYZjbQf0mN
4mn3xn4FT1VaumtLPyRtg8uwFDejrNu654csFoLr/bOEXpPAbMY/xFWF
-----END PRIVATE KEY-----
";

/// The only account, besides anonymous.
pub(crate) const USER: &str = "ann";
pub(crate) const PASSWORD: &str = "secret";

/// What the test server offers.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Offers {
    /// MLSD, MLST and MFMT, as a modern server has.
    pub modern: bool,
    pub epsv: bool,
    /// TLS: explicit (AUTH TLS), or implicit (from the start).
    pub tls: Option<Security>,
    /// Whether signing in is refused until TLS has started, as a server set up for FTPS only does.
    pub insists_on_tls: bool,
}

impl Default for Offers {
    fn default() -> Self {
        Self { modern: true, epsv: true, tls: None, insists_on_tls: false }
    }
}

/// A server running on a port of its own, serving `root`.
pub(crate) struct TestServer {
    pub port: u16,
    pub root: PathBuf,
}

impl Drop for TestServer {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

pub(crate) fn certificate_fingerprint() -> String {
    use rustls::pki_types::pem::PemObject;
    crate::remote::fingerprint(CertificateDer::from_pem_slice(CERTIFICATE.as_bytes()).unwrap().as_ref())
}

fn server_config() -> Arc<rustls::ServerConfig> {
    use rustls::pki_types::PrivateKeyDer;
    use rustls::pki_types::pem::PemObject;
    let certificate = CertificateDer::from_pem_slice(CERTIFICATE.as_bytes()).unwrap();
    let key = PrivateKeyDer::from_pem_slice(PRIVATE_KEY.as_bytes()).unwrap();
    let provider = Arc::new(rustls::crypto::ring::default_provider());
    let config = rustls::ServerConfig::builder_with_provider(provider).with_safe_default_protocol_versions().unwrap().with_no_client_auth().with_single_cert(vec![certificate], key).unwrap();
    Arc::new(config)
}

pub(crate) fn start(offers: Offers) -> TestServer {
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let root = std::env::temp_dir().join(format!("limoniter-ftp-test-{}-{}", std::process::id(), NEXT.fetch_add(1, Ordering::Relaxed)));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(root.join("home/ann")).unwrap();
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let config = offers.tls.map(|_| server_config());
    let served = root.clone();
    std::thread::spawn(move || {
        for stream in listener.incoming().flatten() {
            let (root, config) = (served.clone(), config.clone());
            std::thread::spawn(move || {
                let _ = Session::run(stream, root, offers, config);
            });
        }
    });
    TestServer { port, root }
}

enum ServerStream {
    Plain(TcpStream),
    Tls(Box<rustls::StreamOwned<rustls::ServerConnection, TcpStream>>),
}

impl Read for ServerStream {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        match self {
            ServerStream::Plain(s) => s.read(buf),
            ServerStream::Tls(s) => s.read(buf),
        }
    }
}

impl Write for ServerStream {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        match self {
            ServerStream::Plain(s) => s.write(buf),
            ServerStream::Tls(s) => s.write(buf),
        }
    }
    fn flush(&mut self) -> std::io::Result<()> {
        match self {
            ServerStream::Plain(s) => s.flush(),
            ServerStream::Tls(s) => s.flush(),
        }
    }
}

fn accept_tls(tcp: TcpStream, config: &Arc<rustls::ServerConfig>) -> std::io::Result<ServerStream> {
    let connection = rustls::ServerConnection::new(config.clone()).map_err(std::io::Error::other)?;
    let mut tls = Box::new(rustls::StreamOwned::new(connection, tcp));
    while tls.conn.is_handshaking() {
        tls.conn.complete_io(&mut tls.sock)?;
    }
    Ok(ServerStream::Tls(tls))
}

struct Session {
    control: ServerStream,
    root: PathBuf,
    offers: Offers,
    config: Option<Arc<rustls::ServerConfig>>,
    cwd: String,
    user: Option<String>,
    signed_in: bool,
    protect: bool,
    passive: Option<TcpListener>,
    renaming: Option<PathBuf>,
}

impl Session {
    fn run(tcp: TcpStream, root: PathBuf, offers: Offers, config: Option<Arc<rustls::ServerConfig>>) -> std::io::Result<()> {
        let control = match (&config, offers.tls) {
            (Some(config), Some(Security::Implicit)) => accept_tls(tcp, config)?,
            _ => ServerStream::Plain(tcp),
        };
        let mut session = Session { control, root, offers, config, cwd: "/".into(), user: None, signed_in: false, protect: false, passive: None, renaming: None };
        session.say("220 Limoniter's test server")?;
        loop {
            let line = session.read_line()?;
            if line.is_empty() {
                return Ok(());
            }
            let (word, argument) = line.split_once(' ').map_or((line.as_str(), ""), |(w, a)| (w, a));
            if !session.handle(&word.to_ascii_uppercase(), argument)? {
                return Ok(());
            }
        }
    }

    fn read_line(&mut self) -> std::io::Result<String> {
        let mut bytes = Vec::new();
        let mut one = [0u8; 1];
        loop {
            if self.control.read(&mut one)? == 0 {
                return Ok(String::new());
            }
            if one[0] == b'\n' {
                return Ok(String::from_utf8_lossy(&bytes).trim_end_matches('\r').to_string());
            }
            bytes.push(one[0]);
        }
    }

    fn say(&mut self, text: &str) -> std::io::Result<()> {
        self.control.write_all(format!("{}\r\n", text.replace('\n', "\r\n")).as_bytes())?;
        self.control.flush()
    }

    /// A path the client gave, on this computer: inside the root, whatever it says.
    fn place(&self, argument: &str) -> PathBuf {
        let virtual_path = if argument.starts_with('/') { argument.to_string() } else { format!("{}/{argument}", self.cwd.trim_end_matches('/')) };
        let mut at = self.root.clone();
        for part in virtual_path.split('/').filter(|p| !p.is_empty() && *p != "." && *p != "..") {
            at.push(part);
        }
        at
    }

    fn data(&mut self) -> std::io::Result<ServerStream> {
        let listener = self.passive.take().ok_or_else(|| std::io::Error::other("no passive port"))?;
        let (tcp, _) = listener.accept()?;
        match (&self.config, self.protect) {
            (Some(config), true) => accept_tls(tcp, config),
            _ => Ok(ServerStream::Plain(tcp)),
        }
    }

    fn finish_data(stream: ServerStream) -> std::io::Result<()> {
        match stream {
            ServerStream::Plain(tcp) => tcp.shutdown(Shutdown::Write),
            ServerStream::Tls(mut tls) => {
                tls.conn.send_close_notify();
                tls.flush()?;
                tls.sock.shutdown(Shutdown::Write)
            }
        }
    }

    fn listing(&self, path: &Path, modern: bool) -> String {
        let mut entries: Vec<_> = std::fs::read_dir(path).map(|r| r.flatten().collect()).unwrap_or_default();
        entries.sort_by_key(std::fs::DirEntry::file_name);
        let mut out = String::new();
        for entry in entries {
            let meta = entry.metadata().unwrap();
            let name = entry.file_name().to_string_lossy().into_owned();
            let time = chrono::DateTime::<chrono::Utc>::from(meta.modified().unwrap());
            if modern {
                out += &format!("type={};size={};modify={}; {name}\r\n", if meta.is_dir() { "dir" } else { "file" }, meta.len(), time.format("%Y%m%d%H%M%S"));
            } else {
                // As ls -l writes it: a time of day for this year's files, the year for older ones.
                let when = if chrono::Utc::now().signed_duration_since(time).num_days() < 150 { time.format("%b %e %H:%M").to_string() } else { time.format("%b %e  %Y").to_string() };
                out += &format!("{}rw-r--r--   1 ann      staff    {:>8} {when} {name}\r\n", if meta.is_dir() { 'd' } else { '-' }, meta.len());
            }
        }
        out
    }

    /// Does one command. False once the client has said goodbye.
    fn handle(&mut self, word: &str, argument: &str) -> std::io::Result<bool> {
        if !self.signed_in && !matches!(word, "USER" | "PASS" | "AUTH" | "QUIT" | "FEAT") {
            self.say("530 Sign in first")?;
            return Ok(true);
        }
        if self.offers.insists_on_tls && self.config.is_some() && !matches!(self.control, ServerStream::Tls(_)) && matches!(word, "USER" | "PASS") {
            self.say("530 TLS first")?;
            return Ok(true);
        }
        match word {
            "USER" => {
                self.user = Some(argument.to_string());
                self.say("331 Password, please")?;
            }
            "PASS" => {
                let fine = match self.user.as_deref() {
                    Some("anonymous") => true,
                    Some(user) => user == USER && argument == PASSWORD,
                    None => false,
                };
                self.signed_in = fine;
                self.say(if fine { "230 Signed in" } else { "530 Not that password" })?;
            }
            "AUTH" => match &self.config {
                Some(config) if self.offers.tls == Some(Security::Explicit) => {
                    let config = config.clone();
                    self.say("234 Go ahead")?;
                    let ServerStream::Plain(tcp) = &self.control else { return Ok(true) };
                    let tcp = tcp.try_clone()?;
                    self.control = accept_tls(tcp, &config)?;
                }
                _ => self.say("502 No TLS here")?,
            },
            "PBSZ" => self.say("200 PBSZ=0")?,
            "PROT" => {
                self.protect = argument.eq_ignore_ascii_case("P");
                self.say("200 Protection set")?;
            }
            "FEAT" => {
                let mut text = String::from("211-Features:\n UTF8\n SIZE\n");
                if self.offers.modern {
                    text += " MLST type*;size*;modify*;\n MFMT\n";
                }
                text += "211 End";
                self.say(&text)?;
            }
            "OPTS" | "TYPE" => self.say("200 Fine")?,
            "PWD" => {
                let cwd = self.cwd.clone();
                self.say(&format!("257 \"{cwd}\" is where you are"))?;
            }
            "CWD" => {
                if self.place(argument).is_dir() {
                    self.cwd = if argument.starts_with('/') { argument.to_string() } else { format!("{}/{argument}", self.cwd.trim_end_matches('/')) };
                    self.say("250 Moved")?;
                } else {
                    self.say("550 No such folder")?;
                }
            }
            "EPSV" if self.offers.epsv => {
                let listener = TcpListener::bind("127.0.0.1:0")?;
                let port = listener.local_addr()?.port();
                self.passive = Some(listener);
                self.say(&format!("229 Entering Extended Passive Mode (|||{port}|)"))?;
            }
            "PASV" => {
                let listener = TcpListener::bind("127.0.0.1:0")?;
                let port = listener.local_addr()?.port();
                self.passive = Some(listener);
                // An inside address, as a server behind a router gives; the client ignores it.
                self.say(&format!("227 Entering Passive Mode (10,0,0,9,{},{})", port / 256, port % 256))?;
            }
            "LIST" | "MLSD" => {
                let modern = word == "MLSD";
                if modern && !self.offers.modern {
                    self.say("500 What?")?;
                    return Ok(true);
                }
                let folder = self.place(if argument.starts_with('-') { "" } else { argument });
                if !folder.is_dir() {
                    self.say("550 No such folder")?;
                    return Ok(true);
                }
                let text = self.listing(&folder, modern);
                self.say("150 Here it comes")?;
                let mut data = self.data()?;
                data.write_all(text.as_bytes())?;
                Self::finish_data(data)?;
                self.say("226 Done")?;
            }
            "MLST" if self.offers.modern => {
                let at = self.place(argument);
                match std::fs::metadata(&at) {
                    Ok(meta) => {
                        let time = chrono::DateTime::<chrono::Utc>::from(meta.modified()?);
                        let kind = if meta.is_dir() { "dir" } else { "file" };
                        self.say(&format!("250-Listing\n type={kind};size={};modify={}; {argument}\n250 End", meta.len(), time.format("%Y%m%d%H%M%S")))?;
                    }
                    Err(_) => self.say("550 Not there")?,
                }
            }
            "RETR" => match std::fs::read(self.place(argument)) {
                Ok(bytes) => {
                    self.say("150 Here it comes")?;
                    let mut data = self.data()?;
                    data.write_all(&bytes)?;
                    Self::finish_data(data)?;
                    self.say("226 Done")?;
                }
                Err(_) => self.say("550 Not there")?,
            },
            "STOR" => {
                let at = self.place(argument);
                if !at.parent().is_some_and(Path::is_dir) {
                    self.say("553 No such folder")?;
                    return Ok(true);
                }
                self.say("150 Send it")?;
                let mut data = self.data()?;
                let mut bytes = Vec::new();
                let read = data.read_to_end(&mut bytes);
                // A TLS transfer has to end by saying so, or it may have been cut short.
                if read.is_ok() {
                    std::fs::write(&at, &bytes)?;
                    self.say("226 Stored")?;
                } else {
                    self.say("426 Cut short")?;
                }
            }
            "RNFR" => {
                let at = self.place(argument);
                if at.exists() {
                    self.renaming = Some(at);
                    self.say("350 And to?")?;
                } else {
                    self.say("550 Not there")?;
                }
            }
            "RNTO" => match self.renaming.take() {
                Some(from) => match std::fs::rename(&from, self.place(argument)) {
                    Ok(()) => self.say("250 Renamed")?,
                    Err(_) => self.say("553 Couldn't")?,
                },
                None => self.say("503 RNFR first")?,
            },
            "DELE" => match std::fs::remove_file(self.place(argument)) {
                Ok(()) => self.say("250 Gone")?,
                Err(_) => self.say("550 Couldn't")?,
            },
            "MKD" => match std::fs::create_dir(self.place(argument)) {
                Ok(()) => self.say(&format!("257 \"{argument}\" made"))?,
                Err(_) => self.say("550 Couldn't")?,
            },
            "RMD" => match std::fs::remove_dir(self.place(argument)) {
                Ok(()) => self.say("250 Gone")?,
                Err(_) => self.say("550 Couldn't")?,
            },
            "MFMT" if self.offers.modern => {
                let (when, path) = argument.split_once(' ').unwrap_or((argument, ""));
                let time = mlsx_time(when).map(|t| std::time::UNIX_EPOCH + Duration::from_secs(t));
                match (time, std::fs::File::options().write(true).open(self.place(path))) {
                    (Some(time), Ok(file)) if file.set_modified(time).is_ok() => self.say(&format!("213 Modify={when}; {path}"))?,
                    _ => self.say("550 Couldn't")?,
                }
            }
            "QUIT" => {
                self.say("221 Goodbye")?;
                return Ok(false);
            }
            _ => self.say("502 Not done here")?,
        }
        Ok(true)
    }
}

fn stopped() -> (AtomicU64, AtomicBool) {
    (AtomicU64::new(0), AtomicBool::new(false))
}

/// Everything a client does, against whatever the server offers.
fn exercise(client: &mut Client, server: &TestServer) {
    let home = client.home().unwrap();
    assert_eq!(home, "/");
    client.make_folder("/home/ann/notes").unwrap();
    std::fs::write(server.root.join("home/ann/a file.txt"), "first\n").unwrap();
    let listed = client.list("/home/ann").unwrap();
    let names: Vec<(&str, bool)> = listed.iter().map(|e| (e.name.as_str(), e.folder)).collect();
    assert_eq!(names, [("notes", true), ("a file.txt", false)], "folders first, and a name with a space kept whole");
    assert_eq!(listed[1].size, 6);
    let (done, stop) = stopped();
    let local = server.root.with_extension("local");
    let stamp = client.download("/home/ann/a file.txt", &local, &done, &stop).unwrap();
    assert_eq!(std::fs::read_to_string(&local).unwrap(), "first\n");
    assert_eq!(stamp.size, 6);
    // Sent back over itself, beside it first.
    std::fs::write(&local, "second, longer\n").unwrap();
    let sent = client.upload(&local, "/home/ann/a file.txt", Some(stamp), false, &done, &stop).unwrap();
    assert_eq!(sent.size, 15);
    assert_eq!(std::fs::read_to_string(server.root.join("home/ann/a file.txt")).unwrap(), "second, longer\n");
    assert!(!server.root.join("home/ann/a file.txt.limoniter-tmp").exists(), "nothing left behind");
    // Its first stamp is out of date now: someone else changed it.
    assert_eq!(client.upload(&local, "/home/ann/a file.txt", Some(stamp), false, &done, &stop), Err(UploadError::ChangedOnServer));
    // A new file in a folder.
    client.upload(&local, "/home/ann/notes/new.txt", None, false, &done, &stop).unwrap();
    assert!(server.root.join("home/ann/notes/new.txt").is_file());
    assert_eq!(client.look("/home/ann/notes").unwrap().map(|(folder, _)| folder), Some(true));
    assert_eq!(client.look("/home/ann/notes/new.txt").unwrap().map(|(folder, s)| (folder, s.size)), Some((false, 15)));
    assert_eq!(client.look("/home/ann/nothing.txt").unwrap(), None);
    assert_eq!(client.look("/nowhere/nothing.txt").unwrap(), None);
    client.rename("/home/ann/notes/new.txt", "/home/ann/notes/renamed.txt").unwrap();
    assert!(server.root.join("home/ann/notes/renamed.txt").is_file());
    client.remove_file("/home/ann/notes/renamed.txt").unwrap();
    client.remove_folder("/home/ann/notes").unwrap();
    assert!(!server.root.join("home/ann/notes").exists());
    // An empty file goes and comes back empty, which under TLS means ending a connection with nothing sent.
    std::fs::write(&local, "").unwrap();
    client.upload(&local, "/home/ann/empty", None, false, &done, &stop).unwrap();
    assert_eq!(std::fs::read(server.root.join("home/ann/empty")).unwrap(), b"");
    client.download("/home/ann/empty", &local, &done, &stop).unwrap();
    assert_eq!(std::fs::read(&local).unwrap(), b"");
    let _ = std::fs::remove_file(&local);
}

#[test]
fn a_modern_ftp_server_is_browsed_and_files_go_both_ways() {
    let server = start(Offers::default());
    let mut client = connect("127.0.0.1", server.port, Security::Plain, USER, PASSWORD, None).unwrap();
    assert!(client.mlsd && client.mlst && client.mfmt);
    exercise(&mut client, &server);
    // MFMT sets a file's time.
    std::fs::write(server.root.join("home/ann/timed"), "x").unwrap();
    client.set_modified("/home/ann/timed", 1_600_000_000).unwrap();
    assert_eq!(client.look("/home/ann/timed").unwrap().unwrap().1.modified, 1_600_000_000);
    // Said goodbye to, as a polite client does.
    assert!(client.command("QUIT", &[221]).is_ok());
}

#[test]
fn an_older_ftp_server_is_listed_with_list_and_reached_with_pasv() {
    let server = start(Offers { modern: false, epsv: false, ..Offers::default() });
    let mut client = connect("localhost", server.port, Security::Plain, USER, PASSWORD, None).unwrap();
    assert!(!client.mlsd && !client.mfmt);
    exercise(&mut client, &server);
    assert!(client.set_modified("/home/ann/empty", 1).is_err(), "no MFMT, so the time can't be set");
}

#[test]
fn a_wrong_password_is_a_refused_sign_in() {
    let server = start(Offers::default());
    assert!(matches!(connect("127.0.0.1", server.port, Security::Plain, USER, "wrong", None), Err(Refused::SignIn(_))));
    // Anonymous takes anything.
    assert!(connect("127.0.0.1", server.port, Security::Plain, "", "", None).is_ok());
    // Nothing listening.
    let closed = TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port();
    assert!(matches!(connect("127.0.0.1", closed, Security::Plain, USER, PASSWORD, None), Err(Refused::Failed(_))));
}

#[test]
fn ftps_asks_for_a_certificate_no_authority_signed_to_be_trusted() {
    for security in [Security::Explicit, Security::Implicit] {
        let server = start(Offers { tls: Some(security), insists_on_tls: true, ..Offers::default() });
        let print = certificate_fingerprint();
        match connect("127.0.0.1", server.port, security, USER, PASSWORD, None) {
            Err(Refused::UnknownHost(found)) => assert_eq!(found, print),
            other => panic!("{security:?}: a home server's certificate is shown to be trusted first, not {:?}", other.err()),
        }
        match connect("127.0.0.1", server.port, security, USER, PASSWORD, Some("SHA256:something-else")) {
            Err(Refused::HostKeyChanged { expected, found }) => assert_eq!((expected.as_str(), found.as_str()), ("SHA256:something-else", print.as_str())),
            other => panic!("{security:?}: a different certificate is refused, not {:?}", other.err()),
        }
        let mut client = connect("127.0.0.1", server.port, security, USER, PASSWORD, Some(&print)).unwrap();
        assert!(client.protect_data, "{security:?}: the files go under TLS too");
        exercise(&mut client, &server);
    }
}

#[test]
fn plain_ftp_to_a_server_wanting_tls_is_refused_and_ftpes_to_one_without_it_too() {
    let wants_tls = start(Offers { tls: Some(Security::Explicit), insists_on_tls: true, ..Offers::default() });
    assert!(matches!(connect("127.0.0.1", wants_tls.port, Security::Plain, USER, PASSWORD, None), Err(Refused::SignIn(_))));
    let plain = start(Offers::default());
    let Err(Refused::Failed(why)) = connect("127.0.0.1", plain.port, Security::Explicit, USER, PASSWORD, None) else { panic!("TLS was asked for and isn't there") };
    assert!(why.contains("doesn't offer TLS"), "{why}");
}

#[test]
fn listings_are_read_as_unix_windows_and_mlsd_servers_write_them() {
    let now = chrono::DateTime::parse_from_rfc3339("2026-03-10T12:00:00Z").unwrap().with_timezone(&chrono::Utc);
    let unix = parse_list("-rw-r--r--   1 ann  staff     1234 Mar  3 09:15 two  spaces.txt", now).unwrap();
    assert_eq!((unix.name.as_str(), unix.folder, unix.size), ("two  spaces.txt", false, 1234));
    assert_eq!(unix.modified, chrono::DateTime::parse_from_rfc3339("2026-03-03T09:15:00Z").unwrap().timestamp() as u64);
    // A time of day in December, read in March, was last year.
    let last_year = parse_list("-rw-r--r--   1 ann  staff     1 Dec 24 18:00 gift", now).unwrap();
    assert_eq!(last_year.modified, chrono::DateTime::parse_from_rfc3339("2025-12-24T18:00:00Z").unwrap().timestamp() as u64);
    let old = parse_list("drwxr-xr-x   2 ann  staff     4096 Jan  5  1994 demos", now).unwrap();
    assert!(old.folder && old.modified == chrono::DateTime::parse_from_rfc3339("1994-01-05T00:00:00Z").unwrap().timestamp() as u64);
    let link = parse_list("lrwxrwxrwx   1 ann  staff       11 Mar  3 09:15 latest -> demos/v2", now).unwrap();
    assert_eq!(link.name, "latest");
    assert!(parse_list("total 12", now).is_none());
    let windows = parse_list("03-03-26  09:15PM       <DIR>          Program Files", now).unwrap();
    assert_eq!((windows.name.as_str(), windows.folder), ("Program Files", true));
    let file = parse_list("03-03-2026  09:15AM                 5120 game.d64", now).unwrap();
    assert_eq!((file.size, file.modified), (5120, chrono::DateTime::parse_from_rfc3339("2026-03-03T09:15:00Z").unwrap().timestamp() as u64));
    let mlsd = parse_mlsx("type=file;size=42;modify=20260303091500.123;UNIX.mode=0644; a; b.txt").unwrap();
    assert_eq!((mlsd.name.as_str(), mlsd.size, mlsd.folder), ("a; b.txt", 42, false));
    assert!(parse_mlsx("type=cdir;modify=20260303091500; .").is_none());
    assert!(parse_mlsx(" type=dir;modify=20260303091500; /home/ann/sub").unwrap().folder);
    assert_eq!(quoted("257 \"/home/\"\"quoted\"\"\" is current").as_deref(), Some("/home/\"quoted\""));
}

/// Every file transferred is whole, at sizes either side of the buffer's.
#[test]
fn files_of_awkward_sizes_come_back_whole_under_tls() {
    let server = start(Offers { tls: Some(Security::Explicit), ..Offers::default() });
    let mut client = connect("127.0.0.1", server.port, Security::Explicit, USER, PASSWORD, Some(&certificate_fingerprint())).unwrap();
    let (done, stop) = stopped();
    let mut sizes = HashMap::new();
    for size in [1, CHUNK - 1, CHUNK, CHUNK + 1, 3 * CHUNK + 17] {
        let bytes: Vec<u8> = (0..size).map(|i| (i * 7 % 251) as u8).collect();
        let local = server.root.with_extension(format!("{size}"));
        std::fs::write(&local, &bytes).unwrap();
        let remote = format!("/home/ann/{size}.bin");
        client.upload(&local, &remote, None, false, &done, &stop).unwrap();
        let back = local.with_extension("back");
        client.download(&remote, &back, &done, &stop).unwrap();
        sizes.insert(size, std::fs::read(&back).unwrap() == bytes);
        let _ = std::fs::remove_file(&local);
        let _ = std::fs::remove_file(&back);
    }
    assert!(sizes.values().all(|whole| *whole), "{sizes:?}");
}
