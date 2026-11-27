//! Debugging Amiga programs at source level in an emulator, through its GDB server: the GDB remote serial protocol
//! over TCP. Two emulators have one. The FS-UAE that uae-dap ships (4.x, patched with a remote debugger) and WinUAE
//! with Bartman's GDB server (the build vscode-amiga-debug and uae-dap ship) are each told the name of the program
//! (its trigger) and boot; once AmigaDOS has loaded the program and it starts, the emulator stops at its first
//! instruction and waits to be connected to. qOffsets then says where each of the program's hunks was put, and
//! Limoniter maps its own assembly onto them: every source line knows its section and how far into it it is.
//!
//! How the servers behave was learnt from their own sources and from uae-dap (GPL-3.0), whose code isn't used.
//! vAmiga's GDB server (attached by hand, in its own shell) answers the same packets, its offsets given as
//! Text=...;Data=...;Bss=..., and is understood too.

use crate::asm::Assembly;
use crate::remote_debug::{Ask, Stop, Told};
use std::collections::BTreeMap;
use std::io::{Read, Write};
use std::net::TcpStream;
use std::sync::mpsc::{Receiver, Sender, TryRecvError};
use std::time::{Duration, Instant};

/// The port Bartman's WinUAE always listens on.
pub const WINUAE_PORT: u16 = 2345;

/// The registers, in the order GDB numbers them for the 68000.
pub const REGISTER_NAMES: [&str; 18] = ["D0", "D1", "D2", "D3", "D4", "D5", "D6", "D7", "A0", "A1", "A2", "A3", "A4", "A5", "A6", "A7", "SR", "PC"];
pub const A7: usize = 15;
pub const SR: usize = 16;
pub const PC: usize = 17;

/// The custom chips, read at every stop.
pub const CUSTOM: u32 = crate::amiga_hw::CUSTOM;
pub const CUSTOM_SIZE: u32 = 0x200;

/// What the window asks of an Amiga debugger, beyond what every debugger is asked.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AmigaAsk {
    /// The breakpoints, as addresses in the assembly (each section where it was put while assembling).
    Breakpoints(Vec<i64>),
    /// Memory to read now, if it's stopped, and at every stop after: each region's start and length.
    Read(Vec<(u32, u32)>),
    /// The memory whose writing stops it: each region's start and length.
    Watches(Vec<(u32, u32)>),
    /// Runs on to an address in the assembly (a line's), or to a breakpoint before it.
    RunTo(i64),
    /// Changes a register, by its number in `REGISTER_NAMES`.
    SetRegister(usize, u32),
    /// Changes memory.
    Write(u32, Vec<u8>),
}

/// Where an Amiga program stopped, beyond what every stop says.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct AmigaStop {
    pub registers: [u32; 18],
    /// Where each hunk was loaded.
    pub bases: Vec<u32>,
    /// The memory read at this stop: each region's start and bytes. One that couldn't be read is left out.
    pub memory: Vec<(u32, Vec<u8>)>,
    /// Where it stopped, as an address in the assembly, if it's in the program.
    pub source: Option<i64>,
    /// Counts the stops, so a stop sent again with more memory read isn't taken for a new one.
    pub serial: u64,
    /// What's been printed through the emulator's debug output (kprintf), most recent last.
    pub output: String,
}

impl AmigaStop {
    pub fn pc(&self) -> u32 {
        self.registers[PC]
    }

    /// A byte, if it was read.
    pub fn peek(&self, address: u32) -> Option<u8> {
        self.memory.iter().find_map(|(start, bytes)| address.checked_sub(*start).and_then(|at| bytes.get(at as usize)).copied())
    }

    /// Whether all of a region was read.
    pub fn has(&self, start: u32, length: u32) -> bool {
        self.memory.iter().any(|(at, bytes)| *at <= start && u64::from(start) + u64::from(length) <= u64::from(*at) + bytes.len() as u64)
    }

    /// A big-endian word, if it was read.
    pub fn word(&self, address: u32) -> Option<u16> {
        Some(u16::from_be_bytes([self.peek(address)?, self.peek(address.wrapping_add(1))?]))
    }

    pub fn long(&self, address: u32) -> Option<u32> {
        Some(u32::from(self.word(address)?) << 16 | u32::from(self.word(address.wrapping_add(2))?))
    }
}

/// What the window keeps while an Amiga program is debugged.
#[derive(Clone, Debug, Default)]
pub struct Shown {
    pub program: Program,
    /// The breakpoints and watches last sent (None before any).
    pub breakpoints: Option<Vec<i64>>,
    pub watches: Option<Vec<(u32, u32)>>,
    /// Memory already asked to be read.
    pub reads: Vec<(u32, u32)>,
    /// The stop shown, and the one before it, for what's changed to stand out.
    pub now: Option<AmigaStop>,
    pub before: Option<AmigaStop>,
    /// The register being changed, and what's typed.
    pub editing: Option<(usize, String)>,
    /// Memory to change: where, and the bytes in hex, as typed.
    pub poke_at: String,
    pub poke_bytes: String,
}

