//! The Game Boy's cartridges: the header the boot ROM reads at $0100 to $014F, made complete as RGBDS's rgbfix
//! makes it (the Nintendo logo, the ROM's size, the title and both checksums), and a starter program.
//!
//! The boot ROM won't start a cartridge unless the logo at $0104 is Nintendo's, byte for byte, and its header
//! checksum at $014D is right; the global checksum at $014E isn't checked by the Game Boy, though emulators and
//! tools say when it's wrong. The logo is put in by every Game Boy toolchain (rgbfix, GBDK's makebin): copying it
//! so a program can run is what interoperating with the console needs, as Sega v. Accolade found for Sega's own
//! check, and RGBDS carries it in its MIT-licensed source.

use crate::binary_tools::{Rom, fix_rom_checksum};

/// The logo the boot ROM scrolls down the screen and checks, at $0104 to $0133.
pub const NINTENDO_LOGO: [u8; 48] = [
    0xCE, 0xED, 0x66, 0x66, 0xCC, 0x0D, 0x00, 0x0B, 0x03, 0x73, 0x00, 0x83, 0x00, 0x0C, 0x00, 0x0D, 0x00, 0x08, 0x11, 0x1F, 0x88, 0x89, 0x00, 0x0E, 0xDC, 0xCC, 0x6E, 0xE6, 0xDD, 0xDD, 0xD9, 0x99,
    0xBB, 0xBB, 0x67, 0x63, 0x6E, 0x0E, 0xEC, 0xCC, 0xDD, 0xDC, 0x99, 0x9F, 0xBB, 0xB9, 0x33, 0x3E,
];

/// Where the program starts: the boot ROM jumps here when it's done.
pub const ENTRY: usize = 0x100;
const LOGO_AT: usize = 0x104;
const TITLE_AT: usize = 0x134;
const CGB_FLAG: usize = 0x143;
const TYPE_AT: usize = 0x147;
const ROM_SIZE_AT: usize = 0x148;
const HEADER_END: usize = 0x150;

/// What a cartridge's header says.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Header {
    pub title: String,
    /// Made for the Game Boy Color: $80 runs on both, $C0 on the Color alone.
    pub color: bool,
    pub kind: u8,
    /// The ROM's size, from its header.
    pub rom_size: usize,
    /// Whether the header checksum the boot ROM checks is right.
    pub checksum_right: bool,
}

/// Whether bytes are a Game Boy cartridge: they have the logo the boot ROM checks.
pub fn is_cartridge(bytes: &[u8]) -> bool {
    bytes.len() >= HEADER_END && bytes[LOGO_AT..LOGO_AT + 48] == NINTENDO_LOGO
}

/// A cartridge's header, if it is one.
pub fn header(bytes: &[u8]) -> Option<Header> {
    if !is_cartridge(bytes) {
        return None;
    }
    let color = bytes[CGB_FLAG] & 0x80 != 0;
    // Up to 16 characters, or 15 with the Color's flag after them; up to the first 0.
    let title_end = if color { CGB_FLAG } else { CGB_FLAG + 1 };
    let title: String = bytes[TITLE_AT..title_end].iter().take_while(|b| **b != 0).map(|b| if b.is_ascii_graphic() || *b == b' ' { char::from(*b) } else { '?' }).collect();
    let rom_size = 0x8000usize.checked_shl(u32::from(bytes[ROM_SIZE_AT])).unwrap_or(0);
    let (has, right) = crate::binary_tools::rom_checksum(bytes, Rom::GameBoy)?;
    let checksum_right = has.get(..2) == right.get(..2);
    Some(Header { title: title.trim_end().to_string(), color, kind: bytes[TYPE_AT], rom_size, checksum_right })
}

/// What kind of cartridge a header's type byte says, for describing one.
pub fn kind_name(kind: u8) -> &'static str {
    match kind {
        0x00 => "ROM only",
        0x01..=0x03 => "MBC1",
        0x05 | 0x06 => "MBC2",
        0x0F..=0x13 => "MBC3",
        0x19..=0x1E => "MBC5",
        _ => "another memory controller",
    }
}

