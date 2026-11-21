//! limoniter --assemble SOURCE OUTPUT [NAME=VALUE...]: 68000 source made into an Amiga program from the command
//! line, by Limoniter's own assembler, as F9 makes it (with the NDK's headers and _LVO offsets, if it's set up).
//! NAME=VALUE sets a setting: a line of the source that starts NAME equ is given that value instead, or, if there's
//! no such line, one is put at the start. So a demo can be built in each of its ways, OS_FRIENDLY=1 and so on,
//! from a script or a Makefile.
//!
//! An OUTPUT ending .tap, .sna or .nex makes Z80 source into a ZX Spectrum or Next program instead, as F9 makes it:
//! the tape (or .nex) the source saves itself, if it does, or else a tape with a BASIC loader for a 48K or 128K
//! (or a .nex for a Next), by the DEVICE it chooses. A .sna is that tape loaded and stopped where its code starts.
//! How To Code for the ZX Spectrum builds and tests its examples this way.

use std::ffi::OsString;
use std::path::{Path, PathBuf};

/// Whether the command line asks for this.
pub fn wanted(args: &[OsString]) -> bool {
    args.first().is_some_and(|a| a == "--assemble")
}

/// The source with its settings given these values.
pub fn set_values(text: &str, values: &[(String, String)]) -> String {
    let mut out: Vec<String> = Vec::new();
    let mut done = vec![false; values.len()];
    for line in text.lines() {
        let word = line.split_whitespace().next().unwrap_or("");
        let is_equ = line.split_whitespace().nth(1).is_some_and(|w| w.eq_ignore_ascii_case("equ") || w == "=");
        match values.iter().position(|(name, _)| is_equ && !line.starts_with([' ', '\t']) && word.trim_end_matches(':').eq_ignore_ascii_case(name)) {
            Some(i) => {
                out.push(format!("{}\tequ\t{}", values[i].0, values[i].1));
                done[i] = true;
            }
            None => out.push(line.to_string()),
        }
    }
    let mut first: Vec<String> = values.iter().zip(&done).filter(|(_, d)| !**d).map(|((n, v), _)| format!("{n}\tequ\t{v}")).collect();
    first.extend(out);
    first.join("\n") + "\n"
}

/// Does what was asked, saying what happened; returns the exit code.
pub fn run(args: &[OsString]) -> i32 {
    #[cfg(windows)]
    {
        // A window program has no console of its own: what it says goes to the one it was started from.
        // SAFETY: a plain call; failing only means there's no console to write to.
        unsafe {
            let _ = windows::Win32::System::Console::AttachConsole(windows::Win32::System::Console::ATTACH_PARENT_PROCESS);
        }
    }
    let usage = crate::i18n::tr("Usage: limoniter --assemble SOURCE OUTPUT [NAME=VALUE...] (OUTPUT ending .tap, .sna or .nex for Z80 source)");
    let (Some(source), Some(output)) = (args.get(1).map(PathBuf::from), args.get(2).map(PathBuf::from)) else {
        eprintln!("{usage}");
        return 2;
    };
    let mut values = Vec::new();
    for arg in &args[3..] {
        let arg = arg.to_string_lossy();
        match arg.split_once('=') {
            Some((name, value)) if !name.is_empty() => values.push((name.to_string(), value.to_string())),
            _ => {
                eprintln!("{}\n{usage}", crate::i18n::tf!("{arg} isn't NAME=VALUE."));
                return 2;
            }
        }
    }
    let source = std::path::absolute(&source).unwrap_or(source);
    let text = match std::fs::read(&source) {
        Ok(bytes) => String::from_utf8_lossy(&bytes).into_owned(),
        Err(e) => {
            eprintln!("{}: {e}", source.display());
            return 1;
        }
    };
    match assemble_for(&source, &set_values(&text, &values), &output) {
        Ok(program) => match std::fs::write(&output, &program) {
            Ok(()) => {
                println!("{}", crate::i18n::tf!("{}: {} bytes", output.display(), program.len()));
                0
            }
            Err(e) => {
                eprintln!("{}: {e}", output.display());
                1
            }
        },
        Err(problems) => {
            for problem in problems {
                eprintln!("{problem}");
            }
            1
        }
    }
}

/// The program the output's ending asks for: a Spectrum's or Next's from Z80 source (.tap, .sna, .nex), else an
/// Amiga's from 68000 source.
pub fn assemble_for(source: &Path, text: &str, output: &Path) -> Result<Vec<u8>, Vec<String>> {
    let ending = output.extension().map(|e| e.to_string_lossy().to_ascii_lowercase()).unwrap_or_default();
    match ending.as_str() {
        "tap" | "sna" | "nex" => assemble_z80(source, text, &ending),
        _ => assemble(source, text),
    }
}