/// A register (D0-D7, A0-A7 or SP, PC, SR, CCR, with .B, .W or .L for part of one) or a flag (XF, NF, ZF, VF, CF)
/// by name, for watches and conditions.
pub fn register_value(name: &str, registers: &[u32; 18]) -> Option<i64> {
    let upper = name.to_ascii_uppercase();
    let (base, size) = match upper.rsplit_once('.') {
        Some((base, "B")) => (base.to_string(), 0xFF),
        Some((base, "W")) => (base.to_string(), 0xFFFF),
        Some((base, "L")) => (base.to_string(), 0xFFFF_FFFF),
        Some(_) => return None,
        None => (upper.clone(), 0xFFFF_FFFFu32),
    };
    let flag = |bit: u32| Some(i64::from(registers[SR] >> bit & 1));
    let value = match base.as_str() {
        "SP" => registers[A7],
        "CCR" => registers[SR] & 0xFF,
        "XF" => return flag(4),
        "NF" => return flag(3),
        "ZF" => return flag(2),
        "VF" => return flag(1),
        "CF" => return flag(0),
        other => registers[REGISTER_NAMES.iter().position(|n| *n == other)?],
    };
    Some(i64::from(value & size))
}

/// The status register as words: supervisor or user, the interrupt mask, trace, and the flags.
pub fn status_words(sr: u32) -> String {
    let flags: String = [(4, 'X'), (3, 'N'), (2, 'Z'), (1, 'V'), (0, 'C')].iter().map(|(bit, name)| if sr >> bit & 1 != 0 { *name } else { '-' }).collect();
    let trace = if sr & 0x8000 != 0 { " T" } else { "" };
    format!("{} IPL {}{trace}  {flags}", if sr & 0x2000 != 0 { "S" } else { "U" }, sr >> 8 & 7)
}

/// How long the 68000 instruction at the start of `bytes` is, if it's a call to step over: BSR, JSR, or a TRAP
/// (whose handler, in the ROM, returns to the instruction after it).
pub fn call_length(bytes: &[u8]) -> Option<u32> {
    let op = u16::from_be_bytes([*bytes.first()?, *bytes.get(1)?]);
    if op & 0xFF00 == 0x6100 {
        return Some(match op & 0xFF {
            0 => 4,
            0xFF => 6,
            _ => 2,
        });
    }
    if op & 0xFFF0 == 0x4E40 {
        return Some(2);
    }
    if op & 0xFFC0 == 0x4E80 {
        let (mode, register) = (op >> 3 & 7, op & 7);
        return match (mode, register) {
            (2, _) => Some(2),
            (5 | 6, _) => Some(4),
            (7, 0 | 2 | 3) => Some(4),
            (7, 1) => Some(6),
            _ => None,
        };
    }
    None
}

/// A program as Limoniter assembled it, as much as the debugger needs: its hunks, where its lines start, and its
/// code, to read instructions from without asking the emulator.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Program {
    /// Each hunk's place in the assembly: where its section starts and ends.
    pub hunks: Vec<(i64, i64)>,
    /// Where each source line's code starts, in order.
    pub line_starts: Vec<i64>,
    /// Each hunk's bytes, as assembled (before the loader moved its addresses).
    pub images: Vec<Vec<u8>>,
}

impl Program {
    pub fn of(assembly: &Assembly) -> Program {
        let hunks: Vec<(i64, i64)> = assembly.sections.iter().filter(|s| s.size() > 0).map(|s| (s.start, s.end)).collect();
        let mut line_starts: Vec<i64> = assembly.lines.iter().filter(|p| !p.bytes.is_empty()).map(|p| p.address).collect();
        line_starts.sort_unstable();
        line_starts.dedup();
        let images = hunks.iter().map(|(start, end)| assembly.image(*start, *end)).collect();
        Program { hunks, line_starts, images }
    }

    /// Where an address in the assembly is in the Amiga's memory, now its hunks have been loaded at `bases`.
    pub fn to_runtime(&self, bases: &[u32], address: i64) -> Option<u32> {
        let (index, (start, _)) = self.hunks.iter().enumerate().find(|(_, (start, end))| address >= *start && address < *end)?;
        Some(bases.get(index)?.wrapping_add((address - start) as u32))
    }

    /// Where an address in the Amiga's memory is in the assembly, if it's in one of the program's hunks.
    pub fn to_source(&self, bases: &[u32], address: u32) -> Option<i64> {
        self.hunks.iter().zip(bases).find_map(|((start, end), base)| {
            let offset = i64::from(address.wrapping_sub(*base));
            (address >= *base && offset < end - start).then_some(start + offset)
        })
    }

