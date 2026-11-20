//! FTP, and FTP over TLS (FTPS), for opening and saving files on a server and browsing its folders, as remote
//! files do over SFTP. Written here over plain sockets rather than with a library, as it's small: a line of
//! words to the server and a numbered reply back, with each file or listing sent over a second connection.
//!
//! Three ways to connect, as FileZilla names them in an address:
//! - ftp:// is plain FTP, where everything, the password included, crosses the network as it is. It's what most
//!   older machines and home devices offer (an Ultimate 64, an Amiga or a MiSTer running an FTP server).
//! - ftpes:// asks for TLS on the usual port (AUTH TLS), and won't go on without it.
//! - ftps:// speaks TLS from the start, on port 990.
//!
//! A server's certificate is accepted if an authority this computer trusts vouches for it. Many home servers make
//! their own, which no authority signs, so as with an SFTP server's key, its fingerprint is shown the first time
//! for the person to trust, and a different one after that is refused.

use crate::remote::{Entry, Refused, Stamp, UploadError};
use rustls::pki_types::{CertificateDer, ServerName, UnixTime};
use std::io::{Read, Write};
use std::net::{IpAddr, Shutdown, TcpStream, ToSocketAddrs};
use std::path::Path;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

const TIMEOUT: Duration = Duration::from_secs(20);
const CHUNK: usize = 256 * 1024;

/// How a connection is kept private.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Security {
    /// Nothing is: plain FTP.
    #[default]
    Plain,
    /// TLS asked for once connected, on the usual port (AUTH TLS).
    Explicit,
    /// TLS from the start, on its own port.
    Implicit,
}

/// A connection, plain or under TLS.
enum Stream {
    Plain(TcpStream),
    Tls(Box<rustls::StreamOwned<rustls::ClientConnection, TcpStream>>),
}

impl Stream {
    fn tcp(&self) -> &TcpStream {
        match self {
            Stream::Plain(tcp) => tcp,
            Stream::Tls(tls) => &tls.sock,
        }
    }

    /// Ends what's sent, so the server knows the file is whole: under TLS, by saying so in TLS first, as servers
    /// that check for a cut-short transfer want. Then waits for the server to close its end. Closing this end
    /// with anything unread from the server (under TLS 1.3, the tickets it sends after the greeting) would reset
    /// the connection, and the end of the file could be lost with it.
    fn finish(mut self) -> std::io::Result<()> {
        if let Stream::Tls(tls) = &mut self {
            tls.conn.send_close_notify();
            tls.flush()?;
        }
        let mut tcp = self.tcp();
        tcp.shutdown(Shutdown::Write)?;
        let mut unread = [0u8; 4096];
        while tcp.read(&mut unread).is_ok_and(|got| got > 0) {}
        Ok(())
    }
}

impl Read for Stream {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        match self {
            Stream::Plain(tcp) => tcp.read(buf),
            // A server that closes a finished transfer without saying so in TLS isn't unusual; the reply that
            // follows on the control connection says whether it was whole.
            Stream::Tls(tls) => match tls.read(buf) {
                Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => Ok(0),
                other => other,
            },
        }
    }
}

impl Write for Stream {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        match self {
            Stream::Plain(tcp) => tcp.write(buf),
            Stream::Tls(tls) => tls.write(buf),
        }
    }

    fn flush(&mut self) -> std::io::Result<()> {
        match self {
            Stream::Plain(tcp) => tcp.flush(),
            Stream::Tls(tls) => tls.flush(),
        }
    }
}

// ---------- Certificates ----------

/// Accepts a certificate an authority vouches for, or the one trusted before, and notes the fingerprint of the
/// one it saw so that a refusal can show it.
#[derive(Debug)]
struct Verifier {
    authorities: Arc<rustls::client::WebPkiServerVerifier>,
    provider: Arc<rustls::crypto::CryptoProvider>,
    trusted: Option<String>,
    seen: Mutex<Option<String>>,
}

