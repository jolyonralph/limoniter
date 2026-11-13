//! Debugging on a real ZX Spectrum Next, as DeZog does: the DeZog Remote Protocol (DZRP 2.2) to dezogif, the
//! debugger DeZog puts in the Next's NMI ROM, over a serial cable to the Next's UART (921600 baud). dezogif keeps
//! breakpoints the simple way, as RST 0 instructions written over the program's own bytes and put back when it
//! stops, so a step is a CONTINUE with breakpoints after the instruction and where it may jump.
//!
//! The protocol is in DeZog's design/DeZogProtocol.md: a command is a 4-byte little-endian length of its payload,
//! a sequence number (1 to 15), the command and the payload; a response is a length, the same sequence number and
//! its payload; a notification (NTF_PAUSE when it stops) has sequence number 0.

use crate::zx_machine::Registers;
use std::io::{Read, Write};
use std::time::{Duration, Instant};

pub const CMD_INIT: u8 = 1;
pub const CMD_CLOSE: u8 = 2;
pub const CMD_GET_REGISTERS: u8 = 3;
pub const CMD_SET_REGISTER: u8 = 4;
pub const CMD_CONTINUE: u8 = 6;
pub const CMD_PAUSE: u8 = 7;
pub const CMD_SET_SLOT: u8 = 10;
pub const CMD_SET_BREAKPOINTS: u8 = 13;
pub const CMD_RESTORE_MEM: u8 = 14;
pub const CMD_WRITE_BANK_MEM: u8 = 26;
pub const CMD_READ_MEM_BLOCKS: u8 = 28;
pub const NTF_PAUSE: u8 = 1;

/// The version spoken: 2.2.0.
pub const VERSION: [u8; 3] = [2, 2, 0];

/// How much is read or written at a time: the Next has no flow control, and a USB adapter's buffer is small.
const CHUNK: usize = 1024;

/// What came back from the remote.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Message {
    Response { seq: u8, payload: Vec<u8> },
    Notification { id: u8, payload: Vec<u8> },
}

/// A connection to dezogif, over anything that reads and writes: a serial port, or a socket.
pub struct Dzrp<T: Read + Write> {
    link: T,
    seq: u8,
    /// A stop that arrived while waiting for a response.
    pub paused: Option<Vec<u8>>,
}

impl<T: Read + Write> Dzrp<T> {
    pub fn new(link: T) -> Self {
        Dzrp { link, seq: 0, paused: None }
    }

    fn read_exact(&mut self, count: usize, wait: Duration) -> Result<Option<Vec<u8>>, String> {
        let began = Instant::now();
        let mut got = Vec::with_capacity(count);
        let mut buffer = vec![0u8; count];
        while got.len() < count {
            match self.link.read(&mut buffer[..count - got.len()]) {
                Ok(0) => return Err("The Next closed the connection.".into()),
                Ok(n) => got.extend_from_slice(&buffer[..n]),
                Err(e) if matches!(e.kind(), std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut | std::io::ErrorKind::Interrupted) => {
                    if got.is_empty() && began.elapsed() > wait {
                        return Ok(None);
                    }
                    if began.elapsed() > wait + Duration::from_secs(5) {
                        return Err("The Next stopped partway through a message.".into());
                    }
                }
                Err(e) => return Err(format!("The Next couldn't be read: {e}")),
            }
        }
        Ok(Some(got))
    }

    /// The next message, if one comes within `wait`.
    pub fn receive(&mut self, wait: Duration) -> Result<Option<Message>, String> {
        let Some(length) = self.read_exact(4, wait)? else {
            return Ok(None);
        };
        let length = u32::from_le_bytes([length[0], length[1], length[2], length[3]]) as usize;
        if length == 0 || length > 0x20000 {
            return Err(format!("The Next sent a message of {length} bytes, which isn't right."));
        }
        let body = self.read_exact(length, Duration::from_secs(5))?.ok_or("The Next stopped partway through a message.")?;
        Ok(Some(if body[0] == 0 {
            Message::Notification { id: body.get(1).copied().unwrap_or(0), payload: body.get(2..).unwrap_or(&[]).to_vec() }
        } else {
            Message::Response { seq: body[0] & 0x0F, payload: body[1..].to_vec() }
        }))
    }

