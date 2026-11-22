//! ZMTP 3.0 and 3.1, the wire protocol ZeroMQ speaks, written out here so Limoniter can talk to Jupyter kernels
//! without a ZeroMQ library. Only what Jupyter needs is here: TCP, the NULL mechanism (no encryption: the kernel
//! listens on 127.0.0.1 only, and its messages are signed), and the socket types a Jupyter client uses (DEALER,
//! SUB and REQ), plus the kernel's side of each (ROUTER, PUB and REP) so the tests can play a kernel.
//!
//! A connection starts with a 64-byte greeting from each side, then each sends a READY command naming its socket
//! type. After that come frames: a flags byte (MORE, LONG, COMMAND), a length (one byte, or eight for a long
//! frame) and the body. A message is a run of frames, all but the last with MORE set.

use std::io::{self, BufReader, Read, Write};
use std::net::{Shutdown, SocketAddr, TcpStream};
use std::sync::{Arc, Mutex};
use std::time::Duration;

/// More frames of this message follow.
pub const MORE: u8 = 0x01;
/// The length is eight bytes rather than one.
pub const LONG: u8 = 0x02;
/// A command (READY, PING, SUBSCRIBE…) rather than part of a message.
pub const COMMAND: u8 = 0x04;

/// The newest minor version spoken: 3.1, which adds PING, PONG and the SUBSCRIBE command.
pub const MINOR: u8 = 1;

/// No frame larger than this is accepted, so that a broken peer can't make the program ask for all its memory.
const LARGEST_FRAME: u64 = 1 << 30;

/// The kinds of ZeroMQ socket spoken here.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SocketType {
    Dealer,
    Router,
    Sub,
    Pub,
    XPub,
    Req,
    Rep,
}

impl SocketType {
    pub fn wire_name(self) -> &'static str {
        match self {
            SocketType::Dealer => "DEALER",
            SocketType::Router => "ROUTER",
            SocketType::Sub => "SUB",
            SocketType::Pub => "PUB",
            SocketType::XPub => "XPUB",
            SocketType::Req => "REQ",
            SocketType::Rep => "REP",
        }
    }

    pub fn from_name(name: &[u8]) -> Option<SocketType> {
        [SocketType::Dealer, SocketType::Router, SocketType::Sub, SocketType::Pub, SocketType::XPub, SocketType::Req, SocketType::Rep]
            .into_iter()
            .find(|kind| kind.wire_name().as_bytes() == name)
    }

    /// Whether ZeroMQ lets these two talk, as the ZMTP specification lists them.
    pub fn fits(self, peer: SocketType) -> bool {
        use SocketType::*;
        matches!(
            (self, peer),
            (Dealer, Router | Rep | Dealer)
                | (Router, Dealer | Req | Router)
                | (Sub, Pub | XPub)
                | (Pub | XPub, Sub)
                | (Req, Rep | Router)
                | (Rep, Req | Dealer)
        )
    }

    /// Whether the socket may carry an Identity in its READY command (the sockets a ROUTER can tell apart).
    fn has_identity(self) -> bool {
        matches!(self, SocketType::Dealer | SocketType::Req | SocketType::Router)
    }
}

/// The 64 bytes each side sends first: the signature, the version, the mechanism (NULL) and whether this side is
/// the server, which NULL doesn't use.
pub fn greeting(minor: u8, as_server: bool) -> [u8; 64] {
    let mut bytes = [0u8; 64];
    bytes[0] = 0xFF;
    bytes[9] = 0x7F;
    bytes[10] = 3;
    bytes[11] = minor;
    bytes[12..16].copy_from_slice(b"NULL");
    bytes[32] = u8::from(as_server);
    bytes
}

/// One frame as it goes on the wire.
pub fn frame(body: &[u8], more: bool, command: bool) -> Vec<u8> {
    let mut flags = 0;
    if more {
        flags |= MORE;
    }
    if command {
        flags |= COMMAND;
    }
    let mut bytes = Vec::with_capacity(body.len() + 9);
    if body.len() > 255 {
        bytes.push(flags | LONG);
        bytes.extend_from_slice(&(body.len() as u64).to_be_bytes());
    } else {
        bytes.push(flags);
        bytes.push(body.len() as u8);
    }
    bytes.extend_from_slice(body);
    bytes
}

/// A command's body: its name, preceded by the name's length, then its data.
pub fn command_body(name: &str, data: &[u8]) -> Vec<u8> {
    let mut body = Vec::with_capacity(1 + name.len() + data.len());
    body.push(name.len() as u8);
    body.extend_from_slice(name.as_bytes());
    body.extend_from_slice(data);
    body
}

