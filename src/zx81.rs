//! The ZX81 built into Limoniter: a Z80 with the ZX81's own ROM (chosen by the person; see `micro_roms`), 1K or 16K
//! of RAM, its keyboard, and its picture made as the real one makes it, by the processor itself.
//!
//! How the ZX81 draws, as Logan and O'Hara's ROM disassembly and Wilf Rigter's account of the ZX81's video describe
//! it: the ROM jumps into the display file with bit 15 of the address set, so the processor "runs" the characters.
//! The ULA sees each opcode fetch from above 32K whose byte has bit 6 clear, hands the processor a NOP instead, and
//! during the refresh cycle that follows reads the character's pattern from (I and the character code times eight
//! and its line counter), which it shifts out as eight pixels, inverted if bit 7 was set. A NEWLINE (&76) is a HALT,
//! so the line ends; the maskable interrupt comes when bit 6 of the refresh register drops, which the ROM times to
//! the end of each line, and acknowledging it starts the next line's horizontal sync. Between the screen's lines, in
//! SLOW mode, a non-maskable interrupt every 207 T-states (64 microseconds) counts the blank lines above and below,
//! while BASIC runs; OUT to port &FE switches that generator on and OUT to &FD off. Reading the keyboard with it off
//! starts the vertical sync, and any OUT ends it. In FAST mode the generator is off and the screen is only drawn while
//! the ROM waits for a key.
//!
//! The ZX80 (see `zx80`) is the same machine with the NMI generator taken away: its ULA makes a horizontal sync only
//! when the processor acknowledges the line interrupt, so there are no syncs at all, and no picture, unless the ROM
//! is drawing one, which it does only while it waits for a key.
//!
//! A `.p` file (memory from &4009, as SAVE writes it) is put into memory as the ROM's LOAD leaves it, and the ROM
//! goes on from there, so a program saved as it ran starts again; LOAD from a program is caught and given the next
//! program from the tape it came from.

use crate::z80_micro::{self, Core, HEIGHT, Hardware, Key, WIDTH};
use rustzx_z80::{Z80, Z80Bus};
use std::cell::Cell;

/// T-states in a scan line.
const LINE: u32 = 207;
/// Where the picture's first column and line are, in pixels from the horizontal sync and lines from the vertical:
/// the ROM's 256 by 192 screen is put in the middle, 32 pixels and 24 lines in.
const LEFT: i32 = 52;
const TOP: i32 = 32;
/// A vertical sync this long or longer starts a new picture; shorter ones (a program reading the keyboard in FAST
/// mode) just upset the picture, as on a television.
const VSYNC_LEAST: u64 = 160;
/// Lines without a vertical sync before the picture is given up on, as a television rolls.
const LINES_MOST: u32 = 400;

/// Where the ROM's LOAD starts reading the tape, with DE at the name, and where it goes on once loaded.
const LOAD_ROUTINE: u16 = 0x0347;
const AFTER_LOAD: u16 = 0x0207;
/// The program's memory, from VERSN at &4009, and where the program's lines start.
pub const FILE_START: u16 = 0x4009;

/// The keyboard's rows, by the address line (A8 to A15) that reads each, five keys a row from bit 0.
pub const ROWS: [[&str; 5]; 8] = [
    ["SHIFT", "Z", "X", "C", "V"],
    ["A", "S", "D", "F", "G"],
    ["Q", "W", "E", "R", "T"],
    ["1", "2", "3", "4", "5"],
    ["0", "9", "8", "7", "6"],
    ["P", "O", "I", "U", "Y"],
    ["NEWLINE", "L", "K", "J", "H"],
    ["SPACE", ".", "M", "N", "B"],
];

/// A key by its name on the ZX81's keyboard.
pub fn key(name: &str) -> Option<Key> {
    ROWS.iter().enumerate().find_map(|(row, keys)| keys.iter().position(|k| *k == name).map(|bit| (row, bit as u8)))
}

