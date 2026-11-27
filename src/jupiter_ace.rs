//! The Jupiter Ace built into Limoniter: a Z80 with the Ace's own ROM (chosen by the person; see `micro_roms`), its
//! 3K, or 19K with the 16K pack, its keyboard, and its screen of characters whose patterns are in RAM, so programs
//! can draw their own.
//!
//! The Ace's memory, as its manual (Steven Vickers's "Jupiter Ace FORTH Programming") and the Ace's circuit set it
//! out: the ROM to 8K; the screen's 768 characters at &2400 (and seen again at &2000); the 128 characters' patterns,
//! eight bytes each, at &2C00 (and &2800), which the processor can only write; 1K of RAM at &3C00, seen four times
//! from &3000; and the pack from &4000. The keyboard is read on port &FE, its eight rows by the address's top byte
//! as on the Spectrum, with SHIFT, SYMBOL SHIFT, Z, X and C on the first and SPACE, M, N, B and V on the last. A
//! maskable interrupt comes each frame, fifty times a second.
//!
//! Tapes are `.tap` files as xAce and EightyOne write them: each block's length, then the block without a flag
//! (a header's 25 bytes, or the data) and its check byte; or a TZX of the same with flags. The ROM's tape routine is
//! caught where it reads a block, and given the next one at once. A dictionary is loaded by typing LOAD and its name.
//! Forth source is typed in, a line at a time, as if at the keyboard.

use crate::z80_micro::{self, Core, HEIGHT, Hardware, Key, WIDTH};
use rustzx_z80::{Z80, Z80Bus};

/// Where the ROM reads a tape block (HL where it goes, DE how long, C its kind), and where its key tables are: the
/// keys plain, with SHIFT and with SYMBOL SHIFT, forty each, as the ROM decodes them.
const READ_BLOCK: u16 = 0x18A7;
/// Where it writes one (HL from where, DE how long).
const WRITE_BLOCK: u16 = 0x1820;
const KEY_TABLES: [u16; 3] = [0x0376, 0x039E, 0x03C6];
const SCREEN: u16 = 0x2400;
const PATTERNS: u16 = 0x2C00;
/// The border round the 256 by 192 screen in the picture.
const LEFT: usize = 32;
const TOP: usize = 24;

/// The keys on the matrix, by row (A8 to A15) and bit, as the Ace's keyboard has them.
pub const ROWS: [[&str; 5]; 8] = [
    ["SHIFT", "SYMBOL SHIFT", "Z", "X", "C"],
    ["A", "S", "D", "F", "G"],
    ["Q", "W", "E", "R", "T"],
    ["1", "2", "3", "4", "5"],
    ["0", "9", "8", "7", "6"],
    ["P", "O", "I", "U", "Y"],
    ["ENTER", "L", "K", "J", "H"],
    ["SPACE", "M", "N", "B", "V"],
];

pub const SHIFT: Key = (0, 0);
pub const SYMBOL_SHIFT: Key = (0, 1);

/// A key by its name on the Ace's keyboard.
pub fn key(name: &str) -> Option<Key> {
    ROWS.iter().enumerate().find_map(|(row, keys)| keys.iter().position(|k| *k == name).map(|bit| (row, bit as u8)))
}

pub struct Hw {
    /// What the program has written to the debug console port.
    console: crate::debug_console::Console,
    rom: Vec<u8>,
    /// &2000 to &3FFF (the screen, the patterns and the 1K), then the pack.
    ram: Vec<u8>,
    t: u64,
    /// When the next frame's interrupt comes.
    next_frame: u64,
    /// The frame's interrupt, waiting to be taken: kept until interrupts are on to take it, as ZEsarUX does, so a
    /// frame's keyboard scan isn't lost while the ROM has them off.
    int_pending: std::cell::Cell<bool>,
    iff1: bool,
    keys: [u8; 8],
    rgba: Vec<u8>,
    /// The tape's blocks, each without its length (the data and its check byte), and the next to give.
    tape: Vec<Vec<u8>>,
    next: usize,
    /// What SAVE has written, as a `.tap` file.
    pub saved: Vec<u8>,
    pub frames: u64,
}

