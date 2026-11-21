//! Compilers for the older machines: Boriel's ZX Basic (zxbc) for the Spectrum, and C with cc65, llvm-mos,
//! z88dk, SDCC, SGDK (the Mega Drive's), vbcc and Bebbo's amiga-gcc. Each is found where it's installed, run in a
//! folder of its own with the command line its documentation gives, and what it prints read for problems by file
//! and line. What it makes (a tape, a .prg, a cartridge, an Amiga program) is run as an assembled program is.

use crate::running::{Format, Machine, Runnable};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

/// Which compiler, for how it's run and what it makes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Toolchain {
    Zxbc,
    Cc65,
    LlvmMos,
    Z88dk,
    Sdcc,
    Sgdk,
    Vbcc,
    AmigaGcc,
    /// SAS/C itself, run through vamos (see `amiga_vamos`): found as vamos, with SAS/C's folder known.
    SasC,
}

/// A compiler Limoniter knows: where it's usually installed and what it compiles for.
#[derive(Debug, PartialEq, Eq)]
pub struct Compiler {
    pub id: &'static str,
    pub name: &'static str,
    pub toolchain: Toolchain,
    pub machines: &'static [Machine],
    /// The program's names; llvm-mos has one for each machine (see `programs`).
    pub programs: &'static [&'static str],
    /// Words a folder it's been unzipped into has in its name.
    pub folder_hints: &'static [&'static str],
    /// Variables naming the folder it's installed in, as its documentation has them set.
    pub homes: &'static [&'static str],
    /// Where its installers put it, when that isn't on the PATH.
    pub usual: &'static [&'static str],
    pub website: &'static str,
}

const SPECTRUM: &[Machine] = &[Machine::Spectrum48];
const COMMODORES: &[Machine] = &[Machine::C64, Machine::C128, Machine::Vic20, Machine::Plus4, Machine::Pet];

pub const KNOWN: [Compiler; 9] = [
    Compiler { id: "zxbc", name: "Boriel's ZX Basic (zxbc)", toolchain: Toolchain::Zxbc, machines: SPECTRUM, programs: &["zxbc", "zxbc.py", "zxb", "zxb.py"], folder_hints: &["zxbasic", "zxbc", "boriel"], homes: &[], usual: &[], website: "https://github.com/boriel-basic/zxbasic" },
    Compiler { id: "cc65", name: "cc65", toolchain: Toolchain::Cc65, machines: COMMODORES, programs: &["cl65"], folder_hints: &["cc65"], homes: &["CC65_HOME"], usual: &["/usr/local/cc65/bin", "/opt/cc65/bin", "C:\\cc65\\bin"], website: "https://cc65.github.io" },
    Compiler { id: "llvm-mos", name: "llvm-mos", toolchain: Toolchain::LlvmMos, machines: &[Machine::C64, Machine::C128, Machine::Vic20, Machine::Pet, Machine::Atari2600], programs: &[], folder_hints: &["llvm-mos"], homes: &["LLVM_MOS_SDK"], usual: &["/opt/llvm-mos/bin", "/usr/local/llvm-mos/bin", "C:\\llvm-mos\\bin"], website: "https://www.llvm-mos.org" },
    Compiler { id: "z88dk", name: "z88dk", toolchain: Toolchain::Z88dk, machines: &[Machine::Spectrum48, Machine::MasterSystem, Machine::GameGear, Machine::ColecoVision], programs: &["zcc"], folder_hints: &["z88dk"], homes: &["Z88DK"], usual: &["/opt/z88dk/bin", "/usr/local/z88dk/bin", "C:\\z88dk\\bin"], website: "https://z88dk.org" },
    Compiler { id: "sdcc", name: "SDCC", toolchain: Toolchain::Sdcc, machines: SPECTRUM, programs: &["sdcc"], folder_hints: &["sdcc"], homes: &["SDCC_HOME"], usual: &["/usr/local/bin", "C:\\Program Files\\SDCC\\bin"], website: "https://sdcc.sourceforge.net" },
    Compiler { id: "sgdk", name: "SGDK", toolchain: Toolchain::Sgdk, machines: &[Machine::MegaDrive], programs: &[], folder_hints: &["sgdk"], homes: &["GDK", "SGDK", "GDK_WIN"], usual: &["/opt/sgdk", "C:\\SGDK", "C:\\sgdk"], website: "https://github.com/Stephane-D/SGDK" },
    Compiler { id: "amiga-gcc", name: "amiga-gcc (Bebbo's)", toolchain: Toolchain::AmigaGcc, machines: &[Machine::Amiga], programs: &["m68k-amigaos-gcc"], folder_hints: &["amiga-gcc", "amiga"], homes: &[], usual: &["/opt/amiga/bin", "C:\\amiga-gcc\\bin"], website: "https://codeberg.org/bebbo/amiga-gcc" },
    Compiler { id: "vbcc", name: "vbcc", toolchain: Toolchain::Vbcc, machines: &[Machine::Amiga], programs: &["vc"], folder_hints: &["vbcc"], homes: &["VBCC"], usual: &["/opt/vbcc/bin", "C:\\vbcc\\bin"], website: "http://www.compilers.de/vbcc.html" },
    Compiler { id: "sasc", name: "SAS/C (through vamos)", toolchain: Toolchain::SasC, machines: &[Machine::Amiga], programs: &["vamos"], folder_hints: &["amitools"], homes: &[], usual: &[], website: crate::amiga_vamos::WEBSITE },
];

pub fn by_id(id: &str) -> Option<&'static Compiler> {
    KNOWN.iter().find(|c| c.id.eq_ignore_ascii_case(id) || c.id.replace('-', "") == id.to_ascii_lowercase().replace(['-', ' '], ""))
}

/// llvm-mos's program for a machine: a clang of its own for each, which knows the machine's memory and start-up.
fn llvm_mos_program(machine: Machine) -> &'static [&'static str] {
    match machine {
        Machine::C64 => &["mos-c64-clang"],
        Machine::C128 => &["mos-c128-clang"],
        Machine::Vic20 => &["mos-vic20-clang"],
        Machine::Pet => &["mos-pet-clang"],
        Machine::Atari2600 => &["mos-atari2600-4k-clang"],
        _ => &[],
    }
}

