//! Graphics for the Atari 2600, which has no screen memory: what's on a line is what the program writes into the
//! TIA's registers as the line is drawn. Two kinds are made here.
//!
//! - **Player sprites**: 8 pixels across, a byte a line (bit 7 the left pixel), any number of lines, and a colour
//!   a line, as kernels write GRP0 and COLUP0; frames of the same height, for animation.
//! - **The playfield**: 40 blocks across, each 4 colour clocks wide. The TIA holds only 20 (PF0's top four bits,
//!   PF1 left to right and PF2 right to left), and shows them again on the right, mirrored or repeated, unless the
//!   program writes the registers again halfway across. Rows of a number of lines, each with its own colour and
//!   background colour.
//!
//! Both are written as DASM source (`.byte` tables, as kernels read them, top or bottom line first) and read back
//! from it, and both become a whole program that shows them, for Stella or MAME. The colours are Stella's NTSC,
//! PAL and SECAM palettes.

use crate::quantise::{self, Fit, Filter, Image};

/// The 128 colours of each TV system, as Stella shows them (from Stella's PaletteHandler): a colour's value is its
/// index twice, $00 to $FE, the lowest bit unused.
const NTSC: [u32; 128] = [
    0x000000, 0x4A4A4A, 0x6F6F6F, 0x8E8E8E, 0xAAAAAA, 0xC0C0C0, 0xD6D6D6, 0xECECEC,
    0x484800, 0x69690F, 0x86861D, 0xA2A22A, 0xBBBB35, 0xD2D240, 0xE8E84A, 0xFCFC54,
    0x7C2C00, 0x904811, 0xA26221, 0xB47A30, 0xC3903D, 0xD2A44A, 0xDFB755, 0xECC860,
    0x901C00, 0xA33915, 0xB55328, 0xC66C3A, 0xD5824A, 0xE39759, 0xF0AA67, 0xFCBC74,
    0x940000, 0xA71A1A, 0xB83232, 0xC84848, 0xD65C5C, 0xE46F6F, 0xF08080, 0xFC9090,
    0x840064, 0x97197A, 0xA8308F, 0xB846A2, 0xC659B3, 0xD46CC3, 0xE07CD2, 0xEC8CE0,
    0x500084, 0x68199A, 0x7D30AD, 0x9246C0, 0xA459D0, 0xB56CE0, 0xC57CEE, 0xD48CFC,
    0x140090, 0x331AA3, 0x4E32B5, 0x6848C6, 0x7F5CD5, 0x956FE3, 0xA980F0, 0xBC90FC,
    0x000094, 0x181AA7, 0x2D32B8, 0x4248C8, 0x545CD6, 0x656FE4, 0x7580F0, 0x8490FC,
    0x001C88, 0x183B9D, 0x2D57B0, 0x4272C2, 0x548AD2, 0x65A0E1, 0x75B5EF, 0x84C8FC,
    0x003064, 0x185080, 0x2D6D98, 0x4288B0, 0x54A0C5, 0x65B7D9, 0x75CCEB, 0x84E0FC,
    0x004030, 0x18624E, 0x2D8169, 0x429E82, 0x54B899, 0x65D1AE, 0x75E7C2, 0x84FCD4,
    0x004400, 0x1A661A, 0x328432, 0x48A048, 0x5CBA5C, 0x6FD26F, 0x80E880, 0x90FC90,
    0x143C00, 0x355F18, 0x527E2D, 0x6E9C42, 0x87B754, 0x9ED065, 0xB4E775, 0xC8FC84,
    0x303800, 0x505916, 0x6D762B, 0x88923E, 0xA0AB4F, 0xB7C25F, 0xCCD86E, 0xE0EC7C,
    0x482C00, 0x694D14, 0x866A26, 0xA28638, 0xBB9F47, 0xD2B656, 0xE8CC63, 0xFCE070,
];

const PAL: [u32; 128] = [
    0x0B0B0B, 0x333333, 0x595959, 0x7B7B7B, 0x999999, 0xB6B6B6, 0xCFCFCF, 0xE6E6E6,
    0x0B0B0B, 0x333333, 0x595959, 0x7B7B7B, 0x999999, 0xB6B6B6, 0xCFCFCF, 0xE6E6E6,
    0x3B2400, 0x664700, 0x8B7000, 0xAC9200, 0xC5AE36, 0xDEC85E, 0xF7E27F, 0xFFF19E,
    0x004500, 0x006F00, 0x3B9200, 0x65B009, 0x85CA3D, 0xA3E364, 0xBFFC84, 0xD5FFA5,
    0x590000, 0x802700, 0xA15700, 0xBC7937, 0xD6985F, 0xEEB381, 0xFFCE9E, 0xFFDCBD,
    0x004900, 0x007200, 0x169216, 0x45AF45, 0x6BC96B, 0x8BE38B, 0xA9FBA9, 0xC5FFC5,
    0x640012, 0x890821, 0xA73D4D, 0xC26472, 0xDC8491, 0xF4A3AE, 0xFFBECA, 0xFFDAE0,
    0x003D29, 0x006A48, 0x048E63, 0x3CAA84, 0x62C5A2, 0x83DFBE, 0xA1F8D9, 0xBEFFE9,
    0x550046, 0x88006E, 0xA5318D, 0xC159AA, 0xDA7CC5, 0xF39ADF, 0xFFB9F3, 0xFFD4F6,
    0x003651, 0x005A7D, 0x117E9C, 0x429CB8, 0x68B7D2, 0x88D2EB, 0xA6EBFF, 0xC3FFFF,
    0x4C007C, 0x75009D, 0x932EB8, 0xAF57D2, 0xCA7AEB, 0xE499FF, 0xECB7FF, 0xF3D4FF,
    0x002D83, 0x003EA4, 0x2D65BF, 0x5685DA, 0x79A2F2, 0x99BFFF, 0xB7DBFF, 0xD3F5FF,
    0x220096, 0x5200B6, 0x7538CF, 0x945FE8, 0xB181FF, 0xC5A0FF, 0xD6BDFF, 0xE8DAFF,
    0x00009A, 0x241DB6, 0x504AD0, 0x746FE9, 0x928EFF, 0xB1ADFF, 0xCECAFF, 0xE9E5FF,
    0x0B0B0B, 0x333333, 0x595959, 0x7B7B7B, 0x999999, 0xB6B6B6, 0xCFCFCF, 0xE6E6E6,
    0x0B0B0B, 0x333333, 0x595959, 0x7B7B7B, 0x999999, 0xB6B6B6, 0xCFCFCF, 0xE6E6E6,
];