    /// Sends a command and waits for its response, keeping a stop that comes on the way.
    pub fn command(&mut self, command: u8, payload: &[u8]) -> Result<Vec<u8>, String> {
        self.seq = self.seq % 15 + 1;
        let mut message = (payload.len() as u32).to_le_bytes().to_vec();
        message.push(self.seq);
        message.push(command);
        message.extend_from_slice(payload);
        self.link.write_all(&message).map_err(|e| format!("The Next couldn't be told: {e}"))?;
        self.link.flush().map_err(|e| e.to_string())?;
        let began = Instant::now();
        while began.elapsed() < Duration::from_secs(10) {
            match self.receive(Duration::from_millis(200))? {
                Some(Message::Response { seq, payload }) if seq == self.seq => return Ok(payload),
                Some(Message::Notification { id: NTF_PAUSE, payload }) => self.paused = Some(payload),
                _ => {}
            }
        }
        Err("The Next stopped answering. Is dezogif running (press its NMI button), and the cable at 921600 baud?".into())
    }

    /// Says hello: the Next must be a Next.
    pub fn init(&mut self) -> Result<String, String> {
        let mut payload = VERSION.to_vec();
        payload.extend_from_slice(b"Limoniter\0");
        let said = self.command(CMD_INIT, &payload)?;
        if said.first() != Some(&0) {
            return Err("dezogif refused to start.".into());
        }
        let version = said.get(1..4).map(|v| format!("{}.{}.{}", v[0], v[1], v[2])).unwrap_or_default();
        if said.get(1) != Some(&VERSION[0]) {
            return Err(format!("dezogif speaks DZRP {version}, and Limoniter {}.{}.{}.", VERSION[0], VERSION[1], VERSION[2]));
        }
        let name: String = said.get(5..).unwrap_or(&[]).iter().take_while(|b| **b != 0).map(|b| char::from(*b)).collect();
        Ok(format!("{name} (DZRP {version})"))
    }

    pub fn registers(&mut self) -> Result<Registers, String> {
        let r = self.command(CMD_GET_REGISTERS, &[])?;
        let word = |at: usize| r.get(at..at + 2).map_or(0, |b| u16::from_le_bytes([b[0], b[1]]));
        Ok(Registers { pc: word(0), sp: word(2), af: word(4), bc: word(6), de: word(8), hl: word(10), ix: word(12), iy: word(14), af2: word(16), bc2: word(18), de2: word(20), hl2: word(22), r: r.get(24).copied().unwrap_or(0), i: r.get(25).copied().unwrap_or(0), im: r.get(26).copied().unwrap_or(0), iff: false })
    }

    pub fn set_register(&mut self, id: u8, value: u16) -> Result<(), String> {
        let mut payload = vec![id];
        payload.extend(value.to_le_bytes());
        self.command(CMD_SET_REGISTER, &payload).map(|_| ())
    }

    /// The 64K as the Z80 sees it now, a piece at a time.
    pub fn memory(&mut self) -> Result<Vec<u8>, String> {
        let mut memory = Vec::with_capacity(65536);
        for start in (0..0x10000usize).step_by(CHUNK) {
            let mut payload = (CHUNK as u32).to_le_bytes().to_vec();
            payload.extend((start as u16).to_le_bytes());
            payload.extend((CHUNK as u16).to_le_bytes());
            let got = self.command(CMD_READ_MEM_BLOCKS, &payload)?;
            memory.extend_from_slice(&got[..got.len().min(CHUNK)]);
            memory.resize(start + CHUNK, 0);
        }
        Ok(memory)
    }

    /// Writes an 8K bank, a piece at a time.
    pub fn write_bank(&mut self, bank: u8, bytes: &[u8]) -> Result<(), String> {
        for (n, piece) in bytes.chunks(CHUNK).enumerate() {
            let mut payload = vec![bank];
            payload.extend(((n * CHUNK) as u16).to_le_bytes());
            payload.extend_from_slice(piece);
            self.command(CMD_WRITE_BANK_MEM, &payload)?;
        }
        Ok(())
    }

    /// Puts an 8K bank in a slot of the 64K.
    pub fn set_slot(&mut self, slot: u8, bank: u8) -> Result<(), String> {
        let said = self.command(CMD_SET_SLOT, &[slot, bank])?;
        if said.first().is_some_and(|e| *e != 0) {
            return Err(format!("The Next couldn't put bank {bank} in slot {slot}."));
        }
        Ok(())
    }