impl rustls::client::danger::ServerCertVerifier for Verifier {
    fn verify_server_cert(&self, end_entity: &CertificateDer<'_>, intermediates: &[CertificateDer<'_>], server_name: &ServerName<'_>, ocsp: &[u8], now: UnixTime) -> Result<rustls::client::danger::ServerCertVerified, rustls::Error> {
        let print = crate::remote::fingerprint(end_entity.as_ref());
        *self.seen.lock().unwrap_or_else(std::sync::PoisonError::into_inner) = Some(print.clone());
        if self.trusted.as_deref() == Some(print.as_str()) {
            return Ok(rustls::client::danger::ServerCertVerified::assertion());
        }
        self.authorities.verify_server_cert(end_entity, intermediates, server_name, ocsp, now)
    }

    fn verify_tls12_signature(&self, message: &[u8], cert: &CertificateDer<'_>, dss: &rustls::DigitallySignedStruct) -> Result<rustls::client::danger::HandshakeSignatureValid, rustls::Error> {
        rustls::crypto::verify_tls12_signature(message, cert, dss, &self.provider.signature_verification_algorithms)
    }

    fn verify_tls13_signature(&self, message: &[u8], cert: &CertificateDer<'_>, dss: &rustls::DigitallySignedStruct) -> Result<rustls::client::danger::HandshakeSignatureValid, rustls::Error> {
        rustls::crypto::verify_tls13_signature(message, cert, dss, &self.provider.signature_verification_algorithms)
    }

    fn supported_verify_schemes(&self) -> Vec<rustls::SignatureScheme> {
        self.provider.signature_verification_algorithms.supported_schemes()
    }
}

/// What TLS needs to connect: the settings (which keep the session, so the file connections can take it up
/// again, as many servers insist), the name to check the certificate against, and the verifier.
struct Tls {
    config: Arc<rustls::ClientConfig>,
    name: ServerName<'static>,
    verifier: Arc<Verifier>,
}

impl Tls {
    fn new(host: &str, trusted: Option<&str>) -> Result<Tls, String> {
        let provider = Arc::new(rustls::crypto::ring::default_provider());
        let roots = rustls::RootCertStore { roots: webpki_roots::TLS_SERVER_ROOTS.to_vec() };
        let authorities = rustls::client::WebPkiServerVerifier::builder_with_provider(Arc::new(roots), provider.clone()).build().map_err(|e| e.to_string())?;
        let verifier = Arc::new(Verifier { authorities, provider: provider.clone(), trusted: trusted.map(str::to_string), seen: Mutex::new(None) });
        let config = rustls::ClientConfig::builder_with_provider(provider).with_safe_default_protocol_versions().map_err(|e| e.to_string())?.dangerous().with_custom_certificate_verifier(verifier.clone()).with_no_client_auth();
        let name = ServerName::try_from(host.trim_matches(|c| c == '[' || c == ']').to_string()).map_err(|_| crate::i18n::tf!("{host} isn't a name TLS can check a certificate against."))?;
        Ok(Tls { config: Arc::new(config), name, verifier })
    }

    /// Starts TLS on a connection and finishes the greeting, so a certificate that isn't trusted is found now.
    fn wrap(&self, tcp: TcpStream) -> Result<Stream, std::io::Error> {
        let connection = rustls::ClientConnection::new(self.config.clone(), self.name.clone()).map_err(std::io::Error::other)?;
        let mut tls = Box::new(rustls::StreamOwned::new(connection, tcp));
        while tls.conn.is_handshaking() {
            tls.conn.complete_io(&mut tls.sock)?;
        }
        Ok(Stream::Tls(tls))
    }

    /// Why the greeting failed, in the terms of remote files: a certificate to be trusted, one that has changed,
    /// or something else.
    fn refusal(&self, host: &str, problem: &std::io::Error) -> Refused {
        let seen = self.verifier.seen.lock().unwrap_or_else(std::sync::PoisonError::into_inner).clone();
        let about_certificate = problem.get_ref().and_then(|inner| inner.downcast_ref::<rustls::Error>()).is_some_and(|e| matches!(e, rustls::Error::InvalidCertificate(_)));
        match (seen, &self.verifier.trusted) {
            (Some(found), None) if about_certificate => Refused::UnknownHost(found),
            (Some(found), Some(expected)) if about_certificate => Refused::HostKeyChanged { expected: expected.clone(), found },
            _ => Refused::Failed(crate::i18n::tf!("The TLS greeting with {host} failed: {problem}")),
        }
    }
}