/// The cartridge from assembled code: its bytes from address 0 (each ROM bank after the first further on, as
/// SECTION ROMX puts them), made up to a size a cartridge comes in, with its header made complete. Whatever the
/// source wrote in the header is kept, except the logo and the sizes and checksums, which have to be right: a title
/// left empty is the file's name, a jump to the program is put at $0100 if nothing's there, and a ROM bigger than
/// 32K is given an MBC5 to switch its banks, unless the source says which controller it has.
pub fn cartridge(assembly: &crate::asm::Assembly, stem: &str) -> Result<Vec<u8>, String> {
    let (Some(low), Some(high)) = (assembly.lowest(), assembly.highest()) else {
        return Err(crate::i18n::tr("Nothing was assembled.").into());
    };
    if low < 0 {
        return Err(crate::i18n::tr("A Game Boy cartridge starts at address 0.").into());
    }
    if high > 8 * 1024 * 1024 {
        return Err(crate::i18n::tf!("A Game Boy cartridge holds at most 8M, and this reaches ${high:X}."));
    }
    // ROM is $0000 to $7FFF (with the banks after the first further on in the file, though their code is for
    // $4000 to $7FFF); code or data for $8000 to $FFFF is for RAM or the hardware, which a cartridge can't fill.
    if let Some(placed) = assembly.lines.iter().find(|p| !p.bytes.is_empty() && (0x8000..0x10000).contains(&p.address)) {
        return Err(crate::i18n::tf!("${:04X} is in the Game Boy's RAM, not its cartridge: only ROM ($0000 to $7FFF) can hold code and data. Use SECTION \"name\", WRAM0 and DS for variables.", placed.address));
    }
    let size = (high as usize).max(0x8000).next_power_of_two();
    let mut rom = assembly.image(0, size as i64);
    if rom[ENTRY..ENTRY + 4].iter().all(|b| *b == 0) {
        // The boot ROM goes to $0100, which has room for a jump over the header.
        let start = assembly.entry.or_else(|| assembly.chunks.iter().filter(|c| !c.bytes.is_empty() && c.address >= HEADER_END as i64).map(|c| c.address).min());
        let Some(start) = start else {
            return Err(crate::i18n::tr("The program has to start at $0150 or after, past the cartridge's header, or at $0100 with a jump over it.").into());
        };
        rom[ENTRY..ENTRY + 4].copy_from_slice(&[0x00, 0xC3, start as u8, (start >> 8) as u8]);
    }
    rom[LOGO_AT..LOGO_AT + 48].copy_from_slice(&NINTENDO_LOGO);
    if rom[TITLE_AT..CGB_FLAG].iter().all(|b| *b == 0) {
        let title: Vec<u8> = stem.bytes().filter(|b| b.is_ascii_alphanumeric() || *b == b' ').map(|b| b.to_ascii_uppercase()).take(11).collect();
        rom[TITLE_AT..TITLE_AT + title.len()].copy_from_slice(&title);
    }
    if size > 0x8000 && rom[TYPE_AT] == 0 {
        rom[TYPE_AT] = 0x19;
    }
    rom[ROM_SIZE_AT] = (size / 0x8000).trailing_zeros() as u8;
    fix_rom_checksum(&mut rom, Rom::GameBoy);
    Ok(rom)
}

/// What of a cartridge to disassemble: its first 32K (bank 0, and bank 1 as it's first seen at $4000), from
/// address 0, followed from $0100 and from each interrupt's place that has code; and what was found, to say.
pub fn for_disassembly(bytes: &[u8]) -> Option<(Vec<u8>, u16, Vec<u16>, String)> {
    let header = header(bytes)?;
    let code = bytes[..bytes.len().min(0x8000)].to_vec();
    let mut starts = vec![ENTRY as u16];
    // VBlank, STAT, the timer, the serial port and the joypad.
    for vector in [0x40u16, 0x48, 0x50, 0x58, 0x60] {
        if !matches!(code[usize::from(vector)], 0x00 | 0xFF) {
            starts.push(vector);
        }
    }
    let kind = kind_name(header.kind);
    let size = header.rom_size / 1024;
    let more = if bytes.len() > 0x8000 { crate::i18n::tr(": its first 32K, banks 0 and 1").to_string() } else { String::new() };
    let found = crate::i18n::tf!("A Game Boy cartridge, \u{201C}{}\u{201D} ({kind}, {size}K){more}", header.title);
    Some((code, 0, starts, found))
}