/// SECAM has eight colours, chosen by the luminance bits alone.
const SECAM: [u32; 8] = [0x000000, 0x2121FF, 0xF03C79, 0xFF50FF, 0x7FFF00, 0x7FFFFF, 0xFFFF3F, 0xFFFFFF];

/// The TV system: its colours, how many lines a frame shows, and the shape of a colour clock.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tv {
    Ntsc,
    Pal,
    Secam,
}

impl Tv {
    pub const ALL: [Tv; 3] = [Tv::Ntsc, Tv::Pal, Tv::Secam];

    pub fn name(self) -> &'static str {
        match self {
            Tv::Ntsc => "NTSC",
            Tv::Pal => "PAL",
            Tv::Secam => "SECAM",
        }
    }

    /// A colour value's red, green and blue.
    pub fn rgb(self, value: u8) -> [u8; 3] {
        let index = usize::from(value >> 1);
        let c = match self {
            Tv::Ntsc => NTSC[index],
            Tv::Pal => PAL[index],
            Tv::Secam => SECAM[index & 7],
        };
        [(c >> 16) as u8, (c >> 8) as u8, c as u8]
    }

    /// The lines of picture in a frame, between the vertical blank and overscan.
    pub fn lines(self) -> usize {
        match self {
            Tv::Ntsc => 192,
            Tv::Pal | Tv::Secam => 228,
        }
    }

    /// How much wider a colour clock is than a line is tall on a 4:3 TV: the rate of square pixels (12.27 MHz for
    /// NTSC, 14.75 for PAL) over the colour clock (3.58 MHz, 3.55), halved for a field's lines.
    pub fn aspect(self) -> f32 {
        match self {
            Tv::Ntsc => 12.272_727 / 3.579_545 / 2.0,
            Tv::Pal | Tv::Secam => 14.75 / 3.546_894 / 2.0,
        }
    }

    /// The lines of vertical blank and of overscan: 37 and 30 (262 in all, with 3 of sync and 192 of picture), or
    /// 45 and 36 (312, with 228).
    fn blank_lines(self) -> (usize, usize) {
        match self {
            Tv::Ntsc => (37, 30),
            Tv::Pal | Tv::Secam => (45, 36),
        }
    }

    /// The colour values that look different, for choosing among.
    fn distinct(self) -> Vec<u8> {
        let mut seen = Vec::new();
        let mut out = Vec::new();
        for value in (0..=254u8).step_by(2) {
            let rgb = self.rgb(value);
            if !seen.contains(&rgb) {
                seen.push(rgb);
                out.push(value);
            }
        }
        out
    }
}

/// Which line of a table comes first.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Order {
    TopFirst,
    /// For kernels that count down to 0 with the line in Y.
    BottomFirst,
}

impl Order {
    pub fn name(self) -> &'static str {
        match self {
            Order::TopFirst => "Top line first",
            Order::BottomFirst => "Bottom line first",
        }
    }

    fn words(self) -> &'static str {
        match self {
            Order::TopFirst => "top line first",
            Order::BottomFirst => "bottom line first",
        }
    }

    fn arrange<T: Clone>(self, items: &[T]) -> Vec<T> {
        match self {
            Order::TopFirst => items.to_vec(),
            Order::BottomFirst => items.iter().rev().cloned().collect(),
        }
    }
}

fn binary(byte: u8) -> String {
    format!("%{byte:08b}")
}

/// Bytes as `.byte` lines, `per` to a line, as binary (which shows the pixels) or hex.
fn byte_lines(out: &mut String, bytes: &[u8], per: usize, as_binary: bool) {
    for chunk in bytes.chunks(per.max(1)) {
        let items: Vec<String> = chunk.iter().map(|&b| if as_binary { binary(b) } else { format!("${b:02X}") }).collect();
        out.push_str(&format!("        .byte {}\n", items.join(",")));
    }
}

// ---------- Player sprites ----------

/// One frame of a player sprite: a byte a line, and the colour of each line.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Frame {
    pub rows: Vec<u8>,
    pub colours: Vec<u8>,
}

/// How many copies NUSIZ makes and how wide.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Size {
    One,
    Double,
    Quad,
}

impl Size {
    pub const ALL: [Size; 3] = [Size::One, Size::Double, Size::Quad];

    pub fn name(self) -> &'static str {
        match self {
            Size::One => "1\u{D7} (NUSIZ 0)",
            Size::Double => "2\u{D7} (NUSIZ 5)",
            Size::Quad => "4\u{D7} (NUSIZ 7)",
        }
    }

    pub fn nusiz(self) -> u8 {
        match self {
            Size::One => 0,
            Size::Double => 5,
            Size::Quad => 7,
        }
    }

    /// Colour clocks a pixel.
    pub fn clocks(self) -> usize {
        match self {
            Size::One => 1,
            Size::Double => 2,
            Size::Quad => 4,
        }
    }
}