// ---------- The connection ----------

/// A signed-in FTP connection.
pub struct Client {
    control: Stream,
    /// What has been read from the control connection but not used yet.
    pending: Vec<u8>,
    peer: IpAddr,
    tls: Option<Tls>,
    /// Whether files and listings go under TLS too (PROT P).
    protect_data: bool,
    /// What the server says it can do, from FEAT.
    mlsd: bool,
    mlst: bool,
    mfmt: bool,
}

fn connect_tcp(host: &str, port: u16) -> Result<TcpStream, Refused> {
    let host = host.trim().trim_matches(|c| c == '[' || c == ']');
    let addresses: Vec<_> = (host, port).to_socket_addrs().map_err(|e| Refused::Failed(crate::i18n::tf!("{host} couldn't be found: {e}")))?.collect();
    let mut last = Refused::Failed(crate::i18n::tf!("{host} couldn't be found."));
    for address in addresses {
        match TcpStream::connect_timeout(&address, TIMEOUT) {
            Ok(stream) => {
                stream.set_read_timeout(Some(TIMEOUT)).ok();
                stream.set_write_timeout(Some(TIMEOUT)).ok();
                return Ok(stream);
            }
            Err(e) => last = Refused::Failed(crate::i18n::tf!("Couldn't reach {host} on port {port}: {e}")),
        }
    }
    Err(last)
}

/// Connects and signs in. `trusted` is the fingerprint of a certificate trusted before, for FTPS.
pub fn connect(host: &str, port: u16, security: Security, user: &str, password: &str, trusted: Option<&str>) -> Result<Client, Refused> {
    let tcp = connect_tcp(host, port)?;
    let peer = tcp.peer_addr().map_err(|e| Refused::Failed(e.to_string()))?.ip();
    let tls = match security {
        Security::Plain => None,
        _ => Some(Tls::new(host, trusted).map_err(Refused::Failed)?),
    };
    let control = match (&tls, security) {
        (Some(tls), Security::Implicit) => tls.wrap(tcp).map_err(|e| tls.refusal(host, &e))?,
        _ => Stream::Plain(tcp),
    };
    let mut client = Client { control, pending: Vec::new(), peer, tls, protect_data: false, mlsd: false, mlst: false, mfmt: false };
    client.expect(&[220]).map_err(|text| Refused::Failed(crate::i18n::tf!("{host} didn't greet as an FTP server does.\n\n{text}")))?;
    if security == Security::Explicit {
        if client.command("AUTH TLS", &[234]).is_err() {
            client.command("AUTH SSL", &[234]).map_err(|text| Refused::Failed(crate::i18n::tf!("{host} doesn't offer TLS, so nothing was sent. An ftp:// address connects without it, password and all in the open.\n\n{text}")))?;
        }
        // A second handle on the same socket, for TLS to take over; the plain one is let go once it has.
        let tcp = client.control.tcp().try_clone().map_err(|e| Refused::Failed(e.to_string()))?;
        let tls = client.tls.as_ref().expect("TLS settings are made for FTPS");
        client.control = tls.wrap(tcp).map_err(|e| tls.refusal(host, &e))?;
    }
    client.sign_in(user, password)?;
    if client.tls.is_some() {
        // Files and listings under TLS too, where the server allows; otherwise only the control connection,
        // with the password, is private.
        client.protect_data = client.command("PBSZ 0", &[200]).is_ok() && client.command("PROT P", &[200]).is_ok();
    }
    if let Ok(features) = client.command("FEAT", &[211]) {
        for line in features.lines().skip(1) {
            let word = line.split_whitespace().next().unwrap_or("").to_ascii_uppercase();
            match word.as_str() {
                "MLSD" => client.mlsd = true,
                "MLST" => {
                    client.mlst = true;
                    client.mlsd = true;
                }
                "MFMT" => client.mfmt = true,
                "UTF8" => {
                    let _ = client.command("OPTS UTF8 ON", &[200, 202]);
                }
                _ => {}
            }
        }
    }
    client.command("TYPE I", &[200]).map_err(Refused::Failed)?;
    Ok(client)
}

