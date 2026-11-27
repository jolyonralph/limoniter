//! Running a CP/M program in RunCPM, which is CP/M 2.2 run as an ordinary program on this computer.
//!
//! Z80 source is for CP/M when it says so (; machine: cpm) or is put at 100h, where CP/M loads a .COM program (a
//! Spectrum or any other Z80 machine has ROM there). F9 builds the .COM, then makes a folder for the run:
//!
//! - RunCPM keeps CP/M's drives as folders in the one it's started in: `A/0/` is drive A, user area 0 (RunCPM's
//!   README, "Folders and files"), so the program goes in `A/0/` as NAME.COM, 8.3 and in capitals.
//! - The CCP (the command processor, a file named like `CCP-DR.60K` that comes with RunCPM) is looked for in the
//!   folder RunCPM is started in, so each is copied from RunCPM's own folder, with the files in its `A/0/`.
//! - An `AUTOEXEC.TXT` beside the drives, one line with a CP/M command, runs the program at once: RunCPM puts it
//!   in the command line on its first start, with its own CCP and with the CCP files alike (RunCPM's README, and
//!   `AUTOEXEC` in its globals.h and ccp.h, read relative to the folder it's started in).
//!
//! RunCPM is a program for a terminal, so it's started in Limoniter's Terminal panel, in the folder made.
//! MAME's CP/M machines would need a disk with CP/M itself on it, which can't be made here, so RunCPM is the one.

use crate::asm::Assembly;
use crate::running::{Format, Machine, Runnable};
use std::path::{Path, PathBuf};
use std::sync::Arc;

/// The most a CP/M program can be: it starts at 100h and has to leave the BDOS and CCP, at the top of memory,
/// alone, which a 60K system (RunCPM's) puts above E000h.
pub const MOST: usize = 0xE000 - 0x100;

/// What a `; machine:` line near the top of Z80 source says, if it's CP/M.
pub fn says_cpm<'a>(lines: impl IntoIterator<Item = &'a str>) -> bool {
    lines.into_iter().take(50).any(|line| {
        let line = line.trim_start();
        let Some(comment) = line.strip_prefix(';').or_else(|| line.strip_prefix("//")) else {
            return false;
        };
        let comment = comment.trim_start_matches([';', '/', ' ', '\t']).to_ascii_lowercase();
        let Some(value) = comment.strip_prefix("machine").map(str::trim_start).and_then(|c| c.strip_prefix(':')) else {
            return false;
        };
        matches!(value.trim().replace(' ', "").as_str(), "cpm" | "cp/m" | "cpm80" | "cp/m80" | "cpm2.2" | "cp/m2.2" | "runcpm")
    })
}

/// A name as CP/M has them: up to eight capital letters and digits, starting with a letter.
pub fn cpm_name(stem: &str) -> String {
    let letters: String = stem.chars().filter(char::is_ascii_alphanumeric).collect::<String>().to_ascii_uppercase();
    if letters.is_empty() {
        return "PROGRAM".into();
    }
    let letters = if letters.starts_with(|c: char| c.is_ascii_alphabetic()) { letters } else { format!("P{letters}") };
    letters.chars().take(8).collect()
}

/// The .COM program Z80 source makes: its bytes from 100h.
pub fn program(assembly: &Assembly, stem: &str) -> Result<Runnable, String> {
    let (Some(low), Some(high)) = (assembly.lowest(), assembly.highest()) else {
        return Err(crate::i18n::tr("Nothing was assembled.").into());
    };
    if low != 0x100 {
        return Err(crate::i18n::tf!("A CP/M program is loaded at 100h, but this starts at {low:X}h: put ORG 100H at its top."));
    }
    let bytes = assembly.image(low, high);
    if bytes.len() > MOST {
        return Err(crate::i18n::tf!("A CP/M program has room for {MOST} bytes, and this is {}.", bytes.len()));
    }
    Ok(Runnable { machine: Machine::Cpm, format: Format::CpmCom, name: format!("{}.com", cpm_name(stem).to_ascii_lowercase()), bytes: Arc::new(bytes) })
}

/// The `AUTOEXEC.TXT` that makes RunCPM run one command as it starts: the command alone, at most 125 characters.
pub fn autoexec(command: &str) -> String {
    command.to_ascii_uppercase().chars().take(125).collect()
}

/// Copies the files directly in a folder, whose names pass the test, into another.
fn copy_files(from: &Path, to: &Path, wanted: impl Fn(&str) -> bool) -> std::io::Result<()> {
    let Ok(entries) = std::fs::read_dir(from) else {
        return Ok(());
    };
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        if entry.path().is_file() && wanted(&name) {
            std::fs::copy(entry.path(), to.join(&name))?;
        }
    }
    Ok(())
}

/// Makes the folder RunCPM is started in: its CCP and the files in its own `A/0/` (the person's utilities),
/// then the program, and the `AUTOEXEC.TXT` that runs it. `home` is the folder RunCPM was found in. Gives back the
/// name the program is run by.
pub fn stage(folder: &Path, home: Option<&Path>, run: &Runnable) -> Result<String, String> {
    let made = |e: std::io::Error| crate::i18n::tf!("Couldn't make the copy to run.\n\n{e}");
    let drive = folder.join("A").join("0");
    std::fs::create_dir_all(&drive).map_err(made)?;
    if let Some(home) = home {
        // The CCP files are named for the CCP and the memory it's built for (CCP-DR.60K, CCP-CCPZ.64K).
        copy_files(home, folder, |name| name.to_ascii_uppercase().starts_with("CCP-")).map_err(made)?;
        copy_files(&home.join("A").join("0"), &drive, |name| !name.starts_with("$$$")).map_err(made)?;
    }
    let name = cpm_name(run.name.split('.').next().unwrap_or("program"));
    std::fs::write(drive.join(format!("{name}.COM")), run.bytes.as_slice()).map_err(made)?;
    std::fs::write(folder.join("AUTOEXEC.TXT"), autoexec(&name)).map_err(made)?;
    Ok(name)
}

