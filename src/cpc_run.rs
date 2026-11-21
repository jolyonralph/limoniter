//! Amstrad CPC programs: Z80 code put on a disk in AMSDOS's data format, as a binary file RUN"name loads and
//! starts; what to type to run a disk's program; and a starter program.
//!
//! A data format disk (Amstrad's CPC 6128 User Instructions, chapter 7) is 40 tracks of nine 512-byte sectors,
//! numbered &C1 to &C9, on one side, with no tracks kept for CP/M: the directory is the first two 1K blocks, 64
//! entries, and CP/M's file system the rest. It's kept as a DSK image (CPCEMU's standard format: "MV - CPCEMU",
//! then each track's header and its sectors). A binary file there starts with AMSDOS's 128-byte header, as the
//! CPC Firmware Guide (section 9) and the CPCWiki's "AMSDOS Header" lay it out: user 0, the name (eight and
//! three, in capitals, padded with spaces) at 1, the file's type at &12 (2 for binary), its length at &13, where it
//! loads at &15, &FF at &17, the length again at &18, where it starts at &1A, the length as 24 bits at &40, and at
//! &43 the sum of bytes 0 to &42, as 16 bits.

use crate::media::write::Addition;

/// A blank data format disk.
pub fn blank_data_disk() -> Vec<u8> {
    const TRACK: usize = 256 + 9 * 512;
    let mut out = b"MV - CPCEMU Disk-File\r\nDisk-Info\r\n".to_vec();
    out.extend(b"Limoniter     ");
    out.resize(0x30, 0);
    out.extend([40, 1]);
    out.extend((TRACK as u16).to_le_bytes());
    out.resize(256, 0);
    for track in 0..40u8 {
        let mut header = b"Track-Info\r\n".to_vec();
        header.resize(0x10, 0);
        // Its number, side 0, 512-byte sectors (size 2), nine of them, the gap and the filler byte.
        header.extend([track, 0, 0, 0, 2, 9, 0x4E, 0xE5]);
        for id in 0xC1..=0xC9u8 {
            header.extend([track, 0, id, 2, 0, 0, 0, 0]);
        }
        header.resize(256, 0);
        out.extend(header);
        // &E5 throughout: an empty directory's entries all say they're deleted.
        out.extend(std::iter::repeat_n(0xE5u8, 9 * 512));
    }
    out
}

/// A name as AMSDOS keeps it: up to eight letters and digits, in capitals; PROGRAM if nothing's left.
pub fn amsdos_name(stem: &str) -> String {
    let name: String = stem.chars().filter(char::is_ascii_alphanumeric).take(8).collect::<String>().to_ascii_uppercase();
    if name.is_empty() { "PROGRAM".to_string() } else { name }
}

/// A binary file with its AMSDOS header: `name` (eight characters at most) and `ending`, loaded at `load` and
/// started at `exec`.
pub fn binary_file(name: &str, ending: &str, load: u16, exec: u16, data: &[u8]) -> Vec<u8> {
    let mut header = [0u8; 128];
    header[1..12].fill(b' ');
    for (i, b) in name.bytes().take(8).enumerate() {
        header[1 + i] = b.to_ascii_uppercase();
    }
    for (i, b) in ending.bytes().take(3).enumerate() {
        header[9 + i] = b.to_ascii_uppercase();
    }
    let length = data.len() as u16;
    header[0x12] = 2;
    header[0x13..0x15].copy_from_slice(&length.to_le_bytes());
    header[0x15..0x17].copy_from_slice(&load.to_le_bytes());
    header[0x17] = 0xFF;
    header[0x18..0x1A].copy_from_slice(&length.to_le_bytes());
    header[0x1A..0x1C].copy_from_slice(&exec.to_le_bytes());
    header[0x40..0x43].copy_from_slice(&(data.len() as u32).to_le_bytes()[..3]);
    let sum: u16 = header[..0x43].iter().fold(0u16, |sum, b| sum.wrapping_add(u16::from(*b)));
    header[0x43..0x45].copy_from_slice(&sum.to_le_bytes());
    let mut file = header.to_vec();
    file.extend_from_slice(data);
    file
}

/// A data format disk holding the code as NAME.BIN, and the disk's file name.
pub fn disk_holding(stem: &str, load: u16, exec: u16, code: &[u8]) -> Result<(String, Vec<u8>), String> {
    if usize::from(load) + code.len() > 0xA67C {
        return Err(crate::i18n::tf!("The program reaches past &A67B, where AMSDOS keeps its workspace, so it can't be loaded from disk. Put it lower (it's at &{load:04X})."));
    }
    let name = amsdos_name(stem);
    let file = binary_file(&name, "BIN", load, exec, code);
    let mut disk = crate::media::write::writer("program.dsk", blank_data_disk())?;
    disk.add(None, &Addition { name: format!("{name}.BIN"), bytes: file, ..Default::default() })?;
    Ok((format!("{}.dsk", name.to_ascii_lowercase()), disk.bytes().to_vec()))
}