/// The properties of a READY command: Socket-Type always, and Identity where the socket type has one.
pub fn ready_properties(kind: SocketType, identity: Option<&[u8]>) -> Vec<u8> {
    let mut data = Vec::new();
    let mut property = |name: &str, value: &[u8]| {
        data.push(name.len() as u8);
        data.extend_from_slice(name.as_bytes());
        data.extend_from_slice(&(value.len() as u32).to_be_bytes());
        data.extend_from_slice(value);
    };
    property("Socket-Type", kind.wire_name().as_bytes());
    if let Some(identity) = identity.filter(|_| kind.has_identity()) {
        property("Identity", identity);
    }
    data
}

/// A READY command's properties read back, as name and value.
pub fn parse_properties(mut data: &[u8]) -> io::Result<Vec<(String, Vec<u8>)>> {
    let mut properties = Vec::new();
    while !data.is_empty() {
        let name_length = usize::from(data[0]);
        let value_start = 1 + name_length + 4;
        if data.len() < value_start {
            return Err(broken("a READY property is cut short"));
        }
        let name = String::from_utf8_lossy(&data[1..1 + name_length]).into_owned();
        let value_length = u32::from_be_bytes([data[1 + name_length], data[2 + name_length], data[3 + name_length], data[4 + name_length]]) as usize;
        if data.len() < value_start + value_length {
            return Err(broken("a READY property's value is cut short"));
        }
        properties.push((name, data[value_start..value_start + value_length].to_vec()));
        data = &data[value_start + value_length..];
    }
    Ok(properties)
}

fn broken(what: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, what.to_string())
}

/// One frame read from the wire: its flags and its body.
pub fn read_frame(reader: &mut impl Read) -> io::Result<(u8, Vec<u8>)> {
    let mut flags = [0u8; 1];
    reader.read_exact(&mut flags)?;
    let flags = flags[0];
    if flags & !(MORE | LONG | COMMAND) != 0 {
        return Err(broken("a frame has flags ZMTP doesn't know"));
    }
    let length = if flags & LONG != 0 {
        let mut length = [0u8; 8];
        reader.read_exact(&mut length)?;
        u64::from_be_bytes(length)
    } else {
        let mut length = [0u8; 1];
        reader.read_exact(&mut length)?;
        u64::from(length[0])
    };
    if length > LARGEST_FRAME {
        return Err(broken("a frame is too large"));
    }
    let mut body = vec![0u8; length as usize];
    reader.read_exact(&mut body)?;
    Ok((flags, body))
}

/// A command split into its name and data.
fn split_command(body: &[u8]) -> io::Result<(String, Vec<u8>)> {
    let length = usize::from(*body.first().ok_or_else(|| broken("an empty command"))?);
    if body.len() < 1 + length {
        return Err(broken("a command's name is cut short"));
    }
    Ok((String::from_utf8_lossy(&body[1..1 + length]).into_owned(), body[1 + length..].to_vec()))
}

/// What the other side said of itself in its greeting and READY.
#[derive(Clone, Debug)]
#[allow(dead_code)] // Read in the tests, and kept for looking at a connection in a debugger.
pub struct Peer {
    pub kind: SocketType,
    pub minor: u8,
    pub identity: Option<Vec<u8>>,
}

/// The sending half of a connection, shared between threads.
#[derive(Clone)]
pub struct Sender {
    stream: Arc<Mutex<TcpStream>>,
    /// The same socket, kept outside the lock so it can be closed while another thread is writing.
    raw: Arc<TcpStream>,
    kind: SocketType,
    peer_minor: u8,
}

impl Sender {
    /// Sends one message. A REQ socket puts the empty frame in front that REP and ROUTER expect.
    pub fn send<B: AsRef<[u8]>>(&self, frames: &[B]) -> io::Result<()> {
        let mut bytes = Vec::new();
        if self.kind == SocketType::Req {
            bytes.extend(frame(&[], true, false));
        }
        for (i, body) in frames.iter().enumerate() {
            bytes.extend(frame(body.as_ref(), i + 1 < frames.len(), false));
        }
        self.write(&bytes)
    }

    pub fn send_command(&self, name: &str, data: &[u8]) -> io::Result<()> {
        self.write(&frame(&command_body(name, data), false, true))
    }