impl Client {
    /// One line from the control connection, without its line ending.
    fn read_line(&mut self) -> Result<String, String> {
        loop {
            if let Some(end) = self.pending.iter().position(|&b| b == b'\n') {
                let line: Vec<u8> = self.pending.drain(..=end).collect();
                return Ok(String::from_utf8_lossy(&line).trim_end_matches(['\r', '\n']).to_string());
            }
            let mut buffer = [0u8; 4096];
            let got = self.control.read(&mut buffer).map_err(|e| crate::i18n::tf!("The server stopped answering: {e}"))?;
            if got == 0 {
                return Err(crate::i18n::tr("The server closed the connection.").into());
            }
            self.pending.extend_from_slice(&buffer[..got]);
        }
    }

    /// Reads one reply, of one line or several: its number and all its text.
    fn reply(&mut self) -> Result<(u16, String), String> {
        let first = self.read_line()?;
        let bytes = first.as_bytes();
        if bytes.len() < 3 || !bytes[..3].iter().all(u8::is_ascii_digit) {
            return Err(crate::i18n::tf!("The server said something unexpected: {first}"));
        }
        let code: u16 = first[..3].parse().unwrap_or(0);
        let mut text = first.clone();
        if bytes.get(3) == Some(&b'-') {
            // Several lines, ending with one that starts with the same number and a space.
            loop {
                let line = self.read_line()?;
                text.push('\n');
                text.push_str(&line);
                if line.len() >= 4 && line.starts_with(&first[..3]) && line.as_bytes()[3] == b' ' {
                    break;
                }
            }
        }
        Ok((code, text))
    }

    fn expect(&mut self, codes: &[u16]) -> Result<String, String> {
        let (code, text) = self.reply()?;
        if codes.contains(&code) { Ok(text) } else { Err(text) }
    }

    fn send(&mut self, line: &str) -> Result<(), String> {
        self.control.write_all(format!("{line}\r\n").as_bytes()).and_then(|()| self.control.flush()).map_err(|e| crate::i18n::tf!("The server stopped answering: {e}"))
    }

    fn command(&mut self, line: &str, codes: &[u16]) -> Result<String, String> {
        self.send(line)?;
        self.expect(codes)
    }

    fn sign_in(&mut self, user: &str, password: &str) -> Result<(), Refused> {
        let user = if user.is_empty() { "anonymous" } else { user };
        self.send(&format!("USER {user}")).map_err(Refused::Failed)?;
        match self.reply().map_err(Refused::Failed)? {
            (230, _) => Ok(()),
            (331 | 332, _) => match self.command(&format!("PASS {password}"), &[230, 202]) {
                Ok(_) => Ok(()),
                Err(text) => Err(Refused::SignIn(text)),
            },
            (_, text) => Err(Refused::SignIn(text)),
        }
    }

    /// The folder the server starts in.
    pub fn home(&mut self) -> Result<String, String> {
        let text = self.command("PWD", &[257])?;
        quoted(&text).ok_or_else(|| crate::i18n::tf!("The server's answer couldn't be read: {text}"))
    }

