//! The Amiga debugger: its pieces, a whole session against a GDB server made here (running enough of a 68000 for
//! a small program), and, where one is installed, against a real emulator.

use super::*;
use crate::remote_debug::RemoteDebug;
use std::net::TcpListener;
use std::sync::{Arc, Mutex};

/// A program with a subroutine of its own, a call into the ROM, and a loop that never ends. Line numbers are
/// counted from 0, as the editor does.
pub const SOURCE: &str = "\tsection\tcode,code
start:\tmoveq\t#1,d0
\tmoveq\t#2,d1
\tbsr.w\tsub
\tjsr\t$f80000
\tmoveq\t#3,d2
\tbsr.w\tsub
\tmoveq\t#4,d4
loop:\tbra.s\tloop
sub:\tmoveq\t#7,d7
\trts
\tsection\tdata,data
value:\tdc.l\t$12345678
\tdc.w\t1,2
\tdc.w\t3,4
\tdc.b\t5,6
";

/// Where the made-up Amiga loads the program's two hunks.
pub const BASES: [u32; 2] = [0x00C0_1000, 0x00C2_0040];

pub fn assembled() -> crate::asm::Assembly {
    let (assembly, file) = crate::asm::hunk::assemble_executable(std::path::Path::new("t.s"), SOURCE, &|_| Err("no".into()), false, &crate::asm::Options::default());
    assert!(assembly.ok() && file.is_some(), "{:?}", assembly.problems);
    assembly
}

/// The address a line's code is at, once loaded.
pub fn line_address(assembly: &crate::asm::Assembly, line: usize) -> u32 {
    Program::of(assembly).to_runtime(&BASES, assembly.address_of(0, line).unwrap()).unwrap()
}

/// What the made-up server was sent, for the test to look at.
pub type Heard = Arc<Mutex<Vec<String>>>;

/// A GDB server on a port of its own, for one connection, like the UAEs': it answers nothing until the program
/// "starts" (a moment after connecting), then is stopped at its first instruction. It runs enough 68000 for
/// `SOURCE`: MOVEQ, BSR, JSR to an absolute address, RTS, BRA.S and NOP, with a ROM routine at $F80000 setting D3
/// to 9. It prints kprintf text as O packets before each stop, as the UAEs do.
pub fn fake_server(assembly: &crate::asm::Assembly) -> (u16, Heard) {
    let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
    let port = listener.local_addr().unwrap().port();
    let program = Program::of(assembly);
    let heard: Heard = Arc::new(Mutex::new(Vec::new()));
    let log = heard.clone();
    std::thread::spawn(move || {
        let Ok((stream, _)) = listener.accept() else { return };
        let mut machine = Machine::new(&program);
        machine.serve(stream, &log);
    });
    (port, heard)
}

struct Machine {
    memory: Vec<u8>,
    registers: [u32; 18],
    breakpoints: Vec<u32>,
    acks: bool,
}

impl Machine {
    fn new(program: &Program) -> Machine {
        let mut memory = vec![0u8; 0x0100_0000];
        for (image, base) in program.images.iter().zip(BASES) {
            memory[base as usize..base as usize + image.len()].copy_from_slice(image);
        }
        memory[0xF8_0000..0xF8_0006].copy_from_slice(&[0x76, 0x09, 0x4E, 0x71, 0x4E, 0x75]);
        let mut registers = [0u32; 18];
        registers[A7] = 0x0001_0000;
        registers[PC] = BASES[0];
        registers[SR] = 0x0000;
        Machine { memory, registers, breakpoints: Vec::new(), acks: true }
    }

    fn word(&self, at: u32) -> u16 {
        u16::from_be_bytes([self.memory[at as usize], self.memory[at as usize + 1]])
    }

    fn long(&self, at: u32) -> u32 {
        u32::from(self.word(at)) << 16 | u32::from(self.word(at + 2))
    }

    fn push(&mut self, value: u32) {
        self.registers[A7] -= 4;
        let at = self.registers[A7] as usize;
        self.memory[at..at + 4].copy_from_slice(&value.to_be_bytes());
    }

