//! The NES (the Famicom): its cartridges' iNES and NES 2.0 headers, read and written, a cartridge made from
//! assembled 6502 code, and a starter program.
//!
//! An .nes file is a 16-byte header, then the program's ROM (PRG, in 16K pieces, seen by the 6502 at $8000 to
//! $FFFF), then the tiles' ROM (CHR, in 8K pieces, seen by the picture chip at $0000 to $1FFF), or none, when the
//! cartridge has RAM for its tiles that the program fills. The header says how big each is, which mapper (the
//! cartridge's own hardware for switching banks) it has, and how the background's name tables are mirrored.
//! NES 2.0 is iNES with byte 7's bits 2 and 3 set to 10 and more said in bytes 8 to 15; anything that reads iNES
//! reads it.

/// How the two name tables the NES has room for appear in its four.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mirroring {
    /// Side by side ($2000 and $2400 differ): for scrolling sideways.
    Vertical,
    /// One above the other ($2000 and $2800 differ): for scrolling up and down.
    Horizontal,
    /// The cartridge has RAM for all four.
    FourScreen,
}

/// What an iNES or NES 2.0 header says.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Header {
    pub nes2: bool,
    /// The program's ROM and the tiles' ROM, in bytes.
    pub prg: usize,
    pub chr: usize,
    pub mapper: u16,
    pub submapper: u8,
    pub mirroring: Mirroring,
    /// Battery-backed RAM, to keep saved games.
    pub battery: bool,
    /// 512 bytes for $7000 before the program's ROM.
    pub trainer: bool,
    /// RAM for tiles, in bytes, when there's no ROM for them (NES 2.0 says how much; iNES means 8K).
    pub chr_ram: usize,
}

const MAGIC: &[u8; 4] = b"NES\x1A";

impl Header {
    /// The header of an .nes file, if it is one whose ROMs are all there.
    pub fn read(bytes: &[u8]) -> Option<Header> {
        if bytes.len() < 16 || &bytes[..4] != MAGIC {
            return None;
        }
        let nes2 = bytes[7] & 0x0C == 0x08;
        // NES 2.0's sizes have a high nibble each in byte 9; $F there means an exponent and a multiplier.
        let size = |low: u8, high: u8, unit: usize| -> usize {
            if !nes2 || high < 0x0F {
                let high = if nes2 { usize::from(high) } else { 0 };
                return ((high << 8) | usize::from(low)) * unit;
            }
            let exponent = u32::from(low >> 2);
            let multiplier = usize::from(low & 3) * 2 + 1;
            1usize.checked_shl(exponent).unwrap_or(0) * multiplier
        };
        let prg = size(bytes[4], bytes[9] & 0x0F, 0x4000);
        let chr = size(bytes[5], bytes[9] >> 4, 0x2000);
        // An old header with junk in its last bytes (DiskDude!) says nothing in byte 7.
        let junk = !nes2 && bytes[12..16].iter().any(|b| *b != 0);
        let mut mapper = u16::from(bytes[6] >> 4) | if junk { 0 } else { u16::from(bytes[7] & 0xF0) };
        let mut submapper = 0;
        if nes2 {
            mapper |= u16::from(bytes[8] & 0x0F) << 8;
            submapper = bytes[8] >> 4;
        }
        let mirroring = match (bytes[6] & 8 != 0, bytes[6] & 1 != 0) {
            (true, _) => Mirroring::FourScreen,
            (false, true) => Mirroring::Vertical,
            (false, false) => Mirroring::Horizontal,
        };
        let trainer = bytes[6] & 4 != 0;
        let chr_ram = match (nes2, chr) {
            (true, _) if bytes[11] & 0x0F != 0 => 64 << (bytes[11] & 0x0F),
            (false, 0) => 0x2000,
            _ => 0,
        };
        let header = Header { nes2, prg, chr, mapper, submapper, mirroring, battery: bytes[6] & 2 != 0, trainer, chr_ram };
        (prg > 0 && bytes.len() >= 16 + if trainer { 512 } else { 0 } + prg + chr).then_some(header)
    }