/// What to type at BASIC to run a disk's program: RUN" and the first BASIC program on it, or else its first
/// binary, or else its first file of any kind; |CPM for a disk with none (a CP/M disk boots that way).
pub fn run_command(disk: &[u8]) -> String {
    let files: Vec<String> = crate::media::open(std::sync::Arc::new(disk.to_vec()), "disk.dsk")
        .and_then(|volume| volume.list(None))
        .map(|entries| entries.into_iter().filter(|e| !e.is_dir).map(|e| e.name).collect())
        .unwrap_or_default();
    let ending = |file: &str, wanted: &str| file.rsplit_once('.').is_some_and(|(_, e)| e.trim().eq_ignore_ascii_case(wanted));
    let chosen = files.iter().find(|f| ending(f, "BAS")).or_else(|| files.iter().find(|f| ending(f, "BIN"))).or_else(|| files.first());
    match chosen {
        Some(file) => format!("run\"{}", file.rsplit_once('.').map_or(file.as_str(), |(stem, _)| stem).trim()),
        None => "|cpm".to_string(),
    }
}

/// Caprice32's arguments: what to type is given with -a (each one typed with Return after it), then the file,
/// whose kind it tells by its ending. A snapshot just carries on; a tape needs |TAPE on a 6128, then RUN" and a
/// key, and the tape's PLAY pressed (CAP32_TAPEPLAY, Caprice's own key for it).
pub fn caprice_arguments(file: &std::path::Path, format: crate::running::Format) -> Vec<String> {
    use crate::running::Format;
    let named = file.display().to_string();
    let typed: Vec<String> = match format {
        Format::Dsk => vec![run_command(&std::fs::read(file).unwrap_or_default())],
        Format::Cdt => ["|tape", "run\"", "", "CAP32_TAPEPLAY"].map(str::to_string).to_vec(),
        _ => Vec::new(),
    };
    let mut args = Vec::new();
    for line in typed {
        args.extend(["-a".to_string(), line]);
    }
    args.push(named);
    args
}

/// MAME's arguments for a CPC 6128 (which needs its ROMs, cpc6128.zip): a disk in its first drive with what runs
/// it typed once it's started (-autoboot_command is put in a Lua string, so \n in it is Return), or a snapshot.
pub fn mame_arguments(file: &std::path::Path, format: crate::running::Format) -> Vec<String> {
    let named = file.display().to_string();
    if format == crate::running::Format::Sna {
        return ["cpc6128".to_string(), "-dump".into(), named, "-skip_gameinfo".into()].to_vec();
    }
    let typed = run_command(&std::fs::read(file).unwrap_or_default());
    ["cpc6128".to_string(), "-flop1".into(), named, "-skip_gameinfo".into(), "-autoboot_delay".into(), "2".into(), "-autoboot_command".into(), format!("{typed}\\n")].to_vec()
}