impl Compiler {
    /// The names its program goes by, for a machine.
    pub fn programs(&self, machine: Machine) -> &'static [&'static str] {
        match self.toolchain {
            Toolchain::LlvmMos => llvm_mos_program(machine),
            _ => self.programs,
        }
    }

    /// Where it's installed, for a machine: its program, or for SGDK, its folder (the one with makefile.gen).
    pub fn find(&self, machine: Machine) -> Option<PathBuf> {
        // SAS/C is there when both vamos and SAS/C's own folder are: its program is vamos.
        if self.toolchain == Toolchain::SasC {
            return crate::amiga_vamos::find_sasc(&crate::amiga_vamos::settings()).and_then(|_| crate::amiga_vamos::find());
        }
        let homes: Vec<PathBuf> = self.homes.iter().filter_map(std::env::var_os).map(PathBuf::from).collect();
        // z88dk's documentation has ZCCCFG name its lib/config folder, two below where it's installed.
        let z88dk = (self.toolchain == Toolchain::Z88dk).then(|| std::env::var_os("ZCCCFG").map(PathBuf::from).and_then(|p| p.parent()?.parent().map(Path::to_path_buf))).flatten();
        let mut folders: Vec<PathBuf> = homes.iter().chain(z88dk.iter()).flat_map(|h| [h.join("bin"), h.clone()]).collect();
        folders.extend(self.usual.iter().map(PathBuf::from));
        if self.toolchain == Toolchain::Sgdk {
            let sgdk = |folder: &Path| folder.join("makefile.gen").is_file();
            if let Some(found) = folders.iter().find(|f| sgdk(f)) {
                return Some(found.clone());
            }
            // A folder named for it, where downloads are unzipped, holding makefile.gen.
            return crate::emulators::find_program(&["makefile.gen"], self.folder_hints).and_then(|p| p.parent().map(Path::to_path_buf)).filter(|f| sgdk(f));
        }
        let programs = self.programs(machine);
        for folder in &folders {
            for program in programs {
                for name in [program.to_string(), format!("{program}.exe")] {
                    let candidate = folder.join(name);
                    if candidate.is_file() {
                        return Some(candidate);
                    }
                }
            }
        }
        // Then the one Limoniter installed itself, when asked to (see `boriel_get`).
        crate::emulators::find_program(programs, self.folder_hints).or_else(|| if self.toolchain == Toolchain::Zxbc { crate::boriel_get::installed() } else { None })
    }
}

/// The compilers for a machine's C (or, for the Spectrum, Boriel's BASIC too), in the order they're tried.
pub fn for_machine(machine: Machine, basic: bool) -> Vec<&'static Compiler> {
    KNOWN.iter().filter(|c| c.machines.contains(&machine) && (c.toolchain == Toolchain::Zxbc) == basic).collect()
}

/// What to compile: the compiler, where it is, the machine, the files and where the result goes.
#[derive(Clone, Debug)]
pub struct Job {
    pub compiler: &'static Compiler,
    pub program: PathBuf,
    pub machine: Machine,
    /// The file in front, and every file compiled with it (it among them, first).
    pub main: PathBuf,
    pub sources: Vec<PathBuf>,
    /// The folder it's built in: the source's, or for SGDK, the project's (the one above src).
    pub folder: PathBuf,
    /// More for the compiler's command line, from a SAS/C project's SCOPTIONS.
    pub flags: Vec<String>,
    /// What the program made is called, without an ending.
    pub stem: String,
    /// SAS/C source, compiled through Limoniter's compatibility header (see `sasc`).
    pub sasc: bool,
    /// Libraries linked after the files, from an Amiga project's makefile (see `amiga_project`).
    pub libs: Vec<String>,
}

/// A problem a compiler printed: the file, the line (from 1; 0 if it didn't say), what's wrong, and whether it's
/// only a warning.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Problem {
    pub file: String,
    pub line: usize,
    pub message: String,
    pub warning: bool,
}

/// What compiling gave: the problems, everything printed, and the file made, if it was.
#[derive(Clone, Debug, Default)]
pub struct Compiled {
    pub problems: Vec<Problem>,
    pub printed: Vec<String>,
    pub made: Option<(String, Vec<u8>)>,
    /// What Limoniter itself has to say about the source: SAS/C constructs that won't carry over, and so on.
    pub notes: Vec<Problem>,
}

impl Compiled {
    pub fn ok(&self) -> bool {
        self.made.is_some() && !self.problems.iter().any(|p| !p.warning)
    }
}

/// cc65's name for a machine, after -t.
fn cc65_target(machine: Machine) -> &'static str {
    match machine {
        Machine::C128 => "c128",
        Machine::Vic20 => "vic20",
        Machine::Plus4 => "plus4",
        Machine::Pet => "pet",
        _ => "c64",
    }
}

/// z88dk's name for a machine, after +, and what else it needs.
fn z88dk_target(machine: Machine) -> &'static [&'static str] {
    match machine {
        Machine::MasterSystem => &["+sms"],
        Machine::GameGear => &["+sms", "-subtype=gamegear"],
        Machine::ColecoVision => &["+coleco"],
        _ => &["+zx"],
    }
}

fn shown(path: &Path) -> String {
    path.display().to_string()
}

