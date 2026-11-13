//! Debugging in two more emulators: XRoar, the Dragon and Color Computer emulator, through its GDB stub (the GDB
//! remote serial protocol over TCP, which XRoar answers for the 6809); and Hatari, the Atari ST emulator, through
//! its own debugger, typed to on its console. Both give where the program stopped and its registers; XRoar gives
//! its memory too, and both stop on writes being watched for.

use super::{Ask, Stop, Told};
use std::io::{Read, Write};
use std::net::TcpStream;
use std::sync::mpsc::{Receiver, Sender, TryRecvError};
use std::time::{Duration, Instant};

// ---------- XRoar: the GDB remote serial protocol ----------

/// A GDB stub, a packet at a time: $data#checksum, each acknowledged with +.
struct Gdb {
    stream: TcpStream,
    /// What's been read but not yet taken as a packet.
    pending: Vec<u8>,
}

impl Gdb {
    fn send(&mut self, data: &str) -> Result<(), String> {
        let sum = data.bytes().fold(0u8, |a, b| a.wrapping_add(b));
        self.stream.write_all(format!("${data}#{sum:02x}").as_bytes()).map_err(|e| format!("XRoar couldn't be told: {e}"))
    }

    /// The next packet, if one comes within `wait`.
    fn packet(&mut self, wait: Duration) -> Result<Option<String>, String> {
        let began = Instant::now();
        let mut buffer = [0u8; 4096];
        loop {
            // Acknowledgements and stray bytes before a packet are passed over.
            if let Some(start) = self.pending.iter().position(|b| *b == b'$') {
                if let Some(hash) = self.pending[start..].iter().position(|b| *b == b'#').map(|h| start + h)
                    && self.pending.len() >= hash + 3
                {
                    let data = String::from_utf8_lossy(&self.pending[start + 1..hash]).into_owned();
                    self.pending.drain(..hash + 3);
                    self.stream.write_all(b"+").map_err(|e| e.to_string())?;
                    return Ok(Some(data));
                }
            } else {
                self.pending.clear();
            }
            if began.elapsed() > wait {
                return Ok(None);
            }
            match self.stream.read(&mut buffer) {
                Ok(0) => return Err("XRoar closed the connection.".into()),
                Ok(got) => self.pending.extend_from_slice(&buffer[..got]),
                Err(e) if matches!(e.kind(), std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut) => {}
                Err(e) => return Err(format!("XRoar stopped answering: {e}")),
            }
        }
    }

    fn command(&mut self, data: &str) -> Result<String, String> {
        self.send(data)?;
        self.packet(Duration::from_secs(5))?.ok_or_else(|| "XRoar stopped answering.".to_string())
    }

    /// The 6809's registers: CC, A, B, DP, X, Y, U, S, PC, as XRoar sends them.
    fn registers(&mut self) -> Result<[u16; 9], String> {
        let said = self.command("g")?;
        let field = |from: usize, len: usize| said.get(from..from + len).and_then(|h| u16::from_str_radix(h, 16).ok()).unwrap_or(0);
        Ok([field(0, 2), field(2, 2), field(4, 2), field(6, 2), field(8, 4), field(12, 4), field(16, 4), field(20, 4), field(24, 4)])
    }

    fn memory(&mut self) -> Result<Vec<u8>, String> {
        let mut memory = Vec::with_capacity(65536);
        for start in (0..0x10000u32).step_by(0x400) {
            let said = self.command(&format!("m{start:x},400"))?;
            let bytes: Vec<u8> = said.as_bytes().chunks(2).map(|pair| u8::from_str_radix(std::str::from_utf8(pair).unwrap_or("00"), 16).unwrap_or(0)).collect();
            memory.extend(bytes);
            memory.resize(start as usize + 0x400, 0);
        }
        Ok(memory)
    }

