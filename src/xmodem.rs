//! XMODEM and YMODEM: files sent and received over a serial line, as terminal programs on old machines (and
//! bootloaders on new ones) still do it.
//!
//! XMODEM (Ward Christensen, 1977) sends a file in numbered blocks of 128 bytes, each with a one-byte sum, and
//! waits for the receiver to say yes (ACK) or no (NAK) to each. XMODEM-CRC puts a 16-bit CRC in place of the sum,
//! and XMODEM-1K allows blocks of 1024 bytes. YMODEM (Chuck Forsberg, 1985) is XMODEM-1K with a block 0 before
//! each file that carries its name and size, so several files go in one batch and arrive at their right length.
//! The rules followed are those in Forsberg's "XMODEM/YMODEM Protocol Reference" (1988).
//!
//! The protocols run over a [`Line`]: anything bytes can be read from, with a time limit, and written to. A
//! serial port, a socket, and the tests' in-memory pipes are all lines.

use std::io::{self, Read, Write};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

pub const SOH: u8 = 0x01;
pub const STX: u8 = 0x02;
pub const EOT: u8 = 0x04;
pub const ACK: u8 = 0x06;
pub const NAK: u8 = 0x15;
pub const CAN: u8 = 0x18;
/// What the last block of an XMODEM file is padded with: CP/M's end of file.
pub const SUB: u8 = 0x1A;
/// The receiver's request for blocks checked with a CRC rather than a sum.
pub const WANT_CRC: u8 = b'C';

/// Which of the protocols, as the person chooses it.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Protocol {
    /// 128-byte blocks with a one-byte sum: the original, which everything understands.
    Xmodem,
    /// 128-byte blocks with a CRC.
    XmodemCrc,
    /// 1024-byte blocks with a CRC.
    Xmodem1k,
    /// Batches of files, each with its name and size, in 1024-byte blocks with a CRC.
    Ymodem,
}

impl Protocol {
    pub const ALL: [Protocol; 4] = [Protocol::Xmodem, Protocol::XmodemCrc, Protocol::Xmodem1k, Protocol::Ymodem];

    pub fn label(self) -> &'static str {
        match self {
            Protocol::Xmodem => "XMODEM (checksum, 128-byte blocks)",
            Protocol::XmodemCrc => "XMODEM-CRC (128-byte blocks)",
            Protocol::Xmodem1k => "XMODEM-1K (CRC, 1024-byte blocks)",
            Protocol::Ymodem => "YMODEM (batch, with names and sizes)",
        }
    }

    pub fn id(self) -> &'static str {
        match self {
            Protocol::Xmodem => "xmodem",
            Protocol::XmodemCrc => "xmodem-crc",
            Protocol::Xmodem1k => "xmodem-1k",
            Protocol::Ymodem => "ymodem",
        }
    }

    pub fn from_id(id: &str) -> Option<Protocol> {
        Protocol::ALL.into_iter().find(|p| p.id() == id)
    }

    /// Whether a receiver asks for a CRC first (it falls back to the sum if the sender doesn't answer that).
    fn asks_for_crc(self) -> bool {
        self != Protocol::Xmodem
    }

    /// The biggest block a sender uses.
    fn block_size(self) -> usize {
        match self {
            Protocol::Xmodem | Protocol::XmodemCrc => 128,
            Protocol::Xmodem1k | Protocol::Ymodem => 1024,
        }
    }
}

/// How long each side waits, and how many times it tries again before giving up. The defaults are the
/// reference's: a minute for the other side to start, ten seconds for an answer, a second between the bytes of a
/// block, and ten tries.
#[derive(Clone, Copy, Debug)]
pub struct Timing {
    /// How long to wait for the other side to begin at all.
    pub start: Duration,
    /// How long a sender waits for the answer to a block, and a receiver for the next block.
    pub answer: Duration,
    /// The longest gap allowed between two bytes of one block.
    pub byte: Duration,
    /// How often a receiver asks again while waiting for the first block.
    pub poll: Duration,
    /// How many times one block (or one answer) is tried before the transfer is given up.
    pub retries: u32,
}

impl Default for Timing {
    fn default() -> Self {
        Timing { start: Duration::from_secs(60), answer: Duration::from_secs(10), byte: Duration::from_secs(1), poll: Duration::from_secs(3), retries: 10 }
    }
}

/// How a transfer is going, passed to whoever is watching after each block.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Progress {
    /// The file being sent or received; empty for XMODEM receiving, which isn't told a name.
    pub file: String,
    /// Bytes of this file that have gone across.
    pub done: u64,
    /// The file's whole size, where it's known.
    pub total: Option<u64>,
    /// Blocks sent again, or asked for again, so far in the whole transfer.
    pub retries: u32,
    /// Files finished, in a YMODEM batch.
    pub files_done: usize,
}

/// A file to send.
#[derive(Clone, Debug)]
pub struct Outgoing {
    pub name: String,
    pub data: Vec<u8>,
    /// When it was last changed, in seconds since 1970, for YMODEM's header.
    pub modified: Option<u64>,
}

/// A file received.
#[derive(Clone, Debug, PartialEq)]
pub struct Incoming {
    /// The name the sender gave (YMODEM only).
    pub name: Option<String>,
    pub data: Vec<u8>,
    pub modified: Option<u64>,
}

/// Why a transfer ended without finishing.
#[derive(Clone, Debug, PartialEq)]
pub enum Failure {
    /// The other side cancelled it.
    Cancelled,
    /// The person stopped it here.
    Stopped,
    /// The other side never began.
    NeverStarted,
    /// One block (or the end) failed too many times.
    TooManyErrors,
    /// The other side sent blocks out of order, or something that isn't these protocols.
    OutOfStep,
    /// Reading or writing the line failed.
    Line(String),
}