    /// Runs one instruction; false at one it doesn't know.
    fn step(&mut self) -> bool {
        let pc = self.registers[PC];
        let op = self.word(pc);
        let next = match op {
            0x4E71 => pc + 2,
            0x4E75 => {
                let back = self.long(self.registers[A7]);
                self.registers[A7] += 4;
                back
            }
            0x4EB9 => {
                self.push(pc + 6);
                self.long(pc + 2)
            }
            0x6100 => {
                self.push(pc + 4);
                pc.wrapping_add(2).wrapping_add(self.word(pc + 2) as i16 as u32)
            }
            _ if op & 0xFF00 == 0x6000 => pc.wrapping_add(2).wrapping_add((op & 0xFF) as i8 as u32),
            _ if op & 0xF100 == 0x7000 => {
                self.registers[usize::from(op >> 9 & 7)] = (op & 0xFF) as i8 as i32 as u32;
                pc + 2
            }
            _ => return false,
        };
        self.registers[PC] = next;
        true
    }

    fn serve(&mut self, mut stream: TcpStream, heard: &Heard) {
        stream.set_read_timeout(Some(Duration::from_millis(5))).unwrap();
        let mut pending: Vec<u8> = Vec::new();
        let mut started = false;
        let connected = Instant::now();
        let send = |stream: &mut TcpStream, data: &str| {
            let sum = data.bytes().fold(0u8, |a, b| a.wrapping_add(b));
            let _ = stream.write_all(format!("${data}#{sum:02x}").as_bytes());
        };
        loop {
            let mut buffer = [0u8; 4096];
            match stream.read(&mut buffer) {
                Ok(0) => return,
                Ok(got) => pending.extend_from_slice(&buffer[..got]),
                Err(e) if matches!(e.kind(), std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut) => {}
                Err(_) => return,
            }
            // Nothing is answered until the Amiga has "booted".
            if !started {
                if connected.elapsed() < Duration::from_millis(400) {
                    continue;
                }
                started = true;
            }
            while let Some(start) = pending.iter().position(|b| *b == b'$') {
                let Some(hash) = pending[start..].iter().position(|b| *b == b'#').map(|h| start + h) else { break };
                if pending.len() < hash + 3 {
                    break;
                }
                let packet = String::from_utf8_lossy(&pending[start + 1..hash]).into_owned();
                pending.drain(..hash + 3);
                if self.acks {
                    let _ = stream.write_all(b"+");
                }
                heard.lock().unwrap().push(packet.clone());
                let reply = self.answer(&packet, &mut stream, &mut pending);
                if let Some(reply) = reply {
                    send(&mut stream, &reply);
                }
            }
            pending.retain(|b| *b != b'+' && *b != 3);
        }
    }

    /// The answer to a packet; None for one that has none (yet).
    fn answer(&mut self, packet: &str, stream: &mut TcpStream, pending: &mut Vec<u8>) -> Option<String> {
        let hex = |s: &str| u32::from_str_radix(s, 16).unwrap_or(0);
        let output = |stream: &mut TcpStream| {
            let text: String = b"DBG: hello\n".iter().map(|b| format!("{b:02x}")).collect();
            let data = format!("O{text}");
            let sum = data.bytes().fold(0u8, |a, b| a.wrapping_add(b));
            let _ = stream.write_all(format!("${data}#{sum:02x}").as_bytes());
        };
        Some(match packet {
            "QStartNoAckMode" => {
                self.acks = false;
                "OK".into()
            }
            "?" => {
                output(stream);
                "S05".into()
            }
            "qOffsets" => format!("{:08x};{:08x}", BASES[0], BASES[1]),
            "g" => self.registers.iter().map(|r| format!("{r:08x}")).collect(),
            "vCont;s" => {
                if !self.step() {
                    return Some("S04".into());
                }
                output(stream);
                "S05".into()
            }
            "vCont;c" => {
                // Runs until a breakpoint, or Ctrl+C.
                let mut count = 0u64;
                loop {
                    if !self.step() {
                        return Some("S04".into());
                    }
                    if self.breakpoints.contains(&self.registers[PC]) {
                        output(stream);
                        return Some("T05swbreak:;".into());
                    }
                    count += 1;
                    if count.is_multiple_of(500) {
                        let mut byte = [0u8; 64];
                        if let Ok(got) = stream.read(&mut byte)
                            && byte[..got].contains(&3)
                        {
                            pending.extend(byte[..got].iter().filter(|b| **b != 3));
                            return Some("S05".into());
                        }
                    }
                }
            }
            _ if packet.starts_with('m') => {
                let (at, length) = packet[1..].split_once(',').unwrap();
                let (at, length) = (hex(at) as usize, hex(length) as usize);
                self.memory.get(at..at + length).map_or("E01".into(), |b| b.iter().map(|b| format!("{b:02x}")).collect())
            }
            _ if packet.starts_with('M') => {
                let (place, data) = packet[1..].split_once(':').unwrap();
                let at = hex(place.split(',').next().unwrap()) as usize;
                for (i, pair) in data.as_bytes().chunks(2).enumerate() {
                    self.memory[at + i] = u8::from_str_radix(std::str::from_utf8(pair).unwrap(), 16).unwrap();
                }
                "OK".into()
            }
            _ if packet.starts_with('P') => {
                let (index, value) = packet[1..].split_once('=').unwrap();
                self.registers[hex(index) as usize] = hex(value);
                "OK".into()
            }
            _ if packet.starts_with("Z0,") => {
                self.breakpoints.push(hex(packet[3..].split(',').next().unwrap()));
                "OK".into()
            }
            _ if packet.starts_with("z0,") => {
                let at = hex(packet[3..].split(',').next().unwrap());
                self.breakpoints.retain(|b| *b != at);
                "OK".into()
            }
            _ => String::new(),
        })
    }
}

