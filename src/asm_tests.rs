//! Unit tests for assembly: a routine run on the built-in Z80 or 6502 with its registers and memory set first,
//! and what it leaves checked. Each test is written beside the code it tests, as a block of comments:
//!
//! ```text
//! ; test: adds one to each byte
//! ; given: HL=buffer B=3 [buffer]=1,2,$FF
//! ; call: add_ones
//! ; expect: [buffer]=2,3,0 B=0 flags: Z=1
//! ; within: 200 cycles
//! ```
//!
//! or in a file beside the source with `.tests` on the end of its name (`maths.asm.tests`), the same lines with
//! or without the comment marks. `given` and `expect` can be written more than once; `call` can be left out,
//! when the routine is the first label after the test's block; `within` is the most cycles (T-states on the Z80)
//! the routine may take. The file is assembled with Limoniter's own assembler, loaded into an empty 64K, and the
//! routine is called with a return address on the stack that nothing else uses: the run ends when it returns
//! there, or fails when it goes on too long. The cycles counted are the routine's own, up to and including its
//! RET or RTS, without the CALL or JSR a caller would add.
//!
//! The 68000 isn't here: Limoniter has no 68000 to run code on, only its assembler.

use crate::asm::expr::{self, Scope};
use crate::asm::{Assembly, Problem};
use crate::assemblers::Cpu;
use std::path::{Path, PathBuf};

/// How long a routine may run, at most, when its test doesn't say: enough for anything a test is for, and short
/// enough that a routine that never returns fails in a moment.
pub const MOST_CYCLES: u64 = 5_000_000;

/// The processors tests run on.
pub fn runs_on(cpu: Cpu) -> bool {
    matches!(cpu, Cpu::Z80 | Cpu::M6502)
}

/// One `NAME=VALUE` of a `given` or `expect` line.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Setting {
    /// A register, in capitals (`HL`, `A`, `BC'`).
    Register(String, String),
    /// A flag, in capitals (`Z`, `C`, `PV`).
    Flag(String, String),
    /// Memory from an address: its bytes, or its words (`[addr].w=`), low byte first.
    Memory { address: String, values: Vec<String>, words: bool },
}

/// A test as written.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Spec {
    pub name: String,
    /// The file it's written in, and the line its `test:` is on, from 0.
    pub file: PathBuf,
    pub line: usize,
    /// The first line after its block, from 0: where the routine it calls by default is.
    pub after: usize,
    /// Whether it's in the source itself, so `after` means something.
    pub in_source: bool,
    pub given: Vec<Setting>,
    pub call: Option<String>,
    pub expect: Vec<Setting>,
    pub within: Option<u64>,
    /// What couldn't be understood in its lines.
    pub problems: Vec<String>,
}

/// How a test went.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Outcome {
    pub name: String,
    pub file: PathBuf,
    /// From 1, as the panel shows lines.
    pub line: usize,
    pub passed: bool,
    /// The cycles the routine took, if it ran.
    pub cycles: Option<u64>,
    /// What wasn't as expected, or why it couldn't run.
    pub differences: Vec<String>,
}

/// The file beside a source its tests can be kept in.
pub fn tests_file_for(source: &Path) -> PathBuf {
    let mut name = source.file_name().map(|n| n.to_os_string()).unwrap_or_default();
    name.push(".tests");
    source.with_file_name(name)
}

/// A line's comment, if it's only a comment: what follows `;`, `//` or `*` at its start. In a tests file the
/// marks can be left out.
fn comment_of(line: &str, marks_needed: bool) -> Option<&str> {
    let t = line.trim_start();
    if let Some(rest) = t.strip_prefix("//") {
        return Some(rest.trim());
    }
    if let Some(rest) = t.strip_prefix(';').or_else(|| t.strip_prefix('*')) {
        return Some(rest.trim_start_matches(';').trim());
    }
    (!marks_needed).then(|| t.trim())
}

/// A key at the start of a comment (`test:`, `given:`, ...) and what follows it.
fn keyed(comment: &str) -> Option<(&'static str, &str)> {
    let (key, rest) = comment.split_once(':')?;
    let key = key.trim().to_ascii_lowercase();
    ["test", "given", "call", "expect", "within"].into_iter().find(|k| *k == key).map(|k| (k, rest.trim()))
}

