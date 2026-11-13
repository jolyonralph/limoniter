//! Assemblers: Limoniter's own, and the ones people already use (sjasmplus and pasmo for the Z80), found the way
//! emulators are. Whichever assembles a program, what comes back is the same: the bytes at their addresses, which
//! source line made which, and the problems, so running, the debugger and the problems' marks work alike.

use crate::asm::{self, Assembly, Problem};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

/// A processor Limoniter assembles for.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Cpu {
    Z80,
    M6502,
    M6809,
    M68k,
}

impl Cpu {
    pub fn name(self) -> &'static str {
        match self {
            Cpu::Z80 => "Z80",
            Cpu::M6502 => "6502",
            Cpu::M6809 => "6809",
            Cpu::M68k => "68000",
        }
    }

    pub fn id(self) -> &'static str {
        match self {
            Cpu::Z80 => "z80",
            Cpu::M6502 => "6502",
            Cpu::M6809 => "6809",
            Cpu::M68k => "68000",
        }
    }

    /// The processor whose source a highlighting language is.
    pub fn for_language(language: &str) -> Option<Cpu> {
        match language {
            "Assembly Z80" => Some(Cpu::Z80),
            "Assembly 6502" => Some(Cpu::M6502),
            "Assembly 6809" => Some(Cpu::M6809),
            "Assembly 680x0" => Some(Cpu::M68k),
            _ => None,
        }
    }

    pub fn isa(self) -> &'static dyn asm::Isa {
        match self {
            Cpu::Z80 => &asm::z80::Z80,
            Cpu::M6502 => &asm::m6502::M6502,
            Cpu::M6809 => &asm::m6809::M6809,
            Cpu::M68k => &asm::m68k::M68k,
        }
    }
}

/// How an assembler is run and its output read.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Style {
    /// sjasmplus: a listing gives every line's address and bytes, and SAVEBIN the memory.
    Sjasmplus,
    /// pasmo: makes a tape with a BASIC loader itself.
    Pasmo,
    /// ACME: makes a Commodore program, and a report of each line's address and bytes.
    Acme,
    /// lwasm: makes a Color Computer binary, and a listing of each line's address and bytes.
    Lwasm,
    /// vasm (Motorola syntax): makes a plain binary, and a listing.
    Vasm,
}

/// An assembler of someone else's.
pub struct External {
    pub id: &'static str,
    pub name: &'static str,
    pub cpu: Cpu,
    pub programs: &'static [&'static str],
    pub folder_hints: &'static [&'static str],
    pub style: Style,
    pub website: &'static str,
}

pub const KNOWN: [External; 5] = [
    External { id: "sjasmplus", name: "sjasmplus", cpu: Cpu::Z80, programs: &["sjasmplus"], folder_hints: &["sjasmplus"], style: Style::Sjasmplus, website: "https://github.com/z00m128/sjasmplus" },
    External { id: "pasmo", name: "Pasmo", cpu: Cpu::Z80, programs: &["pasmo"], folder_hints: &["pasmo"], style: Style::Pasmo, website: "https://pasmo.speccy.org" },
    External { id: "lwasm", name: "lwasm (LWTOOLS)", cpu: Cpu::M6809, programs: &["lwasm"], folder_hints: &["lwtools", "lwasm"], style: Style::Lwasm, website: "http://www.lwtools.ca" },
    External { id: "vasm", name: "vasm", cpu: Cpu::M68k, programs: &["vasmm68k_mot"], folder_hints: &["vasm"], style: Style::Vasm, website: "http://sun.hasenbraten.de/vasm/" },
    External { id: "acme", name: "ACME", cpu: Cpu::M6502, programs: &["acme"], folder_hints: &["acme"], style: Style::Acme, website: "https://sourceforge.net/projects/acme-crossass/" },
];

/// Limoniter's own assembler's name in the settings.
pub const OWN: &str = "limoniter";

/// Which assembler a processor's source is assembled with: Limoniter's own, or a program.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Choice {
    Own,
    Program { id: &'static str, path: PathBuf },
}

impl Choice {
    pub fn name(&self) -> String {
        match self {
            Choice::Own => "Limoniter's own".into(),
            Choice::Program { id, path } => format!("{} ({})", external(id).map_or(*id, |e| e.name), path.display()),
        }
    }

    /// As kept in the settings.
    pub fn setting(&self) -> String {
        match self {
            Choice::Own => OWN.into(),
            Choice::Program { id, path } => format!("{id}\u{2}{}", path.display()),
        }
    }

    pub fn from_setting(value: &str) -> Choice {
        match value.split_once('\u{2}') {
            Some((id, path)) => match KNOWN.iter().find(|e| e.id == id) {
                Some(found) => Choice::Program { id: found.id, path: PathBuf::from(path) },
                None => Choice::Own,
            },
            None => Choice::Own,
        }
    }
}

pub fn external(id: &str) -> Option<&'static External> {
    KNOWN.iter().find(|e| e.id == id)
}