/// A player's frames, all of one height, and how they're shown.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Sprites {
    pub height: usize,
    pub frames: Vec<Frame>,
    /// Shown on this background colour.
    pub background: u8,
    pub size: Size,
    /// Lines a row: 2 for the two-line kernels most games use.
    pub lines: usize,
    /// TV frames each frame is shown for when they're played; 0 shows the first alone.
    pub speed: usize,
}

pub const MOST_LINES: usize = 128;

impl Default for Sprites {
    fn default() -> Self {
        let rows = vec![0x18, 0x3C, 0x7E, 0xDB, 0xFF, 0x24, 0x5A, 0xA5];
        let colours = vec![0x1E, 0x1C, 0x1A, 0x18, 0x18, 0x46, 0x44, 0x42];
        Sprites { height: 8, frames: vec![Frame { rows, colours }], background: 0x00, size: Size::One, lines: 2, speed: 8 }
    }
}

impl Sprites {
    pub fn blank(height: usize) -> Frame {
        Frame { rows: vec![0; height], colours: vec![0x0E; height] }
    }

    /// Every frame made `height` lines tall: cut off at the bottom, or lines added there in the last line's colour.
    pub fn set_height(&mut self, height: usize) {
        let height = height.clamp(1, MOST_LINES);
        self.height = height;
        for frame in &mut self.frames {
            let last = frame.colours.last().copied().unwrap_or(0x0E);
            frame.rows.resize(height, 0);
            frame.colours.resize(height, last);
        }
    }

    pub fn pixel(&self, frame: usize, x: usize, y: usize) -> bool {
        self.frames[frame].rows[y] & (0x80 >> x) != 0
    }

    pub fn set(&mut self, frame: usize, x: usize, y: usize, on: bool) {
        if x < 8 && y < self.height {
            let row = &mut self.frames[frame].rows[y];
            if on {
                *row |= 0x80 >> x;
            } else {
                *row &= !(0x80 >> x);
            }
        }
    }

    /// Whether every frame has the same colours, so that one table does.
    pub fn shared_colours(&self) -> bool {
        self.frames.windows(2).all(|w| w[0].colours == w[1].colours)
    }

    /// DASM source: the height, each frame's lines (binary, so the pixels show) and the colours, one table for all
    /// frames when they're the same.
    pub fn source(&self, label: &str, order: Order) -> String {
        let mut out = format!(
            "; Atari 2600 player graphics: {} frame{} of 8\u{D7}{}, {}. Bit 7 is the left pixel.\n",
            self.frames.len(),
            if self.frames.len() == 1 { "" } else { "s" },
            self.height,
            order.words()
        );
        out.push_str(&format!("{}_HEIGHT = {}\n", label.to_uppercase(), self.height));
        for (n, frame) in self.frames.iter().enumerate() {
            out.push_str(&format!("{label}_frame{n}:\n"));
            byte_lines(&mut out, &order.arrange(&frame.rows), 1, true);
        }
        if self.shared_colours() {
            out.push_str(&format!("{label}_colours:\n"));
            byte_lines(&mut out, &order.arrange(&self.frames[0].colours), 8, false);
        } else {
            for (n, frame) in self.frames.iter().enumerate() {
                out.push_str(&format!("{label}_colours{n}:\n"));
                byte_lines(&mut out, &order.arrange(&frame.colours), 8, false);
            }
        }
        out
    }

    /// The graphics alone, frame after frame, for INCBIN.
    pub fn graphics_bytes(&self, order: Order) -> Vec<u8> {
        self.frames.iter().flat_map(|f| order.arrange(&f.rows)).collect()
    }

    pub fn colour_bytes(&self, order: Order) -> Vec<u8> {
        self.frames.iter().flat_map(|f| order.arrange(&f.colours)).collect()
    }

    /// Sprites read back from source: labelled tables whose names say colours are colours, the rest frames; one
    /// table of graphics is cut into frames of `height` (or the height the source gives).
    pub fn from_source(text: &str, height: usize) -> Option<Sprites> {
        let read = Tables::read(text);
        let bottom_first = read.says("bottom line first");
        let arrange = |bytes: &Vec<u8>| if bottom_first { bytes.iter().rev().copied().collect() } else { bytes.clone() };
        let (colours, graphics): (Vec<_>, Vec<_>) = read.tables.iter().partition(|(name, _)| name.to_lowercase().contains("colo"));
        let graphics: Vec<&Vec<u8>> = graphics.iter().map(|(_, b)| b).filter(|b| !b.is_empty()).collect();
        if graphics.is_empty() {
            return None;
        }
        let height = read.equates.iter().find(|(name, _)| name.to_uppercase().ends_with("HEIGHT")).map_or(height, |(_, v)| *v as usize).clamp(1, MOST_LINES);
        let mut frames: Vec<Vec<u8>> = Vec::new();
        if graphics.len() > 1 && graphics.iter().all(|g| g.len() == graphics[0].len()) && graphics[0].len() <= MOST_LINES {
            frames = graphics.iter().map(|g| arrange(g)).collect();
        } else {
            let all: Vec<u8> = graphics.iter().flat_map(|g| g.iter().copied()).collect();
            for chunk in all.chunks(height) {
                let mut rows = chunk.to_vec();
                rows.resize(height, 0);
                frames.push(if bottom_first { rows.into_iter().rev().collect() } else { rows });
            }
        }
        let height = frames[0].len();
        let colour_tables: Vec<Vec<u8>> = colours.iter().map(|(_, b)| arrange(b)).collect();
        let frames = frames
            .into_iter()
            .enumerate()
            .map(|(n, rows)| {
                let mut colours = match colour_tables.len() {
                    0 => vec![0x0E; height],
                    1 => colour_tables[0].clone(),
                    _ => colour_tables.get(n).cloned().unwrap_or_else(|| colour_tables[0].clone()),
                };
                colours.resize(height, colours.last().copied().unwrap_or(0x0E));
                Frame { rows, colours }
            })
            .collect();
        Some(Sprites { height, frames, ..Sprites::default() })
    }