    /// Writes RST 0 at each address; what was there, to put back.
    pub fn set_breakpoints(&mut self, addresses: &[u16]) -> Result<Vec<u8>, String> {
        if addresses.is_empty() {
            return Ok(Vec::new());
        }
        let payload: Vec<u8> = addresses.iter().flat_map(|a| [a.to_le_bytes()[0], a.to_le_bytes()[1], 0]).collect();
        let said = self.command(CMD_SET_BREAKPOINTS, &payload)?;
        Ok(said.iter().take(addresses.len()).copied().collect())
    }

    /// Puts back what breakpoints were written over.
    pub fn restore(&mut self, addresses: &[u16], originals: &[u8]) -> Result<(), String> {
        if addresses.is_empty() {
            return Ok(());
        }
        let payload: Vec<u8> = addresses.iter().zip(originals).flat_map(|(a, v)| [a.to_le_bytes()[0], a.to_le_bytes()[1], 0, *v]).collect();
        self.command(CMD_RESTORE_MEM, &payload).map(|_| ())
    }

    /// Runs on, with up to two breakpoints of its own for a step.
    pub fn carry_on(&mut self, one: Option<u16>, two: Option<u16>) -> Result<(), String> {
        let mut payload = Vec::new();
        for at in [one, two] {
            payload.push(u8::from(at.is_some()));
            payload.extend(at.unwrap_or(0).to_le_bytes());
        }
        payload.extend([0, 0, 0, 0, 0]);
        self.paused = None;
        self.command(CMD_CONTINUE, &payload).map(|_| ())
    }

    pub fn pause(&mut self) -> Result<(), String> {
        self.command(CMD_PAUSE, &[]).map(|_| ())
    }

    pub fn close(&mut self) {
        let _ = self.command(CMD_CLOSE, &[]);
    }
}

/// Why it stopped, from NTF_PAUSE's payload: the reason and the address.
pub fn pause_reason(payload: &[u8]) -> Option<String> {
    let address = payload.get(1..3).map_or(0, |b| u16::from_le_bytes([b[0], b[1]]));
    let words: String = payload.get(4..).unwrap_or(&[]).iter().take_while(|b| **b != 0).map(|b| char::from(*b)).collect();
    match payload.first()? {
        0 | 2 => None,
        1 => Some("Paused".into()),
        3 => Some(format!("Stopped reading ${address:04X}")),
        4 => Some(format!("Stopped after writing to ${address:04X}")),
        _ => Some(if words.is_empty() { "Stopped".into() } else { words }),
    }
}

/// Where a Z80 instruction can go next: the instruction after it, and where it may jump instead. With `over`, a
/// CALL, RST or the repeating instructions (LDIR and the like, DJNZ) go only to what follows them.
pub fn z80_next(memory: &dyn Fn(u16) -> u8, r: &Registers, over: bool) -> (u16, Option<u16>) {
    let pc = r.pc;
    let at = |o: u16| memory(pc.wrapping_add(o));
    let word = |o: u16| u16::from_le_bytes([at(o), at(o + 1)]);
    let op = at(0);
    let length = z80_length(memory, pc);
    let next = pc.wrapping_add(length);
    let relative = |o: u16| next.wrapping_add(at(o) as i8 as u16);
    let stacked = u16::from_le_bytes([memory(r.sp), memory(r.sp.wrapping_add(1))]);
    let target = match op {
        0xC3 => Some(word(1)),
        0xC2 | 0xCA | 0xD2 | 0xDA | 0xE2 | 0xEA | 0xF2 | 0xFA => Some(word(1)),
        0x18 | 0x20 | 0x28 | 0x30 | 0x38 => Some(relative(1)),
        0x10 if !over => Some(relative(1)),
        0xCD | 0xC4 | 0xCC | 0xD4 | 0xDC | 0xE4 | 0xEC | 0xF4 | 0xFC if !over => Some(word(1)),
        op if op & 0xC7 == 0xC7 && !over => Some(u16::from(op & 0x38)),
        0xC9 | 0xC0 | 0xC8 | 0xD0 | 0xD8 | 0xE0 | 0xE8 | 0xF0 | 0xF8 => Some(stacked),
        0xE9 => Some(r.hl),
        0xDD if at(1) == 0xE9 => Some(r.ix),
        0xFD if at(1) == 0xE9 => Some(r.iy),
        // RETN, RETI; and LDIR and the like, which repeat themselves until done.
        0xED if matches!(at(1), 0x45 | 0x4D | 0x55 | 0x5D | 0x65 | 0x6D | 0x75 | 0x7D) => Some(stacked),
        0xED if matches!(at(1), 0xB0..=0xB3 | 0xB8..=0xBB) && !over => Some(pc),
        _ => None,
    };
    (next, target.filter(|t| *t != next))
}