const PAPER: [u8; 4] = [0x08, 0x08, 0x08, 0xFF];
const INK: [u8; 4] = [0xF0, 0xF0, 0xF0, 0xFF];

impl Hw {
    fn new(rom: Vec<u8>, pack: bool) -> Hw {
        let mut rgba = vec![0; WIDTH * HEIGHT * 4];
        z80_micro::paint(&vec![0; WIDTH * HEIGHT], PAPER, INK, &mut rgba);
        Hw { console: Default::default(), rom, ram: vec![0; if pack { 0x6000 } else { 0x2000 }], t: 0, next_frame: z80_micro::FRAME_T, int_pending: std::cell::Cell::new(false), iff1: false, keys: [0xFF; 8], rgba, tape: Vec::new(), next: 0, saved: Vec::new(), frames: 0 }
    }

    /// Where an address is in the RAM, if it's RAM: the screen's and the patterns' 1K each seen twice, and the 1K at
    /// &3C00 four times.
    fn ram_index(&self, address: u16) -> Option<usize> {
        let at = match address {
            0x2000..=0x27FF => 0x2400 | (address & 0x03FF),
            0x2800..=0x2FFF => 0x2C00 | (address & 0x03FF),
            0x3000..=0x3FFF => 0x3C00 | (address & 0x03FF),
            _ => address,
        };
        let index = usize::from(at).checked_sub(0x2000)?;
        (index < self.ram.len()).then_some(index)
    }

    fn tick(&mut self, n: u32) {
        self.t += u64::from(n);
        if self.t >= self.next_frame {
            self.next_frame += z80_micro::FRAME_T;
            self.int_pending.set(true);
            self.draw();
        }
    }

    /// The screen drawn from its characters and their patterns.
    fn draw(&mut self) {
        let mut pixels = vec![0u8; WIDTH * HEIGHT];
        for row in 0..24 {
            for column in 0..32 {
                let code = self.ram[usize::from(SCREEN - 0x2000) + row * 32 + column];
                for line in 0..8 {
                    let mut pattern = self.ram[usize::from(PATTERNS - 0x2000) + usize::from(code & 0x7F) * 8 + line];
                    if code & 0x80 != 0 {
                        pattern = !pattern;
                    }
                    let y = TOP + row * 8 + line;
                    for bit in 0..8 {
                        pixels[y * WIDTH + LEFT + column * 8 + bit] = (pattern >> (7 - bit)) & 1;
                    }
                }
            }
        }
        z80_micro::paint(&pixels, PAPER, INK, &mut self.rgba);
        self.frames += 1;
    }
}

impl Hw {
    /// A RET, done for a routine that was caught.
    fn ret(cpu: &mut Z80, hw: &Hw) {
        let sp = cpu.regs.get_sp();
        cpu.regs.set_pc(u16::from_le_bytes([hw.peek(sp), hw.peek(sp.wrapping_add(1))]));
        cpu.regs.set_sp(sp.wrapping_add(2));
    }
}

impl Z80Bus for Hw {
    fn read_internal(&mut self, addr: u16) -> u8 {
        self.peek(addr)
    }

    fn write_internal(&mut self, addr: u16, data: u8) {
        self.poke(addr, data);
    }

    fn wait_mreq(&mut self, _addr: u16, clk: usize) {
        self.tick(clk as u32);
    }

    fn wait_no_mreq(&mut self, _addr: u16, clk: usize) {
        self.tick(clk as u32);
    }

    fn wait_internal(&mut self, clk: usize) {
        self.tick(clk as u32);
    }

    fn read_io(&mut self, port: u16) -> u8 {
        self.tick(4);
        if port & 1 != 0 {
            return 0xFF;
        }
        let mut keys = 0x1F;
        for (row, held) in self.keys.iter().enumerate() {
            if port & (0x100 << row) == 0 {
                keys &= held;
            }
        }
        keys
    }

