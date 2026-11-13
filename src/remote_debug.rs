//! Debugging a program running in an emulator, as the built-in Spectrum is debugged: breakpoints at source lines,
//! stopping, stepping and the registers. ZEsarUX is driven through its remote protocol (ZRCP) in step mode, its
//! `run` command carrying on until a breakpoint fires; VICE through its binary monitor's checkpoints. A thread of
//! its own talks to the emulator, so the window never waits.

use crate::zx_machine::Registers;
use std::io::{Read, Write};
use std::net::TcpStream;
use std::sync::mpsc::{Receiver, Sender, TryRecvError, channel};
use std::time::{Duration, Instant};

/// What the window asks of the debugger.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Ask {
    /// The addresses to stop at; set as soon as it can, carrying on afterwards if it was running.
    Breakpoints(Vec<u16>),
    /// The addresses whose writing stops it.
    Watches(Vec<u16>),
    CarryOn,
    Step,
    StepOver,
    Pause,
    /// Lets ZEsarUX run on by itself, and ends.
    Finish,
}

/// Where a program stopped, and its registers: a Z80's, or another processor's as words to show.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Stop {
    pub pc: u16,
    pub z80: Option<Registers>,
    pub words: Vec<String>,
    /// Why, when it's more than reaching a breakpoint: a write it was watching for.
    pub reason: Option<String>,
    /// All 64K as they were when it stopped, where the emulator gives it.
    pub memory: Option<std::sync::Arc<Vec<u8>>>,
}

impl Stop {
    pub fn z80(registers: Registers) -> Stop {
        Stop { pc: registers.pc, z80: Some(registers), ..Stop::default() }
    }
}

/// Which emulator, and so which way of talking to it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Emulator {
    Zesarux,
    Vice,
    /// XRoar, through its GDB stub.
    Xroar,
}

#[path = "remote_debug_more.rs"]
mod more;
pub use more::Console;

/// What the debugger tells the window.
#[derive(Clone, Debug, PartialEq)]
pub enum Told {
    Stopped(Stop),
    Running,
    Problem(String),
}

/// The window's end of a debugger.
pub struct RemoteDebug {
    asks: Sender<Ask>,
    told: Receiver<Told>,
    /// Where it last stopped, while it's stopped.
    pub stopped: Option<Stop>,
    pub problem: Option<String>,
}

impl RemoteDebug {
    /// Starts debugging the ZEsarUX listening on a port: once connected, it's stopped, given its breakpoints, and
    /// then loads the file (if there's one to load) and runs it, so not even the first instruction is missed.
    pub fn start(emulator: Emulator, port: u16, file: Option<std::path::PathBuf>, breakpoints: Vec<u16>, wake: impl Fn() + Send + 'static) -> RemoteDebug {
        let (asks, asked) = channel();
        let (tell, told) = channel();
        std::thread::spawn(move || {
            let talked = match emulator {
                Emulator::Zesarux => talk(port, file, breakpoints, &asked, &tell, &wake),
                Emulator::Vice => talk_vice(port, file, breakpoints, &asked, &tell, &wake),
                Emulator::Xroar => more::talk_xroar(port, breakpoints, &asked, &tell, &wake),
            };
            if let Err(problem) = talked {
                let _ = tell.send(Told::Problem(problem));
            }
            wake();
        });
        RemoteDebug { asks, told, stopped: None, problem: None }
    }

    /// Starts debugging a program in Hatari, through its debugger on the console it was started with.
    pub fn start_hatari(console: Console, child_id: u32, breakpoints: Vec<u16>, wake: impl Fn() + Send + 'static) -> RemoteDebug {
        let (asks, asked) = channel();
        let (tell, told) = channel();
        std::thread::spawn(move || {
            if let Err(problem) = more::talk_hatari(console, child_id, breakpoints, &asked, &tell, &wake) {
                let _ = tell.send(Told::Problem(problem));
            }
            wake();
        });
        RemoteDebug { asks, told, stopped: None, problem: None }
    }

