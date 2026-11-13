//! Reaching a machine while it runs: reading and writing its memory, reading its screen, typing on it and loading
//! into it. The built-in Spectrum is reached directly; ZEsarUX through its remote command protocol (ZRCP, text
//! over TCP); VICE through its binary monitor (framed binary over TCP).

use std::io::{Read, Write};
use std::net::TcpStream;
use std::path::Path;
use std::time::{Duration, Instant};

const WAIT: Duration = Duration::from_secs(5);

/// A machine that can be reached while it runs.
pub trait Live {
    fn read(&mut self, address: u16, length: usize) -> Result<Vec<u8>, String>;
    fn write(&mut self, address: u16, bytes: &[u8]) -> Result<(), String>;
}

// ---------- ZEsarUX ----------

/// A connection to ZEsarUX's remote command protocol.
pub struct Zrcp {
    stream: TcpStream,
}

impl Zrcp {
    pub fn connect(port: u16) -> Result<Zrcp, String> {
        let stream = TcpStream::connect_timeout(&([127, 0, 0, 1], port).into(), WAIT).map_err(|e| format!("ZEsarUX couldn't be reached on port {port}: {e}"))?;
        stream.set_read_timeout(Some(WAIT)).map_err(|e| e.to_string())?;
        let mut link = Zrcp { stream };
        // It greets, then waits at its prompt.
        link.until_prompt()?;
        Ok(link)
    }

    /// Everything said up to the next prompt.
    fn until_prompt(&mut self) -> Result<String, String> {
        let mut said = Vec::new();
        let mut buffer = [0u8; 65536];
        let began = Instant::now();
        loop {
            if said.ends_with(b"command> ") {
                said.truncate(said.len() - b"command> ".len());
                return Ok(String::from_utf8_lossy(&said).trim_end().to_string());
            }
            if began.elapsed() > WAIT {
                return Err("ZEsarUX stopped answering.".into());
            }
            let got = self.stream.read(&mut buffer).map_err(|e| format!("ZEsarUX stopped answering: {e}"))?;
            if got == 0 {
                return Err("ZEsarUX closed the connection.".into());
            }
            said.extend_from_slice(&buffer[..got]);
        }
    }

    /// Sends a command and returns what it said back.
    pub fn command(&mut self, command: &str) -> Result<String, String> {
        self.stream.write_all(format!("{command}\n").as_bytes()).map_err(|e| e.to_string())?;
        let said = self.until_prompt()?;
        if said.starts_with("Error") {
            return Err(format!("ZEsarUX said: {said}"));
        }
        Ok(said)
    }

    /// The screen as text, as ZEsarUX reads it.
    pub fn screen_text(&mut self) -> Result<String, String> {
        self.command("get-ocr")
    }
}

impl Live for Zrcp {
    fn read(&mut self, address: u16, length: usize) -> Result<Vec<u8>, String> {
        let said = self.command(&format!("read-memory {address} {length}"))?;
        let hex: String = said.chars().filter(char::is_ascii_hexdigit).collect();
        (0..hex.len() / 2).map(|i| u8::from_str_radix(&hex[i * 2..i * 2 + 2], 16).map_err(|e| e.to_string())).collect()
    }

    fn write(&mut self, address: u16, bytes: &[u8]) -> Result<(), String> {
        for (i, chunk) in bytes.chunks(256).enumerate() {
            let values: Vec<String> = chunk.iter().map(u8::to_string).collect();
            self.command(&format!("write-memory {} {}", address.wrapping_add((i * 256) as u16), values.join(" ")))?;
        }
        Ok(())
    }
}

/// Loads a file into a ZEsarUX that's already running, as its Smart Load does.
pub fn zrcp_load(port: u16, file: &Path) -> Result<(), String> {
    let mut link = Zrcp::connect(port)?;
    link.command(&format!("smartload {}", file.display()))?;
    Ok(())
}

// ---------- VICE ----------

/// A connection to VICE's binary monitor.
pub struct ViceMonitor {
    stream: TcpStream,
    request: u32,
}

const VICE_API: u8 = 2;
const MEMORY_GET: u8 = 0x01;
const MEMORY_SET: u8 = 0x02;
const EXIT: u8 = 0xAA;
const KEYBOARD_FEED: u8 = 0x72;