    /// What a frame looks like on the TV: a TIA colour value for each colour clock and line, `width` across.
    pub fn picture(&self, frame: usize, width: usize) -> Vec<Option<u8>> {
        let clocks = self.size.clocks();
        let lines = self.lines.max(1);
        let mut out = vec![None; width * self.height * lines];
        let f = &self.frames[frame];
        for y in 0..self.height * lines {
            for x in 0..(8 * clocks).min(width) {
                if f.rows[y / lines] & (0x80 >> (x / clocks)) != 0 {
                    out[y * width + x] = Some(f.colours[y / lines]);
                }
            }
        }
        out
    }

    /// A whole program for DASM that shows the sprite in the middle of the screen on its background, its frames
    /// played in turn: a 4K cartridge.
    pub fn program(&self, tv: Tv, name: &str) -> String {
        let height = self.height;
        let lines = self.lines.clamp(1, 4);
        let tall = height * lines;
        let visible = tv.lines();
        let above = ((visible.saturating_sub(tall + 1)) / 2).max(1);
        let below = visible.saturating_sub(above + tall + 1).max(1);
        let (vblank, overscan) = tv.blank_lines();
        let frames = self.frames.len();
        let speed = if frames > 1 { self.speed.min(255) } else { 0 };
        let mut out = format!(
            "; machine: atari 2600
; {name}: {frames} frame{plural} of an 8\u{D7}{height} player sprite, {lines} line{lplural} a row, {tv_name}.
; Made by Limoniter's Atari 2600 graphics editor. F9 builds the 4K cartridge and runs it in Stella or MAME.

        processor 6502

VSYNC   = $00
VBLANK  = $01
WSYNC   = $02
NUSIZ0  = $04
COLUP0  = $06
COLUBK  = $09
RESP0   = $10
GRP0    = $1B

HEIGHT  = {height}
FRAMES  = {frames}
SPEED   = {speed}

        seg.u variables
        org $80
shown   ds 1                ; the frame being shown
count   ds 1                ; TV frames until the next
graphic ds 2                ; where its lines are
colour  ds 2                ; and their colours

        seg code
        org $F000
Reset:  sei
        cld
        ldx #$FF
        txs
        lda #0
Clear:  sta $00,x           ; the TIA and RAM cleared
        dex
        bne Clear
        lda #${background:02X}
        sta COLUBK
        lda #{nusiz}
        sta NUSIZ0
        sta WSYNC           ; the sprite placed near the middle
        ldx #8
Place:  dex
        bne Place
        sta RESP0
        jmp Overscan        ; the first frame chosen

; {total} lines a frame, each part counted out with WSYNC: 3 of sync, {vblank} of vertical blank, {visible} of
; picture and {overscan} of overscan.
Frame:  lda #2
        sta WSYNC           ; the end of the overscan
        sta VSYNC           ; three lines of vertical sync
        sta WSYNC
        sta WSYNC
        sta WSYNC
        lda #0
        sta VSYNC
        ldx #{vblank}
VBlank: sta WSYNC
        dex
        bne VBlank
        sta VBLANK          ; A is 0: the picture on

        ldx #{above}            ; lines above it
Above:  sta WSYNC
        dex
        bne Above
        ldy #HEIGHT-1       ; its lines, bottom first in memory
Row:    ldx #{lines}
Line:   sta WSYNC
        lda (graphic),y
        sta GRP0
        lda (colour),y
        sta COLUP0
        dex
        bne Line
        dey
        bpl Row
        sta WSYNC
        lda #0
        sta GRP0
        ldx #{below}            ; and below it
Below:  sta WSYNC
        dex
        bne Below

Overscan:
        lda #2
        sta VBLANK          ; the picture off
",
            total = visible + 3 + vblank + overscan,
            plural = if frames == 1 { "" } else { "s" },
            lplural = if lines == 1 { "" } else { "s" },
            tv_name = tv.name(),
            background = self.background,
            nusiz = self.size.nusiz(),
        );
        if speed > 0 {
            out.push_str(
                "        dec count           ; the next frame when it's time
        bpl Chosen
        lda #SPEED-1
        sta count
        inc shown
        lda shown
        cmp #FRAMES
        bcc Chosen
        lda #0
        sta shown
Chosen:
",
            );
        }
        out.push_str(&format!(
            "        ldx shown           ; where its lines are: all this in the overscan's first line
        lda Graphics,x
        sta graphic
        lda Graphics+FRAMES,x
        sta graphic+1
        lda Colours,x
        sta colour
        lda Colours+FRAMES,x
        sta colour+1
        ldx #{rest}            ; the rest of the overscan but its last line, which Frame ends
Over:   sta WSYNC
        dex
        bne Over
        jmp Frame

; Where each frame's lines and colours are: the low bytes, then the high.
Graphics:
",
            rest = overscan - 1
        ));
        let list = |prefix: &str, part: &str| (0..frames).map(|n| format!("{part}{prefix}{n}")).collect::<Vec<_>>().join(",");
        out.push_str(&format!("        .byte {}\n        .byte {}\nColours:\n", list("Frame", "<"), list("Frame", ">")));
        out.push_str(&format!("        .byte {}\n        .byte {}\n\n; Each frame bottom line first, as Y counts down.\n", list("Colour", "<"), list("Colour", ">")));
        for (n, frame) in self.frames.iter().enumerate() {
            out.push_str(&format!("Frame{n}:\n"));
            byte_lines(&mut out, &Order::BottomFirst.arrange(&frame.rows), 1, true);
            out.push_str(&format!("Colour{n}:\n"));
            byte_lines(&mut out, &Order::BottomFirst.arrange(&frame.colours), 8, false);
        }
        out.push_str("\n        org $FFFC\n        .word Reset\n        .word Reset\n");
        out
    }
}

