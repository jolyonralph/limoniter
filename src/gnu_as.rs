//! GNU as for the Amiga's 68000 (m68k-amigaos-as, from Bebbo's amiga-gcc), as one of the assemblers 68000 source
//! can be built with. It reads its own way of writing 68000 source (see the binutils manual's M68K-Dependent
//! chapter): MIT syntax as well as Motorola's operands, | comments, ; between statements, and .directives, so it
//! only ever builds source in that dialect, and the other assemblers never do.
//!
//! It makes an object file, which its linker beside it (m68k-amigaos-ld) makes into an Amiga program; a listing
//! (-al) says which source line made which bytes, for the debugger. Its problems are printed as
//! `prog.s:12: Error: what`, under a line saying `prog.s: Assembler messages:`.

use crate::asm::{self, Assembly, Problem};
use crate::assemblers::{Assembled, file_index, run};
use std::path::{Path, PathBuf};

/// Where Bebbo's amiga-gcc puts GNU as unless told otherwise.
pub fn usual_place() -> Option<PathBuf> {
    let name = if cfg!(windows) { "m68k-amigaos-as.exe" } else { "m68k-amigaos-as" };
    ["/opt/amiga/bin", "C:\\amiga-gcc\\bin", "C:\\amiga\\bin"].iter().map(|folder| Path::new(folder).join(name)).find(|path| path.is_file())
}

/// A problem as GNU as prints it: the file, the line, what's wrong, and whether it's only a warning.
/// "prog.s:12: Error: unknown operator", "prog.s:3: Warning: ...", "prog.s:40: Fatal error: can't open x".
pub fn problem(line: &str) -> Option<(String, usize, String, bool)> {
    let text = line.trim_end();
    for (kind, warning) in [(": Error: ", false), (": Fatal error: ", false), (": Warning: ", true)] {
        let Some((place, message)) = text.split_once(kind) else { continue };
        // The place is the file and its line; a file may start with a drive letter (C:\src\prog.s:12).
        let (file, number) = place.rsplit_once(':')?;
        let number: usize = number.trim().parse().ok()?;
        if file.is_empty() {
            return None;
        }
        return Some((file.to_string(), number, message.trim().to_string(), warning));
    }
    None
}

/// Reads GNU as's listing (-al) into the assembly's lines: each one's address in its section and its bytes.
/// A listing line is "  12 0004 4EAE FF3A \tjsr _LVOAllocMem(a6)", its number first, then where it is, its bytes
/// in words and then the source after a tab; bytes that don't fit go on lines of their own below, with the same
/// number. Lines from INCLUDEd files are numbered in their own files, so only those whose text is the source's
/// own line are taken as the source's.
pub fn read_listing(listing: &str, source: &[&str], assembly: &mut Assembly) {
    let hex = |s: &str| s.len().is_multiple_of(2) && !s.is_empty() && s.chars().all(|c| c.is_ascii_hexdigit());
    let bytes_of = |words: &[&str]| -> Vec<u8> { words.iter().flat_map(|w| (0..w.len()).step_by(2).filter_map(|i| u8::from_str_radix(&w[i..i + 2], 16).ok())).collect() };
    // The line being read, while its bytes may go on below it.
    let mut last: Option<usize> = None;
    for line in listing.lines() {
        let (head, text) = match line.split_once('\t') {
            Some((head, text)) => (head, Some(text)),
            None => (line, None),
        };
        let words: Vec<&str> = head.split_whitespace().collect();
        let Some(number) = words.first().and_then(|n| n.parse::<usize>().ok()) else {
            last = None;
            continue;
        };
        match text {
            Some(text) => {
                last = None;
                let (Some(place), rest) = (words.get(1), &words[2.min(words.len())..]) else { continue };
                let Ok(address) = i64::from_str_radix(place, 16) else { continue };
                if rest.is_empty() || !rest.iter().all(|w| hex(w)) || !hex(place) {
                    continue;
                }
                let own = number >= 1 && source.get(number - 1).is_some_and(|s| s.trim() == text.trim());
                if own {
                    assembly.lines.push(asm::Placed { file: 0, line: number - 1, address, bytes: bytes_of(rest) });
                    last = Some(assembly.lines.len() - 1);
                }
            }
            // More of the bytes of the line above.
            None => {
                let rest = &words[1..];
                if let Some(index) = last
                    && !rest.is_empty()
                    && rest.iter().all(|w| hex(w))
                    && assembly.lines[index].line + 1 == number
                {
                    let more = bytes_of(rest);
                    assembly.lines[index].bytes.extend(more);
                }
            }
        }
    }
}