impl Failure {
    pub fn describe(&self) -> String {
        match self {
            Failure::Cancelled => crate::i18n::tr("The other side cancelled the transfer.").into(),
            Failure::Stopped => crate::i18n::tr("The transfer was stopped.").into(),
            Failure::NeverStarted => crate::i18n::tr("The other side never began. Start the transfer there too (or first, if it's sending).").into(),
            Failure::TooManyErrors => crate::i18n::tr("Too many blocks went wrong, so the transfer was given up. A slower speed, or flow control, may help.").into(),
            Failure::OutOfStep => crate::i18n::tr("The other side sent blocks out of order, so the transfer was given up.").into(),
            Failure::Line(e) => crate::i18n::tf!("The connection failed: {e}"),
        }
    }
}

impl From<io::Error> for Failure {
    fn from(e: io::Error) -> Self {
        Failure::Line(e.to_string())
    }
}

/// Something bytes go both ways over.
pub trait Line {
    /// The next byte, waiting up to `wait` for it: `None` if none came in that time.
    fn get(&mut self, wait: Duration) -> io::Result<Option<u8>>;
    /// Sends the bytes, all of them.
    fn put(&mut self, bytes: &[u8]) -> io::Result<()>;

    /// Throws away whatever has already arrived, without waiting for more.
    fn drain(&mut self) -> io::Result<()> {
        while self.get(Duration::ZERO)?.is_some() {}
        Ok(())
    }

    /// Throws away what arrives until the line has been quiet for a moment: what's left of a bad block.
    fn purge(&mut self, quiet: Duration) -> io::Result<()> {
        let began = Instant::now();
        // A line that never goes quiet (a noisy cable) mustn't hold the transfer up for ever.
        while self.get(quiet)?.is_some() && began.elapsed() < quiet * 20 {}
        Ok(())
    }
}

/// A reader and writer opened with a short read timeout, as a serial port or a socket is, made into a line.
pub struct Stream<T: Read + Write> {
    inner: T,
    buffer: Vec<u8>,
    at: usize,
}

impl<T: Read + Write> Stream<T> {
    pub fn new(inner: T) -> Self {
        Stream { inner, buffer: Vec::new(), at: 0 }
    }

    /// Whatever was read ahead but not yet taken, for the terminal to show once a transfer is over.
    pub fn leftover(&self) -> &[u8] {
        &self.buffer[self.at..]
    }
}

impl<T: Read + Write> Line for Stream<T> {
    fn get(&mut self, wait: Duration) -> io::Result<Option<u8>> {
        let until = Instant::now() + wait;
        loop {
            if self.at < self.buffer.len() {
                self.at += 1;
                return Ok(Some(self.buffer[self.at - 1]));
            }
            self.buffer.resize(4096, 0);
            self.at = 0;
            let got = self.inner.read(&mut self.buffer);
            match got {
                Ok(n) => self.buffer.truncate(n),
                Err(e) if matches!(e.kind(), io::ErrorKind::TimedOut | io::ErrorKind::WouldBlock | io::ErrorKind::Interrupted) => self.buffer.clear(),
                Err(e) => {
                    self.buffer.clear();
                    return Err(e);
                }
            }
            if self.buffer.is_empty() {
                if Instant::now() >= until {
                    return Ok(None);
                }
                // A port that answers at once with nothing rather than waiting mustn't spin.
                std::thread::sleep(Duration::from_millis(1));
            }
        }
    }

    fn put(&mut self, bytes: &[u8]) -> io::Result<()> {
        self.inner.write_all(bytes)?;
        self.inner.flush()
    }
}

/// XMODEM's CRC: CRC-16 with the CCITT polynomial (0x1021), starting at zero.
pub fn crc16(data: &[u8]) -> u16 {
    let mut crc: u16 = 0;
    for &byte in data {
        crc ^= u16::from(byte) << 8;
        for _ in 0..8 {
            crc = if crc & 0x8000 != 0 { (crc << 1) ^ 0x1021 } else { crc << 1 };
        }
    }
    crc
}

/// The original XMODEM check: the bytes added up, keeping the low eight bits.
pub fn checksum(data: &[u8]) -> u8 {
    data.iter().fold(0u8, |sum, b| sum.wrapping_add(*b))
}

/// A block as it goes down the line: its start (SOH for 128 bytes, STX for 1024), its number and that number's
/// complement, the data padded out with `pad`, then the check.
pub fn block(number: u8, data: &[u8], size: usize, crc: bool, pad: u8) -> Vec<u8> {
    let mut out = Vec::with_capacity(size + 5);
    out.push(if size == 1024 { STX } else { SOH });
    out.push(number);
    out.push(!number);
    out.extend_from_slice(data);
    out.resize(3 + size, pad);
    let body = &out[3..];
    if crc {
        let crc = crc16(body);
        out.extend_from_slice(&crc.to_be_bytes());
    } else {
        let sum = checksum(body);
        out.push(sum);
    }
    out
}

/// YMODEM's block 0: the name, a nought, then the size in decimal and the time it was changed in octal (seconds
/// since 1970), as `sz` writes them. Blank for the end of a batch.
pub fn header(file: Option<&Outgoing>) -> Vec<u8> {
    let mut out = Vec::new();
    if let Some(file) = file {
        // Only the name goes, not the folders; spaces become underscores, as the reference asks.
        let name = file.name.rsplit(['/', '\\']).next().unwrap_or(&file.name).replace(' ', "_");
        out.extend_from_slice(name.as_bytes());
        out.push(0);
        out.extend_from_slice(file.data.len().to_string().as_bytes());
        if let Some(when) = file.modified {
            out.extend_from_slice(format!(" {when:o}").as_bytes());
        }
        out.push(0);
    }
    out
}