/// The tests written in a file: in source, in comment blocks starting `test:`; in a tests file, the same with or
/// without comment marks.
pub fn find(file: &Path, text: &str, in_source: bool) -> Vec<Spec> {
    let mut specs: Vec<Spec> = Vec::new();
    let mut open = false;
    for (index, line) in text.lines().enumerate() {
        let Some(comment) = comment_of(line, in_source) else {
            if let Some(last) = specs.last_mut().filter(|_| open) {
                last.after = index;
            }
            open = false;
            continue;
        };
        match keyed(comment) {
            Some(("test", name)) => {
                if let Some(last) = specs.last_mut().filter(|_| open) {
                    last.after = index;
                }
                let name = if name.is_empty() { crate::i18n::tf!("test on line {}", index + 1) } else { name.to_string() };
                specs.push(Spec { name, file: file.to_path_buf(), line: index, after: index + 1, in_source, given: Vec::new(), call: None, expect: Vec::new(), within: None, problems: Vec::new() });
                open = true;
            }
            Some((key, rest)) if open => {
                let spec = specs.last_mut().expect("a test is open");
                match key {
                    "given" => settings(rest, &mut spec.given, &mut spec.problems),
                    "expect" => settings(rest, &mut spec.expect, &mut spec.problems),
                    "call" => spec.call = Some(rest.to_string()).filter(|r| !r.is_empty()),
                    _ => match rest.split(|c: char| !c.is_ascii_digit() && c != '_').find(|w| !w.is_empty()).and_then(|n| n.replace('_', "").parse().ok()) {
                        Some(n) => spec.within = Some(n),
                        None => spec.problems.push(crate::i18n::tf!("\"within: {rest}\" needs a number of cycles")),
                    },
                }
                spec.after = index + 1;
            }
            // A comment of any other kind, or a blank one, inside a test's block is a note, and the block goes on.
            _ if open => specs.last_mut().expect("a test is open").after = index + 1,
            _ => {}
        }
    }
    specs
}

/// The words of a line, a quoted string kept whole.
fn words(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut word = String::new();
    let mut quote = None;
    for c in text.chars() {
        match quote {
            Some(q) => {
                word.push(c);
                if c == q {
                    quote = None;
                }
            }
            // A ' after a register's name (HL') is the other set's, not a quote.
            None if c == '"' || c == '\'' && (word.is_empty() || word.ends_with(['=', ','])) => {
                quote = Some(c);
                word.push(c);
            }
            None if c.is_whitespace() => {
                if !word.is_empty() {
                    out.push(std::mem::take(&mut word));
                }
            }
            None => word.push(c),
        }
    }
    if !word.is_empty() {
        out.push(word);
    }
    out
}

/// Values separated by commas, a quoted string kept whole.
fn values(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut value = String::new();
    let mut quote = None;
    for c in text.chars() {
        match quote {
            Some(q) => {
                value.push(c);
                if c == q {
                    quote = None;
                }
            }
            None if c == '"' => {
                quote = Some(c);
                value.push(c);
            }
            None if c == ',' => out.push(std::mem::take(&mut value)),
            None => value.push(c),
        }
    }
    out.push(value);
    out.into_iter().map(|v| v.trim().to_string()).collect()
}

/// Reads `A=5 HL=$4000 [$4000]=1,2,3 flags: Z=1 C=0`.
fn settings(text: &str, into: &mut Vec<Setting>, problems: &mut Vec<String>) {
    let mut flags = false;
    for word in words(text) {
        if word.eq_ignore_ascii_case("flags:") || word.eq_ignore_ascii_case("flags") {
            flags = true;
            continue;
        }
        if word.eq_ignore_ascii_case("registers:") {
            flags = false;
            continue;
        }
        let Some((name, value)) = word.split_once('=') else {
            problems.push(crate::i18n::tf!("\"{word}\" should be NAME=VALUE"));
            continue;
        };
        if value.is_empty() {
            problems.push(crate::i18n::tf!("\"{word}\" has no value"));
            continue;
        }
        if let Some(inner) = name.strip_prefix('[') {
            let (address, words) = match inner.strip_suffix("].w").or_else(|| inner.strip_suffix("].W")) {
                Some(address) => (address, true),
                None => (inner.strip_suffix(']').unwrap_or(inner), false),
            };
            into.push(Setting::Memory { address: address.trim().to_string(), values: values(value), words });
        } else if flags {
            into.push(Setting::Flag(name.to_ascii_uppercase(), value.to_string()));
        } else {
            into.push(Setting::Register(name.to_ascii_uppercase(), value.to_string()));
        }
    }
}

/// The assembly's labels, for working out the values in a test.
struct Labels<'a>(&'a Assembly);

impl Scope for Labels<'_> {
    fn value(&self, name: &str) -> Option<i64> {
        self.0.symbols.get(name).copied().or_else(|| self.0.symbols.iter().find(|(k, _)| k.eq_ignore_ascii_case(name)).map(|(_, v)| *v))
    }
    fn here(&self) -> i64 {
        0
    }
    fn full_name(&self, name: &str) -> String {
        name.to_string()
    }
}