    fn stop(&mut self) -> Result<Stop, String> {
        let [cc, a, b, dp, x, y, u, s, pc] = self.registers()?;
        let flag_names: String = [(7, 'E'), (6, 'F'), (5, 'H'), (4, 'I'), (3, 'N'), (2, 'Z'), (1, 'V'), (0, 'C')].iter().map(|(bit, name)| if cc & (1 << bit) != 0 { *name } else { '-' }).collect();
        let words = vec![format!("A {a:02X}"), format!("B {b:02X}"), format!("D {:04X}", a << 8 | b), format!("X {x:04X}"), format!("Y {y:04X}"), format!("U {u:04X}"), format!("S {s:04X}"), format!("DP {dp:02X}"), format!("PC {pc:04X}"), format!("CC {flag_names}")];
        let memory = self.memory()?;
        Ok(Stop { pc, z80: None, words, reason: None, memory: Some(std::sync::Arc::new(memory)) })
    }

    /// Replaces one kind of breakpoint (0 for running an instruction, 2 for a write) at these addresses.
    fn set(&mut self, kind: u8, old: &[u16], new: &[u16]) -> Result<(), String> {
        for address in old {
            self.command(&format!("z{kind},{address:x},1"))?;
        }
        for address in new {
            self.command(&format!("Z{kind},{address:x},1"))?;
        }
        Ok(())
    }
}

/// How long the 6809 instruction at the start of `bytes` is, if it's a call (BSR, LBSR or JSR) to step over.
pub fn call_length_6809(bytes: &[u8]) -> Option<u16> {
    match bytes.first()? {
        0x8D | 0x9D => Some(2),
        0x17 | 0xBD => Some(3),
        0xAD => {
            let post = *bytes.get(1)?;
            let extra = if post & 0x80 == 0 {
                0
            } else {
                match post & 0x0F {
                    0x08 | 0x0C => 1,
                    0x09 | 0x0D | 0x0F => 2,
                    _ => 0,
                }
            };
            Some(2 + extra)
        }
        _ => None,
    }
}

/// Waits for a stop reply (S05 and the like), or for being asked to pause or finish.
fn wait_for_stop(gdb: &mut Gdb, asked: &Receiver<Ask>, finish: &mut bool) -> Result<(), String> {
    loop {
        if let Some(reply) = gdb.packet(Duration::from_millis(150))?
            && (reply.starts_with('S') || reply.starts_with('T'))
        {
            return Ok(());
        }
        match asked.try_recv() {
            // ^C stops it.
            Ok(Ask::Pause) => gdb.stream.write_all(&[3]).map_err(|e| e.to_string())?,
            Ok(Ask::Finish) | Err(TryRecvError::Disconnected) => {
                gdb.stream.write_all(&[3]).map_err(|e| e.to_string())?;
                *finish = true;
            }
            Ok(_) | Err(TryRecvError::Empty) => {}
        }
    }
}