/// The commands that build a job, in order: each program with its arguments. `work` is a folder of the job's own
/// for what's made along the way. They follow each compiler's documentation (see NOTES.md for where).
pub fn commands(job: &Job, work: &Path) -> Vec<(PathBuf, Vec<String>)> {
    let program = job.program.clone();
    let stem = &job.stem;
    let sources: Vec<String> = job.sources.iter().map(|s| shown(s)).collect();
    let flags = job.flags.clone();
    match job.compiler.toolchain {
        // zxbc program.bas --tap --BASIC --autorun (-taB): a tape with a BASIC loader that runs the code.
        Toolchain::Zxbc => {
            let mut args = vec![shown(&job.main), "--tap".into(), "--BASIC".into(), "--autorun".into(), "-o".into(), shown(&work.join(format!("{stem}.tap")))];
            args.extend(flags);
            // The Python version is run by Python where a .py can't be run itself.
            if program.extension().is_some_and(|e| e.eq_ignore_ascii_case("py")) && cfg!(windows) {
                args.insert(0, shown(&program));
                return vec![(PathBuf::from("python"), args)];
            }
            vec![(program, args)]
        }
        // cl65 leaves each object beside its source, so each is compiled to the work folder with -c, then linked.
        Toolchain::Cc65 => {
            let target = cc65_target(job.machine);
            let mut steps: Vec<(PathBuf, Vec<String>)> = Vec::new();
            let mut objects = Vec::new();
            for (n, source) in sources.iter().enumerate() {
                let object = shown(&work.join(format!("{n}.o")));
                let mut args = vec!["-t".to_string(), target.into(), "-O".into()];
                args.extend(flags.iter().cloned());
                args.extend(["-c".into(), "-o".into(), object.clone(), source.clone()]);
                steps.push((program.clone(), args));
                objects.push(object);
            }
            let mut link = vec!["-t".to_string(), target.into(), "-o".into(), shown(&work.join(format!("{stem}.prg")))];
            link.extend(objects);
            steps.push((program, link));
            steps
        }
        // mos-c64-clang -Os -o hello.prg hello.c: llvm-mos's clang for the machine makes the program whole.
        Toolchain::LlvmMos => {
            let ending = if job.machine == Machine::Atari2600 { "a26" } else { "prg" };
            let mut args = vec!["-Os".to_string(), "-o".into(), shown(&work.join(format!("{stem}.{ending}")))];
            args.extend(flags);
            args.extend(sources);
            vec![(program, args)]
        }
        // zcc +zx -create-app: a .tap (or for +sms a .sms, +coleco a .rom) beside the binary named by -o.
        Toolchain::Z88dk => {
            let mut args: Vec<String> = z88dk_target(job.machine).iter().map(|s| s.to_string()).collect();
            args.extend(["-vn".into(), "-create-app".into(), "-o".into(), shown(&work.join(format!("{stem}.bin")))]);
            args.extend(flags);
            args.extend(sources);
            vec![(program, args)]
        }
        // sdcc -mz80, each file to a .rel with -c, then linked at 32768 with no start-up code of SDCC's own, so
        // main (which comes first) is where it starts; the data goes from 49152.
        Toolchain::Sdcc => {
            let mut steps = Vec::new();
            let mut objects = Vec::new();
            for (n, source) in sources.iter().enumerate() {
                let object = shown(&work.join(format!("{n}.rel")));
                let mut args = vec!["-mz80".to_string()];
                args.extend(flags.iter().cloned());
                args.extend(["-c".into(), "-o".into(), object.clone(), source.clone()]);
                steps.push((program.clone(), args));
                objects.push(object);
            }
            let mut link = vec!["-mz80".to_string(), "--code-loc".into(), "0x8000".into(), "--data-loc".into(), "0xC000".into(), "--no-std-crt0".into(), "-o".into(), shown(&work.join(format!("{stem}.ihx")))];
            link.extend(objects);
            steps.push((program, link));
            steps
        }
        // SGDK's own makefile, run in the project's folder: <SGDK>\bin\make -f <SGDK>\makefile.gen, making
        // out/rom.bin. Elsewhere than Windows, the system's make.
        Toolchain::Sgdk => {
            let make = if cfg!(windows) { program.join("bin").join("make.exe") } else { PathBuf::from("make") };
            vec![(make, vec!["-f".into(), shown(&program.join("makefile.gen"))])]
        }
        // vc +aos68k -c99 -o prog prog.c -lamiga: an AmigaOS program, with C99's // comments allowed. A project's
        // makefile may give another configuration (+kick13) and its libraries, which go after the files.
        Toolchain::Vbcc => {
            let config = flags.iter().any(|f| f.starts_with('+'));
            let mut args = if config { Vec::new() } else { vec!["+aos68k".to_string()] };
            args.extend(["-c99".to_string(), "-O1".into()]);
            args.extend(flags);
            args.extend(["-o".into(), shown(&work.join(stem))]);
            args.extend(sources);
            args.extend(job.libs.iter().cloned());
            if !job.libs.iter().any(|l| l == "-lamiga") {
                args.push("-lamiga".into());
            }
            vec![(program, args)]
        }
        // vamos running SAS/C's smake or sc in the project's folder (see `amiga_vamos::sasc_arguments`).
        Toolchain::SasC => {
            let settings = crate::amiga_vamos::settings();
            let sasc = crate::amiga_vamos::find_sasc(&settings).unwrap_or_default();
            vec![(program, crate::amiga_vamos::sasc_arguments(&settings, &sasc, &job.folder, &job.sources, stem))]
        }
        // m68k-amigaos-gcc -Os hello.c -noixemul -o hello: with libnix, for Kickstart 2.0 and on, unless a project's
        // makefile chooses another C library (-mcrt=nix13 for 1.3).
        Toolchain::AmigaGcc => {
            let mut args = vec!["-Os".to_string()];
            if !flags.iter().any(|f| f.starts_with("-mcrt=") || f == "-noixemul") {
                args.push("-noixemul".into());
            }
            args.extend(flags);
            args.extend(["-o".into(), shown(&work.join(stem))]);
            args.extend(sources);
            args.extend(job.libs.iter().cloned());
            vec![(program, args)]
        }
    }
}

/// A problem as the compilers print one, or None for any other line:
/// - zxbc: `prog.bas:12: error: Syntax Error...` and `prog.bas:5: warning: [W100] ...`
/// - cc65: `prog.c(6): Error: ';' expected` and `prog.c(2): Warning: ...`
/// - gcc, clang (llvm-mos, SGDK, amiga-gcc) and z88dk's sccz80: `prog.c:6:5: error: ...`
/// - SDCC: `prog.c:6: error 20: Undefined identifier 'y'` and `prog.c:3: syntax error: token -> 'x' ; column 5`
/// - vbcc: `error 82 in line 4 of "prog.c": unknown identifier <foo>`
pub fn problem_in(line: &str) -> Option<Problem> {
    // SAS/C, run through vamos: "hello.c 6 Error 57: Semi-colon expected".
    if let Some(problem) = crate::amiga_vamos::sasc_problem(line) {
        return Some(problem);
    }
    let text = line.trim_end();
    // vbcc (and vasm): "error 82 in line 4 of "prog.c": unknown identifier <foo>".
    for (word, warning) in [("error ", false), ("warning ", true), ("fatal error ", false)] {
        if let Some(rest) = text.strip_prefix(word)
            && let Some((_, rest)) = rest.split_once(" in line ")
            && let Some((number, rest)) = rest.split_once(" of \"")
            && let Some((file, message)) = rest.split_once("\": ")
        {
            return Some(Problem { file: file.into(), line: number.trim().parse().ok()?, message: message.trim().into(), warning });
        }
    }
    // cc65: "prog.c(6): Error: ';' expected".
    if let Some(open) = text.find('(')
        && let Some(close) = text[open..].find("): ").map(|c| open + c)
        && let Ok(number) = text[open + 1..close].parse::<usize>()
        && open > 0
    {
        let rest = &text[close + 3..];
        let (kind, message) = rest.split_once(": ").unwrap_or(("", rest));
        let warning = match kind.to_ascii_lowercase().as_str() {
            "error" | "fatal" => false,
            "warning" => true,
            _ => return None,
        };
        return Some(Problem { file: text[..open].into(), line: number, message: message.trim().into(), warning });
    }
    // The rest: "file:line:[column:] kind[ number]: message". A file may start with a drive letter.
    let (drive, rest) = if text.len() > 2 && text.as_bytes()[1] == b':' && text.as_bytes()[0].is_ascii_alphabetic() { text.split_at(2) } else { ("", text) };
    let (file, rest) = rest.split_once(':')?;
    if file.is_empty() || file.starts_with(' ') {
        return None;
    }
    let (number, rest) = rest.split_once(':')?;
    let number: usize = number.trim().parse().ok()?;
    // A column, as gcc and clang give it.
    let rest = match rest.split_once(':') {
        Some((column, after)) if column.trim().parse::<usize>().is_ok() => after,
        _ => rest,
    };
    let rest = rest.trim_start();
    let (kind, message) = rest.split_once(':')?;
    let kind = kind.trim().to_ascii_lowercase();
    let word = kind.split_whitespace().take_while(|w| !w.chars().all(|c| c.is_ascii_digit())).collect::<Vec<_>>().join(" ");
    let (warning, message) = match word.as_str() {
        "error" | "fatal error" => (false, message.trim().to_string()),
        "warning" => (true, message.trim().to_string()),
        // SDCC: "syntax error: token -> 'x' ; column 5".
        "syntax error" => (false, format!("syntax error: {}", message.trim())),
        _ => return None,
    };
    Some(Problem { file: format!("{drive}{file}"), line: number, message, warning })
}