/// The keys pressed together to type a character, in L (letters) mode: a letter, digit, space, full stop or
/// NEWLINE on its own, and the signs with SHIFT.
pub fn keys_for(c: char) -> Option<Vec<Key>> {
    let plain = |name: &str| key(name).map(|k| vec![k]);
    let shifted = |name: &str| key(name).map(|k| vec![(0, 0), k]);
    match c.to_ascii_uppercase() {
        c @ ('A'..='Z' | '0'..='9') => plain(&c.to_string()),
        ' ' => plain("SPACE"),
        '.' => plain("."),
        '\n' | '\r' => plain("NEWLINE"),
        '"' => shifted("P"),
        '$' => shifted("U"),
        '(' => shifted("I"),
        ')' => shifted("O"),
        '-' => shifted("J"),
        '+' => shifted("K"),
        '=' => shifted("L"),
        ':' => shifted("Z"),
        ';' => shifted("X"),
        '?' => shifted("C"),
        '/' => shifted("V"),
        '*' => shifted("B"),
        '<' => shifted("N"),
        '>' => shifted("M"),
        ',' => shifted("."),
        '£' => shifted("SPACE"),
        _ => None,
    }
}

/// Which of the two machines the hardware is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Model {
    /// The ZX80: no NMI generator, so no horizontal syncs but those the line interrupt brings.
    Zx80,
    Zx81,
}

/// The ZX81's hardware (and the ZX80's): its memory, the ULA's counters and the keyboard.
pub struct Hw {
    pub(crate) model: Model,
    rom: Vec<u8>,
    pub(crate) ram: Vec<u8>,
    t: u64,
    /// When the last picture was finished, for the ZX80's to be given up on when the ROM stops drawing.
    last_picture: u64,
    /// T-states since the last horizontal sync, and until the one an acknowledged interrupt starts.
    line_t: u32,
    delayed: u32,
    /// The ULA's line counter, which picks the line of each character's pattern.
    lcntr: u8,
    /// Lines since the last vertical sync.
    beam: u32,
    nmi_on: bool,
    /// When the vertical sync began, while it's on.
    vsync: Option<u64>,
    nmi: Cell<bool>,
    int_pending: Cell<bool>,
    int_acked: Cell<bool>,
    iff1: bool,
    i: u8,
    halted: bool,
    keys: [u8; 8],
    /// The picture being drawn (0 paper, 1 ink), and the last one finished.
    pixels: Vec<u8>,
    rgba: Vec<u8>,
    /// Pictures finished, for the tests.
    pub frames: u64,
    /// The programs on the tape the machine was started from, by name, and the next to give LOAD.
    pub(crate) tape: Vec<(String, Vec<u8>)>,
    pub(crate) next: usize,
    /// What the ZX80's SAVE has saved, each as a `.o` file's bytes.
    pub saved: Vec<Vec<u8>>,
}

const PAPER: [u8; 4] = [0xF0, 0xF0, 0xE8, 0xFF];
const INK: [u8; 4] = [0x10, 0x10, 0x10, 0xFF];

impl Hw {
    pub(crate) fn new(rom: Vec<u8>, ram_k: usize, model: Model) -> Hw {
        let mut rgba = vec![0; WIDTH * HEIGHT * 4];
        z80_micro::paint(&vec![0; WIDTH * HEIGHT], PAPER, INK, &mut rgba);
        Hw {
            model,
            rom,
            ram: vec![0; ram_k.clamp(1, 16) * 1024],
            t: 0,
            last_picture: 0,
            line_t: 0,
            delayed: 0,
            lcntr: 0,
            beam: 0,
            nmi_on: false,
            vsync: None,
            nmi: Cell::new(false),
            int_pending: Cell::new(false),
            int_acked: Cell::new(false),
            iff1: false,
            i: 0,
            halted: false,
            keys: [0xFF; 8],
            pixels: vec![0; WIDTH * HEIGHT],
            rgba,
            frames: 0,
            tape: Vec::new(),
            next: 0,
            saved: Vec::new(),
        }
    }

