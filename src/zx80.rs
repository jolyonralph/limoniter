//! The ZX80 built into Limoniter: Sinclair's first computer (1980), which is the ZX81's machine (see `zx81`) with
//! its NMI generator taken away, its own 4K ROM (chosen by the person; see `micro_roms`), and 1K of RAM, or 16K
//! for a program that needs the RAM pack.
//!
//! How it differs, from Geoff Wearmouth's assembly listing of the ZX80's ROM (after John Grant's original for
//! Sinclair, 1979, and Wilf Rigter's account of the ZX80's and ZX81's video): the picture is made only by the
//! ROM's display routine, which the ROM runs while it waits for a key. Each line is a HALT that the maskable
//! interrupt ends, as on the ZX81, and the interrupt's acknowledgement is what makes the horizontal sync; the top
//! and bottom margins are lines of the same kind, counted in RESULT. There is no NMI to count lines while BASIC
//! runs, so a running program leaves the television with no syncs at all, and the picture goes blank until it
//! stops, as it did on a real ZX80.
//!
//! A `.o` file is the ZX80's memory from &4000 (its forty bytes of system variables first) up to E_LINE, as SAVE
//! writes it (SAVE at &01B6 and LOAD at &0206 in the ROM, which both stop at E_LINE, read from the bytes as they
//! go). LOAD is caught as it starts and given the next program from the tape the machine was started with; SAVE is
//! caught too, and what it saves is kept.

use crate::z80_micro::{Core, Hardware, Key};
use crate::zx81::{Hw, Model};
use rustzx_z80::Z80;

/// Where RAM, and a `.o` file, starts: the system variables.
pub const FILE_START: u16 = 0x4000;
/// Where the program's lines start, after the forty system variables.
pub const PROGRAM: u16 = 0x4028;
/// The ROM's SAVE and LOAD, each entered by a jump from the command table with a return address on the stack
/// that they drop, and MAIN-EXEC, where both go when they've finished.
const SAVE_ROUTINE: u16 = 0x01B6;
const LOAD_ROUTINE: u16 = 0x0206;
const MAIN_EXEC: u16 = 0x0283;
/// The system variable E_LINE, where a program's memory ends.
const E_LINE: u16 = 0x400A;

/// The keys pressed together to type a character, in L (letters) mode: a letter, digit, space, full stop or
/// NEWLINE on its own, and the signs with SHIFT, which are on different keys from the ZX81's.
pub fn keys_for(c: char) -> Option<Vec<Key>> {
    let plain = |name: &str| crate::zx81::key(name).map(|k| vec![k]);
    let shifted = |name: &str| crate::zx81::key(name).map(|k| vec![(0, 0), k]);
    match c.to_ascii_uppercase() {
        c @ ('A'..='Z' | '0'..='9') => plain(&c.to_string()),
        ' ' => plain("SPACE"),
        '.' => plain("."),
        '\n' | '\r' => plain("NEWLINE"),
        ':' => shifted("Z"),
        ';' => shifted("X"),
        '?' => shifted("C"),
        '/' => shifted("V"),
        '"' => shifted("Y"),
        '$' => shifted("U"),
        '(' => shifted("I"),
        ')' => shifted("O"),
        '*' => shifted("P"),
        '-' => shifted("J"),
        '+' => shifted("K"),
        '=' => shifted("L"),
        '<' => shifted("N"),
        '>' => shifted("M"),
        ',' => shifted("."),
        '£' => shifted("SPACE"),
        _ => None,
    }
}

/// LOAD and SAVE caught as the ROM starts them: LOAD is given the tape's next program, if there is one, and SAVE's
/// bytes are kept. Both then go on to MAIN-EXEC, as the ROM does when the tape has finished. Whether it was.
pub(crate) fn catch_tape(hw: &mut Hw, cpu: &mut Z80, pc: u16) -> bool {
    let word = |hw: &Hw, a: u16| u16::from_le_bytes([hw.peek(a), hw.peek(a + 1)]);
    match pc {
        LOAD_ROUTINE if hw.next < hw.tape.len() => {
            let (_, program) = hw.tape[hw.next].clone();
            hw.next += 1;
            hw.load_at(FILE_START, &program);
        }
        SAVE_ROUTINE => {
            let end = word(hw, E_LINE).max(PROGRAM);
            let bytes = (FILE_START..end).map(|a| hw.peek(a)).collect();
            hw.saved.push(bytes);
        }
        _ => return false,
    }
    // Both drop the return address they were entered with.
    cpu.regs.set_sp(cpu.regs.get_sp().wrapping_add(2));
    cpu.regs.set_pc(MAIN_EXEC);
    true
}