    /// Opens a passive connection for a file or listing. The address the server gives is ignored in favour of the
    /// one connected to, as servers behind a router often give their inside address.
    fn data(&mut self) -> Result<TcpStream, String> {
        let port = match self.command("EPSV", &[229]) {
            Ok(text) => text.split('(').nth(1).and_then(|rest| rest.split(')').next()).and_then(|inside| inside.trim_matches('|').parse::<u16>().ok()),
            Err(_) => {
                let text = self.command("PASV", &[227])?;
                let inside = text.split_once('(').map_or(text.get(3..).unwrap_or_default(), |(_, rest)| rest);
                let numbers: Vec<u16> = inside.split(|c: char| !c.is_ascii_digit()).filter(|s| !s.is_empty()).filter_map(|s| s.parse().ok()).take(6).collect();
                (numbers.len() == 6).then(|| numbers[4] * 256 + numbers[5])
            }
        };
        let port = port.ok_or(crate::i18n::tr("The server's passive reply couldn't be read."))?;
        let stream = TcpStream::connect_timeout(&(self.peer, port).into(), TIMEOUT).map_err(|e| crate::i18n::tf!("Couldn't open a connection for the file: {e}"))?;
        stream.set_read_timeout(Some(TIMEOUT)).ok();
        stream.set_write_timeout(Some(TIMEOUT)).ok();
        Ok(stream)
    }

    /// Asks for a transfer: a passive connection, the command on it, and TLS on it once the server has agreed.
    fn transfer(&mut self, line: &str) -> Result<Stream, String> {
        let tcp = self.data()?;
        self.command(line, &[125, 150])?;
        match &self.tls {
            Some(tls) if self.protect_data => tls.wrap(tcp).map_err(|e| crate::i18n::tf!("TLS on the connection for the file failed: {e}")),
            _ => Ok(Stream::Plain(tcp)),
        }
    }

    /// Everything a transfer sends.
    fn fetch_all(&mut self, line: &str) -> Result<Vec<u8>, String> {
        let mut data = self.transfer(line)?;
        let mut bytes = Vec::new();
        let read = data.read_to_end(&mut bytes);
        drop(data);
        let done = self.expect(&[226, 250]);
        read.map_err(|e| e.to_string())?;
        done?;
        Ok(bytes)
    }