/// Bytes from an Intel HEX file, as SDCC links to: each record's bytes at its address, from the lowest to the
/// highest. The lowest address, and the bytes.
pub fn read_intel_hex(text: &str) -> Option<(u32, Vec<u8>)> {
    let mut placed: Vec<(u32, Vec<u8>)> = Vec::new();
    let mut base = 0u32;
    for line in text.lines() {
        let Some(record) = line.trim().strip_prefix(':') else { continue };
        let bytes: Vec<u8> = (0..record.len() / 2).filter_map(|i| u8::from_str_radix(record.get(i * 2..i * 2 + 2)?, 16).ok()).collect();
        if bytes.len() < 5 || bytes.len() != bytes[0] as usize + 5 || bytes.iter().fold(0u8, |a, b| a.wrapping_add(*b)) != 0 {
            return None;
        }
        let address = u16::from_be_bytes([bytes[1], bytes[2]]) as u32;
        let data = &bytes[4..4 + bytes[0] as usize];
        match bytes[3] {
            0 => placed.push((base + address, data.to_vec())),
            1 => break,
            2 if data.len() == 2 => base = (u16::from_be_bytes([data[0], data[1]]) as u32) << 4,
            4 if data.len() == 2 => base = (u16::from_be_bytes([data[0], data[1]]) as u32) << 16,
            _ => {}
        }
    }
    let low = placed.iter().map(|(a, _)| *a).min()?;
    let high = placed.iter().map(|(a, b)| *a + b.len() as u32).max()?;
    let mut image = vec![0u8; (high - low) as usize];
    for (address, data) in placed {
        let at = (address - low) as usize;
        image[at..at + data.len()].copy_from_slice(&data);
    }
    Some((low, image))
}

/// Runs a program in a folder with some variables set, giving up after `limit`: whether it succeeded, and what it
/// printed.
pub(crate) fn run(program: &Path, args: &[String], folder: &Path, env: &[(String, String)], limit: Duration) -> Result<(bool, Vec<String>), String> {
    let mut command = std::process::Command::new(program);
    command.args(args).current_dir(folder).stdin(std::process::Stdio::null()).stdout(std::process::Stdio::piped()).stderr(std::process::Stdio::piped());
    for (name, value) in env {
        command.env(name, value);
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x0800_0000);
    }
    let mut child = command.spawn().map_err(|e| crate::i18n::tf!("{} couldn't be started: {e}", program.display()))?;
    // What it prints is read as it goes, so a long build's output can't fill the pipe and stop it.
    let stdout = child.stdout.take();
    let stderr = child.stderr.take();
    let read = |pipe: Option<Box<dyn std::io::Read + Send>>| {
        std::thread::spawn(move || {
            let mut text = Vec::new();
            if let Some(mut pipe) = pipe {
                let _ = pipe.read_to_end(&mut text);
            }
            String::from_utf8_lossy(&text).lines().map(str::to_string).collect::<Vec<String>>()
        })
    };
    let out = read(stdout.map(|p| Box::new(p) as Box<dyn std::io::Read + Send>));
    let err = read(stderr.map(|p| Box::new(p) as Box<dyn std::io::Read + Send>));
    let started = std::time::Instant::now();
    let status = loop {
        if let Some(status) = child.try_wait().map_err(|e| e.to_string())? {
            break status;
        }
        if started.elapsed() > limit {
            let _ = child.kill();
            return Err(crate::i18n::tf!("{} took over {} seconds, so it was stopped.", program.display(), limit.as_secs()));
        }
        std::thread::sleep(Duration::from_millis(20));
    };
    let mut printed = out.join().unwrap_or_default();
    printed.extend(err.join().unwrap_or_default());
    Ok((status.success(), printed))
}