    pub fn ask(&mut self, ask: Ask) {
        if matches!(ask, Ask::CarryOn | Ask::Step | Ask::StepOver) {
            self.stopped = None;
        }
        let _ = self.asks.send(ask);
    }

    /// Takes in what the debugger has said since last time; true if it's newly stopped.
    pub fn check(&mut self) -> bool {
        let mut stopped = false;
        while let Ok(told) = self.told.try_recv() {
            match told {
                Told::Stopped(registers) => {
                    self.stopped = Some(registers);
                    stopped = true;
                }
                Told::Running => self.stopped = None,
                Told::Problem(problem) => self.problem = Some(problem),
            }
        }
        stopped && self.stopped.is_some()
    }
}

impl Drop for RemoteDebug {
    fn drop(&mut self) {
        let _ = self.asks.send(Ask::Finish);
    }
}

/// The registers as get-registers gives them: "PC=800b SP=5d58 AF=0754 ... AF'=0044 ... I=3f R=01 ... IM1 IFF--".
pub fn parse_registers(text: &str) -> Option<Registers> {
    let mut r = Registers::default();
    let mut found_pc = false;
    for word in text.split_whitespace() {
        let Some((name, value)) = word.split_once('=') else {
            if let Some(mode) = word.strip_prefix("IM") {
                r.im = mode.parse().unwrap_or(0);
            }
            if let Some(flags) = word.strip_prefix("IFF") {
                r.iff = flags.starts_with('1');
            }
            continue;
        };
        let Ok(v) = u16::from_str_radix(value, 16) else { continue };
        match name {
            "PC" => {
                r.pc = v;
                found_pc = true;
            }
            "SP" => r.sp = v,
            "AF" => r.af = v,
            "BC" => r.bc = v,
            "DE" => r.de = v,
            "HL" => r.hl = v,
            "IX" => r.ix = v,
            "IY" => r.iy = v,
            "AF'" => r.af2 = v,
            "BC'" => r.bc2 = v,
            "DE'" => r.de2 = v,
            "HL'" => r.hl2 = v,
            "I" => r.i = v as u8,
            "R" => r.r = v as u8,
            _ => {}
        }
    }
    found_pc.then_some(r)
}

struct Link {
    stream: TcpStream,
}

impl Link {
    /// Waits for the prompt, whichever it is, giving up after `wait`; what came before it.
    fn until_prompt(&mut self, wait: Duration) -> Result<Option<String>, String> {
        let mut said = Vec::new();
        let mut buffer = [0u8; 8192];
        let began = Instant::now();
        loop {
            for prompt in [&b"command@cpu-step> "[..], &b"command> "[..]] {
                if said.ends_with(prompt) {
                    said.truncate(said.len() - prompt.len());
                    return Ok(Some(String::from_utf8_lossy(&said).trim().to_string()));
                }
            }
            if began.elapsed() > wait {
                return Ok(None);
            }
            match self.stream.read(&mut buffer) {
                Ok(0) => return Err("ZEsarUX closed the connection.".into()),
                Ok(got) => said.extend_from_slice(&buffer[..got]),
                Err(e) if matches!(e.kind(), std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut) => {}
                Err(e) => return Err(format!("ZEsarUX stopped answering: {e}")),
            }
        }
    }

    fn send(&mut self, command: &str) -> Result<(), String> {
        self.stream.write_all(format!("{command}\n").as_bytes()).map_err(|e| format!("ZEsarUX couldn't be told: {e}"))
    }

    fn command(&mut self, command: &str) -> Result<String, String> {
        self.send(command)?;
        let said = self.until_prompt(Duration::from_secs(5))?.ok_or("ZEsarUX stopped answering.")?;
        if said.starts_with("Error") {
            return Err(format!("ZEsarUX said: {said}"));
        }
        Ok(said)
    }

