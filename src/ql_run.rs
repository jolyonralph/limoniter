//! Making a QL program from 68000 source and starting it in a QL emulator.
//!
//! Source is the QL's when it says so (; machine: ql) or is for the 68008 (MACHINE 68008, or the 68008 chosen in
//! the settings). It becomes an executable: its bytes from address 0, which have to run wherever QDOS loads them
//! (it's assembled a second time 64K further on, and any byte that moved is an absolute address, refused on its
//! line), kept with the XTcc trailer giving its data space (`ql`). F9 writes it beside the source too, as Save does.
//!
//! Each emulator gets it where it reads programs from, and is told to start it:
//!
//! - sQLux (its manual, docs/sqlux.md, 5.1, and src/emulator_options.c): `--device WIN8,folder,qdos-like` makes the
//!   run's folder the drive win8_ (a drive the person's own sqlux.ini is unlikely to use, since devices on the
//!   command line are set before the ini's, which would win), and `-b` gives the line SuperBASIC runs as it starts,
//!   in place of a BOOT file: EXEC win8_hello_exe. sQLux reads the program's type and data space from the XTcc
//!   trailer (uxfile.c, `FillXHXtcc`). A cartridge's files are copied into the folder, programs with Q-emuLator's
//!   header as sQLux writes them, and its first program started.
//! - MAME's QL (src/mame/sinclair/ql.cpp): `ql -microtape1 file.mdv` puts a cartridge in the first microdrive,
//!   which MAME reads from version 0.289 (microdrv.cpp, "Sinclair QL Microdrive working", July 2026). The QL waits
//!   at its start for F1 (monitor) or F2 (TV), so an `-autoboot_script` presses F1 through MAME's natural keyboard
//!   (`natkeyboard:post_coded("{F1}")`, luaengine_input.cpp); QDOS then runs the cartridge's BOOT, which EXECs the
//!   program. MAME needs the QL's ROMs (ql.zip) in its ROM folder.
//! - Q-emuLator documents no command line: it's given the cartridge, as a file to open, and the cartridge's BOOT
//!   starts the program once it's in MDV1.

use crate::assemblers::{Assembled, Cpu};
use crate::media::ql::Cartridge;
use crate::ql;
use crate::running::{Format, Machine, Runnable};
use std::path::{Path, PathBuf};
use std::sync::Arc;

/// How far on the program is assembled a second time to find any absolute address in it.
const SHIFT: i64 = 0x10000;

/// The drive sQLux is given the run's folder as.
pub const SQLUX_DRIVE: &str = "WIN8";

/// The script that presses F1 in MAME's QL, beside the cartridge.
pub const MAME_SCRIPT: &str = "ql_boot.lua";

/// The name the cartridge made for a run is given.
const CARTRIDGE_NAME: &str = "Limoniter";

/// Whether assembled 68000 source is for the QL: it says so, or it's for the 68008.
pub fn is_ql(assembled: &Assembled) -> bool {
    let said = assembled.source.as_deref().is_some_and(|text| ql::says_ql(text.lines()));
    said || assembled.assembly.settings.iter().any(|(_, key, value)| *key == crate::asm::m68k::SETTING && *value == 68008)
}