/// The XRoar thread: connects (which stops the machine), sets the breakpoints before the program is loaded and
/// run, carries on, and then does as it's asked.
pub fn talk_xroar(port: u16, breakpoints: Vec<u16>, asked: &Receiver<Ask>, tell: &Sender<Told>, wake: &dyn Fn()) -> Result<(), String> {
    let began = Instant::now();
    let stream = loop {
        match TcpStream::connect_timeout(&([127, 0, 0, 1], port).into(), Duration::from_secs(2)) {
            Ok(stream) => break stream,
            Err(_) if began.elapsed() < Duration::from_secs(20) => std::thread::sleep(Duration::from_millis(100)),
            Err(e) => return Err(format!("XRoar couldn't be reached to debug on port {port}: {e}")),
        }
    };
    stream.set_read_timeout(Some(Duration::from_millis(100))).map_err(|e| e.to_string())?;
    let _ = stream.set_nodelay(true);
    let mut gdb = Gdb { stream, pending: Vec::new() };
    let say = |told: Told| {
        let _ = tell.send(told);
        wake();
    };
    // It stops as it's connected to, and says so.
    gdb.command("?")?;
    let mut set = breakpoints;
    gdb.set(0, &[], &set)?;
    let mut watches: Vec<u16> = Vec::new();
    let mut next = Some(Ask::CarryOn);
    let mut finish = false;
    loop {
        let ask = match next.take() {
            Some(ask) => ask,
            None => asked.recv().unwrap_or(Ask::Finish),
        };
        match ask {
            Ask::Breakpoints(addresses) => {
                gdb.set(0, &set, &addresses)?;
                set = addresses;
            }
            Ask::Watches(addresses) => {
                gdb.set(2, &watches, &addresses)?;
                watches = addresses;
            }
            Ask::Pause => {}
            Ask::Step => {
                gdb.send("s")?;
                wait_for_stop(&mut gdb, asked, &mut finish)?;
                say(Told::Stopped(gdb.stop()?));
            }
            Ask::CarryOn | Ask::StepOver => {
                // Over a call: on to the instruction after it, by a breakpoint there for the while.
                let mut after = None;
                if ask == Ask::StepOver {
                    let pc = gdb.registers()?[8];
                    let said = gdb.command(&format!("m{pc:x},4"))?;
                    let bytes: Vec<u8> = said.as_bytes().chunks(2).filter_map(|p| u8::from_str_radix(std::str::from_utf8(p).ok()?, 16).ok()).collect();
                    match call_length_6809(&bytes) {
                        Some(length) => after = Some(pc.wrapping_add(length)),
                        None => {
                            next = Some(Ask::Step);
                            continue;
                        }
                    }
                }
                if let Some(at) = after.filter(|at| !set.contains(at)) {
                    gdb.command(&format!("Z0,{at:x},1"))?;
                }
                gdb.send("c")?;
                say(Told::Running);
                wait_for_stop(&mut gdb, asked, &mut finish)?;
                if let Some(at) = after.filter(|at| !set.contains(at)) {
                    gdb.command(&format!("z0,{at:x},1"))?;
                }
                if finish {
                    next = Some(Ask::Finish);
                } else {
                    say(Told::Stopped(gdb.stop()?));
                }
            }
            Ask::Finish => {
                let _ = gdb.set(0, &set, &[]);
                let _ = gdb.set(2, &watches, &[]);
                let _ = gdb.command("D");
                return Ok(());
            }
        }
    }
}

// ---------- Hatari: its own debugger, on its console ----------

/// Hatari's console: what it prints comes through a thread, line by line, so waiting for its prompt can time out.
pub struct Console {
    input: std::process::ChildStdin,
    lines: Receiver<String>,
}

impl Console {
    /// Its input, and what it prints: Hatari's debugger writes to both its outputs.
    pub fn new(input: std::process::ChildStdin, output: impl Read + Send + 'static, errors: impl Read + Send + 'static) -> Console {
        let (send, lines) = std::sync::mpsc::channel();
        Self::read_lines(errors, send.clone());
        Self::read_lines(output, send);
        Console { input, lines }
    }

    fn read_lines(output: impl Read + Send + 'static, send: Sender<String>) {
        std::thread::spawn(move || {
            let mut reader = std::io::BufReader::new(output);
            let mut line = Vec::new();
            loop {
                line.clear();
                // The prompt, "> ", has no line ending after it, so a read stops at the space after > too.
                let mut byte = [0u8; 1];
                loop {
                    match reader.read(&mut byte) {
                        Ok(0) | Err(_) => return,
                        Ok(_) => {}
                    }
                    if byte[0] == b'\n' {
                        break;
                    }
                    line.push(byte[0]);
                    if line == b"> " {
                        break;
                    }
                }
                if send.send(String::from_utf8_lossy(&line).trim_end_matches('\r').to_string()).is_err() {
                    return;
                }
            }
        });
    }

    fn type_line(&mut self, command: &str) -> Result<(), String> {
        self.input.write_all(format!("{command}\n").as_bytes()).and_then(|()| self.input.flush()).map_err(|e| format!("Hatari couldn't be told: {e}"))
    }

    /// What it printed until its prompt, if the prompt comes within `wait`.
    fn until_prompt(&mut self, wait: Duration) -> Result<Option<Vec<String>>, String> {
        let began = Instant::now();
        let mut said = Vec::new();
        loop {
            let left = wait.saturating_sub(began.elapsed());
            match self.lines.recv_timeout(left.min(Duration::from_millis(150))) {
                Ok(line) if line == "> " => return Ok(Some(said)),
                Ok(line) => said.push(line),
                Err(std::sync::mpsc::RecvTimeoutError::Timeout) if began.elapsed() < wait => {}
                Err(std::sync::mpsc::RecvTimeoutError::Timeout) => return Ok(None),
                Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => return Err("Hatari has stopped.".into()),
            }
        }
    }