    /// What's in a folder: folders first, then files, each by name.
    pub fn list(&mut self, folder: &str) -> Result<Vec<Entry>, String> {
        let mut entries = if self.mlsd {
            let text = self.fetch_all(&format!("MLSD {folder}")).map_err(|e| crate::i18n::tf!("{folder} couldn't be listed: {e}"))?;
            String::from_utf8_lossy(&text).lines().filter_map(parse_mlsx).collect::<Vec<_>>()
        } else {
            // LIST takes a path; a folder's name with spaces in it can confuse an older server, so it's gone to.
            self.command(&format!("CWD {folder}"), &[250]).map_err(|e| crate::i18n::tf!("{folder} couldn't be listed: {e}"))?;
            let text = self.fetch_all("LIST").map_err(|e| crate::i18n::tf!("{folder} couldn't be listed: {e}"))?;
            let now = chrono::Utc::now();
            String::from_utf8_lossy(&text).lines().filter_map(|line| parse_list(line, now)).collect()
        };
        entries.retain(|entry| entry.name != "." && entry.name != "..");
        entries.sort_by(|a, b| b.folder.cmp(&a.folder).then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase())));
        Ok(entries)
    }

    /// Whether something is at a path: if so, whether it's a folder, and its size and time.
    pub fn look(&mut self, path: &str) -> Result<Option<(bool, Stamp)>, String> {
        let path = path.trim_end_matches('/');
        if path.is_empty() {
            return Ok(Some((true, Stamp::default())));
        }
        if self.mlst {
            self.send(&format!("MLST {path}"))?;
            return match self.reply()? {
                (250, text) => Ok(text.lines().skip(1).find_map(parse_mlsx).map(|entry| (entry.folder, Stamp { size: entry.size, modified: entry.modified }))),
                (450 | 550, _) => Ok(None),
                (_, text) => Err(text),
            };
        }
        // Without MLST, the folder it's in is listed.
        let (folder, name) = match path.rsplit_once('/') {
            Some(("", name)) => ("/".to_string(), name),
            Some((folder, name)) => (folder.to_string(), name),
            None => (self.home()?, path),
        };
        match self.list(&folder) {
            Ok(entries) => Ok(entries.into_iter().find(|entry| entry.name == name).map(|entry| (entry.folder, Stamp { size: entry.size, modified: entry.modified }))),
            // The folder it would be in isn't there either.
            Err(_) if self.command(&format!("CWD {folder}"), &[250]).is_err() => Ok(None),
            Err(problem) => Err(problem),
        }
    }

    /// Copies a remote file to a local one, counting the bytes as they come.
    pub fn download(&mut self, remote: &str, local: &Path, done: &AtomicU64, stop: &AtomicBool) -> Result<Stamp, String> {
        let stamp = match self.look(remote)? {
            Some((true, _)) => return Err(crate::i18n::tf!("{remote} is a folder.")),
            Some((false, stamp)) => stamp,
            None => return Err(crate::i18n::tf!("{remote} couldn't be found.")),
        };
        if let Some(folder) = local.parent() {
            std::fs::create_dir_all(folder).map_err(|e| e.to_string())?;
        }
        let mut to = std::io::BufWriter::new(std::fs::File::create(local).map_err(|e| e.to_string())?);
        let mut data = self.transfer(&format!("RETR {remote}")).map_err(|e| crate::i18n::tf!("{remote} couldn't be opened: {e}"))?;
        let copied = pump(&mut data, &mut to, done, stop);
        drop(data);
        if copied.is_err() {
            // Stopped part way: the server's reply, whatever it is, is read so the connection stays in step.
            let _ = self.reply();
        } else {
            self.expect(&[226, 250]).map_err(|e| crate::i18n::tf!("Reading {remote} failed: {e}"))?;
        }
        copied?;
        to.flush().map_err(|e| e.to_string())?;
        Ok(stamp)
    }

    /// Sends a local file in place of a remote one, with the same care as over SFTP: to a new file beside it
    /// first, then put in its place, so the file on the server is whole all the time. Unless `force`, it's
    /// refused if the remote file has changed since `expected`.
    pub fn upload(&mut self, local: &Path, remote: &str, expected: Option<Stamp>, force: bool, done: &AtomicU64, stop: &AtomicBool) -> Result<Stamp, UploadError> {
        let before = self.look(remote).map_err(UploadError::Failed)?;
        if !force
            && let (Some(expected), Some((_, now))) = (expected, before)
            && now != expected
        {
            return Err(UploadError::ChangedOnServer);
        }
        let temp = format!("{remote}.limoniter-tmp");
        let result = (|| -> Result<(), String> {
            let mut from = std::fs::File::open(local).map_err(|e| e.to_string())?;
            let mut data = self.transfer(&format!("STOR {temp}")).map_err(|e| crate::i18n::tf!("{temp} couldn't be made: {e}"))?;
            let sent = pump(&mut from, &mut data, done, stop);
            let finished = data.finish();
            let reply = self.expect(&[226, 250]);
            sent?;
            finished.map_err(|e| crate::i18n::tf!("Sending failed: {e}"))?;
            reply.map_err(|e| crate::i18n::tf!("Sending failed: {e}"))?;
            self.replace(&temp, remote, before.is_some())
        })();
        if let Err(problem) = result {
            let _ = self.command(&format!("DELE {temp}"), &[250]);
            return Err(UploadError::Failed(problem));
        }
        match self.look(remote) {
            Ok(Some((_, stamp))) => Ok(stamp),
            Ok(None) => Err(UploadError::Failed(crate::i18n::tf!("{remote} isn't there after sending it."))),
            Err(problem) => Err(UploadError::Failed(problem)),
        }
    }

    /// Puts `temp` where `target` is. Some servers won't rename over a file, so the old one is moved aside first,
    /// and put back if that fails.
    fn replace(&mut self, temp: &str, target: &str, exists: bool) -> Result<(), String> {
        if self.rename(temp, target).is_ok() {
            return Ok(());
        }
        if !exists {
            return self.rename(temp, target);
        }
        let aside = format!("{target}.limoniter-old");
        let _ = self.command(&format!("DELE {aside}"), &[250]);
        self.rename(target, &aside).map_err(|e| crate::i18n::tf!("{target} couldn't be replaced: {e}"))?;
        if let Err(e) = self.rename(temp, target) {
            let _ = self.rename(&aside, target);
            return Err(crate::i18n::tf!("{target} couldn't be replaced: {e}"));
        }
        let _ = self.command(&format!("DELE {aside}"), &[250]);
        Ok(())
    }

    pub fn rename(&mut self, from: &str, to: &str) -> Result<(), String> {
        self.command(&format!("RNFR {from}"), &[350]).and_then(|_| self.command(&format!("RNTO {to}"), &[250])).map(|_| ()).map_err(|e| crate::i18n::tf!("{from} couldn't be renamed: {e}"))
    }

    pub fn make_folder(&mut self, remote: &str) -> Result<(), String> {
        self.command(&format!("MKD {remote}"), &[257, 250]).map(|_| ()).map_err(|e| crate::i18n::tf!("{remote} couldn't be made: {e}"))
    }

    pub fn remove_file(&mut self, remote: &str) -> Result<(), String> {
        self.command(&format!("DELE {remote}"), &[250]).map(|_| ()).map_err(|e| crate::i18n::tf!("{remote} couldn't be taken away: {e}"))
    }

    pub fn remove_folder(&mut self, remote: &str) -> Result<(), String> {
        self.command(&format!("RMD {remote}"), &[250]).map(|_| ()).map_err(|e| crate::i18n::tf!("{remote} couldn't be taken away: {e}"))
    }

    /// Gives a file the modified time it's to have, where the server can (MFMT).
    pub fn set_modified(&mut self, remote: &str, time: u64) -> Result<(), String> {
        if !self.mfmt {
            return Err(crate::i18n::tr("The server can't set a file's time.").into());
        }
        let when = chrono::DateTime::from_timestamp(time as i64, 0).ok_or("?")?.format("%Y%m%d%H%M%S");
        self.command(&format!("MFMT {when} {remote}"), &[213, 250]).map(|_| ())
    }

}

