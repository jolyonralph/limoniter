//! The Commodore 64 demo scene's own assemblers, run the way the others are: KickAssembler (a Java program, its
//! KickAss.jar run by Java) and 64tass. Each makes a .prg, and says which line made which bytes (KickAssembler in
//! the debug file it writes for C64 Debugger, 64tass in its listing), so hover, the Functions panel and debugging
//! in VICE work as they do with ACME. Neither is bundled with Limoniter: KickAssembler is freeware from
//! theweb.dk, and 64tass is GPL, from SourceForge.

use crate::asm::{self, Assembly, Problem};
use crate::assemblers::{Assembled, file_index, run};
use std::path::{Path, PathBuf};

/// Java, to run KickAssembler: JAVA_HOME's, or the one on the path.
pub fn find_java() -> Option<PathBuf> {
    let exe = if cfg!(windows) { "java.exe" } else { "java" };
    if let Some(home) = std::env::var_os("JAVA_HOME") {
        let candidate = PathBuf::from(home).join("bin").join(exe);
        if candidate.is_file() {
            return Some(candidate);
        }
    }
    let path = std::env::var_os("PATH")?;
    std::env::split_paths(&path).map(|folder| folder.join(exe)).find(|c| c.is_file())
}

/// Where KickAss.jar is: on the path, in a folder of its own under the home folder, Downloads or the usual places
/// for programs, or named by KICKASS_JAR.
pub fn find_kickass() -> Option<PathBuf> {
    if let Some(named) = std::env::var_os("KICKASS_JAR").map(PathBuf::from).filter(|p| p.is_file()) {
        return Some(named);
    }
    // One downloaded by Limoniter, when asked.
    if let Some(jar) = kickass_folder().map(|f| f.join("KickAss.jar")).filter(|p| p.is_file()) {
        return Some(jar);
    }
    let mut folders: Vec<PathBuf> = std::env::var_os("PATH").map(|p| std::env::split_paths(&p).collect()).unwrap_or_default();
    let mut roots: Vec<PathBuf> = Vec::new();
    for variable in ["HOME", "USERPROFILE"] {
        if let Some(home) = std::env::var_os(variable).map(PathBuf::from) {
            roots.extend(["", "Downloads", "Desktop", "tools", "Tools", "bin", "c64", "C64", "Applications", "Documents"].map(|f| home.join(f)));
        }
    }
    for variable in ["ProgramFiles", "ProgramFiles(x86)", "LOCALAPPDATA"] {
        if let Some(value) = std::env::var_os(variable) {
            roots.push(PathBuf::from(value));
        }
    }
    if cfg!(windows) {
        roots.extend(["C:\\", "C:\\c64", "C:\\C64", "C:\\Tools", "D:\\c64"].map(PathBuf::from));
    } else {
        roots.extend(["/opt", "/usr/local/share", "/usr/share", "/usr/local/lib", "/usr/lib"].map(PathBuf::from));
    }
    folders.extend(roots.iter().cloned());
    // One or two levels into folders whose names say KickAssembler.
    for root in &roots {
        let Ok(entries) = std::fs::read_dir(root) else { continue };
        let mut found: Vec<PathBuf> = entries.flatten().map(|e| e.path()).filter(|p| p.is_dir() && p.file_name().is_some_and(|n| n.to_string_lossy().to_ascii_lowercase().contains("kick"))).collect();
        found.sort();
        for folder in found {
            folders.push(folder.join("bin"));
            if let Ok(inside) = std::fs::read_dir(&folder) {
                folders.extend(inside.flatten().map(|e| e.path()).filter(|p| p.is_dir()));
            }
            folders.push(folder);
        }
    }
    folders.into_iter().map(|f| f.join("KickAss.jar")).find(|c| c.is_file())
}

/// Where KickAssembler's own download comes from: its author's website.
pub const KICKASS_DOWNLOAD: &str = "https://theweb.dk/KickAssembler/KickAssembler.zip";

/// What the person is told before KickAssembler is downloaded.
pub const KICKASS_NOTE: &str = "KickAssembler is Mads Nielsen's, free to download from theweb.dk. Neither its download nor its website states a licence, so Limoniter doesn't include it: it fetches it from there only when you ask, and keeps it beside the settings. Check its terms yourself before you pass it on. It needs Java.";