/// A value in a test: a number, a label or a sum of them, or a character in quotes.
fn number(text: &str, assembly: &Assembly) -> Result<i64, String> {
    match expr::eval(text, &Labels(assembly)) {
        Ok(expr::Value::Known(v)) => Ok(v),
        Ok(expr::Value::Unknown) => Err(crate::i18n::tf!("\"{text}\" names a label the source doesn't have")),
        Err(e) => Err(crate::i18n::tf!("\"{text}\": {e}")),
    }
}

/// A memory setting's bytes, a string in double quotes giving its characters.
fn bytes_of(values: &[String], words: bool, assembly: &Assembly) -> Result<Vec<u8>, String> {
    let mut out = Vec::new();
    for value in values {
        if let Some(text) = value.strip_prefix('"').and_then(|v| v.strip_suffix('"')) {
            out.extend(text.bytes());
            continue;
        }
        let n = number(value, assembly)?;
        if words {
            out.extend((n as u16).to_le_bytes());
        } else {
            out.push(n as u8);
        }
    }
    Ok(out)
}

/// The processor being tested, its registers named as the test names them.
trait Core {
    /// A register's value and how many bits it has; None if there's no such register.
    fn register(&mut self, name: &str) -> Option<(u32, u32)>;
    fn set_register(&mut self, name: &str, value: u32) -> bool;
    fn flag(&self, name: &str) -> Option<bool>;
    fn set_flag(&mut self, name: &str, on: bool) -> bool;
    fn memory(&mut self) -> &mut Vec<u8>;
    /// Calls a routine, its return to `back`; the cycles it took to get there, or why it didn't.
    fn call(&mut self, routine: u16, back: u16, most: u64) -> Result<u64, (String, u64)>;
}

struct Z80Core {
    cpu: rustzx_z80::Z80,
    bus: crate::sfx::z80::Bus,
}

const Z80_FLAGS: [(&str, u8); 8] = [("S", 0x80), ("Z", 0x40), ("H", 0x10), ("PV", 0x04), ("P", 0x04), ("V", 0x04), ("N", 0x02), ("C", 0x01)];

impl Z80Core {
    fn new(memory: Vec<u8>) -> Z80Core {
        let mut cpu = rustzx_z80::Z80::default();
        cpu.regs.set_sp(0);
        Z80Core { cpu, bus: crate::sfx::z80::Bus { memory, t: 0, outs: Vec::new(), halted: false } }
    }
}

impl Core for Z80Core {
    fn register(&mut self, name: &str) -> Option<(u32, u32)> {
        // The other set is read by swapping it in (rustzx-z80's own getters for H' and L' give H and L).
        match name {
            "AF'" => {
                self.cpu.regs.swap_af_alt();
                let value = self.register("AF");
                self.cpu.regs.swap_af_alt();
                return value;
            }
            "BC'" | "DE'" | "HL'" => {
                self.cpu.regs.exx();
                let value = self.register(name.trim_end_matches('\''));
                self.cpu.regs.exx();
                return value;
            }
            _ => {}
        }
        let r = &self.cpu.regs;
        use rustzx_z80::{RegName8 as R8, RegName16 as R16};
        let eight = |n| Some((u32::from(r.get_reg_8(n)), 8));
        let sixteen = |n| Some((u32::from(r.get_reg_16(n)), 16));
        match name {
            "A" => eight(R8::A),
            "F" => eight(R8::F),
            "B" => eight(R8::B),
            "C" => eight(R8::C),
            "D" => eight(R8::D),
            "E" => eight(R8::E),
            "H" => eight(R8::H),
            "L" => eight(R8::L),
            "IXH" => eight(R8::IXH),
            "IXL" => eight(R8::IXL),
            "IYH" => eight(R8::IYH),
            "IYL" => eight(R8::IYL),
            "I" => eight(R8::I),
            "R" => eight(R8::R),
            "AF" => sixteen(R16::AF),
            "BC" => sixteen(R16::BC),
            "DE" => sixteen(R16::DE),
            "HL" => sixteen(R16::HL),
            "IX" => sixteen(R16::IX),
            "IY" => sixteen(R16::IY),
            "SP" => sixteen(R16::SP),
            _ => None,
        }
    }

