//! Apple II programs run from a floppy Limoniter makes for them, which boots straight into the program, and a
//! starter program.
//!
//! A DOS 3.3 disk would need DOS itself on it, which is Apple's, so the disk is made to boot on its own. The Disk
//! II controller's boot ROM (the 16-sector P5A, in slot 6) reads sector 0 of track 0 into $0800, then reads the
//! track's next sectors into the pages after it for as long as their number is below the byte at $0800, and jumps
//! to $0801. Sector 0 here holds that count and a few instructions: they switch the drive's motor off, copy the
//! program from $0900 to where it was assembled for, and jump to its start. So a program can be up to fifteen
//! sectors (3,840 bytes) long, anywhere from $0900 up to $BFFF that the copy doesn't overlap itself.
//!
//! The ROM reads the sectors by the numbers written on the disk itself, its physical sectors; a .dsk keeps them
//! in DOS 3.3's order, which emulators turn into the physical ones by DOS's interleave. So physical sector 1 is
//! the .dsk's sector 7, and so on.

use crate::asm::Assembly;

/// DOS 3.3's sector, for each physical sector on a track, as a .dsk keeps them (the inverse of DOS's
/// interleave: logical 0, 13, 11 ... are physical 0, 1, 2 ...).
const PHYSICAL_TO_DOS: [usize; 16] = [0, 7, 14, 6, 13, 5, 12, 4, 11, 3, 10, 2, 9, 1, 8, 15];

/// Where the boot ROM puts the first sector, and the program after it.
const BOOT: i64 = 0x0800;
const LOADED: i64 = 0x0900;

/// The boot sector's code, as source for Limoniter's own assembler: the ROM leaves the slot's number times 16 in
/// X, so $C088,X is that slot's drive's motor off.
fn boot_source(load: i64, start: i64, pages: usize, sectors: usize) -> String {
    format!(
        "        org $0800
        .byte {sectors}
        lda $C088,x
        lda #$00
        sta $06
        lda #$09
        sta $07
        lda #{load_low}
        sta $08
        lda #{load_high}
        sta $09
        ldx #{pages}
        ldy #0
copy:   lda ($06),y
        sta ($08),y
        iny
        bne copy
        inc $07
        inc $09
        dex
        bne copy
        jmp {start}
",
        load_low = load & 0xFF,
        load_high = load >> 8,
    )
}

/// A 140K floppy (.dsk, in DOS 3.3's order) that boots into the code: `code` goes at `load` and is started at
/// `start`.
pub fn boot_disk(load: i64, start: i64, code: &[u8]) -> Result<Vec<u8>, String> {
    let pages = code.len().div_ceil(256);
    if pages == 0 {
        return Err(crate::i18n::tr("Nothing was assembled.").into());
    }
    if pages > 15 {
        return Err(crate::i18n::tf!("An Apple II program booted by Limoniter's disk is at most 3,840 bytes (fifteen sectors of track 0), and this is {}.", code.len()));
    }
    let end = load + pages as i64 * 256;
    if (load != LOADED && load < LOADED + pages as i64 * 256) || end > 0xC000 {
        return Err(crate::i18n::tf!("The program is put at ${load:04X}, where the copy from $0900 would overwrite itself or reach $C000: put it at $0900, or from ${:04X} up to $BFFF.", LOADED + pages as i64 * 256));
    }
    let source = boot_source(load, start, pages, pages + 1);
    let assembly = crate::asm::assemble(&crate::asm::m6502::M6502, std::path::Path::new("boot.a65"), &source, &|_| Err(String::new()));
    if !assembly.ok() {
        return Err(crate::i18n::tr("The disk's boot sector couldn't be made.").into());
    }
    let boot = assembly.image(BOOT, assembly.highest().unwrap_or(BOOT));
    let mut disk = vec![0u8; crate::running::APPLE_FLOPPY];
    let mut physical = boot;
    physical.resize(256, 0);
    let mut padded = code.to_vec();
    padded.resize(pages * 256, 0);
    physical.extend(padded);
    for (sector, bytes) in physical.chunks(256).enumerate() {
        let at = PHYSICAL_TO_DOS[sector] * 256;
        disk[at..at + 256].copy_from_slice(bytes);
    }
    Ok(disk)
}

/// The disk for assembled code: from its lowest address to its highest, started at the END line's label (or the
/// lowest address).
pub fn program_disk(assembly: &Assembly) -> Result<Vec<u8>, String> {
    let (Some(low), Some(high)) = (assembly.lowest(), assembly.highest()) else {
        return Err(crate::i18n::tr("Nothing was assembled.").into());
    };
    boot_disk(low, assembly.start().unwrap_or(low), &assembly.image(low, high))
}