/// What a YMODEM header says: the name, and the size and time if it gives them. `None` for the blank header
/// that ends a batch.
pub fn read_header(data: &[u8]) -> Option<(String, Option<u64>, Option<u64>)> {
    let end = data.iter().position(|b| *b == 0).unwrap_or(data.len());
    if end == 0 {
        return None;
    }
    let name = String::from_utf8_lossy(&data[..end]).into_owned();
    let rest = data.get(end + 1..).unwrap_or_default();
    let rest = &rest[..rest.iter().position(|b| *b == 0).unwrap_or(rest.len())];
    let rest = String::from_utf8_lossy(rest);
    let mut fields = rest.split_ascii_whitespace();
    let size = fields.next().and_then(|s| s.parse().ok());
    let modified = fields.next().and_then(|s| u64::from_str_radix(s, 8).ok()).filter(|t| *t != 0);
    Some((name, size, modified))
}

/// One byte, waiting no longer than `wait`, and noticing the person's stop every tenth of a second meanwhile.
fn get(line: &mut dyn Line, wait: Duration, stop: &AtomicBool) -> Result<Option<u8>, Failure> {
    let until = Instant::now() + wait;
    loop {
        if stop.load(Ordering::Relaxed) {
            cancel(line);
            return Err(Failure::Stopped);
        }
        let left = until.saturating_duration_since(Instant::now());
        if let Some(byte) = line.get(left.min(Duration::from_millis(100)))? {
            return Ok(Some(byte));
        }
        if left.is_zero() {
            return Ok(None);
        }
    }
}

/// Tells the other side the transfer is over: CAN twice is enough, but more makes sure, as `sz` does.
pub fn cancel(line: &mut dyn Line) {
    let _ = line.put(&[CAN; 8]);
}

/// Whether a CAN just read is the first of two: a lone one may be noise inside a block's answer.
fn second_cancel(line: &mut dyn Line, timing: &Timing, stop: &AtomicBool) -> Result<bool, Failure> {
    Ok(get(line, timing.byte, stop)? == Some(CAN))
}

/// What a sender heard after a block.
enum Answer {
    Yes,
    No,
    Nothing,
}

/// Waits for the answer to a block. A C from a receiver still asking to begin is ignored even then: taking it
/// for a NAK would send the first block twice, and the second ACK would be taken for the next block's.
fn answer(line: &mut dyn Line, timing: &Timing, stop: &AtomicBool) -> Result<Answer, Failure> {
    let until = Instant::now() + timing.answer;
    loop {
        let left = until.saturating_duration_since(Instant::now());
        match get(line, left, stop)? {
            None => return Ok(Answer::Nothing),
            Some(ACK) => return Ok(Answer::Yes),
            Some(NAK) => return Ok(Answer::No),
            Some(CAN) if second_cancel(line, timing, stop)? => return Err(Failure::Cancelled),
            Some(_) => {}
        }
    }
}

/// Sends one block until the receiver takes it.
fn send_block(line: &mut dyn Line, packet: &[u8], timing: &Timing, stop: &AtomicBool, progress: &mut Progress) -> Result<(), Failure> {
    let mut tries = 0;
    loop {
        line.put(packet)?;
        match answer(line, timing, stop)? {
            Answer::Yes => return Ok(()),
            Answer::No | Answer::Nothing => {
                tries += 1;
                progress.retries += 1;
                if tries > timing.retries {
                    cancel(line);
                    return Err(Failure::TooManyErrors);
                }
                // What's left of a garbled answer mustn't be taken for the next one.
                line.drain()?;
            }
        }
    }
}

/// Waits for the receiver to ask for the first block: NAK for sums, C for a CRC. Says which.
fn wait_to_start(line: &mut dyn Line, wait: Duration, timing: &Timing, stop: &AtomicBool) -> Result<Option<bool>, Failure> {
    let until = Instant::now() + wait;
    loop {
        let left = until.saturating_duration_since(Instant::now());
        match get(line, left, stop)? {
            None => return Ok(None),
            Some(NAK) => return Ok(Some(false)),
            Some(WANT_CRC) => return Ok(Some(true)),
            Some(CAN) if second_cancel(line, timing, stop)? => return Err(Failure::Cancelled),
            Some(_) => {}
        }
    }
}

/// Sends a file's data as blocks 1 onwards, then the end.
fn send_data(line: &mut dyn Line, data: &[u8], size: usize, crc: bool, ymodem: bool, timing: &Timing, stop: &AtomicBool, progress: &mut Progress, report: &mut dyn FnMut(&Progress)) -> Result<(), Failure> {
    // Repeated requests to start that crossed with the first block would read as answers to it.
    line.drain()?;
    let mut number: u8 = 1;
    let mut at = 0;
    while at < data.len() {
        let left = data.len() - at;
        // The tail goes in short blocks when that wastes less of the line than padding a long one.
        let size = if size == 1024 && left > 896 { 1024 } else { 128 };
        let piece = &data[at..(at + size).min(data.len())];
        send_block(line, &block(number, piece, size, crc, SUB), timing, stop, progress)?;
        at += piece.len();
        number = number.wrapping_add(1);
        progress.done = at as u64;
        report(progress);
    }
    // The end: YMODEM receivers say no to the first EOT, to make sure of it, and yes to the second.
    let mut tries = 0;
    loop {
        line.put(&[EOT])?;
        match answer(line, timing, stop)? {
            Answer::Yes => return Ok(()),
            Answer::No if ymodem && tries == 0 => tries += 1,
            _ => {
                tries += 1;
                progress.retries += 1;
                if tries > timing.retries {
                    cancel(line);
                    return Err(Failure::TooManyErrors);
                }
            }
        }
    }
}