    fn write_io(&mut self, port: u16, data: u8) {
        self.tick(4);
        if port & 0xFF == u16::from(crate::debug_console::Z80_PORT) {
            self.console.put(data);
        }
    }

    fn read_interrupt(&mut self) -> u8 {
        0xFF
    }

    fn reti(&mut self) {}

    fn halt(&mut self, _halted: bool) {}

    fn int_active(&self) -> bool {
        self.iff1 && self.int_pending.replace(false)
    }

    fn nmi_active(&self) -> bool {
        false
    }

    fn pc_callback(&mut self, _addr: u16) {}
}

impl Hardware for Hw {
    fn take_console(&mut self) -> Vec<String> {
        self.console.take()
    }

    fn now(&self) -> u64 {
        self.t
    }

    fn before(&mut self, cpu: &mut Z80) -> bool {
        self.iff1 = cpu.regs.get_iff1();
        let pc = cpu.regs.get_pc();
        if pc == WRITE_BLOCK {
            // SAVE's block, kept as a .tap file keeps it: its length, the bytes and their check byte.
            let (from, length) = (cpu.regs.get_hl(), cpu.regs.get_de());
            let block: Vec<u8> = (0..length).map(|i| self.peek(from.wrapping_add(i))).collect();
            self.saved.extend((length + 1).to_le_bytes());
            self.saved.extend(&block);
            self.saved.push(block.iter().fold(0, |c, b| c ^ b));
            Self::ret(cpu, self);
            self.tick(1000);
            return true;
        }
        if pc != READ_BLOCK || self.next >= self.tape.len() {
            return false;
        }
        // The tape's next block, given at once: as much as was asked for goes where HL says, and the routine returns
        // with carry set if it was all there.
        let block = self.tape[self.next].clone();
        self.next += 1;
        let (mut to, wanted) = (cpu.regs.get_hl(), usize::from(cpu.regs.get_de()));
        let data = &block[..block.len().saturating_sub(1)];
        for b in data.iter().take(wanted) {
            self.poke(to, *b);
            to = to.wrapping_add(1);
        }
        cpu.regs.set_hl(to);
        cpu.regs.set_de(0);
        let flags = cpu.regs.get_flags();
        cpu.regs.set_flags(if data.len() >= wanted { flags | 1 } else { flags & !1 });
        Self::ret(cpu, self);
        self.tick(1000);
        true
    }

    fn after(&mut self, _cpu: &mut Z80, _r_before: u8) {}

    fn peek(&self, address: u16) -> u8 {
        if address < 0x2000 {
            return self.rom[usize::from(address)];
        }
        self.ram_index(address).map_or(0xFF, |at| self.ram[at])
    }

    fn poke(&mut self, address: u16, value: u8) {
        if let Some(at) = self.ram_index(address) {
            self.ram[at] = value;
        }
    }

    fn picture(&self) -> &[u8] {
        &self.rgba
    }

    fn set_key(&mut self, row: usize, bit: u8, down: bool) {
        if down {
            self.keys[row] &= !(1 << bit);
        } else {
            self.keys[row] |= 1 << bit;
        }
    }
}

/// The Jupiter Ace, running.
pub struct Ace {
    pub core: Core<Hw>,
    /// The ROM's own key tables, by which characters are typed.
    tables: [Vec<u8>; 3],
}

/// Frames for the ROM to start and clear its screen.
const BOOT_FRAMES: usize = 50;

impl Ace {
    /// Switches on an Ace with this ROM, and with the 16K pack if `pack`.
    pub fn start(rom: Vec<u8>, pack: bool) -> Result<Ace, String> {
        if rom.len() != 8192 {
            return Err(crate::i18n::tr("The Jupiter Ace's ROM is 8K.").into());
        }
        let tables = KEY_TABLES.map(|at| rom[usize::from(at)..usize::from(at) + 40].to_vec());
        let mut ace = Ace { core: Core::new(Z80::default(), Hw::new(rom, pack)), tables };
        for _ in 0..BOOT_FRAMES {
            ace.core.frame();
        }
        Ok(ace)
    }