    /// Asks a PUB for everything starting with `prefix` (empty for everything): a SUBSCRIBE command for ZMTP
    /// 3.1, a message starting with 1 for 3.0.
    pub fn subscribe(&self, prefix: &[u8]) -> io::Result<()> {
        if self.peer_minor >= 1 {
            self.send_command("SUBSCRIBE", prefix)
        } else {
            let mut body = vec![1u8];
            body.extend_from_slice(prefix);
            self.write(&frame(&body, false, false))
        }
    }

    fn write(&self, bytes: &[u8]) -> io::Result<()> {
        let mut stream = self.stream.lock().unwrap_or_else(|e| e.into_inner());
        stream.write_all(bytes)?;
        stream.flush()
    }

    /// Closes the connection, which also wakes a thread waiting in `Receiver::recv`.
    pub fn close(&self) {
        let _ = self.raw.shutdown(Shutdown::Both);
    }
}

/// Something read from a connection.
#[derive(Debug, PartialEq, Eq)]
pub enum Incoming {
    Message(Vec<Vec<u8>>),
    Command(String, Vec<u8>),
}

/// The receiving half of a connection, used by one thread.
pub struct Receiver {
    reader: BufReader<TcpStream>,
    sender: Sender,
}

impl Receiver {
    /// The next message or command. PING is answered here with PONG.
    pub fn recv_any(&mut self) -> io::Result<Incoming> {
        let mut frames = Vec::new();
        loop {
            let (flags, body) = read_frame(&mut self.reader)?;
            if flags & COMMAND != 0 {
                if !frames.is_empty() {
                    return Err(broken("a command came in the middle of a message"));
                }
                let (name, data) = split_command(&body)?;
                if name == "PING" {
                    // The PING's data is a time-to-live (two bytes) and then a context, which PONG gives back.
                    let context = data.get(2..).unwrap_or_default();
                    self.sender.send_command("PONG", context)?;
                }
                return Ok(Incoming::Command(name, data));
            }
            frames.push(body);
            if flags & MORE == 0 {
                break;
            }
        }
        if self.sender.kind == SocketType::Req {
            // REP and ROUTER send the empty frame back first; REQ takes it away again.
            if frames.first().is_some_and(|f| f.is_empty()) {
                frames.remove(0);
            }
        }
        Ok(Incoming::Message(frames))
    }

    /// The next message, passing over commands.
    pub fn recv(&mut self) -> io::Result<Vec<Vec<u8>>> {
        loop {
            if let Incoming::Message(frames) = self.recv_any()? {
                return Ok(frames);
            }
        }
    }

    /// Waits no longer than this for each read, or for ever with None.
    pub fn set_timeout(&self, timeout: Option<Duration>) -> io::Result<()> {
        self.reader.get_ref().set_read_timeout(timeout)
    }

}

/// Connects to a ZeroMQ socket on TCP, as this kind of socket, and greets it.
pub fn connect(address: SocketAddr, kind: SocketType, identity: Option<&[u8]>, timeout: Duration) -> io::Result<(Sender, Receiver, Peer)> {
    let stream = TcpStream::connect_timeout(&address, timeout)?;
    handshake(stream, kind, identity, false, MINOR, timeout)
}