/// File, New as, Apple II (6502): the screen cleared and a line printed, through the Monitor ROM's own routines.
pub const STARTER: &str = r#"; machine: apple2
; An Apple II program. F9 puts it on a floppy (.dsk) that boots straight into it, and runs it in an emulator:
; AppleWin, or MAME's Apple IIe. The disk's first sector copies the program to where it's put here, so it can
; be anywhere from $0900 (or above the copy, from $1800) to $BFFF, and up to 3,840 bytes long.

HOME    = $FC58                 ; the Monitor ROM: clears the text screen
COUT    = $FDED                 ; prints the character in A (with its top bit set, as the Apple's text has it)

        org $6000
start:
        jsr HOME
        ldx #0
print:  lda message,x
        beq done
        ora #$80
        jsr COUT
        inx
        bne print
done:   jmp done

message:
        .byte "HELLO FROM LIMONITER", 0

        end start
"#;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::m6502_cpu::{Bus, Cpu};
    use std::path::Path;

    struct Ram(Vec<u8>);
    impl Bus for Ram {
        fn read(&mut self, address: u16) -> u8 {
            self.0[usize::from(address)]
        }
        fn write(&mut self, address: u16, value: u8) {
            self.0[usize::from(address)] = value;
        }
    }

    /// What the boot ROM does with a disk: track 0's physical sectors, read into $0800 on, as many as the first
    /// one's first byte says; then it jumps to $0801 with the slot (6) times 16 in X.
    fn boot(disk: &[u8]) -> (Cpu, Ram) {
        let mut ram = Ram(vec![0; 0x10000]);
        let sector = |physical: usize| &disk[PHYSICAL_TO_DOS[physical] * 256..][..256];
        let count = usize::from(sector(0)[0]);
        for physical in 0..count {
            ram.0[0x0800 + physical * 256..][..256].copy_from_slice(sector(physical));
        }
        let cpu = Cpu { pc: 0x0801, x: 0x60, s: 0xFF, ..Cpu::default() };
        (cpu, ram)
    }

    #[test]
    fn the_starter_boots_from_its_disk_into_the_program() {
        let assembly = crate::asm::assemble(&crate::asm::m6502::M6502, Path::new("apple.asm"), STARTER, &|_| Err("no".into()));
        assert!(assembly.ok(), "{:?}", assembly.problems);
        let disk = program_disk(&assembly).unwrap();
        assert_eq!(disk.len(), 143_360);
        assert_eq!(crate::running::Runnable::from_file("hello.dsk", disk.clone()).map(|r| (r.machine, r.format)), Some((crate::running::Machine::Apple2, crate::running::Format::Dsk)));
        let (mut cpu, mut ram) = boot(&disk);
        assert_eq!(ram.0[0x0800], 2, "the boot sector and one of program");
        for _ in 0..10_000 {
            if cpu.pc == 0x6000 {
                break;
            }
            cpu.step(&mut ram);
        }
        assert_eq!(cpu.pc, 0x6000, "it jumped to the program's start");
        let program = assembly.image(0x6000, assembly.highest().unwrap());
        assert_eq!(&ram.0[0x6000..0x6000 + program.len()], &program[..]);
    }

    #[test]
    fn a_longer_program_is_read_from_sectors_in_the_rom_s_order() {
        // Five pages, each filled with its number, put at $2000 and started at $2100.
        let code: Vec<u8> = (0..5 * 256).map(|i| (i / 256) as u8 + 1).collect();
        let disk = boot_disk(0x2000, 0x2100, &code).unwrap();
        // Physical sector 1, the program's first page, is the .dsk's sector 7; physical 2 its sector 14.
        assert!(disk[7 * 256..8 * 256].iter().all(|b| *b == 1));
        assert!(disk[14 * 256..15 * 256].iter().all(|b| *b == 2));
        let (mut cpu, mut ram) = boot(&disk);
        assert_eq!(ram.0[0x0800], 6);
        for _ in 0..100_000 {
            if cpu.pc == 0x2100 {
                break;
            }
            cpu.step(&mut ram);
        }
        assert_eq!(cpu.pc, 0x2100);
        assert_eq!(&ram.0[0x2000..0x2000 + code.len()], &code[..]);
        // At $0900 there's nothing to copy, but the copy of itself does no harm.
        assert!(boot_disk(0x0900, 0x0900, &code).is_ok());
    }

    #[test]
    fn programs_that_do_not_fit_are_refused() {
        assert!(boot_disk(0x6000, 0x6000, &vec![0; 16 * 256]).unwrap_err().contains("3,840"));
        assert!(boot_disk(0x0A00, 0x0A00, &[0; 512]).is_err(), "overlapping the copy");
        assert!(boot_disk(0xBF00, 0xBF00, &[0; 512]).is_err(), "into the I/O at $C000");
        assert!(boot_disk(0x0400, 0x0400, &[0; 256]).is_err(), "below the boot sector");
        assert!(boot_disk(0x1800, 0x1800, &vec![0; 15 * 256]).is_ok(), "above the copy, however long");
        assert!(boot_disk(0x1700, 0x1700, &vec![0; 15 * 256]).is_err());
    }
}
