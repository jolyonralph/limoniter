//! Debugging a program running in ZEsarUX, as the built-in Spectrum is debugged: breakpoints at source lines,
//! stopping, stepping and the registers. ZEsarUX is driven through its remote protocol (ZRCP) in step mode, its
//! `run` command carrying on until a breakpoint fires; a thread of its own talks to it, so the window never waits.

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
    CarryOn,
    Step,
    StepOver,
    Pause,
    /// Lets ZEsarUX run on by itself, and ends.
    Finish,
}

/// What the debugger tells the window.
#[derive(Clone, Debug, PartialEq)]
pub enum Told {
    Stopped(Registers),
    Running,
    Problem(String),
}

/// The window's end of a debugger.
pub struct RemoteDebug {
    asks: Sender<Ask>,
    told: Receiver<Told>,
    /// Where it last stopped, while it's stopped.
    pub stopped: Option<Registers>,
    pub problem: Option<String>,
}

impl RemoteDebug {
    /// Starts debugging the ZEsarUX listening on a port: once connected, it's stopped, given its breakpoints, and
    /// then loads the file (if there's one to load) and runs it, so not even the first instruction is missed.
    pub fn start(port: u16, file: Option<std::path::PathBuf>, breakpoints: Vec<u16>, wake: impl Fn() + Send + 'static) -> RemoteDebug {
        let (asks, asked) = channel();
        let (tell, told) = channel();
        std::thread::spawn(move || {
            if let Err(problem) = talk(port, file, breakpoints, &asked, &tell, &wake) {
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

    /// Takes in what the debugger has said since last time.
    pub fn check(&mut self) {
        while let Ok(told) = self.told.try_recv() {
            match told {
                Told::Stopped(registers) => self.stopped = Some(registers),
                Told::Running => self.stopped = None,
                Told::Problem(problem) => self.problem = Some(problem),
            }
        }
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
            Ask::Pause => say(Told::Stopped(link.registers()?)),
            Ask::Step => {
                link.command("cpu-step")?;
                say(Told::Stopped(link.registers()?));
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
                        Ok(_) | Err(TryRecvError::Empty) => {}
                    }
                }
                if next.is_none() {
                    say(Told::Stopped(link.registers()?));
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
        let mut debug = RemoteDebug::start(port, Some(file.clone()), vec![0x8002], || {});
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
        wait_until(&mut debug, &|d| d.stopped.is_some_and(|r| r.pc == 0x8002));
        let at = debug.stopped;
        assert_eq!(at.map(|r| (r.pc, r.af >> 8)), Some((0x8002, 7)), "{:?}", debug.problem);
        debug.ask(Ask::Step);
        wait_until(&mut debug, &|d| d.stopped.is_some());
        assert_eq!(debug.stopped.map(|r| (r.pc, r.bc)), Some((0x8005, 0x1234)));
        debug.ask(Ask::StepOver);
        wait_until(&mut debug, &|d| d.stopped.is_some());
        assert_eq!(debug.stopped.map(|r| (r.pc, r.hl)), Some((0x8008, 0xABCD)), "over the CALL");
        debug.ask(Ask::Breakpoints(vec![0x800B]));
        debug.ask(Ask::CarryOn);
        wait_until(&mut debug, &|d| d.stopped.is_some());
        assert_eq!(debug.stopped.map(|r| (r.pc, r.de)), Some((0x800B, 0x5678)));
        drop(debug);
        let _ = child.kill();
        let _ = child.wait();
        let _ = std::fs::remove_dir_all(&folder);
    }
}