/// Z80 source made into what F9 runs, as a .tap or .nex; or, for a .sna, that tape loaded on the built-in
/// Spectrum and stopped where the code starts.
pub fn assemble_z80(source: &Path, text: &str, ending: &str) -> Result<Vec<u8>, Vec<String>> {
    use crate::running::Format;
    let assembled = crate::assemblers::assemble_own(crate::assemblers::Cpu::Z80, source, text);
    let assembly = &assembled.assembly;
    if !assembly.ok() {
        return Err(problems(source, assembly));
    }
    let stem = source.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
    let run = crate::app::asm_host::runnable_from(crate::assemblers::Cpu::Z80, &assembled, &stem, Default::default()).map_err(|problem| vec![problem])?;
    match (ending, run.format) {
        ("tap", Format::Tap) | ("nex", Format::Nex) | ("sna", Format::Sna) => Ok(run.bytes.to_vec()),
        ("sna", Format::Tap) => {
            let start = assembly.start().or(assembly.lowest()).unwrap_or(0x8000) as u16;
            crate::zx_machine::BuiltIn::snapshot_at(&run, start).map_err(|problem| vec![problem])
        }
        _ => Err(vec![crate::i18n::tf!("{}: this makes {}, not .{ending}.", source.display(), run.name)]),
    }
}

/// Each problem as file:line: message.
fn problems(source: &Path, assembly: &crate::asm::Assembly) -> Vec<String> {
    assembly.problems.iter().map(|p| format!("{}:{}: {}", assembly.files.get(p.file).map_or_else(|| source.display().to_string(), |f| f.display().to_string()), p.line + 1, p.message)).collect()
}

/// The program, or each problem as file:line: message.
pub fn assemble(source: &Path, text: &str) -> Result<Vec<u8>, Vec<String>> {
    let assembled = crate::assemblers::assemble_own_to_run(crate::assemblers::Cpu::M68k, source, text);
    let assembly = &assembled.assembly;
    if !assembly.ok() {
        return Err(problems(source, assembly));
    }
    match assembled.tape {
        Some((_, bytes)) => Ok(bytes),
        None => Err(vec![crate::i18n::tf!("{}: this isn't 68000 source for the Amiga: it has no SECTION, and uses no system calls or custom chips.", source.display())]),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn settings_are_set_in_place_or_added_first() {
        let text = "OS_FRIENDLY     equ     0\n  move.l d0,d1\nOther = 3\n";
        let set = set_values(text, &[("OS_FRIENDLY".into(), "1".into()), ("AUTOEXIT".into(), "500".into()), ("Other".into(), "4".into())]);
        assert_eq!(set, "AUTOEXIT\tequ\t500\nOS_FRIENDLY\tequ\t1\n  move.l d0,d1\nOther\tequ\t4\n");
        assert!(wanted(&["--assemble".into()]));
        assert!(!wanted(&["file.s".into()]));
    }

    #[test]
    fn an_amiga_program_is_made_as_f9_makes_it() {
        let program = assemble(Path::new("t.s"), "        section code,code\n        moveq   #0,d0\n        rts\n").unwrap();
        assert!(program.starts_with(&[0, 0, 3, 0xF3]), "a hunk executable");
        let problems = assemble(Path::new("t.s"), "        section code,code\n        nonsense d0\n").unwrap_err();
        assert!(problems[0].starts_with("t.s:2: "), "{problems:?}");
    }

    #[test]
    fn z80_source_becomes_a_spectrum_tape_snapshot_or_nex() {
        let source = "        org 32768\nstart:  ld a,2\n        out (254),a\n        jr $\n";
        let tape = assemble_for(Path::new("t.asm"), source, Path::new("t.tap")).unwrap();
        assert_eq!(&tape[..4], &[19, 0, 0, 0], "a BASIC loader's header first");
        let sna = assemble_for(Path::new("t.asm"), source, Path::new("t.sna")).unwrap();
        assert_eq!(sna.len(), 49179, "a 48K snapshot");
        let sp = u16::from_le_bytes([sna[23], sna[24]]) as usize;
        assert_eq!(u16::from_le_bytes([sna[27 + sp - 0x4000], sna[28 + sp - 0x4000]]), 32768, "stopped where it starts");
        let nex = assemble_for(Path::new("t.asm"), &format!("        device zxspectrumnext\n{source}"), Path::new("t.nex")).unwrap();
        assert!(nex.starts_with(b"NextV1.2"));
        let problems = assemble_for(Path::new("t.asm"), "        org 32768\n        nonsense\n", Path::new("t.tap")).unwrap_err();
        assert!(problems[0].starts_with("t.asm:2: "), "{problems:?}");
        assert!(assemble_for(Path::new("t.asm"), source, Path::new("t.nex")).is_err(), "a 48K's program isn't a .nex");
    }
}