    /// Time passing: the horizontal syncs, each counting a line and, in SLOW mode, bringing a non-maskable
    /// interrupt.
    fn tick(&mut self, n: u32) {
        self.t += u64::from(n);
        let mut n = n;
        while n > 0 {
            let step = n.min(LINE - self.line_t).min(if self.delayed > 0 { self.delayed } else { u32::MAX });
            self.line_t += step;
            n -= step;
            if self.delayed > 0 {
                self.delayed -= step;
                if self.delayed == 0 {
                    self.line_t = 0;
                    self.hsync(false);
                    continue;
                }
            }
            if self.line_t >= LINE {
                self.line_t -= LINE;
                // Only the ZX81's ULA counts lines out by itself.
                if self.model == Model::Zx81 {
                    self.hsync(true);
                }
            }
        }
        // A ZX80 running a program makes no proper syncs (only, when it looks for BREAK, a vertical one that never
        // ends), so the television has nothing to show: the picture goes blank until the ROM draws one again.
        if self.model == Model::Zx80 && self.t - self.last_picture > u64::from(LINES_MOST * LINE) {
            self.finish_picture();
        }
    }

    fn hsync(&mut self, from_counter: bool) {
        if from_counter && self.nmi_on {
            self.nmi.set(true);
        }
        self.lcntr = if self.vsync.is_some() { 0 } else { (self.lcntr + 1) & 7 };
        self.beam += 1;
        if self.beam > LINES_MOST {
            self.finish_picture();
        }
    }

    fn finish_picture(&mut self) {
        z80_micro::paint(&self.pixels, PAPER, INK, &mut self.rgba);
        self.pixels.fill(0);
        self.beam = 0;
        self.frames += 1;
        self.last_picture = self.t;
    }

    /// A character the ULA shifts out: eight pixels of its pattern, where the beam is.
    fn draw(&mut self, byte: u8) {
        let address = (u16::from(self.i & 0xFE) << 8) | (u16::from(byte & 0x3F) << 3) | u16::from(self.lcntr);
        let mut pattern = self.read_internal(address);
        if byte & 0x80 != 0 {
            pattern = !pattern;
        }
        let y = self.beam as i32 - TOP;
        if !(0..HEIGHT as i32).contains(&y) {
            return;
        }
        let x0 = self.line_t as i32 * 2 - LEFT;
        for bit in 0..8 {
            let x = x0 + bit;
            if (0..WIDTH as i32).contains(&x) {
                self.pixels[y as usize * WIDTH + x as usize] = (pattern >> (7 - bit)) & 1;
            }
        }
    }

    fn ram_index(&self, address: u16) -> usize {
        usize::from(address & 0x3FFF) % self.ram.len()
    }
}

impl Z80Bus for Hw {
    fn read_internal(&mut self, addr: u16) -> u8 {
        self.peek(addr)
    }

    fn write_internal(&mut self, addr: u16, data: u8) {
        self.poke(addr, data);
    }

    fn read(&mut self, addr: u16, clk: usize) -> u8 {
        self.tick(clk as u32);
        let byte = self.peek(addr);
        // An opcode fetched from the display file: a character to draw, and a NOP to run.
        if clk == 4 && addr & 0x8000 != 0 && byte & 0x40 == 0 && !self.halted {
            self.draw(byte);
            return 0;
        }
        byte
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
        if !self.nmi_on && self.vsync.is_none() {
            self.vsync = Some(self.t);
            self.lcntr = 0;
        }
        // Bit 6 set says it's a 50 Hz machine; bit 7 is the tape, silent. The ZX80's LOAD waits for bit 7 to drop,
        // so a ZX80 with no tape playing reads it high, and waits until BREAK (SPACE) is pressed.
        let tape = if self.model == Model::Zx80 { 0x80 } else { 0 };
        (keys & 0x1F) | 0x60 | tape
    }