/// The executable the source makes: its bytes, which have to start at 0 and run wherever they're put, with the
/// data space the source asks for.
pub fn program(assembled: &Assembled, stem: &str) -> Result<Runnable, String> {
    let assembly = &assembled.assembly;
    let (Some(low), Some(high)) = (assembly.lowest(), assembly.highest()) else {
        return Err(crate::i18n::tr("Nothing was assembled.").into());
    };
    if low != 0 || assembly.absolute {
        return Err(crate::i18n::tf!("A QL program is loaded wherever QDOS finds room for it, and starts at its first byte, but this is put at ${low:04X}: leave out the ORG."));
    }
    let code = assembly.image(0, high);
    let path = assembly.files.first().cloned().unwrap_or_default();
    let source = assembled.source.clone().or_else(|| std::fs::read_to_string(&path).ok()).unwrap_or_default();
    if !source.is_empty() {
        let moved = crate::assemblers::assemble_own_at(Cpu::M68k, &path, &source, SHIFT);
        if moved.ok() && let Some(at) = first_moved(&code, &moved.image(SHIFT, SHIFT + code.len() as i64)) {
            let line = assembly.lines.iter().find(|p| p.file == 0 && p.address <= at as i64 && (at as i64) < p.address + p.bytes.len() as i64).map_or(0, |p| p.line + 1);
            return Err(crate::i18n::tf!("Line {line} uses an absolute address (at ${at:04X} in the program), and a QL program has to run wherever QDOS loads it: use the PC-relative form, as lea name(pc),a0 does."));
        }
    }
    let data_space = ql::data_space_said(source.lines()).unwrap_or(ql::DATA_SPACE);
    Ok(Runnable { machine: Machine::Ql, format: Format::QdosExe, name: ql::program_name(stem), bytes: Arc::new(ql::with_xtcc(&code, data_space)) })
}

/// Where the code assembled further on first differs from the code as it is.
fn first_moved(here: &[u8], there: &[u8]) -> Option<usize> {
    here.iter().zip(there).position(|(a, b)| a != b)
}

/// A cartridge holding a program and a BOOT file that EXECs it, as the QL boots.
pub fn cartridge_with(program_name: &str, program: &[u8]) -> Result<Vec<u8>, String> {
    let kept = ql::kept(program).ok_or(crate::i18n::tr("This isn't a QL program kept with its header."))?;
    let mut cartridge = Cartridge::open(Cartridge::blank(CARTRIDGE_NAME, [0x4C, 0x49]))?;
    let name = ql::qdos_name(program_name);
    cartridge.add(&name, kept.kind, kept.data_space, kept.code)?;
    cartridge.add("BOOT", 0, 0, ql::boot_program("mdv1_", &name).as_bytes())?;
    Ok(cartridge.bytes)
}

/// Gets a QL program (or cartridge) ready for an emulator in the run's folder: the file it's to be given, and what
/// kind it is.
pub fn prepare(emulator: &str, folder: &Path, file: &Path, run: &Runnable) -> Result<(PathBuf, Format), String> {
    let write = |path: &Path, bytes: &[u8]| std::fs::write(path, bytes).map_err(|e| crate::i18n::tf!("Couldn't make the copy to run.\n\n{e}"));
    match emulator {
        "sqlux" if run.format == Format::Mdv => {
            let cartridge = Cartridge::open(run.bytes.to_vec())?;
            let files = cartridge.files();
            for each in &files {
                let bytes = if each.header.kind == ql::EXECUTABLE { ql::with_qemulator_header(&each.bytes, each.header.kind, each.header.data_space) } else { each.bytes.clone() };
                write(&folder.join(ql::qdos_name(&each.header.name)), &bytes)?;
            }
            let start = files.iter().find(|f| f.header.kind == ql::EXECUTABLE).or_else(|| files.iter().find(|f| f.header.name.eq_ignore_ascii_case("boot")));
            let start = start.ok_or(crate::i18n::tr("There's no program on the cartridge to start, and no BOOT file."))?;
            Ok((folder.join(ql::qdos_name(&start.header.name)), Format::QdosExe))
        }
        "sqlux" => Ok((file.to_path_buf(), Format::QdosExe)),
        _ => {
            let (path, bytes) = match run.format {
                Format::Mdv => (file.to_path_buf(), run.bytes.to_vec()),
                _ => (folder.join(format!("{}.mdv", file.file_stem().map_or_else(|| "program".into(), |s| s.to_string_lossy().into_owned()))), cartridge_with(&run.name, &run.bytes)?),
            };
            write(&path, &bytes)?;
            if emulator == "mame" {
                write(&folder.join(MAME_SCRIPT), mame_script().as_bytes())?;
            }
            Ok((path, Format::Mdv))
        }
    }
}