impl Drop for Client {
    fn drop(&mut self) {
        // Ends TLS properly, so the server doesn't log an error.
        if let Stream::Tls(tls) = &mut self.control {
            tls.conn.send_close_notify();
            let _ = tls.flush();
        }
    }
}

fn pump(from: &mut dyn Read, to: &mut dyn Write, done: &AtomicU64, stop: &AtomicBool) -> Result<(), String> {
    let mut buffer = vec![0u8; CHUNK];
    let mut total = 0u64;
    loop {
        if stop.load(Ordering::Relaxed) {
            return Err(crate::i18n::tr("It was stopped.").to_string());
        }
        let got = from.read(&mut buffer).map_err(|e| e.to_string())?;
        if got == 0 {
            return Ok(());
        }
        to.write_all(&buffer[..got]).map_err(|e| e.to_string())?;
        total += got as u64;
        done.store(total, Ordering::Relaxed);
    }
}

/// The path in a reply such as 257 "/home/ann" is the current folder, with "" for a quote inside it.
fn quoted(text: &str) -> Option<String> {
    let start = text.find('"')? + 1;
    let mut out = String::new();
    let mut chars = text[start..].chars().peekable();
    while let Some(c) = chars.next() {
        if c == '"' {
            if chars.peek() == Some(&'"') {
                chars.next();
                out.push('"');
            } else {
                return Some(out);
            }
        } else {
            out.push(c);
        }
    }
    None
}

/// A time as MLSD and MDTM give it, YYYYMMDDHHMMSS in UTC (perhaps with fractions of a second), in seconds since 1970.
fn mlsx_time(text: &str) -> Option<u64> {
    let digits = text.get(..14)?;
    let time = chrono::NaiveDateTime::parse_from_str(digits, "%Y%m%d%H%M%S").ok()?;
    u64::try_from(time.and_utc().timestamp()).ok()
}