    fn set_register(&mut self, name: &str, value: u32) -> bool {
        use rustzx_z80::{RegName8 as R8, RegName16 as R16};
        let r = &mut self.cpu.regs;
        let (b, w) = (value as u8, value as u16);
        match name {
            "A" => r.set_reg_8(R8::A, b),
            "F" => r.set_reg_8(R8::F, b),
            "B" => r.set_reg_8(R8::B, b),
            "C" => r.set_reg_8(R8::C, b),
            "D" => r.set_reg_8(R8::D, b),
            "E" => r.set_reg_8(R8::E, b),
            "H" => r.set_reg_8(R8::H, b),
            "L" => r.set_reg_8(R8::L, b),
            "IXH" => r.set_reg_8(R8::IXH, b),
            "IXL" => r.set_reg_8(R8::IXL, b),
            "IYH" => r.set_reg_8(R8::IYH, b),
            "IYL" => r.set_reg_8(R8::IYL, b),
            "I" => r.set_reg_8(R8::I, b),
            "R" => r.set_reg_8(R8::R, b),
            "AF" => r.set_reg_16(R16::AF, w) as u8,
            "BC" => r.set_reg_16(R16::BC, w) as u8,
            "DE" => r.set_reg_16(R16::DE, w) as u8,
            "HL" => r.set_reg_16(R16::HL, w) as u8,
            "IX" => r.set_reg_16(R16::IX, w) as u8,
            "IY" => r.set_reg_16(R16::IY, w) as u8,
            "SP" => r.set_reg_16(R16::SP, w) as u8,
            // The other set is reached by swapping it in, setting it and swapping it back.
            "AF'" => {
                r.swap_af_alt();
                r.set_reg_16(R16::AF, w);
                r.swap_af_alt();
                0
            }
            "BC'" | "DE'" | "HL'" => {
                r.exx();
                r.set_reg_16([R16::BC, R16::DE, R16::HL][usize::from(name.as_bytes()[0] == b'D') + 2 * usize::from(name.as_bytes()[0] == b'H')], w);
                r.exx();
                0
            }
            _ => return false,
        };
        true
    }

    fn flag(&self, name: &str) -> Option<bool> {
        let bit = Z80_FLAGS.iter().find(|(n, _)| *n == name)?.1;
        Some(self.cpu.regs.get_flags() & bit != 0)
    }

    fn set_flag(&mut self, name: &str, on: bool) -> bool {
        let Some(&(_, bit)) = Z80_FLAGS.iter().find(|(n, _)| *n == name) else {
            return false;
        };
        let f = self.cpu.regs.get_flags();
        self.cpu.regs.set_flags(if on { f | bit } else { f & !bit });
        true
    }

    fn memory(&mut self) -> &mut Vec<u8> {
        &mut self.bus.memory
    }

    fn call(&mut self, routine: u16, back: u16, most: u64) -> Result<u64, (String, u64)> {
        let sp = self.cpu.regs.get_sp().wrapping_sub(2);
        self.cpu.regs.set_sp(sp);
        let [low, high] = back.to_le_bytes();
        self.bus.memory[usize::from(sp)] = low;
        self.bus.memory[usize::from(sp.wrapping_add(1))] = high;
        self.cpu.regs.set_pc(routine);
        let start = self.bus.t;
        loop {
            let taken = self.bus.t - start;
            if self.cpu.regs.get_pc() == back {
                return Ok(taken);
            }
            if taken > most {
                return Err((crate::i18n::tf!("It hadn't returned after {most} T-states: it was at ${:04X}.", self.cpu.regs.get_pc()), taken));
            }
            self.cpu.emulate(&mut self.bus);
            if self.bus.halted {
                return Err((crate::i18n::tf!("It stopped at a HALT, at ${:04X}.", self.cpu.regs.get_pc().wrapping_sub(1)), self.bus.t - start));
            }
        }
    }
}

/// 64K of memory for the 6502, nothing else on its bus.
struct Ram(Vec<u8>);

impl crate::m6502_cpu::Bus for Ram {
    fn read(&mut self, address: u16) -> u8 {
        self.0[usize::from(address)]
    }
    fn write(&mut self, address: u16, value: u8) {
        self.0[usize::from(address)] = value;
    }
}

struct M6502Core {
    cpu: crate::m6502_cpu::Cpu,
    ram: Ram,
}

impl M6502Core {
    fn new(memory: Vec<u8>) -> M6502Core {
        // As after a reset: the stack at its top, interrupts off.
        let cpu = crate::m6502_cpu::Cpu { s: 0xFF, i: true, ..Default::default() };
        M6502Core { cpu, ram: Ram(memory) }
    }