/// The greeting and READY on a connected stream, from either end. `minor` is the version this side claims (the
/// tests also play a 3.0 peer). Reads wait no longer than `timeout` during the handshake and for ever afterwards.
pub fn handshake(stream: TcpStream, kind: SocketType, identity: Option<&[u8]>, as_server: bool, minor: u8, timeout: Duration) -> io::Result<(Sender, Receiver, Peer)> {
    stream.set_nodelay(true)?;
    stream.set_read_timeout(Some(timeout))?;
    let mut writer = stream.try_clone()?;
    let raw = Arc::new(stream.try_clone()?);
    let mut reader = BufReader::new(stream);

    writer.write_all(&greeting(minor, as_server))?;
    writer.flush()?;
    let mut theirs = [0u8; 64];
    reader.read_exact(&mut theirs)?;
    if theirs[0] != 0xFF || theirs[9] & 0x01 != 0x01 {
        return Err(broken("the other end isn't speaking ZeroMQ"));
    }
    if theirs[10] < 3 {
        return Err(broken("the other end speaks an older ZeroMQ (before ZMTP 3.0)"));
    }
    let mechanism = &theirs[12..32];
    if mechanism.split(|&b| b == 0).next() != Some(b"NULL".as_slice()) {
        return Err(broken("the other end wants a ZeroMQ security mechanism other than NULL"));
    }
    // With a newer major version, the peer falls back to ours; its minor counts only when the majors match.
    let peer_minor = if theirs[10] == 3 { theirs[11].min(minor) } else { minor };

    writer.write_all(&frame(&command_body("READY", &ready_properties(kind, identity)), false, true))?;
    writer.flush()?;
    let (flags, body) = read_frame(&mut reader)?;
    if flags & COMMAND == 0 {
        return Err(broken("the other end sent a message before READY"));
    }
    let (name, data) = split_command(&body)?;
    if name == "ERROR" {
        let length = usize::from(data.first().copied().unwrap_or(0));
        let reason = String::from_utf8_lossy(data.get(1..1 + length).unwrap_or_default()).into_owned();
        return Err(io::Error::new(io::ErrorKind::ConnectionRefused, reason));
    }
    if name != "READY" {
        return Err(broken("the other end didn't send READY"));
    }
    let properties = parse_properties(&data)?;
    let peer_kind = properties
        .iter()
        .find(|(name, _)| name.eq_ignore_ascii_case("Socket-Type"))
        .and_then(|(_, value)| SocketType::from_name(value))
        .ok_or_else(|| broken("the other end didn't say what kind of socket it is"))?;
    if !kind.fits(peer_kind) {
        return Err(broken("the other end is a kind of socket this one can't talk to"));
    }
    let identity = properties.into_iter().find(|(name, _)| name.eq_ignore_ascii_case("Identity")).map(|(_, value)| value);
    reader.get_ref().set_read_timeout(None)?;

    let sender = Sender { stream: Arc::new(Mutex::new(writer)), raw, kind, peer_minor };
    let receiver = Receiver { reader, sender: sender.clone() };
    Ok((sender, receiver, Peer { kind: peer_kind, minor: peer_minor, identity }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::TcpListener;

    const WAIT: Duration = Duration::from_secs(5);

    #[test]
    fn the_greeting_is_as_the_specification_draws_it() {
        let bytes = greeting(1, false);
        assert_eq!(&bytes[..12], &[0xFF, 0, 0, 0, 0, 0, 0, 0, 0, 0x7F, 3, 1]);
        assert_eq!(&bytes[12..20], b"NULL\0\0\0\0");
        assert!(bytes[20..].iter().all(|&b| b == 0));
        assert_eq!(greeting(0, true)[32], 1);
    }

    #[test]
    fn short_and_long_frames_are_framed_by_hand() {
        assert_eq!(frame(b"hi", false, false), vec![0x00, 2, b'h', b'i']);
        assert_eq!(frame(b"", true, false), vec![0x01, 0]);
        let long = vec![7u8; 300];
        let bytes = frame(&long, true, false);
        assert_eq!(&bytes[..9], &[0x03, 0, 0, 0, 0, 0, 0, 0x01, 0x2C]);
        assert_eq!(bytes.len(), 309);
        let mut reader = &bytes[..];
        assert_eq!(read_frame(&mut reader).unwrap(), (0x03, long));
        // Exactly 255 bytes still fits a short frame.
        assert_eq!(frame(&[0; 255], false, false)[..2], [0x00, 255]);
    }

    #[test]
    fn ready_is_framed_by_hand() {
        let body = command_body("READY", &ready_properties(SocketType::Dealer, Some(b"ab")));
        let mut expected = vec![5];
        expected.extend_from_slice(b"READY");
        expected.push(11);
        expected.extend_from_slice(b"Socket-Type");
        expected.extend_from_slice(&[0, 0, 0, 6]);
        expected.extend_from_slice(b"DEALER");
        expected.push(8);
        expected.extend_from_slice(b"Identity");
        expected.extend_from_slice(&[0, 0, 0, 2]);
        expected.extend_from_slice(b"ab");
        assert_eq!(body, expected);
        assert_eq!(frame(&body, false, true)[..2], [0x04, expected.len() as u8]);
        // A SUB has no identity to give.
        assert_eq!(parse_properties(&ready_properties(SocketType::Sub, Some(b"ab"))).unwrap(), vec![("Socket-Type".to_string(), b"SUB".to_vec())]);
        let (name, data) = split_command(&body).unwrap();
        assert_eq!(name, "READY");
        assert_eq!(parse_properties(&data).unwrap()[1], ("Identity".to_string(), b"ab".to_vec()));
    }

    #[test]
    fn broken_frames_are_refused() {
        assert!(read_frame(&mut &[0x08u8, 0][..]).is_err());
        assert!(read_frame(&mut &[0x00u8, 5, b'a'][..]).is_err());
        assert!(read_frame(&mut &[0x02u8, 0xFF, 0, 0, 0, 0, 0, 0, 0][..]).is_err());
        assert!(parse_properties(&[11, b'S']).is_err());
    }

    /// A peer on its own thread: it greets as `kind` at version 3.`minor` and then runs `then`.
    fn peer(kind: SocketType, minor: u8, then: impl FnOnce(Sender, Receiver, Peer) + Send + 'static) -> SocketAddr {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        std::thread::spawn(move || {
            let (stream, _) = listener.accept().unwrap();
            // A handshake meant to fail ends the thread quietly.
            if let Ok((sender, receiver, them)) = handshake(stream, kind, None, true, minor, WAIT) {
                then(sender, receiver, them);
            }
        });
        address
    }

    #[test]
    fn a_dealer_and_a_router_swap_messages() {
        let address = peer(SocketType::Router, 1, |sender, mut receiver, them| {
            assert_eq!(them.kind, SocketType::Dealer);
            assert_eq!(them.identity.as_deref(), Some(b"me".as_slice()));
            let frames = receiver.recv().unwrap();
            // Echoed back with a long frame added.
            let mut answer = frames.clone();
            answer.push(vec![b'x'; 1000]);
            sender.send(&answer).unwrap();
        });
        let (sender, mut receiver, them) = connect(address, SocketType::Dealer, Some(b"me"), WAIT).unwrap();
        assert_eq!((them.kind, them.minor), (SocketType::Router, 1));
        sender.send(&[b"<IDS|MSG>".as_slice(), b"", b"{}"]).unwrap();
        let frames = receiver.recv().unwrap();
        assert_eq!(frames.len(), 4);
        assert_eq!(frames[0], b"<IDS|MSG>");
        assert_eq!(frames[1], b"");
        assert_eq!(frames[3].len(), 1000);
    }

    #[test]
    fn a_sub_subscribes_as_its_peer_understands() {
        for minor in [0u8, 1] {
            let (told, heard) = std::sync::mpsc::channel();
            let address = peer(SocketType::Pub, minor, move |sender, mut receiver, _| {
                told.send(receiver.recv_any().unwrap()).unwrap();
                sender.send(&[b"topic".as_slice(), b"news"]).unwrap();
            });
            let (sender, mut receiver, them) = connect(address, SocketType::Sub, None, WAIT).unwrap();
            assert_eq!(them.minor, minor);
            sender.subscribe(b"").unwrap();
            let expected = if minor == 0 { Incoming::Message(vec![vec![1]]) } else { Incoming::Command("SUBSCRIBE".into(), vec![]) };
            assert_eq!(heard.recv_timeout(WAIT).unwrap(), expected);
            assert_eq!(receiver.recv().unwrap(), vec![b"topic".to_vec(), b"news".to_vec()]);
        }
    }

    #[test]
    fn a_req_hides_its_empty_frame_and_ping_gets_pong() {
        let address = peer(SocketType::Router, 1, |sender, mut receiver, _| {
            let frames = receiver.recv().unwrap();
            // A ROUTER sees REQ's empty frame first.
            assert_eq!(frames, vec![b"".to_vec(), b"ping".to_vec()]);
            sender.send_command("PING", b"\0\x0Asaw").unwrap();
            assert_eq!(receiver.recv_any().unwrap(), Incoming::Command("PONG".into(), b"saw".to_vec()));
            sender.send(&frames).unwrap();
        });
        let (sender, mut receiver, _) = connect(address, SocketType::Req, None, WAIT).unwrap();
        sender.send(&[b"ping"]).unwrap();
        // The PING is answered inside and passed over by recv.
        assert_eq!(receiver.recv().unwrap(), vec![b"ping".to_vec()]);
    }

    #[test]
    fn mismatched_sockets_and_errors_are_refused() {
        let address = peer(SocketType::Pub, 1, |_, _, _| {});
        assert!(connect(address, SocketType::Dealer, None, WAIT).is_err());
        // A peer that answers READY with ERROR.
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            stream.write_all(&greeting(1, true)).unwrap();
            let mut body = vec![6u8];
            body.extend_from_slice(b"denied");
            stream.write_all(&frame(&command_body("ERROR", &body), false, true)).unwrap();
            std::thread::sleep(Duration::from_millis(200));
        });
        let error = connect(address, SocketType::Dealer, None, WAIT).err().unwrap();
        assert_eq!(error.to_string(), "denied");
    }
}