    fn registers(&mut self) -> Result<Registers, String> {
        let said = self.command("get-registers")?;
        parse_registers(&said).ok_or_else(|| format!("ZEsarUX's registers weren't understood: {said}"))
    }

    /// Where it stopped, with its registers and all 64K.
    fn stop(&mut self) -> Result<Stop, String> {
        let mut stop = Stop::z80(self.registers()?);
        let said = self.command("read-memory 0 65536")?;
        let digits: Vec<u8> = said.bytes().filter(u8::is_ascii_hexdigit).collect();
        let mut memory: Vec<u8> = digits.as_chunks::<2>().0.iter().map(|pair| u8::from_str_radix(std::str::from_utf8(pair).unwrap_or("00"), 16).unwrap_or(0)).collect();
        memory.resize(65536, 0);
        stop.memory = Some(std::sync::Arc::new(memory));
        Ok(stop)
    }

    /// Stops it when it writes to these addresses.
    fn set_watches(&mut self, addresses: &[u16]) -> Result<(), String> {
        self.command("clear-membreakpoints")?;
        for address in addresses {
            self.command(&format!("set-membreakpoint {address} 2"))?;
        }
        Ok(())
    }

    fn set_breakpoints(&mut self, addresses: &[u16], before: usize) -> Result<usize, String> {
        for (i, address) in addresses.iter().enumerate() {
            self.command(&format!("set-breakpoint {} PC={address:04X}H", i + 1))?;
        }
        // Those left over from before are cleared.
        for i in addresses.len()..before {
            self.command(&format!("set-breakpoint {} ", i + 1))?;
        }
        Ok(addresses.len())
    }
}

/// The thread: connects, stops the machine, and then does as it's asked.
fn talk(port: u16, file: Option<std::path::PathBuf>, breakpoints: Vec<u16>, asked: &Receiver<Ask>, tell: &Sender<Told>, wake: &dyn Fn()) -> Result<(), String> {
    // ZEsarUX may still be starting.
    let began = Instant::now();
    let stream = loop {
        match TcpStream::connect_timeout(&([127, 0, 0, 1], port).into(), Duration::from_secs(2)) {
            Ok(stream) => break stream,
            Err(_) if began.elapsed() < Duration::from_secs(20) => std::thread::sleep(Duration::from_millis(200)),
            Err(e) => return Err(format!("ZEsarUX couldn't be reached to debug on port {port}: {e}")),
        }
    };
    stream.set_read_timeout(Some(Duration::from_millis(100))).map_err(|e| e.to_string())?;
    let mut link = Link { stream };
    link.until_prompt(Duration::from_secs(5))?;
    link.command("enter-cpu-step")?;
    link.command("enable-breakpoints")?;
    let mut set = link.set_breakpoints(&breakpoints, 0)?;
    if let Some(file) = file {
        link.command(&format!("smartload {}", file.display()))?;
    }
    let say = |told: Told| {
        let _ = tell.send(told);
        wake();
    };
    // It carries on at once, to its first breakpoint.
    let mut next = Some(Ask::CarryOn);
    loop {
        let ask = match next.take() {
            Some(ask) => ask,
            None => match asked.recv() {
                Ok(ask) => ask,
                Err(_) => Ask::Finish,
            },
        };
        match ask {
            Ask::Breakpoints(addresses) => set = link.set_breakpoints(&addresses, set)?,
            Ask::Watches(addresses) => link.set_watches(&addresses)?,
            Ask::Pause => say(Told::Stopped(link.stop()?)),
            Ask::Step => {
                link.command("cpu-step")?;
                say(Told::Stopped(link.stop()?));
            }
            Ask::CarryOn | Ask::StepOver => {
                link.send(if ask == Ask::CarryOn { "run" } else { "cpu-step-over" })?;
                say(Told::Running);
                // Until it stops by itself, or is asked to stop: any data sent ends ZEsarUX's run.
                loop {
                    if link.until_prompt(Duration::from_millis(150))?.is_some() {
                        break;
                    }
                    match asked.try_recv() {
                        Ok(Ask::Pause) => link.send("")?,
                        Ok(Ask::Finish) | Err(TryRecvError::Disconnected) => {
                            link.send("")?;
                            next = Some(Ask::Finish);
                        }
                        Ok(Ask::Breakpoints(addresses)) => {
                            // They can only be set while it's stopped: stop, set them, and carry on.
                            link.send("")?;
                            link.until_prompt(Duration::from_secs(5))?;
                            set = link.set_breakpoints(&addresses, set)?;
                            link.send("run")?;
                        }
                        Ok(Ask::Watches(addresses)) => {
                            link.send("")?;
                            link.until_prompt(Duration::from_secs(5))?;
                            link.set_watches(&addresses)?;
                            link.send("run")?;
                        }
                        Ok(_) | Err(TryRecvError::Empty) => {}
                    }
                }
                if next.is_none() {
                    say(Told::Stopped(link.stop()?));
                }
            }
            Ask::Finish => {
                let _ = link.command("disable-breakpoints");
                let _ = link.command("exit-cpu-step");
                return Ok(());
            }
        }
    }
}