/// Sends files: the first alone for XMODEM, all of them for YMODEM.
pub fn send(line: &mut dyn Line, protocol: Protocol, files: &[Outgoing], timing: &Timing, stop: &AtomicBool, report: &mut dyn FnMut(&Progress)) -> Result<(), Failure> {
    let mut progress = Progress::default();
    let Some(crc) = wait_to_start(line, timing.start, timing, stop)? else {
        cancel(line);
        return Err(Failure::NeverStarted);
    };
    if protocol != Protocol::Ymodem {
        let file = files.first().ok_or(Failure::OutOfStep)?;
        progress.file = file.name.clone();
        progress.total = Some(file.data.len() as u64);
        report(&progress);
        // 1K blocks need the CRC: a receiver that only knows sums gets 128-byte ones.
        let size = if crc { protocol.block_size() } else { 128 };
        send_data(line, &file.data, size, crc, false, timing, stop, &mut progress, report)?;
        progress.files_done = 1;
        report(&progress);
        return Ok(());
    }
    for (index, file) in files.iter().enumerate() {
        if index > 0 && wait_to_start(line, timing.answer, timing, stop)?.is_none() {
            cancel(line);
            return Err(Failure::TooManyErrors);
        }
        progress.file = file.name.clone();
        progress.done = 0;
        progress.total = Some(file.data.len() as u64);
        report(&progress);
        send_header(line, Some(file), crc, timing, stop, &mut progress)?;
        send_data(line, &file.data, 1024, crc, true, timing, stop, &mut progress, report)?;
        progress.files_done += 1;
        report(&progress);
    }
    // The batch's end: a blank header, once the receiver asks for the next file.
    if wait_to_start(line, timing.answer, timing, stop)?.is_none() {
        cancel(line);
        return Err(Failure::TooManyErrors);
    }
    let packet = block(0, &header(None), 128, crc, 0);
    send_block(line, &packet, timing, stop, &mut progress)
}

/// Sends a YMODEM header, until the receiver takes it and asks for the data with another C.
fn send_header(line: &mut dyn Line, file: Option<&Outgoing>, crc: bool, timing: &Timing, stop: &AtomicBool, progress: &mut Progress) -> Result<(), Failure> {
    let bytes = header(file);
    let packet = block(0, &bytes, if bytes.len() > 128 { 1024 } else { 128 }, crc, 0);
    let mut tries = 0;
    loop {
        send_block(line, &packet, timing, stop, progress)?;
        if wait_to_start(line, timing.answer, timing, stop)?.is_some() {
            return Ok(());
        }
        tries += 1;
        progress.retries += 1;
        if tries > timing.retries {
            cancel(line);
            return Err(Failure::TooManyErrors);
        }
    }
}

/// A block as the receiver read it.
enum Got {
    Good(u8, Vec<u8>),
    /// Garbled, cut short or failing its check: to be asked for again.
    Bad,
}

/// Reads the rest of a block whose first byte (SOH or STX) has come.
fn read_block(line: &mut dyn Line, start: u8, crc: bool, timing: &Timing, stop: &AtomicBool) -> Result<Got, Failure> {
    let size = if start == STX { 1024 } else { 128 };
    let length = 2 + size + if crc { 2 } else { 1 };
    let mut bytes = Vec::with_capacity(length);
    while bytes.len() < length {
        match get(line, timing.byte, stop)? {
            Some(b) => bytes.push(b),
            None => return Ok(Got::Bad),
        }
    }
    let (number, complement) = (bytes[0], bytes[1]);
    if number != !complement {
        return Ok(Got::Bad);
    }
    let data = &bytes[2..2 + size];
    let good = if crc { u16::from_be_bytes([bytes[2 + size], bytes[3 + size]]) == crc16(data) } else { bytes[2 + size] == checksum(data) };
    Ok(if good { Got::Good(number, data.to_vec()) } else { Got::Bad })
}

/// What a receiver found waiting for a block.
enum Arrived {
    Block(u8, Vec<u8>),
    End,
}

/// The receiver's side of one file (or one YMODEM header): asks to start, then reads blocks until the end.
struct Receiver<'a> {
    line: &'a mut dyn Line,
    timing: Timing,
    stop: &'a AtomicBool,
    crc: bool,
    /// Whether the kind of check has been settled by a block arriving.
    settled: bool,
    /// Whether it may fall back to sums when C goes unanswered: XMODEM only, as YMODEM always has the CRC.
    fallback: bool,
    retries: u32,
}