// ---------- The playfield ----------

/// How the right half of the playfield comes from the left.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Halves {
    /// CTRLPF's reflect bit: the right half is the left the other way round.
    Mirrored,
    /// The left half again.
    Repeated,
    /// Two halves of their own, the registers written again as the line is drawn.
    Asymmetric,
}

impl Halves {
    pub const ALL: [Halves; 3] = [Halves::Mirrored, Halves::Repeated, Halves::Asymmetric];

    pub fn name(self) -> &'static str {
        match self {
            Halves::Mirrored => "Mirrored",
            Halves::Repeated => "Repeated",
            Halves::Asymmetric => "Two halves (asymmetric)",
        }
    }

    fn words(self) -> &'static str {
        match self {
            Halves::Mirrored => "mirrored",
            Halves::Repeated => "repeated",
            Halves::Asymmetric => "two halves",
        }
    }

    /// The block on the left that a block is, if it isn't its own.
    pub fn source_of(self, x: usize) -> usize {
        match self {
            _ if x < 20 => x,
            Halves::Mirrored => 39 - x,
            Halves::Repeated => x - 20,
            Halves::Asymmetric => x,
        }
    }
}

/// The playfield: 40 blocks across and rows of `lines` lines, with each row's colours.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Playfield {
    pub halves: Halves,
    pub rows: usize,
    /// Lines a row.
    pub lines: usize,
    /// Row by row, 40 a row.
    pub pixels: Vec<bool>,
    /// COLUPF for each row.
    pub colours: Vec<u8>,
    /// COLUBK for each row.
    pub backgrounds: Vec<u8>,
}

impl Default for Playfield {
    fn default() -> Self {
        Playfield::new(Halves::Mirrored, 24, 8)
    }
}

/// A half's 20 blocks as PF0 (its top four bits, the left block in bit 4), PF1 (the left in bit 7) and PF2 (the
/// left in bit 0).
pub fn registers(blocks: &[bool]) -> [u8; 3] {
    let mut pf = [0u8; 3];
    for (i, &on) in blocks.iter().enumerate().take(20) {
        if on {
            match i {
                0..4 => pf[0] |= 0x10 << i,
                4..12 => pf[1] |= 0x80 >> (i - 4),
                _ => pf[2] |= 1 << (i - 12),
            }
        }
    }
    pf
}

/// The 20 blocks the three registers show, left to right.
pub fn blocks_of(pf: [u8; 3]) -> [bool; 20] {
    let mut out = [false; 20];
    for (i, block) in out.iter_mut().enumerate() {
        *block = match i {
            0..4 => pf[0] & (0x10 << i) != 0,
            4..12 => pf[1] & (0x80 >> (i - 4)) != 0,
            _ => pf[2] & (1 << (i - 12)) != 0,
        };
    }
    out
}

impl Playfield {
    pub fn new(halves: Halves, rows: usize, lines: usize) -> Playfield {
        Playfield { halves, rows, lines, pixels: vec![false; 40 * rows], colours: vec![0x0E; rows], backgrounds: vec![0x80; rows] }
    }

    pub fn pixel(&self, x: usize, y: usize) -> bool {
        self.pixels[y * 40 + x]
    }

    /// A block set, and the one it shows as on the other half when the halves aren't apart.
    pub fn set(&mut self, x: usize, y: usize, on: bool) {
        if x >= 40 || y >= self.rows {
            return;
        }
        let left = self.halves.source_of(x);
        self.pixels[y * 40 + left] = on;
        if self.halves != Halves::Asymmetric {
            let other = match self.halves {
                Halves::Mirrored => 39 - left,
                _ => left + 20,
            };
            self.pixels[y * 40 + other] = on;
        }
    }

    /// The right half made from the left again, after the halves change.
    pub fn set_halves(&mut self, halves: Halves) {
        self.halves = halves;
        for x in 20..40 {
            let from = halves.source_of(x);
            for y in 0..self.rows {
                self.pixels[y * 40 + x] = self.pixels[y * 40 + from];
            }
        }
    }

    /// More or fewer rows, keeping those at the top; new ones blank in the last row's colours.
    pub fn set_rows(&mut self, rows: usize) {
        let rows = rows.max(1);
        let colour = self.colours.last().copied().unwrap_or(0x0E);
        let background = self.backgrounds.last().copied().unwrap_or(0x80);
        self.rows = rows;
        self.pixels.resize(40 * rows, false);
        self.colours.resize(rows, colour);
        self.backgrounds.resize(rows, background);
    }

    /// The six registers' values for a row: PF0, PF1 and PF2 for the left half, then for the right.
    pub fn row_registers(&self, y: usize) -> [u8; 6] {
        let row = &self.pixels[y * 40..y * 40 + 40];
        let left = registers(&row[..20]);
        let right = if self.halves == Halves::Asymmetric { registers(&row[20..]) } else { left };
        [left[0], left[1], left[2], right[0], right[1], right[2]]
    }

    /// The tables' names after the label, and which registers each holds.
    fn table_names(&self) -> Vec<(&'static str, usize)> {
        let mut names = vec![("pf0", 0), ("pf1", 1), ("pf2", 2)];
        if self.halves == Halves::Asymmetric {
            names.extend([("pf0r", 3), ("pf1r", 4), ("pf2r", 5)]);
        }
        names
    }