/// The Lua MAME runs once the QL has started: F1 at its first screen, for the monitor, after which QDOS boots
/// from the cartridge in MDV1.
pub fn mame_script() -> String {
    "-- Made by Limoniter for a run: chooses the monitor at the QL's first screen, so it boots mdv1_BOOT.\nmanager.machine.natkeyboard:post_coded(\"{F1}\")\n".to_string()
}

/// sQLux's arguments: the program's folder as a drive, and the line that starts it (LRUN for a BOOT file).
pub fn sqlux_arguments(file: &Path) -> Vec<String> {
    let folder = file.parent().map(|p| p.display().to_string()).unwrap_or_default();
    let name = file.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let drive = SQLUX_DRIVE.to_ascii_lowercase();
    let line = if name.eq_ignore_ascii_case("boot") { format!("LRUN {drive}_{name}") } else { format!("EXEC {drive}_{name}") };
    let separator = if folder.ends_with(['/', '\\']) { "" } else { std::path::MAIN_SEPARATOR_STR };
    vec!["--device".into(), format!("{SQLUX_DRIVE},{folder}{separator},qdos-like"), "-b".into(), line]
}

/// MAME's arguments for a QL cartridge, its F1 script beside it.
pub fn mame_arguments(file: &Path) -> Vec<String> {
    let script = file.with_file_name(MAME_SCRIPT);
    ["ql", "-microtape1", &file.display().to_string(), "-skip_gameinfo", "-autoboot_delay", "5", "-autoboot_script", &script.display().to_string()].map(str::to_string).to_vec()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::assemblers::assemble_own;

    fn assembled(source: &str) -> Assembled {
        let mut assembled = assemble_own(Cpu::M68k, Path::new("hello.s"), source);
        assembled.source = Some(source.to_string());
        assembled
    }

    /// The starter assembles to an executable: a job header with $4AFB at 6 and its name, the XTcc trailer with the
    /// 4K of data space its source asks for, and only PC-relative addresses.
    #[test]
    fn the_starter_becomes_a_ql_program() {
        let made = assembled(ql::STARTER);
        assert!(made.assembly.ok(), "{:?}", made.assembly.problems);
        assert!(is_ql(&made));
        let run = program(&made, "hello").unwrap();
        assert_eq!((run.machine, run.format, run.name.as_str()), (Machine::Ql, Format::QdosExe, "hello_exe"));
        let kept = ql::kept(&run.bytes).unwrap();
        assert_eq!((kept.kind, kept.data_space, kept.keeping), (ql::EXECUTABLE, 4096, ql::Keeping::Xtcc));
        assert_eq!(&kept.code[..2], &[0x60, 0x0E], "bra.s to the code after the header");
        assert_eq!(&kept.code[6..8], &[0x4A, 0xFB]);
        assert_eq!(ql::job_name(kept.code).as_deref(), Some("Hello"));
        // The traps it calls: IO.OPEN (trap #2), IO.SSTRG and IO.FBYTE (trap #3), MT.FRJOB (trap #1).
        let code = kept.code;
        for trap in [[0x4E, 0x42], [0x4E, 0x43], [0x4E, 0x41]] {
            assert!(code.windows(2).any(|w| w == trap), "{trap:02X?}");
        }
        assert!(code.windows(2).any(|w| w == [0x70, 0x07]), "moveq #io.sstrg,d0");
        assert!(code.windows(2).any(|w| w == [0x70, 0x05]), "moveq #mt.frjob,d0");
        assert!(code.windows(16).any(|w| w == b"con_400x60a56x80"));
        // F9 runs it from a file, as the emulators read it.
        let again = Runnable::from_file("hello_exe", run.bytes.to_vec()).unwrap();
        assert_eq!((again.machine, again.format), (Machine::Ql, Format::QdosExe));
    }

    #[test]
    fn absolute_addresses_and_orgs_are_refused() {
        let absolute = assembled("; machine: ql\nstart   lea message,a1\n        rts\nmessage dc.b 'hi'\n");
        assert!(absolute.assembly.ok(), "{:?}", absolute.assembly.problems);
        let problem = program(&absolute, "t").unwrap_err();
        assert!(problem.starts_with("Line 2 uses an absolute address"), "{problem}");
        let org = assembled("; machine: ql\n        org $30000\n        rts\n");
        assert!(program(&org, "t").unwrap_err().contains("leave out the ORG"));
        // 68008 source that doesn't say ; machine: ql is the QL's too, and 68000 source isn't.
        assert!(is_ql(&assembled("        machine 68008\n        rts\n")));
        assert!(!is_ql(&assembled("        rts\n")));
    }

    /// A program goes on a cartridge with a BOOT file that EXECs it, and each emulator is given what it reads.
    #[test]
    fn each_emulator_is_given_the_program_where_it_reads_it() {
        let run = program(&assembled(ql::STARTER), "hello").unwrap();
        let folder = std::env::temp_dir().join(format!("limoniter-ql-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&folder);
        std::fs::create_dir_all(&folder).unwrap();
        let file = folder.join(&run.name);
        std::fs::write(&file, run.bytes.as_slice()).unwrap();
        // sQLux: the folder as WIN8, and the program EXECed from it.
        let (given, format) = prepare("sqlux", &folder, &file, &run).unwrap();
        assert_eq!((given.as_path(), format), (file.as_path(), Format::QdosExe));
        let args = sqlux_arguments(&given);
        assert_eq!(args[0], "--device");
        assert_eq!(args[1], format!("WIN8,{}{},qdos-like", folder.display(), std::path::MAIN_SEPARATOR));
        assert_eq!(args[2..], ["-b", "EXEC win8_hello_exe"]);
        // MAME and Q-emuLator: a cartridge with the program and a BOOT that runs it; MAME's F1 script beside it.
        let (cartridge, format) = prepare("mame", &folder, &file, &run).unwrap();
        assert_eq!((cartridge.file_name().unwrap().to_str().unwrap(), format), ("hello_exe.mdv", Format::Mdv));
        let read = Cartridge::open(std::fs::read(&cartridge).unwrap()).unwrap();
        assert_eq!(read.find("BOOT").unwrap().bytes, b"10 EXEC mdv1_hello_exe\n");
        let on = read.find("hello_exe").unwrap();
        assert_eq!((on.header.kind, on.header.data_space), (1, 4096));
        assert_eq!(on.bytes, ql::kept(&run.bytes).unwrap().code);
        assert!(std::fs::read_to_string(folder.join(MAME_SCRIPT)).unwrap().contains("post_coded(\"{F1}\")"));
        let args = mame_arguments(&cartridge);
        assert_eq!(args[..3], ["ql".to_string(), "-microtape1".into(), cartridge.display().to_string()]);
        assert_eq!(args[7], folder.join(MAME_SCRIPT).display().to_string());
        // A cartridge run in sQLux has its files copied out, and its program started.
        let mdv = Runnable { machine: Machine::Ql, format: Format::Mdv, name: "hello.mdv".into(), bytes: Arc::new(std::fs::read(&cartridge).unwrap()) };
        let out = folder.join("out");
        std::fs::create_dir_all(&out).unwrap();
        let (given, _) = prepare("sqlux", &out, &out.join("hello.mdv"), &mdv).unwrap();
        assert_eq!(given, out.join("hello_exe"));
        let copied = std::fs::read(&given).unwrap();
        assert_eq!(ql::kept(&copied).map(|k| (k.keeping, k.data_space)), Some((ql::Keeping::Qemulator, 4096)));
        assert_eq!(std::fs::read(out.join("BOOT")).unwrap(), b"10 EXEC mdv1_hello_exe\n");
        let _ = std::fs::remove_dir_all(&folder);
    }
}
