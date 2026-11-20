//! The serial terminal's other end: a serial port (an Arduino, a USB adapter on a retro machine's RS-232, a
//! Raspberry Pi's UART), or a network socket (the Ultimate 64's and Ultimate II+'s modem and telnet menu, a
//! WiFi modem, a telnet BBS). What comes in is kept with the time it came, and read three ways: as lines of
//! text (ASCII, PETSCII or ATASCII), as a VT100 screen, and as hex.
//!
//! A worker thread owns the connection: it reads all the time, writes what it's given, and runs XMODEM and
//! YMODEM transfers on the line itself, so nothing the window does can lose a byte.

use crate::xmodem::{self, Incoming, Outgoing, Progress, Protocol};
use chrono::{DateTime, Local};
use std::collections::VecDeque;
use std::io::{self, Read, Write};
use std::net::{TcpStream, ToSocketAddrs};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};

/// The speeds offered; any other can be typed.
pub const SPEEDS: [u32; 14] = [300, 1200, 2400, 4800, 9600, 19_200, 38_400, 57_600, 115_200, 230_400, 460_800, 921_600, 1_000_000, 2_000_000];

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Kind {
    Serial,
    Network,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Parity {
    None,
    Odd,
    Even,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Flow {
    None,
    /// XON and XOFF characters in the data.
    Software,
    /// The RTS and CTS wires.
    Hardware,
}

impl Flow {
    pub const ALL: [Flow; 3] = [Flow::None, Flow::Software, Flow::Hardware];

    pub fn label(self) -> &'static str {
        match self {
            Flow::None => "None",
            Flow::Software => "XON/XOFF (software)",
            Flow::Hardware => "RTS/CTS (hardware)",
        }
    }
}

/// What ends a line typed and sent.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Ending {
    Nothing,
    Cr,
    Lf,
    CrLf,
}

impl Ending {
    pub const ALL: [Ending; 4] = [Ending::Nothing, Ending::Cr, Ending::Lf, Ending::CrLf];

    pub fn label(self) -> &'static str {
        match self {
            Ending::Nothing => "Nothing",
            Ending::Cr => "CR",
            Ending::Lf => "LF",
            Ending::CrLf => "CR LF",
        }
    }

    /// The bytes, in a character set: PETSCII's Return is CR, and ATASCII has its own end of line.
    fn bytes(self, chars: Chars) -> &'static [u8] {
        match (self, chars) {
            (Ending::Nothing, _) => b"",
            (_, Chars::Atascii) => &[0x9B],
            (_, Chars::Petscii | Chars::PetsciiLower) => b"\r",
            (Ending::Cr, _) => b"\r",
            (Ending::Lf, _) => b"\n",
            (Ending::CrLf, _) => b"\r\n",
        }
    }
}

/// How the bytes are read as characters.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Chars {
    /// ASCII, and UTF-8 where it's that.
    Ascii,
    /// Commodore PETSCII, starting in upper case and graphics.
    Petscii,
    /// PETSCII starting in lower and upper case, as most C64 terminal programs use.
    PetsciiLower,
    /// Atari 8-bit ATASCII.
    Atascii,
}

impl Chars {
    pub const ALL: [Chars; 4] = [Chars::Ascii, Chars::Petscii, Chars::PetsciiLower, Chars::Atascii];

    pub fn label(self) -> &'static str {
        match self {
            Chars::Ascii => "ASCII and UTF-8",
            Chars::Petscii => "PETSCII, upper case and graphics",
            Chars::PetsciiLower => "PETSCII, lower and upper case",
            Chars::Atascii => "ATASCII (Atari 8-bit)",
        }
    }

    fn id(self) -> &'static str {
        match self {
            Chars::Ascii => "ascii",
            Chars::Petscii => "petscii",
            Chars::PetsciiLower => "petscii-lower",
            Chars::Atascii => "atascii",
        }
    }

    fn charset(self) -> Option<&'static crate::charset::Charset> {
        match self {
            Chars::Ascii => None,
            Chars::Petscii | Chars::PetsciiLower => crate::charset::by_id("petscii-c64"),
            Chars::Atascii => crate::charset::by_id("atascii"),
        }
    }

    fn first_table(self) -> usize {
        usize::from(self == Chars::PetsciiLower)
    }
}

/// How what came in is shown.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum View {
    /// Lines of text, as a serial monitor shows them.
    Lines,
    /// A VT100 screen, for full-screen menus and BBSes that move the cursor about.
    Screen,
    /// Every byte both ways in hex, with the time it went.
    Hex,
}

/// The choices, kept between runs.
#[derive(Clone, PartialEq, Debug)]
pub struct Prefs {
    pub kind: Kind,
    pub port: String,
    pub speed: u32,
    pub data_bits: u8,
    pub parity: Parity,
    pub stop_bits: u8,
    pub flow: Flow,
    pub host: String,
    pub tcp_port: u16,
    /// Telnet's commands answered and kept out of the data.
    pub telnet: bool,
    pub ending: Ending,
    pub chars: Chars,
    pub view: View,
    /// The time beside each line of text.
    pub times: bool,
    /// What's typed shown among what comes in, for a far end that doesn't echo it.
    pub echo: bool,
    pub protocol: Protocol,
}

impl Default for Prefs {
    fn default() -> Self {
        Prefs {
            kind: Kind::Serial,
            port: String::new(),
            speed: 9600,
            data_bits: 8,
            parity: Parity::None,
            stop_bits: 1,
            flow: Flow::None,
            host: String::new(),
            tcp_port: 23,
            telnet: false,
            ending: Ending::CrLf,
            chars: Chars::Ascii,
            view: View::Lines,
            times: false,
            echo: false,
            protocol: Protocol::Ymodem,
        }
    }
}