    /// Types a command, and takes what it printed: up to the prompt, and, as its two outputs can arrive in
    /// either order, whatever comes in the quiet moment after.
    fn command(&mut self, command: &str) -> Result<Vec<String>, String> {
        self.type_line(command)?;
        let mut said = self.until_prompt(Duration::from_secs(10))?.ok_or_else(|| "Hatari stopped answering.".to_string())?;
        while let Ok(line) = self.lines.recv_timeout(Duration::from_millis(120)) {
            if line != "> " {
                said.push(line);
            }
        }
        Ok(said)
    }

    /// Where it is, and its registers.
    fn stop(&mut self, text: u32) -> Result<Stop, String> {
        let pc = hatari_value(&self.command("e pc")?).ok_or("Hatari didn't say where it is.")?;
        let mut stop = hatari_stop(&self.command("r")?, text);
        stop.pc = pc.wrapping_sub(text) as u16;
        stop.words.retain(|w| !w.starts_with("PC "));
        stop.words.push(format!("PC {pc:06X}"));
        Ok(stop)
    }
}

/// The value of an expression, as Hatari's e command prints it: "= %... (hex) / $1234 (...)" in some form; the
/// first $ number is taken.
fn hatari_value(said: &[String]) -> Option<u32> {
    said.iter().filter(|line| line.starts_with('=')).find_map(|line| {
        let at = line.find('$')?;
        let hex: String = line[at + 1..].chars().take_while(char::is_ascii_hexdigit).collect();
        u32::from_str_radix(&hex, 16).ok()
    })
}

/// The 68000's registers and PC as r prints them.
fn hatari_stop(said: &[String], text: u32) -> Stop {
    let mut words = Vec::new();
    let mut pc = 0u32;
    for line in said {
        let trimmed = line.trim();
        if trimmed.starts_with('D') || trimmed.starts_with('A') {
            words.extend(trimmed.split("   ").map(|w| w.trim().to_string()).filter(|w| !w.is_empty()));
        } else if let Some(rest) = trimmed.strip_prefix("Next PC:") {
            let _ = rest;
        } else if trimmed.len() > 9 && trimmed.chars().take(8).all(|c| c.is_ascii_hexdigit()) && pc == 0 {
            pc = u32::from_str_radix(&trimmed[..8], 16).unwrap_or(0);
        }
        if let Some(flags) = trimmed.strip_prefix("T=") {
            words.push(format!("T={flags}"));
        }
    }
    words.push(format!("PC {pc:06X}"));
    // Lines are found by where they are in the program, so the stop is told as that.
    Stop { pc: pc.wrapping_sub(text) as u16, z80: None, words, reason: None, memory: None }
}