// ---------- VICE ----------

/// VICE's binary monitor, read and written a message at a time.
struct Monitor {
    stream: TcpStream,
    request: u32,
    /// The registers VICE last said, with the stop that came after them.
    registers: Vec<(u8, u16)>,
}

/// What VICE sent: its type, error, request and body.
type Message = (u8, u8, u32, Vec<u8>);

impl Monitor {
    fn send(&mut self, command: u8, body: &[u8]) -> Result<u32, String> {
        self.request += 1;
        let mut message = vec![0x02, 0x02];
        message.extend_from_slice(&(body.len() as u32).to_le_bytes());
        message.extend_from_slice(&self.request.to_le_bytes());
        message.push(command);
        message.extend_from_slice(body);
        self.stream.write_all(&message).map_err(|e| format!("VICE couldn't be told: {e}"))?;
        Ok(self.request)
    }

    /// The next message, if one comes within the read timeout.
    fn next(&mut self) -> Result<Option<Message>, String> {
        let mut head = [0u8; 12];
        match self.stream.read(&mut head[..1]) {
            Ok(0) => return Err("VICE closed the connection.".into()),
            Ok(_) => {}
            Err(e) if matches!(e.kind(), std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut) => return Ok(None),
            Err(e) => return Err(format!("VICE stopped answering: {e}")),
        }
        // The rest of a message comes at once.
        self.stream.set_read_timeout(Some(Duration::from_secs(5))).map_err(|e| e.to_string())?;
        let rest = (|| -> std::io::Result<Message> {
            self.stream.read_exact(&mut head[1..])?;
            let length = u32::from_le_bytes([head[2], head[3], head[4], head[5]]) as usize;
            let mut body = vec![0u8; length];
            self.stream.read_exact(&mut body)?;
            Ok((head[6], head[7], u32::from_le_bytes([head[8], head[9], head[10], head[11]]), body))
        })();
        self.stream.set_read_timeout(Some(Duration::from_millis(100))).map_err(|e| e.to_string())?;
        let message = rest.map_err(|e| format!("VICE stopped answering: {e}"))?;
        // Registers come before a stop; keep them for it.
        if message.0 == 0x31 {
            let body = &message.3;
            let count = body.get(..2).map_or(0, |b| u16::from_le_bytes([b[0], b[1]]) as usize);
            let mut at = 2;
            self.registers.clear();
            for _ in 0..count {
                let Some(&size) = body.get(at) else { break };
                if let (Some(&id), Some(value)) = (body.get(at + 1), body.get(at + 2..at + 4)) {
                    self.registers.push((id, u16::from_le_bytes([value[0], value[1]])));
                }
                at += size as usize + 1;
            }
        }
        Ok(Some(message))
    }

