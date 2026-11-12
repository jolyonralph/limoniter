//! What runs where: the machines programs run on, the files they load, and working out which machine a file is
//! for. Nothing here starts anything; see RUNNING.md for how the pieces fit.

use crate::basic;
use std::sync::Arc;

/// A machine a program runs on.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Machine {
    Spectrum48,
    Spectrum128,
    SpectrumPlus3,
    Pentagon,
    Next,
    C64,
    C128,
    Vic20,
    Plus4,
    Pet,
    Bbc,
    Msx,
    Cpc,
}

impl Machine {
    pub const ALL: [Machine; 13] = [
        Machine::Spectrum48,
        Machine::Spectrum128,
        Machine::SpectrumPlus3,
        Machine::Pentagon,
        Machine::Next,
        Machine::C64,
        Machine::C128,
        Machine::Vic20,
        Machine::Plus4,
        Machine::Pet,
        Machine::Bbc,
        Machine::Msx,
        Machine::Cpc,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Machine::Spectrum48 => "ZX Spectrum 48K",
            Machine::Spectrum128 => "ZX Spectrum 128K",
            Machine::SpectrumPlus3 => "ZX Spectrum +3",
            Machine::Pentagon => "Pentagon (TR-DOS)",
            Machine::Next => "ZX Spectrum Next",
            Machine::C64 => "Commodore 64",
            Machine::C128 => "Commodore 128",
            Machine::Vic20 => "Commodore VIC-20",
            Machine::Plus4 => "Commodore 16 and Plus/4",
            Machine::Pet => "Commodore PET",
            Machine::Bbc => "BBC Micro",
            Machine::Msx => "MSX",
            Machine::Cpc => "Amstrad CPC",
        }
    }

    /// Its name in the settings file.
    pub fn id(self) -> &'static str {
        match self {
            Machine::Spectrum48 => "spectrum48",
            Machine::Spectrum128 => "spectrum128",
            Machine::SpectrumPlus3 => "spectrum+3",
            Machine::Pentagon => "pentagon",
            Machine::Next => "next",
            Machine::C64 => "c64",
            Machine::C128 => "c128",
            Machine::Vic20 => "vic20",
            Machine::Plus4 => "plus4",
            Machine::Pet => "pet",
            Machine::Bbc => "bbc",
            Machine::Msx => "msx",
            Machine::Cpc => "cpc",
        }
    }

    pub fn from_id(id: &str) -> Option<Machine> {
        Machine::ALL.into_iter().find(|machine| machine.id() == id)
    }

    /// A Spectrum of the kind the built-in machine, and any Spectrum emulator, runs.
    pub fn is_classic_spectrum(self) -> bool {
        matches!(self, Machine::Spectrum48 | Machine::Spectrum128 | Machine::SpectrumPlus3 | Machine::Pentagon)
    }
}

/// A kind of file a machine loads.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Format {
    Tap,
    Tzx,
    Z80,
    Sna,
    /// A file with a +3DOS header: a +3 or Next BASIC program, or code.
    Plus3dos,
    Nex,
    Dsk,
    Trd,
    Scl,
    Prg,
    P00,
    T64,
    D64,
    D71,
    D81,
    Crt,
    Ssd,
    Dsd,
    Rom,
    Cas,
}

impl Format {
    pub const ALL: [Format; 20] = [
        Format::Tap,
        Format::Tzx,
        Format::Z80,
        Format::Sna,
        Format::Plus3dos,
        Format::Nex,
        Format::Dsk,
        Format::Trd,
        Format::Scl,
        Format::Prg,
        Format::P00,
        Format::T64,
        Format::D64,
        Format::D71,
        Format::D81,
        Format::Crt,
        Format::Ssd,
        Format::Dsd,
        Format::Rom,
        Format::Cas,
    ];