impl Prefs {
    pub fn to_settings(&self) -> String {
        let parity = match self.parity {
            Parity::None => "none",
            Parity::Odd => "odd",
            Parity::Even => "even",
        };
        let flow = match self.flow {
            Flow::None => "none",
            Flow::Software => "software",
            Flow::Hardware => "hardware",
        };
        let ending = match self.ending {
            Ending::Nothing => "none",
            Ending::Cr => "cr",
            Ending::Lf => "lf",
            Ending::CrLf => "crlf",
        };
        let view = match self.view {
            View::Lines => "lines",
            View::Screen => "screen",
            View::Hex => "hex",
        };
        format!(
            "serial_kind={}\nserial_port={}\nserial_speed={}\nserial_format={}{}{}\nserial_flow={flow}\nserial_host={}\nserial_tcp_port={}\nserial_telnet={}\nserial_ending={ending}\nserial_chars={}\nserial_view={view}\nserial_times={}\nserial_echo={}\nserial_protocol={}\n",
            if self.kind == Kind::Serial { "serial" } else { "network" },
            self.port,
            self.speed,
            self.data_bits,
            &parity[..1],
            self.stop_bits,
            self.host,
            self.tcp_port,
            self.telnet,
            self.chars.id(),
            self.times,
            self.echo,
            self.protocol.id(),
        )
    }

    pub fn read_setting(&mut self, name: &str, value: &str) {
        let value = value.trim();
        match name {
            "serial_kind" => self.kind = if value == "network" { Kind::Network } else { Kind::Serial },
            "serial_port" => self.port = value.to_string(),
            "serial_speed" => self.speed = value.parse().unwrap_or(self.speed),
            "serial_format" => {
                // Written as a terminal program writes it: 8N1, 7E1 and so on.
                let bytes = value.as_bytes();
                if let [bits @ b'5'..=b'8', parity, stop @ b'1'..=b'2'] = bytes {
                    self.data_bits = bits - b'0';
                    self.stop_bits = stop - b'0';
                    self.parity = match parity {
                        b'o' | b'O' => Parity::Odd,
                        b'e' | b'E' => Parity::Even,
                        _ => Parity::None,
                    };
                }
            }
            "serial_flow" => {
                self.flow = match value {
                    "software" => Flow::Software,
                    "hardware" => Flow::Hardware,
                    _ => Flow::None,
                }
            }
            "serial_host" => self.host = value.to_string(),
            "serial_tcp_port" => self.tcp_port = value.parse().unwrap_or(self.tcp_port),
            "serial_telnet" => self.telnet = value == "true",
            "serial_ending" => {
                self.ending = match value {
                    "none" => Ending::Nothing,
                    "cr" => Ending::Cr,
                    "lf" => Ending::Lf,
                    _ => Ending::CrLf,
                }
            }
            "serial_chars" => self.chars = Chars::ALL.into_iter().find(|c| c.id() == value).unwrap_or(Chars::Ascii),
            "serial_view" => {
                self.view = match value {
                    "screen" => View::Screen,
                    "hex" => View::Hex,
                    _ => View::Lines,
                }
            }
            "serial_times" => self.times = value == "true",
            "serial_echo" => self.echo = value == "true",
            "serial_protocol" => self.protocol = Protocol::from_id(value).unwrap_or(self.protocol),
            _ => {}
        }
    }

    /// The format in a terminal program's shorthand: 8N1, 7E2.
    pub fn format(&self) -> String {
        let parity = match self.parity {
            Parity::None => 'N',
            Parity::Odd => 'O',
            Parity::Even => 'E',
        };
        format!("{}{parity}{}", self.data_bits, self.stop_bits)
    }

    /// What's connected to, for the window's title line.
    pub fn describe(&self) -> String {
        match self.kind {
            Kind::Serial => format!("{} {} {}", self.port, self.speed, self.format()),
            Kind::Network => format!("{}:{}", self.host, self.tcp_port),
        }
    }
}

/// The serial ports this computer has, by name. Linux lists them from /sys, which a sandbox may hide.
pub fn ports() -> Vec<String> {
    if cfg!(target_os = "linux") && !Path::new("/sys/class/tty").is_dir() {
        return Vec::new();
    }
    let mut names: Vec<String> = serialport::available_ports().map(|ports| ports.into_iter().map(|p| p.port_name).collect()).unwrap_or_default();
    // A Mac lists each port twice, as tty. and cu.: cu. is the one to open from this side.
    if cfg!(target_os = "macos") {
        names.retain(|n| !n.starts_with("/dev/tty."));
    }
    names.sort();
    names.dedup();
    names
}

/// Telnet's commands: a far end that speaks telnet (the Ultimate's menu, a BBS) asks about options with them, and
/// they mustn't show among the data. Byte 255 in the data is sent doubled.
#[derive(Default)]
pub struct Telnet {
    state: TelnetState,
    /// The options already answered, so that an answer is never repeated into a loop.
    answered: Vec<(u8, u8)>,
    after_cr: bool,
}

#[derive(Default, Clone, Copy, PartialEq)]
enum TelnetState {
    #[default]
    Data,
    Command,
    Option(u8),
    Sub,
    SubCommand,
}

const IAC: u8 = 255;
const WILL: u8 = 251;
const WONT: u8 = 252;
const DO: u8 = 253;
const DONT: u8 = 254;
const SB: u8 = 250;
const SE: u8 = 240;

impl Telnet {
    /// Takes the commands out of what came in, putting the data in `data` and any answers in `reply`. The far
    /// end may echo and suppress go-ahead, and both sides may send 8-bit binary (which XMODEM needs); everything
    /// else is declined.
    pub fn filter(&mut self, input: &[u8], data: &mut Vec<u8>, reply: &mut Vec<u8>) {
        for &byte in input {
            self.state = match self.state {
                TelnetState::Data if byte == IAC => TelnetState::Command,
                TelnetState::Data => {
                    // A carriage return on its own is sent as CR NUL.
                    if !(self.after_cr && byte == 0) {
                        data.push(byte);
                    }
                    self.after_cr = byte == b'\r';
                    TelnetState::Data
                }
                TelnetState::Command => match byte {
                    IAC => {
                        data.push(IAC);
                        self.after_cr = false;
                        TelnetState::Data
                    }
                    WILL | WONT | DO | DONT => TelnetState::Option(byte),
                    SB => TelnetState::Sub,
                    _ => TelnetState::Data,
                },
                TelnetState::Option(verb) => {
                    let answer = match verb {
                        // ECHO 1, SUPPRESS-GO-AHEAD 3, BINARY 0.
                        WILL => Some(if matches!(byte, 0 | 1 | 3) { DO } else { DONT }),
                        DO => Some(if matches!(byte, 0 | 3) { WILL } else { WONT }),
                        _ => None,
                    };
                    if let Some(answer) = answer
                        && !self.answered.contains(&(verb, byte))
                    {
                        self.answered.push((verb, byte));
                        reply.extend_from_slice(&[IAC, answer, byte]);
                    }
                    TelnetState::Data
                }
                TelnetState::Sub if byte == IAC => TelnetState::SubCommand,
                TelnetState::Sub => TelnetState::Sub,
                TelnetState::SubCommand if byte == SE => TelnetState::Data,
                TelnetState::SubCommand => TelnetState::Sub,
            };
        }
    }