    /// The header as NES 2.0 writes it.
    pub fn write(&self) -> [u8; 16] {
        let mut h = [0u8; 16];
        h[..4].copy_from_slice(MAGIC);
        let (prg, chr) = (self.prg / 0x4000, self.chr / 0x2000);
        h[4] = prg as u8;
        h[5] = chr as u8;
        let mirroring = match self.mirroring {
            Mirroring::Vertical => 1,
            Mirroring::Horizontal => 0,
            Mirroring::FourScreen => 8,
        };
        h[6] = ((self.mapper as u8 & 0x0F) << 4) | mirroring | if self.battery { 2 } else { 0 } | if self.trainer { 4 } else { 0 };
        h[7] = (self.mapper as u8 & 0xF0) | 0x08;
        h[8] = (self.submapper << 4) | ((self.mapper >> 8) as u8 & 0x0F);
        h[9] = (((chr >> 8) as u8 & 0x0F) << 4) | ((prg >> 8) as u8 & 0x0F);
        // RAM for tiles: 64 shifted left by the number.
        if self.chr_ram > 0 {
            h[11] = (self.chr_ram / 64).trailing_zeros() as u8;
        }
        // The usual controllers plugged in.
        h[15] = 1;
        h
    }

    /// A sentence about it, for the disassembly box and the like.
    pub fn describe(&self) -> String {
        let tiles = if self.chr > 0 { crate::i18n::tf!("{}K of tiles", self.chr / 1024) } else { crate::i18n::tf!("{}K of RAM for tiles", self.chr_ram / 1024) };
        let kind = if self.nes2 { "NES 2.0" } else { "iNES" };
        crate::i18n::tf!("An NES cartridge ({kind}): mapper {}, {}K of program, {tiles}", self.mapper, self.prg / 1024)
    }
}

/// What of a cartridge to disassemble: the last 16K of its program, at $C000 (where mapper 0, and most others,
/// put it), followed from its reset vector and NMI; and what was found, to say.
pub fn for_disassembly(bytes: &[u8]) -> Option<(Vec<u8>, u16, Vec<u16>, String)> {
    let header = Header::read(bytes)?;
    let start = 16 + if header.trainer { 512 } else { 0 };
    let prg = &bytes[start..start + header.prg];
    let (code, origin) = if prg.len() >= 0x8000 && header.mapper == 0 { (prg[prg.len() - 0x8000..].to_vec(), 0x8000u16) } else { (prg[prg.len() - 0x4000..].to_vec(), 0xC000) };
    let vector = |at: usize| u16::from_le_bytes([code[code.len() - 6 + at], code[code.len() - 5 + at]]);
    let starts: Vec<u16> = [vector(2), vector(0), vector(4)].into_iter().filter(|a| *a >= origin).fold(Vec::new(), |mut list, a| {
        if !list.contains(&a) {
            list.push(a);
        }
        list
    });
    Some((code, origin, starts, header.describe()))
}

/// What the source says of its cartridge near its top, as `; mirroring: horizontal` and `; mapper: 0` (and the
/// same after // or *).
fn said<'a>(lines: impl IntoIterator<Item = &'a str>, key: &str) -> Option<String> {
    lines.into_iter().take(50).find_map(|line| {
        let line = line.trim_start();
        let comment = line.strip_prefix("//").or_else(|| line.strip_prefix(';')).or_else(|| line.strip_prefix('*'))?;
        let comment = comment.trim_start_matches([';', '/', '*', ' ', '\t']).to_ascii_lowercase();
        Some(comment.strip_prefix(key)?.trim_start().strip_prefix(':')?.trim().to_string())
    })
}