/// The ZX80, running.
pub struct Zx80 {
    pub core: Core<Hw>,
}

/// Frames to let the ROM start up before a program is loaded: it fills and checks its RAM, which takes a few.
const BOOT_FRAMES: usize = 40;
/// How long a key is held, and let go, when the machine is typed to: the ROM wants the same key read on two
/// frames running, and no key for five, before it takes the next.
const HOLD: u32 = 4;
const GAP: u32 = 8;

impl Zx80 {
    /// Switches on a ZX80 with this ROM, and its programs (each a `.o` file's bytes, by name): the first is loaded
    /// with LOAD, typed as a person would, and RUN, and the others are kept for LOAD.
    pub fn start(rom: Vec<u8>, programs: Vec<(String, Vec<u8>)>) -> Result<Zx80, String> {
        if rom.len() != 4096 {
            return Err(crate::i18n::tr("The ZX80's ROM is 4K.").into());
        }
        // 1K, as the ZX80 came, unless the program wants the 16K RAM pack: its own bytes, a screen and a stack.
        let wanted = programs.first().map_or(0, |(_, p)| p.len()) + 33 * 24 + 100;
        let ram_k = if wanted <= 1024 { 1 } else { 16 };
        if let Some((_, first)) = programs.first()
            && first.len() > 16 * 1024 - 100
        {
            return Err(crate::i18n::tf!("The program is {} bytes, more than fits in a ZX80 with 16K.", first.len()));
        }
        let mut machine = Zx80 { core: Core::new(Z80::default(), Hw::new(rom, ram_k, Model::Zx80)) };
        for _ in 0..BOOT_FRAMES {
            machine.core.frame();
        }
        if !programs.is_empty() {
            machine.core.hw.tape = programs;
            machine.core.hw.next = 0;
            // In K (keyword) mode, W is LOAD and R is RUN.
            for key in ["W", "NEWLINE", "R", "NEWLINE"] {
                machine.press(key);
            }
        }
        Ok(machine)
    }

    /// A key typed on the ZX80's keyboard.
    pub fn press(&mut self, name: &str) {
        if let Some(key) = crate::zx81::key(name) {
            self.core.type_keys(vec![key], HOLD, GAP);
        }
    }

    /// What the ZX80's own SAVE has saved, each as a `.o` file.
    pub fn saved(&self) -> &[Vec<u8>] {
        &self.core.hw.saved
    }

    /// The characters on the screen, from the display file, a row a line.
    #[cfg(test)]
    pub fn screen_text(&self) -> Vec<String> {
        let hw = &self.core.hw;
        let word = |a: u16| u16::from_le_bytes([hw.peek(a), hw.peek(a + 1)]);
        let set = crate::charset::known("zx80");
        let mut at = word(0x400C).wrapping_add(1);
        let mut rows = Vec::new();
        for _ in 0..24 {
            let mut row = String::new();
            for _ in 0..33 {
                let b = hw.peek(at);
                at = at.wrapping_add(1);
                if b == 0x76 {
                    break;
                }
                row.push(set.entry(0, b & 0x3F).and_then(|e| e.unicode).unwrap_or(' '));
            }
            rows.push(row.trim_end().to_string());
        }
        rows
    }
}