/// Where a downloaded KickAssembler is kept.
fn kickass_folder() -> Option<PathBuf> {
    if cfg!(test) {
        return None;
    }
    crate::settings::data_folder().map(|dir| dir.join("kickassembler"))
}

/// Downloads KickAssembler from its website and keeps KickAss.jar and its manual (not KickAss.cfg, which can tell
/// it to start an emulator itself). Gives the jar's path.
pub fn download_kickass() -> Result<PathBuf, String> {
    let folder = kickass_folder().ok_or(crate::i18n::tr("There's nowhere to keep it: Limoniter's settings folder couldn't be found."))?;
    let response = ureq::get(KICKASS_DOWNLOAD).header("User-Agent", "Limoniter").call().map_err(|e| crate::i18n::tf!("theweb.dk couldn't be reached: {e}"))?;
    let zip = response.into_body().with_config().limit(32 << 20).read_to_vec().map_err(|e| crate::i18n::tf!("The download stopped: {e}"))?;
    unpack_kickass(zip, &folder)
}

/// Takes KickAss.jar and the manual out of KickAssembler's zip into a folder, with a note of where they came from.
fn unpack_kickass(zip: Vec<u8>, folder: &Path) -> Result<PathBuf, String> {
    let volume = crate::media::open(std::sync::Arc::new(zip), "KickAssembler.zip")?;
    let entries = volume.list(None)?;
    let jar = entries.iter().find(|e| e.name.eq_ignore_ascii_case("KickAss.jar")).ok_or(crate::i18n::tr("The download has no KickAss.jar in it."))?;
    std::fs::create_dir_all(folder).map_err(|e| crate::i18n::tf!("{} couldn't be made: {e}", folder.display()))?;
    let path = folder.join("KickAss.jar");
    std::fs::write(&path, volume.read(jar)?).map_err(|e| crate::i18n::tf!("{} couldn't be saved: {e}", path.display()))?;
    if let Some(manual) = entries.iter().find(|e| e.name.eq_ignore_ascii_case("KickAssembler.pdf"))
        && let Ok(bytes) = volume.read(manual)
    {
        let _ = std::fs::write(folder.join("KickAssembler.pdf"), bytes);
    }
    let _ = std::fs::write(folder.join("README.txt"), format!("KickAssembler by Mads Nielsen, downloaded from {KICKASS_DOWNLOAD} when asked.\nNo licence is stated with it; see http://theweb.dk/KickAssembler/ for its terms.\n"));
    Ok(path)
}

/// A problem as KickAssembler or 64tass prints it. Some(None) for a line in their way that isn't an error (a
/// warning); None for a line in neither's way.
///
/// KickAssembler: "  (/home/me/demo.asm 3:5) Error: Unknown symbol 'nowhere'".
/// 64tass: "demo.asm:3:8: error: not defined symbol 'nowhere'".
pub fn problem_line(line: &str) -> Option<Option<(String, usize, String)>> {
    let trimmed = line.trim_start();
    if let Some(rest) = trimmed.strip_prefix('(')
        && let Some((place, message)) = rest.split_once(") ")
        && let Some((file, at)) = place.rsplit_once(' ')
        && let Some((number, column)) = at.split_once(':')
        && number.chars().all(|c| c.is_ascii_digit())
        && column.chars().all(|c| c.is_ascii_digit())
        && !number.is_empty()
    {
        let message = message.trim();
        return Some(message.strip_prefix("Error:").map(|m| (file.to_string(), number.parse().unwrap_or(1), m.trim().to_string())));
    }
    // file:line:column: kind: message, read from the left of ": error" so a drive letter's colon is kept.
    for (kind, error) in [(": error: ", true), (": fatal error: ", true), (": warning: ", false), (": note: ", false)] {
        let Some((place, message)) = line.split_once(kind) else { continue };
        let mut parts = place.rsplitn(3, ':');
        let (Some(column), Some(number), Some(file)) = (parts.next(), parts.next(), parts.next()) else { continue };
        if column.is_empty() || number.is_empty() || !column.chars().all(|c| c.is_ascii_digit()) || !number.chars().all(|c| c.is_ascii_digit()) {
            continue;
        }
        return Some(error.then(|| (file.to_string(), number.parse().unwrap_or(1), message.trim().to_string())));
    }
    None
}