/// The assemblers installed for a processor, where they are.
pub fn installed(cpu: Cpu) -> Vec<(&'static External, PathBuf)> {
    KNOWN.iter().filter(|e| e.cpu == cpu).filter_map(|e| crate::emulators::find_program(e.programs, e.folder_hints).map(|path| (e, path))).collect()
}

/// What assembling gave: the program, and, from another assembler, the file it made ready to run.
pub struct Assembled {
    pub assembly: Assembly,
    pub tape: Option<(crate::running::Format, Vec<u8>)>,
    /// What the assembler printed, for the Output panel.
    pub printed: Vec<String>,
}

/// Assembles with Limoniter's own assembler. `text` is the source as it is in the editor.
pub fn assemble_own(cpu: Cpu, path: &Path, text: &str) -> Assembled {
    let read = |p: &Path| std::fs::read(p).map_err(|e| e.to_string());
    let assembly = asm::assemble(cpu.isa(), path, text, &read);
    Assembled { assembly, tape: None, printed: Vec::new() }
}

/// Assembles a saved file with someone else's assembler.
pub fn assemble_external(found: &External, program: &Path, source: &Path) -> Result<Assembled, String> {
    let work = std::env::temp_dir().join(format!("limoniter-asm-{}-{}", std::process::id(), found.id));
    let _ = std::fs::remove_dir_all(&work);
    std::fs::create_dir_all(&work).map_err(|e| format!("A folder to assemble in couldn't be made: {e}"))?;
    let result = match found.style {
        Style::Sjasmplus => sjasmplus(program, source, &work),
        Style::Pasmo => pasmo(program, source, &work),
        Style::Acme => acme(program, source, &work),
        Style::Lwasm => lwasm(program, source, &work),
        Style::Vasm => vasm(program, source, &work),
    };
    let _ = std::fs::remove_dir_all(&work);
    result
}