/// The programs a file holds, by name, each as a `.o` file's bytes from &4000: a `.o` (or `.80`).
pub fn programs_in(name: &str, bytes: &[u8]) -> Result<Vec<(String, Vec<u8>)>, String> {
    let length = crate::basic_extra::zx80_length(bytes)?;
    let stem = name.rsplit_once('.').map_or(name, |(stem, _)| stem);
    Ok(vec![(stem.to_string(), bytes[..length].to_vec())])
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::z80_micro::{HEIGHT, WIDTH};

    /// Where the ZX80 ROM the person has allowed for testing is, if it's here: named by LIMONITER_ZX80_ROM, or in
    /// its usual place. It's read from there, never part of Limoniter or copied.
    pub(crate) fn real_rom_path() -> Option<std::path::PathBuf> {
        let path = std::path::PathBuf::from(std::env::var_os("LIMONITER_ZX80_ROM").unwrap_or_else(|| "/root/zxroms/zx80.rom".into()));
        std::fs::read(&path).ok().filter(|rom| crate::micro_roms::identify(crate::micro_roms::Needs::Zx80, rom).is_ok()).map(|_| path)
    }

    pub(crate) fn real_rom() -> Option<Vec<u8>> {
        real_rom_path().and_then(|path| std::fs::read(path).ok())
    }

    /// Whether the picture has any ink in it.
    fn inked(zx80: &Zx80) -> usize {
        zx80.core.hw.picture().chunks(4).filter(|p| p[0] < 0x80).count()
    }

    #[test]
    fn the_signs_are_on_the_zx80s_own_keys() {
        assert_eq!(keys_for('"'), Some(vec![(0, 0), (5, 4)]), "\" is SHIFT and Y");
        assert_eq!(keys_for('*'), Some(vec![(0, 0), (5, 0)]), "* is SHIFT and P");
        assert_eq!(keys_for('a'), Some(vec![(1, 0)]));
        assert_eq!(keys_for('{'), None);
    }

    /// With no interrupts there are no syncs, and the television shows nothing: a blank picture, as a ZX80 running
    /// a program shows.
    #[test]
    fn with_no_syncs_the_picture_goes_blank() {
        // A ROM of one instruction: JR to itself.
        let mut rom = vec![0; 4096];
        rom[..2].copy_from_slice(&[0x18, 0xFE]);
        let mut core = Core::new(Z80::default(), Hw::new(rom, 1, Model::Zx80));
        for _ in 0..20 {
            core.frame();
        }
        assert!(core.hw.frames >= 2, "the television gives up and shows a blank picture ({})", core.hw.frames);
        assert!(core.hw.picture().chunks(4).all(|p| p[0] > 0x80), "all paper");
    }

    /// LOAD and SAVE are caught as the ROM starts them, with the return address they were entered with dropped.
    #[test]
    fn load_and_save_are_caught() {
        let mut hw = Hw::new(vec![0; 4096], 1, Model::Zx80);
        let program = crate::zx80_program::program(&["10 PRINT 1"]).unwrap();
        hw.tape = vec![("one".into(), program.clone())];
        let mut cpu = Z80::default();
        cpu.regs.set_sp(0x43FE);
        assert!(catch_tape(&mut hw, &mut cpu, LOAD_ROUTINE));
        assert_eq!((cpu.regs.get_pc(), cpu.regs.get_sp()), (MAIN_EXEC, 0x4400));
        assert_eq!((FILE_START..FILE_START + program.len() as u16).map(|a| hw.peek(a)).collect::<Vec<_>>(), program);
        assert!(!catch_tape(&mut hw, &mut cpu, LOAD_ROUTINE), "the tape has run out: LOAD waits for one");
        assert!(catch_tape(&mut hw, &mut cpu, SAVE_ROUTINE));
        assert_eq!(hw.saved, vec![program], "SAVE keeps memory up to E_LINE");
    }

    /// With the real ROM, the ZX80 starts with its K cursor at the bottom of the screen.
    #[test]
    fn with_the_real_rom_it_starts_with_the_k_cursor() {
        let Some(rom) = real_rom() else {
            return;
        };
        let mut zx80 = Zx80::start(rom.clone(), Vec::new()).unwrap();
        for _ in 0..20 {
            zx80.core.frame();
        }
        assert!(zx80.core.hw.frames > 50, "a picture a frame while it waits for a key ({})", zx80.core.hw.frames);
        // The inverse K, drawn from the ROM's own patterns at &0E00, as the only thing on the screen.
        let rgba = zx80.core.hw.picture();
        let ink: Vec<(usize, usize)> = (0..HEIGHT).flat_map(|y| (0..WIDTH).map(move |x| (x, y))).filter(|(x, y)| rgba[(y * WIDTH + x) * 4] < 0x80).collect();
        let (left, top) = (ink.iter().map(|p| p.0).min().unwrap(), ink.iter().map(|p| p.1).min().unwrap());
        assert_eq!(ink.len(), 64 - (0..8).map(|line| rom[0x0E00 + 0x30 * 8 + line].count_ones() as usize).sum::<usize>(), "an inverse K and nothing else");
        let line = |x: usize, y: usize| (0..8).fold(0u8, |b, i| b << 1 | u8::from(rgba[(y * WIDTH + x + i) * 4] < 0x80));
        for row_line in 0..8 {
            assert_eq!(line(left, top + row_line), !rom[0x0E00 + 0x30 * 8 + row_line], "line {row_line} of the K");
        }
        assert_eq!(left, 32, "at the left of the screen, inside the border");
        if let Some(out) = std::env::var_os("LIMONITER_PICTURE_OUT") {
            let mut ppm = format!("P6 {WIDTH} {HEIGHT} 255\n").into_bytes();
            ppm.extend(rgba.chunks(4).flat_map(|p| [p[0], p[1], p[2]]));
            std::fs::write(out, ppm).unwrap();
        }
    }

    /// With the real ROM, a program is loaded and run, the picture blank while it runs; and the ROM's own SAVE
    /// saves it back as it was loaded.
    #[test]
    fn with_the_real_rom_a_program_runs_with_the_picture_blank() {
        let Some(rom) = real_rom() else {
            return;
        };
        let program = crate::zx80_program::program(&["10 FOR I=1 TO 5", "20 PRINT I*I", "30 NEXT I", "40 LET J=0", "50 FOR K=1 TO 1000", "60 LET J=J+1", "70 NEXT K", "80 PRINT \"SUM \";J"]).unwrap();
        let mut zx80 = Zx80::start(rom.clone(), vec![("squares".into(), program.clone())]).unwrap();
        // While it's typed LOAD and RUN, and runs, the picture comes and goes; while the long loop runs, it's blank.
        let mut blank = 0;
        let mut text = Vec::new();
        for _ in 0..3000 {
            zx80.core.frame();
            if inked(&zx80) == 0 {
                blank += 1;
            }
            text = zx80.screen_text();
            if text.iter().any(|row| row.starts_with("SUM")) {
                break;
            }
        }
        assert_eq!(text[..6], ["1", "4", "9", "16", "25", "SUM 1000"], "{text:?}");
        assert!(blank >= 10, "the picture was blank while it ran ({blank} frames)");
        // Once it stops, the ROM draws the screen again, and waits for a key.
        for _ in 0..30 {
            zx80.core.frame();
        }
        assert!(inked(&zx80) > 0, "the screen is back");
        // NEWLINE to go back from the program's output to the listing, then SAVE (E in K mode), caught: the program
        // as it was loaded, with its variables now after it.
        for key in ["NEWLINE", "E", "NEWLINE"] {
            zx80.press(key);
        }
        for _ in 0..100 {
            zx80.core.frame();
        }
        let saved = zx80.saved().first().expect("SAVE saved").clone();
        let listing = crate::basic_extra::list_zx80(&saved);
        assert_eq!(listing.lines, crate::basic_extra::list_zx80(&program).lines, "{:?}", listing.notes);
        let vars = |file: &[u8]| usize::from(u16::from_le_bytes([file[8], file[9]]) - FILE_START);
        assert_eq!(saved[..0x28].len(), 0x28);
        assert_eq!(saved[0x28..vars(&saved)], program[0x28..vars(&program)], "the lines saved as they were tokenised");
        // Loaded again on a fresh ZX80, it runs the same.
        let mut again = Zx80::start(rom, vec![("again".into(), saved)]).unwrap();
        for _ in 0..3000 {
            again.core.frame();
            if again.screen_text().iter().any(|row| row.starts_with("SUM")) {
                break;
            }
        }
        assert_eq!(again.screen_text()[5], "SUM 1000");
    }
}