    fn write_io(&mut self, port: u16, _data: u8) {
        self.tick(4);
        if let Some(since) = self.vsync.take() {
            self.line_t = 0;
            if self.t - since >= VSYNC_LEAST {
                self.finish_picture();
            }
        }
        // The ZX80 has no NMI generator to switch on.
        if port & 1 == 0 && self.model == Model::Zx81 {
            self.nmi_on = true;
        }
        if port & 2 == 0 {
            self.nmi_on = false;
        }
    }

    fn read_interrupt(&mut self) -> u8 {
        0xFF
    }

    fn reti(&mut self) {}

    fn halt(&mut self, halted: bool) {
        self.halted = halted;
    }

    fn int_active(&self) -> bool {
        if self.int_pending.get() && self.iff1 {
            self.int_pending.set(false);
            self.int_acked.set(true);
            return true;
        }
        false
    }

    fn nmi_active(&self) -> bool {
        self.nmi.replace(false)
    }

    fn pc_callback(&mut self, _addr: u16) {}
}

impl Hardware for Hw {
    fn now(&self) -> u64 {
        self.t
    }

    fn before(&mut self, cpu: &mut Z80) -> bool {
        self.iff1 = cpu.regs.get_iff1();
        self.i = cpu.regs.get_i();
        let pc = cpu.regs.get_pc();
        if self.model == Model::Zx80 {
            return crate::zx80::catch_tape(self, cpu, pc);
        }
        // LOAD "name" (DE at the name; above 32K is LOAD with no name in a program) is given the tape's next program.
        if pc == LOAD_ROUTINE && self.next < self.tape.len() {
            let (_, program) = self.tape[self.next].clone();
            self.next += 1;
            self.load(&program);
            let (hl, de) = (cpu.regs.get_hl(), cpu.regs.get_de());
            cpu.regs.set_hl(de);
            cpu.regs.set_de(hl);
            cpu.regs.set_pc(AFTER_LOAD);
            return true;
        }
        // Halted while the NMI generator runs, the ULA holds the processor until the next sync: time passes to it.
        if self.halted && self.nmi_on && !self.nmi.get() {
            let gap = LINE - self.line_t;
            let r = cpu.regs.get_r();
            cpu.regs.set_r((r & 0x80) | (r.wrapping_add((gap / 4) as u8) & 0x7F));
            self.tick(gap);
            return true;
        }
        false
    }

    fn after(&mut self, cpu: &mut Z80, r_before: u8) {
        if self.int_acked.replace(false) {
            // Acknowledging an interrupt restarts the line: its sync comes 16 T-states on.
            self.line_t = 0;
            self.delayed = 16;
        }
        let r = cpu.regs.get_r();
        if cpu.regs.get_iff1() && r_before & 0x40 != 0 && r & 0x40 == 0 {
            self.int_pending.set(true);
        }
    }

    fn peek(&self, address: u16) -> u8 {
        if address & 0x4000 == 0 {
            self.rom[usize::from(address) & (self.rom.len() - 1)]
        } else {
            self.ram[self.ram_index(address)]
        }
    }