    /// Data made safe to send: IAC doubled.
    pub fn escape(bytes: &[u8]) -> Vec<u8> {
        let mut out = Vec::with_capacity(bytes.len());
        for &b in bytes {
            out.push(b);
            if b == IAC {
                out.push(IAC);
            }
        }
        out
    }
}

/// A network connection, with telnet's commands handled inside it when asked for.
pub struct Socket {
    stream: TcpStream,
    telnet: Option<Telnet>,
    waiting: VecDeque<u8>,
}

impl Read for Socket {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        if self.waiting.is_empty() {
            let mut raw = [0u8; 4096];
            let n = self.stream.read(&mut raw)?;
            if n == 0 {
                return Err(io::Error::new(io::ErrorKind::ConnectionAborted, crate::i18n::tr("The other end closed the connection.")));
            }
            match &mut self.telnet {
                Some(telnet) => {
                    let (mut data, mut reply) = (Vec::new(), Vec::new());
                    telnet.filter(&raw[..n], &mut data, &mut reply);
                    if !reply.is_empty() {
                        self.stream.write_all(&reply)?;
                    }
                    self.waiting.extend(data);
                }
                None => self.waiting.extend(&raw[..n]),
            }
            if self.waiting.is_empty() {
                // Only commands came: nothing to read yet.
                return Err(io::ErrorKind::WouldBlock.into());
            }
        }
        let n = buf.len().min(self.waiting.len());
        for (slot, byte) in buf.iter_mut().zip(self.waiting.drain(..n)) {
            *slot = byte;
        }
        Ok(n)
    }
}

impl Write for Socket {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        match self.telnet {
            Some(_) => self.stream.write_all(&Telnet::escape(buf)).map(|_| buf.len()),
            None => self.stream.write(buf),
        }
    }

    fn flush(&mut self) -> io::Result<()> {
        self.stream.flush()
    }
}

/// An open connection of either kind.
pub enum Connection {
    Serial(Box<dyn serialport::SerialPort>),
    Network(Socket),
}

impl Read for Connection {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        match self {
            Connection::Serial(port) => port.read(buf),
            Connection::Network(socket) => socket.read(buf),
        }
    }
}

impl Write for Connection {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        match self {
            Connection::Serial(port) => port.write(buf),
            Connection::Network(socket) => socket.write(buf),
        }
    }

    fn flush(&mut self) -> io::Result<()> {
        match self {
            Connection::Serial(port) => port.flush(),
            Connection::Network(socket) => socket.flush(),
        }
    }
}

/// How long one read waits before the worker looks for something to send.
const READ_WAIT: Duration = Duration::from_millis(15);

/// Opens the port or the socket.
pub fn open(prefs: &Prefs) -> Result<Connection, String> {
    match prefs.kind {
        Kind::Serial => {
            let port = prefs.port.trim();
            if port.is_empty() {
                return Err(crate::i18n::tr("Choose a serial port first.").into());
            }
            let data_bits = match prefs.data_bits {
                5 => serialport::DataBits::Five,
                6 => serialport::DataBits::Six,
                7 => serialport::DataBits::Seven,
                _ => serialport::DataBits::Eight,
            };
            let parity = match prefs.parity {
                Parity::None => serialport::Parity::None,
                Parity::Odd => serialport::Parity::Odd,
                Parity::Even => serialport::Parity::Even,
            };
            let flow = match prefs.flow {
                Flow::None => serialport::FlowControl::None,
                Flow::Software => serialport::FlowControl::Software,
                Flow::Hardware => serialport::FlowControl::Hardware,
            };
            let stop = if prefs.stop_bits == 2 { serialport::StopBits::Two } else { serialport::StopBits::One };
            let opened = serialport::new(port, prefs.speed).data_bits(data_bits).parity(parity).stop_bits(stop).flow_control(flow).timeout(READ_WAIT).open();
            opened.map(Connection::Serial).map_err(|e| crate::i18n::tf!("The serial port {port} couldn't be opened: {e}"))
        }
        Kind::Network => {
            let host = prefs.host.trim();
            if host.is_empty() {
                return Err(crate::i18n::tr("Say which machine: its name or address.").into());
            }
            let addresses = (host, prefs.tcp_port).to_socket_addrs().map_err(|e| crate::i18n::tf!("{host} couldn't be found: {e}"))?;
            let mut last = None;
            for address in addresses {
                match TcpStream::connect_timeout(&address, Duration::from_secs(5)) {
                    Ok(stream) => {
                        stream.set_read_timeout(Some(READ_WAIT)).map_err(|e| e.to_string())?;
                        let _ = stream.set_nodelay(true);
                        return Ok(Connection::Network(Socket { stream, telnet: prefs.telnet.then(Telnet::default), waiting: VecDeque::new() }));
                    }
                    Err(e) => last = Some(e),
                }
            }
            let port = prefs.tcp_port;
            Err(last.map_or_else(|| crate::i18n::tf!("{host} couldn't be found."), |e| crate::i18n::tf!("Couldn't connect to {host} on port {port}: {e}")))
        }
    }
}

/// A run of bytes that went one way at about one time.
#[derive(Clone, Debug)]
pub struct Burst {
    /// Counts up from the first, so a view knows where it got to after older bursts are let go.
    pub id: u64,
    pub at: DateTime<Local>,
    pub incoming: bool,
    pub bytes: Vec<u8>,
    began: Instant,
}

