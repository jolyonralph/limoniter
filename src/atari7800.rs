//! The Atari 7800: its cartridges' A78 headers, read and written, a cartridge made from assembled 6502 code, and a
//! starter program.
//!
//! An .a78 file is a 128-byte header, then the cartridge's ROM as the 6502 sees it, ending at $FFFF. The header is
//! laid out as 8BitDev.org's A78 Header Specification has it (version 3; version 4 only adds fields for the newer
//! mappers): its version at 0, ATARI7800 at 1 (sixteen bytes, padded with zeros), the title at 17 (32 bytes), the
//! ROM's size without the header at 49 (four bytes, high first), the cartridge's kind at 53 (two bytes of flags,
//! high first: 0 is plain ROM), the two controllers at 55 and 56 (1 is a 7800 joystick), the TV at 57 (bit 0 set
//! for PAL), and "ACTUAL CART DATA STARTS HERE" at 100.
//!
//! A real 7800 sold in America checks a cartridge's digital signature, at $FF80 to $FFF7, before it runs it in 7800
//! mode; A7800 needs no BIOS and runs any cartridge, as the European 7800 (and MAME's a7800p) does. Limoniter
//! doesn't sign cartridges: 7800sign, from the 7800 development tools, does.

use crate::asm::Assembly;

/// What an A78 header says.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Header {
    pub version: u8,
    pub title: String,
    /// The ROM after the header, in bytes.
    pub rom_size: u32,
    /// The cartridge's hardware, as flags (POKEY, bank switching, RAM); 0 for plain ROM.
    pub cart_type: u16,
    pub pal: bool,
}

const MAGIC: &[u8] = b"ATARI7800";
const END_MAGIC: &[u8; 28] = b"ACTUAL CART DATA STARTS HERE";

/// The sizes a cartridge with no bank switching can be: up to 48K, from $4000 to $FFFF.
const PLAIN_SIZES: [usize; 6] = [0x1000, 0x2000, 0x4000, 0x8000, 0xA000, 0xC000];

impl Header {
    /// The header of an .a78 file, if it starts with one.
    pub fn read(bytes: &[u8]) -> Option<Header> {
        if bytes.len() < 128 || &bytes[1..1 + MAGIC.len()] != MAGIC {
            return None;
        }
        let title = String::from_utf8_lossy(&bytes[17..49]).trim_end_matches(['\0', ' ']).to_string();
        Some(Header {
            version: bytes[0],
            title,
            rom_size: u32::from_be_bytes([bytes[49], bytes[50], bytes[51], bytes[52]]),
            cart_type: u16::from_be_bytes([bytes[53], bytes[54]]),
            pal: bytes[57] & 1 == 1,
        })
    }

    /// The 128 bytes, as version 3 lays them out, with a 7800 joystick in each port.
    pub fn write(&self) -> [u8; 128] {
        let mut out = [0u8; 128];
        out[0] = self.version;
        out[1..1 + MAGIC.len()].copy_from_slice(MAGIC);
        let title: Vec<u8> = self.title.bytes().filter(|b| (0x20..0x7F).contains(b)).take(32).collect();
        out[17..17 + title.len()].copy_from_slice(&title);
        out[49..53].copy_from_slice(&self.rom_size.to_be_bytes());
        out[53..55].copy_from_slice(&self.cart_type.to_be_bytes());
        out[55] = 1;
        out[56] = 1;
        out[57] = u8::from(self.pal);
        out[100..128].copy_from_slice(END_MAGIC);
        out
    }
}

/// Whether the source says the cartridge is for a European 7800: a `; tv: pal` line near its top.
fn says_pal(source: Option<&str>) -> bool {
    source.unwrap_or_default().lines().take(50).any(|line| {
        let comment = line.trim_start().trim_start_matches([';', '/', '*', ' ', '\t']).to_ascii_lowercase().replace(' ', "");
        comment.strip_prefix("tv:").is_some_and(|v| v == "pal")
    })
}