    pub fn is_line_start(&self, address: i64) -> bool {
        self.line_starts.binary_search(&address).is_ok()
    }

    /// The bytes assembled at an address in the assembly.
    fn code_at(&self, address: i64, length: usize) -> Option<Vec<u8>> {
        let (index, (start, _)) = self.hunks.iter().enumerate().find(|(_, (start, end))| address >= *start && address < *end)?;
        let at = (address - start) as usize;
        let image = self.images.get(index)?;
        Some(image.get(at..(at + length).min(image.len()))?.to_vec())
    }
}

/// The program's labels where they are now: those in a hunk moved to where it was loaded, constants as they are.
pub fn relocate_symbols(symbols: &BTreeMap<String, i64>, program: &Program, bases: &[u32]) -> BTreeMap<String, i64> {
    symbols.iter().map(|(name, value)| (name.clone(), program.to_runtime(bases, *value).map_or(*value, i64::from))).collect()
}

/// The calls a stopped 68000 program is inside, innermost first: longs on the stack that are just after a JSR or
/// BSR in the program. Each is the call's address, as an address in the assembly. A long pushed by MOVE that
/// happens to look like one can be taken for one.
pub fn call_stack(stop: &AmigaStop, program: &Program) -> Vec<i64> {
    let sp = stop.registers[A7];
    let mut calls = Vec::new();
    for offset in (0..256u32).step_by(2) {
        let Some(value) = stop.long(sp.wrapping_add(offset)) else { break };
        let Some(after) = program.to_source(&stop.bases, value) else { continue };
        for length in [2u32, 4, 6] {
            let call = after - i64::from(length);
            // A TRAP's return address is in its exception frame, not one of these.
            let is_call = program.code_at(call, 6).is_some_and(|bytes| call_length(&bytes) == Some(length) && u16::from_be_bytes([bytes[0], bytes[1]]) & 0xFFF0 != 0x4E40);
            if is_call {
                calls.push(call);
                break;
            }
        }
        if calls.len() >= 12 {
            break;
        }
    }
    calls
}

/// The 68000's registers as GDB's g packet gives them: 18 longs, D0 to PC.
pub fn parse_registers(said: &str) -> Option<[u32; 18]> {
    let mut registers = [0u32; 18];
    for (i, register) in registers.iter_mut().enumerate() {
        let hex = said.get(i * 8..i * 8 + 8)?;
        // A register the server can't give is xxxxxxxx.
        *register = u32::from_str_radix(hex, 16).unwrap_or(0);
    }
    Some(registers)
}

/// Where the hunks were loaded, as qOffsets says: each hunk's address, joined with ; (as the UAEs say), or
/// Text=...;Data=...;Bss=... (as vAmiga, and GDB itself, say).
pub fn parse_offsets(said: &str) -> Vec<u32> {
    said.split(';').filter(|part| !part.is_empty()).filter_map(|part| u32::from_str_radix(part.rsplit('=').next().unwrap_or(part), 16).ok()).collect()
}

/// Why it stopped, from a stop reply, when it's more than a breakpoint, a step or a pause.
pub fn stop_reason(reply: &str) -> Option<String> {
    if let Some(rest) = reply.strip_prefix("T05") {
        for kind in ["watch", "rwatch", "awatch"] {
            if let Some(address) = rest.strip_prefix(kind).and_then(|r| r.strip_prefix(':')) {
                let address = address.split(';').next().and_then(|a| u32::from_str_radix(a, 16).ok()).unwrap_or(0);
                return Some(match kind {
                    "watch" => crate::i18n::tf!("Stopped after a write to ${address:08X}"),
                    "rwatch" => crate::i18n::tf!("Stopped after a read of ${address:08X}"),
                    _ => crate::i18n::tf!("Stopped after an access to ${address:08X}"),
                });
            }
        }
        return None;
    }
    let signal = reply.get(1..3).and_then(|s| u8::from_str_radix(s, 16).ok())?;
    match signal {
        2 | 5 => None,
        4 => Some(crate::i18n::tr("Stopped at an illegal instruction").into()),
        7 => Some(crate::i18n::tr("Stopped at TRAP #7").into()),
        8 => Some(crate::i18n::tr("Stopped by an arithmetic exception, such as a division by zero").into()),
        10 => Some(crate::i18n::tr("Stopped by an address error: a word or long at an odd address").into()),
        11 => Some(crate::i18n::tr("Stopped by a read or write of address 0").into()),
        other => Some(crate::i18n::tf!("Stopped by an exception (signal {other})")),
    }
}

