//! Running a 6502 program on the Neo6502 in its emulator.
//!
//! 6502 source is the Neo6502's when it says so (; machine: neo6502), or the person chooses it when asked. Its
//! bytes are written as they are, and the emulator is given them with where they go and where to start, as its
//! command line takes them: `file@address` loads a file at an address (hex), and `run@address` starts the
//! program there. Programs are loaded at $0800 as a rule, above the system's zero page, stack and buffers, and
//! the top page ($FF00 and up) belongs to the machine's own registers.
//!
//! The command line is from the neo6502-firmware repository's documentation of its emulator, as remembered, not
//! checked here. Its BASIC isn't run: nothing known to the author gives the emulator a BASIC program to start.

use crate::asm::Assembly;
use crate::running::{Format, Machine, Runnable};
use std::path::Path;
use std::sync::Arc;

/// Where programs are loaded unless the source says otherwise.
pub const USUAL: u16 = 0x0800;

/// The lowest address a program may be put at: the zero page, stack and the system's work areas are below.
pub const LOWEST: i64 = 0x0200;

/// The file beside the program saying where it loads and starts (two hexadecimal numbers).
pub const PLACE: &str = "neo-place.txt";

/// Where a program loads and where it starts.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Place {
    pub load: u16,
    pub start: u16,
}

/// The program 6502 source makes, and where it goes.
pub fn program(assembly: &Assembly, stem: &str) -> Result<(Runnable, Place), String> {
    let (Some(low), Some(high)) = (assembly.lowest(), assembly.highest()) else {
        return Err(crate::i18n::tr("Nothing was assembled.").into());
    };
    if low < LOWEST {
        return Err(crate::i18n::tf!("The code starts at ${low:04X}, in the Neo6502's zero page and stack. Put it at $0800 (ORG $0800) or above."));
    }
    if high > 0xFF00 {
        return Err(crate::i18n::tf!("The code reaches ${:04X}, in the Neo6502's own registers at $FF00. Keep it below.", high - 1));
    }
    let start = assembly.start().unwrap_or(low);
    let place = Place { load: low as u16, start: start as u16 };
    let name = format!("{}.bin", stem.chars().map(|c| if c.is_ascii_alphanumeric() || matches!(c, '-' | '_') { c } else { '_' }).collect::<String>());
    Ok((Runnable { machine: Machine::Neo6502, format: Format::NeoBin, name, bytes: Arc::new(assembly.image(low, high)) }, place))
}

/// Writes where the program goes beside it, for the emulator's arguments to read.
pub fn write_place(file: &Path, place: Place) -> Result<(), String> {
    std::fs::write(file.with_file_name(PLACE), format!("{:04X} {:04X}\n", place.load, place.start)).map_err(|e| crate::i18n::tf!("Couldn't make the copy to run.\n\n{e}"))
}

fn read_place(file: &Path) -> Place {
    let text = std::fs::read_to_string(file.with_file_name(PLACE)).unwrap_or_default();
    let mut numbers = text.split_whitespace().filter_map(|n| u16::from_str_radix(n, 16).ok());
    match (numbers.next(), numbers.next()) {
        (Some(load), Some(start)) => Place { load, start },
        _ => Place { load: USUAL, start: USUAL },
    }
}

/// The emulator's arguments: the program loaded where it goes, and started.
pub fn arguments(file: &Path) -> Vec<String> {
    let place = read_place(file);
    vec![format!("{}@{:X}", file.display(), place.load), format!("run@{:X}", place.start)]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn assembled(source: &str) -> crate::assemblers::Assembled {
        crate::assemblers::assemble_own(crate::assemblers::Cpu::M6502, Path::new("p.asm"), source)
    }

    #[test]
    fn a_program_is_its_bytes_with_where_they_go() {
        let made = assembled("\t* = $0900\nstart\tlda #1\n\trts\n");
        let (run, place) = program(&made.assembly, "my demo").unwrap();
        assert_eq!((run.machine, run.format, run.name.as_str()), (Machine::Neo6502, Format::NeoBin, "my_demo.bin"));
        assert_eq!(run.bytes.as_slice(), [0xA9, 0x01, 0x60]);
        assert_eq!(place, Place { load: 0x0900, start: 0x0900 });
    }

    #[test]
    fn the_wrong_places_are_refused() {
        assert!(program(&assembled("\t* = $0100\n\trts\n").assembly, "p").unwrap_err().contains("0100"));
        assert!(program(&assembled("\t* = $FF10\n\trts\n").assembly, "p").is_err());
    }

    #[test]
    fn the_emulator_is_told_where_to_load_and_start() {
        let folder = std::env::temp_dir().join(format!("limoniter-neo-run-{}", std::process::id()));
        std::fs::create_dir_all(&folder).unwrap();
        let file = folder.join("demo.bin");
        let named = file.display().to_string();
        // With nothing written beside it, the usual place.
        assert_eq!(arguments(&file), [format!("{named}@800"), "run@800".to_string()]);
        write_place(&file, Place { load: 0x0A00, start: 0x0A10 }).unwrap();
        assert_eq!(arguments(&file), [format!("{named}@A00"), "run@A10".to_string()]);
        let _ = std::fs::remove_dir_all(&folder);
    }
}