/// Everything that went both ways, up to a limit, oldest first.
#[derive(Default)]
pub struct Traffic {
    pub bursts: VecDeque<Burst>,
    size: usize,
    next_id: u64,
}

/// The most traffic kept for the views; the log file keeps it all.
const MOST_KEPT: usize = 8 << 20;
/// Bytes arriving within this long of the last go into the same burst, under the same time.
const BURST_GAP: Duration = Duration::from_millis(50);

impl Traffic {
    pub fn add(&mut self, incoming: bool, bytes: &[u8], now: Instant, at: DateTime<Local>) {
        if bytes.is_empty() {
            return;
        }
        match self.bursts.back_mut() {
            Some(last) if last.incoming == incoming && now.duration_since(last.began) < BURST_GAP && last.bytes.len() < 4096 => last.bytes.extend_from_slice(bytes),
            _ => {
                self.bursts.push_back(Burst { id: self.next_id, at, incoming, bytes: bytes.to_vec(), began: now });
                self.next_id += 1;
            }
        }
        self.size += bytes.len();
        while self.size > MOST_KEPT && self.bursts.len() > 1 {
            if let Some(old) = self.bursts.pop_front() {
                self.size -= old.bytes.len();
            }
        }
    }

    /// What's new since a place `(burst id, bytes of it already seen)`, and the place after it.
    pub fn since(&self, place: (u64, usize)) -> (Vec<(&Burst, &[u8])>, (u64, usize)) {
        let mut out = Vec::new();
        let mut end = place;
        for burst in &self.bursts {
            if burst.id < place.0 {
                continue;
            }
            let from = if burst.id == place.0 { place.1.min(burst.bytes.len()) } else { 0 };
            if from < burst.bytes.len() {
                out.push((burst, &burst.bytes[from..]));
            }
            end = (burst.id, burst.bytes.len());
        }
        (out, end)
    }
}

/// A transfer under way, or just over.
#[derive(Clone, Debug)]
pub struct Transfer {
    pub protocol: Protocol,
    pub sending: bool,
    pub progress: Progress,
    /// The files received (none when sending), or what went wrong.
    pub outcome: Option<Result<Vec<Incoming>, String>>,
}

/// What a log file keeps.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum LogKind {
    /// The bytes that came in, exactly: a capture to open later.
    Raw,
    /// Both ways, as hex with times, as the hex view shows them.
    Hex,
}

struct Log {
    file: std::fs::File,
    kind: LogKind,
    path: PathBuf,
}

/// What the worker and the window share.
#[derive(Default)]
pub struct Shared {
    pub traffic: Mutex<Traffic>,
    pub transfer: Mutex<Option<Transfer>>,
    /// Set to stop a transfer.
    pub stop: AtomicBool,
    /// Why the connection ended, once it has.
    pub ended: Mutex<Option<String>>,
    /// Set when something new has come, so the window draws again.
    pub changed: AtomicBool,
    log: Mutex<Option<Log>>,
}

pub fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

enum Order {
    Bytes(Vec<u8>),
    Send(Protocol, Vec<Outgoing>),
    Receive(Protocol, bool),
    /// DTR dropped for a moment: an Arduino resets itself on this.
    PulseDtr,
    /// A break: the line held low for a quarter of a second.
    Break,
    Close,
}

/// An open connection, with its worker.
pub struct Session {
    pub shared: Arc<Shared>,
    orders: Sender<Order>,
    pub described: String,
    pub serial: bool,
}

impl Session {
    pub fn start(connection: Connection, described: String) -> Session {
        let shared = Arc::new(Shared::default());
        let (orders, inbox) = mpsc::channel();
        let serial = matches!(connection, Connection::Serial(_));
        let worker = shared.clone();
        std::thread::spawn(move || work(connection, &worker, &inbox));
        Session { shared, orders, described, serial }
    }

    pub fn send(&self, bytes: Vec<u8>) {
        if !bytes.is_empty() {
            let _ = self.orders.send(Order::Bytes(bytes));
        }
    }

    pub fn send_files(&self, protocol: Protocol, files: Vec<Outgoing>) {
        self.begin_transfer(protocol, true);
        let _ = self.orders.send(Order::Send(protocol, files));
    }

    pub fn receive_files(&self, protocol: Protocol, trim: bool) {
        self.begin_transfer(protocol, false);
        let _ = self.orders.send(Order::Receive(protocol, trim));
    }

    fn begin_transfer(&self, protocol: Protocol, sending: bool) {
        self.shared.stop.store(false, Ordering::Relaxed);
        *lock(&self.shared.transfer) = Some(Transfer { protocol, sending, progress: Progress::default(), outcome: None });
    }

    pub fn transferring(&self) -> bool {
        lock(&self.shared.transfer).as_ref().is_some_and(|t| t.outcome.is_none())
    }

    pub fn stop_transfer(&self) {
        self.shared.stop.store(true, Ordering::Relaxed);
    }

    pub fn pulse_dtr(&self) {
        let _ = self.orders.send(Order::PulseDtr);
    }

    pub fn send_break(&self) {
        let _ = self.orders.send(Order::Break);
    }

    pub fn ended(&self) -> Option<String> {
        lock(&self.shared.ended).clone()
    }

    /// Starts writing to a log file, or stops (with `None`).
    pub fn log_to(&self, path: Option<(&Path, LogKind)>) -> Result<(), String> {
        let log = match path {
            Some((path, kind)) => {
                let file = std::fs::OpenOptions::new().create(true).append(true).open(path).map_err(|e| crate::i18n::tf!("Couldn't write to {}: {e}", path.display()))?;
                Some(Log { file, kind, path: path.to_path_buf() })
            }
            None => None,
        };
        *lock(&self.shared.log) = log;
        Ok(())
    }

    pub fn log_path(&self) -> Option<PathBuf> {
        lock(&self.shared.log).as_ref().map(|l| l.path.clone())
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        self.shared.stop.store(true, Ordering::Relaxed);
        let _ = self.orders.send(Order::Close);
    }
}