/// Builds a job: runs its commands in turn in its folder, stopping at the first that fails, and reads the problems
/// from what they print. `work` is a folder of the job's own, emptied first.
pub fn build(job: &Job, work: &Path) -> Result<Compiled, String> {
    let _ = std::fs::remove_dir_all(work);
    std::fs::create_dir_all(work).map_err(|e| crate::i18n::tf!("A folder to compile in couldn't be made: {e}"))?;
    let mut job = job.clone();
    let mut compiled = Compiled::default();
    // SAS/C source goes through a copy of its own, made to suit the compiler, with lines kept where they were.
    if job.sasc {
        let (sources, notes, include) = crate::sasc::prepare(&job, work)?;
        job.sources = sources;
        compiled.notes = notes;
        job.flags.extend(include.iter().map(|folder| format!("-I{}", folder.display())));
    }
    // SGDK builds a project laid out its way, with the source in src: a file elsewhere is copied into one.
    let mut copied_from = None;
    if job.compiler.toolchain == Toolchain::Sgdk && !job.folder.join("src").is_dir() {
        let project = work.join("project");
        std::fs::create_dir_all(project.join("src")).map_err(|e| e.to_string())?;
        for source in &job.sources {
            if let Some(name) = source.file_name() {
                std::fs::copy(source, project.join("src").join(name)).map_err(|e| crate::i18n::tf!("{} couldn't be copied to build: {e}", source.display()))?;
            }
        }
        copied_from = Some(job.folder.clone());
        job.folder = project;
    }
    let mut env = Vec::new();
    if job.compiler.toolchain == Toolchain::Sgdk {
        env.push(("GDK".to_string(), shown(&job.program)));
    }
    if job.compiler.toolchain == Toolchain::Zxbc {
        env.extend(crate::boriel_get::environment());
    }
    let limit = if job.compiler.toolchain == Toolchain::Sgdk { Duration::from_secs(600) } else { Duration::from_secs(120) };
    let mut ok = true;
    for (program, args) in commands(&job, work) {
        let (worked, printed) = run(&program, &args, &job.folder, &env, limit)?;
        compiled.printed.extend(printed);
        if !worked {
            ok = false;
            break;
        }
    }
    for line in &compiled.printed {
        if let Some(mut problem) = problem_in(line) {
            // A file of the copy SGDK built is the person's own file of that name.
            if let Some(original) = &copied_from
                && let Some(name) = Path::new(&problem.file).file_name()
            {
                problem.file = shown(&original.join(name));
            }
            if !compiled.problems.contains(&problem) {
                compiled.problems.push(problem);
            }
        }
    }
    if !ok && !compiled.problems.iter().any(|p| !p.warning) {
        let said: Vec<&str> = compiled.printed.iter().map(|l| l.trim()).filter(|l| !l.is_empty()).collect();
        let message = if said.is_empty() { crate::i18n::tf!("{} stopped without saying why.", job.compiler.name) } else { said.iter().rev().take(3).rev().cloned().collect::<Vec<_>>().join(" ") };
        compiled.problems.push(Problem { file: shown(&job.main), line: 0, message, warning: false });
    }
    if ok {
        compiled.made = made(&job, work);
        if compiled.made.is_none() {
            compiled.problems.push(Problem { file: shown(&job.main), line: 0, message: crate::i18n::tf!("{} finished, but the program it should have made wasn't there.", job.compiler.name), warning: false });
        }
    }
    Ok(compiled)
}

/// The file a job made: its name and bytes.
fn made(job: &Job, work: &Path) -> Option<(String, Vec<u8>)> {
    let stem = &job.stem;
    let read = |path: PathBuf| std::fs::read(&path).ok();
    match job.compiler.toolchain {
        Toolchain::Zxbc => read(work.join(format!("{stem}.tap"))).map(|b| (format!("{stem}.tap"), b)),
        Toolchain::Cc65 => read(work.join(format!("{stem}.prg"))).map(|b| (format!("{stem}.prg"), b)),
        Toolchain::LlvmMos => {
            let ending = if job.machine == Machine::Atari2600 { "a26" } else { "prg" };
            read(work.join(format!("{stem}.{ending}"))).map(|b| (format!("{stem}.{ending}"), b))
        }
        // appmake names what it makes for the machine: .tap, .sms, .gg or .rom.
        Toolchain::Z88dk => ["tap", "sms", "gg", "rom", "col"].iter().find_map(|ending| read(work.join(format!("{stem}.{ending}"))).map(|b| (format!("{stem}.{ending}"), b))),
        Toolchain::Sdcc => {
            let text = std::fs::read_to_string(work.join(format!("{stem}.ihx"))).ok()?;
            let (low, bytes) = read_intel_hex(&text)?;
            let assembly = crate::asm::Assembly { chunks: vec![crate::asm::Chunk { address: low as i64, bytes }], entry: Some(low as i64), ..crate::asm::Assembly::default() };
            crate::asm::output::spectrum_tape(&assembly, stem).ok().map(|tape| (format!("{stem}.tap"), tape))
        }
        Toolchain::Sgdk => read(job.folder.join("out").join("rom.bin")).map(|b| (format!("{stem}.md"), b)),
        Toolchain::Vbcc | Toolchain::AmigaGcc => read(work.join(stem)).map(|b| (stem.clone(), b)),
        // SAS/C leaves the program in the project's folder, as it would on the Amiga.
        Toolchain::SasC => read(job.folder.join(stem)).map(|b| (stem.clone(), b)),
    }
}

/// What a job made, ready to run on its machine.
pub fn runnable(job: &Job, name: &str, bytes: Vec<u8>) -> Result<Runnable, String> {
    let format = match job.compiler.toolchain {
        Toolchain::Zxbc | Toolchain::Sdcc => Format::Tap,
        Toolchain::Cc65 => Format::Prg,
        Toolchain::LlvmMos if job.machine == Machine::Atari2600 => Format::A26,
        Toolchain::LlvmMos => Format::Prg,
        Toolchain::Z88dk => match name.rsplit_once('.').map(|(_, e)| e) {
            Some("tap") => Format::Tap,
            Some("sms") => Format::SmsRom,
            Some("gg") => Format::GgRom,
            _ => Format::ColRom,
        },
        Toolchain::Sgdk => Format::MdRom,
        Toolchain::Vbcc | Toolchain::AmigaGcc | Toolchain::SasC => Format::Hunk,
    };
    let name = if format == Format::Hunk { format!("{name}.exe") } else { name.to_string() };
    if format == Format::Hunk && !bytes.starts_with(&[0, 0, 0x03, 0xF3]) {
        return Err(crate::i18n::tf!("{} didn't make an Amiga program.", job.compiler.name));
    }
    Ok(Runnable { machine: job.machine, format, name, bytes: Arc::new(bytes) })
}

/// Which machine C source is for: a `// machine:` line near its top says (c64, c128, vic20, plus4, pet,
/// spectrum, sms, gamegear, coleco, megadrive, amiga or atari2600), or else the headers it includes show it.
pub fn c_machine<'a>(lines: impl IntoIterator<Item = &'a str>) -> Option<Machine> {
    let mut by_header = None;
    for line in lines.into_iter().take(150) {
        let trimmed = line.trim_start();
        if let Some(value) = said(trimmed, "machine") {
            if let Some(machine) = machine_named(&value) {
                return Some(machine);
            }
            continue;
        }
        if by_header.is_none()
            && let Some(header) = trimmed.strip_prefix("#include").or_else(|| trimmed.strip_prefix("# include"))
        {
            let header = header.trim().trim_matches(['<', '>', '"']).to_ascii_lowercase();
            by_header = match header.as_str() {
                "genesis.h" => Some(Machine::MegaDrive),
                "c64.h" => Some(Machine::C64),
                "c128.h" => Some(Machine::C128),
                "vic20.h" => Some(Machine::Vic20),
                "plus4.h" | "c16.h" => Some(Machine::Plus4),
                "pet.h" => Some(Machine::Pet),
                "atari2600.h" => Some(Machine::Atari2600),
                "spectrum.h" | "arch/zx.h" | "zxlib.h" => Some(Machine::Spectrum48),
                "arch/sms.h" | "sms.h" | "smslib.h" => Some(Machine::MasterSystem),
                "arch/coleco.h" | "coleco.h" => Some(Machine::ColecoVision),
                h if ["exec/", "proto/", "clib/", "intuition/", "dos/", "graphics/", "hardware/", "libraries/", "workbench/", "devices/"].iter().any(|p| h.starts_with(p)) => Some(Machine::Amiga),
                _ => None,
            };
        }
    }
    by_header
}