impl ViceMonitor {
    pub fn connect(port: u16) -> Result<ViceMonitor, String> {
        let stream = TcpStream::connect_timeout(&([127, 0, 0, 1], port).into(), WAIT).map_err(|e| format!("VICE couldn't be reached on port {port}: {e}"))?;
        stream.set_read_timeout(Some(WAIT)).map_err(|e| e.to_string())?;
        Ok(ViceMonitor { stream, request: 1 })
    }

    /// Sends a command and returns its response's body. Events VICE sends on its own along the way are skipped.
    fn command(&mut self, command: u8, body: &[u8]) -> Result<Vec<u8>, String> {
        self.request += 1;
        let id = self.request;
        let mut message = vec![0x02, VICE_API];
        message.extend_from_slice(&(body.len() as u32).to_le_bytes());
        message.extend_from_slice(&id.to_le_bytes());
        message.push(command);
        message.extend_from_slice(body);
        self.stream.write_all(&message).map_err(|e| e.to_string())?;
        loop {
            let mut head = [0u8; 12];
            self.stream.read_exact(&mut head).map_err(|e| format!("VICE stopped answering: {e}"))?;
            if head[0] != 0x02 {
                return Err("VICE answered in a way Limoniter doesn't understand.".into());
            }
            let length = u32::from_le_bytes([head[2], head[3], head[4], head[5]]) as usize;
            let error = head[7];
            let answered = u32::from_le_bytes([head[8], head[9], head[10], head[11]]);
            let mut response = vec![0u8; length];
            self.stream.read_exact(&mut response).map_err(|e| format!("VICE stopped answering: {e}"))?;
            if answered != id {
                continue;
            }
            if error != 0 {
                return Err(format!("VICE refused it (error {error:#04X})."));
            }
            return Ok(response);
        }
    }

    /// Lets the machine carry on: VICE stops it while the monitor is being used.
    pub fn resume(&mut self) -> Result<(), String> {
        self.command(EXIT, &[]).map(|_| ())
    }

    /// Types text into the machine's keyboard buffer. Text is sent as PETSCII, as the C64's keyboard makes it.
    pub fn type_text(&mut self, text: &str) -> Result<(), String> {
        let bytes: Vec<u8> = text.chars().filter_map(|c| match c {
            '\n' => Some(13),
            'a'..='z' => Some(c.to_ascii_uppercase() as u8),
            ' '..='_' => Some(c as u8),
            _ => None,
        }).take(255).collect();
        let mut body = vec![bytes.len() as u8];
        body.extend(bytes);
        self.command(KEYBOARD_FEED, &body).map(|_| ())
    }
}

impl Live for ViceMonitor {
    fn read(&mut self, address: u16, length: usize) -> Result<Vec<u8>, String> {
        if length == 0 {
            return Ok(Vec::new());
        }
        let end = address.wrapping_add(length as u16 - 1);
        let mut body = vec![0u8];
        body.extend_from_slice(&address.to_le_bytes());
        body.extend_from_slice(&end.to_le_bytes());
        body.extend_from_slice(&[0, 0, 0]);
        let response = self.command(MEMORY_GET, &body)?;
        let count = response.get(..2).map_or(0, |b| usize::from(u16::from_le_bytes([b[0], b[1]])));
        let mut bytes = response.get(2..).unwrap_or_default().to_vec();
        // 65536 bytes are said as 0.
        bytes.truncate(if count == 0 { 65536 } else { count });
        Ok(bytes)
    }

    fn write(&mut self, address: u16, bytes: &[u8]) -> Result<(), String> {
        if bytes.is_empty() {
            return Ok(());
        }
        let end = address.wrapping_add(bytes.len() as u16 - 1);
        let mut body = vec![0u8];
        body.extend_from_slice(&address.to_le_bytes());
        body.extend_from_slice(&end.to_le_bytes());
        body.extend_from_slice(&[0, 0, 0]);
        body.extend_from_slice(bytes);
        self.command(MEMORY_SET, &body).map(|_| ())
    }
}

impl Live for crate::zx_machine::BuiltIn {
    fn read(&mut self, address: u16, length: usize) -> Result<Vec<u8>, String> {
        Ok((0..length).map(|i| self.peek(address.wrapping_add(i as u16))).collect())
    }

    fn write(&mut self, address: u16, bytes: &[u8]) -> Result<(), String> {
        self.poke(address, bytes);
        Ok(())
    }
}