/// Puts bytes into the traffic and the log.
fn record(shared: &Shared, incoming: bool, bytes: &[u8]) {
    if bytes.is_empty() {
        return;
    }
    let at = Local::now();
    lock(&shared.traffic).add(incoming, bytes, Instant::now(), at);
    if let Some(log) = lock(&shared.log).as_mut() {
        let _ = match log.kind {
            LogKind::Raw if incoming => log.file.write_all(bytes),
            LogKind::Raw => Ok(()),
            LogKind::Hex => log.file.write_all(hex_rows(at, incoming, bytes).join("\n").as_bytes()).and_then(|_| log.file.write_all(b"\n")),
        };
    }
    shared.changed.store(true, Ordering::Relaxed);
}

/// The worker: reads, writes and runs transfers until told to close or the line fails.
fn work(mut connection: Connection, shared: &Shared, inbox: &Receiver<Order>) {
    let mut buffer = [0u8; 4096];
    loop {
        loop {
            let order = match inbox.try_recv() {
                Ok(order) => order,
                Err(mpsc::TryRecvError::Empty) => break,
                Err(mpsc::TryRecvError::Disconnected) => return,
            };
            match order {
                Order::Close => return,
                Order::Bytes(bytes) => {
                    if let Err(e) = connection.write_all(&bytes).and_then(|_| connection.flush()) {
                        return end(shared, e.to_string());
                    }
                    record(shared, false, &bytes);
                }
                Order::Send(protocol, files) => run_transfer(&mut connection, shared, |line, report| xmodem::send(line, protocol, &files, &xmodem::Timing::default(), &shared.stop, report).map(|_| Vec::new())),
                Order::Receive(protocol, trim) => run_transfer(&mut connection, shared, |line, report| xmodem::receive(line, protocol, trim, &xmodem::Timing::default(), &shared.stop, report)),
                Order::PulseDtr => {
                    if let Connection::Serial(port) = &mut connection {
                        let _ = port.write_data_terminal_ready(false);
                        std::thread::sleep(Duration::from_millis(100));
                        let _ = port.write_data_terminal_ready(true);
                    }
                }
                Order::Break => {
                    if let Connection::Serial(port) = &mut connection {
                        let _ = port.set_break();
                        std::thread::sleep(Duration::from_millis(250));
                        let _ = port.clear_break();
                    }
                }
            }
        }
        match connection.read(&mut buffer) {
            Ok(n) => record(shared, true, &buffer[..n]),
            Err(e) if matches!(e.kind(), io::ErrorKind::TimedOut | io::ErrorKind::WouldBlock | io::ErrorKind::Interrupted) => {}
            Err(e) => return end(shared, e.to_string()),
        }
    }
}

fn end(shared: &Shared, why: String) {
    *lock(&shared.ended) = Some(why);
    if let Some(transfer) = lock(&shared.transfer).as_mut().filter(|t| t.outcome.is_none()) {
        transfer.outcome = Some(Err(crate::i18n::tr("The connection closed.").into()));
    }
    shared.changed.store(true, Ordering::Relaxed);
}

/// Runs a transfer on the line, telling the window how it goes; whatever came in after it ends goes to the
/// terminal as usual.
fn run_transfer(connection: &mut Connection, shared: &Shared, run: impl FnOnce(&mut dyn xmodem::Line, &mut dyn FnMut(&Progress)) -> Result<Vec<Incoming>, xmodem::Failure>) {
    let mut line = xmodem::Stream::new(&mut *connection);
    let mut report = |progress: &Progress| {
        if let Some(transfer) = lock(&shared.transfer).as_mut() {
            transfer.progress = progress.clone();
        }
        shared.changed.store(true, Ordering::Relaxed);
    };
    let result = run(&mut line, &mut report);
    let leftover = line.leftover().to_vec();
    if let Some(transfer) = lock(&shared.transfer).as_mut() {
        transfer.outcome = Some(result.map_err(|f| f.describe()));
    }
    record(shared, true, &leftover);
    shared.changed.store(true, Ordering::Relaxed);
}

/// Bytes as rows of the hex view: the time, the way they went, sixteen bytes in hex and as ASCII.
pub fn hex_rows(at: DateTime<Local>, incoming: bool, bytes: &[u8]) -> Vec<String> {
    let time = at.format("%H:%M:%S%.3f").to_string();
    bytes.chunks(16).map(|row| hex_row(&time, incoming, row)).collect()
}

pub fn hex_row(time: &str, incoming: bool, row: &[u8]) -> String {
    let hex: Vec<String> = row.iter().map(|b| format!("{b:02X}")).collect();
    let text: String = row.iter().map(|&b| if (0x20..0x7F).contains(&b) { b as char } else { '.' }).collect();
    format!("{time} {} {:<47}  {text}", if incoming { '\u{2190}' } else { '\u{2192}' }, hex.join(" "))
}

/// Text typed to send: its characters in the chosen set, then the line ending. Typed as hex ("1B 5B 41" or
/// "1b5b41"), it's those bytes, and no ending.
pub fn typed_bytes(text: &str, chars: Chars, ending: Ending, as_hex: bool) -> Result<Vec<u8>, String> {
    if as_hex {
        let digits: String = text.chars().filter(|c| !c.is_whitespace() && *c != ',').collect();
        let digits = digits.strip_prefix("0x").unwrap_or(&digits);
        if !digits.len().is_multiple_of(2) || !digits.chars().all(|c| c.is_ascii_hexdigit()) {
            return Err(crate::i18n::tr("That isn't hex: pairs of digits 0 to 9 and A to F, such as 1B 5B 41.").into());
        }
        return Ok((0..digits.len()).step_by(2).filter_map(|i| u8::from_str_radix(&digits[i..i + 2], 16).ok()).collect());
    }
    let mut out = match chars.charset() {
        None => text.as_bytes().to_vec(),
        Some(set) => {
            let table = set.table(chars.first_table());
            text.chars()
                .filter_map(|c| {
                    let table = table?;
                    // Upper and lower case are where the set has them; failing that, the other case, as a
                    // machine with capitals only would take it.
                    table.byte_of(c).or_else(|| table.byte_of(c.to_ascii_uppercase())).or_else(|| table.byte_of(c.to_ascii_lowercase()))
                })
                .collect()
        }
    };
    out.extend_from_slice(ending.bytes(chars));
    Ok(out)
}