    /// DASM source: a table for each register (and for the right half's, when it's its own), a byte a row in
    /// binary, then each row's colour and background.
    pub fn source(&self, label: &str, order: Order) -> String {
        let mut out = format!(
            "; Atari 2600 playfield: {}, {} rows of {} line{} ({} lines), {}.\n",
            self.halves.words(),
            self.rows,
            self.lines,
            if self.lines == 1 { "" } else { "s" },
            self.rows * self.lines,
            order.words()
        );
        out.push_str(match self.halves {
            Halves::Mirrored => "; Set CTRLPF's bit 0 to mirror the right half.\n",
            Halves::Repeated => "; Leave CTRLPF's bit 0 clear to repeat the left half.\n",
            Halves::Asymmetric => "; CTRLPF's bit 0 clear; the r tables are written again in the middle of each line.\n",
        });
        let upper = label.to_uppercase();
        out.push_str(&format!("{upper}_ROWS = {}\n{upper}_LINES = {}\n", self.rows, self.lines));
        let rows: Vec<[u8; 6]> = (0..self.rows).map(|y| self.row_registers(y)).collect();
        for (name, index) in self.table_names() {
            out.push_str(&format!("{label}_{name}:\n"));
            let bytes: Vec<u8> = order.arrange(&rows).iter().map(|r| r[index]).collect();
            byte_lines(&mut out, &bytes, 4, true);
        }
        out.push_str(&format!("{label}_colours:\n"));
        byte_lines(&mut out, &order.arrange(&self.colours), 8, false);
        out.push_str(&format!("{label}_background:\n"));
        byte_lines(&mut out, &order.arrange(&self.backgrounds), 8, false);
        out
    }

    /// The tables as one block of bytes, each `rows` long, in the order the source has them.
    pub fn bytes(&self, order: Order) -> Vec<u8> {
        let rows: Vec<[u8; 6]> = order.arrange(&(0..self.rows).map(|y| self.row_registers(y)).collect::<Vec<_>>());
        let mut out = Vec::new();
        for (_, index) in self.table_names() {
            out.extend(rows.iter().map(|r| r[index]));
        }
        out.extend(order.arrange(&self.colours));
        out.extend(order.arrange(&self.backgrounds));
        out
    }

    /// A playfield read back from source as `source` writes it (or any with tables named pf0, pf1 and pf2).
    pub fn from_source(text: &str) -> Option<Playfield> {
        let read = Tables::read(text);
        let bottom_first = read.says("bottom line first");
        let halves = if read.says("two halves") || read.says("asymmetric") {
            Halves::Asymmetric
        } else if read.says("repeated") {
            Halves::Repeated
        } else {
            Halves::Mirrored
        };
        let find = |suffixes: &[&str]| read.tables.iter().find(|(name, _)| suffixes.iter().any(|s| name.to_lowercase().ends_with(s))).map(|(_, b)| if bottom_first { b.iter().rev().copied().collect() } else { b.clone() });
        let (pf0, pf1, pf2) = (find(&["pf0", "pf0l"])?, find(&["pf1", "pf1l"])?, find(&["pf2", "pf2l"])?);
        let rows = pf0.len().min(pf1.len()).min(pf2.len());
        if rows == 0 {
            return None;
        }
        let right = if halves == Halves::Asymmetric { Some((find(&["pf0r"])?, find(&["pf1r"])?, find(&["pf2r"])?)) } else { None };
        let lines = read.equates.iter().find(|(n, _)| n.to_uppercase().ends_with("_LINES")).map_or(1, |(_, v)| (*v as usize).max(1));
        let mut field = Playfield::new(halves, rows, lines);
        for y in 0..rows {
            let left = blocks_of([pf0[y], pf1[y], pf2[y]]);
            for (x, &on) in left.iter().enumerate() {
                field.set(x, y, on);
            }
            if let Some((r0, r1, r2)) = &right {
                let blocks = blocks_of([r0.get(y).copied().unwrap_or(0), r1.get(y).copied().unwrap_or(0), r2.get(y).copied().unwrap_or(0)]);
                for (x, &on) in blocks.iter().enumerate() {
                    field.set(20 + x, y, on);
                }
            }
        }
        if let Some(colours) = find(&["colours", "colors", "colour", "color"]) {
            for (y, c) in colours.into_iter().take(rows).enumerate() {
                field.colours[y] = c;
            }
        }
        if let Some(backgrounds) = find(&["background", "backgrounds", "colubk"]) {
            for (y, c) in backgrounds.into_iter().take(rows).enumerate() {
                field.backgrounds[y] = c;
            }
        }
        Some(field)
    }

    /// Where the rows start on the TV, to be in the middle of it.
    pub fn top(&self, tv: Tv) -> usize {
        tv.lines().saturating_sub(self.rows * self.lines) / 2
    }

    /// What the TV shows: a colour value for each of 160 colour clocks on each line of the picture, the rows in the
    /// middle and black above and below.
    #[cfg(test)]
    pub fn picture(&self, tv: Tv) -> Vec<u8> {
        let lines = tv.lines();
        let top = self.top(tv);
        let mut out = vec![0u8; 160 * lines];
        for line in 0..lines {
            let Some(y) = line.checked_sub(top).map(|l| l / self.lines.max(1)).filter(|&y| y < self.rows) else { continue };
            for x in 0..160 {
                out[line * 160 + x] = if self.pixel(x / 4, y) { self.colours[y] } else { self.backgrounds[y] };
            }
        }
        out
    }