/// The .nes file from assembled 6502 code. Source that writes its own header (NES and $1A at address 0, the ROMs
/// after it, as ASM6 and ACME programs do) is taken as it is. Otherwise the program's ROM is what's at $8000 to
/// $FFFF (16K if it all starts at $C000 or after), ending with its vectors at $FFFA; anything at $10000 and on is
/// the tiles' ROM, and with none the cartridge has 8K of RAM for them. The header is NES 2.0's, for mapper 0
/// unless the source says another, mirrored as it says (vertically if it doesn't).
pub fn cartridge(assembly: &crate::asm::Assembly, source: Option<&str>) -> Result<Vec<u8>, String> {
    let (Some(low), Some(high)) = (assembly.lowest(), assembly.highest()) else {
        return Err(crate::i18n::tr("Nothing was assembled.").into());
    };
    if low == 0 {
        let image = assembly.image(0, high);
        if image.starts_with(MAGIC) {
            return match Header::read(&image) {
                Some(_) => Ok(image),
                None => Err(crate::i18n::tr("The source writes an iNES header, but the ROMs it says there are aren't all there after it.").into()),
            };
        }
    }
    if let Some(placed) = assembly.lines.iter().find(|p| !p.bytes.is_empty() && p.address < 0x8000) {
        return Err(crate::i18n::tf!("${:04X} isn't in the cartridge: an NES program's ROM is $8000 to $FFFF (its RAM, $0000 to $07FF, is for variables, given addresses with =).", placed.address));
    }
    let prg_end = assembly.chunks.iter().filter(|c| !c.bytes.is_empty() && c.address < 0x10000).map(|c| (c.address + c.bytes.len() as i64).min(0x10000)).max().unwrap_or(0);
    if prg_end != 0x10000 {
        return Err(crate::i18n::tr("An NES program ends with its three vectors at $FFFA: where NMI, reset and IRQ go (as .word nmi, reset, irq).").into());
    }
    let prg_start = if low >= 0xC000 { 0xC000 } else { 0x8000 };
    let prg = assembly.image(prg_start, 0x10000);
    let chr = if high > 0x10000 {
        let length = ((high - 0x10000) as usize).div_ceil(0x2000) * 0x2000;
        assembly.image(0x10000, 0x10000 + length as i64)
    } else {
        Vec::new()
    };
    let lines: Vec<&str> = source.map(|s| s.lines().collect()).unwrap_or_default();
    let mirroring = match said(lines.iter().copied(), "mirroring").as_deref() {
        Some("horizontal") => Mirroring::Horizontal,
        Some("four" | "four screen" | "four-screen" | "4") => Mirroring::FourScreen,
        _ => Mirroring::Vertical,
    };
    let mapper = said(lines.iter().copied(), "mapper").and_then(|m| m.parse::<u16>().ok()).unwrap_or(0);
    let header = Header { nes2: true, prg: prg.len(), chr: chr.len(), mapper, submapper: 0, mirroring, battery: false, trainer: false, chr_ram: if chr.is_empty() { 0x2000 } else { 0 } };
    let mut file = header.write().to_vec();
    file.extend(prg);
    file.extend(chr);
    Ok(file)
}

/// Whether 6502 code is an NES program: it ends with the vectors at $FFFA and works the picture chip at $2000 to
/// $2007 (or its sprites' DMA at $4014).
pub fn looks_like(assembly: &crate::asm::Assembly) -> bool {
    let ends_with_vectors = assembly.chunks.iter().filter(|c| !c.bytes.is_empty() && c.address < 0x10000).map(|c| (c.address + c.bytes.len() as i64).min(0x10000)).max() == Some(0x10000);
    // STA, STX, STY, LDA and BIT on an absolute address.
    let ppu = assembly.lines.iter().any(|p| match p.bytes[..] {
        [0x8D | 0x8E | 0x8C | 0xAD | 0x2C, low, high] => matches!(u16::from_le_bytes([low, high]), 0x2000..=0x2007 | 0x4014),
        _ => false,
    });
    ends_with_vectors && ppu
}