/// The value of a `// name: value` line (or `/* name: value */`), lower case.
fn said(line: &str, name: &str) -> Option<String> {
    let comment = line.strip_prefix("//").or_else(|| line.strip_prefix("/*"))?;
    let comment = comment.trim_start_matches(['/', '*', ' ', '\t']).trim_end().trim_end_matches("*/").to_ascii_lowercase();
    let value = comment.strip_prefix(name)?.trim_start().strip_prefix(':')?;
    Some(value.trim().to_string())
}

/// The compiler C source asks for with a `// compiler:` line, if it does.
pub fn compiler_said<'a>(lines: impl IntoIterator<Item = &'a str>) -> Option<&'static Compiler> {
    lines.into_iter().take(150).find_map(|line| said(line.trim_start(), "compiler").and_then(|value| by_id(value.split_whitespace().next().unwrap_or(""))))
}

fn machine_named(value: &str) -> Option<Machine> {
    let value: String = value.chars().filter(|c| c.is_ascii_alphanumeric()).collect();
    Some(match value.as_str() {
        "c64" | "commodore64" => Machine::C64,
        "c128" => Machine::C128,
        "vic20" => Machine::Vic20,
        "plus4" | "c16" => Machine::Plus4,
        "pet" => Machine::Pet,
        "spectrum" | "zx" | "zxspectrum" | "spectrum48" => Machine::Spectrum48,
        "sms" | "mastersystem" => Machine::MasterSystem,
        "gamegear" | "gg" => Machine::GameGear,
        "coleco" | "colecovision" => Machine::ColecoVision,
        "megadrive" | "genesis" => Machine::MegaDrive,
        "amiga" => Machine::Amiga,
        "atari2600" | "2600" | "vcs" => Machine::Atari2600,
        _ => return None,
    })
}

/// The C files built together with the one in front: it, then the other .c files in its folder that don't have a
/// main of their own.
pub fn c_sources(main: &Path) -> Vec<PathBuf> {
    let mut sources = vec![main.to_path_buf()];
    let Some(folder) = main.parent() else { return sources };
    let Ok(entries) = std::fs::read_dir(folder) else { return sources };
    let mut others: Vec<PathBuf> = entries.flatten().map(|e| e.path()).filter(|p| p.extension().is_some_and(|e| e.eq_ignore_ascii_case("c")) && p.file_name() != main.file_name()).collect();
    others.sort();
    for other in others {
        let has_main = std::fs::read_to_string(&other).is_ok_and(|text| text.lines().any(defines_main));
        if !has_main {
            sources.push(other);
        }
    }
    sources
}

fn defines_main(line: &str) -> bool {
    let words: String = line.split_whitespace().collect::<Vec<_>>().join(" ");
    ["int main(", "void main(", "int main (", "void main (", "main("].iter().any(|start| words.starts_with(start))
}