    fn poke(&mut self, address: u16, value: u8) {
        if address & 0x4000 != 0 {
            let at = self.ram_index(address);
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

impl Hw {
    /// A program's memory from &4009 put in place, as LOAD leaves it.
    fn load(&mut self, program: &[u8]) {
        self.load_at(FILE_START, program);
    }

    /// Bytes put in memory from `start`, as far as the RAM goes.
    pub(crate) fn load_at(&mut self, start: u16, program: &[u8]) {
        for (i, b) in program.iter().enumerate() {
            let address = usize::from(start) + i;
            if address >= 0x4000 + self.ram.len() {
                break;
            }
            self.poke(address as u16, *b);
        }
    }
}

/// The ZX81, running.
pub struct Zx81 {
    pub core: Core<Hw>,
}

/// Frames to let the ROM start up before a program is put in: its memory check and NEW take under a second.
const BOOT_FRAMES: usize = 60;

impl Zx81 {
    /// Switches on a ZX81 with this ROM and RAM, and its programs (each a `.p` file's bytes, by name): the first is
    /// put in memory and run on from where its SAVE left it, and the others are kept for LOAD.
    pub fn start(rom: Vec<u8>, ram_k: usize, programs: Vec<(String, Vec<u8>)>) -> Result<Zx81, String> {
        if rom.len() != 8192 {
            return Err(crate::i18n::tr("The ZX81's ROM is 8K.").into());
        }
        let mut machine = Zx81 { core: Core::new(Z80::default(), Hw::new(rom, ram_k, Model::Zx81)) };
        for _ in 0..BOOT_FRAMES {
            machine.core.frame();
        }
        if let Some((_, first)) = programs.first() {
            let first = first.clone();
            machine.core.hw.tape = programs;
            machine.core.hw.next = 1;
            machine.put_program(&first)?;
        }
        Ok(machine)
    }

    /// Puts a program in as LOAD would have, with the stack and registers as the ROM has them after loading, and
    /// carries on in SLOW mode from where LOAD returns.
    fn put_program(&mut self, program: &[u8]) -> Result<(), String> {
        let hw = &mut self.core.hw;
        let top = 0x4000 + hw.ram.len() as u16;
        if usize::from(FILE_START) + program.len() > usize::from(top) {
            return Err(crate::i18n::tf!("The program is {} bytes, more than fits in the ZX81's {}K.", program.len(), hw.ram.len() / 1024));
        }
        hw.load(program);
        let sp = top - 4;
        // ERR_NR, FLAGS, ERR_SP, RAMTOP, MODE and PPC, which come before VERSN and aren't saved.
        let [sp_low, sp_high] = sp.to_le_bytes();
        let [top_low, top_high] = top.to_le_bytes();
        for (i, b) in [0xFF, 0x80, sp_low, sp_high, top_low, top_high, 0x00, 0xFE, 0xFF].into_iter().enumerate() {
            hw.poke(0x4000 + i as u16, b);
        }
        // The stack: where the line runner goes back to, and the end marker for GOSUB.
        for (i, b) in [0x76, 0x06, 0x00, 0x3E].into_iter().enumerate() {
            hw.poke(sp + i as u16, b);
        }
        hw.nmi_on = true;
        hw.vsync = None;
        let cpu = &mut self.core.cpu;
        let regs = &mut cpu.regs;
        regs.swap_af_alt();
        regs.exx();
        regs.set_af(0xE2A1);
        regs.set_bc(0x8102);
        regs.set_de(0x002B);
        regs.set_hl(0x0000);
        regs.swap_af_alt();
        regs.exx();
        regs.set_af(0x0B85);
        regs.set_bc(0x00FF);
        regs.set_de(0x4399);
        regs.set_hl(0xC399);
        regs.set_i(0x1E);
        regs.set_r(0xDD);
        regs.set_iff1(false);
        regs.set_iff2(false);
        regs.set_ix(0x0281);
        regs.set_iy(0x4000);
        regs.set_sp(sp);
        regs.set_pc(AFTER_LOAD);
        cpu.set_im(1);
        Ok(())
    }

    /// The characters on the screen, from the display file, a row a line: what a program has printed.
    #[cfg(test)]
    pub fn screen_text(&self) -> Vec<String> {
        let hw = &self.core.hw;
        let word = |a: u16| u16::from_le_bytes([hw.peek(a), hw.peek(a + 1)]);
        let set = crate::charset::known("zx81");
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
                row.push(set.entry(0, b & 0xBF).and_then(|e| e.unicode).unwrap_or(' '));
            }
            rows.push(row.trim_end().to_string());
        }
        rows
    }
}

/// The programs a file holds, by name, each as a `.p` file's bytes from &4009: a `.p` (or `.81`), a `.p81` with its
/// name first, or a TZX tape of them.
pub fn programs_in(name: &str, bytes: &[u8]) -> Result<Vec<(String, Vec<u8>)>, String> {
    let ending = name.rsplit_once('.').map(|(_, e)| e.to_ascii_lowercase()).unwrap_or_default();
    if ending == "tzx" {
        let programs = crate::media::spectrum::zx81_programs(bytes);
        if programs.is_empty() {
            return Err(crate::i18n::tr("There's no ZX81 program on this tape.").into());
        }
        return Ok(programs);
    }
    let skip = crate::basic_extra::zx81_skip(bytes)?;
    let stem = name.rsplit_once('.').map_or(name, |(stem, _)| stem);
    Ok(vec![(stem.to_string(), bytes[skip..].to_vec())])
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A ROM of our own, assembled here, that does what the ZX81's does to show a screen (the real one can't be
    /// part of the tests): a display file of two rows, run through the ULA in SLOW mode's way, with the line
    /// interrupt and the NMI counting the margins, and the keyboard read to start each frame.
    fn test_rom() -> Vec<u8> {
        let source = r#"
            org 0
            out ($fd),a         ; NMI generator off
            ld sp,$4400
            ld a,$1e
            ld i,a              ; the patterns at $1E00
            im 1
            jp start
            org $38
            ; The line interrupt: C counts the lines of a row, B the rows.
            dec c
            jp nz,scanline
            pop hl
            dec b
            ret z
            set 3,c
waitint:    ld r,a
            ei
            jp (hl)
scanline:   pop de
            ret z
            jr waitint
            org $66
            ; The NMI: counts the margin's lines up in A'; at nought, the display.
            ex af,af'
            inc a
            jp m,nmiret
            jr z,nmicont
nmiret:     ex af,af'
            ret
nmicont:    ex af,af'
            push af
            push bc
            push de
            push hl
            ld hl,dfile+$8000
            halt
            out ($fd),a
            jp (ix)
start:      ld hl,dfile
            ld de,$4000
            ld bc,dend-dfile
            ldir
            ld hl,loop          ; as if an NMI had come in the loop and gone on to the display
            push hl
            push af
            push bc
            push de
            push hl
            jp frame
loop:       jr loop
frame:      ld a,$7f
            in a,($fe)          ; the vertical sync starts
            ld ($4100),a        ; the bottom row's keys, kept for the test
            ld b,0
vs:         djnz vs
            out ($ff),a         ; and ends
            ld ix,display
            jr margin
display:    ld bc,$0301         ; a line to start, then two rows of eight
            ld a,$f5
            call show
            ld ix,frame
margin:     ld a,256-54         ; 54 lines of margin, counted by the NMI
            ex af,af'
            pop hl
            pop de
            pop bc
            pop af
            out ($fe),a         ; NMI generator on, and back to the program
            ret
show:       ld r,a
            ld a,$dd
            ei
            ld hl,$c000
            jp (hl)
dfile:      db $76
            db $26,$80,$27,$00,$76
            db $28,$29,$76
dend:
            org $1e00 + $26*8
            db $ff,$81,$81,$81,$81,$81,$81,$ff
            db $f0,$f0,$f0,$f0,$0f,$0f,$0f,$0f
            db $aa,$55,$aa,$55,$aa,$55,$aa,$55
            db $18,$18,$18,$18,$18,$18,$18,$18
            org $1fff
            db 0
        "#;
        let assembly = crate::assemblers::assemble_own(crate::assemblers::Cpu::Z80, std::path::Path::new("rom.asm"), source).assembly;
        assert!(assembly.ok(), "{:?}", assembly.problems);
        assembly.image(0, 0x2000)
    }

    #[test]
    fn the_processor_draws_the_screen_from_the_display_file() {
        let mut core = Core::new(Z80::default(), Hw::new(test_rom(), 1, Model::Zx81));
        for _ in 0..10 {
            core.frame();
        }
        assert!(core.hw.frames >= 8, "each frame's vertical sync finishes a picture ({})", core.hw.frames);
        // Two rows of characters, the first a box, an inverse space (all ink), a chequer and a space.
        let rgba = core.hw.picture();
        let ink = |x: usize, y: usize| rgba[(y * WIDTH + x) * 4] < 0x80;
        let (mut top, mut left) = (None, None);
        'find: for y in 0..HEIGHT {
            for x in 0..WIDTH {
                if ink(x, y) {
                    (top, left) = (Some(y), Some(x));
                    break 'find;
                }
            }
        }
        let (top, left) = (top.expect("something is drawn"), left.unwrap());
        // The box: a full line, then its sides.
        assert!((0..8).all(|x| ink(left + x, top)), "the box's top");
        assert!(ink(left, top + 1) && !ink(left + 1, top + 1) && ink(left + 7, top + 1), "its sides");
        assert!((0..8).all(|x| ink(left + x, top + 7)), "its bottom: the line counter went through all eight lines");
        // The inverse space beside it is solid ink, and the half blocks after that.
        assert!((0..8).all(|y| (8..16).all(|x| ink(left + x, top + y))), "an inverse space");
        assert!(ink(left + 16, top) && !ink(left + 20, top) && !ink(left + 16, top + 4) && ink(left + 20, top + 4), "the half blocks, a line at a time");
        // The second row, eight lines down: the chequer, then the bar.
        assert!(ink(left, top + 8) && !ink(left + 1, top + 8) && !ink(left, top + 9) && ink(left + 1, top + 9), "the chequer");
        assert!(ink(left + 11, top + 8) && !ink(left + 10, top + 8) && ink(left + 12, top + 15), "the bar");
        // Where the ROM's screen would be: about the middle.
        assert_eq!((left, top), (32, 24), "the screen sits inside the border");
    }