    /// The file ending it's written with.
    pub fn ending(self) -> &'static str {
        match self {
            Format::Tap => "tap",
            Format::Tzx => "tzx",
            Format::Z80 => "z80",
            Format::Sna => "sna",
            Format::Plus3dos => "bas",
            Format::Nex => "nex",
            Format::Dsk => "dsk",
            Format::Trd => "trd",
            Format::Scl => "scl",
            Format::Prg => "prg",
            Format::P00 => "p00",
            Format::T64 => "t64",
            Format::D64 => "d64",
            Format::D71 => "d71",
            Format::D81 => "d81",
            Format::Crt => "crt",
            Format::Ssd => "ssd",
            Format::Dsd => "dsd",
            Format::Rom => "rom",
            Format::Cas => "cas",
        }
    }

    pub fn from_ending(ending: &str) -> Option<Format> {
        let ending = ending.to_ascii_lowercase();
        Format::ALL.into_iter().find(|format| format.ending() == ending)
    }

    /// Whether it's a whole disk, which a machine mounts rather than loads.
    pub fn is_disk(self) -> bool {
        matches!(self, Format::Dsk | Format::Trd | Format::Scl | Format::D64 | Format::D71 | Format::D81 | Format::Ssd | Format::Dsd)
    }
}

/// Something ready to run: the machine it's for, and the file that machine loads, under the name it goes by.
#[derive(Clone, Debug)]
pub struct Runnable {
    pub machine: Machine,
    pub format: Format,
    pub name: String,
    pub bytes: Arc<Vec<u8>>,
}

impl Runnable {
    /// A file as it is, if the machine it's for can be told from it.
    pub fn from_file(name: &str, bytes: Vec<u8>) -> Option<Runnable> {
        let ending = name.rsplit_once('.').map(|(_, ending)| ending)?;
        let format = Format::from_ending(ending)?;
        let machine = machine_for(format, &bytes)?;
        Some(Runnable { machine, format, name: name.to_string(), bytes: Arc::new(bytes) })
    }

    /// A BASIC listing, tokenised as the program its machine loads. `stem` names it.
    pub fn from_listing<S: AsRef<str>>(lines: &[S], machine: basic::Machine, stem: &str) -> Result<Runnable, String> {
        let (machine, format, bytes) = match machine {
            basic::Machine::Spectrum => (Machine::Spectrum48, Format::Tap, basic::spectrum_tape(&basic::tokenise_spectrum(lines, basic::ZxDialect::Spectrum)?, stem, Some(0))),
            basic::Machine::Next => (Machine::Next, Format::Plus3dos, basic::plus3dos_file(&basic::tokenise_spectrum(lines, basic::ZxDialect::Next)?, Some(0))),
            basic::Machine::Commodore(cbm) => (commodore(cbm), Format::Prg, basic::tokenise_commodore(lines, cbm)?),
            basic::Machine::Bbc => (Machine::Bbc, Format::Ssd, bbc_disc(stem, &crate::basic_more::tokenise_bbc(lines)?)),
        };
        Ok(Runnable { machine, format, name: format!("{}.{}", file_stem(stem), format.ending()), bytes: Arc::new(bytes) })
    }
}

fn commodore(cbm: basic::CbmMachine) -> Machine {
    match cbm {
        basic::CbmMachine::C64 => Machine::C64,
        basic::CbmMachine::Vic20 => Machine::Vic20,
        basic::CbmMachine::Pet => Machine::Pet,
        basic::CbmMachine::Plus4 => Machine::Plus4,
        basic::CbmMachine::C128 => Machine::C128,
    }
}

/// A name that's safe as a file name anywhere.
fn file_stem(stem: &str) -> String {
    // A name ends at its first dot: notes.bas.txt is notes.
    let stem = stem.split('.').next().unwrap_or(stem);
    let safe: String = stem.chars().map(|c| if c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | ' ') { c } else { '_' }).collect();
    if safe.trim().is_empty() { "program".to_string() } else { safe.trim().to_string() }
}