/// GNU as's linker, beside it and named as it is: m68k-amigaos-ld for m68k-amigaos-as.
pub fn linker_beside(assembler: &Path) -> Option<PathBuf> {
    let name = assembler.file_name()?.to_string_lossy().into_owned();
    let at = name.rfind("as")?;
    let linker = assembler.with_file_name(format!("{}ld{}", &name[..at], &name[at + 2..]));
    linker.is_file().then_some(linker)
}

/// Assembles a saved file with GNU as, and links it into an Amiga program with the linker beside it.
pub fn assemble(program: &Path, source: &Path, work: &Path) -> Result<Assembled, String> {
    let object = work.join("out.o");
    let listing = work.join("out.lst");
    let folder = source.parent().unwrap_or(Path::new("."));
    let mut args = vec!["-o".to_string(), object.display().to_string(), format!("-aln={}", listing.display()), format!("-I{}", folder.display())];
    args.push(source.display().to_string());
    let (ok, mut printed) = run(program, &args, folder)?;
    let mut assembly = Assembly { files: vec![source.to_path_buf()], ..Assembly::default() };
    for line in &printed {
        if let Some((file, number, message, warning)) = problem(line) {
            let file = file_index(&mut assembly, &file, source);
            let place = Problem { file, line: number.saturating_sub(1), message };
            if warning { assembly.warnings.push(place) } else { assembly.problems.push(place) }
        }
    }
    if !ok && assembly.problems.is_empty() {
        assembly.problems.push(Problem { file: 0, line: 0, message: printed.join(" ") });
    }
    if let Ok(text) = std::fs::read_to_string(&listing) {
        let own = std::fs::read_to_string(source).unwrap_or_default();
        let lines: Vec<&str> = own.lines().collect();
        read_listing(&text, &lines, &mut assembly);
    }
    if !ok {
        return Ok(Assembled { assembly, source: None, tape: None, printed });
    }
    let Some(linker) = linker_beside(program) else {
        return Err(crate::i18n::tf!("GNU as assembled it, but its linker (m68k-amigaos-ld) wasn't found beside {}, so no program could be made to run.", program.display()));
    };
    let made = work.join("out");
    let (linked, said) = run(&linker, &["-o".to_string(), made.display().to_string(), object.display().to_string()], folder)?;
    printed.extend(said.iter().cloned());
    if !linked {
        // The linker's complaints (an undefined symbol, mostly) belong to no line of the source.
        assembly.problems.push(Problem { file: 0, line: 0, message: crate::i18n::tf!("The linker couldn't make the program: {}", said.join(" ")) });
        return Ok(Assembled { assembly, source: None, tape: None, printed });
    }
    let tape = std::fs::read(&made).ok().map(|bytes| (crate::running::Format::Hunk, bytes));
    Ok(Assembled { assembly, source: None, tape, printed })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn problems_are_read_as_gnu_as_prints_them() {
        assert_eq!(problem("demo.s:12: Error: unknown operator -- statement `frob d0' ignored"), Some(("demo.s".into(), 12, "unknown operator -- statement `frob d0' ignored".into(), false)));
        assert_eq!(problem("C:\\src\\demo.s:3: Warning: value 0x100 truncated to 0x0"), Some(("C:\\src\\demo.s".into(), 3, "value 0x100 truncated to 0x0".into(), true)));
        assert_eq!(problem("/w/demo.s:40: Fatal error: can't open nothing.i for reading: No such file or directory").map(|p| (p.1, p.3)), Some((40, false)));
        assert_eq!(problem("demo.s: Assembler messages:"), None);
        assert_eq!(problem("prog.asm(5): error: Label not found"), None);
    }

    #[test]
    fn the_listing_gives_each_source_line_its_address_and_bytes() {
        let source = ["\t.text", "start:\tmoveq #0,d0", "\tmovel #0x12345678,a0@(8)", "\t.include \"x.i\"", "\trts"];
        // As GNU as lays it out: the included file's own line 1 has a number that's also the source's.
        let listing = "GAS LISTING demo.s \t\t\tpage 1\n\n\n   1              \t.text\n   2 0000 7000     \tstart:\tmoveq #0,d0\n   3 0002 217C 1234 \tmovel #0x12345678,a0@(8)\n   3      5678 0008 \n   4              \t.include \"x.i\"\n   1 0008 4E71     \tnop\n   5 000a 4E75     \trts\n";
        let mut assembly = Assembly::default();
        read_listing(listing, &source, &mut assembly);
        let placed: Vec<(usize, i64, Vec<u8>)> = assembly.lines.iter().map(|p| (p.line, p.address, p.bytes.clone())).collect();
        assert_eq!(placed, [(1, 0, vec![0x70, 0x00]), (2, 2, vec![0x21, 0x7C, 0x12, 0x34, 0x56, 0x78, 0x00, 0x08]), (4, 10, vec![0x4E, 0x75])]);
    }

    #[test]
    fn the_linker_is_found_beside_the_assembler() {
        let dir = std::env::temp_dir().join(format!("limoniter-gnu-as-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let assembler = dir.join("m68k-amigaos-as");
        assert_eq!(linker_beside(&assembler), None);
        std::fs::write(dir.join("m68k-amigaos-ld"), b"").unwrap();
        assert_eq!(linker_beside(&assembler), Some(dir.join("m68k-amigaos-ld")));
        std::fs::write(dir.join("m68k-amigaos-ld.exe"), b"").unwrap();
        assert_eq!(linker_beside(&dir.join("m68k-amigaos-as.exe")), Some(dir.join("m68k-amigaos-ld.exe")));
        std::fs::remove_dir_all(&dir).unwrap();
    }

    /// With LIMONITER_GNU_AS naming m68k-amigaos-as (and its linker beside it): a program is assembled into an Amiga
    /// program whose code is that of the same program written Motorola's way, assembled by Limoniter's own.
    #[test]
    #[ignore]
    fn a_real_gnu_as_makes_the_program_limoniter_s_own_does() {
        let Ok(program) = std::env::var("LIMONITER_GNU_AS") else { return };
        let work = std::env::temp_dir().join(format!("limoniter-gnu-as-real-{}", std::process::id()));
        std::fs::create_dir_all(work.join("out")).unwrap();
        let source = work.join("demo.s");
        std::fs::write(&source, "\t.text\nstart:\tmoveq #0,d0\n\tlea data,a0\n\tmovel a0@+,d1\n\trts\n\t.data\ndata:\t.long 0x12345678\n").unwrap();
        let made = assemble(Path::new(&program), &source, &work.join("out")).unwrap();
        println!("{:?}", made.printed);
        assert!(made.assembly.ok(), "{:?}", made.assembly.problems);
        assert!(made.tape.is_some_and(|(_, bytes)| bytes.starts_with(&[0, 0, 3, 0xF3])), "an Amiga program");
        assert_eq!(made.assembly.lines.len(), 4);
        std::fs::write(&source, "\t.text\nstart:\tmoveq #0,d0\n\tfrob d0\n").unwrap();
        let failed = assemble(Path::new(&program), &source, &work.join("out")).unwrap();
        assert_eq!(failed.assembly.problems.first().map(|p| p.line), Some(2));
        let _ = std::fs::remove_dir_all(&work);
    }
}