    fn flag_mut(&mut self, name: &str) -> Option<&mut bool> {
        let c = &mut self.cpu;
        Some(match name {
            "N" => &mut c.n,
            "V" => &mut c.v,
            "D" => &mut c.d,
            "I" => &mut c.i,
            "Z" => &mut c.z,
            "C" => &mut c.c,
            _ => return None,
        })
    }
}

impl Core for M6502Core {
    fn register(&mut self, name: &str) -> Option<(u32, u32)> {
        let c = &self.cpu;
        let v = match name {
            "A" => c.a,
            "X" => c.x,
            "Y" => c.y,
            "S" | "SP" => c.s,
            "P" => c.status(false),
            _ => return None,
        };
        Some((u32::from(v), 8))
    }

    fn set_register(&mut self, name: &str, value: u32) -> bool {
        let v = value as u8;
        match name {
            "A" => self.cpu.a = v,
            "X" => self.cpu.x = v,
            "Y" => self.cpu.y = v,
            "S" | "SP" => self.cpu.s = v,
            "P" => {
                for (bit, flag) in [(0x80, "N"), (0x40, "V"), (0x08, "D"), (0x04, "I"), (0x02, "Z"), (0x01, "C")] {
                    self.set_flag(flag, v & bit != 0);
                }
            }
            _ => return false,
        }
        true
    }

    fn flag(&self, name: &str) -> Option<bool> {
        let c = &self.cpu;
        Some(match name {
            "N" => c.n,
            "V" => c.v,
            "D" => c.d,
            "I" => c.i,
            "Z" => c.z,
            "C" => c.c,
            _ => return None,
        })
    }

    fn set_flag(&mut self, name: &str, on: bool) -> bool {
        match self.flag_mut(name) {
            Some(flag) => {
                *flag = on;
                true
            }
            None => false,
        }
    }

    fn memory(&mut self) -> &mut Vec<u8> {
        &mut self.ram.0
    }

    fn call(&mut self, routine: u16, back: u16, most: u64) -> Result<u64, (String, u64)> {
        // RTS takes the address from the stack and adds one, so one less than where it's to come back to.
        let [low, high] = back.wrapping_sub(1).to_le_bytes();
        self.ram.0[0x100 | usize::from(self.cpu.s)] = high;
        self.cpu.s = self.cpu.s.wrapping_sub(1);
        self.ram.0[0x100 | usize::from(self.cpu.s)] = low;
        self.cpu.s = self.cpu.s.wrapping_sub(1);
        self.cpu.pc = routine;
        let mut taken = 0u64;
        loop {
            if self.cpu.pc == back {
                return Ok(taken);
            }
            if taken > most {
                return Err((crate::i18n::tf!("It hadn't returned after {most} cycles: it was at ${:04X}.", self.cpu.pc), taken));
            }
            taken += u64::from(self.cpu.step(&mut self.ram));
            if self.cpu.jammed {
                return Err((crate::i18n::tf!("It jammed the processor, at ${:04X}.", self.cpu.pc.wrapping_sub(1)), taken));
            }
        }
    }
}

/// An address the return can go to: one no code was assembled to and no test sets, so nothing else gets there.
fn spare_address(assembly: &Assembly, taken: &[(u16, usize)]) -> u16 {
    let used = |a: u16| {
        assembly.chunks.iter().any(|c| (c.address..c.address + c.bytes.len() as i64).contains(&i64::from(a)))
            || taken.iter().any(|(at, len)| (usize::from(*at)..usize::from(*at) + len).contains(&usize::from(a)))
    };
    // $0000 first (the Spectrum's ROM starts there, so a routine in RAM won't be), then down from the top.
    std::iter::once(0u16).chain((0x0100..=0xFFFDu16).rev()).find(|a| !used(*a) && !used(a.wrapping_add(1))).unwrap_or(0)
}

/// The routine a test calls: as its `call:` says, or else the first label after its block in the source.
fn routine_of(spec: &Spec, assembly: &Assembly) -> Result<u16, String> {
    if let Some(call) = &spec.call {
        return number(call, assembly).map(|v| v as u16);
    }
    if spec.in_source
        && let Some(place) = assembly.definitions.iter().filter(|d| d.file == 0 && d.line >= spec.after && !d.name.contains('.')).min_by_key(|d| d.line)
        && let Some(value) = assembly.symbols.get(&place.name)
    {
        return Ok(*value as u16);
    }
    Err(crate::i18n::tr("It doesn't say which routine to call: add \"call: name\", or put the routine's label straight after the test.").into())
}

fn hex(value: u32, bits: u32) -> String {
    if bits <= 8 { format!("${value:02X}") } else { format!("${value:04X}") }
}