    #[test]
    fn the_keyboard_is_read_by_rows() {
        let mut core = Core::new(Z80::default(), Hw::new(test_rom(), 1, Model::Zx81));
        core.hold(&[key("SPACE").unwrap(), key("M").unwrap()], true);
        core.frame();
        core.frame();
        assert_eq!(core.hw.peek(0x4100) & 0x1F, 0b11010, "SPACE and M on the bottom row are down");
        core.release_all();
        core.frame();
        assert_eq!(core.hw.peek(0x4100), 0x7F, "nothing down, 50 Hz, and the tape silent");
    }

    #[test]
    fn characters_are_typed_with_shift_for_the_signs() {
        assert_eq!(keys_for('a'), Some(vec![(1, 0)]));
        assert_eq!(keys_for('"'), Some(vec![(0, 0), (5, 0)]));
        assert_eq!(keys_for('\n'), Some(vec![(6, 0)]));
        assert_eq!(keys_for('{'), None);
    }

    #[test]
    fn memory_is_rom_below_16k_and_ram_above_mirrored() {
        let mut hw = Hw::new(vec![0xAB; 8192], 1, Model::Zx81);
        hw.poke(0x4000, 7);
        assert_eq!(hw.peek(0x4400), 7, "1K is seen again every 1K");
        assert_eq!(hw.peek(0xC000), 7, "and above 48K");
        hw.poke(0x1000, 1);
        assert_eq!(hw.peek(0x1000), 0xAB, "the ROM can't be written");
        assert_eq!(hw.peek(0x8000), 0xAB, "and is seen again at 32K");
    }