/// The Hatari thread: waits for the program to start (its first instruction, when TEXT is in RAM), then sets
/// the breakpoints where the program is, and carries on.
pub fn talk_hatari(mut console: Console, child_id: u32, breakpoints: Vec<u16>, asked: &Receiver<Ask>, tell: &Sender<Told>, wake: &dyn Fn()) -> Result<(), String> {
    let say = |told: Told| {
        let _ = tell.send(told);
        wake();
    };
    say(Told::Running);
    // Until the start of the program, when the debugger takes over.
    loop {
        match console.until_prompt(Duration::from_millis(200))? {
            Some(said) if said.iter().any(|l| l.contains("entered debug mode")) => break,
            Some(_) => {}
            None => {}
        }
        if matches!(asked.try_recv(), Ok(Ask::Finish) | Err(TryRecvError::Disconnected)) {
            return Ok(());
        }
    }
    let text = hatari_value(&console.command("e TEXT")?).ok_or("Hatari didn't say where the program is.")?;
    let set_all = |console: &mut Console, breakpoints: &[u16], watches: &[u16]| -> Result<(), String> {
        console.command("b all")?;
        for offset in breakpoints {
            console.command(&format!("b pc = ${:x}", text + u32::from(*offset)))?;
        }
        for address in watches {
            // A byte that changes: Hatari's own way of watching memory.
            let at = text + u32::from(*address);
            console.command(&format!("b (${at:x}).b ! (${at:x}).b"))?;
        }
        Ok(())
    };
    let (mut set, mut watches) = (breakpoints, Vec::new());
    set_all(&mut console, &set, &watches)?;
    let mut next = Some(Ask::CarryOn);
    loop {
        let ask = match next.take() {
            Some(ask) => ask,
            None => asked.recv().unwrap_or(Ask::Finish),
        };
        match ask {
            Ask::Breakpoints(addresses) => {
                set = addresses;
                set_all(&mut console, &set, &watches)?;
            }
            Ask::Watches(addresses) => {
                watches = addresses;
                set_all(&mut console, &set, &watches)?;
            }
            Ask::Pause => {}
            Ask::Step | Ask::StepOver => {
                console.command(if ask == Ask::Step { "s" } else { "n" })?;
                say(Told::Stopped(console.stop(text)?));
            }
            Ask::CarryOn => {
                console.type_line("c")?;
                say(Told::Running);
                loop {
                    if let Some(said) = console.until_prompt(Duration::from_millis(200))?
                        && said.iter().any(|l| l.contains("entered debug mode") || l.contains("breakpoint"))
                    {
                        break;
                    }
                    match asked.try_recv() {
                        Ok(Ask::Pause) => interrupt(child_id),
                        Ok(Ask::Finish) | Err(TryRecvError::Disconnected) => return Ok(()),
                        Ok(_) | Err(TryRecvError::Empty) => {}
                    }
                }
                // What the stop printed after its prompt, let go of.
                while console.lines.recv_timeout(Duration::from_millis(120)).is_ok() {}
                say(Told::Stopped(console.stop(text)?));
            }
            Ask::Finish => {
                let _ = console.command("b all");
                let _ = console.type_line("c");
                return Ok(());
            }
        }
    }
}

/// Ctrl+C to Hatari, which takes it into its debugger.
fn interrupt(child_id: u32) {
    #[cfg(unix)]
    {
        let _ = std::process::Command::new("kill").args(["-INT", &child_id.to_string()]).status();
    }
    #[cfg(not(unix))]
    let _ = child_id;
}

// ---------- A real Next, through dezogif ----------

/// Anything dezogif can be reached over: a serial port or a socket.
pub trait Link: Read + Write + Send {}
impl<T: Read + Write + Send> Link for T {}

/// The cable's serial port, or host:port for dezogif over Wi-Fi.
pub fn open_next(port: &str) -> Result<Box<dyn Link>, String> {
    if let Some((host, number)) = port.rsplit_once(':').filter(|(host, number)| !host.is_empty() && number.parse::<u16>().is_ok() && !host.starts_with('/') && !host.contains('\\')) {
        let address = std::net::ToSocketAddrs::to_socket_addrs(&(host, number.parse::<u16>().unwrap_or(0))).map_err(|e| e.to_string())?.next().ok_or("That address wasn't found.")?;
        let stream = TcpStream::connect_timeout(&address, Duration::from_secs(5)).map_err(|e| format!("The Next couldn't be reached at {port}: {e}"))?;
        stream.set_read_timeout(Some(Duration::from_millis(100))).map_err(|e| e.to_string())?;
        return Ok(Box::new(stream));
    }
    let serial = serialport::new(port, 921_600).timeout(Duration::from_millis(100)).open().map_err(|e| format!("The serial port {port} couldn't be opened: {e}"))?;
    Ok(Box::new(serial))
}

/// What's loaded onto the Next before it runs: the 64K, which 16K quarters have code, and where it starts.
pub struct NextProgram {
    pub image: Vec<u8>,
    pub used: [bool; 4],
    pub pc: u16,
    pub sp: u16,
}