/// Runs one test on a processor, its program loaded.
fn run_one(spec: &Spec, cpu: Cpu, assembly: &Assembly, image: &[u8]) -> Outcome {
    let mut outcome = Outcome { name: spec.name.clone(), file: spec.file.clone(), line: spec.line + 1, passed: false, cycles: None, differences: spec.problems.clone() };
    if !outcome.differences.is_empty() {
        return outcome;
    }
    let routine = match routine_of(spec, assembly) {
        Ok(r) => r,
        Err(e) => {
            outcome.differences.push(e);
            return outcome;
        }
    };
    let mut core: Box<dyn Core> = match cpu {
        Cpu::Z80 => Box::new(Z80Core::new(image.to_vec())),
        _ => Box::new(M6502Core::new(image.to_vec())),
    };
    // What the test sets, then where the return goes, out of the way of it all.
    let mut set: Vec<(u16, usize)> = Vec::new();
    for setting in &spec.given {
        let done = match setting {
            Setting::Register(name, value) => number(value, assembly).and_then(|v| if core.set_register(name, v as u32) { Ok(()) } else { Err(crate::i18n::tf!("There's no register {name} on the {}.", cpu.name())) }),
            Setting::Flag(name, value) => number(value, assembly).and_then(|v| if core.set_flag(name, v != 0) { Ok(()) } else { Err(crate::i18n::tf!("There's no flag {name} on the {}.", cpu.name())) }),
            Setting::Memory { address, values, words } => number(address, assembly).and_then(|at| {
                let bytes = bytes_of(values, *words, assembly)?;
                let memory = core.memory();
                for (i, b) in bytes.iter().enumerate() {
                    memory[(at as usize + i) & 0xFFFF] = *b;
                }
                set.push((at as u16, bytes.len()));
                Ok(())
            }),
        };
        if let Err(e) = done {
            outcome.differences.push(e);
        }
    }
    if !outcome.differences.is_empty() {
        return outcome;
    }
    let back = spare_address(assembly, &set);
    let most = spec.within.map_or(MOST_CYCLES, |w| w.max(MOST_CYCLES));
    match core.call(routine, back, most) {
        Ok(taken) => outcome.cycles = Some(taken),
        Err((why, taken)) => {
            outcome.cycles = Some(taken);
            outcome.differences.push(why);
            return outcome;
        }
    }
    for setting in &spec.expect {
        match setting {
            Setting::Register(name, value) => match (core.register(name), number(value, assembly)) {
                (Some((got, bits)), Ok(want)) => {
                    let want = want as u32 & ((1u64 << bits) - 1) as u32;
                    if got != want {
                        outcome.differences.push(crate::i18n::tf!("{name} is {}, not {}", hex(got, bits), hex(want, bits)));
                    }
                }
                (None, _) => outcome.differences.push(crate::i18n::tf!("There's no register {name} on the {}.", cpu.name())),
                (_, Err(e)) => outcome.differences.push(e),
            },
            Setting::Flag(name, value) => match (core.flag(name), number(value, assembly)) {
                (Some(got), Ok(want)) => {
                    if got != (want != 0) {
                        outcome.differences.push(crate::i18n::tf!("flag {name} is {}, not {}", u8::from(got), u8::from(want != 0)));
                    }
                }
                (None, _) => outcome.differences.push(crate::i18n::tf!("There's no flag {name} on the {}.", cpu.name())),
                (_, Err(e)) => outcome.differences.push(e),
            },
            Setting::Memory { address, values, words } => match number(address, assembly).and_then(|at| Ok((at, bytes_of(values, *words, assembly)?))) {
                Ok((at, want)) => {
                    let memory = core.memory();
                    let got: Vec<u8> = (0..want.len()).map(|i| memory[(at as usize + i) & 0xFFFF]).collect();
                    if got != want {
                        let show = |bytes: &[u8]| {
                            if *words {
                                bytes.chunks(2).map(|w| format!("${:04X}", u16::from_le_bytes([w[0], *w.get(1).unwrap_or(&0)]))).collect::<Vec<_>>().join(",")
                            } else {
                                bytes.iter().map(|b| format!("${b:02X}")).collect::<Vec<_>>().join(",")
                            }
                        };
                        outcome.differences.push(crate::i18n::tf!("[${:04X}] is {}, not {}", at as u16, show(&got), show(&want)));
                    }
                }
                Err(e) => outcome.differences.push(e),
            },
        }
    }
    if let (Some(within), Some(taken)) = (spec.within, outcome.cycles)
        && taken > within
    {
        outcome.differences.push(crate::i18n::tf!("it took {taken} cycles, more than the {within} allowed"));
    }
    outcome.passed = outcome.differences.is_empty();
    outcome
}