/// The machine a file is for, from the kind of file and what's in it.
pub fn machine_for(format: Format, bytes: &[u8]) -> Option<Machine> {
    Some(match format {
        Format::Tap | Format::Tzx => Machine::Spectrum48,
        Format::Z80 => z80_machine(bytes),
        Format::Sna => {
            if bytes.len() == 49179 {
                Machine::Spectrum48
            } else {
                Machine::Spectrum128
            }
        }
        Format::Plus3dos => {
            if !bytes.starts_with(b"PLUS3DOS") {
                return None;
            }
            // A BASIC program is the Next's if it's NextBASIC; anything else a +3 runs too.
            let listed = basic::list(bytes, "bas", crate::charset::utf8());
            if listed.dialect == basic::ZxDialect::Next.name() { Machine::Next } else { Machine::SpectrumPlus3 }
        }
        Format::Nex => Machine::Next,
        Format::Dsk => dsk_machine(bytes),
        Format::Trd | Format::Scl => Machine::Pentagon,
        Format::Prg => commodore_by_load(bytes.get(0..2)?),
        Format::P00 => commodore_by_load(bytes.get(26..28)?),
        Format::T64 | Format::D64 | Format::D71 | Format::D81 | Format::Crt => Machine::C64,
        Format::Ssd | Format::Dsd => Machine::Bbc,
        Format::Rom => {
            if bytes.starts_with(b"AB") {
                Machine::Msx
            } else {
                return None;
            }
        }
        Format::Cas => Machine::Msx,
    })
}

/// A Commodore program says which machine it's for by where it loads.
fn commodore_by_load(load: &[u8]) -> Machine {
    match u16::from_le_bytes([load[0], load[1]]) {
        0x1C01 => Machine::C128,
        0x1001 | 0x1201 => Machine::Vic20,
        0x0401 => Machine::Pet,
        _ => Machine::C64,
    }
}

/// A Z80 snapshot's header says which machine it was taken on. Version 1 is always a 48K; later versions name
/// the hardware, numbered a little differently in version 2 and version 3.
fn z80_machine(bytes: &[u8]) -> Machine {
    let pc = bytes.get(6..8).map_or(0, |b| u16::from_le_bytes([b[0], b[1]]));
    if pc != 0 || bytes.len() < 35 {
        return Machine::Spectrum48;
    }
    let version2 = bytes.get(30..32).is_some_and(|b| u16::from_le_bytes([b[0], b[1]]) == 23);
    match (version2, bytes[34]) {
        (true, 3 | 4) | (false, 4..=6) | (_, 12) => Machine::Spectrum128,
        (_, 7 | 8 | 13) => Machine::SpectrumPlus3,
        (_, 9) => Machine::Pentagon,
        _ => Machine::Spectrum48,
    }
}

/// A +3 disk and an Amstrad CPC disk share a container; the CPC numbers its sectors from &41 or &C1, the +3 from 1.
fn dsk_machine(bytes: &[u8]) -> Machine {
    // The first track's header follows the disk's own, and lists its sectors from byte 0x18, the number third.
    match bytes.get(0x100 + 0x18 + 2) {
        Some(0x41..=0x49 | 0xC1..=0xC9) => Machine::Cpc,
        _ => Machine::SpectrumPlus3,
    }
}