/// An .a78 file from assembled code: the header, then the bytes from the lowest address assembled to $FFFF, which a
/// plain cartridge (4K to 48K, with no bank switching) fills.
pub fn cartridge(assembly: &Assembly, title: &str, source: Option<&str>) -> Result<Vec<u8>, String> {
    let (Some(low), Some(high)) = (assembly.lowest(), assembly.highest()) else {
        return Err(crate::i18n::tr("Nothing was assembled.").into());
    };
    if high != 0x10000 {
        return Err(crate::i18n::tr("An Atari 7800 cartridge ends at $FFFF, with the vectors: end the source with ORG $FFFA and .word nmi, reset, irq.").into());
    }
    let rom = assembly.image(low, high);
    if !PLAIN_SIZES.contains(&rom.len()) {
        return Err(crate::i18n::tf!("A 7800 cartridge with no bank switching starts at $F000, $E000, $C000, $8000, $6000 or $4000, but this starts at ${low:04X}."));
    }
    let header = Header { version: 3, title: title.to_string(), rom_size: rom.len() as u32, cart_type: 0, pal: says_pal(source) };
    let mut file = header.write().to_vec();
    file.extend(rom);
    Ok(file)
}

/// The MAME system for a cartridge: the European 7800 for one whose header says PAL, else the American one.
pub fn mame_system(file: &[u8]) -> &'static str {
    if Header::read(file).is_some_and(|h| h.pal) { "a7800p" } else { "a7800" }
}

/// File, New as, Atari 7800 (6502): colour bars, a new colour each line, rolling up the screen.
pub const STARTER: &str = r#"; machine: 7800
; An Atari 7800 cartridge. F9 makes the .a78 file (its A78 header says 16K of plain ROM, NTSC, a joystick in
; each port) and runs it in an emulator: A7800, or MAME. MARIA draws nothing but the background here: each
; line WSYNC waits for, the program gives the background the next colour.
; An American 7800's BIOS runs only cartridges signed with Atari's key: A7800, which needs no BIOS, runs it
; as it is; for MAME's a7800 (or a real 7800), sign it with 7800sign. Put ; tv: pal near the top for a European
; 7800, which checks no signature.

INPTCTRL = $01                  ; locks the 7800 into its own mode
BACKGRND = $20                  ; MARIA's background colour
WSYNC    = $24                  ; writing it waits for the end of the line
MSTAT    = $28                  ; bit 7 is set during the vertical blank
DPPH     = $2C                  ; where the display list list is
DPPL     = $30
CTRL     = $3C                  ; MARIA's DMA and display mode

colour   = $40                  ; in zero page: the first line's colour this frame

        org $C000
reset:
        sei
        cld
        lda #$07
        sta INPTCTRL            ; 7800 mode, locked, the BIOS switched out
        lda #$7F
        sta CTRL                ; no DMA while it's set up
        ldx #$FF
        txs
        lda #>dll
        sta DPPH
        lda #<dll
        sta DPPL
vblank: bit MSTAT
        bpl vblank              ; MARIA is told where the list is in a vertical blank
        lda #$40
        sta CTRL                ; DMA on, 160 pixels across

frame:  inc colour              ; each frame starts a colour further on
        ldx colour
picture:
        bit MSTAT
        bmi picture             ; wait for the picture to start
line:   stx BACKGRND
        sta WSYNC
        inx
        bit MSTAT
        bpl line                ; until the vertical blank
        jmp frame

nmi:
irq:    rti