/// The problems printed, each once (KickAssembler lists them, then shows the first again).
fn problems(printed: &[String], assembly: &mut Assembly, source: &Path) {
    for line in printed {
        if let Some(Some((file, number, message))) = problem_line(line) {
            let file = file_index(assembly, &file, source);
            let problem = Problem { file, line: number.saturating_sub(1), message };
            if !assembly.problems.iter().any(|p| p.file == problem.file && p.line == problem.line && p.message == problem.message) {
                assembly.problems.push(problem);
            }
        }
    }
}

/// The program made: as it is when it loads where BASIC starts or below (it has its own BASIC line, or none is
/// wanted), and otherwise left for Limoniter to put a SYS line in front of, as with its own assembler.
fn finish(mut assembly: Assembly, prg: Option<Vec<u8>>, printed: Vec<String>) -> Assembled {
    let mut tape = None;
    if let Some(prg) = prg.filter(|p| p.len() > 2) {
        let load = u16::from_le_bytes([prg[0], prg[1]]) as i64;
        assembly.chunks = vec![asm::Chunk { address: load, bytes: prg[2..].to_vec() }];
        if load <= 0x0801 {
            tape = Some((crate::running::Format::Prg, prg));
        }
    }
    // The lines' bytes, from the program, where a listing left them out.
    if let Some(chunk) = assembly.chunks.first().cloned() {
        for placed in &mut assembly.lines {
            let from = (placed.address - chunk.address) as usize;
            if placed.address >= chunk.address
                && let Some(bytes) = chunk.bytes.get(from..from + placed.bytes.len())
            {
                placed.bytes = bytes.to_vec();
            }
        }
    }
    assembly.entry = ["start", "main", "begin", "entry", "init"]
        .iter()
        .find_map(|name| assembly.symbols.iter().find(|(label, _)| label.eq_ignore_ascii_case(name)).map(|(_, v)| *v))
        .or_else(|| assembly.lines.iter().find(|p| p.address >= 0x080D || p.address < 0x0801).map(|p| p.address));
    Assembled { assembly, source: None, tape, printed }
}

/// KickAssembler: `java -jar KickAss.jar source -o out.prg -debugdump`, the debug file giving each line's address.
pub fn kickass(program: &Path, source: &Path, work: &Path) -> Result<Assembled, String> {
    let out = work.join("out.prg");
    let folder = source.parent().unwrap_or(Path::new("."));
    let jar = program.extension().is_some_and(|e| e.eq_ignore_ascii_case("jar"));
    // A KickAss.cfg beside the jar adds its options to every run; one that starts an emulator would start a
    // second, so then a copy of the jar on its own is run.
    let mut program = program.to_path_buf();
    if jar && std::fs::read_to_string(program.with_file_name("KickAss.cfg")).is_ok_and(|cfg| cfg.lines().any(|l| l.trim_start().starts_with("-execute"))) {
        let copy = work.join("KickAss.jar");
        if std::fs::copy(&program, &copy).is_ok() {
            program = copy;
        }
    }
    let mut args: Vec<String> = Vec::new();
    let runner = if jar {
        let java = find_java().ok_or(crate::i18n::tr("KickAssembler needs Java, which wasn't found: install it (from adoptium.net, or your system's packages), or set JAVA_HOME."))?;
        args.extend(["-jar".to_string(), program.display().to_string()]);
        java
    } else {
        program.clone()
    };
    args.extend([source.display().to_string(), "-o".into(), out.display().to_string(), "-debugdump".into(), "-symbolfiledir".into(), work.display().to_string()]);
    let (ok, printed) = run(&runner, &args, folder)?;
    let mut assembly = Assembly { files: vec![source.to_path_buf()], ..Assembly::default() };
    problems(&printed, &mut assembly, source);
    if !ok && assembly.problems.is_empty() {
        assembly.problems.push(Problem { file: 0, line: 0, message: printed.iter().filter(|l| !l.starts_with("//")).cloned().collect::<Vec<_>>().join(" ") });
    }
    if let Ok(debug) = std::fs::read_to_string(work.join("out.dbg")) {
        read_debug_file(&debug, &mut assembly, source);
    }
    let prg = if ok { std::fs::read(&out).ok() } else { None };
    Ok(finish(assembly, prg, printed))
}