/// Lines of text, as a serial monitor shows them. CR, LF and CR LF each end a line; ANSI escape sequences are
/// left out; other control characters show as their symbols (␇, ␛).
pub struct Lines {
    pub lines: VecDeque<(DateTime<Local>, String)>,
    chars: Chars,
    /// The line being added to is open (its end hasn't come).
    open: bool,
    after_cr: bool,
    escape: Escape,
    /// Bytes of a UTF-8 character still to come.
    pending: Vec<u8>,
    petscii: crate::charset::CellState,
}

#[derive(Clone, Copy, PartialEq)]
enum Escape {
    None,
    Started,
    Sequence,
}

/// The most lines kept for the text view.
const MOST_LINES: usize = 20_000;

impl Lines {
    pub fn new(chars: Chars) -> Lines {
        Lines {
            lines: VecDeque::new(),
            chars,
            open: false,
            after_cr: false,
            escape: Escape::None,
            pending: Vec::new(),
            petscii: crate::charset::CellState::new(crate::charset::Model::Petscii, chars.first_table(), false),
        }
    }

    fn push(&mut self, c: char, at: DateTime<Local>) {
        if !self.open {
            self.lines.push_back((at, String::new()));
            self.open = true;
            if self.lines.len() > MOST_LINES {
                self.lines.pop_front();
            }
        }
        if let Some((_, line)) = self.lines.back_mut() {
            if c == '\t' {
                let column = line.chars().count();
                line.push_str(&" ".repeat(8 - column % 8));
            } else {
                line.push(c);
            }
        }
    }

    fn end_line(&mut self, at: DateTime<Local>) {
        if !self.open {
            self.lines.push_back((at, String::new()));
        }
        self.open = false;
    }

    fn back_space(&mut self) {
        if self.open
            && let Some((_, line)) = self.lines.back_mut()
        {
            line.pop();
        }
    }

    pub fn feed(&mut self, bytes: &[u8], at: DateTime<Local>) {
        for &byte in bytes {
            let after_cr = std::mem::replace(&mut self.after_cr, false);
            match self.chars {
                Chars::Ascii => self.feed_ascii(byte, after_cr, at),
                Chars::Petscii | Chars::PetsciiLower => {
                    match byte {
                        0x0D | 0x8D => self.end_line(at),
                        0x0A if after_cr => {}
                        0x14 => self.back_space(),
                        _ => {
                            if let Some(set) = self.chars.charset()
                                && let Some(entry) = set.entry(self.petscii.table_for(set, byte), byte)
                                && entry.kind == crate::charset::Kind::Printable
                                && let Some(c) = entry.unicode
                            {
                                self.push(c, at);
                            }
                        }
                    }
                    self.after_cr = matches!(byte, 0x0D | 0x8D);
                    self.petscii.advance(byte);
                }
                Chars::Atascii => match byte {
                    0x9B => self.end_line(at),
                    0x7E => self.back_space(),
                    _ => {
                        if let Some(set) = self.chars.charset()
                            && let Some(entry) = set.entry(0, byte)
                            && entry.kind == crate::charset::Kind::Printable
                            && let Some(c) = entry.unicode
                        {
                            self.push(c, at);
                        }
                    }
                },
            }
        }
    }

