//! limoniter --assemble SOURCE OUTPUT [NAME=VALUE...]: 68000 source made into an Amiga program from the command
//! line, by Limoniter's own assembler, as F9 makes it (with the NDK's headers and _LVO offsets, if it's set up).
//! NAME=VALUE sets a setting: a line of the source that starts NAME equ is given that value instead, or, if there's
//! no such line, one is put at the start. So a demo can be built in each of its ways, OS_FRIENDLY=1 and so on,
//! from a script or a Makefile.

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
    let usage = crate::i18n::tr("Usage: limoniter --assemble SOURCE OUTPUT [NAME=VALUE...]");
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
    match assemble(&source, &set_values(&text, &values)) {
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

/// The program, or each problem as file:line: message.
pub fn assemble(source: &Path, text: &str) -> Result<Vec<u8>, Vec<String>> {
    let assembled = crate::assemblers::assemble_own_to_run(crate::assemblers::Cpu::M68k, source, text);
    let assembly = &assembled.assembly;
    if !assembly.ok() {
        return Err(assembly.problems.iter().map(|p| format!("{}:{}: {}", assembly.files.get(p.file).map_or_else(|| source.display().to_string(), |f| f.display().to_string()), p.line + 1, p.message)).collect());
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
}