/// A BBC Micro disc (an 80-track single-sided DFS image) holding one BASIC program, which boots by CHAINing it.
pub fn bbc_disc(stem: &str, program: &[u8]) -> Vec<u8> {
    const SECTOR: usize = 256;
    const SECTORS: usize = 800;
    let name: String = stem.chars().filter(|c| c.is_ascii_alphanumeric()).take(7).collect::<String>().to_ascii_uppercase();
    let name = if name.is_empty() { "PROGRAM".to_string() } else { name };
    let boot = format!("CHAIN \"{name}\"\r").into_bytes();
    let mut disc = vec![0u8; SECTORS * SECTOR];
    // The files after the two catalogue sectors: !BOOT, then the program.
    let boot_start = 2;
    let program_start = boot_start + boot.len().div_ceil(SECTOR);
    disc[boot_start * SECTOR..][..boot.len()].copy_from_slice(&boot);
    disc[program_start * SECTOR..][..program.len()].copy_from_slice(program);
    let title = b"LIMONITER   ";
    disc[..8].copy_from_slice(&title[..8]);
    disc[SECTOR..SECTOR + 4].copy_from_slice(&title[8..12]);
    // Listed highest start sector first, as DFS keeps them.
    let files: [(&str, usize, u32, u32, usize); 2] = [(&name, program_start, 0x1900, 0x8023, program.len()), ("!BOOT", boot_start, 0, 0, boot.len())];
    for (i, (file, start, load, exec, length)) in files.iter().enumerate() {
        let mut padded = [b' '; 7];
        padded[..file.len()].copy_from_slice(file.as_bytes());
        disc[8 + i * 8..][..7].copy_from_slice(&padded);
        disc[8 + i * 8 + 7] = b'$';
        let entry = &mut disc[SECTOR + 8 + i * 8..][..8];
        entry[0..2].copy_from_slice(&(*load as u16).to_le_bytes());
        entry[2..4].copy_from_slice(&(*exec as u16).to_le_bytes());
        entry[4..6].copy_from_slice(&(*length as u16).to_le_bytes());
        // The top bits of each, packed: exec, length, load, start sector. BASIC's addresses are in the I/O processor.
        let high = |value: u32| if *load != 0 { 3 } else { ((value >> 16) & 3) as u8 };
        entry[6] = (high(*exec) << 6) | ((((*length >> 16) & 3) as u8) << 4) | (high(*load) << 2) | (((*start >> 8) & 3) as u8);
        entry[7] = (*start & 0xFF) as u8;
    }
    disc[SECTOR + 5] = (files.len() * 8) as u8;
    // Boot option 3 (EXEC !BOOT), and the disc's size in sectors.
    disc[SECTOR + 6] = (3 << 4) | ((SECTORS >> 8) as u8 & 3);
    disc[SECTOR + 7] = (SECTORS & 0xFF) as u8;
    disc
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn files_say_which_machine_they_are_for() {
        let run = |name: &str, bytes: Vec<u8>| Runnable::from_file(name, bytes).map(|r| (r.machine, r.format));
        assert_eq!(run("game.tap", vec![0; 30]), Some((Machine::Spectrum48, Format::Tap)));
        assert_eq!(run("GAME.PRG", vec![0x01, 0x08, 0, 0]), Some((Machine::C64, Format::Prg)));
        assert_eq!(run("game.prg", vec![0x01, 0x1C, 0, 0]).map(|r| r.0), Some(Machine::C128));
        assert_eq!(run("game.prg", vec![0x01, 0x10, 0, 0]).map(|r| r.0), Some(Machine::Vic20));
        assert_eq!(run("game.sna", vec![0; 49179]).map(|r| r.0), Some(Machine::Spectrum48));
        assert_eq!(run("game.sna", vec![0; 131103]).map(|r| r.0), Some(Machine::Spectrum128));
        assert_eq!(run("game.ssd", vec![0; 512]).map(|r| r.0), Some(Machine::Bbc));
        assert_eq!(run("game.rom", b"AB\0\0".to_vec()).map(|r| r.0), Some(Machine::Msx));
        assert_eq!(run("game.rom", vec![0; 4]), None, "a ROM that isn't an MSX cartridge isn't guessed at");
        assert_eq!(run("notes.txt", vec![0; 4]), None);
        assert_eq!(run("game.bas", b"10 PRINT".to_vec()), None, "a .bas without a +3DOS header isn't a Spectrum's");
    }

    #[test]
    fn a_z80_snapshot_says_which_spectrum_took_it() {
        let mut v1 = vec![0u8; 40];
        v1[6] = 0x34;
        assert_eq!(z80_machine(&v1), Machine::Spectrum48);
        let mut v2 = vec![0u8; 60];
        v2[30] = 23;
        v2[34] = 3;
        assert_eq!(z80_machine(&v2), Machine::Spectrum128);
        let mut v3 = vec![0u8; 90];
        v3[30] = 54;
        v3[34] = 3;
        assert_eq!(z80_machine(&v3), Machine::Spectrum48, "in version 3, 3 is a 48K with an M.G.T.");
        v3[34] = 4;
        assert_eq!(z80_machine(&v3), Machine::Spectrum128);
        v3[34] = 7;
        assert_eq!(z80_machine(&v3), Machine::SpectrumPlus3);
    }

    #[test]
    fn listings_become_the_programs_their_machines_load() {
        let next = Runnable::from_listing(&["10 LAYER 2,1"], basic::Machine::Next, "stars").unwrap();
        assert_eq!((next.machine, next.format, next.name.as_str()), (Machine::Next, Format::Plus3dos, "stars.bas"));
        assert_eq!(machine_for(Format::Plus3dos, &next.bytes), Some(Machine::Next), "and it reads back as the Next's");
        let tape = Runnable::from_listing(&["10 PRINT \"hi\""], basic::Machine::Spectrum, "hello world!").unwrap();
        assert_eq!((tape.machine, tape.name.as_str()), (Machine::Spectrum48, "hello world_.tap"));
        assert_eq!(Runnable::from_listing(&["10 STOP"], basic::Machine::Spectrum, "rainbow.bas.txt").unwrap().name, "rainbow.tap");
        let c64 = Runnable::from_listing(&["10 PRINT \"HI\""], basic::Machine::Commodore(basic::CbmMachine::C64), "hi").unwrap();
        assert_eq!((c64.machine, &c64.bytes[..2]), (Machine::C64, &[0x01, 0x08][..]));
    }

    #[test]
    fn a_bbc_listing_goes_on_a_disc_that_boots_it() {
        let program = crate::basic_more::tokenise_bbc(&["10 PRINT \"HELLO\"", "20 GOTO 10"]).unwrap();
        let disc = Runnable::from_listing(&["10 PRINT \"HELLO\"", "20 GOTO 10"], basic::Machine::Bbc, "hello").unwrap();
        assert_eq!((disc.machine, disc.format, disc.name.as_str()), (Machine::Bbc, Format::Ssd, "hello.ssd"));
        // Limoniter's own DFS reader finds both files, the program intact.
        let volume = crate::media::open(Arc::new(disc.bytes.to_vec()), "hello.ssd").unwrap();
        let entries = volume.list(None).unwrap();
        let names: Vec<String> = entries.iter().map(|e| e.name.clone()).collect();
        assert!(names.iter().any(|n| n.contains("!BOOT")) && names.iter().any(|n| n.contains("HELLO")), "{names:?}");
        let found = entries.iter().find(|e| e.name.contains("HELLO")).unwrap();
        assert_eq!(volume.read(found).unwrap(), program);
        let boot = entries.iter().find(|e| e.name.contains("!BOOT")).unwrap();
        assert_eq!(volume.read(boot).unwrap(), b"CHAIN \"HELLO\"\r");
        assert_eq!(disc.bytes[256 + 6] >> 4, 3, "it boots by EXECing !BOOT");
    }

    #[test]
    fn plus3_and_cpc_disks_are_told_apart() {
        let mut disk = vec![0u8; 0x200];
        disk[0x11A] = 0xC1;
        assert_eq!(dsk_machine(&disk), Machine::Cpc);
        disk[0x11A] = 1;
        assert_eq!(dsk_machine(&disk), Machine::SpectrumPlus3);
    }
}