/// How many bytes a Z80 instruction takes, the Next's included.
pub fn z80_length(memory: &dyn Fn(u16) -> u8, pc: u16) -> u16 {
    let at = |o: u16| memory(pc.wrapping_add(o));
    match at(0) {
        0xCB => 2,
        0xED => match at(1) {
            // LD (nn),rr and LD rr,(nn); and the Next's NEXTREG n,n, PUSH nn, ADD rr,nn, NEXTREG n,A and TEST n.
            0x43 | 0x4B | 0x53 | 0x5B | 0x63 | 0x6B | 0x73 | 0x7B | 0x91 | 0x8A | 0x34..=0x36 => 4,
            0x92 | 0x27 => 3,
            _ => 2,
        },
        // IX and IY: the instruction after the prefix, with a displacement where it uses (HL).
        0xDD | 0xFD => match at(1) {
            0xCB => 4,
            0xDD | 0xFD | 0xED => 1,
            op => 1 + plain_length(op) + u16::from(uses_hl_memory(op)),
        },
        op => plain_length(op),
    }
}

/// An unprefixed instruction's length.
fn plain_length(op: u8) -> u16 {
    match op {
        0x01 | 0x11 | 0x21 | 0x31 | 0x22 | 0x2A | 0x32 | 0x3A => 3,
        0xC2 | 0xC3 | 0xCA | 0xD2 | 0xDA | 0xE2 | 0xEA | 0xF2 | 0xFA => 3,
        0xC4 | 0xCC | 0xCD | 0xD4 | 0xDC | 0xE4 | 0xEC | 0xF4 | 0xFC => 3,
        0x06 | 0x0E | 0x16 | 0x1E | 0x26 | 0x2E | 0x36 | 0x3E => 2,
        0x10 | 0x18 | 0x20 | 0x28 | 0x30 | 0x38 => 2,
        0xC6 | 0xCE | 0xD6 | 0xDE | 0xE6 | 0xEE | 0xF6 | 0xFE | 0xD3 | 0xDB => 2,
        _ => 1,
    }
}

/// Whether an instruction reads or writes (HL), which with IX or IY becomes (IX+d).
fn uses_hl_memory(op: u8) -> bool {
    match op {
        0x34..=0x36 => true,
        0x76 => false,
        0x40..=0x7F => op & 7 == 6 || op & 0xF8 == 0x70,
        0x80..=0xBF => op & 7 == 6,
        _ => false,
    }
}

/// A program's code put in the Next's banks, as a .nex would load it: 16K banks 5, 2 and 0 at $4000, $8000 and
/// $C000, each two 8K banks, with the slots set to them. `image` is the 64K, and `used` which 16K banks have code.
pub fn load_program<T: Read + Write>(dzrp: &mut Dzrp<T>, image: &[u8], used: &[bool; 4], pc: u16, sp: u16) -> Result<(), String> {
    for (quarter, bank16) in [(1usize, 5u8), (2, 2), (3, 0)] {
        if used[quarter] {
            let bytes = &image[quarter * 0x4000..quarter * 0x4000 + 0x4000];
            dzrp.write_bank(bank16 * 2, &bytes[..0x2000])?;
            dzrp.write_bank(bank16 * 2 + 1, &bytes[0x2000..])?;
        }
        dzrp.set_slot(quarter as u8 * 2, bank16 * 2)?;
        dzrp.set_slot(quarter as u8 * 2 + 1, bank16 * 2 + 1)?;
    }
    dzrp.set_register(1, sp)?;
    dzrp.set_register(0, pc)
}

#[cfg(test)]
pub mod tests {
    use super::*;
    use std::collections::{BTreeMap, VecDeque};