    fn feed_ascii(&mut self, byte: u8, after_cr: bool, at: DateTime<Local>) {
        match self.escape {
            Escape::Started => {
                self.escape = if byte == b'[' { Escape::Sequence } else { Escape::None };
                return;
            }
            Escape::Sequence => {
                if (0x40..=0x7E).contains(&byte) {
                    self.escape = Escape::None;
                }
                return;
            }
            Escape::None => {}
        }
        if byte >= 0x80 || !self.pending.is_empty() {
            self.pending.push(byte);
            match std::str::from_utf8(&self.pending) {
                Ok(text) => {
                    let text = text.to_string();
                    self.pending.clear();
                    for c in text.chars() {
                        self.push(c, at);
                    }
                }
                Err(e) if e.error_len().is_some() || self.pending.len() >= 4 => {
                    // Not UTF-8: each byte shown as Latin-1 would show it.
                    let bytes = std::mem::take(&mut self.pending);
                    for b in bytes {
                        self.push(char::from(b), at);
                    }
                }
                Err(_) => {}
            }
            return;
        }
        match byte {
            b'\r' => {
                self.end_line(at);
                self.after_cr = true;
            }
            b'\n' if after_cr => {}
            b'\n' => self.end_line(at),
            0x1B => self.escape = Escape::Started,
            0x08 => self.back_space(),
            0x00 | 0x07 => {}
            b'\t' => self.push('\t', at),
            0x01..=0x1F => self.push(char::from_u32(0x2400 + u32::from(byte)).unwrap_or('?'), at),
            0x7F => self.push('\u{2421}', at),
            _ => self.push(char::from(byte), at),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn now() -> DateTime<Local> {
        Local::now()
    }

    fn text(lines: &Lines) -> Vec<String> {
        lines.lines.iter().map(|(_, l)| l.clone()).collect()
    }

    #[test]
    fn settings_come_back_as_they_went() {
        let prefs = Prefs { kind: Kind::Network, port: "/dev/ttyUSB0".into(), speed: 57_600, data_bits: 7, parity: Parity::Even, stop_bits: 2, flow: Flow::Hardware, host: "u64.local".into(), tcp_port: 6400, telnet: true, ending: Ending::Cr, chars: Chars::PetsciiLower, view: View::Hex, times: true, echo: true, protocol: Protocol::Xmodem1k };
        let mut read = Prefs::default();
        for line in prefs.to_settings().lines() {
            let (name, value) = line.split_once('=').unwrap();
            read.read_setting(name, value);
        }
        assert_eq!(read, prefs);
        assert_eq!(prefs.format(), "7E2");
        assert_eq!(Prefs::default().describe(), " 9600 8N1");
    }

    #[test]
    fn lines_end_at_cr_lf_or_both_and_escapes_are_left_out() {
        let mut lines = Lines::new(Chars::Ascii);
        lines.feed(b"one\r\ntwo\rthree\nfo", now());
        lines.feed(b"ur\x1b[1;32mgreen\x1b[0m\tx\x07\x08y\x01", now());
        assert_eq!(text(&lines), ["one", "two", "three", "fourgreen       y\u{2401}"]);
        // UTF-8 split across two reads, and a byte that isn't UTF-8.
        let mut lines = Lines::new(Chars::Ascii);
        lines.feed(&[b'c', 0xC3], now());
        lines.feed(&[0xA9, b' ', 0xFF, b'!'], now());
        assert_eq!(text(&lines), ["c\u{e9} \u{ff}!"]);
        // An empty line between two ends.
        let mut lines = Lines::new(Chars::Ascii);
        lines.feed(b"a\r\n\r\nb", now());
        assert_eq!(text(&lines), ["a", "", "b"]);
    }

    #[test]
    fn petscii_and_atascii_read_through_limoniter_s_tables() {
        // "HELLO" in PETSCII's upper case set; then a switch to lower case, where 0x41 is "a" and 0xC1 is "A".
        let mut lines = Lines::new(Chars::Petscii);
        lines.feed(b"HELLO\r\x0e\x41\xc1\x93\r", now());
        assert_eq!(text(&lines), ["HELLO", "aA"]);
        let mut lines = Lines::new(Chars::PetsciiLower);
        lines.feed(b"\x48\x45\x4c\x4c\x4f \xd7\x4f\x52\x4c\x44\r", now());
        assert_eq!(text(&lines), ["hello World"]);
        // ATASCII: 0x9B ends the line; CR and LF are ordinary characters (graphics) there.
        let mut lines = Lines::new(Chars::Atascii);
        lines.feed(b"READY\x9bOK\x9b", now());
        assert_eq!(text(&lines), ["READY", "OK"]);
    }

    #[test]
    fn typed_lines_are_encoded_with_their_ending() {
        assert_eq!(typed_bytes("AT", Chars::Ascii, Ending::CrLf, false).unwrap(), b"AT\r\n");
        assert_eq!(typed_bytes("ls", Chars::Ascii, Ending::Lf, false).unwrap(), b"ls\n");
        // PETSCII: Return is CR whatever's chosen; in upper case, lower case typed is sent as capitals.
        assert_eq!(typed_bytes("load", Chars::Petscii, Ending::CrLf, false).unwrap(), b"LOAD\r");
        assert_eq!(typed_bytes("Hi", Chars::PetsciiLower, Ending::Cr, false).unwrap(), [0xC8, 0x49, 0x0D]);
        assert_eq!(typed_bytes("OK", Chars::Atascii, Ending::Lf, false).unwrap(), [b'O', b'K', 0x9B]);
        assert_eq!(typed_bytes("1B 5b41, ff", Chars::Ascii, Ending::CrLf, true).unwrap(), [0x1B, 0x5B, 0x41, 0xFF]);
        assert!(typed_bytes("1B 5", Chars::Ascii, Ending::Cr, true).is_err());
        assert!(typed_bytes("zz", Chars::Ascii, Ending::Cr, true).is_err());
    }

    #[test]
    fn hex_rows_show_the_time_the_way_and_the_bytes() {
        let at = chrono::TimeZone::with_ymd_and_hms(&Local, 2026, 11, 20, 13, 4, 5).unwrap();
        let rows = hex_rows(at, true, b"Hello, world!\r\n\x00\xFFmore");
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0], "13:04:05.000 \u{2190} 48 65 6C 6C 6F 2C 20 77 6F 72 6C 64 21 0D 0A 00  Hello, world!...");
        assert!(rows[1].starts_with("13:04:05.000 \u{2190} FF 6D 6F 72 65"));
        assert!(hex_row("t", false, b"A").contains('\u{2192}'));
    }

    #[test]
    fn traffic_gathers_bursts_and_views_pick_up_where_they_left_off() {
        let mut traffic = Traffic::default();
        let start = Instant::now();
        traffic.add(true, b"abc", start, now());
        traffic.add(true, b"def", start + Duration::from_millis(10), now());
        let (new, place) = traffic.since((0, 0));
        assert_eq!(new.iter().map(|(_, b)| b.to_vec()).collect::<Vec<_>>(), [b"abcdef".to_vec()]);
        // More in the same burst, then a reply, then a later burst.
        traffic.add(true, b"g", start + Duration::from_millis(20), now());
        traffic.add(false, b"OK", start + Duration::from_millis(30), now());
        traffic.add(true, b"h", start + Duration::from_secs(1), now());
        let (new, place) = traffic.since(place);
        assert_eq!(new.iter().map(|(b, bytes)| (b.incoming, bytes.to_vec())).collect::<Vec<_>>(), [(true, b"g".to_vec()), (false, b"OK".to_vec()), (true, b"h".to_vec())]);
        assert!(traffic.since(place).0.is_empty());
        // Old traffic goes once there's too much, newest kept.
        let mut big = Traffic::default();
        for i in 0..20u32 {
            big.add(i % 2 == 0, &vec![0u8; 1 << 20], start + Duration::from_secs(u64::from(i)), now());
        }
        assert!(big.size <= MOST_KEPT);
        assert_eq!(big.bursts.back().unwrap().id, 19);
    }

    #[test]
    fn telnet_commands_are_answered_and_kept_out_of_the_data() {
        let mut telnet = Telnet::default();
        let (mut data, mut reply) = (Vec::new(), Vec::new());
        // WILL ECHO, DO TERMINAL-TYPE (24), a subnegotiation, a doubled 255, and CR NUL.
        telnet.filter(&[IAC, WILL, 1, b'H', IAC, DO, 24, b'i', IAC, SB, 24, 1, IAC, SE, IAC, IAC, b'\r', 0, b'x'], &mut data, &mut reply);
        assert_eq!(data, [b'H', b'i', 0xFF, b'\r', b'x']);
        assert_eq!(reply, [IAC, DO, 1, IAC, WONT, 24]);
        // Asked again, it doesn't answer again; split across reads, it still follows.
        let (mut data, mut reply) = (Vec::new(), Vec::new());
        telnet.filter(&[IAC], &mut data, &mut reply);
        telnet.filter(&[WILL, 1, IAC, DO], &mut data, &mut reply);
        telnet.filter(&[0, b'!'], &mut data, &mut reply);
        assert_eq!((data, reply), (vec![b'!'], vec![IAC, WILL, 0]));
        assert_eq!(Telnet::escape(&[1, 0xFF, 2]), [1, 0xFF, 0xFF, 2]);
    }