impl Receiver<'_> {
    /// Waits for the next block. `ask` is what asks the sender to begin (C or NAK), sent at once and again
    /// while nothing comes; once blocks are flowing it's None, and a NAK asks for one again. Errors are counted,
    /// and too many give the transfer up.
    fn next(&mut self, ask: Option<u8>, eot_twice: bool) -> Result<Arrived, Failure> {
        let mut errors = 0;
        let began = Instant::now();
        let mut asked = 0;
        let mut seen_eot = false;
        if let Some(ask) = ask {
            self.line.put(&[ask])?;
        }
        loop {
            // Waiting longer than the sender does for its answer means a lost block is sent again by the
            // sender before the receiver asks for it, so the two never both send at once and fall out of step.
            let wait = if ask.is_some() { self.timing.poll } else { self.timing.answer + self.timing.answer / 2 };
            let Some(first) = get(self.line, wait, self.stop)? else {
                if let Some(ask) = ask {
                    if began.elapsed() >= self.timing.start {
                        cancel(self.line);
                        return Err(Failure::NeverStarted);
                    }
                    asked += 1;
                    // A sender that doesn't answer C after three tries only knows sums.
                    if self.fallback && !self.settled && self.crc && asked >= 3 {
                        self.crc = false;
                    }
                    self.line.put(&[if ask == WANT_CRC && !self.crc { NAK } else { ask }])?;
                } else {
                    errors += 1;
                    self.retries += 1;
                    if errors > self.timing.retries {
                        cancel(self.line);
                        return Err(Failure::TooManyErrors);
                    }
                    self.line.put(&[NAK])?;
                }
                continue;
            };
            match first {
                SOH | STX => match read_block(self.line, first, self.crc, &self.timing, self.stop)? {
                    Got::Good(number, data) => {
                        self.settled = true;
                        return Ok(Arrived::Block(number, data));
                    }
                    Got::Bad => {
                        errors += 1;
                        self.retries += 1;
                        if errors > self.timing.retries {
                            cancel(self.line);
                            return Err(Failure::TooManyErrors);
                        }
                        self.line.purge(self.timing.byte)?;
                        self.line.put(&[NAK])?;
                    }
                },
                EOT => {
                    if eot_twice && !seen_eot {
                        seen_eot = true;
                        self.line.put(&[NAK])?;
                        continue;
                    }
                    return Ok(Arrived::End);
                }
                CAN if second_cancel(self.line, &self.timing, self.stop)? => return Err(Failure::Cancelled),
                // Noise between blocks.
                _ => {}
            }
        }
    }

    /// Reads a file's blocks, numbered from 1, until its end; ACKs each.
    fn data(&mut self, ymodem: bool, name: &str, total: Option<u64>, progress: &mut Progress, report: &mut dyn FnMut(&Progress)) -> Result<Vec<u8>, Failure> {
        let mut data = Vec::new();
        let mut expected: u8 = 1;
        let mut ask = Some(if self.crc { WANT_CRC } else { NAK });
        progress.file = name.to_string();
        progress.done = 0;
        progress.total = total;
        loop {
            let arrived = self.next(ask, ymodem)?;
            progress.retries = self.retries;
            match arrived {
                Arrived::End => {
                    self.line.put(&[ACK])?;
                    return Ok(data);
                }
                Arrived::Block(number, block) if number == expected => {
                    data.extend_from_slice(&block);
                    expected = expected.wrapping_add(1);
                    ask = None;
                    self.line.put(&[ACK])?;
                    progress.done = total.map_or(data.len() as u64, |t| t.min(data.len() as u64));
                    report(progress);
                }
                // The block before again: the sender missed the ACK. Said yes to again, kept once.
                Arrived::Block(number, _) if number == expected.wrapping_sub(1) => {
                    // A YMODEM header again, before any data, is answered the same way: the C that the sender
                    // is waiting for goes again as this loop asks for the first block.
                    self.line.put(&[ACK])?;
                }
                Arrived::Block(..) => {
                    cancel(self.line);
                    return Err(Failure::OutOfStep);
                }
            }
        }
    }
}