/// Waits until the debugger says what's wanted, a problem, or ten seconds pass.
pub fn wait_until(debug: &mut RemoteDebug, what: &dyn Fn(&RemoteDebug) -> bool) {
    let began = Instant::now();
    while began.elapsed() < Duration::from_secs(10) {
        debug.check();
        if what(debug) || debug.problem.is_some() {
            return;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
}

fn stopped(debug: &RemoteDebug) -> Option<AmigaStop> {
    debug.stopped.as_ref().and_then(|s| s.amiga.as_deref()).cloned()
}

#[test]
fn calls_are_known_to_step_over() {
    assert_eq!(call_length(&[0x61, 0x10]), Some(2), "BSR.S");
    assert_eq!(call_length(&[0x61, 0x00, 0x01, 0x00]), Some(4), "BSR.W");
    assert_eq!(call_length(&[0x61, 0xFF, 0, 0, 1, 0]), Some(6), "BSR.L");
    assert_eq!(call_length(&[0x4E, 0xAE, 0xFD, 0xD8]), Some(4), "JSR -552(A6)");
    assert_eq!(call_length(&[0x4E, 0x90]), Some(2), "JSR (A0)");
    assert_eq!(call_length(&[0x4E, 0xB9, 0, 0xF8, 0, 0]), Some(6), "JSR $F80000");
    assert_eq!(call_length(&[0x4E, 0xBA, 0, 8]), Some(4), "JSR label(PC)");
    assert_eq!(call_length(&[0x4E, 0x41]), Some(2), "TRAP #1");
    assert_eq!(call_length(&[0x4E, 0x75]), None, "RTS");
    assert_eq!(call_length(&[0x70, 0x01]), None, "MOVEQ");
    assert_eq!(call_length(&[0x60, 0x10]), None, "BRA isn't a call");
}

#[test]
fn what_the_servers_say_is_understood() {
    assert_eq!(parse_offsets("00c3a248;00c3a320;00c3daf8"), [0xC3_A248, 0xC3_A320, 0xC3_DAF8], "the UAEs: every hunk");
    assert_eq!(parse_offsets("Text=00c00000;Data=00c10000;Bss=00c20000"), [0xC0_0000, 0xC1_0000, 0xC2_0000], "vAmiga's");
    let said = "0000000100f923140000400000c0178800c3e1e0000000000000000000c3b97c00c3b97c00c3e1e000c38d2800c3474800c3a24800f902dc00f9032800c421780000001000c3a248";
    let registers = parse_registers(said).unwrap();
    assert_eq!((registers[0], registers[A7], registers[SR], registers[PC]), (1, 0x00C4_2178, 0x10, 0x00C3_A248));
    assert!(parse_registers("0011").is_none());
    assert_eq!(stop_reason("T05swbreak:;"), None);
    assert_eq!(stop_reason("S05"), None);
    assert_eq!(stop_reason("T05watch:00c3dd50;").as_deref(), Some("Stopped after a write to $00C3DD50"));
    assert!(stop_reason("S0A").unwrap().contains("address error"));
    assert!(stop_reason("S04").unwrap().contains("illegal"));
    assert_eq!(unpack(b"0* 1}]"), "00001}", "run lengths and escapes");
}

#[test]
fn registers_and_flags_are_named() {
    let mut registers = [0u32; 18];
    registers[0] = 0x1234_5678;
    registers[A7] = 0x0007_FFF0;
    registers[SR] = 0x2715;
    assert_eq!(register_value("d0", &registers), Some(0x1234_5678));
    assert_eq!(register_value("D0.w", &registers), Some(0x5678));
    assert_eq!(register_value("d0.b", &registers), Some(0x78));
    assert_eq!(register_value("SP", &registers), Some(0x0007_FFF0));
    assert_eq!(register_value("ccr", &registers), Some(0x15));
    assert_eq!((register_value("XF", &registers), register_value("ZF", &registers), register_value("CF", &registers), register_value("NF", &registers)), (Some(1), Some(1), Some(1), Some(0)));
    assert_eq!(register_value("D8", &registers), None);
    assert_eq!(register_value("loop", &registers), None);
    assert_eq!(status_words(0x2715), "S IPL 7  X-Z-C");
    // As the panel works them out: registers, labels, memory big-endian, and the custom chips by name.
    let mut stop = AmigaStop { registers, memory: vec![(0x100, vec![0x12, 0x34, 0x56, 0x78]), (CUSTOM + 0x180, vec![0x0F, 0x00])], ..Default::default() };
    stop.registers[8] = 0x100;
    let names = |name: &str| register_value(name, &stop.registers);
    let peek = |a: u32| stop.peek(a).unwrap_or(0);
    let wide = crate::debug_panel::Wide { names: &names, peek: &peek };
    let labels: BTreeMap<String, i64> = [("table".to_string(), 0x100)].into();
    let v = |t: &str| crate::debug_panel::eval_wide(t, &wide, Some(&labels));
    assert_eq!(v("long[a0]"), Ok(0x1234_5678));
    assert_eq!(v("word[table+2]"), Ok(0x5678));
    assert_eq!(v("[table+1] == $34 && ZF"), Ok(1));
    assert_eq!(v("d0.w + 1"), Ok(0x5679));
    assert_eq!(stop.word(CUSTOM + 0x180), Some(0x0F00));
    assert!(v("nothing").is_err());
}

#[test]
fn hunks_are_mapped_both_ways() {
    let assembly = assembled();
    let program = Program::of(&assembly);
    assert_eq!(program.hunks.len(), 2);
    let start = assembly.address_of(0, 1).unwrap();
    assert_eq!(program.to_runtime(&BASES, start), Some(BASES[0]));
    assert_eq!(program.to_source(&BASES, BASES[0] + 4), Some(start + 4));
    assert_eq!(assembly.line_at(program.to_source(&BASES, BASES[0] + 4).unwrap()), Some((0, 3)), "BSR is on line 4");
    let value = *assembly.symbols.get("value").unwrap();
    assert_eq!(program.to_runtime(&BASES, value), Some(BASES[1]), "the data hunk");
    assert_eq!(program.to_source(&BASES, BASES[1] + 0x40), None, "past its end");
    assert_eq!(program.to_source(&BASES, 0xF8_0000), None, "the ROM");
    assert!(program.is_line_start(start + 2) && !program.is_line_start(start + 5));
    let moved = relocate_symbols(&assembly.symbols, &program, &BASES);
    assert_eq!(moved.get("value"), Some(&i64::from(BASES[1])));
    assert_eq!(moved.get("sub"), Some(&i64::from(line_address(&assembly, 9))));
    assert_eq!(config_lines("winuae", 2345, "demo"), "debugging_features=gdbserver\r\ndebugging_trigger=demo\r\n");
    assert!(config_lines("fs-uae", 6860, "demo").contains("remote_debugger_port = 6860\nremote_debugger_trigger = demo\n"));
}

#[test]
fn the_call_stack_is_read_from_return_addresses() {
    let assembly = assembled();
    let program = Program::of(&assembly);
    // Inside sub, called from line 4's BSR: its return address (line 5's) is on the stack, under a pushed long
    // that isn't one.
    let mut registers = [0u32; 18];
    registers[A7] = 0x8000;
    let mut stack = 0x1234_5678u32.to_be_bytes().to_vec();
    stack.extend(line_address(&assembly, 4).to_be_bytes());
    let stop = AmigaStop { registers, bases: BASES.to_vec(), memory: vec![(0x8000, stack)], ..Default::default() };
    let calls = call_stack(&stop, &program);
    assert_eq!(calls, [assembly.address_of(0, 3).unwrap()]);
}

/// A whole session against a made-up GDB server: it waits for the program to start, stops at a breakpoint on a
/// source line, steps into a subroutine and back, over a call into the ROM, over a subroutine, changes a
/// register and memory, runs to a line, and is paused in a loop.
#[test]
fn a_program_is_debugged_through_a_gdb_server() {
    let assembly = assembled();
    let (port, heard) = fake_server(&assembly);
    let at = |line| line_address(&assembly, line);
    // The breakpoint is set while the Amiga is still booting.
    let mut debug = RemoteDebug::start_amiga(port, Program::of(&assembly), Vec::new(), || {});
    debug.ask(Ask::Amiga(AmigaAsk::Breakpoints(vec![assembly.address_of(0, 2).unwrap()])));
    let pc_is = |debug: &RemoteDebug, pc: u32| stopped(debug).is_some_and(|s| s.pc() == pc);
    wait_until(&mut debug, &|d| pc_is(d, at(2)));
    let stop = stopped(&debug).unwrap_or_else(|| panic!("{:?}", debug.problem));
    assert_eq!((stop.pc(), stop.registers[0], stop.source), (at(2), 1, assembly.address_of(0, 2)), "at the breakpoint on line 3, after line 2 ran");
    assert_eq!(stop.bases, BASES);
    assert!(stop.has(CUSTOM, CUSTOM_SIZE) && stop.has(stop.registers[A7] & !1, 256), "the custom chips and the stack are read");
    assert!(stop.output.contains("DBG: hello"), "what was printed: {:?}", stop.output);
    let step = |debug: &mut RemoteDebug, ask: Ask, to: u32| {
        debug.ask(ask);
        wait_until(debug, &|d| pc_is(d, to));
        assert_eq!(stopped(debug).map(|s| s.pc()), Some(to), "{:?}", debug.problem);
    };
    step(&mut debug, Ask::Step, at(3));
    step(&mut debug, Ask::Step, at(9));
    step(&mut debug, Ask::Step, at(10));
    step(&mut debug, Ask::Step, at(4));
    assert_eq!(stopped(&debug).unwrap().registers[7], 7, "the subroutine ran");
    // JSR $F80000 goes into the ROM: stepping comes back out after it.
    step(&mut debug, Ask::Step, at(5));
    assert_eq!(stopped(&debug).unwrap().registers[3], 9, "the ROM routine ran");
    step(&mut debug, Ask::StepOver, at(6));
    debug.ask(Ask::Amiga(AmigaAsk::SetRegister(7, 0)));
    wait_until(&mut debug, &|d| stopped(d).is_some_and(|s| s.registers[7] == 0));
    step(&mut debug, Ask::StepOver, at(7));
    assert_eq!(stopped(&debug).unwrap().registers[7], 7, "over the subroutine, which ran");
    // Registers and memory changed.
    debug.ask(Ask::Amiga(AmigaAsk::SetRegister(0, 0x55)));
    wait_until(&mut debug, &|d| stopped(d).is_some_and(|s| s.registers[0] == 0x55));
    assert_eq!(stopped(&debug).unwrap().registers[0], 0x55);
    debug.ask(Ask::Amiga(AmigaAsk::Read(vec![(BASES[1], 4)])));
    wait_until(&mut debug, &|d| stopped(d).is_some_and(|s| s.long(BASES[1]) == Some(0x1234_5678)));
    debug.ask(Ask::Amiga(AmigaAsk::Write(BASES[1], vec![0xAB, 0xCD])));
    wait_until(&mut debug, &|d| stopped(d).is_some_and(|s| s.word(BASES[1]) == Some(0xABCD)));
    assert_eq!(stopped(&debug).unwrap().long(BASES[1]), Some(0xABCD_5678));
    let serial = stopped(&debug).unwrap().serial;
    // Run to the loop's line, with the breakpoints gone.
    debug.ask(Ask::Amiga(AmigaAsk::Breakpoints(Vec::new())));
    step(&mut debug, Ask::Amiga(AmigaAsk::RunTo(assembly.address_of(0, 8).unwrap())), at(8));
    assert!(stopped(&debug).unwrap().serial > serial, "a new stop");
    // Carrying on, it loops for ever, until paused.
    debug.ask(Ask::CarryOn);
    std::thread::sleep(Duration::from_millis(200));
    debug.check();
    assert!(debug.stopped.is_none(), "running");
    debug.ask(Ask::Pause);
    wait_until(&mut debug, &|d| d.stopped.is_some());
    assert_eq!(stopped(&debug).map(|s| s.pc()), Some(at(8)), "paused in the loop: {:?}", debug.problem);
    // A breakpoint set while running stops it there.
    debug.ask(Ask::Amiga(AmigaAsk::Breakpoints(vec![assembly.address_of(0, 8).unwrap()])));
    debug.ask(Ask::CarryOn);
    wait_until(&mut debug, &|d| d.stopped.is_some());
    assert_eq!(stopped(&debug).map(|s| s.pc()), Some(at(8)));
    assert!(debug.problem.is_none(), "{:?}", debug.problem);
    drop(debug);
    std::thread::sleep(Duration::from_millis(300));
    let heard = heard.lock().unwrap().clone();
    assert_eq!(heard.first().map(String::as_str), Some("QStartNoAckMode"));
    assert!(heard.contains(&format!("Z0,{:x},2", at(2))), "{heard:?}");
    assert!(heard.contains(&format!("z0,{:x},2", at(8))), "breakpoints are taken out at the end: {heard:?}");
}

/// A server that never answers (an emulator without one, or a program that never starts) is given up on when the
/// debugging ends, rather than waited for.
#[test]
fn a_server_that_never_answers_is_given_up_on() {
    let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
    let port = listener.local_addr().unwrap().port();
    std::thread::spawn(move || {
        let _held = listener.accept();
        std::thread::sleep(Duration::from_secs(5));
    });
    let mut debug = RemoteDebug::start_amiga(port, Program::default(), Vec::new(), || {});
    std::thread::sleep(Duration::from_millis(300));
    debug.check();
    assert!(debug.stopped.is_none() && debug.problem.is_none(), "still waiting");
    drop(debug);
}

/// With LIMONITER_AMIGA_GDB naming an emulator with a GDB server (uae-dap's FS-UAE, run with a screen such as
/// xvfb-run gives; or Bartman's WinUAE, from WSL, with LIMONITER_AMIGA_GDB_FOLDER a folder Windows can see):
/// a real program, made by Limoniter's own assembler, boots on the emulator's own AROS, stops at a breakpoint on
/// a source line, shows its registers, and steps, into a library call and out of it.
#[test]
#[ignore]
fn a_program_is_debugged_in_a_real_emulator() {
    let Some(emulator) = std::env::var_os("LIMONITER_AMIGA_GDB") else { return };
    let emulator = std::path::PathBuf::from(emulator);
    let winuae = emulator.file_name().is_some_and(|n| n.to_string_lossy().to_lowercase().contains("winuae"));
    assert!(has_gdb_server(&emulator), "{} has no GDB server", emulator.display());
    let source = include_str!("asm/testdata/amiga_hello.s");
    let (assembly, file) = crate::asm::hunk::assemble_executable(std::path::Path::new("hello.s"), source, &|_| Err("no".into()), true, &crate::asm::Options::default());
    let file = file.expect("it assembles");
    let folder = match std::env::var_os("LIMONITER_AMIGA_GDB_FOLDER") {
        Some(folder) => std::path::PathBuf::from(folder).join(format!("run-{}", std::process::id())),
        None => std::env::temp_dir().join(format!("limoniter-amiga-gdb-{}", std::process::id())),
    };
    let _ = std::fs::remove_dir_all(&folder);
    std::fs::create_dir_all(&folder).unwrap();
    let run = crate::running::Runnable { machine: crate::running::Machine::Amiga, format: crate::running::Format::Hunk, name: "hello.exe".into(), bytes: Arc::new(file) };
    let settings = crate::amiga_run::Settings::default();
    let id = if winuae { "winuae" } else { "fs-uae" };
    let config = crate::amiga_run::prepare(id, &folder, &run, &settings).unwrap();
    let port = if winuae { WINUAE_PORT } else { TcpListener::bind(("127.0.0.1", 0)).unwrap().local_addr().unwrap().port() };
    let mut text = std::fs::read_to_string(&config).unwrap();
    // WinUAE, a Windows program, is told Windows's names for the folders.
    let windows = |path: &std::path::Path| String::from_utf8(std::process::Command::new("wslpath").arg("-w").arg(path).output().unwrap().stdout).unwrap().trim().to_string();
    if winuae {
        text = text.replace(&folder.join("dh0").display().to_string(), &windows(&folder.join("dh0")));
    }
    text.push_str(&config_lines(id, port, "hello"));
    std::fs::write(&config, text).unwrap();
    let mut command = std::process::Command::new(&emulator);
    if winuae {
        command.args(["-portable", "-f", &windows(&config)]);
    } else {
        command.arg(&config);
        if let Some(home) = emulator.parent() {
            command.env("LD_LIBRARY_PATH", home);
        }
    }
    if let Some(home) = emulator.parent() {
        command.current_dir(home);
    }
    let mut child = command.stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null()).spawn().unwrap();
    // A breakpoint on line 7 (MOVE.L D0,dosbase), after OpenLibrary.
    let program = Program::of(&assembly);
    let line = |n: usize| assembly.address_of(0, n);
    let mut debug = RemoteDebug::start_amiga(port, program.clone(), vec![line(6).unwrap()], || {});
    let began = Instant::now();
    while began.elapsed() < Duration::from_secs(120) && stopped(&debug).is_none() && debug.problem.is_none() {
        debug.check();
        std::thread::sleep(Duration::from_millis(50));
    }
    let stop = stopped(&debug).unwrap_or_else(|| panic!("it never stopped: {:?}", debug.problem));
    eprintln!("Stopped at {:08X} after {:.1}s; hunks at {:08X?}; D0 {:08X} A6 {:08X}", stop.pc(), began.elapsed().as_secs_f32(), stop.bases, stop.registers[0], stop.registers[14]);
    assert_eq!(stop.source, line(6), "at the breakpoint's line");
    assert_eq!(stop.bases.len(), 3, "code, data and BSS");
    assert_ne!(stop.registers[0], 0, "OpenLibrary gave dos.library");
    assert!(stop.has(CUSTOM, CUSTOM_SIZE), "the custom chips were read");
    // Line by line: BEQ.S, MOVE.L D0,A6, MOVE.L #filename,D1.
    for next in [7, 8, 9] {
        debug.ask(Ask::Step);
        wait_until(&mut debug, &|d| stopped(d).is_some_and(|s| s.source == line(next)));
        assert_eq!(stopped(&debug).and_then(|s| s.source), line(next), "stepped to line {}: {:?}", next + 1, debug.problem);
    }
    assert_eq!(stopped(&debug).unwrap().registers[14], stop.registers[0], "A6 is dos.library");
    // Line 12 (JSR -30(A6), Open) calls into the library: stepping runs it and stops on line 13.
    debug.ask(Ask::Amiga(AmigaAsk::RunTo(line(11).unwrap())));
    wait_until(&mut debug, &|d| stopped(d).is_some_and(|s| s.source == line(11)));
    assert_eq!(stopped(&debug).and_then(|s| s.source), line(11), "ran to line 12: {:?}", debug.problem);
    debug.ask(Ask::Step);
    wait_until(&mut debug, &|d| stopped(d).is_some_and(|s| s.source == line(12)));
    let after = stopped(&debug).unwrap();
    assert_eq!(after.source, line(12), "over Open, onto the next line: {:?}", debug.problem);
    assert_ne!(after.registers[0], 0, "Open gave a file handle");
    // A register changed.
    debug.ask(Ask::Amiga(AmigaAsk::SetRegister(5, 0x1234_5678)));
    wait_until(&mut debug, &|d| stopped(d).is_some_and(|s| s.registers[5] == 0x1234_5678));
    assert_eq!(stopped(&debug).map(|s| s.registers[5]), Some(0x1234_5678));
    // On to the end: the file is written.
    debug.ask(Ask::Amiga(AmigaAsk::Breakpoints(Vec::new())));
    debug.ask(Ask::CarryOn);
    let began = Instant::now();
    let mut written = None;
    while began.elapsed() < Duration::from_secs(30) && written.is_none() {
        written = std::fs::read_to_string(folder.join("dh0/result.txt")).ok().filter(|t| !t.is_empty());
        std::thread::sleep(Duration::from_millis(200));
    }
    drop(debug);
    let _ = child.kill();
    let _ = child.wait();
    if winuae {
        let _ = std::process::Command::new("/mnt/c/Windows/System32/taskkill.exe").args(["/IM", emulator.file_name().unwrap().to_str().unwrap(), "/F"]).output();
    }
    assert_eq!(written.as_deref(), Some("Hello from Limoniter\n"));
    let _ = std::fs::remove_dir_all(&folder);
}