    #[test]
    fn a_session_over_a_socket_echoes_logs_and_transfers() {
        // A far end on this computer: answers each line with "OK", then receives a YMODEM batch.
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let far = std::thread::spawn(move || {
            let (stream, _) = listener.accept().unwrap();
            stream.set_read_timeout(Some(Duration::from_millis(20))).unwrap();
            let mut line = xmodem::Stream::new(stream);
            let mut got = Vec::new();
            while !got.ends_with(b"\r\n") {
                if let Some(b) = xmodem::Line::get(&mut line, Duration::from_secs(5)).unwrap() {
                    got.push(b);
                }
            }
            xmodem::Line::put(&mut line, b"OK\r\n").unwrap();
            let files = xmodem::receive(&mut line, Protocol::Ymodem, false, &xmodem::Timing::default(), &AtomicBool::new(false), &mut |_| {}).unwrap();
            xmodem::Line::put(&mut line, b"done\r\n").unwrap();
            (got, files)
        });
        let prefs = Prefs { kind: Kind::Network, host: "127.0.0.1".into(), tcp_port: port, ..Prefs::default() };
        let session = Session::start(open(&prefs).unwrap(), prefs.describe());
        let log = std::env::temp_dir().join(format!("limoniter-serial-log-{}.bin", std::process::id()));
        let _ = std::fs::remove_file(&log);
        session.log_to(Some((&log, LogKind::Raw))).unwrap();
        session.send(typed_bytes("AT", Chars::Ascii, Ending::CrLf, false).unwrap());
        let wait_for = |what: &dyn Fn() -> bool| {
            let began = Instant::now();
            while !what() {
                assert!(began.elapsed() < Duration::from_secs(20), "timed out");
                std::thread::sleep(Duration::from_millis(10));
            }
        };
        let came_in = |needle: &[u8]| lock(&session.shared.traffic).bursts.iter().filter(|b| b.incoming).flat_map(|b| b.bytes.clone()).collect::<Vec<_>>().windows(needle.len()).any(|w| w == needle);
        wait_for(&|| came_in(b"OK\r\n"));
        session.send_files(Protocol::Ymodem, vec![Outgoing { name: "a.prg".into(), data: vec![7; 3000], modified: None }]);
        wait_for(&|| !session.transferring());
        assert_eq!(lock(&session.shared.transfer).as_ref().unwrap().outcome, Some(Ok(Vec::new())));
        wait_for(&|| came_in(b"done\r\n"));
        let (got, files) = far.join().unwrap();
        assert_eq!(got, b"AT\r\n");
        assert_eq!((files[0].name.as_deref(), files[0].data.len()), (Some("a.prg"), 3000));
        // The log holds what came in, but not the transfer's own bytes.
        session.log_to(None).unwrap();
        let logged = std::fs::read(&log).unwrap();
        assert!(logged.starts_with(b"OK\r\n") && logged.ends_with(b"done\r\n"), "{logged:?}");
        let _ = std::fs::remove_file(&log);
        // The far end hanging up is noticed.
        wait_for(&|| session.ended().is_some());
    }

    /// A real serial line: socat's pair of pseudo-terminals, opened as serial ports, with a YMODEM batch across
    /// them. Skipped where socat isn't installed.
    #[cfg(unix)]
    #[test]
    fn ymodem_crosses_a_pseudo_terminal_pair() {
        if !crate::lsp::on_path("socat") {
            eprintln!("socat isn't installed: the pseudo-terminal test is skipped");
            return;
        }
        let dir = std::env::temp_dir().join(format!("limoniter-pty-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let (a, b) = (dir.join("a"), dir.join("b"));
        let mut socat = std::process::Command::new("socat")
            .arg(format!("pty,raw,echo=0,link={}", a.display()))
            .arg(format!("pty,raw,echo=0,link={}", b.display()))
            .stderr(std::process::Stdio::null())
            .spawn()
            .unwrap();
        let began = Instant::now();
        while !(a.exists() && b.exists()) && began.elapsed() < Duration::from_secs(5) {
            std::thread::sleep(Duration::from_millis(20));
        }
        let port = |path: &Path| {
            let prefs = Prefs { port: path.display().to_string(), speed: 115_200, ..Prefs::default() };
            open(&prefs).unwrap()
        };
        let (left, right) = (port(&a), port(&b));
        let files = vec![Outgoing { name: "kernal.rom".into(), data: (0..8192u32).map(|i| (i * 7 + i / 256) as u8).collect(), modified: Some(1_700_000_000) }, Outgoing { name: "basic.rom".into(), data: vec![0xEA; 1000], modified: None }];
        let out = files.clone();
        let sending = std::thread::spawn(move || xmodem::send(&mut xmodem::Stream::new(left), Protocol::Ymodem, &out, &xmodem::Timing::default(), &AtomicBool::new(false), &mut |_| {}));
        let got = xmodem::receive(&mut xmodem::Stream::new(right), Protocol::Ymodem, false, &xmodem::Timing::default(), &AtomicBool::new(false), &mut |_| {});
        let sent = sending.join().unwrap();
        let _ = socat.kill();
        let _ = socat.wait();
        let _ = std::fs::remove_dir_all(&dir);
        assert_eq!(sent, Ok(()));
        let got = got.unwrap();
        assert_eq!(got.len(), 2);
        assert_eq!(got[0].data, files[0].data);
        assert_eq!(got[1].data, files[1].data);
        assert_eq!(got[0].modified, Some(1_700_000_000));
    }
}