/// Receives: one file for XMODEM (its length rounded up to a whole block, the padding left on unless `trim`
/// is set, when trailing SUB bytes are taken off), or a batch of files for YMODEM, each at its right length.
pub fn receive(line: &mut dyn Line, protocol: Protocol, trim: bool, timing: &Timing, stop: &AtomicBool, report: &mut dyn FnMut(&Progress)) -> Result<Vec<Incoming>, Failure> {
    let mut progress = Progress::default();
    let mut receiver = Receiver { line, timing: *timing, stop, crc: protocol.asks_for_crc(), settled: false, fallback: protocol != Protocol::Ymodem, retries: 0 };
    if protocol != Protocol::Ymodem {
        let mut data = receiver.data(false, "", None, &mut progress, report)?;
        if trim {
            while data.last() == Some(&SUB) {
                data.pop();
            }
        }
        progress.files_done = 1;
        report(&progress);
        return Ok(vec![Incoming { name: None, data, modified: None }]);
    }
    let mut files = Vec::new();
    loop {
        // Each file begins with its header, asked for with a C.
        let arrived = receiver.next(Some(WANT_CRC), false)?;
        let header = match arrived {
            Arrived::Block(0, header) => header,
            // The last file's second EOT again: its ACK went astray.
            Arrived::End => {
                receiver.line.put(&[ACK])?;
                continue;
            }
            // The last file's final block again, before its end.
            Arrived::Block(..) if !files.is_empty() => {
                receiver.line.put(&[ACK])?;
                continue;
            }
            Arrived::Block(..) => {
                cancel(receiver.line);
                return Err(Failure::OutOfStep);
            }
        };
        receiver.line.put(&[ACK])?;
        let Some((name, size, modified)) = read_header(&header) else {
            return Ok(files);
        };
        let mut data = receiver.data(true, &name, size, &mut progress, report)?;
        if let Some(size) = size {
            data.truncate(size as usize);
        }
        files.push(Incoming { name: Some(name), data, modified });
        progress.files_done = files.len();
        report(&progress);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::VecDeque;
    use std::sync::mpsc::{Receiver as Rx, Sender, channel};
    use std::sync::{Arc, Mutex};

    /// What a fault does to one write.
    type Fault = Box<dyn FnMut(usize, &mut Vec<u8>) + Send>;

    /// One end of an in-memory cable. Each write is one message; a fault can garble or lose it.
    struct End {
        to: Sender<Vec<u8>>,
        from: Rx<Vec<u8>>,
        waiting: VecDeque<u8>,
        fault: Option<Fault>,
        writes: usize,
        /// Everything this end sent, before any fault, for looking at afterwards.
        sent: Arc<Mutex<Vec<u8>>>,
    }

    fn cable() -> (End, End) {
        let (a_to, b_from) = channel();
        let (b_to, a_from) = channel();
        let end = |to, from| End { to, from, waiting: VecDeque::new(), fault: None, writes: 0, sent: Arc::default() };
        (end(a_to, a_from), end(b_to, b_from))
    }

    impl Line for End {
        fn get(&mut self, wait: Duration) -> io::Result<Option<u8>> {
            if let Some(b) = self.waiting.pop_front() {
                return Ok(Some(b));
            }
            let got = if wait.is_zero() { self.from.try_recv().ok() } else { self.from.recv_timeout(wait).ok() };
            if let Some(bytes) = got {
                self.waiting.extend(bytes);
            }
            Ok(self.waiting.pop_front())
        }

        fn put(&mut self, bytes: &[u8]) -> io::Result<()> {
            self.sent.lock().unwrap().extend_from_slice(bytes);
            let mut bytes = bytes.to_vec();
            if let Some(fault) = &mut self.fault {
                fault(self.writes, &mut bytes);
            }
            self.writes += 1;
            if !bytes.is_empty() {
                let _ = self.to.send(bytes);
            }
            Ok(())
        }
    }

    fn quick() -> Timing {
        Timing { start: Duration::from_secs(5), answer: Duration::from_millis(300), byte: Duration::from_millis(100), poll: Duration::from_millis(100), retries: 10 }
    }

    fn pattern(length: usize, seed: u32) -> Vec<u8> {
        let mut x = seed.wrapping_mul(2_654_435_761).max(1);
        (0..length)
            .map(|_| {
                x ^= x << 13;
                x ^= x >> 17;
                x ^= x << 5;
                x as u8
            })
            .collect()
    }

    /// Runs a sender and a receiver against each other, each on its own thread, and gives back both results.
    fn transfer(protocol_out: Protocol, protocol_in: Protocol, files: Vec<Outgoing>, sender: End, receiver: End, trim: bool) -> (Result<(), Failure>, Result<Vec<Incoming>, Failure>, Vec<Progress>) {
        let stop = Arc::new(AtomicBool::new(false));
        let (s1, s2) = (stop.clone(), stop.clone());
        let sending = std::thread::spawn(move || {
            let mut line = sender;
            send(&mut line, protocol_out, &files, &quick(), &s1, &mut |_| {})
        });
        let receiving = std::thread::spawn(move || {
            let mut line = receiver;
            let mut seen = Vec::new();
            let result = receive(&mut line, protocol_in, trim, &quick(), &s2, &mut |p| seen.push(p.clone()));
            (result, seen)
        });
        let sent = sending.join().unwrap();
        let (received, seen) = receiving.join().unwrap();
        (sent, received, seen)
    }

    fn one(name: &str, data: Vec<u8>) -> Outgoing {
        Outgoing { name: name.into(), data, modified: None }
    }

    #[test]
    fn the_crc_and_sum_are_xmodem_s() {
        // The standard check value of CRC-16/XMODEM.
        assert_eq!(crc16(b"123456789"), 0x31C3);
        assert_eq!(checksum(&[0xFF, 0x02]), 0x01);
        // A block laid out by hand: SOH, 1, 254, "AB" padded with SUB to 128, then the sum.
        let made = block(1, b"AB", 128, false, SUB);
        assert_eq!(made.len(), 132);
        assert_eq!(&made[..5], &[SOH, 1, 254, b'A', b'B']);
        assert!(made[5..131].iter().all(|b| *b == SUB));
        assert_eq!(made[131], checksum(&made[3..131]));
        let long = block(7, &[0; 1024], 1024, true, SUB);
        assert_eq!((long[0], long.len()), (STX, 1029));
        assert_eq!(&long[1027..], &[0, 0], "the CRC of noughts is nought");
    }

    #[test]
    fn ymodem_headers_carry_the_name_size_and_time() {
        let file = Outgoing { name: "games/My Game.prg".into(), data: vec![0; 1234], modified: Some(0o14_000_000_000) };
        let bytes = header(Some(&file));
        assert_eq!(bytes, b"My_Game.prg\x001234 14000000000\x00");
        assert_eq!(read_header(&bytes), Some(("My_Game.prg".into(), Some(1234), Some(0o14_000_000_000))));
        // As sz writes it, with its mode and serial number after.
        assert_eq!(read_header(b"a.bin\x00100 14472213760 100644 0 1 100\x00\x00\x00"), Some(("a.bin".into(), Some(100), Some(0o14472213760))));
        // A name with nothing after it, and the blank header that ends a batch.
        assert_eq!(read_header(b"b\x00\x00"), Some(("b".into(), None, None)));
        assert_eq!(read_header(&[0; 128]), None);
    }

    #[test]
    fn each_protocol_sends_files_of_awkward_lengths_intact() {
        // Lengths either side of each block size, and one long enough for the block numbers to wrap past 255.
        for protocol in Protocol::ALL {
            for length in [0usize, 1, 127, 128, 129, 896, 897, 1023, 1024, 1025, 70_000] {
                let data = pattern(length, length as u32 + 1);
                let (a, b) = cable();
                let (sent, received, _) = transfer(protocol, protocol, vec![one("t.bin", data.clone())], a, b, false);
                assert_eq!(sent, Ok(()), "{protocol:?} {length}");
                let received = received.unwrap();
                assert_eq!(received.len(), 1);
                let got = &received[0].data;
                if protocol == Protocol::Ymodem {
                    assert_eq!(got, &data, "{protocol:?} {length}");
                    assert_eq!(received[0].name.as_deref(), Some("t.bin"));
                } else {
                    // XMODEM doesn't know the length: the last block is padded out with SUB.
                    assert_eq!(&got[..length], &data[..], "{protocol:?} {length}");
                    assert!(got[length..].iter().all(|b| *b == SUB));
                    assert_eq!(got.len() % 128, 0);
                }
            }
        }
    }

    #[test]
    fn short_blocks_finish_a_1k_file_and_padding_can_be_trimmed() {
        let (a, b) = cable();
        let sent_bytes = a.sent.clone();
        let data = b"HELLO".repeat(250); // 1250 bytes: one 1K block, then two of 128.
        let (sent, received, _) = transfer(Protocol::Xmodem1k, Protocol::Xmodem1k, vec![one("h", data.clone())], a, b, true);
        assert_eq!(sent, Ok(()));
        assert_eq!(received.unwrap()[0].data, data);
        let wire = sent_bytes.lock().unwrap().clone();
        assert_eq!(wire[0], STX);
        assert_eq!(wire[1029], SOH);
        assert_eq!(wire[1029 + 133], SOH);
        assert_eq!(*wire.last().unwrap(), EOT);
    }

    #[test]
    fn a_receiver_that_knows_only_sums_gets_them() {
        // XMODEM-1K asked for, but the receiver sends NAK (sums only): 128-byte blocks with sums go.
        let (a, b) = cable();
        let sent_bytes = a.sent.clone();
        let data = pattern(3000, 9);
        let (sent, received, _) = transfer(Protocol::Xmodem1k, Protocol::Xmodem, vec![one("x", data.clone())], a, b, false);
        assert_eq!(sent, Ok(()));
        assert_eq!(&received.unwrap()[0].data[..3000], &data[..]);
        let wire = sent_bytes.lock().unwrap().clone();
        assert_eq!((wire[0], wire[132]), (SOH, SOH), "128-byte blocks, 132 bytes each with the sum");
    }

    #[test]
    fn a_receiver_falls_back_to_sums_when_a_sender_ignores_c() {
        // An old sender that waits for NAK alone: the receiver gives up asking for C after three tries.
        let (mut old, b) = cable();
        let data = pattern(200, 4);
        let expected = data.clone();
        let stop = Arc::new(AtomicBool::new(false));
        let receiving = std::thread::spawn(move || {
            let mut line = b;
            receive(&mut line, Protocol::XmodemCrc, false, &quick(), &stop, &mut |_| {})
        });
        let mut cs = 0;
        loop {
            match old.get(Duration::from_secs(5)).unwrap() {
                Some(WANT_CRC) => cs += 1,
                Some(NAK) => break,
                other => panic!("unexpected {other:?}"),
            }
        }
        assert_eq!(cs, 3);
        for (n, piece) in data.chunks(128).enumerate() {
            old.put(&block(n as u8 + 1, piece, 128, false, SUB)).unwrap();
            assert_eq!(old.get(Duration::from_secs(5)).unwrap(), Some(ACK));
        }
        old.put(&[EOT]).unwrap();
        assert_eq!(old.get(Duration::from_secs(5)).unwrap(), Some(ACK));
        let got = receiving.join().unwrap().unwrap();
        assert_eq!(&got[0].data[..200], &expected[..]);
    }

    #[test]
    fn a_ymodem_batch_keeps_names_sizes_and_times() {
        let files = vec![
            Outgoing { name: "first.prg".into(), data: pattern(5000, 1), modified: Some(1_700_000_000) },
            Outgoing { name: "empty.txt".into(), data: Vec::new(), modified: None },
            Outgoing { name: "third.d64".into(), data: pattern(174_848, 3), modified: Some(1) },
        ];
        let (a, b) = cable();
        let (sent, received, seen) = transfer(Protocol::Ymodem, Protocol::Ymodem, files.clone(), a, b, false);
        assert_eq!(sent, Ok(()));
        let received = received.unwrap();
        assert_eq!(received.len(), 3);
        for (out, got) in files.iter().zip(&received) {
            assert_eq!(got.name.as_deref(), Some(out.name.as_str()));
            assert_eq!(got.data, out.data);
            assert_eq!(got.modified, out.modified);
        }
        // Progress was told of each file, knowing its size, and ended with all three done.
        assert!(seen.iter().any(|p| p.file == "third.d64" && p.total == Some(174_848) && p.done > 0 && p.done < 174_848));
        assert_eq!(seen.last().unwrap().files_done, 3);
    }

    #[test]
    fn garbled_and_lost_blocks_are_sent_again() {
        // The sender's 4th write is garbled, its 7th lost entirely, and its 10th cut short.
        let (mut a, b) = cable();
        a.fault = Some(Box::new(|n, bytes: &mut Vec<u8>| match n {
            3 => bytes[40] ^= 0x55,
            6 => bytes.clear(),
            9 => bytes.truncate(50),
            _ => {}
        }));
        let data = pattern(20 * 128, 7);
        let (sent, received, seen) = transfer(Protocol::XmodemCrc, Protocol::XmodemCrc, vec![one("r", data.clone())], a, b, false);
        assert_eq!(sent, Ok(()));
        assert_eq!(received.unwrap()[0].data, data, "nothing doubled, nothing missing");
        // The receiver asked again for the garbled and the short block; the lost one the sender sent again by
        // itself, before the receiver's longer wait ran out.
        assert!(seen.last().unwrap().retries >= 2, "{:?}", seen.last());
    }

    #[test]
    fn a_lost_ack_brings_a_repeat_that_is_kept_once() {
        // The receiver's writes: 'C' first, then ACKs; the third ACK is lost, so block 3 comes twice.
        let (a, mut b) = cable();
        b.fault = Some(Box::new(|n, bytes: &mut Vec<u8>| {
            if n == 3 {
                bytes.clear();
            }
        }));
        let data = pattern(10 * 1024, 11);
        let (sent, received, _) = transfer(Protocol::Ymodem, Protocol::Ymodem, vec![one("y", data.clone())], a, b, false);
        assert_eq!(sent, Ok(()));
        assert_eq!(received.unwrap()[0].data, data);
        // The checksum variant too, losing an ACK in the middle.
        let (a, mut b) = cable();
        b.fault = Some(Box::new(|n, bytes: &mut Vec<u8>| {
            if n == 5 {
                bytes.clear();
            }
        }));
        let (sent, received, _) = transfer(Protocol::Xmodem, Protocol::Xmodem, vec![one("x", data.clone())], a, b, true);
        assert_eq!(sent, Ok(()));
        assert_eq!(received.unwrap()[0].data, data);
    }

    #[test]
    fn a_line_that_always_garbles_is_given_up() {
        let (mut a, b) = cable();
        a.fault = Some(Box::new(|n, bytes: &mut Vec<u8>| {
            if n > 0 && bytes.len() > 100 {
                bytes[60] ^= 1;
            }
        }));
        let (sent, received, _) = transfer(Protocol::XmodemCrc, Protocol::XmodemCrc, vec![one("g", pattern(1000, 2))], a, b, false);
        // One side gives up, and tells the other, which hears it as cancelling.
        let received = received.map(|_| ());
        assert!(matches!(sent, Err(Failure::TooManyErrors | Failure::Cancelled)), "{sent:?}");
        assert!(matches!(received, Err(Failure::TooManyErrors | Failure::Cancelled)), "{received:?}");
        assert!(sent == Err(Failure::TooManyErrors) || received == Err(Failure::TooManyErrors));
    }

    #[test]
    fn cancelling_and_stopping_end_both_sides() {
        // The receiver cancels after the first block: the sender hears it.
        let (mut a, b) = cable();
        let stop = Arc::new(AtomicBool::new(false));
        let sending = std::thread::spawn(move || {
            let mut line = b;
            send(&mut line, Protocol::XmodemCrc, &[one("c", pattern(5000, 1))], &quick(), &AtomicBool::new(false), &mut |_| {})
        });
        a.put(&[WANT_CRC]).unwrap();
        let mut got = Vec::new();
        while got.len() < 133 {
            got.push(a.get(Duration::from_secs(2)).unwrap().unwrap());
        }
        a.put(&[CAN, CAN]).unwrap();
        assert_eq!(sending.join().unwrap(), Err(Failure::Cancelled));
        // The person stops a receiver: it sends CANs, which the sender takes as cancelling.
        let (a, b) = cable();
        let (s1, s2) = (stop.clone(), stop.clone());
        let receiving = std::thread::spawn(move || {
            let mut line = b;
            let mut count = 0;
            receive(&mut line, Protocol::Xmodem1k, false, &quick(), &s2, &mut |_| {
                count += 1;
                if count == 3 {
                    s1.store(true, Ordering::Relaxed);
                }
            })
        });
        let mut line = a;
        let sent = send(&mut line, Protocol::Xmodem1k, &[one("s", pattern(50_000, 5))], &quick(), &AtomicBool::new(false), &mut |_| {});
        assert_eq!(receiving.join().unwrap(), Err(Failure::Stopped));
        assert_eq!(sent, Err(Failure::Cancelled));
    }

    #[test]
    fn nobody_at_the_other_end_is_noticed() {
        let (mut a, _b) = cable();
        let timing = Timing { start: Duration::from_millis(400), ..quick() };
        assert_eq!(send(&mut a, Protocol::Xmodem, &[one("n", vec![1])], &timing, &AtomicBool::new(false), &mut |_| {}), Err(Failure::NeverStarted));
        let (mut a, b) = cable();
        assert_eq!(receive(&mut a, Protocol::Ymodem, false, &timing, &AtomicBool::new(false), &mut |_| {}), Err(Failure::NeverStarted));
        // It kept asking meanwhile, with C each time (YMODEM never falls back to sums), then cancelled.
        let mut heard = Vec::new();
        let mut b = b;
        while let Some(byte) = b.get(Duration::ZERO).unwrap() {
            heard.push(byte);
        }
        let cs = heard.iter().take_while(|b| **b == WANT_CRC).count();
        assert!(cs >= 3, "{heard:?}");
        assert!(heard[cs..].iter().all(|b| *b == CAN) && heard.len() > cs, "{heard:?}");
    }

    #[test]
    fn blocks_out_of_order_end_the_transfer() {
        let (mut a, b) = cable();
        let receiving = std::thread::spawn(move || {
            let mut line = b;
            receive(&mut line, Protocol::XmodemCrc, false, &quick(), &AtomicBool::new(false), &mut |_| {})
        });
        assert_eq!(a.get(Duration::from_secs(2)).unwrap(), Some(WANT_CRC));
        a.put(&block(1, b"one", 128, true, SUB)).unwrap();
        assert_eq!(a.get(Duration::from_secs(2)).unwrap(), Some(ACK));
        a.put(&block(5, b"five", 128, true, SUB)).unwrap();
        assert_eq!(receiving.join().unwrap(), Err(Failure::OutOfStep));
        assert_eq!(a.get(Duration::from_secs(2)).unwrap(), Some(CAN));
    }

    #[test]
    fn noise_before_a_block_is_ignored() {
        // Line noise (and a lone CAN, which isn't two) before the first block doesn't upset the receiver.
        let (mut a, b) = cable();
        a.fault = Some(Box::new(|n, bytes: &mut Vec<u8>| {
            if n == 0 {
                bytes.splice(0..0, [0x00, 0xFF, CAN, b'x']);
            }
        }));
        let data = pattern(300, 8);
        let (sent, received, _) = transfer(Protocol::XmodemCrc, Protocol::XmodemCrc, vec![one("n", data.clone())], a, b, true);
        assert_eq!(sent, Ok(()));
        assert_eq!(&received.unwrap()[0].data[..300], &data[..]);
    }

    #[cfg(unix)]
    #[test]
    fn a_ymodem_batch_crosses_a_real_socket() {
        use std::os::unix::net::UnixStream;
        let (a, b) = UnixStream::pair().unwrap();
        for end in [&a, &b] {
            end.set_read_timeout(Some(Duration::from_millis(20))).unwrap();
        }
        let files = vec![one("one.bin", pattern(40_000, 1)), one("two.bin", pattern(77, 2))];
        let out = files.clone();
        let sending = std::thread::spawn(move || send(&mut Stream::new(a), Protocol::Ymodem, &out, &quick(), &AtomicBool::new(false), &mut |_| {}));
        let received = receive(&mut Stream::new(b), Protocol::Ymodem, false, &quick(), &AtomicBool::new(false), &mut |_| {}).unwrap();
        assert_eq!(sending.join().unwrap(), Ok(()));
        assert_eq!(received.iter().map(|f| f.data.clone()).collect::<Vec<_>>(), files.iter().map(|f| f.data.clone()).collect::<Vec<_>>());
    }
}