    /// Sends a command and waits for its answer, noting a stop on the way.
    fn command(&mut self, command: u8, body: &[u8]) -> Result<Vec<u8>, String> {
        let id = self.send(command, body)?;
        let began = Instant::now();
        while began.elapsed() < Duration::from_secs(5) {
            if let Some((_, error, answered, body)) = self.next()?
                && answered == id
            {
                if error != 0 {
                    return Err(format!("VICE refused it (error {error:#04X})."));
                }
                return Ok(body);
            }
        }
        Err("VICE stopped answering.".into())
    }

    /// Where it stopped, with the 6502's registers in words.
    fn stop(&self, pc: u16) -> Stop {
        let get = |id: u8| self.registers.iter().find(|(i, _)| *i == id).map(|(_, v)| *v);
        let flags = get(5).unwrap_or(0) as u8;
        let flag_names: String = [(7, 'N'), (6, 'V'), (4, 'B'), (3, 'D'), (2, 'I'), (1, 'Z'), (0, 'C')].iter().map(|(bit, name)| if flags & (1 << bit) != 0 { *name } else { '-' }).collect();
        let words = vec![
            format!("A {:02X}", get(0).unwrap_or(0)),
            format!("X {:02X}", get(1).unwrap_or(0)),
            format!("Y {:02X}", get(2).unwrap_or(0)),
            format!("SP {:02X}", get(4).unwrap_or(0)),
            format!("PC {pc:04X}"),
            format!("Flags {flag_names}"),
        ];
        Stop { pc, z80: None, words, ..Stop::default() }
    }

    /// All 64K, in two halves, as one answer's length can't say 65536.
    fn memory(&mut self) -> Result<Vec<u8>, String> {
        let mut memory = Vec::with_capacity(65536);
        for (start, end) in [(0u16, 0x7FFFu16), (0x8000, 0xFFFF)] {
            let mut body = vec![0];
            body.extend(start.to_le_bytes());
            body.extend(end.to_le_bytes());
            body.extend([0, 0, 0]);
            let answer = self.command(0x01, &body)?;
            memory.extend(answer.get(2..).unwrap_or(&[]));
            memory.resize(usize::from(end) + 1, 0);
        }
        Ok(memory)
    }

    /// Where it stopped, with the memory.
    fn stop_with_memory(&mut self, pc: u16) -> Result<Stop, String> {
        let mut stop = self.stop(pc);
        stop.memory = Some(std::sync::Arc::new(self.memory()?));
        Ok(stop)
    }

    /// `op` is 4 to stop on running an instruction there, 2 on a write.
    fn set_checkpoints(&mut self, addresses: &[u16], numbers: &mut Vec<u32>) -> Result<(), String> {
        self.set_checkpoints_for(addresses, numbers, 4)
    }

    fn set_checkpoints_for(&mut self, addresses: &[u16], numbers: &mut Vec<u32>, op: u8) -> Result<(), String> {
        for number in numbers.drain(..) {
            let _ = self.command(0x13, &number.to_le_bytes());
        }
        for address in addresses {
            let mut body = address.to_le_bytes().to_vec();
            body.extend(address.to_le_bytes());
            // Stop when hit, enabled, on running the instruction there (or a write), not temporary.
            body.extend([1, 1, op, 0]);
            let answer = self.command(0x12, &body)?;
            if let Some(number) = answer.get(..4) {
                numbers.push(u32::from_le_bytes([number[0], number[1], number[2], number[3]]));
            }
        }
        Ok(())
    }
}