/// KickAssembler's debug file (the one C64 Debugger reads): its sources by number, each block's runs of bytes as
/// "$0810,$0811,1,6,5,6,7" (first and last address, source, line and column), and the labels.
pub fn read_debug_file(text: &str, assembly: &mut Assembly, source: &Path) {
    let mut files: std::collections::HashMap<String, usize> = Default::default();
    let mut part = "";
    for line in text.lines() {
        let line = line.trim();
        if line.starts_with("<Sources") {
            part = "sources";
            continue;
        }
        if line.starts_with("<Block") || line.starts_with("<Segment") {
            part = "block";
            continue;
        }
        if line.starts_with("<Labels") {
            part = "labels";
            continue;
        }
        if line.starts_with('<') {
            if line.starts_with("</") {
                part = if line.starts_with("</Block") { "" } else { part };
            }
            continue;
        }
        match part {
            "sources" => {
                if let Some((n, name)) = line.split_once(',') {
                    // KickAssembler's own included file isn't the user's.
                    if !name.starts_with("KickAss.jar:") {
                        let index = file_index(assembly, name, source);
                        files.insert(n.to_string(), index);
                    }
                }
            }
            "block" => {
                let fields: Vec<&str> = line.split(',').collect();
                if fields.len() < 4 {
                    continue;
                }
                let hex = |s: &str| i64::from_str_radix(s.trim().trim_start_matches('$'), 16).ok();
                let (Some(first), Some(last), Some(&file), Ok(number)) = (hex(fields[0]), hex(fields[1]), files.get(fields[2]), fields[3].parse::<usize>()) else { continue };
                if last < first {
                    continue;
                }
                assembly.lines.push(asm::Placed { file, line: number.saturating_sub(1), address: first, bytes: vec![0; (last - first + 1) as usize] });
            }
            "labels" => {
                // SEGMENT,ADDRESS,NAME,...
                let fields: Vec<&str> = line.split(',').collect();
                if fields.len() >= 3
                    && let Ok(value) = i64::from_str_radix(fields[1].trim_start_matches('$'), 16)
                {
                    assembly.symbols.insert(fields[2].to_string(), value);
                }
            }
            _ => {}
        }
    }
}

/// 64tass: `64tass -a source -o out.prg -L out.lst --line-numbers -l out.lab`. -a reads text as ASCII and makes
/// it PETSCII, as most C64 sources for it expect.
pub fn tass64(program: &Path, source: &Path, work: &Path) -> Result<Assembled, String> {
    let out = work.join("out.prg");
    let listing = work.join("out.lst");
    let labels = work.join("out.lab");
    let folder = source.parent().unwrap_or(Path::new("."));
    let args = vec![
        "-a".to_string(),
        "--no-caret-diag".into(),
        "-m".into(),
        "--line-numbers".into(),
        "-L".into(),
        listing.display().to_string(),
        "-l".into(),
        labels.display().to_string(),
        "-o".into(),
        out.display().to_string(),
        source.display().to_string(),
    ];
    let (ok, printed) = run(program, &args, folder)?;
    let mut assembly = Assembly { files: vec![source.to_path_buf()], ..Assembly::default() };
    problems(&printed, &mut assembly, source);
    if !ok && assembly.problems.is_empty() {
        assembly.problems.push(Problem { file: 0, line: 0, message: printed.join(" ") });
    }
    if let Ok(text) = std::fs::read_to_string(&listing) {
        read_listing(&text, &mut assembly, source);
    }
    if let Ok(text) = std::fs::read_to_string(&labels) {
        // "name		= $c000"
        for line in text.lines() {
            if let Some((name, value)) = line.split_once('=')
                && let Some(value) = value.trim().strip_prefix('$').and_then(|v| i64::from_str_radix(v, 16).ok())
            {
                assembly.symbols.insert(name.trim().to_string(), value);
            }
        }
    }
    let prg = if ok { std::fs::read(&out).ok() } else { None };
    Ok(finish(assembly, prg, printed))
}