/// The tests of a source: those in its comments, then those in the tests file beside it (`extra`, its path and
/// text, if there is one). Each is run, or all fail if the source doesn't assemble. `only` runs one, by name.
pub fn run(cpu: Cpu, path: &Path, text: &str, extra: Option<(&Path, &str)>, only: Option<&str>) -> Vec<Outcome> {
    let mut specs = find(path, text, true);
    if let Some((file, tests)) = extra {
        specs.extend(find(file, tests, false));
    }
    specs.retain(|s| only.is_none_or(|o| s.name == o));
    if specs.is_empty() {
        return Vec::new();
    }
    if !runs_on(cpu) {
        return specs.iter().map(|s| Outcome { name: s.name.clone(), file: s.file.clone(), line: s.line + 1, passed: false, cycles: None, differences: vec![crate::i18n::tf!("Tests run on the Z80 and the 6502; there's no {} here to run them on.", cpu.name())] }).collect();
    }
    let assembly = crate::assemblers::assemble_own_at(cpu, path, text, 0);
    if let Some(Problem { file, line, message }) = assembly.problems.first() {
        let place = assembly.files.get(*file).map(|f| f.display().to_string()).unwrap_or_default();
        let why = crate::i18n::tf!("The source doesn't assemble: {place}, line {}: {message}", line + 1);
        return specs.iter().map(|s| Outcome { name: s.name.clone(), file: s.file.clone(), line: s.line + 1, passed: false, cycles: None, differences: vec![why.clone()] }).collect();
    }
    let mut image = vec![0u8; 0x10000];
    for chunk in &assembly.chunks {
        for (i, b) in chunk.bytes.iter().enumerate() {
            image[(chunk.address as usize + i) & 0xFFFF] = *b;
        }
    }
    specs.iter().map(|spec| run_one(spec, cpu, &assembly, &image)).collect()
}