/// The thread for a real Next: says hello to dezogif, loads the program into its banks, and debugs it.
pub fn talk_next(link: Box<dyn Link>, program: NextProgram, breakpoints: Vec<u16>, asked: &Receiver<Ask>, tell: &Sender<Told>, wake: &dyn Fn()) -> Result<(), String> {
    use crate::dzrp::{Dzrp, Message, NTF_PAUSE};
    let say = |told: Told| {
        let _ = tell.send(told);
        wake();
    };
    let mut dzrp = Dzrp::new(link);
    dzrp.init()?;
    crate::dzrp::load_program(&mut dzrp, &program.image, &program.used, program.pc, program.sp)?;
    let stop = |dzrp: &mut Dzrp<Box<dyn Link>>, paused: &[u8]| -> Result<Stop, String> {
        let mut stop = Stop::z80(dzrp.registers()?);
        stop.reason = crate::dzrp::pause_reason(paused);
        stop.memory = Some(std::sync::Arc::new(dzrp.memory()?));
        Ok(stop)
    };
    let mut set = breakpoints;
    let mut next = Some(Ask::CarryOn);
    loop {
        let ask = match next.take() {
            Some(ask) => ask,
            None => asked.recv().unwrap_or(Ask::Finish),
        };
        match ask {
            // Breakpoints go in as it carries on; dezogif can't change them while it runs.
            Ask::Breakpoints(addresses) => set = addresses,
            Ask::Watches(_) | Ask::Pause => {}
            Ask::CarryOn | Ask::Step | Ask::StepOver => {
                let (one, two, originals) = if ask == Ask::CarryOn {
                    (None, None, dzrp.set_breakpoints(&set)?)
                } else {
                    let r = dzrp.registers()?;
                    let memory = dzrp.memory()?;
                    let (after, jump) = crate::dzrp::z80_next(&|a| memory[a as usize], &r, ask == Ask::StepOver);
                    (Some(after), jump, Vec::new())
                };
                let placed: Vec<u16> = if ask == Ask::CarryOn { set.clone() } else { Vec::new() };
                dzrp.carry_on(one, two)?;
                say(Told::Running);
                let paused = loop {
                    if let Some(paused) = dzrp.paused.take() {
                        break paused;
                    }
                    if let Some(Message::Notification { id: NTF_PAUSE, payload }) = dzrp.receive(Duration::from_millis(150))? {
                        break payload;
                    }
                    match asked.try_recv() {
                        Ok(Ask::Pause) => dzrp.pause()?,
                        Ok(Ask::Finish) | Err(TryRecvError::Disconnected) => {
                            dzrp.pause()?;
                            next = Some(Ask::Finish);
                        }
                        Ok(Ask::Breakpoints(addresses)) => set = addresses,
                        Ok(_) | Err(TryRecvError::Empty) => {}
                    }
                };
                dzrp.restore(&placed, &originals)?;
                if next.is_none() {
                    say(Told::Stopped(stop(&mut dzrp, &paused)?));
                }
            }
            Ask::Finish => {
                dzrp.close();
                return Ok(());
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn calls_are_known_to_step_over() {
        assert_eq!(call_length_6809(&[0x8D, 0x10]), Some(2), "BSR");
        assert_eq!(call_length_6809(&[0x17, 0x01, 0x00]), Some(3), "LBSR");
        assert_eq!(call_length_6809(&[0xBD, 0x30, 0x00]), Some(3), "JSR extended");
        assert_eq!(call_length_6809(&[0xAD, 0x84]), Some(2), "JSR ,X");
        assert_eq!(call_length_6809(&[0xAD, 0x89, 0x01, 0x00]), Some(4), "JSR 256,X");
        assert_eq!(call_length_6809(&[0x86, 0x07]), None, "LDA isn't a call");
    }

    #[test]
    fn hatari_s_registers_are_read() {
        let said: Vec<String> = ["r", "  D0 0000002C   D1 00000000   D2 00000000   D3 00000000 ", "  A0 00000000   A1 00000000   A2 00000000   A3 00000000 ", "T=00 S=0 M=0 X=0 N=0 Z=0 V=0 C=0 IMASK=3 STP=0", "00E1E80C 2a4f                     MOVEA.L A7,A5", "Next PC: 00e1e80e"].iter().map(|s| s.to_string()).collect();
        let stop = hatari_stop(&said, 0xE1E800);
        assert_eq!(stop.pc, 0x0C, "where it is in the program");
        assert!(stop.words.contains(&"D0 0000002C".to_string()), "{:?}", stop.words);
        assert!(stop.words.contains(&"PC E1E80C".to_string()));
        assert_eq!(hatari_value(&["e TEXT".into(), "= %1 (#123), $7b (hex)".into()]), Some(0x7B));
    }
}