; The display list list: twenty zones of sixteen lines, enough for PAL's picture too, each with a display
; list that's empty (a header whose second byte is 0 ends one).
dll:    .byte $0F, >empty, <empty, $0F, >empty, <empty, $0F, >empty, <empty, $0F, >empty, <empty
        .byte $0F, >empty, <empty, $0F, >empty, <empty, $0F, >empty, <empty, $0F, >empty, <empty
        .byte $0F, >empty, <empty, $0F, >empty, <empty, $0F, >empty, <empty, $0F, >empty, <empty
        .byte $0F, >empty, <empty, $0F, >empty, <empty, $0F, >empty, <empty, $0F, >empty, <empty
        .byte $0F, >empty, <empty, $0F, >empty, <empty, $0F, >empty, <empty, $0F, >empty, <empty
empty:  .byte 0, 0

; $FFF8 and $FFF9 tell the BIOS what the cartridge is: $FF, then where its ROM starts ($C000) and 7 for a 7800
; cartridge.
        org $FFF8
        .byte $FF, $C7
        .word nmi, reset, irq
"#;

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    fn assemble(source: &str) -> Assembly {
        let assembly = crate::asm::assemble(&crate::asm::m6502::M6502, Path::new("a7800.asm"), source, &|_| Err("no".into()));
        assert!(assembly.ok(), "{:?}", assembly.problems);
        assembly
    }

    #[test]
    fn the_starter_becomes_an_a78_file() {
        let assembly = assemble(STARTER);
        let file = cartridge(&assembly, "hello", Some(STARTER)).unwrap();
        assert_eq!(file.len(), 128 + 0x4000);
        // The header, byte by byte, as the specification's table lays it out.
        assert_eq!(file[0], 3, "version 3");
        assert_eq!(&file[1..17], b"ATARI7800\0\0\0\0\0\0\0");
        assert_eq!(&file[17..22], b"hello");
        assert!(file[22..49].iter().all(|b| *b == 0));
        assert_eq!(&file[49..53], &[0, 0, 0x40, 0], "16K, high byte first");
        assert_eq!(&file[53..55], &[0, 0], "plain ROM");
        assert_eq!((file[55], file[56], file[57]), (1, 1, 0), "joysticks, NTSC");
        assert_eq!(&file[100..128], b"ACTUAL CART DATA STARTS HERE");
        assert_eq!(Header::read(&file), Some(Header { version: 3, title: "hello".into(), rom_size: 0x4000, cart_type: 0, pal: false }));
        // The cartridge's last bytes: $FF, $C7, then the vectors, reset at $C000.
        let rom = &file[128..];
        assert_eq!(&rom[0x3FF8..0x3FFA], &[0xFF, 0xC7]);
        assert_eq!(&rom[0x3FFC..0x3FFE], &[0x00, 0xC0]);
        assert_eq!(&rom[..3], &[0x78, 0xD8, 0xA9], "SEI, CLD, LDA #");
        assert_eq!(crate::running::Runnable::from_file("hello.a78", file.clone()).map(|r| r.machine), Some(crate::running::Machine::Atari7800));
        assert_eq!(mame_system(&file), "a7800");
        // Said to be for a European 7800, the header says PAL.
        let pal = format!("; tv: PAL\n{STARTER}");
        let file = cartridge(&assemble(&pal), "hello", Some(&pal)).unwrap();
        assert!(Header::read(&file).unwrap().pal);
        assert_eq!(mame_system(&file), "a7800p");
    }

    #[test]
    fn a_cartridge_needs_its_vectors_and_a_size_it_can_be() {
        let no_vectors = assemble(" org $C000\nreset: jmp reset\n");
        assert!(cartridge(&no_vectors, "x", None).is_err());
        let odd = assemble(" org $D000\nreset: jmp reset\n org $FFFA\n .word reset,reset,reset\n");
        assert!(cartridge(&odd, "x", None).unwrap_err().contains("$D000"));
        let small = assemble(" org $F000\nreset: jmp reset\n org $FFFA\n .word reset,reset,reset\n");
        assert_eq!(cartridge(&small, "x", None).unwrap().len(), 128 + 0x1000);
        assert!(Header::read(b"not a cartridge").is_none());
    }
}