/// The machine and file a cartridge is: for the Game Boy Color if its header says it is.
pub fn ending(rom: &[u8]) -> &'static str {
    if rom.get(CGB_FLAG).is_some_and(|f| f & 0x80 != 0) { "gbc" } else { "gb" }
}

/// The starter: waits for the screen's vertical blank to turn it off, puts a tile drawn in the source into video
/// memory, fills a row of the background with it, turns the screen on and scrolls it a pixel a frame.
pub const STARTER: &str = r#"; A Game Boy program, in RGBDS's syntax. F9 makes the cartridge (its header, logo and
; checksums put in) and runs it in an emulator: SameBoy, mGBA, Gambatte, BGB, Mesen or MAME.

; The hardware's registers, as hardware.inc names them.
DEF rP1   EQU $FF00
DEF rNR52 EQU $FF26             ; sound on or off
DEF rLCDC EQU $FF40             ; the screen's control
DEF rSCY  EQU $FF42             ; the background's scroll
DEF rSCX  EQU $FF43
DEF rLY   EQU $FF44             ; the line being drawn: 144 to 153 is the vertical blank
DEF rBGP  EQU $FF47             ; the background's four shades

SECTION "Header", ROM0[$100]
        nop
        jp Start
        ds $150 - @, 0          ; the header: filled in when the cartridge is made

SECTION "Main", ROM0
Start:
        di
        xor a
        ldh [rNR52], a          ; sound off: this doesn't use it

        ; The screen may only be turned off in the vertical blank, or the hardware can be harmed.
.wait
        ldh a, [rLY]
        cp 144
        jr c, .wait
        xor a
        ldh [rLCDC], a          ; screen off, so video memory can be written at any time

        ; Tile 1 (at $8010) is the smiley below; tile 0 is left blank.
        ld hl, $8000
        ld bc, 16
.blank
        xor a
        ld [hl+], a
        dec bc
        ld a, b
        or c
        jr nz, .blank
        ld de, Smiley
        ld bc, SmileyEnd - Smiley
.copy
        ld a, [de]
        ld [hl+], a
        inc de
        dec bc
        ld a, b
        or c
        jr nz, .copy

        ; The background's map, 32 by 32 tiles at $9800, all blank...
        ld hl, $9800
        ld bc, 32 * 32
.clear
        xor a
        ld [hl+], a
        dec bc
        ld a, b
        or c
        jr nz, .clear
        ; ...but for a row of smileys across the middle.
        ld hl, $9800 + 8 * 32
        ld b, 32
        ld a, 1
.row
        ld [hl+], a
        dec b
        jr nz, .row

        ld a, %11100100         ; shades 3, 2, 1, 0: colour 0 white, colour 3 black
        ldh [rBGP], a
        xor a
        ldh [rSCX], a
        ldh [rSCY], a
        ld a, %10010001         ; screen on, the background on, its tiles from $8000
        ldh [rLCDC], a

Main:
        ; Once a frame, at the start of the vertical blank, scroll the background a pixel.
.vblank
        ldh a, [rLY]
        cp 144
        jr nz, .vblank
        ldh a, [rSCX]
        inc a
        ldh [rSCX], a
.leave
        ldh a, [rLY]
        cp 144
        jr z, .leave
        jr Main

; Eight rows of eight pixels, each 0 to 3.
Smiley:
        dw `00333300
        dw `03111130
        dw `31311313
        dw `31111113
        dw `31311313
        dw `31133113
        dw `03111130
        dw `00333300