    /// The keys that type a character, as the ROM's tables have them: on its own, with SHIFT, or with SYMBOL SHIFT.
    pub fn keys_for(&self, c: char) -> Option<Vec<Key>> {
        let code = match c {
            '\n' | '\r' => 0x0D,
            c if c.is_ascii() => c as u8,
            _ => return None,
        };
        for (table, shift) in self.tables.iter().zip([None, Some(SHIFT), Some(SYMBOL_SHIFT)]) {
            if let Some(index) = table.iter().position(|b| *b == code && code != 0) {
                // The tables go a bit at a time from bit 4, and within each, row by row from the last.
                let key = (7 - index % 8, 4 - (index / 8) as u8);
                return Some(shift.into_iter().chain([key]).collect());
            }
        }
        None
    }

    /// Text typed in, as at the keyboard: each character's keys held for four frames (the ROM takes a key once it's
    /// been down for three of its scans), with two between. What can't be typed is said.
    pub fn type_text(&mut self, text: &str) -> Result<(), String> {
        let mut presses = Vec::new();
        for c in text.chars() {
            let c = if c == '\t' { ' ' } else { c };
            presses.push(self.keys_for(c).ok_or_else(|| crate::i18n::tf!("The Jupiter Ace has no key for {c}."))?);
        }
        for keys in presses {
            self.core.type_keys(keys, 4, 2);
        }
        Ok(())
    }

    /// Puts a tape in: its blocks are given to the ROM's tape routine as it asks for them.
    pub fn insert_tape(&mut self, blocks: Vec<Vec<u8>>) {
        self.core.hw.tape = blocks;
        self.core.hw.next = 0;
    }

    /// The characters on the screen, a row a line.
    #[cfg(test)]
    pub fn screen_text(&self) -> Vec<String> {
        (0..24)
            .map(|row| {
                let line: String = (0..32)
                    .map(|column| {
                        let b = self.core.hw.peek(SCREEN + row * 32 + column) & 0x7F;
                        if (0x20..0x7F).contains(&b) { b as char } else { ' ' }
                    })
                    .collect();
                line.trim_end().to_string()
            })
            .collect()
    }
}

/// A tape's blocks, as the Ace's ROM reads them (no flag; the data and its check byte), from a `.tap` (as xAce
/// writes it) or a `.tzx` (whose blocks have a flag first, as on the Spectrum).
pub fn tape_blocks(name: &str, bytes: &[u8]) -> Result<Vec<Vec<u8>>, String> {
    let tzx = name.to_ascii_lowercase().ends_with(".tzx");
    let tap = if tzx { crate::zx_machine::tzx_to_tap(bytes)? } else { bytes.to_vec() };
    let mut blocks = Vec::new();
    let mut at = 0;
    while at + 2 <= tap.len() {
        let length = usize::from(u16::from_le_bytes([tap[at], tap[at + 1]]));
        let Some(block) = tap.get(at + 2..at + 2 + length) else {
            return Err(crate::i18n::tr("The tape stops partway through a block.").into());
        };
        blocks.push(if tzx { block.get(1..).unwrap_or_default().to_vec() } else { block.to_vec() });
        at += 2 + length;
    }
    if blocks.is_empty() {
        return Err(crate::i18n::tr("There's nothing on this tape.").into());
    }
    Ok(blocks)
}

/// Whether a `.tap` file is the Ace's: its first block a 25-byte header (and its check byte) for a dictionary or
/// bytes, where a Spectrum's header is 17 bytes and a flag.
pub fn is_ace_tape(bytes: &[u8]) -> bool {
    bytes.get(0..3).is_some_and(|b| b[0] == 26 && b[1] == 0 && (b[2] == 0x00 || b[2] == 0x20))
}

/// Whether a TZX tape is the Ace's: its first block a flag of 0 and then an Ace's 25-byte header.
pub fn is_ace_tzx(bytes: &[u8]) -> bool {
    bytes.starts_with(b"ZXTape!") && crate::zx_machine::tzx_to_tap(bytes).is_ok_and(|tap| tap.get(0..4).is_some_and(|b| b[0] == 27 && b[1] == 0 && b[2] == 0x00 && (b[3] == 0x00 || b[3] == 0x20)))
}