/// The folder RunCPM's program is in, where its CCP and `A/` are.
pub fn home_of(program: &Path) -> Option<PathBuf> {
    program.parent().map(Path::to_path_buf)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn source_says_it_is_cpm() {
        assert!(says_cpm(["; machine: cpm", "\torg 100h"]));
        assert!(says_cpm(["// Machine: CP/M"]));
        assert!(says_cpm([";; machine : CP/M 2.2"]));
        assert!(!says_cpm(["; machine: zx81"]));
        assert!(!says_cpm(["\tld a,1 ; machine: cpm"]), "only a comment on its own line");
    }

    #[test]
    fn names_are_cpms() {
        assert_eq!(cpm_name("hello"), "HELLO");
        assert_eq!(cpm_name("my-game_v2.final"), "MYGAMEV2");
        assert_eq!(cpm_name("3d"), "P3D");
        assert_eq!(cpm_name("--"), "PROGRAM");
    }

    #[test]
    fn a_program_at_100h_is_a_com_file() {
        let source = "\torg 100h\n\tld c,9\n\tld de,msg\n\tcall 5\n\tret\nmsg:\tdb 'Hi$'\n";
        let assembled = crate::assemblers::assemble_own(crate::assemblers::Cpu::Z80, Path::new("hello.asm"), source);
        let run = program(&assembled.assembly, "Hello").unwrap();
        assert_eq!((run.machine, run.format, run.name.as_str()), (Machine::Cpm, Format::CpmCom, "hello.com"));
        assert_eq!(&run.bytes[..3], &[0x0E, 0x09, 0x11], "ld c,9 then ld de,msg");
        assert_eq!(run.bytes.len(), 2 + 3 + 3 + 1 + 3);
        // Not at 100h, it isn't one.
        let elsewhere = crate::assemblers::assemble_own(crate::assemblers::Cpu::Z80, Path::new("a.asm"), "\torg 8000h\n\tret\n");
        assert!(program(&elsewhere.assembly, "a").unwrap_err().contains("100h"));
    }

    #[test]
    fn the_autoexec_runs_one_command_in_capitals() {
        assert_eq!(autoexec("hello"), "HELLO");
        assert_eq!(autoexec(&"x".repeat(200)).len(), 125, "RunCPM reads at most 125 characters");
    }

    #[test]
    fn the_folder_has_the_drive_the_ccp_and_the_program() {
        let base = std::env::temp_dir().join(format!("limoniter-cpm-run-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        let (home, run_folder) = (base.join("RunCPM"), base.join("run"));
        std::fs::create_dir_all(home.join("A").join("0")).unwrap();
        std::fs::write(home.join("CCP-DR.60K"), b"ccp").unwrap();
        std::fs::write(home.join("README.md"), b"not a ccp").unwrap();
        std::fs::write(home.join("A").join("0").join("STAT.COM"), b"stat").unwrap();
        std::fs::write(home.join("A").join("0").join("$$$.SUB"), b"old").unwrap();
        let run = Runnable { machine: Machine::Cpm, format: Format::CpmCom, name: "hello.com".into(), bytes: Arc::new(vec![0xC9]) };
        assert_eq!(stage(&run_folder, Some(&home), &run).unwrap(), "HELLO");
        assert_eq!(std::fs::read(run_folder.join("CCP-DR.60K")).unwrap(), b"ccp");
        assert!(!run_folder.join("README.md").exists());
        let drive = run_folder.join("A").join("0");
        assert_eq!(std::fs::read(drive.join("STAT.COM")).unwrap(), b"stat");
        assert_eq!(std::fs::read(drive.join("HELLO.COM")).unwrap(), [0xC9]);
        assert_eq!(std::fs::read_to_string(run_folder.join("AUTOEXEC.TXT")).unwrap(), "HELLO");
        assert!(!drive.join("$$$.SUB").exists(), "an old submit file isn't carried over, or it would run as well");
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn dos_and_cpm_com_files_are_told_apart() {
        // JP 0107h, then data: CP/M's. A DOS program doesn't start with a RET, or a jump from a COM's own bytes.
        let cpm = vec![0xC3, 0x07, 0x01, 0x48, 0x69, 0x24, 0, 0xC9];
        assert!(crate::running::looks_like_cpm(&cpm));
        let run = Runnable::from_file("hello.com", cpm).unwrap();
        assert_eq!((run.machine, run.format), (Machine::Cpm, Format::CpmCom));
        let dos = Runnable::from_file("hello.com", vec![0xB4, 0x09, 0xBA, 0x08, 0x01, 0xCD, 0x21, 0xC3]).unwrap();
        assert_eq!((dos.machine, dos.format), (Machine::Pc, Format::Com));
        // A jump out of the program isn't a program's own jump.
        assert!(!crate::running::looks_like_cpm(&[0xC3, 0x00, 0x00, 0, 0]));
    }
}