    /// A stand-in for dezogif, as the protocol document has it: 64K of memory, the registers, RST 0 breakpoints,
    /// and a pretend Z80 that only knows the instructions the tests use.
    #[derive(Default)]
    pub struct FakeNext {
        pub memory: Vec<u8>,
        pub banks: BTreeMap<u8, Vec<u8>>,
        pub slots: [u8; 8],
        pub pc: u16,
        pub sp: u16,
        pub a: u8,
        incoming: Vec<u8>,
        outgoing: VecDeque<u8>,
        breaks: Vec<u16>,
    }

    impl FakeNext {
        pub fn new() -> Self {
            FakeNext { memory: vec![0; 65536], ..Default::default() }
        }

        fn reply(&mut self, seq: u8, payload: &[u8]) {
            self.outgoing.extend(((payload.len() + 1) as u32).to_le_bytes());
            self.outgoing.push_back(seq);
            self.outgoing.extend(payload);
        }

        fn pause(&mut self, reason: u8) {
            let body = [0, NTF_PAUSE, reason, self.pc as u8, (self.pc >> 8) as u8, 0, 0];
            self.outgoing.extend((body.len() as u32).to_le_bytes());
            self.outgoing.extend(body);
        }

        /// Runs from PC until an RST 0 (a breakpoint) or one of `extra`: LD A,n; INC A; CALL nn; RET; JR e; NOP.
        fn run(&mut self, extra: &[u16]) {
            for _ in 0..10_000 {
                if extra.contains(&self.pc) || self.memory[self.pc as usize] == 0xC7 {
                    return self.pause(if extra.contains(&self.pc) { 0 } else { 2 });
                }
                let op = self.memory[self.pc as usize];
                let n = self.memory[self.pc.wrapping_add(1) as usize];
                let nn = u16::from_le_bytes([n, self.memory[self.pc.wrapping_add(2) as usize]]);
                self.pc = match op {
                    0x3E => {
                        self.a = n;
                        self.pc + 2
                    }
                    0x3C => {
                        self.a = self.a.wrapping_add(1);
                        self.pc + 1
                    }
                    0xCD => {
                        self.sp -= 2;
                        let back = (self.pc + 3).to_le_bytes();
                        self.memory[self.sp as usize] = back[0];
                        self.memory[self.sp as usize + 1] = back[1];
                        nn
                    }
                    0xC9 => {
                        let back = u16::from_le_bytes([self.memory[self.sp as usize], self.memory[self.sp as usize + 1]]);
                        self.sp += 2;
                        back
                    }
                    0x18 => self.pc.wrapping_add(2).wrapping_add(n as i8 as u16),
                    _ => self.pc + 1,
                };
            }
        }

        fn handle(&mut self, seq: u8, command: u8, p: &[u8]) {
            match command {
                CMD_INIT => self.reply(seq, &[0, 2, 2, 0, 4, b'd', b'e', b'z', b'o', b'g', b'i', b'f', 0]),
                CMD_GET_REGISTERS => {
                    let mut r = Vec::new();
                    for w in [self.pc, self.sp, u16::from(self.a) << 8, 0, 0, 0, 0, 0, 0, 0, 0, 0] {
                        r.extend(w.to_le_bytes());
                    }
                    r.extend([0, 0, 1, 0, 8]);
                    r.extend(self.slots);
                    self.reply(seq, &r);
                }
                CMD_SET_REGISTER => {
                    let value = u16::from_le_bytes([p[1], p[2]]);
                    match p[0] {
                        0 => self.pc = value,
                        1 => self.sp = value,
                        _ => {}
                    }
                    self.reply(seq, &[]);
                }
                CMD_WRITE_BANK_MEM => {
                    let bank = self.banks.entry(p[0]).or_insert_with(|| vec![0; 0x2000]);
                    let offset = usize::from(u16::from_le_bytes([p[1], p[2]]));
                    bank[offset..offset + p.len() - 3].copy_from_slice(&p[3..]);
                    self.reply(seq, &[]);
                }
                CMD_SET_SLOT => {
                    self.slots[p[0] as usize] = p[1];
                    // The 64K follows what's in the slots.
                    if let Some(bank) = self.banks.get(&p[1]) {
                        let at = usize::from(p[0]) * 0x2000;
                        self.memory[at..at + 0x2000].copy_from_slice(bank);
                    }
                    self.reply(seq, &[0]);
                }
                CMD_READ_MEM_BLOCKS => {
                    let at = usize::from(u16::from_le_bytes([p[4], p[5]]));
                    let size = usize::from(u16::from_le_bytes([p[6], p[7]]));
                    let bytes = self.memory[at..at + size].to_vec();
                    self.reply(seq, &bytes);
                }
                CMD_SET_BREAKPOINTS => {
                    let mut originals = Vec::new();
                    for b in p.chunks(3) {
                        let at = u16::from_le_bytes([b[0], b[1]]);
                        originals.push(self.memory[at as usize]);
                        self.memory[at as usize] = 0xC7;
                        self.breaks.push(at);
                    }
                    self.reply(seq, &originals);
                }
                CMD_RESTORE_MEM => {
                    for b in p.chunks(4) {
                        let at = u16::from_le_bytes([b[0], b[1]]);
                        if self.memory[at as usize] == 0xC7 {
                            self.memory[at as usize] = b[3];
                        }
                    }
                    self.reply(seq, &[]);
                }
                CMD_CONTINUE => {
                    self.reply(seq, &[]);
                    let extra: Vec<u16> = [(0, 1), (3, 4)].iter().filter(|(e, _)| p[*e] == 1).map(|(_, a)| u16::from_le_bytes([p[*a], p[*a + 1]])).collect();
                    // Off a breakpoint first: dezogif runs the instruction that was there.
                    let here = self.pc;
                    if self.memory[here as usize] == 0xC7 && self.breaks.contains(&here) {
                        self.pc = here.wrapping_add(1);
                    }
                    self.run(&extra);
                }
                CMD_PAUSE => {
                    self.reply(seq, &[]);
                    self.pause(1);
                }
                _ => self.reply(seq, &[]),
            }
        }
    }