/// What's on an Ace tape: each file's name and whether it's a dictionary (rather than bytes).
pub fn tape_files(blocks: &[Vec<u8>]) -> Vec<(String, bool)> {
    blocks
        .iter()
        .filter(|b| b.len() == 26 && (b[0] == 0x00 || b[0] == 0x20))
        .map(|b| (String::from_utf8_lossy(&b[1..11]).trim_end().to_string(), b[0] == 0x00))
        .collect()
}

/// A tape of one dictionary, as the Ace's SAVE writes it: a header with the name and where the dictionary is, and
/// the dictionary's bytes, each with its check byte. Used by the tests.
#[cfg(test)]
pub fn dictionary_tape(name: &str, at: u16, bytes: &[u8], header_words: [u16; 4]) -> Vec<u8> {
    let mut header = vec![0x00];
    let mut padded = name.as_bytes().to_vec();
    padded.resize(10, b' ');
    header.extend(padded);
    header.extend((bytes.len() as u16).to_le_bytes());
    header.extend(at.to_le_bytes());
    for word in header_words {
        header.extend(word.to_le_bytes());
    }
    header.extend((at + bytes.len() as u16).to_le_bytes());
    let mut tape = Vec::new();
    for block in [header, bytes.to_vec()] {
        let check = block.iter().fold(0, |c, b| c ^ b);
        tape.extend((block.len() as u16 + 1).to_le_bytes());
        tape.extend(&block);
        tape.push(check);
    }
    tape
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A ROM of our own, as the Ace's lays its memory out: it writes a pattern into the character RAM and characters
    /// on the screen, counts frames in its interrupt, keeps the bottom row's keys, and reads a tape block through
    /// the routine the real ROM has at &18A7.
    fn test_rom() -> Vec<u8> {
        let source = r#"
            org 0
            di
            ld sp,$4000
            im 1
            jp start
            org $38
            push af
            ld a,($3c00)
            inc a
            ld ($3c00),a        ; the frame count
            ld a,$7f
            in a,($fe)
            ld ($3c01),a        ; the bottom row
            pop af
            ei
            reti
start:      ld hl,$2c00+$41*8   ; the pattern for A: a box
            ld (hl),$ff
            ld b,6
side:       inc hl
            ld (hl),$81
            djnz side
            inc hl
            ld (hl),$ff
            ld a,$41
            ld ($2400),a        ; A at the top left
            ld a,$c1
            ld ($2401),a        ; and inverse beside it
            ld hl,$3c10
            ld de,3
            ld c,0
            call $18a7          ; a block from the tape
            ei
loop:       halt
            jr loop
            org $18a7
            and a               ; with no tape: carry clear
            ret
            org $1fff
            db 0
        "#;
        let assembly = crate::assemblers::assemble_own(crate::assemblers::Cpu::Z80, std::path::Path::new("ace.asm"), source).assembly;
        assert!(assembly.ok(), "{:?}", assembly.problems);
        let mut rom = assembly.image(0, 0x2000);
        // Key tables as the real ROM's: "a" plain is the first row's A, bit 0 of row 1, so index 4*8 + 6.
        rom[0x0376 + 4 * 8 + 6] = b'a';
        rom[0x039E + 4 * 8 + 6] = b'A';
        rom[0x03C6 + 4 * 8 + 6] = b'~';
        rom
    }

    #[test]
    fn the_screen_is_drawn_from_characters_and_their_patterns_in_ram() {
        let mut ace = Ace::start(test_rom(), true).unwrap();
        ace.core.frame();
        let rgba = ace.core.hw.picture();
        let ink = |x: usize, y: usize| rgba[(y * WIDTH + x) * 4] > 0x80;
        assert!((0..8).all(|x| ink(LEFT + x, TOP)), "the box's top");
        assert!(ink(LEFT, TOP + 3) && !ink(LEFT + 3, TOP + 3) && ink(LEFT + 7, TOP + 3), "its sides");
        assert!(!ink(LEFT + 8, TOP) && ink(LEFT + 9, TOP + 3), "the inverse box beside it");
        assert!(!ink(LEFT - 1, TOP) && !ink(LEFT, TOP - 1), "the border");
        assert!(ace.core.hw.peek(0x3C00) >= 49, "an interrupt each frame ({})", ace.core.hw.peek(0x3C00));
        assert_eq!(ace.core.hw.peek(0x3000), ace.core.hw.peek(0x3C00), "the 1K is seen four times");
    }

    #[test]
    fn keys_are_typed_as_the_roms_tables_say() {
        let mut ace = Ace::start(test_rom(), false).unwrap();
        assert_eq!(ace.keys_for('a'), Some(vec![(1, 0)]));
        assert_eq!(ace.keys_for('A'), Some(vec![SHIFT, (1, 0)]));
        assert_eq!(ace.keys_for('~'), Some(vec![SYMBOL_SHIFT, (1, 0)]));
        assert!(ace.type_text("a€").unwrap_err().contains('€'));
        ace.core.hold(&[key("SPACE").unwrap(), key("V").unwrap()], true);
        ace.core.frame();
        assert_eq!(ace.core.hw.peek(0x3C01), 0b01110, "SPACE and V on the bottom row");
    }

    #[test]
    fn a_tape_block_is_given_to_the_roms_routine() {
        let tape = dictionary_tape("test", 0x3C51, &[1, 2, 3], [0; 4]);
        assert!(is_ace_tape(&tape));
        let blocks = tape_blocks("test.tap", &tape).unwrap();
        assert_eq!(tape_files(&blocks), vec![("test".to_string(), true)]);
        // The test ROM asks for 3 bytes: it's given the header's first three.
        let mut ace = Ace { core: Core::new(Z80::default(), Hw::new(test_rom(), true)), tables: Default::default() };
        ace.insert_tape(vec![blocks[1].clone()]);
        for _ in 0..3 {
            ace.core.frame();
        }
        assert_eq!([ace.core.hw.peek(0x3C10), ace.core.hw.peek(0x3C11), ace.core.hw.peek(0x3C12)], [1, 2, 3]);
        assert!(!is_ace_tape(&[19, 0, 0, 0]), "a Spectrum's header");
    }

    /// With the real ROM (LIMONITER_ACE_ROM names it), Forth typed in runs.
    #[test]
    fn with_the_real_rom_forth_typed_in_runs() {
        let Some(Ok(rom)) = crate::micro_roms::rom(crate::micro_roms::Needs::Ace) else {
            return;
        };
        let mut ace = Ace::start(rom, true).unwrap();
        ace.type_text(": STARS 0 DO 42 EMIT LOOP ;\n: SQ DUP * ;\n7 SQ .\n3 STARS\n").unwrap();
        for _ in 0..1500 {
            ace.core.frame();
        }
        let text = ace.screen_text().join("\n");
        assert!(text.contains("49") && text.contains("***"), "{text}");
        // SAVE writes the dictionary to tape; a fresh Ace loads it with LOAD, and the word runs.
        ace.type_text("SAVE SQUARE\n").unwrap();
        for _ in 0..300 {
            ace.core.frame();
        }
        let tape = ace.core.hw.saved.clone();
        assert!(is_ace_tape(&tape), "{:?}", &tape[..tape.len().min(30)]);
        let blocks = tape_blocks("square.tap", &tape).unwrap();
        assert_eq!(tape_files(&blocks), vec![("SQUARE".to_string(), true)]);
        let mut fresh = Ace::start(crate::micro_roms::rom(crate::micro_roms::Needs::Ace).unwrap().unwrap(), true).unwrap();
        fresh.insert_tape(blocks);
        fresh.type_text("LOAD SQUARE\n12 SQ .\n").unwrap();
        for _ in 0..1500 {
            fresh.core.frame();
        }
        let text = fresh.screen_text().join("\n");
        assert!(text.contains("144"), "{text}");
    }
}