/// The starter: sets the NES up as its hardware needs (two vertical blanks' wait for the picture chip), writes a
/// palette, a tile into the cartridge's RAM for tiles and a row of it across the background, turns the picture on
/// and scrolls it a pixel a frame from the NMI.
pub const STARTER: &str = r#"; machine: NES
; mirroring: horizontal
; An NES program. F9 makes the .nes file (an NES 2.0 header: mapper 0, 16K of program and 8K of RAM for the
; tiles, which the program fills) and runs it in an emulator: Mesen, FCEUX, Nestopia or MAME. Mirrored
; horizontally, the name table at $2400 is the one at $2000 again, so the row scrolls round without a gap.

; The picture chip's registers, and two of the sound chip's.
PPUCTRL   = $2000
PPUMASK   = $2001
PPUSTATUS = $2002
PPUSCROLL = $2005
PPUADDR   = $2006
PPUDATA   = $2007
DMC_FREQ  = $4010
APU_FRAME = $4017

scroll    = $00                 ; in zero page: how far the background has scrolled

        org $C000
reset:
        sei                     ; no interrupts while it's set up
        cld
        ldx #$40
        stx APU_FRAME           ; no interrupts from the sound's frame counter
        ldx #$FF
        txs
        inx                     ; X = 0
        stx PPUCTRL             ; no NMI yet
        stx PPUMASK             ; the picture off
        stx DMC_FREQ            ; no interrupts from samples

        ; The picture chip needs two vertical blanks to warm up after the NES is switched on.
        bit PPUSTATUS
vblank1:
        bit PPUSTATUS
        bpl vblank1
        txa
clear:
        sta $0000,x             ; the 2K of RAM cleared
        sta $0100,x
        sta $0200,x
        sta $0300,x
        sta $0400,x
        sta $0500,x
        sta $0600,x
        sta $0700,x
        inx
        bne clear
vblank2:
        bit PPUSTATUS
        bpl vblank2

        ; The palette, at $3F00 in the picture chip's memory: four colours for each of eight palettes.
        lda #$3F
        sta PPUADDR
        lda #$00
        sta PPUADDR
        ldx #0
palette_loop:
        lda palette,x
        sta PPUDATA
        inx
        cpx #32
        bne palette_loop

        ; Tile 0 blank and tile 1 the smiley, into the RAM for tiles at $0000.
        lda #$00
        sta PPUADDR
        sta PPUADDR
        ldx #0
tile_loop:
        lda tiles,x
        sta PPUDATA
        inx
        cpx #32
        bne tile_loop

        ; The background's name table at $2000, and its colours after it: all tile 0...
        lda #$20
        sta PPUADDR
        lda #$00
        sta PPUADDR
        ldy #4                  ; four times 256 bytes
        ldx #0
names:
        sta PPUDATA
        inx
        bne names
        dey
        bne names
        ; ...but for a row of smileys across the middle, row 14.
        lda #$21
        sta PPUADDR
        lda #$C0
        sta PPUADDR
        ldx #32
        lda #1
row:
        sta PPUDATA
        dex
        bne row

        lda #0
        sta PPUSCROLL
        sta PPUSCROLL
        lda #%10000000          ; an NMI at each vertical blank; tiles from $0000
        sta PPUCTRL
        lda #%00001010          ; the background on, in the leftmost column too
        sta PPUMASK
forever:
        jmp forever

; At each vertical blank: the background scrolled a pixel further.
nmi:
        pha
        inc scroll
        lda scroll
        sta PPUSCROLL
        lda #0
        sta PPUSCROLL
        lda #%10000000
        sta PPUCTRL
        pla
        rti

irq:
        rti