/// What to say when no compiler for a machine is installed: which there are, and where to get them.
pub fn not_found(machine: Machine, basic: bool) -> String {
    let compilers = for_machine(machine, basic);
    let named: Vec<String> = compilers.iter().map(|c| format!("{} ({})", c.name, c.website)).collect();
    let variables: Vec<&str> = compilers.iter().flat_map(|c| c.homes.iter().copied()).collect();
    let also = if variables.is_empty() { String::new() } else { crate::i18n::tf!(", in the folder {} names,", variables.join(" or ")) };
    if basic {
        return crate::i18n::tf!("Boriel's ZX Basic compiler (zxbc) wasn't found on this computer. Get it from {}: Limoniter looks for zxbc on the PATH{also} and in a folder named zxbasic in Program Files, Downloads or on the Desktop.", compilers.first().map_or("", |c| c.website));
    }
    crate::i18n::tf!("No C compiler for the {} was found on this computer. Install one of {}: Limoniter looks on the PATH{also} and where each usually goes, or in a folder named for it in Program Files, Downloads or on the Desktop.", machine.name(), named.join(", "))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn job(id: &str, machine: Machine, sources: &[&str]) -> Job {
        let compiler = by_id(id).unwrap();
        Job {
            compiler,
            program: PathBuf::from(format!("/bin/{}", compiler.programs(machine).first().copied().unwrap_or("sgdk"))),
            machine,
            main: PathBuf::from(sources[0]),
            sources: sources.iter().map(PathBuf::from).collect(),
            folder: PathBuf::from("/src"),
            flags: Vec::new(),
            stem: "prog".into(),
            sasc: false,
            libs: Vec::new(),
        }
    }

    /// A command line as text, its paths with / whichever system joined them (Windows joins with \\).
    fn line_of(steps: &[(PathBuf, Vec<String>)], n: usize) -> String {
        let (program, args) = &steps[n];
        format!("{} {}", program.file_name().unwrap().to_string_lossy(), args.join(" ")).replace('\\', "/")
    }

    /// Each command line is the one its compiler's documentation gives (NOTES.md says where).
    #[test]
    fn each_compiler_is_run_as_its_documentation_says() {
        let work = Path::new("/w");
        let zxbc = commands(&job("zxbc", Machine::Spectrum48, &["/src/game.bas"]), work);
        assert_eq!(line_of(&zxbc, 0), "zxbc /src/game.bas --tap --BASIC --autorun -o /w/prog.tap");
        let cc65 = commands(&job("cc65", Machine::Vic20, &["/src/a.c", "/src/b.c"]), work);
        assert_eq!(cc65.len(), 3, "each file compiled, then linked");
        assert_eq!(line_of(&cc65, 0), "cl65 -t vic20 -O -c -o /w/0.o /src/a.c");
        assert_eq!(line_of(&cc65, 2), "cl65 -t vic20 -o /w/prog.prg /w/0.o /w/1.o");
        assert_eq!(line_of(&commands(&job("llvm-mos", Machine::C64, &["/src/a.c"]), work), 0), "mos-c64-clang -Os -o /w/prog.prg /src/a.c");
        assert_eq!(line_of(&commands(&job("llvm-mos", Machine::Atari2600, &["/src/a.c"]), work), 0), "mos-atari2600-4k-clang -Os -o /w/prog.a26 /src/a.c");
        assert_eq!(line_of(&commands(&job("z88dk", Machine::Spectrum48, &["/src/a.c"]), work), 0), "zcc +zx -vn -create-app -o /w/prog.bin /src/a.c");
        assert_eq!(line_of(&commands(&job("z88dk", Machine::GameGear, &["/src/a.c"]), work), 0), "zcc +sms -subtype=gamegear -vn -create-app -o /w/prog.bin /src/a.c");
        let sdcc = commands(&job("sdcc", Machine::Spectrum48, &["/src/a.c"]), work);
        assert_eq!(line_of(&sdcc, 0), "sdcc -mz80 -c -o /w/0.rel /src/a.c");
        assert_eq!(line_of(&sdcc, 1), "sdcc -mz80 --code-loc 0x8000 --data-loc 0xC000 --no-std-crt0 -o /w/prog.ihx /w/0.rel");
        let mut sgdk = job("sgdk", Machine::MegaDrive, &["/src/main.c"]);
        sgdk.program = PathBuf::from("/opt/sgdk");
        let steps = commands(&sgdk, work);
        assert_eq!(steps[0].1.iter().map(|a| a.replace('\\', "/")).collect::<Vec<_>>(), ["-f", "/opt/sgdk/makefile.gen"]);
        assert_eq!(line_of(&commands(&job("vbcc", Machine::Amiga, &["/src/a.c"]), work), 0), "vc +aos68k -c99 -O1 -o /w/prog /src/a.c -lamiga");
        assert_eq!(line_of(&commands(&job("amiga-gcc", Machine::Amiga, &["/src/a.c"]), work), 0), "m68k-amigaos-gcc -Os -noixemul -o /w/prog /src/a.c");
        // An Amiga project's makefile chooses the configuration or C library, and the libraries go after the files.
        let mut kick13 = job("vbcc", Machine::Amiga, &["/src/a.c"]);
        kick13.flags = vec!["+kick13".into(), "-cpu=68000".into()];
        kick13.libs = vec!["-lmieee".into(), "-lamiga".into()];
        assert_eq!(line_of(&commands(&kick13, work), 0), "vc -c99 -O1 +kick13 -cpu=68000 -o /w/prog /src/a.c -lmieee -lamiga");
        let mut nix13 = job("amiga-gcc", Machine::Amiga, &["/src/a.c"]);
        nix13.flags = vec!["-mcrt=nix13".into(), "-m68000".into()];
        nix13.libs = vec!["-lm".into(), "-lamiga".into()];
        assert_eq!(line_of(&commands(&nix13, work), 0), "m68k-amigaos-gcc -Os -mcrt=nix13 -m68000 -o /w/prog /src/a.c -lm -lamiga");
    }

    /// Lines as each compiler prints them: cc65's from running it here, the others' from their documentation and
    /// source (see NOTES.md).
    #[test]
    fn problems_are_read_as_each_compiler_prints_them() {
        let p = |file: &str, line: usize, message: &str, warning: bool| Some(Problem { file: file.into(), line, message: message.into(), warning });
        // zxbc, from src/api/errmsg.py: "%s:%i: error:%s %s" and "%s:%i: warning: [W%d] %s".
        assert_eq!(problem_in("game.bas:12: error: Syntax Error. Unexpected token 'PRINT' <PRINT>"), p("game.bas", 12, "Syntax Error. Unexpected token 'PRINT' <PRINT>", false));
        assert_eq!(problem_in("game.bas:5: warning: [W100] Using default implicit type 'float' for 'x'"), p("game.bas", 5, "[W100] Using default implicit type 'float' for 'x'", true));
        // cc65 2.19, run here on a broken file.
        assert_eq!(problem_in("bad.c(6): Error: ';' expected"), p("bad.c", 6, "';' expected", false));
        assert_eq!(problem_in("/tmp/w/../cc/bad.c(6): Error: Undefined symbol: 'y'"), p("/tmp/w/../cc/bad.c", 6, "Undefined symbol: 'y'", false));
        assert_eq!(problem_in("ok.c(2): Warning: 'u' is defined but never used"), p("ok.c", 2, "'u' is defined but never used", true));
        assert_eq!(problem_in("ld65: Error: 1 unresolved external(s) found - cannot create output file"), None, "the linker names no line");
        assert_eq!(problem_in("  link.s(27)"), None);
        // gcc and clang (amiga-gcc, llvm-mos, SGDK), and z88dk's sccz80 (src/sccz80/error.c: "%s:%d:%d: %s: ").
        assert_eq!(problem_in("src/main.c:7:5: error: 'y' undeclared (first use in this function)"), p("src/main.c", 7, "'y' undeclared (first use in this function)", false));
        assert_eq!(problem_in("C:\\games\\main.c:7:5: warning: unused variable 'u' [-Wunused-variable]"), p("C:\\games\\main.c", 7, "unused variable 'u' [-Wunused-variable]", true));
        assert_eq!(problem_in("main.c:3:1: fatal error: genesis.h: No such file or directory"), p("main.c", 3, "genesis.h: No such file or directory", false));
        assert_eq!(problem_in("main.c:7:5: note: each undeclared identifier is reported only once"), None);
        assert_eq!(problem_in("main.c:(.text+0x12): undefined reference to `f'"), None);
        // SDCC: "test.c:6: error 159: in function test" (as on its mailing list), and its syntax errors.
        assert_eq!(problem_in("test.c:6: error 20: Undefined identifier 'y'"), p("test.c", 6, "Undefined identifier 'y'", false));
        assert_eq!(problem_in("test.c:9: warning 112: function 'f' implicit declaration"), p("test.c", 9, "function 'f' implicit declaration", true));
        assert_eq!(problem_in("test.c:3: syntax error: token -> 'x' ; column 5"), p("test.c", 3, "syntax error: token -> 'x' ; column 5", false));
        // vbcc, from main.c's do_error: "%s %d in line %d of \"%s\": ", and errors.h's error 82.
        assert_eq!(problem_in("error 82 in line 4 of \"hello.c\": unknown identifier <foo>"), p("hello.c", 4, "unknown identifier <foo>", false));
        assert_eq!(problem_in("warning 214 in line 9 of \"hello.c\": suspicious format string"), p("hello.c", 9, "suspicious format string", true));
        assert_eq!(problem_in(">  foo = 1;"), None, "vbcc's copy of the line");
        assert_eq!(problem_in("Compiling hello.c"), None);
    }

    #[test]
    fn the_machine_c_is_for_is_said_or_shown_by_its_headers() {
        assert_eq!(c_machine(["// machine: c64", "#include <stdio.h>"]), Some(Machine::C64));
        assert_eq!(c_machine(["/* machine: Mega Drive */"]), Some(Machine::MegaDrive));
        assert_eq!(c_machine(["#include <genesis.h>"]), Some(Machine::MegaDrive));
        assert_eq!(c_machine(["#include <exec/types.h>", "#include <proto/dos.h>"]), Some(Machine::Amiga));
        assert_eq!(c_machine(["#include <arch/zx.h>"]), Some(Machine::Spectrum48));
        assert_eq!(c_machine(["#include <vic20.h>"]), Some(Machine::Vic20));
        assert_eq!(c_machine(["#include <exec/types.h>", "// machine: c64"]), Some(Machine::C64), "what's said comes first");
        assert_eq!(c_machine(["#include <stdio.h>", "int main(void) { return 0; }"]), None);
        assert_eq!(compiler_said(["// compiler: llvm-mos"]).map(|c| c.id), Some("llvm-mos"));
        assert_eq!(compiler_said(["// compiler: amiga gcc"]).map(|c| c.id), None, "one word");
        assert_eq!(compiler_said(["// compiler: amigagcc"]).map(|c| c.id), Some("amiga-gcc"));
        assert_eq!(compiler_said(["// Compiler: SDCC"]).map(|c| c.id), Some("sdcc"));
    }

    #[test]
    fn compilers_are_offered_for_their_machines() {
        let ids = |m: Machine, basic: bool| for_machine(m, basic).iter().map(|c| c.id).collect::<Vec<_>>();
        assert_eq!(ids(Machine::C64, false), ["cc65", "llvm-mos"]);
        assert_eq!(ids(Machine::Spectrum48, false), ["z88dk", "sdcc"]);
        assert_eq!(ids(Machine::Spectrum48, true), ["zxbc"]);
        assert_eq!(ids(Machine::Amiga, false), ["amiga-gcc", "vbcc", "sasc"], "SAS/C, through vamos, last but for SAS/C projects");
        assert_eq!(ids(Machine::MegaDrive, false), ["sgdk"]);
        let words = not_found(Machine::Amiga, false);
        assert!(words.contains("codeberg.org/bebbo/amiga-gcc") && words.contains("VBCC"), "{words}");
        assert!(not_found(Machine::Spectrum48, true).contains("github.com/boriel-basic/zxbasic"));
    }

    #[test]
    fn compilers_are_found_where_their_variables_say() {
        let base = std::env::temp_dir().join(format!("limoniter-find-cc-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        std::fs::create_dir_all(base.join("sgdk")).unwrap();
        std::fs::write(base.join("sgdk/makefile.gen"), "").unwrap();
        let compiler = Compiler { homes: &["LIMONITER_TEST_GDK"], usual: &[], ..KNOWN[5] };
        // Only this test sets this variable.
        unsafe { std::env::set_var("LIMONITER_TEST_GDK", base.join("sgdk")) };
        assert_eq!(compiler.find(Machine::MegaDrive), Some(base.join("sgdk")));
        unsafe { std::env::remove_var("LIMONITER_TEST_GDK") };
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn sdcc_intel_hex_is_read_into_bytes_at_their_address() {
        let hex = "\
:03800000C3038037
:02800300180063
:00000001FF
";
        let (low, bytes) = read_intel_hex(hex).unwrap();
        assert_eq!(low, 0x8000);
        assert_eq!(bytes, [0xC3, 0x03, 0x80, 0x18, 0x00]);
        assert_eq!(read_intel_hex(":0380000000000000\n"), None, "a bad checksum");
    }

    #[test]
    fn the_c_files_built_together_leave_out_other_programs() {
        let base = std::env::temp_dir().join(format!("limoniter-c-sources-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        std::fs::create_dir_all(&base).unwrap();
        std::fs::write(base.join("main.c"), "int main(void)\n{\n  return draw();\n}\n").unwrap();
        std::fs::write(base.join("draw.c"), "int draw(void) { return 0; }\n").unwrap();
        std::fs::write(base.join("tool.c"), "int main (void) { return 1; }\n").unwrap();
        std::fs::write(base.join("notes.txt"), "").unwrap();
        assert_eq!(c_sources(&base.join("main.c")), [base.join("main.c"), base.join("draw.c")]);
        let _ = std::fs::remove_dir_all(&base);
    }

    /// With cc65 installed: the C64 starter compiles to a program loaded at $0801 with a SYS line, and a broken
    /// file gives its problems by line.
    #[test]
    fn cc65_builds_the_starter_and_reports_problems() {
        let Some(program) = by_id("cc65").unwrap().find(Machine::C64) else {
            eprintln!("cc65 isn't installed: skipped");
            return;
        };
        let base = std::env::temp_dir().join(format!("limoniter-cc65-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        std::fs::create_dir_all(&base).unwrap();
        let source = base.join("hello.c");
        std::fs::write(&source, crate::c_starters::C64).unwrap();
        let mut job = job("cc65", Machine::C64, &[source.to_str().unwrap()]);
        job.program = program;
        job.folder = base.clone();
        let built = build(&job, &base.join("work")).unwrap();
        assert!(built.ok(), "{:?} {:?}", built.problems, built.printed);
        let (name, bytes) = built.made.clone().unwrap();
        assert_eq!(name, "prog.prg");
        assert_eq!(&bytes[..2], [0x01, 0x08], "loaded at $0801");
        assert!(bytes.windows(4).any(|w| w == b"2061"), "a SYS 2061 line starts it");
        let run = runnable(&job, &name, bytes).unwrap();
        assert_eq!((run.machine, run.format), (Machine::C64, Format::Prg));
        assert!(!base.join("hello.o").exists(), "nothing left beside the source");
        std::fs::write(&source, "int main(void)\n{\n    int x = 1\n    return y;\n}\n").unwrap();
        let broken = build(&job, &base.join("work")).unwrap();
        assert!(!broken.ok());
        assert!(broken.problems.iter().any(|p| p.line == 4 && p.message.contains("';' expected")), "{:?}", broken.problems);
        let _ = std::fs::remove_dir_all(&base);
    }
}