/// 64tass's listing, with line numbers: "12\t.c000\ta9 01\t\tlda #1", a line going on as "\t>c004\t05 06 ...";
/// ":2\t;******  Processing file: inc.asm" says which file the numbers are of from there on.
pub fn read_listing(text: &str, assembly: &mut Assembly, source: &Path) {
    let mut by_number: std::collections::HashMap<String, usize> = Default::default();
    let mut file = 0;
    let mut last_line: Option<usize> = None;
    for line in text.lines() {
        if let Some(rest) = line.strip_prefix(':') {
            let Some((n, what)) = rest.split_once('\t') else { continue };
            let name = what.rsplit_once("file: ").map(|(_, f)| f.trim()).unwrap_or("");
            file = match by_number.get(n) {
                Some(index) => *index,
                None if !name.is_empty() => {
                    let index = file_index(assembly, name, source);
                    by_number.insert(n.to_string(), index);
                    index
                }
                None => file,
            };
            continue;
        }
        if line.starts_with(';') || line.is_empty() {
            continue;
        }
        let mut fields = line.split('\t');
        let number = fields.next().unwrap_or("");
        let Some(place) = fields.next() else { continue };
        let line_number = if number.is_empty() { last_line } else { number.parse::<usize>().ok().map(|n| n.saturating_sub(1)) };
        let Some(line_number) = line_number else { continue };
        if !number.is_empty() {
            last_line = Some(line_number);
        }
        // . code, > data; = a value, with no bytes.
        let Some(mut address) = place.strip_prefix(['.', '>']).and_then(|a| i64::from_str_radix(a, 16).ok()) else { continue };
        // With .logical in the source, a column of where the code runs comes before the bytes: empty, or four
        // hex digits.
        let mut hex = fields.next().unwrap_or("");
        if hex.is_empty() || hex.len() == 4 && !hex.contains(' ') {
            if let Ok(runs_at) = i64::from_str_radix(hex, 16) {
                address = runs_at;
            }
            hex = fields.next().unwrap_or("");
        }
        let bytes: Vec<u8> = hex.split_whitespace().map_while(|b| u8::from_str_radix(b, 16).ok().filter(|_| b.len() == 2)).collect();
        if bytes.is_empty() {
            continue;
        }
        match assembly.lines.last_mut() {
            Some(previous) if number.is_empty() && previous.file == file && previous.line == line_number && previous.address + previous.bytes.len() as i64 == address => previous.bytes.extend(bytes),
            _ => assembly.lines.push(asm::Placed { file, line: line_number, address, bytes }),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// With LIMONITER_KICKASS (KickAss.jar) and LIMONITER_64TASS naming the programs: each assembles its demo,
    /// Limoniter's own assembler makes the same bytes from the same source, every line the real one placed is at
    /// the same address with the same bytes, and problems come back on their lines.
    #[test]
    #[ignore]
    fn real_c64_assemblers_give_the_same_program() {
        use crate::assemblers::{Cpu, assemble_external, assemble_own, external};
        let folder = std::env::temp_dir().join(format!("limoniter-c64-asm-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&folder);
        std::fs::create_dir_all(&folder).unwrap();
        for (variable, id, name, text, wrong) in [
            ("LIMONITER_KICKASS", "kickass", "kick_demo.asm", include_str!("asm/testdata/kick_demo.asm"), ".pc = $c000\n    lda #1\n    ldq #2\n    jmp nowhere\n"),
            ("LIMONITER_64TASS", "64tass", "tass_demo.asm", include_str!("asm/testdata/tass_demo.asm"), "  *= $c000\n  lda #1\n  jmp nowhere\n"),
        ] {
            let Ok(program) = std::env::var(variable) else { continue };
            let source = folder.join(name);
            std::fs::write(&source, text).unwrap();
            let theirs = assemble_external(external(id).unwrap(), Path::new(&program), &source).unwrap();
            assert!(theirs.assembly.ok(), "{id}: {:?} {:?}", theirs.assembly.problems, theirs.printed);
            let own = assemble_own(Cpu::M6502, &source, text).assembly;
            assert!(own.ok(), "{id}: {:?}", own.problems);
            let chunk = &theirs.assembly.chunks[0];
            let mine = own.image(chunk.address, chunk.address + chunk.bytes.len() as i64);
            let differ = (0..mine.len()).find(|i| mine[*i] != chunk.bytes[*i]);
            assert!(differ.is_none(), "{id}: differ at ${:04X} (line {:?}): ours {:02X?} theirs {:02X?}", chunk.address + differ.unwrap() as i64, theirs.assembly.line_at(chunk.address + differ.unwrap() as i64), &mine[differ.unwrap()..], &chunk.bytes[differ.unwrap()..]);
            // KickAssembler gives a macro's bytes to the lines inside it, Limoniter to the line using it; and a
            // line under .pseudopc is at where it's put for KickAssembler, where it runs for Limoniter.
            let (mut same, mut other) = (0, 0);
            for placed in theirs.assembly.lines.iter().filter(|p| p.file == 0) {
                if own.lines.iter().any(|o| o.file == 0 && o.line == placed.line && o.address == placed.address && o.bytes == placed.bytes) {
                    same += 1;
                } else {
                    let moved = own.lines.iter().any(|o| o.file == 0 && o.line == placed.line && o.bytes == placed.bytes);
                    let in_macro = own.lines.iter().any(|o| o.address <= placed.address && placed.address < o.address + o.bytes.len() as i64);
                    assert!(moved || in_macro, "{id}: line {} at ${:04X} {:02X?}", placed.line + 1, placed.address, placed.bytes);
                    other += 1;
                }
            }
            println!("{id}: {} bytes the same from ${:04X}; {same} lines at the same addresses, {other} in macros or moved", chunk.bytes.len(), chunk.address);
            assert!(same > 40 && other * 4 < same, "{same} {other}");
            assert_eq!(theirs.tape.as_ref().map(|t| t.0), Some(crate::running::Format::Prg), "{id}: a program with its own BASIC line");
            std::fs::write(&source, wrong).unwrap();
            let failed = assemble_external(external(id).unwrap(), Path::new(&program), &source).unwrap().assembly;
            let lines: Vec<usize> = failed.problems.iter().map(|p| p.line).collect();
            assert!(lines.contains(&2) && failed.problems.iter().all(|p| p.file == 0), "{id}: {:?}", failed.problems);
        }
        let _ = std::fs::remove_dir_all(&folder);
    }

    /// With LIMONITER_KICKASS or LIMONITER_64TASS and LIMONITER_C64_SOURCES (a folder of their programs, looked
    /// through): of those the real assembler assembles, how many Limoniter's own makes the same bytes of. Prints how
    /// each went.
    #[test]
    #[ignore]
    fn real_c64_programs_assemble_as_their_assemblers_do() {
        use crate::assemblers::{Cpu, assemble_external, assemble_own, external};
        let Ok(folder) = std::env::var("LIMONITER_C64_SOURCES") else { return };
        let (id, program) = match (std::env::var("LIMONITER_KICKASS"), std::env::var("LIMONITER_64TASS")) {
            (Ok(p), _) => ("kickass", p),
            (_, Ok(p)) => ("64tass", p),
            _ => return,
        };
        let mut files = Vec::new();
        let mut folders = vec![PathBuf::from(folder)];
        while let Some(folder) = folders.pop() {
            for entry in std::fs::read_dir(&folder).unwrap().flatten() {
                let path = entry.path();
                if path.is_dir() {
                    folders.push(path);
                } else if path.extension().is_some_and(|e| e.eq_ignore_ascii_case("asm") || e.eq_ignore_ascii_case("s") || e.eq_ignore_ascii_case("tas")) {
                    files.push(path);
                }
            }
        }
        files.sort();
        let (mut same, mut differ, mut failed, mut skipped) = (0, 0, 0, 0);
        for path in files {
            let Ok(text) = std::fs::read_to_string(&path) else { continue };
            let theirs = match assemble_external(external(id).unwrap(), Path::new(&program), &path) {
                Ok(t) if t.assembly.ok() && !t.assembly.chunks.is_empty() => t.assembly,
                _ => {
                    skipped += 1;
                    continue;
                }
            };
            let ours = assemble_own(Cpu::M6502, &path, &text).assembly;
            if !ours.ok() {
                failed += 1;
                let p = &ours.problems[0];
                println!("OURS FAILED {}: line {}: {} [{} problems]", path.display(), p.line + 1, p.message, ours.problems.len());
                continue;
            }
            let chunk = &theirs.chunks[0];
            let mine = ours.image(chunk.address, chunk.address + chunk.bytes.len() as i64);
            if mine == chunk.bytes {
                same += 1;
                println!("SAME {}", path.display());
            } else {
                differ += 1;
                let at = (0..mine.len()).find(|i| mine[*i] != chunk.bytes[*i]).unwrap_or(0);
                println!("DIFFER {} at ${:04X} {:?}", path.display(), chunk.address + at as i64, theirs.line_at(chunk.address + at as i64));
            }
        }
        println!("{id}: same {same}, differ {differ}, ours failed {failed}, theirs didn't {skipped}");
    }

    #[test]
    fn problems_are_read_as_kickassembler_and_64tass_print_them() {
        assert_eq!(problem_line("  (/root/c64/e.asm 3:5) Error: Pseudo command 'ldq' not defined"), Some(Some(("/root/c64/e.asm".into(), 3, "Pseudo command 'ldq' not defined".into()))));
        assert_eq!(problem_line("  (C:\\My Demos\\e.asm 12:9) Error: Unknown symbol 'nowhere'"), Some(Some(("C:\\My Demos\\e.asm".into(), 12, "Unknown symbol 'nowhere'".into()))));
        assert_eq!(problem_line("e2.asm:4:7: error: not defined symbol 'nowhere'"), Some(Some(("e2.asm".into(), 4, "not defined symbol 'nowhere'".into()))));
        assert_eq!(problem_line("C:\\demo\\e2.asm:4:7: error: wrong type"), Some(Some(("C:\\demo\\e2.asm".into(), 4, "wrong type".into()))));
        assert_eq!(problem_line("e2.asm:5:1: warning: label not on left side"), Some(None));
        assert_eq!(problem_line("Assembling file:   e2.asm"), None);
        assert_eq!(crate::assemblers::problem_line("e2.asm:5:1: warning: label not on left side"), None);
        assert_eq!(crate::assemblers::problem_line("e2.asm:4:7: error: not defined symbol 'x'").map(|p| p.1), Some(4));
    }

    #[test]
    fn kickassembler_s_debug_file_gives_the_lines() {
        let debug = "<C64debugger version=\"1.0\">\n   <Sources values=\"INDEX,FILE\">\n      0,KickAss.jar:/include/autoinclude.asm\n      1,/src/k.asm\n   </Sources>\n   <Segment name=\"Default\" dest=\"\" values=\"START,END,FILE_IDX,LINE1,COL1,LINE2,COL2\">\n      <Block name=\"Basic\">\n         $0801,$0802,0,56,2,56,6\n      </Block>\n      <Block name=\"Code\">\n         $0810,$0811,1,6,5,6,7\n         $0812,$0814,1,7,5,7,7\n      </Block>\n   </Segment>\n   <Labels values=\"SEGMENT,ADDRESS,NAME,START,END,FILE_IDX,LINE1,COL1,LINE2,COL2\">\n      Default,$0810,start,1,5,1,5,6\n   </Labels>\n</C64debugger>\n";
        let mut assembly = Assembly { files: vec![PathBuf::from("/src/k.asm")], ..Assembly::default() };
        read_debug_file(debug, &mut assembly, Path::new("/src/k.asm"));
        let lines: Vec<(usize, usize, i64, usize)> = assembly.lines.iter().map(|p| (p.file, p.line, p.address, p.bytes.len())).collect();
        assert_eq!(lines, [(0, 5, 0x0810, 2), (0, 6, 0x0812, 3)]);
        assert_eq!(assembly.symbols.get("start"), Some(&0x0810));
    }

    #[test]
    fn the_64tass_listing_gives_the_lines() {
        let listing = ";Line\t;Offset\t;Hex\t\t;Source\n\n:1\t;******  Processing input file: t.asm\n\n2\t>c000\t01 02 03 04\tlab     .byte 1,2,3,4,5,6\n\t>c004\t05 06\n5\t.c006\ta9 01\t\t  lda #1\n6\t=$d020\t\t\tBorder = $d020\n\n:2\t;******  Processing file: inc.asm\n\n1\t.c008\t60\t\trts\n\n:1\t;******  Return to file: t.asm\n\n9\t.c009\tea\t\tnop\n";
        let mut assembly = Assembly { files: vec![PathBuf::from("/src/t.asm")], ..Assembly::default() };
        read_listing(listing, &mut assembly, Path::new("/src/t.asm"));
        let lines: Vec<(usize, usize, i64, Vec<u8>)> = assembly.lines.iter().map(|p| (p.file, p.line, p.address, p.bytes.clone())).collect();
        assert_eq!(lines, [(0, 1, 0xC000, vec![1, 2, 3, 4, 5, 6]), (0, 4, 0xC006, vec![0xA9, 1]), (1, 0, 0xC008, vec![0x60]), (0, 8, 0xC009, vec![0xEA])]);
        assert_eq!(assembly.files[1], PathBuf::from("/src/inc.asm"));
    }

    fn stored_zip(files: &[(&str, Vec<u8>)]) -> Vec<u8> {
        let (mut out, mut directory) = (Vec::new(), Vec::new());
        for (name, bytes) in files {
            let at = out.len() as u32;
            let sizes = [(bytes.len() as u32).to_le_bytes(), (bytes.len() as u32).to_le_bytes()].concat();
            out.extend(b"PK\x03\x04\x14\0\0\0\0\0\0\0\0\0\0\0\0\0");
            out.extend(&sizes);
            out.extend((name.len() as u16).to_le_bytes());
            out.extend([0, 0]);
            out.extend(name.as_bytes());
            out.extend(bytes);
            directory.extend(b"PK\x01\x02\x14\0\x14\0\0\0\0\0\0\0\0\0\0\0\0\0");
            directory.extend(&sizes);
            directory.extend((name.len() as u16).to_le_bytes());
            directory.extend([0u8; 12]);
            directory.extend(at.to_le_bytes());
            directory.extend(name.as_bytes());
        }
        let start = out.len() as u32;
        out.extend(&directory);
        out.extend(b"PK\x05\x06\0\0\0\0");
        out.extend((files.len() as u16).to_le_bytes());
        out.extend((files.len() as u16).to_le_bytes());
        out.extend((directory.len() as u32).to_le_bytes());
        out.extend(start.to_le_bytes());
        out.extend([0, 0]);
        out
    }

    #[test]
    fn kickassembler_s_zip_gives_its_jar_and_manual_but_not_its_settings() {
        let zip = stored_zip(&[("KickAss.cfg", b"-execute x64sc".to_vec()), ("KickAss.jar", b"PK jar".to_vec()), ("KickAssembler.pdf", b"%PDF".to_vec())]);
        let folder = std::env::temp_dir().join(format!("limoniter-kick-{}", std::process::id()));
        let jar = unpack_kickass(zip, &folder).unwrap();
        assert_eq!(std::fs::read(&jar).unwrap(), b"PK jar");
        assert!(folder.join("KickAssembler.pdf").is_file());
        assert!(!folder.join("KickAss.cfg").exists(), "its settings could start an emulator by themselves");
        assert!(std::fs::read_to_string(folder.join("README.txt")).unwrap().contains("No licence"));
        let _ = std::fs::remove_dir_all(&folder);
    }

    /// KickAssembler's real download, unpacked: LIMONITER_KICKASS_ZIP=<KickAssembler.zip> cargo test -- --ignored
    #[test]
    #[ignore]
    fn kickassembler_s_real_zip_unpacks() {
        let Ok(zip) = std::env::var("LIMONITER_KICKASS_ZIP") else { return };
        let folder = std::env::temp_dir().join(format!("limoniter-kick-real-{}", std::process::id()));
        let jar = unpack_kickass(std::fs::read(zip).unwrap(), &folder).unwrap();
        assert!(std::fs::metadata(&jar).unwrap().len() > 100_000);
        let _ = std::fs::remove_dir_all(&folder);
    }
}