    /// A whole program for DASM that shows the playfield, a 4K cartridge. Every line writes all six registers
    /// (the right half's in the middle of the line, between the left half's being drawn and the right's), and its
    /// colours before it starts, from tables of a byte a line, so any rows and colours work alike.
    pub fn program(&self, tv: Tv, name: &str) -> String {
        let lines = tv.lines();
        let (vblank, overscan) = tv.blank_lines();
        let top = self.top(tv);
        let rows: Vec<Option<[u8; 6]>> = (0..lines)
            .map(|line| line.checked_sub(top).map(|l| l / self.lines.max(1)).filter(|&y| y < self.rows).map(|y| self.row_registers(y)))
            .collect();
        let colour = |line: usize, table: &[u8]| line.checked_sub(top).map(|l| l / self.lines.max(1)).and_then(|y| table.get(y).copied()).unwrap_or(0);
        let ctrlpf = u8::from(self.halves == Halves::Mirrored);
        let mut out = format!(
            "; machine: atari 2600
; {name}: a {halves} playfield of {count} rows of {each} line{plural}, {tv_name}.
; Made by Limoniter's Atari 2600 graphics editor. F9 builds the 4K cartridge and runs it in Stella or MAME.

        processor 6502

VSYNC   = $00
VBLANK  = $01
WSYNC   = $02
COLUPF  = $08
COLUBK  = $09
CTRLPF  = $0A
PF0     = $0D
PF1     = $0E
PF2     = $0F

LINES   = {lines}

        seg code
        org $F000
; A byte for each line of the picture, the last line first, as Y counts down; a page each, so that reading
; them never takes a cycle more.
",
            halves = self.halves.words(),
            count = self.rows,
            each = self.lines,
            plural = if self.lines == 1 { "" } else { "s" },
            tv_name = tv.name(),
        );
        let names = ["Pf0Left", "Pf1Left", "Pf2Left", "Pf0Right", "Pf1Right", "Pf2Right"];
        for (index, label) in names.iter().enumerate() {
            out.push_str(&format!("        align 256\n{label}:\n"));
            let bytes: Vec<u8> = rows.iter().rev().map(|r| r.map_or(0, |r| r[index])).collect();
            byte_lines(&mut out, &bytes, 8, true);
        }
        for (label, table) in [("Colours", &self.colours), ("Backgrounds", &self.backgrounds)] {
            out.push_str(&format!("        align 256\n{label}:\n"));
            let bytes: Vec<u8> = (0..lines).rev().map(|line| colour(line, table)).collect();
            byte_lines(&mut out, &bytes, 16, false);
        }
        out.push_str(&format!(
            "
        align 256
Reset:  sei
        cld
        ldx #$FF
        txs
        lda #0
Clear:  sta $00,x           ; the TIA and RAM cleared
        dex
        bne Clear
        lda #{ctrlpf}
        sta CTRLPF          ; {reflect}
        jmp Overscan

; {total} lines a frame, each part counted out with WSYNC: 3 of sync, {vblank} of vertical blank (the last
; black with the picture on, while the first line's colours are read), {lines} of picture and {overscan} of
; overscan.
Frame:  lda #2
        sta WSYNC           ; the end of the overscan
        sta VSYNC           ; three lines of vertical sync
        sta WSYNC
        sta WSYNC
        sta WSYNC
        lda #0
        sta VSYNC
        ldx #{blank}
VBlank: sta WSYNC
        dex
        bne VBlank
        sta VBLANK          ; A is 0: the picture on, and this line black
        ldy #LINES-1        ; the top line's colours ready
        lda Colours,y
        ldx Backgrounds,y

; Each line, its cycles counted from the end of the WSYNC: the colours while the beam is off the picture, the
; left half's registers before each is drawn (PF0 by cycle 22, PF1 by 28, PF2 by 38), and the right half's once
; the left's are drawn and before the right's are (PF0 between 28 and 49, PF1 between 39 and 54, PF2 between 49
; and 65).
Line:   sta WSYNC
        sta COLUPF          ; 3
        stx COLUBK          ; 6
        lda Pf0Left,y       ; 10
        sta PF0             ; 13
        lda Pf1Left,y       ; 17
        sta PF1             ; 20
        lda Pf2Left,y       ; 24
        sta PF2             ; 27
        lda Pf0Right,y      ; 31
        nop                 ; 33
        nop                 ; 35
        sta PF0             ; 38
        lda Pf1Right,y      ; 42
        sta PF1             ; 45
        lda Pf2Right,y      ; 49
        dey                 ; 51  the next line's from here
        sta PF2             ; 54
        lda Colours,y       ; 58
        ldx Backgrounds,y   ; 62
        cpy #$FF            ; 64
        bne Line            ; 67

        sta WSYNC
        lda #0
        sta PF0
        sta PF1
        sta PF2
        sta COLUBK
Overscan:
        lda #2
        sta VBLANK          ; the picture off
        ldx #{rest}            ; the overscan but its last line, which Frame ends
Over:   sta WSYNC
        dex
        bne Over
        jmp Frame

        org $FFFC
        .word Reset         ; where it starts
        .word Reset
",
            reflect = if ctrlpf == 1 { "the right half mirrored" } else { "the right half not mirrored" },
            total = lines + 3 + vblank + overscan,
            blank = vblank - 1,
            rest = overscan - 1,
        ));
        out
    }

    /// A picture made into a playfield of this shape: each row's two colours chosen from the TV's, the pair that
    /// shows its blocks best (both halves' pixels counted for a block that shows on both), and each block whichever
    /// of the two is nearer. With `one_background`, every row has the same background colour, the best for all.
    pub fn from_picture(image: &Image, tv: Tv, halves: Halves, rows: usize, lines: usize, fit: Fit, one_background: bool) -> Playfield {
        let rows = rows.clamp(1, tv.lines());
        let lines = lines.clamp(1, tv.lines() / rows);
        let aspect = 4.0 * tv.aspect() / lines as f32;
        let pixels: Vec<[f32; 3]> = quantise::scale(image, 40, rows, aspect, fit, Filter::Box).into_iter().map(quantise::oklab).collect();
        let values = tv.distinct();
        let labs: Vec<[f32; 3]> = values.iter().map(|&v| quantise::oklab(quantise::as_f32(tv.rgb(v)))).collect();
        let blocks = if halves == Halves::Asymmetric { 40 } else { 20 };
        let covers = |b: usize| -> Vec<usize> {
            match halves {
                Halves::Asymmetric => vec![b],
                Halves::Mirrored => vec![b, 39 - b],
                Halves::Repeated => vec![b, b + 20],
            }
        };
        // For each row, each block's cost in each colour.
        let costs: Vec<Vec<Vec<f32>>> = (0..rows)
            .map(|y| (0..blocks).map(|b| labs.iter().map(|lab| covers(b).iter().map(|&x| quantise::distance(pixels[y * 40 + x], *lab)).sum()).collect()).collect())
            .collect();
        let row_cost = |y: usize, back: usize, fore: usize| -> f32 { costs[y].iter().map(|c| c[back].min(c[fore])).sum() };
        let best_fore = |y: usize, back: usize| -> (usize, f32) { (0..labs.len()).map(|f| (f, row_cost(y, back, f))).fold((back, f32::MAX), |a, b| if b.1 < a.1 { b } else { a }) };
        let mut choice: Vec<(usize, usize)> = Vec::with_capacity(rows);
        if one_background {
            let back = (0..labs.len()).map(|b| (b, (0..rows).map(|y| best_fore(y, b).1).sum::<f32>())).fold((0, f32::MAX), |a, b| if b.1 < a.1 { b } else { a }).0;
            choice.extend((0..rows).map(|y| (back, best_fore(y, back).0)));
        } else {
            for (y, row) in costs.iter().enumerate() {
                let mut best = (0, 0, f32::MAX);
                for a in 0..labs.len() {
                    for b in a..labs.len() {
                        let cost = row_cost(y, a, b);
                        if cost < best.2 {
                            best = (a, b, cost);
                        }
                    }
                }
                // The colour more blocks are becomes the background.
                let (a, b) = (best.0, best.1);
                let a_count = row.iter().filter(|c| c[a] <= c[b]).count();
                choice.push(if a_count * 2 >= blocks { (a, b) } else { (b, a) });
            }
        }
        let mut field = Playfield::new(halves, rows, lines);
        for (y, &(back, fore)) in choice.iter().enumerate() {
            field.backgrounds[y] = values[back];
            field.colours[y] = values[fore];
            for (b, cost) in costs[y].iter().enumerate() {
                field.set(b, y, back != fore && cost[fore] < cost[back]);
            }
        }
        field
    }
}

// ---------- Reading tables from source ----------

/// Labelled tables of bytes read from source, the equates, and the comments.
struct Tables {
    tables: Vec<(String, Vec<u8>)>,
    equates: Vec<(String, i64)>,
    comments: String,
}

fn number(word: &str) -> Option<i64> {
    let word = word.trim().trim_start_matches('#');
    if let Some(bin) = word.strip_prefix('%').or_else(|| word.strip_prefix("0b")) {
        i64::from_str_radix(bin, 2).ok()
    } else if let Some(hex) = word.strip_prefix('$').or_else(|| word.strip_prefix("0x")).or_else(|| word.strip_prefix('&')) {
        i64::from_str_radix(hex, 16).ok()
    } else if let Some(hex) = word.strip_suffix('h').or_else(|| word.strip_suffix('H')) {
        i64::from_str_radix(hex, 16).ok()
    } else {
        word.parse().ok()
    }
}

impl Tables {
    fn read(text: &str) -> Tables {
        let mut tables: Vec<(String, Vec<u8>)> = Vec::new();
        let mut equates = Vec::new();
        let mut comments = String::new();
        for line in text.lines() {
            let (code, comment) = line.split_once(';').unwrap_or((line, ""));
            comments.push_str(&comment.to_lowercase());
            comments.push('\n');
            if code.trim().is_empty() {
                continue;
            }
            let mut rest = code.trim_end();
            if !rest.starts_with(char::is_whitespace) {
                let end = rest.find(|c: char| c.is_whitespace() || c == ':' || c == '=').unwrap_or(rest.len());
                let label = rest[..end].trim_start_matches('.').to_string();
                rest = rest[end..].trim_start_matches(':').trim_start();
                if let Some(value) = rest.strip_prefix('=').or_else(|| rest.strip_prefix("equ ")).or_else(|| rest.strip_prefix("EQU ")) {
                    if let Some(v) = number(value) {
                        equates.push((label, v));
                    }
                    continue;
                }
                tables.push((label, Vec::new()));
            }
            let rest = rest.trim();
            let (directive, operands) = rest.split_once(char::is_whitespace).unwrap_or((rest, ""));
            let directive = directive.trim_start_matches(['.', '!']).to_lowercase();
            if !matches!(directive.as_str(), "byte" | "byt" | "db" | "defb" | "dc.b" | "dc" | "fcb") {
                continue;
            }
            let bytes: Vec<u8> = operands.split(',').filter_map(number).map(|v| v as u8).collect();
            match tables.last_mut() {
                Some((_, table)) => table.extend(bytes),
                None => tables.push((String::new(), bytes)),
            }
        }
        tables.retain(|(_, t)| !t.is_empty());
        Tables { tables, equates, comments }
    }

    fn says(&self, words: &str) -> bool {
        self.comments.contains(words)
    }
}

#[cfg(test)]
mod tests;