    /// With the real ROM, the machine code starter (in line 1's REM, run by RAND USR) prints through the ROM.
    #[test]
    fn with_the_real_rom_machine_code_runs() {
        let Some(Ok(rom)) = crate::micro_roms::rom(crate::micro_roms::Needs::Zx81) else {
            return;
        };
        let assembly = crate::assemblers::assemble_own(crate::assemblers::Cpu::Z80, std::path::Path::new("hello.asm"), crate::zx81_program::STARTER).assembly;
        assert!(assembly.ok(), "{:?}", assembly.problems);
        let (low, high) = (assembly.lowest().unwrap(), assembly.highest().unwrap());
        assert!(crate::zx81_program::is_zx81_code(low));
        let file = crate::zx81_program::machine_code(&assembly.image(low, high), low as u16, low as u16).unwrap();
        let mut zx81 = Zx81::start(rom, 16, vec![("hello".into(), file)]).unwrap();
        for _ in 0..100 {
            zx81.core.frame();
        }
        assert_eq!(zx81.screen_text()[0], "HELLO", "{:?}", zx81.screen_text());
        // A breakpoint at the code's start stops there, with the registers to see.
        let mut zx81 = Zx81::start(crate::micro_roms::rom(crate::micro_roms::Needs::Zx81).unwrap().unwrap(), 16, vec![("hello".into(), crate::zx81_program::machine_code(&assembly.image(low, high), low as u16, low as u16).unwrap())]).unwrap();
        zx81.core.set_breakpoints([low as u16 + 3]);
        for _ in 0..100 {
            zx81.core.frame();
        }
        assert_eq!(zx81.core.stopped, Some(low as u16 + 3), "stopped at next:");
        assert_eq!(zx81.core.registers().hl, low as u16 + 11, "HL at the message");
        zx81.core.step();
        assert_eq!(zx81.core.registers().af >> 8, 0x2D, "LD A,(HL) read the H");
    }