/// A packet's data, from between $ and #: } escapes the byte after it, and * repeats the one before it.
fn unpack(data: &[u8]) -> String {
    let mut out: Vec<u8> = Vec::with_capacity(data.len());
    let mut at = 0;
    while at < data.len() {
        match data[at] {
            b'}' if at + 1 < data.len() => {
                out.push(data[at + 1] ^ 0x20);
                at += 2;
            }
            b'*' if at + 1 < data.len() && !out.is_empty() => {
                let repeat = data[at + 1].saturating_sub(29) as usize;
                let last = *out.last().expect("checked");
                out.extend(std::iter::repeat_n(last, repeat));
                at += 2;
            }
            byte => {
                out.push(byte);
                at += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn hex_bytes(said: &str) -> Option<Vec<u8>> {
    if !said.len().is_multiple_of(2) {
        return None;
    }
    said.as_bytes().chunks(2).map(|pair| std::str::from_utf8(pair).ok().and_then(|p| u8::from_str_radix(p, 16).ok())).collect()
}

/// A GDB server, a packet at a time.
pub struct Gdb {
    stream: TcpStream,
    /// What's been read but not yet taken as a packet.
    pending: Vec<u8>,
    /// Whether packets are still acknowledged with +; QStartNoAckMode stops it.
    acks: bool,
    /// What the server printed (O packets), as text.
    pub output: String,
    /// Whether the program has ended.
    pub ended: bool,
}

impl Gdb {
    /// Connects to a server on this computer, trying for a while as the emulator may still be starting; `give_up`
    /// is asked between tries.
    pub fn connect(port: u16, wait: Duration, give_up: &dyn Fn() -> bool) -> Result<Gdb, String> {
        let began = Instant::now();
        let stream = loop {
            match TcpStream::connect_timeout(&([127, 0, 0, 1], port).into(), Duration::from_secs(2)) {
                Ok(stream) => break stream,
                Err(_) if began.elapsed() < wait && !give_up() => std::thread::sleep(Duration::from_millis(200)),
                Err(e) => return Err(crate::i18n::tf!("The emulator's debugger couldn't be reached on port {port}: {e}")),
            }
        };
        stream.set_read_timeout(Some(Duration::from_millis(50))).map_err(|e| e.to_string())?;
        let _ = stream.set_nodelay(true);
        Ok(Gdb { stream, pending: Vec::new(), acks: true, output: String::new(), ended: false })
    }

    pub fn send(&mut self, data: &str) -> Result<(), String> {
        let sum = data.bytes().fold(0u8, |a, b| a.wrapping_add(b));
        self.stream.write_all(format!("${data}#{sum:02x}").as_bytes()).map_err(|e| crate::i18n::tf!("The emulator couldn't be told: {e}"))
    }

    /// Ctrl+C, which stops it.
    pub fn interrupt(&mut self) -> Result<(), String> {
        self.stream.write_all(&[3]).map_err(|e| crate::i18n::tf!("The emulator couldn't be told: {e}"))
    }

    /// The next packet, if one comes within `wait`. What the server prints (O packets) is kept as output, not
    /// given; acknowledgements and anything else between packets are passed over.
    pub fn packet(&mut self, wait: Duration) -> Result<Option<String>, String> {
        let began = Instant::now();
        let mut buffer = [0u8; 8192];
        loop {
            if let Some(start) = self.pending.iter().position(|b| *b == b'$') {
                if let Some(hash) = self.pending[start..].iter().position(|b| *b == b'#').map(|h| start + h)
                    && self.pending.len() >= hash + 3
                {
                    let data = unpack(&self.pending[start + 1..hash]);
                    self.pending.drain(..hash + 3);
                    if self.acks {
                        self.stream.write_all(b"+").map_err(|e| e.to_string())?;
                    }
                    if let Some(text) = data.strip_prefix('O').filter(|t| !t.is_empty() && data != "OK").and_then(hex_bytes) {
                        self.output.push_str(&String::from_utf8_lossy(&text));
                        let excess = self.output.len().saturating_sub(8000);
                        if excess > 0 {
                            let cut = self.output.char_indices().map(|(i, _)| i).find(|i| *i >= excess).unwrap_or(0);
                            self.output.drain(..cut);
                        }
                        continue;
                    }
                    if data.starts_with('W') || data.starts_with('X') {
                        self.ended = true;
                    }
                    return Ok(Some(data));
                }
            } else {
                self.pending.clear();
            }
            if began.elapsed() > wait {
                return Ok(None);
            }
            match self.stream.read(&mut buffer) {
                Ok(0) => return Err(crate::i18n::tr("The emulator closed the connection.").into()),
                Ok(got) => self.pending.extend_from_slice(&buffer[..got]),
                Err(e) if matches!(e.kind(), std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut) => {}
                Err(e) => return Err(crate::i18n::tf!("The emulator stopped answering: {e}")),
            }
        }
    }

    /// Sends a packet and waits for the answer, up to `wait`.
    pub fn command_within(&mut self, data: &str, wait: Duration) -> Result<String, String> {
        self.send(data)?;
        let said = self.packet(wait)?.ok_or_else(|| crate::i18n::tf!("The emulator didn't answer {data}."))?;
        if said.len() == 3 && said.starts_with('E') {
            return Err(crate::i18n::tf!("The emulator refused {data} ({said})."));
        }
        Ok(said)
    }

    pub fn command(&mut self, data: &str) -> Result<String, String> {
        self.command_within(data, Duration::from_secs(10))
    }

    /// The first exchange: asks to stop acknowledging packets, and waits up to `wait` for the answer, which comes
    /// once the program has started; `give_up` is asked meanwhile. False if given up.
    pub fn first_contact(&mut self, wait: Duration, give_up: &dyn Fn() -> bool) -> Result<bool, String> {
        self.send("QStartNoAckMode")?;
        let began = Instant::now();
        loop {
            if give_up() {
                return Ok(false);
            }
            if let Some(said) = self.packet(Duration::from_millis(300))? {
                if said == "OK" {
                    self.acks = false;
                }
                return Ok(true);
            }
            if began.elapsed() > wait {
                return Err(crate::i18n::tr("The emulator's debugger didn't answer: the program never started, or this emulator has no GDB server.").into());
            }
        }
    }

    pub fn registers(&mut self) -> Result<[u32; 18], String> {
        let said = self.command("g")?;
        parse_registers(&said).ok_or_else(|| crate::i18n::tf!("The emulator's registers weren't understood: {said}"))
    }

    /// Memory, a little at a time; None if the server wouldn't read it.
    pub fn read(&mut self, start: u32, length: u32) -> Result<Option<Vec<u8>>, String> {
        let mut bytes = Vec::with_capacity(length as usize);
        let mut at = 0;
        while at < length {
            let take = (length - at).min(0x100);
            let address = start.wrapping_add(at);
            self.send(&format!("m{address:x},{take:x}"))?;
            let said = self.packet(Duration::from_secs(10))?.ok_or(crate::i18n::tr("The emulator stopped answering."))?;
            match hex_bytes(&said).filter(|b| b.len() == take as usize) {
                Some(got) => bytes.extend(got),
                None => return Ok(None),
            }
            at += take;
        }
        Ok(Some(bytes))
    }

    pub fn write(&mut self, start: u32, bytes: &[u8]) -> Result<(), String> {
        let hex: String = bytes.iter().map(|b| format!("{b:02x}")).collect();
        self.command(&format!("M{start:x},{:x}:{hex}", bytes.len())).map(|_| ())
    }

    pub fn set_register(&mut self, index: usize, value: u32) -> Result<(), String> {
        match self.command(&format!("P{index:x}={value:08x}"))?.as_str() {
            "OK" => Ok(()),
            "" => Err(crate::i18n::tr("This emulator can't change registers.").into()),
            said => Err(crate::i18n::tf!("The emulator didn't change it ({said}).")),
        }
    }
}

fn is_stop(reply: &str) -> bool {
    reply.starts_with('S') || reply.starts_with('T')
}

/// The debugger's side of a session: the server, the program and where its hunks are, and what's been set.
struct Session<'a> {
    gdb: Gdb,
    program: Program,
    bases: Vec<u32>,
    /// The breakpoints asked for, in the assembly; those for a while (stepping over, running to a line), in
    /// memory; and those set in the emulator now.
    wanted: Vec<i64>,
    temporary: Vec<u32>,
    set: Vec<u32>,
    watches_wanted: Vec<(u32, u32)>,
    watches_set: Vec<(u32, u32)>,
    /// The memory read at every stop, besides the stack, the code and the custom chips.
    reads: Vec<(u32, u32)>,
    serial: u64,
    asked: &'a Receiver<Ask>,
    tell: &'a Sender<Told>,
    wake: &'a dyn Fn(),
}

/// What carrying on came to.
enum Ran {
    Stopped(String),
    Finish,
}

impl Session<'_> {
    fn say(&self, told: Told) {
        let _ = self.tell.send(told);
        (self.wake)();
    }

    /// Makes the emulator's breakpoints and watches what's wanted.
    fn apply(&mut self) -> Result<(), String> {
        let mut want: Vec<u32> = self.wanted.iter().filter_map(|a| self.program.to_runtime(&self.bases, *a)).collect();
        want.extend(self.temporary.iter().copied());
        want.sort_unstable();
        want.dedup();
        for address in self.set.clone().iter().filter(|a| !want.contains(a)) {
            self.gdb.command(&format!("z0,{address:x},2"))?;
        }
        for address in want.iter().filter(|a| !self.set.contains(a)) {
            self.gdb.command(&format!("Z0,{address:x},2"))?;
        }
        self.set = want;
        for (address, length) in self.watches_set.clone().iter().filter(|w| !self.watches_wanted.contains(w)) {
            self.gdb.command(&format!("z2,{address:x},{length:x}"))?;
        }
        for (address, length) in self.watches_wanted.clone().iter().filter(|w| !self.watches_set.contains(w)) {
            self.gdb.command(&format!("Z2,{address:x},{length:x}"))?;
        }
        self.watches_set = self.watches_wanted.clone();
        Ok(())
    }

    fn add_reads(&mut self, regions: Vec<(u32, u32)>) {
        for region in regions {
            if !self.reads.contains(&region) {
                self.reads.push(region);
            }
        }
        // The most recent are kept.
        let excess = self.reads.len().saturating_sub(24);
        self.reads.drain(..excess);
    }

    /// Where it's stopped, with its registers and the memory wanted. `new` counts it as a new stop.
    fn stop(&mut self, reason: Option<String>, new: bool) -> Result<Stop, String> {
        let registers = self.gdb.registers()?;
        if new {
            self.serial += 1;
        }
        let mut regions = vec![(registers[PC] & !1, 16), (registers[A7] & !1, 256), (CUSTOM, CUSTOM_SIZE)];
        regions.extend(self.reads.iter().copied());
        let mut memory = Vec::new();
        for (start, length) in regions {
            if let Some(bytes) = self.gdb.read(start, length)? {
                memory.push((start, bytes));
            }
        }
        let source = self.program.to_source(&self.bases, registers[PC]);
        let words = REGISTER_NAMES.iter().zip(registers).map(|(name, value)| format!("{name} {value:08X}")).collect();
        let amiga = AmigaStop { registers, bases: self.bases.clone(), memory, source, serial: self.serial, output: self.gdb.output.clone() };
        Ok(Stop { pc: registers[PC] as u16, words, reason, amiga: Some(Box::new(amiga)), ..Stop::default() })
    }

    /// Carries on until it stops, dealing with what's asked meanwhile: a pause stops it, and breakpoints and
    /// watches are changed with it stopped for a moment.
    fn run(&mut self) -> Result<Ran, String> {
        self.gdb.send("vCont;c")?;
        let (mut changing, mut pausing, mut finishing) = (false, false, false);
        loop {
            if let Some(reply) = self.gdb.packet(Duration::from_millis(100))? {
                if self.gdb.ended {
                    return Err(crate::i18n::tr("The program has ended.").into());
                }
                if !is_stop(&reply) {
                    continue;
                }
                if finishing {
                    return Ok(Ran::Finish);
                }
                if changing {
                    changing = false;
                    self.apply()?;
                    if !pausing {
                        self.gdb.send("vCont;c")?;
                        continue;
                    }
                }
                return Ok(Ran::Stopped(reply));
            }
            match self.asked.try_recv() {
                Ok(Ask::Pause) => {
                    pausing = true;
                    self.gdb.interrupt()?;
                }
                Ok(Ask::Finish) | Err(TryRecvError::Disconnected) => {
                    finishing = true;
                    self.gdb.interrupt()?;
                }
                Ok(Ask::Amiga(AmigaAsk::Breakpoints(wanted))) => {
                    self.wanted = wanted;
                    if !changing {
                        changing = true;
                        self.gdb.interrupt()?;
                    }
                }
                Ok(Ask::Amiga(AmigaAsk::Watches(wanted))) => {
                    self.watches_wanted = wanted;
                    if !changing {
                        changing = true;
                        self.gdb.interrupt()?;
                    }
                }
                Ok(Ask::Amiga(AmigaAsk::Read(regions))) => self.add_reads(regions),
                Ok(_) | Err(TryRecvError::Empty) => {}
            }
        }
    }

    /// Runs one instruction.
    fn step_one(&mut self) -> Result<String, String> {
        self.gdb.send("vCont;s")?;
        loop {
            let reply = self.gdb.packet(Duration::from_secs(10))?.ok_or(crate::i18n::tr("The emulator didn't step."))?;
            if self.gdb.ended {
                return Err(crate::i18n::tr("The program has ended.").into());
            }
            if is_stop(&reply) {
                return Ok(reply);
            }
        }
    }

    /// Runs on until it's back at `to` with the stack no deeper than `sp` (out of a call); what it stopped with,
    /// and whether it got there rather than stopping somewhere else first.
    fn run_back_to(&mut self, to: u32, sp: u32) -> Result<Option<(String, bool)>, String> {
        self.temporary.push(to);
        self.apply()?;
        let outcome = loop {
            let reply = match self.run()? {
                Ran::Stopped(reply) => reply,
                Ran::Finish => break None,
            };
            let registers = self.gdb.registers()?;
            if registers[PC] == to && registers[A7] >= sp {
                break Some((reply, true));
            }
            let wanted_there = self.wanted.iter().any(|a| self.program.to_runtime(&self.bases, *a) == Some(to));
            if registers[PC] != to || wanted_there {
                // Somewhere else first: a breakpoint, a pause or an exception.
                break Some((reply, false));
            }
            // A deeper call (recursion) got there first: on again.
        };
        self.temporary.retain(|a| *a != to);
        self.apply()?;
        Ok(outcome)
    }

    /// Steps to the next source line: into a subroutine of the program's (unless `over`), but over calls out of
    /// it (into the ROM or a library) and, with `over`, over all of them.
    fn step_line(&mut self, over: bool) -> Result<Option<Stop>, String> {
        let began = Instant::now();
        let mut steps = 0;
        loop {
            let registers = self.gdb.registers()?;
            let pc = registers[PC];
            let bytes = match self.program.to_source(&self.bases, pc).and_then(|s| self.program.code_at(s, 6)) {
                Some(bytes) => bytes,
                None => self.gdb.read(pc, 6)?.unwrap_or_default(),
            };
            let call = call_length(&bytes);
            let reply = match call {
                Some(length) if over => match self.run_back_to(pc.wrapping_add(length), registers[A7])? {
                    None => return Ok(None),
                    Some((reply, true)) => reply,
                    Some((reply, false)) => return Ok(Some(self.stop(stop_reason(&reply), true)?)),
                },
                _ => {
                    let reply = self.step_one()?;
                    if let Some(reason) = stop_reason(&reply) {
                        return Ok(Some(self.stop(Some(reason), true)?));
                    }
                    let now = self.gdb.registers()?[PC];
                    if self.program.to_source(&self.bases, now).is_none() {
                        // Into the ROM or a library: back out to the instruction after the call.
                        let Some(length) = call else {
                            return Ok(Some(self.stop(Some(crate::i18n::tr("Stepped out of the program").into()), true)?));
                        };
                        match self.run_back_to(pc.wrapping_add(length), registers[A7])? {
                            None => return Ok(None),
                            Some((reply, true)) => reply,
                            Some((reply, false)) => return Ok(Some(self.stop(stop_reason(&reply), true)?)),
                        }
                    } else {
                        reply
                    }
                }
            };
            steps += 1;
            let now = self.gdb.registers()?[PC];
            if self.program.to_source(&self.bases, now).is_some_and(|s| self.program.is_line_start(s)) {
                return Ok(Some(self.stop(stop_reason(&reply), true)?));
            }
            if self.set.contains(&now) && now != pc {
                return Ok(Some(self.stop(None, true)?));
            }
            if steps >= 2000 || began.elapsed() > Duration::from_secs(8) {
                return Ok(Some(self.stop(Some(crate::i18n::tf!("Stepped {steps} instructions without reaching another line")), true)?));
            }
        }
    }
}

/// The debugger's thread: connects, waits for the program to start, finds its hunks, sets the breakpoints, carries
/// on to the first, and then does as it's asked.
pub fn talk(port: u16, program: Program, breakpoints: Vec<i64>, asked: &Receiver<Ask>, tell: &Sender<Told>, wake: &dyn Fn()) -> Result<(), String> {
    // While it boots, breakpoints and watches changed are kept for when it's started.
    let finished = std::cell::Cell::new(false);
    let early: std::cell::RefCell<Vec<Ask>> = std::cell::RefCell::new(Vec::new());
    let give_up = || {
        match asked.try_recv() {
            Ok(Ask::Finish) | Err(TryRecvError::Disconnected) => finished.set(true),
            Ok(ask) => early.borrow_mut().push(ask),
            Err(TryRecvError::Empty) => {}
        }
        finished.get()
    };
    let mut gdb = Gdb::connect(port, Duration::from_secs(60), &give_up)?;
    // The server answers once the program has started, which may be after the Amiga has booted: a while.
    if !gdb.first_contact(Duration::from_secs(180), &give_up)? {
        return Ok(());
    }
    gdb.command("?")?;
    let bases = parse_offsets(&gdb.command("qOffsets")?);
    if bases.is_empty() {
        return Err(crate::i18n::tr("The emulator didn't say where the program was loaded.").into());
    }
    let mut session = Session { gdb, program, bases, wanted: breakpoints, temporary: Vec::new(), set: Vec::new(), watches_wanted: Vec::new(), watches_set: Vec::new(), reads: Vec::new(), serial: 0, asked, tell, wake };
    for ask in early.into_inner() {
        match ask {
            Ask::Amiga(AmigaAsk::Breakpoints(wanted)) => session.wanted = wanted,
            Ask::Amiga(AmigaAsk::Watches(wanted)) => session.watches_wanted = wanted,
            Ask::Amiga(AmigaAsk::Read(regions)) => session.add_reads(regions),
            _ => {}
        }
    }
    if session.bases.len() < session.program.hunks.len() {
        session.say(Told::Problem(crate::i18n::tf!("The emulator gave {} hunk addresses for a program of {}: only the first {} can be debugged.", session.bases.len(), session.program.hunks.len(), session.bases.len())));
    }
    session.apply()?;
    // At its first instruction now: on to the first breakpoint, unless that's it.
    let pc = session.gdb.registers()?[PC];
    let mut next = if session.set.contains(&pc) {
        let stop = session.stop(None, true)?;
        session.say(Told::Stopped(stop));
        None
    } else {
        Some(Ask::CarryOn)
    };
    loop {
        let ask = match next.take() {
            Some(ask) => ask,
            None => asked.recv().unwrap_or(Ask::Finish),
        };
        match ask {
            Ask::Amiga(AmigaAsk::Breakpoints(wanted)) => {
                session.wanted = wanted;
                session.apply()?;
            }
            Ask::Amiga(AmigaAsk::Watches(wanted)) => {
                session.watches_wanted = wanted;
                session.apply()?;
            }
            Ask::Amiga(AmigaAsk::Read(regions)) => {
                session.add_reads(regions);
                let stop = session.stop(None, false)?;
                session.say(Told::Stopped(stop));
            }
            Ask::Amiga(AmigaAsk::SetRegister(index, value)) => {
                let reason = session.gdb.set_register(index, value).err();
                let stop = session.stop(reason, false)?;
                session.say(Told::Stopped(stop));
            }
            Ask::Amiga(AmigaAsk::Write(at, bytes)) => {
                let reason = session.gdb.write(at, &bytes).err();
                let stop = session.stop(reason, false)?;
                session.say(Told::Stopped(stop));
            }
            Ask::Amiga(AmigaAsk::RunTo(address)) => {
                let Some(to) = session.program.to_runtime(&session.bases, address) else { continue };
                session.temporary.push(to);
                session.apply()?;
                session.say(Told::Running);
                let ran = session.run()?;
                session.temporary.retain(|a| *a != to);
                session.apply()?;
                match ran {
                    Ran::Stopped(reply) => {
                        let stop = session.stop(stop_reason(&reply), true)?;
                        session.say(Told::Stopped(stop));
                    }
                    Ran::Finish => next = Some(Ask::Finish),
                }
            }
            Ask::CarryOn => {
                session.say(Told::Running);
                match session.run()? {
                    Ran::Stopped(reply) => {
                        let stop = session.stop(stop_reason(&reply), true)?;
                        session.say(Told::Stopped(stop));
                    }
                    Ran::Finish => next = Some(Ask::Finish),
                }
            }
            Ask::Step | Ask::StepOver => {
                session.say(Told::Running);
                match session.step_line(ask == Ask::StepOver)? {
                    Some(stop) => session.say(Told::Stopped(stop)),
                    None => next = Some(Ask::Finish),
                }
            }
            // The Z80's breakpoints and watches, and a pause while it's stopped, mean nothing here.
            Ask::Breakpoints(_) | Ask::Watches(_) | Ask::Accesses(_) | Ask::Pause => {}
            Ask::Finish => {
                session.wanted.clear();
                session.temporary.clear();
                session.watches_wanted.clear();
                let _ = session.apply();
                let _ = session.gdb.send("vCont;c");
                return Ok(());
            }
        }
    }
}

/// Whether an emulator's program has a GDB server: the FS-UAE uae-dap ships, or WinUAE with Bartman's. Told by
/// what's written in it, and remembered while the file stays the same.
pub fn has_gdb_server(program: &std::path::Path) -> bool {
    use std::sync::Mutex;
    static KNOWN: Mutex<Vec<(std::path::PathBuf, u64, Option<std::time::SystemTime>, bool)>> = Mutex::new(Vec::new());
    let Ok(meta) = std::fs::metadata(program) else { return false };
    let (size, modified) = (meta.len(), meta.modified().ok());
    if let Ok(known) = KNOWN.lock()
        && let Some(found) = known.iter().find(|(p, s, m, _)| p == program && *s == size && *m == modified)
    {
        return found.3;
    }
    let Ok(bytes) = std::fs::read(program) else { return false };
    // FS-UAE's option, or WinUAE's word for the feature (its options are kept as UTF-16).
    let wide: Vec<u8> = "gdbserver".encode_utf16().flat_map(u16::to_le_bytes).collect();
    let has = [&b"remote_debugger_trigger"[..], wide.as_slice()].iter().any(|word| bytes.windows(word.len()).any(|w| w == *word));
    if let Ok(mut known) = KNOWN.lock() {
        known.retain(|(p, ..)| p != program);
        known.push((program.to_path_buf(), size, modified, has));
    }
    has
}

/// The lines that turn an emulator's GDB server on, for the configuration made for a run: stopping when the
/// program named `trigger` starts.
pub fn config_lines(emulator: &str, port: u16, trigger: &str) -> String {
    match emulator {
        "winuae" => format!("debugging_features=gdbserver\r\ndebugging_trigger={trigger}\r\n"),
        _ => format!("remote_debugger = 60\nremote_debugger_port = {port}\nremote_debugger_trigger = {trigger}\n"),
    }
}

#[cfg(test)]
#[path = "amiga_debug_tests.rs"]
pub(crate) mod tests;