/// The VICE thread: connects, sets the checkpoints, starts the program, and then does as it's asked.
fn talk_vice(port: u16, file: Option<std::path::PathBuf>, breakpoints: Vec<u16>, asked: &Receiver<Ask>, tell: &Sender<Told>, wake: &dyn Fn()) -> Result<(), String> {
    let began = Instant::now();
    let stream = loop {
        match TcpStream::connect_timeout(&([127, 0, 0, 1], port).into(), Duration::from_secs(2)) {
            Ok(stream) => break stream,
            Err(_) if began.elapsed() < Duration::from_secs(30) => std::thread::sleep(Duration::from_millis(200)),
            Err(e) => return Err(format!("VICE couldn't be reached to debug on port {port}: {e}")),
        }
    };
    stream.set_read_timeout(Some(Duration::from_millis(100))).map_err(|e| e.to_string())?;
    let mut vice = Monitor { stream, request: 0, registers: Vec::new() };
    let say = |told: Told| {
        let _ = tell.send(told);
        wake();
    };
    let mut numbers = Vec::new();
    let mut watch_numbers = Vec::new();
    vice.set_checkpoints(&breakpoints, &mut numbers)?;
    // AUTOSTART: load the program, and RUN it; VICE carries on by itself.
    let mut running = false;
    if let Some(file) = file {
        let name = file.display().to_string().into_bytes();
        let mut body = vec![1, 0, 0, name.len() as u8];
        body.extend(name);
        vice.command(0xDD, &body)?;
        running = true;
        say(Told::Running);
    }
    loop {
        if running {
            // Until it stops: at a checkpoint, or asked to.
            match vice.next()? {
                Some((0x62, _, _, body)) => {
                    running = false;
                    let pc = body.get(..2).map_or(0, |b| u16::from_le_bytes([b[0], b[1]]));
                    say(Told::Stopped(vice.stop_with_memory(pc)?));
                }
                Some(_) => {}
                None => match asked.try_recv() {
                    // Any command stops it.
                    Ok(Ask::Pause) => {
                        vice.send(0x81, &[])?;
                    }
                    Ok(Ask::Breakpoints(addresses)) => {
                        vice.set_checkpoints(&addresses, &mut numbers)?;
                        vice.command(0xAA, &[])?;
                    }
                    Ok(Ask::Watches(addresses)) => {
                        vice.set_checkpoints_for(&addresses, &mut watch_numbers, 2)?;
                        vice.command(0xAA, &[])?;
                    }
                    Ok(Ask::Finish) | Err(TryRecvError::Disconnected) => break,
                    Ok(_) | Err(TryRecvError::Empty) => {}
                },
            }
            continue;
        }
        let ask = asked.recv().unwrap_or(Ask::Finish);
        match ask {
            Ask::Breakpoints(addresses) => vice.set_checkpoints(&addresses, &mut numbers)?,
            Ask::Watches(addresses) => vice.set_checkpoints_for(&addresses, &mut watch_numbers, 2)?,
            Ask::CarryOn => {
                vice.command(0xAA, &[])?;
                running = true;
                say(Told::Running);
            }
            Ask::Step | Ask::StepOver => {
                // ADVANCE_INSTRUCTIONS: over subroutines or not, one instruction.
                vice.command(0x71, &[(ask == Ask::StepOver) as u8, 1, 0])?;
                running = true;
            }
            Ask::Pause => {}
            Ask::Finish => break,
        }
    }
    let mut leftover = Vec::new();
    std::mem::swap(&mut leftover, &mut numbers);
    let _ = vice.set_checkpoints(&[], &mut leftover);
    let _ = vice.set_checkpoints(&[], &mut watch_numbers);
    let _ = vice.command(0xAA, &[]);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn zesarux_registers_are_read() {
        let r = parse_registers("PC=800b SP=5d58 AF=0754 BC=1234 HL=abcd DE=5678 IX=ff3c IY=5c3a AF'=0044 BC'=0000 HL'=2758 DE'=369b I=3f R=01  F=-Z-H-P-- F'=-Z---P-- MEMPTR=800b IM1 IFF-- VPS: 0").unwrap();
        assert_eq!(r, Registers { pc: 0x800B, sp: 0x5D58, af: 0x0754, bc: 0x1234, hl: 0xABCD, de: 0x5678, ix: 0xFF3C, iy: 0x5C3A, af2: 0x0044, bc2: 0, hl2: 0x2758, de2: 0x369B, i: 0x3F, r: 1, im: 1, iff: false });
        assert!(parse_registers("Error").is_none());
    }

    /// With LIMONITER_ZESARUX naming the program: a program stops at a breakpoint, steps, and carries on to the
    /// next, in a real ZEsarUX.
    #[test]
    #[ignore]
    fn a_program_is_debugged_in_zesarux() {
        let Ok(program) = std::env::var("LIMONITER_ZESARUX") else { return };
        let assembly = crate::asm::assemble(&crate::asm::z80::Z80, std::path::Path::new("t.asm"), "  org 32768\nstart: ld a,7\n  ld bc,$1234\n  call sub\n  ld de,$5678\nloop: jr loop\nsub: ld hl,$abcd\n  ret\n", &|_| Err("no".into()));
        let tape = crate::asm::output::spectrum_tape(&assembly, "t").unwrap();
        // As Limoniter hands it over: loaded from its tape, and a snapshot taken as it starts.
        let run = crate::running::Runnable { machine: crate::running::Machine::Spectrum48, format: crate::running::Format::Tap, name: "t.tap".into(), bytes: std::sync::Arc::new(tape) };
        let snapshot = crate::zx_machine::BuiltIn::snapshot_at(&run, 0x8000).unwrap();
        let folder = std::env::temp_dir().join(format!("limoniter-zdebug-{}", std::process::id()));
        std::fs::create_dir_all(&folder).unwrap();
        let file = folder.join("t.sna");
        std::fs::write(&file, snapshot).unwrap();
        let port = 10200 + (std::process::id() % 500) as u16;
        let mut child = std::process::Command::new(&program).args(["--vo", "null", "--ao", "null", "--noconfigfile", "--enable-remoteprotocol", "--remoteprotocol-port", &port.to_string(), "--machine", "48k"]).current_dir(std::path::Path::new(&program).parent().unwrap()).spawn().unwrap();
        let mut debug = RemoteDebug::start(Emulator::Zesarux, port, Some(file.clone()), vec![0x8002], || {});
        let wait_until = |debug: &mut RemoteDebug, what: &dyn Fn(&RemoteDebug) -> bool| {
            let began = Instant::now();
            while began.elapsed() < Duration::from_secs(30) {
                debug.check();
                if what(debug) || debug.problem.is_some() {
                    return;
                }
                std::thread::sleep(Duration::from_millis(50));
            }
        };
        wait_until(&mut debug, &|d| d.stopped.as_ref().is_some_and(|r| r.pc == 0x8002));
        let at = debug.stopped.clone().and_then(|s| s.z80);
        assert_eq!(at.map(|r| (r.pc, r.af >> 8)), Some((0x8002, 7)), "{:?}", debug.problem);
        let memory = debug.stopped.clone().and_then(|s| s.memory).expect("ZEsarUX's memory comes with the stop");
        assert_eq!((memory.len(), memory[0x8000], memory[0x8001]), (65536, 0x3E, 7), "LD A,7 at 32768");
        debug.ask(Ask::Step);
        wait_until(&mut debug, &|d| d.stopped.is_some());
        assert_eq!(debug.stopped.clone().and_then(|s| s.z80).map(|r| (r.pc, r.bc)), Some((0x8005, 0x1234)));
        debug.ask(Ask::StepOver);
        wait_until(&mut debug, &|d| d.stopped.is_some());
        assert_eq!(debug.stopped.clone().and_then(|s| s.z80).map(|r| (r.pc, r.hl)), Some((0x8008, 0xABCD)), "over the CALL");
        debug.ask(Ask::Breakpoints(vec![0x800B]));
        debug.ask(Ask::CarryOn);
        wait_until(&mut debug, &|d| d.stopped.is_some());
        assert_eq!(debug.stopped.clone().and_then(|s| s.z80).map(|r| (r.pc, r.de)), Some((0x800B, 0x5678)));
        drop(debug);
        let _ = child.kill();
        let _ = child.wait();
        let _ = std::fs::remove_dir_all(&folder);
    }
}