    /// With the real ROM (LIMONITER_ZX81_ROM names it), a program in BASIC runs and prints.
    #[test]
    fn with_the_real_rom_a_program_runs() {
        let Some(Ok(rom)) = crate::micro_roms::rom(crate::micro_roms::Needs::Zx81) else {
            return;
        };
        let program = crate::zx81_program::program(&["10 PRINT \"HELLO, WORLD\"", "20 PRINT 6*7"]).unwrap();
        let mut zx81 = Zx81::start(rom, 16, vec![("test".into(), program)]).unwrap();
        for _ in 0..100 {
            zx81.core.frame();
        }
        let text = zx81.screen_text();
        assert_eq!(text[0], "HELLO, WORLD", "{text:?}");
        assert_eq!(text[1], "42");
        // The picture: inverse squares in the screen's corners show where it sits, inside the border.
        let program = crate::zx81_program::program(&["10 PRINT AT 0,0;\"% \";AT 21,31;\"% \";AT 10,10;\"ZX81 ABC\"", "20 GOTO 20"]).unwrap();
        let mut zx81 = Zx81::start(crate::micro_roms::rom(crate::micro_roms::Needs::Zx81).unwrap().unwrap(), 16, vec![("test".into(), program)]).unwrap();
        for _ in 0..100 {
            zx81.core.frame();
        }
        let rgba = zx81.core.hw.picture();
        if let Some(out) = std::env::var_os("LIMONITER_PICTURE_OUT") {
            let mut ppm = format!("P6 {WIDTH} {HEIGHT} 255\n").into_bytes();
            ppm.extend(rgba.chunks(4).flat_map(|p| [p[0], p[1], p[2]]));
            std::fs::write(out, ppm).unwrap();
        }
        let ink: Vec<(usize, usize)> = (0..HEIGHT).flat_map(|y| (0..WIDTH).map(move |x| (x, y))).filter(|(x, y)| rgba[(y * WIDTH + x) * 4] < 0x80).collect();
        let (left, right) = (ink.iter().map(|p| p.0).min(), ink.iter().map(|p| p.0).max());
        let (top, bottom) = (ink.iter().map(|p| p.1).min(), ink.iter().map(|p| p.1).max());
        assert_eq!((left, right, top, bottom), (Some(32), Some(287), Some(24), Some(24 + 22 * 8 - 1)), "{} pixels of ink", ink.len());
        // The letters, line by line as the ROM's patterns have them: Z's top line is 01111110 at row 10, column 10.
        let line = |x: usize, y: usize| (0..8).fold(0u8, |b, i| b << 1 | u8::from(rgba[(y * WIDTH + x + i) * 4] < 0x80));
        let rom = crate::micro_roms::rom(crate::micro_roms::Needs::Zx81).unwrap().unwrap();
        for (column, code) in [(10, 0x3F), (11, 0x3D), (12, 0x24), (13, 0x1D), (15, 0x26), (16, 0x27), (17, 0x28)] {
            for row_line in 0..8 {
                assert_eq!(line(32 + column * 8, 24 + 80 + row_line), rom[0x1E00 + code * 8 + row_line], "column {column}, line {row_line}");
            }
        }
    }
}