/// Runs a program in a folder, giving up after a while; what it printed, and whether it succeeded.
fn run(program: &Path, args: &[String], folder: &Path) -> Result<(bool, Vec<String>), String> {
    let mut command = Command::new(program);
    command.args(args).current_dir(folder).stdin(std::process::Stdio::null()).stdout(std::process::Stdio::piped()).stderr(std::process::Stdio::piped());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x0800_0000);
    }
    let mut child = command.spawn().map_err(|e| format!("{} couldn't be started: {e}", program.display()))?;
    let started = std::time::Instant::now();
    loop {
        if child.try_wait().map_err(|e| e.to_string())?.is_some() {
            break;
        }
        if started.elapsed() > Duration::from_secs(60) {
            let _ = child.kill();
            return Err(format!("{} took over a minute, so it was stopped.", program.display()));
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    let output = child.wait_with_output().map_err(|e| e.to_string())?;
    let mut printed: Vec<String> = String::from_utf8_lossy(&output.stdout).lines().map(str::to_string).collect();
    printed.extend(String::from_utf8_lossy(&output.stderr).lines().map(str::to_string));
    Ok((output.status.success(), printed))
}

fn sjasmplus(program: &Path, source: &Path, work: &Path) -> Result<Assembled, String> {
    let text = std::fs::read_to_string(source).unwrap_or_default();
    // A wrapper includes the program and saves the memory it made, as the device sees it.
    let has_device = text.lines().any(|line| asm::strip_comment(line).split_whitespace().take(2).any(|w| w.eq_ignore_ascii_case("device")));
    let mut wrapper = String::new();
    if !has_device {
        wrapper.push_str(" DEVICE ZXSPECTRUM48\n");
    }
    wrapper.push_str(&format!(" INCLUDE \"{}\"\n SAVEBIN \"{}\",0,65536\n", source.display(), work.join("memory.bin").display()));
    std::fs::write(work.join("wrapper.asm"), wrapper).map_err(|e| e.to_string())?;
    let folder = source.parent().unwrap_or(Path::new("."));
    let args = vec!["--nologo".to_string(), "--zxnext".into(), "--fullpath".into(), format!("--lst={}", work.join("out.lst").display()), format!("--sym={}", work.join("out.sym").display()), work.join("wrapper.asm").display().to_string()];
    let (_, printed) = run(program, &args, folder)?;
    let mut assembly = Assembly { files: vec![source.to_path_buf()], ..Assembly::default() };
    for line in &printed {
        if let Some((file, number, message)) = problem_line(line) {
            let file = file_index(&mut assembly, &file, source);
            assembly.problems.push(Problem { file, line: number.saturating_sub(1), message });
        }
    }
    let mut reached = None;
    if let Ok(listing) = std::fs::read_to_string(work.join("out.lst")) {
        reached = read_listing(&listing, &mut assembly, source);
    }
    if let Ok(symbols) = std::fs::read_to_string(work.join("out.sym")) {
        for line in symbols.lines() {
            if let Some((name, value)) = line.split_once(": EQU ")
                && let Ok(value) = i64::from_str_radix(value.trim().trim_start_matches("0x"), 16)
            {
                assembly.symbols.insert(name.trim().to_string(), value);
            }
        }
    }
    // The memory as the program left it, for the lines' bytes; the listing only shows the first few of a line's.
    if let Ok(memory) = std::fs::read(work.join("memory.bin")) {
        for placed in &mut assembly.lines {
            let from = placed.address as usize;
            let length = placed.bytes.len();
            if let Some(bytes) = memory.get(from..from + length) {
                placed.bytes = bytes.to_vec();
            }
        }
        // The memory from the first byte made to the last: lines like DS show no bytes in the listing, but filled
        // the memory all the same.
        let low = assembly.lines.iter().filter(|p| !p.bytes.is_empty()).map(|p| p.address).min();
        let high = assembly.lines.iter().map(|p| p.address + p.bytes.len() as i64).chain(reached).max();
        if let (Some(low), Some(high)) = (low, high)
            && let Some(bytes) = memory.get(low as usize..(high as usize).min(memory.len()))
        {
            assembly.chunks = vec![asm::Chunk { address: low, bytes: bytes.to_vec() }];
        }
    } else if assembly.problems.is_empty() {
        assembly.problems.push(Problem { file: 0, line: 0, message: "sjasmplus didn't save the memory it assembled".into() });
    }
    assembly.entry = ["start", "main", "begin", "entry"].iter().find_map(|name| assembly.symbols.iter().find(|(label, _)| label.eq_ignore_ascii_case(name)).map(|(_, v)| *v));
    assembly.device = text.lines().find_map(|line| {
        let words: Vec<&str> = asm::strip_comment(line).split_whitespace().collect();
        let at = words.iter().position(|w| w.eq_ignore_ascii_case("device"))?;
        words.get(at + 1).map(|w| w.trim_end_matches(',').to_ascii_uppercase())
    });
    Ok(Assembled { assembly, tape: None, printed })
}

/// sjasmplus's listing: each line's number, address and first bytes, under the file it's in. The bytes are only
/// counted here; the memory saved gives them all.
fn read_listing(listing: &str, assembly: &mut Assembly, source: &Path) -> Option<i64> {
    let mut files: Vec<usize> = Vec::new();
    // The furthest address a line of the program's own was at, after the last code: where a DS at the end ends.
    let mut furthest: Option<i64> = None;
    let first_code = |a: &Assembly| a.lines.first().map(|p| p.address);
    let mut last: Option<(usize, usize, i64)> = None;
    for line in listing.lines() {
        if let Some(name) = line.strip_prefix("# file opened: ") {
            let name = name.trim();
            // The wrapper isn't the user's.
            let index = if name.ends_with("wrapper.asm") { usize::MAX } else { file_index(assembly, name, source) };
            files.push(index);
            continue;
        }
        if line.starts_with("# file closed") {
            files.pop();
            continue;
        }
        let Some(&file) = files.last() else {
            continue;
        };
        if file == usize::MAX {
            continue;
        }
        let number_end = line.find(|c: char| !c.is_ascii_digit()).unwrap_or(0);
        let Ok(number) = line[..number_end].parse::<usize>() else {
            continue;
        };
        let rest = line[number_end..].trim_start_matches(['+', '~']);
        let mut words = rest.split_whitespace();
        let Some(address) = words.next().and_then(|a| i64::from_str_radix(a, 16).ok()) else {
            continue;
        };
        // The bytes are the two-digit hex words before the source starts, at a fixed column.
        let columns = &rest[rest.find(|c: char| !c.is_whitespace()).unwrap_or(0)..];
        let after_address = columns.get(4..).unwrap_or("");
        let field: String = after_address.chars().take(13).collect();
        let count = field.split_whitespace().take_while(|w| w.len() == 2 && w.chars().all(|c| c.is_ascii_hexdigit())).count();
        if count == 0 {
            if first_code(assembly).is_some_and(|first| address > first) {
                furthest = Some(furthest.map_or(address, |f: i64| f.max(address)));
            }
            continue;
        }
        let line_index = number - 1;
        match &mut last {
            Some((f, l, end)) if *f == file && *l == line_index && *end == address => {
                *end += count as i64;
                if let Some(placed) = assembly.lines.last_mut() {
                    placed.bytes.extend(std::iter::repeat_n(0, count));
                }
            }
            _ => {
                assembly.lines.push(asm::Placed { file, line: line_index, address, bytes: vec![0; count] });
                last = Some((file, line_index, address + count as i64));
            }
        }
    }
    furthest
}

fn file_index(assembly: &mut Assembly, name: &str, source: &Path) -> usize {
    let path = if Path::new(name).is_absolute() { PathBuf::from(name) } else { source.parent().unwrap_or(Path::new(".")).join(name) };
    let same = |a: &Path, b: &Path| a == b || a.file_name() == b.file_name() && (a.parent() == b.parent() || !a.is_absolute() || !b.is_absolute());
    if let Some(index) = assembly.files.iter().position(|f| same(f, &path)) {
        return index;
    }
    assembly.files.push(path);
    assembly.files.len() - 1
}

/// A problem as assemblers print it: "file.asm(12): error: what", "file.asm:12: what", or pasmo's
/// "ERROR on line 12 of file file.asm".
pub fn problem_line(line: &str) -> Option<(String, usize, String)> {
    // ACME: "Error - File prog.asm, line 5 (Zone <untitled>): Syntax error."
    if let Some(rest) = line.strip_prefix("Error - File ").or_else(|| line.strip_prefix("Serious error - File ")) {
        let (file, rest) = rest.split_once(", line ")?;
        let number_end = rest.find(|c: char| !c.is_ascii_digit())?;
        let message = rest.split_once("): ").map_or("", |(_, m)| m).trim_end_matches('.');
        return Some((file.to_string(), rest[..number_end].parse().ok()?, message.to_string()));
    }
    if let Some(rest) = line.strip_prefix("ERROR on line ") {
        let (number, file) = rest.split_once(" of file ")?;
        return Some((file.trim().to_string(), number.trim().parse().ok()?, String::new()));
    }
    let open = line.find('(')?;
    let close = open + line[open..].find(')')?;
    let after = line[close + 1..].trim_start().strip_prefix(':')?;
    let number: usize = line[open + 1..close].split(',').next()?.trim().parse().ok()?;
    let message = after.trim();
    // lwasm: "prog.asm(3) : ERROR : Bad opcode".
    let message = message.strip_prefix("error:").or_else(|| message.strip_prefix("ERROR :")).unwrap_or(message).trim();
    if message.starts_with("WARNING") {
        return None;
    }
    if message.starts_with("warning") {
        return None;
    }
    Some((line[..open].trim().to_string(), number, message.to_string()))
}

fn pasmo(program: &Path, source: &Path, work: &Path) -> Result<Assembled, String> {
    let tape = work.join("out.tap");
    let folder = source.parent().unwrap_or(Path::new("."));
    let args = vec!["--tapbas".to_string(), source.display().to_string(), tape.display().to_string()];
    let (ok, printed) = run(program, &args, folder)?;
    let mut assembly = Assembly { files: vec![source.to_path_buf()], ..Assembly::default() };
    // pasmo stops at its first problem: where on one line, what on the next.
    let mut lines = printed.iter().peekable();
    while let Some(line) = lines.next() {
        if let Some((file, number, _)) = problem_line(line) {
            let message = lines.peek().and_then(|next| next.strip_prefix("ERROR:")).map(|m| m.trim().to_string()).unwrap_or_else(|| "pasmo found a problem here".into());
            let file = file_index(&mut assembly, &file, source);
            assembly.problems.push(Problem { file, line: number.saturating_sub(1), message });
        }
    }
    if !ok && assembly.problems.is_empty() {
        assembly.problems.push(Problem { file: 0, line: 0, message: printed.join(" ") });
    }
    let tape = if ok { std::fs::read(&tape).ok().map(|t| (crate::running::Format::Tap, t)) } else { None };
    Ok(Assembled { assembly, tape, printed })
}

fn vasm(program: &Path, source: &Path, work: &Path) -> Result<Assembled, String> {
    let out = work.join("out.bin");
    let listing = work.join("out.lst");
    let folder = source.parent().unwrap_or(Path::new("."));
    let args = vec!["-Fbin".to_string(), "-no-opt".into(), "-quiet".into(), "-o".into(), out.display().to_string(), "-L".into(), listing.display().to_string(), source.display().to_string()];
    let (ok, printed) = run(program, &args, folder)?;
    let mut assembly = Assembly { files: vec![source.to_path_buf()], ..Assembly::default() };
    // "error 2030 in line 83 of \"prog.s\": displacement out of range"
    for line in &printed {
        let Some(rest) = line.strip_prefix("error ").or_else(|| line.strip_prefix("fatal error ")) else { continue };
        let Some((_, rest)) = rest.split_once(" in line ") else { continue };
        let Some((number, rest)) = rest.split_once(" of \"") else { continue };
        let Some((file, message)) = rest.split_once("\": ") else { continue };
        let file = file_index(&mut assembly, file, source);
        assembly.problems.push(Problem { file, line: number.trim().parse::<usize>().unwrap_or(1).saturating_sub(1), message: message.to_string() });
    }
    if !ok && assembly.problems.is_empty() {
        assembly.problems.push(Problem { file: 0, line: 0, message: printed.join(" ") });
    }
    // "00:00001000 60FE            \t     2: start: bra.s start", under "Source: \"prog.s\"".
    if let Ok(text) = std::fs::read_to_string(&listing) {
        let mut file = 0;
        for line in text.lines() {
            if let Some(name) = line.strip_prefix("Source: \"").and_then(|n| n.strip_suffix('"')) {
                file = file_index(&mut assembly, name, source);
                continue;
            }
            let Some((head, rest)) = line.split_once('\t') else { continue };
            let mut words = head.split_whitespace();
            let (Some(place), Some(bytes)) = (words.next(), words.next()) else { continue };
            let Some(address) = place.split_once(':').and_then(|(_, a)| i64::from_str_radix(a, 16).ok()) else { continue };
            if bytes.len() % 2 != 0 || !bytes.chars().all(|c| c.is_ascii_hexdigit()) {
                continue;
            }
            let number: usize = rest.trim_start().split(':').next().and_then(|n| n.trim().parse().ok()).unwrap_or(0);
            let bytes: Vec<u8> = (0..bytes.len()).step_by(2).filter_map(|i| u8::from_str_radix(&bytes[i..i + 2], 16).ok()).collect();
            assembly.lines.push(asm::Placed { file, line: number.saturating_sub(1), address, bytes });
        }
    }
    if let (true, Ok(binary)) = (ok, std::fs::read(&out))
        && let Some(low) = assembly.lines.iter().map(|p| p.address).min()
    {
        assembly.chunks = vec![asm::Chunk { address: low, bytes: binary }];
        assembly.entry = Some(low);
    }
    Ok(Assembled { assembly, tape: None, printed })
}

fn lwasm(program: &Path, source: &Path, work: &Path) -> Result<Assembled, String> {
    let out = work.join("out.bin");
    let listing = work.join("out.lst");
    let folder = source.parent().unwrap_or(Path::new("."));
    let args = vec!["--format=decb".to_string(), "-o".into(), out.display().to_string(), format!("--list={}", listing.display()), source.display().to_string()];
    let (ok, printed) = run(program, &args, folder)?;
    let mut assembly = Assembly { files: vec![source.to_path_buf()], ..Assembly::default() };
    for line in &printed {
        if let Some((file, number, message)) = problem_line(line) {
            let file = file_index(&mut assembly, &file, source);
            assembly.problems.push(Problem { file, line: number.saturating_sub(1), message });
        }
    }
    if !ok && assembly.problems.is_empty() {
        assembly.problems.push(Problem { file: 0, line: 0, message: printed.join(" ") });
    }
    // "3002 B70400           (            d.asm):00003  sta $400"
    if let Ok(text) = std::fs::read_to_string(&listing) {
        for line in text.lines() {
            let Some(open) = line.find('(') else { continue };
            let Some(close) = line[open..].find("):").map(|c| open + c) else { continue };
            let mut words = line[..open].split_whitespace();
            let (Some(address), Some(bytes)) = (words.next(), words.next()) else { continue };
            let (Ok(address), true) = (i64::from_str_radix(address, 16), address.len() == 4 && bytes.len() % 2 == 0 && bytes.chars().all(|c| c.is_ascii_hexdigit())) else { continue };
            let number: usize = line[close + 2..].split_whitespace().next().and_then(|n| n.parse().ok()).unwrap_or(0);
            let name = line[open + 1..close].trim();
            let file = file_index(&mut assembly, name, source);
            let bytes: Vec<u8> = (0..bytes.len()).step_by(2).filter_map(|i| u8::from_str_radix(&bytes[i..i + 2], 16).ok()).collect();
            assembly.lines.push(asm::Placed { file, line: number.saturating_sub(1), address, bytes });
        }
    }
    let binary = if ok { std::fs::read(&out).ok() } else { None };
    // The binary's blocks are the program's bytes; its last says where it starts.
    if let Some(bin) = &binary {
        let mut at = 0;
        while at + 5 <= bin.len() {
            let length = u16::from_be_bytes([bin[at + 1], bin[at + 2]]) as usize;
            let address = u16::from_be_bytes([bin[at + 3], bin[at + 4]]) as i64;
            if bin[at] == 0xFF {
                assembly.entry = Some(address);
                break;
            }
            assembly.chunks.push(asm::Chunk { address, bytes: bin.get(at + 5..at + 5 + length).unwrap_or_default().to_vec() });
            at += 5 + length;
        }
    }
    Ok(Assembled { assembly, tape: None, printed })
}

fn acme(program: &Path, source: &Path, work: &Path) -> Result<Assembled, String> {
    let out = work.join("out.prg");
    let report = work.join("out.lst");
    let folder = source.parent().unwrap_or(Path::new("."));
    let args = vec!["-f".to_string(), "cbm".into(), "-o".into(), out.display().to_string(), "-r".into(), report.display().to_string(), source.display().to_string()];
    let (ok, printed) = run(program, &args, folder)?;
    let mut assembly = Assembly { files: vec![source.to_path_buf()], ..Assembly::default() };
    for line in &printed {
        if let Some((file, number, message)) = problem_line(line) {
            let file = file_index(&mut assembly, &file, source);
            assembly.problems.push(Problem { file, line: number.saturating_sub(1), message });
        }
    }
    if !ok && assembly.problems.is_empty() {
        assembly.problems.push(Problem { file: 0, line: 0, message: printed.join(" ") });
    }
    // The report: "     2  c000 a901    lda #1", under the file it's from.
    if let Ok(listing) = std::fs::read_to_string(&report) {
        let mut file = 0;
        for line in listing.lines() {
            if let Some(name) = line.strip_prefix("; ******** Source: ") {
                file = file_index(&mut assembly, name.trim(), source);
                continue;
            }
            let mut words = line.split_whitespace();
            let (Some(number), Some(address), Some(bytes)) = (words.next().and_then(|n| n.parse::<usize>().ok()), words.next(), words.next()) else {
                continue;
            };
            let (Ok(address), true) = (i64::from_str_radix(address, 16), address.len() == 4 && bytes.len() % 2 == 0 && bytes.chars().all(|c| c.is_ascii_hexdigit())) else {
                continue;
            };
            let bytes: Vec<u8> = (0..bytes.len()).step_by(2).filter_map(|i| u8::from_str_radix(&bytes[i..i + 2], 16).ok()).collect();
            assembly.lines.push(asm::Placed { file, line: number - 1, address, bytes });
        }
    }
    let program = if ok { std::fs::read(&out).ok() } else { None };
    if let Some(prg) = program.as_ref().filter(|p| p.len() > 2) {
        assembly.chunks = vec![asm::Chunk { address: u16::from_le_bytes([prg[0], prg[1]]) as i64, bytes: prg[2..].to_vec() }];
    }
    let label = assembly.lines.first().map(|p| p.address);
    assembly.entry = label;
    Ok(Assembled { assembly, tape: program.map(|p| (crate::running::Format::Prg, p)), printed })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn problems_are_read_as_each_assembler_prints_them() {
        assert_eq!(problem_line("prog.asm(5): error: Label not found: nowhere"), Some(("prog.asm".into(), 5, "Label not found: nowhere".into())));
        assert_eq!(problem_line("C:\\src\\prog.asm(12): error: [JR] Target out of range"), Some(("C:\\src\\prog.asm".into(), 12, "[JR] Target out of range".into())));
        assert_eq!(problem_line("ERROR on line 6 of file prog.asm").map(|p| (p.0, p.1)), Some(("prog.asm".into(), 6)));
        assert_eq!(problem_line("prog.asm(790): warning[out0]: unstable"), None);
        assert_eq!(problem_line("Pass 3 complete"), None);
        assert_eq!(problem_line("c.asm(3) : ERROR : Bad opcode"), Some(("c.asm".into(), 3, "Bad opcode".into())));
        assert_eq!(problem_line("Error - File a.asm, line 5 (Zone <untitled>): Syntax error."), Some(("a.asm".into(), 5, "Syntax error".into())));
        assert_eq!(problem_line("Warning - File a.asm, line 5 (Zone <untitled>): Label name not in leftmost column."), None);
    }

    #[test]
    fn choices_are_kept_in_the_settings() {
        let choice = Choice::Program { id: "sjasmplus", path: PathBuf::from("/usr/bin/sjasmplus") };
        assert_eq!(Choice::from_setting(&choice.setting()), choice);
        assert_eq!(Choice::from_setting(OWN), Choice::Own);
        assert_eq!(Choice::from_setting("nothing\u{2}/x"), Choice::Own);
    }

    /// With LIMONITER_SJASMPLUS and LIMONITER_PASMO naming the programs: both assemble a program and report
    /// problems, and sjasmplus's bytes and lines are those of Limoniter's own assembler.
    #[test]
    #[ignore]
    fn real_assemblers_give_the_same_program() {
        let folder = std::env::temp_dir().join(format!("limoniter-ext-asm-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&folder);
        std::fs::create_dir_all(&folder).unwrap();
        let source = folder.join("prog.asm");
        let text = "  org $8000\nstart: ld a,2\n  out ($fe),a\n  db \"hello world, a longer line\",0\n  ld hl,start\n  ret\n";
        std::fs::write(&source, text).unwrap();
        let own = assemble_own(Cpu::Z80, &source, text).assembly;
        if let Ok(program) = std::env::var("LIMONITER_SJASMPLUS") {
            let theirs = assemble_external(external("sjasmplus").unwrap(), Path::new(&program), &source).unwrap().assembly;
            assert!(theirs.ok(), "{:?}", theirs.problems);
            assert_eq!(theirs.chunks, own.chunks);
            assert_eq!(theirs.lines.iter().map(|p| (p.line, p.address, p.bytes.clone())).collect::<Vec<_>>(), own.lines.iter().map(|p| (p.line, p.address, p.bytes.clone())).collect::<Vec<_>>());
            assert_eq!(theirs.entry, Some(0x8000));
            std::fs::write(&source, "  org $8000\n  jr nowhere\n  ld q,1\n").unwrap();
            let wrong = assemble_external(external("sjasmplus").unwrap(), Path::new(&program), &source).unwrap().assembly;
            let lines: Vec<usize> = wrong.problems.iter().map(|p| p.line).collect();
            assert!(lines.contains(&1) && lines.contains(&2), "{:?}", wrong.problems);
            std::fs::write(&source, text).unwrap();
        }
        if let Ok(program) = std::env::var("LIMONITER_PASMO") {
            let theirs = assemble_external(external("pasmo").unwrap(), Path::new(&program), &source).unwrap();
            assert!(theirs.assembly.ok(), "{:?}", theirs.assembly.problems);
            let tape = theirs.tape.unwrap();
            assert!(tape.1.windows(5).any(|w| w == [0x3E, 2, 0xD3, 0xFE, 0x68]), "the code is on the tape");
            std::fs::write(&source, "  org $8000\n  nop\n  ld q,1\n").unwrap();
            let wrong = assemble_external(external("pasmo").unwrap(), Path::new(&program), &source).unwrap().assembly;
            assert_eq!(wrong.problems.first().map(|p| p.line), Some(2), "{:?}", wrong.problems);
        }
        if let Ok(program) = std::env::var("LIMONITER_LWASM") {
            let text = " org $3000\nstart lda #1\n sta $400\n ldx #msg\nloop bra loop\nmsg fcc /hi/\n end start\n";
            std::fs::write(&source, text).unwrap();
            let theirs = assemble_external(external("lwasm").unwrap(), Path::new(&program), &source).unwrap().assembly;
            assert!(theirs.ok(), "{:?}", theirs.problems);
            let own = assemble_own(Cpu::M6809, &source, text).assembly;
            assert_eq!(theirs.chunks, own.chunks);
            assert_eq!((theirs.entry, own.entry), (Some(0x3000), Some(0x3000)));
            assert_eq!(theirs.lines.iter().map(|p| (p.line, p.address, p.bytes.clone())).collect::<Vec<_>>(), own.lines.iter().map(|p| (p.line, p.address, p.bytes.clone())).collect::<Vec<_>>());
            std::fs::write(&source, " org $3000\n lda #1\n frob #2\n").unwrap();
            let wrong = assemble_external(external("lwasm").unwrap(), Path::new(&program), &source).unwrap().assembly;
            assert_eq!(wrong.problems.first().map(|p| p.line), Some(2), "{:?}", wrong.problems);
        }
        if let Ok(program) = std::env::var("LIMONITER_VASM") {
            let text = "\torg $1000\nstart:\tmove.l #$12345678,d0\n\tlea start(pc),a0\nloop:\tdbra d0,loop\n\trts\n";
            std::fs::write(&source, text).unwrap();
            let theirs = assemble_external(external("vasm").unwrap(), Path::new(&program), &source).unwrap().assembly;
            assert!(theirs.ok(), "{:?}", theirs.problems);
            let own = assemble_own(Cpu::M68k, &source, text).assembly;
            assert_eq!(theirs.chunks, own.chunks);
            assert_eq!(theirs.lines.iter().map(|p| (p.line, p.address, p.bytes.clone())).collect::<Vec<_>>(), own.lines.iter().map(|p| (p.line, p.address, p.bytes.clone())).collect::<Vec<_>>());
            std::fs::write(&source, "\torg $1000\n\tmove.l d0,d1\n\tfrob d2\n").unwrap();
            let wrong = assemble_external(external("vasm").unwrap(), Path::new(&program), &source).unwrap().assembly;
            assert_eq!(wrong.problems.first().map(|p| p.line), Some(2), "{:?}", wrong.problems);
        }
        if let Ok(program) = std::env::var("LIMONITER_ACME") {
            let text = "  *=$c000\nstart lda #1\n  sta $d020\n- inx\n  bne -\n  rts\n";
            std::fs::write(&source, text).unwrap();
            let theirs = assemble_external(external("acme").unwrap(), Path::new(&program), &source).unwrap();
            assert!(theirs.assembly.ok(), "{:?}", theirs.assembly.problems);
            let own = assemble_own(Cpu::M6502, &source, text).assembly;
            assert_eq!(theirs.assembly.chunks, own.chunks);
            assert_eq!(theirs.assembly.lines.iter().map(|p| (p.line, p.address, p.bytes.clone())).collect::<Vec<_>>(), own.lines.iter().map(|p| (p.line, p.address, p.bytes.clone())).collect::<Vec<_>>());
            std::fs::write(&source, "  *=$c000\n  lda #1\n  ldq #2\n").unwrap();
            let wrong = assemble_external(external("acme").unwrap(), Path::new(&program), &source).unwrap().assembly;
            assert_eq!(wrong.problems.first().map(|p| p.line), Some(2), "{:?}", wrong.problems);
        }
        let _ = std::fs::remove_dir_all(&folder);
    }
}

/// With LIMONITER_SJASMPLUS and LIMONITER_ASM_SOURCES (a folder of programs): every program sjasmplus assembles
/// cleanly, Limoniter's own assembler makes the same bytes of. Prints how each went.
#[cfg(test)]
#[test]
#[ignore]
fn real_programs_assemble_as_sjasmplus_does() {
    let (Ok(program), Ok(folder)) = (std::env::var("LIMONITER_SJASMPLUS"), std::env::var("LIMONITER_ASM_SOURCES")) else {
        return;
    };
    let mut files = Vec::new();
    let mut folders = vec![PathBuf::from(folder)];
    while let Some(folder) = folders.pop() {
        for entry in std::fs::read_dir(&folder).unwrap().flatten() {
            let path = entry.path();
            if path.is_dir() {
                folders.push(path);
            } else if path.extension().is_some_and(|e| e.eq_ignore_ascii_case("asm") || e.eq_ignore_ascii_case("a80") || e.eq_ignore_ascii_case("z80")) {
                files.push(path);
            }
        }
    }
    files.sort();
    let (mut same, mut differ, mut ours_failed, mut skipped) = (0, 0, 0, 0);
    for path in files {
        let Ok(text) = std::fs::read_to_string(&path) else { continue };
        let theirs = match assemble_external(external("sjasmplus").unwrap(), Path::new(&program), &path) {
            Ok(t) if t.assembly.ok() && !t.assembly.chunks.is_empty() => t.assembly,
            _ => {
                skipped += 1;
                continue;
            }
        };
        let ours = assemble_own(Cpu::Z80, &path, &text).assembly;
        if !ours.ok() {
            ours_failed += 1;
            let p = &ours.problems[0];
            println!("OURS FAILED {}: {}({}): {} [{} problems]", path.display(), ours.files.get(p.file).map(|f| f.display().to_string()).unwrap_or_default(), p.line + 1, p.message, ours.problems.len());
            continue;
        }
        // Only where Limoniter's assembler put bytes: sjasmplus's memory also holds the system variables and so on.
        let mut a = Vec::new();
        let mut b = Vec::new();
        let mut low = 0;
        for chunk in &ours.chunks {
            if a.is_empty() {
                low = chunk.address;
            }
            // The memory as it ends up: a later ORG can write over an earlier run.
            a.extend(ours.image(chunk.address, chunk.address + chunk.bytes.len() as i64));
            b.extend(theirs.image(chunk.address, chunk.address + chunk.bytes.len() as i64));
        }
        if a == b {
            same += 1;
        } else {
            differ += 1;
            let at = (0..a.len()).find(|i| a[*i] != b[*i]).unwrap_or(0);
            let line = theirs.line_at(low + at as i64);
            println!("DIFFER {} at ${:04X} {:?}: ours {:02X?} theirs {:02X?}", path.display(), low + at as i64, line, &a[at..(at + 6).min(a.len())], &b[at..(at + 6).min(b.len())]);
        }
    }
    println!("same {same}, differ {differ}, ours failed {ours_failed}, sjasmplus didn't {skipped}");
}

/// With LIMONITER_ACME and LIMONITER_ACME_SOURCES (a folder of ACME programs): every program ACME assembles,
/// Limoniter's own assembler makes the same bytes of. Prints how each went.
#[cfg(test)]
#[test]
#[ignore]
fn real_programs_assemble_as_acme_does() {
    let (Ok(program), Ok(folder)) = (std::env::var("LIMONITER_ACME"), std::env::var("LIMONITER_ACME_SOURCES")) else {
        return;
    };
    let mut files: Vec<PathBuf> = std::fs::read_dir(&folder).unwrap().flatten().map(|e| e.path()).filter(|p| p.extension().is_some_and(|e| e == "a" || e == "asm")).collect();
    files.sort();
    let (mut same, mut differ, mut failed) = (0, 0, 0);
    for path in files {
        let Ok(text) = std::fs::read_to_string(&path) else { continue };
        let Ok(theirs) = assemble_external(external("acme").unwrap(), Path::new(&program), &path) else { continue };
        if !theirs.assembly.ok() || theirs.assembly.chunks.is_empty() {
            println!("ACME couldn't: {}", path.display());
            continue;
        }
        let ours = assemble_own(Cpu::M6502, &path, &text).assembly;
        if !ours.ok() {
            failed += 1;
            for p in ours.problems.iter().take(3) {
                println!("OURS FAILED {}: {}({}): {}", path.display(), ours.files.get(p.file).map(|f| f.display().to_string()).unwrap_or_default(), p.line + 1, p.message);
            }
            continue;
        }
        let chunk = &theirs.assembly.chunks[0];
        let mine = ours.image(chunk.address, chunk.address + chunk.bytes.len() as i64);
        if mine == chunk.bytes {
            same += 1;
        } else {
            differ += 1;
            let at = (0..mine.len()).find(|i| mine[*i] != chunk.bytes[*i]).unwrap_or(0);
            println!("DIFFER {} at ${:04X} {:?}: ours {:02X?} theirs {:02X?}", path.display(), chunk.address + at as i64, theirs.assembly.line_at(chunk.address + at as i64), &mine[at..(at + 6).min(mine.len())], &chunk.bytes[at..(at + 6).min(chunk.bytes.len())]);
        }
    }
    println!("same {same}, differ {differ}, ours failed {failed}");
}