/// File, New as, Amstrad CPC (Z80): a line printed through the firmware, in mode 1.
pub const STARTER: &str = r#"; machine: cpc
; An Amstrad CPC program. F9 puts it on a disk (AMSDOS's data format) as a binary file, which RUN" and its name
; loads at &4000 and starts, in an emulator: CPCEC, Caprice32 or MAME. It prints through the firmware's jump
; blocks, which every CPC has at the same addresses, so it runs on the 464, 664 and 6128 alike.

SCR_SET_MODE   equ &BC0E        ; A is the mode: 0, 1 or 2
SCR_SET_BORDER equ &BC38        ; B and C are the border's two colours (the same for no flashing)
TXT_OUTPUT     equ &BB5A        ; prints the character in A
KM_WAIT_CHAR   equ &BB06        ; waits for a key

        org &4000
start:  ld a,1
        call SCR_SET_MODE       ; 40 columns
        ld bc,&0606
        call SCR_SET_BORDER     ; a red border
        ld hl,message
print:  ld a,(hl)
        or a
        jr z,wait
        call TXT_OUTPUT
        inc hl
        jr print
wait:   call KM_WAIT_CHAR
        ret                     ; back to BASIC

message:
        db "Hello from Limoniter",13,10,"Press a key.",0

        end start
"#;

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn the_starter_goes_on_a_disk_as_a_binary_amsdos_runs() {
        let assembly = crate::asm::assemble(&crate::asm::z80::Z80, Path::new("cpc.asm"), STARTER, &|_| Err("no".into()));
        assert!(assembly.ok(), "{:?}", assembly.problems);
        let code = assembly.image(0x4000, assembly.highest().unwrap());
        let (name, disk) = disk_holding("hello world", 0x4000, assembly.start().unwrap() as u16, &code).unwrap();
        assert_eq!(name, "hellowor.dsk");
        // The DSK's own header, and a CPC data format track: sectors &C1 to &C9.
        assert!(disk.starts_with(b"MV - CPCEMU Disk-File\r\nDisk-Info\r\n"));
        assert_eq!((disk[0x30], disk[0x31], u16::from_le_bytes([disk[0x32], disk[0x33]])), (40, 1, 0x1300));
        assert!(disk[0x100..].starts_with(b"Track-Info\r\n"));
        assert_eq!(disk[0x100 + 0x18 + 2], 0xC1);
        assert_eq!(disk.len(), 256 + 40 * 0x1300);
        assert_eq!(crate::running::Runnable::from_file(&name, disk.clone()).map(|r| r.machine), Some(crate::running::Machine::Cpc));
        // Limoniter's own CP/M reader finds the file; its header is AMSDOS's, its checksum right.
        let volume = crate::media::open(std::sync::Arc::new(disk.clone()), "hello.dsk").unwrap();
        let entries = volume.list(None).unwrap();
        let entry = entries.iter().find(|e| e.name.starts_with("HELLOWOR")).unwrap_or_else(|| panic!("{:?}", entries.iter().map(|e| &e.name).collect::<Vec<_>>()));
        let file = volume.read(entry).unwrap();
        let header = &file[..128];
        assert_eq!(&header[..12], b"\0HELLOWORBIN");
        assert_eq!(header[0x12], 2, "binary");
        assert_eq!(u16::from_le_bytes([header[0x13], header[0x14]]) as usize, code.len());
        assert_eq!(u16::from_le_bytes([header[0x15], header[0x16]]), 0x4000);
        assert_eq!(u16::from_le_bytes([header[0x1A], header[0x1B]]), 0x4000);
        assert_eq!(header[0x40] as usize | (header[0x41] as usize) << 8, code.len());
        let sum: u16 = header[..0x43].iter().map(|b| u16::from(*b)).sum();
        assert_eq!(u16::from_le_bytes([header[0x43], header[0x44]]), sum);
        assert_eq!(&file[128..128 + code.len()], &code[..]);
        assert_eq!(run_command(&disk), "run\"HELLOWOR");
    }

    #[test]
    fn what_runs_a_disk_is_its_basic_program_first() {
        let mut disk = crate::media::write::writer("x.dsk", blank_data_disk()).unwrap();
        assert_eq!(run_command(disk.bytes()), "|cpm", "nothing on it");
        disk.add(None, &Addition { name: "LOADER.BIN".into(), bytes: binary_file("LOADER", "BIN", 0x4000, 0x4000, &[0xC9]), ..Default::default() }).unwrap();
        assert_eq!(run_command(disk.bytes()), "run\"LOADER");
        disk.add(None, &Addition { name: "GAME.BAS".into(), bytes: vec![0; 10], ..Default::default() }).unwrap();
        assert_eq!(run_command(disk.bytes()), "run\"GAME");
        assert!(disk_holding("big", 0x4000, 0x4000, &vec![0; 0x7000]).is_err(), "into AMSDOS's workspace");
    }

    #[test]
    fn each_emulator_is_told_what_to_type() {
        use crate::running::Format;
        let base = std::env::temp_dir().join(format!("limoniter-cpc-{}", std::process::id()));
        std::fs::create_dir_all(&base).unwrap();
        let (_, disk) = disk_holding("demo", 0x4000, 0x4000, &[0xC9]).unwrap();
        let file = base.join("demo.dsk");
        std::fs::write(&file, disk).unwrap();
        let named = file.display().to_string();
        assert_eq!(caprice_arguments(&file, Format::Dsk), ["-a", "run\"DEMO", &named]);
        assert_eq!(caprice_arguments(Path::new("/tmp/a.cdt"), Format::Cdt), ["-a", "|tape", "-a", "run\"", "-a", "", "-a", "CAP32_TAPEPLAY", "/tmp/a.cdt"]);
        assert_eq!(caprice_arguments(Path::new("/tmp/a.sna"), Format::Sna), ["/tmp/a.sna"]);
        assert_eq!(mame_arguments(&file, Format::Dsk), ["cpc6128", "-flop1", &named, "-skip_gameinfo", "-autoboot_delay", "2", "-autoboot_command", "run\"DEMO\\n"]);
        assert_eq!(mame_arguments(Path::new("/tmp/a.sna"), Format::Sna), ["cpc6128", "-dump", "/tmp/a.sna", "-skip_gameinfo"]);
        let _ = std::fs::remove_dir_all(&base);
    }
}