palette:
        .byte $11,$28,$30,$0F   ; blue behind, a yellow face, white, black
        .byte $11,$28,$30,$0F
        .byte $11,$28,$30,$0F
        .byte $11,$28,$30,$0F
        .byte $11,$28,$30,$0F   ; the sprites', not used
        .byte $11,$28,$30,$0F
        .byte $11,$28,$30,$0F
        .byte $11,$28,$30,$0F

; Each tile is 16 bytes: the low bit of each pixel's colour, eight rows, then the high bit.
tiles:
        .byte 0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0
        .byte $3C,$7E,$FF,$FF,$FF,$FF,$7E,$3C
        .byte $3C,$42,$A5,$81,$A5,$99,$42,$3C

        org $FFFA
        .word nmi, reset, irq
"#;

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    fn assemble(source: &str) -> crate::asm::Assembly {
        let assembly = crate::asm::assemble(&crate::asm::m6502::M6502, Path::new("nes.asm"), source, &|_| Err("no".into()));
        assert!(assembly.ok(), "{:?}", assembly.problems);
        assembly
    }

    #[test]
    fn the_starter_becomes_an_nes_2_file() {
        let assembly = assemble(STARTER);
        assert!(looks_like(&assembly));
        assert_eq!(crate::asm::output::machine_6502(&assembly), Some(crate::asm::output::Micro6502::Nes));
        assert_eq!(crate::asm::output::Micro6502::said_in(STARTER.lines()), Some(crate::asm::output::Micro6502::Nes));
        let file = cartridge(&assembly, Some(STARTER)).unwrap();
        assert_eq!(file.len(), 16 + 0x4000);
        assert_eq!(&file[..16], &[b'N', b'E', b'S', 0x1A, 1, 0, 0x00, 0x08, 0, 0, 0, 7, 0, 0, 0, 1]);
        let header = Header::read(&file).unwrap();
        assert_eq!(header, Header { nes2: true, prg: 0x4000, chr: 0, mapper: 0, submapper: 0, mirroring: Mirroring::Horizontal, battery: false, trainer: false, chr_ram: 0x2000 });
        // The reset vector, the last bytes but two, points at reset: $C000.
        assert_eq!(&file[file.len() - 4..file.len() - 2], &[0x00, 0xC0]);
        assert_eq!(crate::running::machine_for(crate::running::Format::Nes, &file), Some(crate::running::Machine::Nes));
    }

    #[test]
    fn headers_are_read_both_ways() {
        // An old iNES header: mapper 4 (MMC3), 128K of program, 128K of tiles, a battery, horizontal.
        let mut ines = vec![b'N', b'E', b'S', 0x1A, 8, 16, 0x42, 0x00, 0, 0, 0, 0, 0, 0, 0, 0];
        ines.resize(16 + 8 * 0x4000 + 16 * 0x2000, 0);
        let header = Header::read(&ines).unwrap();
        assert_eq!((header.nes2, header.mapper, header.prg, header.chr, header.mirroring, header.battery), (false, 4, 0x20000, 0x20000, Mirroring::Horizontal, true));
        // DiskDude! in the last bytes: byte 7 is junk too.
        ines[7] = b'D';
        ines[12..16].copy_from_slice(b"Dude");
        assert_eq!(Header::read(&ines).unwrap().mapper, 4);
        // Cut short, it isn't one.
        assert!(Header::read(&ines[..1000]).is_none());
        // Written as NES 2.0 and read back: a mapper over 255, and the sizes' high nibbles.
        let big = Header { nes2: true, prg: 0x4000 * 300, chr: 0x2000 * 2, mapper: 0x123, submapper: 2, mirroring: Mirroring::FourScreen, battery: false, trainer: false, chr_ram: 0 };
        let mut file = big.write().to_vec();
        file.resize(16 + big.prg + big.chr, 0);
        assert_eq!(Header::read(&file).unwrap(), big);
        assert!(big.describe().contains("mapper 291"), "{}", big.describe());
    }

    #[test]
    fn source_that_writes_its_own_header_is_kept_and_tiles_go_after() {
        // As ASM6 and ACME write one: the header at 0, the program put there with its own addresses.
        let own = "  org 0\n  .byte \"NES\",$1A,1,0,1,0,0,0,0,0,0,0,0,0\n  phase $C000\nreset: jmp reset\n  .fill $3FF7,0\n  .word reset,reset,reset\n  dephase\n";
        let assembly = crate::asm::assemble(&crate::asm::m6502::M6502, Path::new("own.a"), own, &|_| Err("no".into()));
        assert!(assembly.ok(), "{:?}", assembly.problems);
        let file = cartridge(&assembly, Some(own)).unwrap();
        assert_eq!(file.len(), 16 + 0x4000);
        assert_eq!(&file[16..19], &[0x4C, 0x00, 0xC0]);
        // Tiles at $10000 (straight after the vectors, so in the same run of bytes) become the tiles' ROM, in 8K; the source says horizontal and its mapper.
        let tiles = "; mirroring: horizontal\n; mapper: 3\n  org $8000\nreset: jmp reset\n  org $FFFA\n  .word reset,reset,reset\n  org $10000\n  .byte 1,2,3\n";
        let file = cartridge(&assemble(tiles), Some(tiles)).unwrap();
        let header = Header::read(&file).unwrap();
        assert_eq!((header.prg, header.chr, header.mapper, header.mirroring, header.chr_ram), (0x8000, 0x2000, 3, Mirroring::Horizontal, 0));
        assert_eq!(&file[16 + 0x8000..16 + 0x8003], &[1, 2, 3]);
    }

    #[test]
    fn what_cant_be_a_cartridge_is_said() {
        let no_vectors = cartridge(&assemble("  org $C000\n  rts\n"), None).unwrap_err();
        assert!(no_vectors.contains("$FFFA"), "{no_vectors}");
        let ram = cartridge(&assemble("  org $0600\n  rts\n  org $FFFA\n  .word 0,0,0\n"), None).unwrap_err();
        assert!(ram.contains("$0600"), "{ram}");
    }

    #[test]
    fn a_cartridge_disassembles_from_its_vectors() {
        let file = cartridge(&assemble(STARTER), Some(STARTER)).unwrap();
        let (code, origin, starts, found) = for_disassembly(&file).unwrap();
        assert_eq!((code.len(), origin), (0x4000, 0xC000));
        assert_eq!(starts[0], 0xC000, "reset first");
        assert!(found.contains("mapper 0"), "{found}");
        let source = crate::disasm::disassemble_checked_for(crate::assemblers::Cpu::M6502, &code, origin, &starts, false);
        assert!(source.contains("$2006"), "{source}");
        let again = crate::asm::assemble(&crate::asm::m6502::M6502, Path::new("d.a"), &source, &|_| Err("no".into()));
        assert_eq!(again.image(0xC000, 0x10000), code);
    }

    /// Writes the starter's cartridge to look at in an emulator: LIMONITER_CARTS=folder cargo test -- --ignored
    /// nes::tests::the_starter_is_written_out
    #[test]
    #[ignore]
    fn the_starter_is_written_out() {
        let folder = std::path::PathBuf::from(std::env::var("LIMONITER_CARTS").expect("LIMONITER_CARTS names a folder"));
        std::fs::write(folder.join("starter.nes"), cartridge(&assemble(STARTER), Some(STARTER)).unwrap()).unwrap();
    }

    #[test]
    fn mame_runs_it() {
        let mame = crate::emulators::by_id("mame").unwrap();
        assert!(mame.runs(crate::running::Machine::Nes, crate::running::Format::Nes));
        assert_eq!(mame.arguments(crate::running::Machine::Nes, crate::running::Format::Nes, Path::new("/tmp/n.nes"), None, ""), ["nes", "-cart", "/tmp/n.nes", "-skip_gameinfo"]);
    }
}