    impl Read for FakeNext {
        fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
            if self.outgoing.is_empty() {
                return Err(std::io::ErrorKind::WouldBlock.into());
            }
            let n = buf.len().min(self.outgoing.len());
            for b in buf.iter_mut().take(n) {
                *b = self.outgoing.pop_front().unwrap_or(0);
            }
            Ok(n)
        }
    }

    impl Write for FakeNext {
        fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
            self.incoming.extend_from_slice(buf);
            while self.incoming.len() >= 6 {
                let length = u32::from_le_bytes([self.incoming[0], self.incoming[1], self.incoming[2], self.incoming[3]]) as usize;
                if self.incoming.len() < 6 + length {
                    break;
                }
                let message: Vec<u8> = self.incoming.drain(..6 + length).collect();
                self.handle(message[4], message[5], &message[6..]);
            }
            Ok(buf.len())
        }

        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    #[test]
    fn z80_instructions_are_measured_and_followed() {
        let code = |bytes: &[u8]| {
            let mut m = vec![0u8; 65536];
            m[0x8000..0x8000 + bytes.len()].copy_from_slice(bytes);
            m
        };
        let r = Registers { pc: 0x8000, sp: 0xFF00, hl: 0x1234, ..Registers::default() };
        for (bytes, length) in [(&[0x3Eu8, 7][..], 2), (&[0x21, 0, 0x40], 3), (&[0xCB, 0x27], 2), (&[0xDD, 0x36, 2, 5], 4), (&[0xFD, 0xCB, 2, 0x46], 4), (&[0xED, 0xB0], 2), (&[0xED, 0x91, 7, 3], 4), (&[0xED, 0x5B, 0, 0x80], 4), (&[0xDD, 0x7E, 3], 3), (&[0x00], 1)] {
            let m = code(bytes);
            assert_eq!(z80_length(&|a| m[a as usize], 0x8000), length, "{bytes:02X?}");
        }
        let m = code(&[0xCD, 0x00, 0x90]);
        assert_eq!(z80_next(&|a| m[a as usize], &r, false), (0x8003, Some(0x9000)), "into the CALL");
        assert_eq!(z80_next(&|a| m[a as usize], &r, true), (0x8003, None), "over it");
        let m = code(&[0x18, 0xFE]);
        assert_eq!(z80_next(&|a| m[a as usize], &r, false), (0x8002, Some(0x8000)), "JR to itself");
        let mut m = code(&[0xC9]);
        m[0xFF00] = 0x34;
        m[0xFF01] = 0x12;
        assert_eq!(z80_next(&|a| m[a as usize], &r, false), (0x8001, Some(0x1234)), "RET to what's on the stack");
        let m = code(&[0xE9]);
        assert_eq!(z80_next(&|a| m[a as usize], &r, false).1, Some(0x1234), "JP (HL)");
    }

    #[test]
    fn every_z80_and_next_instruction_is_measured_right() {
        for (name, text) in [("z80_all.asm", include_str!("asm/testdata/z80_all.asm")), ("z80n_all.asm", include_str!("asm/testdata/z80n_all.asm"))] {
            let assembly = crate::asm::assemble(&crate::asm::z80::Z80, std::path::Path::new(name), text, &|_| Err("no".into()));
            let mut checked = 0;
            for placed in assembly.lines.iter().filter(|p| !p.bytes.is_empty() && p.bytes.len() <= 4) {
                let bytes = &placed.bytes;
                let line = text.lines().nth(placed.line).unwrap_or("");
                let first = line.split_whitespace().next().unwrap_or("").to_ascii_lowercase();
                if !line.trim_start().starts_with(|c: char| c.is_ascii_alphabetic()) || crate::asm::DIRECTIVES.contains(&first.as_str()) {
                    continue;
                }
                assert_eq!(z80_length(&|a| bytes.get(usize::from(a)).copied().unwrap_or(0), 0), bytes.len() as u16, "{name} line {}: {line} {bytes:02X?}", placed.line + 1);
                checked += 1;
            }
            assert!(checked > if name == "z80_all.asm" { 500 } else { 25 }, "{name}: {checked}");
        }
    }

    #[test]
    fn a_program_is_loaded_debugged_and_stepped_through_dezogif() {
        let mut dzrp = Dzrp::new(FakeNext::new());
        assert_eq!(dzrp.init().unwrap(), "dezogif (DZRP 2.2.0)");
        // org $8000: LD A,1; CALL sub; INC A; loop: JR loop; sub: INC A; RET
        let mut image = vec![0u8; 65536];
        image[0x8000..0x800B].copy_from_slice(&[0x3E, 1, 0xCD, 0x08, 0x80, 0x3C, 0x18, 0xFE, 0x3C, 0xC9, 0]);
        load_program(&mut dzrp, &image, &[false, false, true, false], 0x8000, 0xFFF0).unwrap();
        let memory = dzrp.memory().unwrap();
        assert_eq!(memory[0x8000..0x8003], [0x3E, 1, 0xCD], "loaded into bank 2's 8K banks 4 and 5, in slots 4 and 5");
        assert_eq!(dzrp.registers().unwrap().pc, 0x8000);
        // A breakpoint at the INC A after the CALL.
        let breakpoints = [0x8005];
        let originals = dzrp.set_breakpoints(&breakpoints).unwrap();
        assert_eq!(originals, [0x3C]);
        dzrp.carry_on(None, None).unwrap();
        let stop = (0..50).find_map(|_| match dzrp.receive(Duration::from_millis(10)).unwrap() {
            Some(Message::Notification { id: NTF_PAUSE, payload }) => Some(payload),
            _ => dzrp.paused.take(),
        });
        assert_eq!(stop.as_deref().and_then(|p| p.first()), Some(&2), "stopped at a breakpoint");
        dzrp.restore(&breakpoints, &originals).unwrap();
        let r = dzrp.registers().unwrap();
        assert_eq!((r.pc, r.af >> 8), (0x8005, 2), "LD A,1 and the subroutine's INC A have run");
        assert_eq!(dzrp.memory().unwrap()[0x8005], 0x3C, "the breakpoint's byte put back");
        // A step: breakpoints after this instruction and where it may jump.
        let memory = dzrp.memory().unwrap();
        let (next, jump) = z80_next(&|a| memory[a as usize], &r, false);
        dzrp.carry_on(Some(next), jump).unwrap();
        let stopped = (0..50).any(|_| matches!(dzrp.receive(Duration::from_millis(10)), Ok(Some(Message::Notification { .. }))) || dzrp.paused.take().is_some());
        assert!(stopped);
        let r = dzrp.registers().unwrap();
        assert_eq!((r.pc, r.af >> 8), (0x8006, 3), "one instruction on");
        dzrp.pause().unwrap();
        assert_eq!(pause_reason(&[1, 0, 0x80, 0, 0]).as_deref(), Some("Paused"));
        assert_eq!(pause_reason(&[4, 0x00, 0x58, 0, 0]).as_deref(), Some("Stopped after writing to $5800"));
    }
}