/// Whether a source has tests: in its comments, or a tests file beside it.
pub fn has_tests(path: Option<&Path>, text: &str) -> bool {
    text.lines().any(|l| comment_of(l, true).and_then(keyed).is_some_and(|(k, _)| k == "test")) || path.is_some_and(|p| tests_file_for(p).is_file())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run_text(cpu: Cpu, text: &str) -> Vec<Outcome> {
        run(cpu, Path::new("t.asm"), text, None, None)
    }

    #[test]
    fn a_test_block_is_read() {
        let specs = find(Path::new("t.asm"), "; test: adds\n;   a note about it\n; given: A=5 HL=$4000 [$4000]=1,2,\"AB\" flags: C=1\n; call: add\n; expect: A=6 [$4001].w=$0302\n; within: 1_200 T-states\nadd: inc a\n ret\n", true);
        assert_eq!(specs.len(), 1);
        let s = &specs[0];
        assert_eq!((s.name.as_str(), s.line, s.after), ("adds", 0, 6));
        assert_eq!(s.given, [Setting::Register("A".into(), "5".into()), Setting::Register("HL".into(), "$4000".into()), Setting::Memory { address: "$4000".into(), values: vec!["1".into(), "2".into(), "\"AB\"".into()], words: false }, Setting::Flag("C".into(), "1".into())]);
        assert_eq!(s.call.as_deref(), Some("add"));
        assert_eq!(s.expect[1], Setting::Memory { address: "$4001".into(), values: vec!["$0302".into()], words: true });
        assert_eq!(s.within, Some(1200));
        assert!(s.problems.is_empty());
        let bad = find(Path::new("t.asm"), "; test: x\n; given: A5\n", true);
        assert_eq!(bad[0].problems.len(), 1);
    }

    #[test]
    fn z80_routines_pass_and_fail_with_what_differed() {
        let source = "        org $8000
; test: adds one to each byte
; given: HL=buffer B=3 [buffer]=1,2,$FF
; expect: [buffer]=2,3,0 B=0 A=$FF flags: Z=1
; within: 200 cycles
add_ones:
        ld a,(hl)
        inc a
        ld (hl),a
        inc hl
        djnz add_ones
        ld a,$ff
        or a
        sub $ff
        ld a,$ff
        ret

; test: wrong on purpose
; given: A=1
; call: double
; expect: A=3 flags: C=1
double: add a,a
        ret
buffer: ds 3
";
        let outcomes = run_text(Cpu::Z80, source);
        assert_eq!(outcomes.len(), 2);
        assert!(outcomes[0].passed, "{:?}", outcomes[0]);
        // LD A,(HL) 7, INC A 4, LD (HL),A 7, INC HL 6, DJNZ 13/8: 37 a byte, 5 less for the last; then 7+4+7+7+10.
        assert_eq!(outcomes[0].cycles, Some(37 * 3 - 5 + 35));
        assert_eq!(outcomes[0].line, 2);
        assert!(!outcomes[1].passed);
        assert_eq!(outcomes[1].differences, ["A is $02, not $03", "flag C is 0, not 1"]);
        assert_eq!(outcomes[1].cycles, Some(4 + 10));
    }

    #[test]
    fn too_slow_and_never_returning_fail() {
        let source = " org $8000\n; test: slow\n; within: 10 cycles\nslow: nop\n nop\n nop\n ret\n; test: forever\nloop: jr loop\n";
        let outcomes = run_text(Cpu::Z80, source);
        assert_eq!(outcomes[0].differences, ["it took 22 cycles, more than the 10 allowed"]);
        assert!(!outcomes[1].passed);
        assert!(outcomes[1].differences[0].starts_with("It hadn't returned after"), "{:?}", outcomes[1]);
    }

    #[test]
    fn the_alternate_registers_and_ix_are_set_and_checked() {
        let source = " org $8000\n; test: swaps\n; given: HL'=$1234 IX=$4000 [$4002]=9 IYL='A'\n; expect: HL=$1234 A=9 HL'=0 IYL=65\nswap: exx\n ld a,(ix+2)\n ret\n";
        let outcomes = run_text(Cpu::Z80, source);
        assert!(outcomes[0].passed, "{:?}", outcomes[0]);
    }

    #[test]
    fn m6502_routines_run_with_their_cycles() {
        let source = "        * = $1000
; test: sums a table
; given: X=4 [table]=1,2,3,4
; expect: A=10 X=0 flags: Z=1 C=0
sum:    lda #0
        clc
loop:   adc table-1,x
        dex
        bne loop
        rts

; test: decimal mode
; given: A=$19 flags: D=1 C=0
; call: add_one
; expect: A=$20 [result]=$20
add_one: adc #1
        sta result
        rts
result: .byte 0
table:  .byte 0,0,0,0
";
        let outcomes = run_text(Cpu::M6502, source);
        assert!(outcomes[0].passed, "{:?}", outcomes[0]);
        // LDA # 2, CLC 2, then ADC abs,X 4 + DEX 2 + BNE 3 four times (the last BNE 2), RTS 6.
        assert_eq!(outcomes[0].cycles, Some(2 + 2 + 9 * 4 - 1 + 6));
        assert!(outcomes[1].passed, "{:?}", outcomes[1]);
    }

    #[test]
    fn a_source_that_doesnt_assemble_fails_every_test() {
        let outcomes = run_text(Cpu::Z80, "; test: one\n; call: x\nx: ld q,1\n ret\n");
        assert!(!outcomes[0].passed);
        assert!(outcomes[0].differences[0].starts_with("The source doesn't assemble"), "{:?}", outcomes[0]);
        let none = run_text(Cpu::M68k, "; test: one\nx: rts\n");
        assert!(none[0].differences[0].contains("68000"));
    }

    #[test]
    fn tests_can_be_kept_in_a_file_beside_the_source() {
        assert_eq!(tests_file_for(Path::new("/a/maths.asm")), Path::new("/a/maths.asm.tests"));
        let source = " org $8000\ntriple: ld b,a\n add a,a\n add a,b\n ret\n";
        let tests = "test: triples\ngiven: A=5\ncall: triple\nexpect: A=15\n\n; test: only this one\n; given: A=1\n; call: triple\n; expect: A=3\n";
        let outcomes = run(Cpu::Z80, Path::new("m.asm"), source, Some((Path::new("m.asm.tests"), tests)), None);
        assert_eq!(outcomes.iter().map(|o| (o.name.as_str(), o.passed, o.line)).collect::<Vec<_>>(), [("triples", true, 1), ("only this one", true, 6)]);
        assert_eq!(outcomes[0].file, Path::new("m.asm.tests"));
        let one = run(Cpu::Z80, Path::new("m.asm"), source, Some((Path::new("m.asm.tests"), tests)), Some("only this one"));
        assert_eq!(one.len(), 1);
        assert!(has_tests(None, "; test: x\n"));
        assert!(!has_tests(None, "; testing things\n"));
    }

    #[test]
    fn a_routine_at_address_zero_still_returns() {
        let outcomes = run_text(Cpu::Z80, " org 0\n; test: at zero\n; expect: A=7\nstart: ld a,7\n ret\n");
        assert!(outcomes[0].passed, "{:?}", outcomes[0]);
    }
}