SmileyEnd:
"#;

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    fn assemble(source: &str) -> crate::asm::Assembly {
        let assembly = crate::asm::assemble(&crate::asm::sm83::Sm83, Path::new("gb.asm"), source, &|_| Err("no".into()));
        assert!(assembly.ok(), "{:?}", assembly.problems);
        assembly
    }

    #[test]
    fn the_starter_becomes_a_cartridge_the_boot_rom_takes() {
        let rom = cartridge(&assemble(STARTER), "hello world").unwrap();
        assert_eq!(rom.len(), 0x8000);
        assert!(is_cartridge(&rom));
        let header = header(&rom).unwrap();
        assert_eq!(header, Header { title: "HELLO WORLD".into(), color: false, kind: 0, rom_size: 0x8000, checksum_right: true });
        // The jump the source put at $0100 is kept: NOP, JP Start, which is right after the header.
        assert_eq!(&rom[0x100..0x104], &[0x00, 0xC3, 0x50, 0x01]);
        // Both checksums are right, as the Binary tools window would say.
        let (has, right) = crate::binary_tools::rom_checksum(&rom, Rom::GameBoy).unwrap();
        assert_eq!(has, right);
        assert_eq!(crate::running::machine_for(crate::running::Format::Gb, &rom), Some(crate::running::Machine::GameBoy));
        assert_eq!(ending(&rom), "gb");
    }

    #[test]
    fn a_program_with_no_header_of_its_own_is_given_one() {
        // Code at $0150 and on, with nothing at $0100: a jump to it is put there.
        let rom = cartridge(&assemble("  org $150\nstart: jr start\n"), "t").unwrap();
        assert_eq!(&rom[0x100..0x104], &[0x00, 0xC3, 0x50, 0x01]);
        assert_eq!(&rom[0x150..0x152], &[0x18, 0xFE]);
        assert!(header(&rom).unwrap().checksum_right);
        // A program in two ROM banks is 64K, with an MBC5 to switch them, and says how big it is.
        let banked = "SECTION \"a\", ROM0[$150]\nstart: jr start\nSECTION \"b\", ROMX[$4000], BANK[3]\n db 7\n";
        let rom = cartridge(&assemble(banked), "t").unwrap();
        assert_eq!((rom.len(), rom[0x147], rom[0x148], rom[0xC000]), (0x10000, 0x19, 1, 7));
        // A Color title is written by the source with its flag after it.
        let color = "SECTION \"h\", ROM0[$134]\n db \"COLOUR\"\n ds $143 - @, 0\n db $C0\nSECTION \"m\", ROM0[$150]\nstart: jr start\n";
        let rom = cartridge(&assemble(color), "t").unwrap();
        let header = header(&rom).unwrap();
        assert_eq!((header.title.as_str(), header.color, ending(&rom)), ("COLOUR", true, "gbc"));
    }

    #[test]
    fn what_cant_be_a_cartridge_is_said() {
        let wram = cartridge(&assemble("  org $C000\n  nop\n"), "t").unwrap_err();
        assert!(wram.contains("RAM"), "{wram}");
        let none = cartridge(&assemble("  org $100\n  db 0,0,0,0\n"), "t").unwrap_err();
        assert!(none.contains("$0150"), "{none}");
    }

    #[test]
    fn a_cartridge_disassembles_into_source_that_makes_it_again() {
        let rom = cartridge(&assemble(STARTER), "hello").unwrap();
        let (code, origin, starts, found) = for_disassembly(&rom).unwrap();
        let cpu = crate::assemblers::Cpu::Sm83;
        assert_eq!((origin, starts.as_slice()), (0, &[0x100][..]));
        assert!(found.contains("HELLO"), "{found}");
        let source = crate::disasm::disassemble_checked_for(cpu, &code, origin, &starts, false);
        assert!(source.contains("DEF rLY EQU $FF44") && source.contains("ldh a,[rLY]"), "{source}");
        assert!(source.contains("jp L0150") && source.contains("jr c,L0154"), "{source}");
        let again = crate::asm::assemble(&crate::asm::sm83::Sm83, Path::new("d.asm"), &source, &|_| Err("no".into()));
        assert!(again.ok(), "{:?}", again.problems);
        assert_eq!(again.image(0, rom.len() as i64), rom);
    }

    /// Writes the starter's cartridge to look at in an emulator: LIMONITER_CARTS=folder cargo test -- --ignored
    /// gameboy::tests::the_starter_is_written_out
    #[test]
    #[ignore]
    fn the_starter_is_written_out() {
        let folder = std::path::PathBuf::from(std::env::var("LIMONITER_CARTS").expect("LIMONITER_CARTS names a folder"));
        std::fs::write(folder.join("starter.gb"), cartridge(&assemble(STARTER), "starter").unwrap()).unwrap();
    }

    #[test]
    fn mame_is_told_its_a_game_boy() {
        let mame = crate::emulators::by_id("mame").unwrap();
        assert!(mame.runs(crate::running::Machine::GameBoy, crate::running::Format::Gb));
        let args = mame.arguments(crate::running::Machine::GameBoy, crate::running::Format::Gb, Path::new("/tmp/g.gb"), None, "");
        assert_eq!(args, ["gameboy", "-cart", "/tmp/g.gb", "-skip_gameinfo"]);
    }
}