/// One line of MLSD (or the second of MLST): facts such as type=file;size=10;modify=20240101120000; then a space
/// and the name.
fn parse_mlsx(line: &str) -> Option<Entry> {
    let line = line.trim_start_matches(' ');
    let (facts, name) = line.split_once(' ')?;
    let mut entry = Entry { name: name.rsplit('/').next().unwrap_or(name).to_string(), folder: false, size: 0, modified: 0 };
    for fact in facts.split(';') {
        let Some((key, value)) = fact.split_once('=') else { continue };
        match key.to_ascii_lowercase().as_str() {
            "type" => match value.to_ascii_lowercase().as_str() {
                "dir" => entry.folder = true,
                // The folder itself and the one above.
                "cdir" | "pdir" => return None,
                _ => {}
            },
            "size" => entry.size = value.parse().unwrap_or(0),
            "modify" => entry.modified = mlsx_time(value).unwrap_or(0),
            _ => {}
        }
    }
    (!entry.name.is_empty()).then_some(entry)
}

const MONTHS: [&str; 12] = ["jan", "feb", "mar", "apr", "may", "jun", "jul", "aug", "sep", "oct", "nov", "dec"];

/// One line of LIST, as Unix servers write it (drwxr-xr-x 2 ann staff 4096 Mar 3 12:00 name) or as Windows's
/// does (03-03-24  12:00PM  <DIR>  name). The times are the server's own, taken as UTC; a recent one has no year,
/// so it's this year unless that would be in the future.
fn parse_list(line: &str, now: chrono::DateTime<chrono::Utc>) -> Option<Entry> {
    use chrono::{Datelike, NaiveDate, NaiveTime};
    let fields: Vec<&str> = line.split_whitespace().collect();
    // The name is everything after the nth field, spaces and all.
    let rest_after = |n: usize| -> Option<&str> {
        let mut at = line;
        for _ in 0..n {
            at = at.trim_start();
            at = &at[at.find(char::is_whitespace)?..];
        }
        Some(at.trim_start())
    };
    let first = *fields.first()?;
    if first.len() >= 10 && "-dlcbps".contains(&first[..1]) && fields.len() >= 9 {
        // Unix: permissions, links, owner, group, size, month, day, year or time, name.
        let month = MONTHS.iter().position(|m| fields[5].to_ascii_lowercase().starts_with(m))? as u32 + 1;
        let day: u32 = fields[6].parse().ok()?;
        let (year, time) = match fields[7].split_once(':') {
            Some((h, m)) => {
                let time = NaiveTime::from_hms_opt(h.parse().ok()?, m.parse().ok()?, 0)?;
                let this_year = NaiveDate::from_ymd_opt(now.year(), month, day)?.and_time(time).and_utc();
                (if this_year > now + chrono::Duration::days(1) { now.year() - 1 } else { now.year() }, time)
            }
            None => (fields[7].parse().ok()?, NaiveTime::MIN),
        };
        let modified = NaiveDate::from_ymd_opt(year, month, day)?.and_time(time).and_utc().timestamp().max(0) as u64;
        let mut name = rest_after(8)?.to_string();
        if first.starts_with('l')
            && let Some((link, _)) = name.split_once(" -> ")
        {
            name = link.to_string();
        }
        return Some(Entry { name, folder: first.starts_with('d'), size: fields[4].parse().unwrap_or(0), modified });
    }
    if fields.len() >= 4 && first.len() >= 8 && first.as_bytes()[2] == b'-' {
        // Windows: date, time, <DIR> or size, name.
        let date = NaiveDate::parse_from_str(first, "%m-%d-%y").or_else(|_| NaiveDate::parse_from_str(first, "%m-%d-%Y")).ok()?;
        let time = NaiveTime::parse_from_str(&fields[1].to_ascii_uppercase(), "%I:%M%p").or_else(|_| NaiveTime::parse_from_str(fields[1], "%H:%M")).ok()?;
        let folder = fields[2].eq_ignore_ascii_case("<DIR>");
        return Some(Entry { name: rest_after(3)?.to_string(), folder, size: if folder { 0 } else { fields[2].parse().unwrap_or(0) }, modified: date.and_time(time).and_utc().timestamp().max(0) as u64 });
    }
    None
}

#[cfg(test)]
#[path = "ftp_tests.rs"]
pub(crate) mod ftp_tests;
