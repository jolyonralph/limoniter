//! The tile map and level editor's workings: a set of 8×8 characters, tiles made of so many of them across and
//! down (1×1 for a plain character map, 2×2 for the 16×16 "metatiles" CharPad and many games use), and maps of tile
//! numbers in one or more layers. Characters are drawn in a machine's own colours: the Commodore 64's text and
//! bitmap modes, the ZX Spectrum's with an attribute to each character, and any number of bits a pixel laid out as
//! one machine or another keeps them (the Game Boy's and Master System's planes row by row, the NES's and Amiga's
//! plane after plane, or chunky).
//!
//! The map is painted, filled, picked from, and copied and pasted a block at a time; the whole goes into the
//! source in any of the table generator's forms or out as binary files, and is kept in a text file of its own
//! (described at `to_text`) or in CharPad's .ctm (see `tile_ctm`).

use crate::table_gen::{Base, Form, Size};
use eframe::egui::Color32;

/// Pixels across and down a character.
pub const CHAR: usize = 8;

/// How the planes of a pixel's bits are laid out in a character's bytes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Layout {
    /// The bits of each pixel together, the left pixel in the top bits.
    Chunky,
    /// Each row in turn, a byte of each plane for it: the Game Boy (2 bits) and Master System or Game Gear (4).
    RowPlanes,
    /// Each plane's eight rows in turn: the NES (2 bits), or a character's Amiga bitplanes one after another.
    CharPlanes,
}

/// What the characters are drawn in.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    C64Hires,
    C64Multicolour,
    /// Extended background colour: 64 shapes, the top two bits of a character's number choosing one of four
    /// backgrounds.
    C64Ecm,
    C64BitmapHires,
    C64BitmapMulticolour,
    /// A bit a pixel, and an attribute (ink, paper, bright) to colour each character.
    Spectrum,
    /// So many bits a pixel (1, 2, 4 or 8), each a palette entry.
    Bits(u8, Layout),
}

impl Mode {
    pub const ALL: [Mode; 14] = [
        Mode::C64Hires,
        Mode::C64Multicolour,
        Mode::C64Ecm,
        Mode::C64BitmapHires,
        Mode::C64BitmapMulticolour,
        Mode::Spectrum,
        Mode::Bits(1, Layout::Chunky),
        Mode::Bits(2, Layout::RowPlanes),
        Mode::Bits(2, Layout::CharPlanes),
        Mode::Bits(2, Layout::Chunky),
        Mode::Bits(4, Layout::RowPlanes),
        Mode::Bits(4, Layout::CharPlanes),
        Mode::Bits(4, Layout::Chunky),
        Mode::Bits(8, Layout::Chunky),
    ];

    pub fn name(self) -> &'static str {
        match self {
            Mode::C64Hires => "C64 hires text",
            Mode::C64Multicolour => "C64 multicolour text",
            Mode::C64Ecm => "C64 extended background colour text (ECM)",
            Mode::C64BitmapHires => "C64 hires bitmap",
            Mode::C64BitmapMulticolour => "C64 multicolour bitmap",
            Mode::Spectrum => "ZX Spectrum, with attributes",
            Mode::Bits(1, _) => "1 bit a pixel",
            Mode::Bits(2, Layout::RowPlanes) => "2 bits a pixel, Game Boy (planes row by row)",
            Mode::Bits(2, Layout::CharPlanes) => "2 bits a pixel, NES (one plane, then the other)",
            Mode::Bits(2, _) => "2 bits a pixel, chunky",
            Mode::Bits(4, Layout::RowPlanes) => "4 bits a pixel, Master System and Game Gear (planes row by row)",
            Mode::Bits(4, Layout::CharPlanes) => "4 bits a pixel, Amiga bitplanes (one after another)",
            Mode::Bits(4, _) => "4 bits a pixel, chunky (Mega Drive, MSX2, Next tiles)",
            Mode::Bits(..) => "8 bits a pixel, chunky",
        }
    }

    /// The name it has in the editor's own file.
    fn id(self) -> String {
        match self {
            Mode::C64Hires => "c64-hires".into(),
            Mode::C64Multicolour => "c64-multicolour".into(),
            Mode::C64Ecm => "c64-ecm".into(),
            Mode::C64BitmapHires => "c64-bitmap-hires".into(),
            Mode::C64BitmapMulticolour => "c64-bitmap-multicolour".into(),
            Mode::Spectrum => "spectrum".into(),
            Mode::Bits(bits, layout) => format!(
                "bits-{bits}-{}",
                match layout {
                    Layout::Chunky => "chunky",
                    Layout::RowPlanes => "row-planes",
                    Layout::CharPlanes => "char-planes",
                }
            ),
        }
    }

    fn from_id(id: &str) -> Option<Mode> {
        Mode::ALL.into_iter().find(|m| m.id() == id).or_else(|| {
            // Any width the layouts take, not only those in the list.
            let rest = id.strip_prefix("bits-")?;
            let (bits, layout) = rest.split_once('-')?;
            let bits: u8 = bits.parse().ok().filter(|b| [1, 2, 4, 8].contains(b))?;
            let layout = match layout {
                "chunky" => Layout::Chunky,
                "row-planes" => Layout::RowPlanes,
                "char-planes" => Layout::CharPlanes,
                _ => return None,
            };
            Some(Mode::Bits(bits, layout))
        })
    }

    /// The bits each of a character's 64 pixels holds.
    pub fn bits(self) -> u8 {
        match self {
            Mode::Bits(bits, _) => bits,
            _ => 1,
        }
    }

    pub fn c64(self) -> bool {
        matches!(self, Mode::C64Hires | Mode::C64Multicolour | Mode::C64Ecm | Mode::C64BitmapHires | Mode::C64BitmapMulticolour)
    }

    /// CharPad's number for the display mode.
    pub fn ctm_display(self) -> Option<u8> {
        Some(match self {
            Mode::C64Hires => 0,
            Mode::C64Multicolour => 1,
            Mode::C64Ecm => 2,
            Mode::C64BitmapHires => 3,
            Mode::C64BitmapMulticolour => 4,
            _ => return None,
        })
    }

    pub fn from_ctm_display(display: u8) -> Option<Mode> {
        Some(match display {
            0 => Mode::C64Hires,
            1 => Mode::C64Multicolour,
            2 => Mode::C64Ecm,
            3 => Mode::C64BitmapHires,
            4 => Mode::C64BitmapMulticolour,
            _ => return None,
        })
    }

    /// The bytes a character takes.
    pub fn char_bytes(self) -> usize {
        CHAR * usize::from(self.bits())
    }

    /// The palette a mode of so many bits starts with: black and white; the Game Boy's four greens; sixteen
    /// colours as the PC's EGA has them; the Next's 256.
    pub fn default_palette(self) -> Vec<[u8; 3]> {
        match self.bits() {
            1 => vec![[0, 0, 0], [255, 255, 255]],
            2 => vec![[0xE0, 0xF8, 0xD0], [0x88, 0xC0, 0x70], [0x34, 0x68, 0x56], [0x08, 0x18, 0x20]],
            4 => (0..16u8)
                .map(|i| {
                    let low = |bit: u8| if i & bit != 0 { 0xAA } else { 0 };
                    let bright = if i & 8 != 0 { 0x55 } else { 0 };
                    let mut rgb = [low(4) + bright, low(2) + bright, low(1) + bright];
                    // EGA's brown, rather than dark yellow.
                    if i == 6 {
                        rgb[1] = 0x55;
                    }
                    rgb
                })
                .collect(),
            _ => (0..=255u8).map(|i| {
                let c = crate::pixel_editor::next_colour(i);
                [c.r(), c.g(), c.b()]
            }).collect(),
        }
    }
}

/// Whose colours a character is drawn in, as CharPad has it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Colouring {
    /// The same colours everywhere.
    Project,
    /// Each tile its own.
    Tile,
    /// Each character its own.
    Char,
}

impl Colouring {
    pub const ALL: [Colouring; 3] = [Colouring::Project, Colouring::Tile, Colouring::Char];

    pub fn name(self) -> &'static str {
        match self {
            Colouring::Project => "The same everywhere",
            Colouring::Tile => "Each tile its own",
            Colouring::Char => "Each character its own",
        }
    }

    fn id(self) -> &'static str {
        match self {
            Colouring::Project => "project",
            Colouring::Tile => "tile",
            Colouring::Char => "char",
        }
    }

    pub fn ctm(self) -> u8 {
        match self {
            Colouring::Project => 0,
            Colouring::Tile => 1,
            Colouring::Char => 2,
        }
    }

    pub fn from_ctm(value: u8) -> Colouring {
        match value {
            0 => Colouring::Project,
            1 => Colouring::Tile,
            _ => Colouring::Char,
        }
    }
}

/// A set of colours: on the C64 the colour RAM's colour, then the screen's low and high nibbles (the two colours of a
/// bitmap cell); on the Spectrum the attribute byte first.
pub type Colours = [u8; 3];

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Char {
    /// 64 pixels, top row first, each the value of its bits (on the C64 and Spectrum, a bit each).
    pub pixels: [u8; 64],
    pub colours: Colours,
    /// CharPad's material, 0 to 15: what a game takes the character to be (wall, ladder and so on).
    pub material: u8,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Tile {
    /// The characters it's made of, across then down.
    pub cells: Vec<u16>,
    pub colours: Colours,
    /// CharPad's tag, a number a game can use.
    pub tag: u8,
    pub name: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Layer {
    pub name: String,
    /// Tile numbers, across then down.
    pub cells: Vec<u16>,
    pub shown: bool,
}

/// What CharPad keeps that the editor doesn't use, kept to write back: the flexi-grid's width, height and settings,
/// and the key map.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CtmKept {
    pub flexigrid: [u8; 5],
    pub key_map: u8,
}

impl Default for CtmKept {
    fn default() -> Self {
        // CharPad's own: 10 by 10, off, a thick line.
        CtmKept { flexigrid: [10, 0, 10, 0, 0x40], key_map: 0 }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Project {
    pub mode: Mode,
    pub colouring: Colouring,
    /// The C64's background colours, $D021 to $D024 ($D022 and $D023 are multicolour 1 and 2).
    pub backgrounds: [u8; 4],
    /// The colours used everywhere when colouring is the same everywhere.
    pub colours: Colours,
    /// For the modes of so many bits: each value's colour.
    pub palette: Vec<[u8; 3]>,
    pub chars: Vec<Char>,
    /// Characters across and down a tile.
    pub tile_size: (usize, usize),
    pub tiles: Vec<Tile>,
    /// Tiles across and down the map.
    pub map_size: (usize, usize),
    pub layers: Vec<Layer>,
    pub ctm: CtmKept,
}

/// A block of tile numbers, copied from the map or chosen in the tile set, to paint with or paste.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Block {
    pub width: usize,
    pub height: usize,
    pub cells: Vec<u16>,
}

impl Block {
    pub fn one(tile: u16) -> Block {
        Block { width: 1, height: 1, cells: vec![tile] }
    }
}

/// A rectangle of the map: x, y, width and height in tiles.
pub type Area = (usize, usize, usize, usize);

/// What goes into the source, or out as binary files.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Part {
    /// The characters' bytes, as the machine keeps them.
    Chars,
    /// Each tile's character numbers.
    Tiles,
    /// The colours of each character or tile (as colouring is), as the machine takes them.
    Colours,
    /// Each character's material.
    Materials,
    /// The map: rows of tile numbers, each layer in turn.
    Map,
    /// The map with its tiles laid out as characters, as a screen holds them.
    Screen,
    /// The colour of each character of that screen: the C64's colour RAM, or the Spectrum's attributes.
    ScreenColours,
}

impl Part {
    pub const ALL: [Part; 7] = [Part::Chars, Part::Tiles, Part::Colours, Part::Materials, Part::Map, Part::Screen, Part::ScreenColours];

    pub fn name(self) -> &'static str {
        match self {
            Part::Chars => "Characters",
            Part::Tiles => "Tiles",
            Part::Colours => "Colours",
            Part::Materials => "Materials",
            Part::Map => "Map",
            Part::Screen => "Map as characters",
            Part::ScreenColours => "Map's colours",
        }
    }

    pub fn tip(self) -> &'static str {
        match self {
            Part::Chars => "The characters' bytes, as the machine keeps them",
            Part::Tiles => "Each tile's character numbers, across then down",
            Part::Colours => "Each tile's or character's colours, as colouring is",
            Part::Materials => "Each character's material, 0 to 15",
            Part::Map => "Rows of tile numbers, a line a row",
            Part::Screen => "The map with each tile laid out as its characters, a row of characters a line, as a screen holds them",
            Part::ScreenColours => "The colour of each of those characters: the C64's colour RAM, or the Spectrum's attributes",
        }
    }

    /// What's added to the label for this part.
    fn suffix(self) -> &'static str {
        match self {
            Part::Chars => "chars",
            Part::Tiles => "tiles",
            Part::Colours => "colours",
            Part::Materials => "materials",
            Part::Map => "map",
            Part::Screen => "screen",
            Part::ScreenColours => "screen_colours",
        }
    }
}

/// One table of the output: its label, its values, and how many to a line.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Table {
    pub label: String,
    pub values: Vec<i64>,
    pub size: Size,
    pub per_line: usize,
}

impl Table {
    fn new(label: String, values: Vec<i64>, per_line: usize) -> Table {
        // Numbers past 255 need words.
        let size = if values.iter().any(|v| *v > 255) { Size::Word } else { Size::Byte };
        Table { label, values, size, per_line: per_line.max(1) }
    }
}

/// A character's bytes, from its pixels, as the mode lays them out.
pub fn encode_char(pixels: &[u8; 64], mode: Mode) -> Vec<u8> {
    let bits = mode.bits();
    let layout = match mode {
        Mode::Bits(_, layout) => layout,
        _ => Layout::Chunky,
    };
    let mask = ((1u16 << bits) - 1) as u8;
    match layout {
        Layout::Chunky => {
            let per_byte = 8 / usize::from(bits);
            pixels.chunks(per_byte).map(|group| group.iter().fold(0u8, |byte, p| if bits == 8 { *p } else { (byte << bits) | (p & mask) })).collect()
        }
        Layout::RowPlanes => (0..CHAR).flat_map(|row| (0..bits).map(move |plane| plane_byte(pixels, row, plane))).collect(),
        Layout::CharPlanes => (0..bits).flat_map(|plane| (0..CHAR).map(move |row| plane_byte(pixels, row, plane))).collect(),
    }
}

/// One row's byte of one plane: bit `plane` of each pixel, the left pixel in the top bit.
fn plane_byte(pixels: &[u8; 64], row: usize, plane: u8) -> u8 {
    (0..CHAR).fold(0u8, |byte, x| (byte << 1) | ((pixels[row * CHAR + x] >> plane) & 1))
}

/// A character's pixels from its bytes; missing bytes are taken as 0.
pub fn decode_char(bytes: &[u8], mode: Mode) -> [u8; 64] {
    let bits = mode.bits();
    let layout = match mode {
        Mode::Bits(_, layout) => layout,
        _ => Layout::Chunky,
    };
    let byte = |i: usize| bytes.get(i).copied().unwrap_or(0);
    let mask = ((1u16 << bits) - 1) as u8;
    let mut pixels = [0u8; 64];
    for (i, pixel) in pixels.iter_mut().enumerate() {
        let (x, row) = (i % CHAR, i / CHAR);
        *pixel = match layout {
            Layout::Chunky => {
                let per_byte = 8 / usize::from(bits);
                let shift = (per_byte - 1 - i % per_byte) * usize::from(bits);
                (byte(i / per_byte) >> shift) & mask
            }
            Layout::RowPlanes => (0..bits).fold(0, |v, plane| v | (((byte(row * usize::from(bits) + usize::from(plane)) >> (7 - x)) & 1) << plane)),
            Layout::CharPlanes => (0..bits).fold(0, |v, plane| v | (((byte(usize::from(plane) * CHAR + row) >> (7 - x)) & 1) << plane)),
        };
    }
    pixels
}

fn rgb(c: [u8; 3]) -> Color32 {
    Color32::from_rgb(c[0], c[1], c[2])
}

/// One of the C64's sixteen colours, as Pepto measured them.
pub fn c64_colour(index: u8) -> Color32 {
    rgb(crate::c64_gfx::PEPTO[usize::from(index & 15)])
}

impl Default for Project {
    fn default() -> Self {
        Project::new(Mode::C64Multicolour, (2, 2), (40, 25))
    }
}

impl Project {
    /// A blank project: 256 empty characters, 64 tiles all of character 0, and a map of tile 0.
    pub fn new(mode: Mode, tile_size: (usize, usize), map_size: (usize, usize)) -> Project {
        let (tw, th) = (tile_size.0.clamp(1, 16), tile_size.1.clamp(1, 16));
        let (mw, mh) = (map_size.0.max(1), map_size.1.max(1));
        let colours = match mode {
            // Light blue on blue, and multicolour for the cells (8 and up).
            Mode::C64Multicolour => [14 | 8, 0, 1],
            m if m.c64() => [14, 0, 1],
            // Black ink on white paper.
            Mode::Spectrum => [0x38, 0, 0],
            _ => [1, 0, 0],
        };
        let tiles = if (tw, th) == (1, 1) { 256 } else { 64 };
        Project {
            mode,
            colouring: if mode == Mode::Spectrum { Colouring::Char } else { Colouring::Project },
            backgrounds: [6, 11, 12, 0],
            colours,
            palette: mode.default_palette(),
            chars: vec![Char { pixels: [0; 64], colours, material: 0 }; 256],
            tile_size: (tw, th),
            tiles: (0..tiles).map(|t| Tile { cells: if (tw, th) == (1, 1) { vec![t as u16] } else { vec![0; tw * th] }, colours, tag: 0, name: String::new() }).collect(),
            map_size: (mw, mh),
            layers: vec![Layer { name: crate::i18n::tr("Map").into(), cells: vec![0; mw * mh], shown: true }],
            ctm: CtmKept::default(),
        }
    }

    /// The colours a character of a tile shows in, as colouring is.
    pub fn colours_for(&self, tile: usize, char: usize) -> Colours {
        match self.colouring {
            Colouring::Project => self.colours,
            Colouring::Tile => self.tiles.get(tile).map_or(self.colours, |t| t.colours),
            Colouring::Char => self.chars.get(char).map_or(self.colours, |c| c.colours),
        }
    }

    /// Whether a character in these colours has pixels twice as wide, two bits each: the C64's multicolour.
    pub fn wide(&self, colours: Colours) -> bool {
        match self.mode {
            Mode::C64Multicolour => colours[0] & 8 != 0,
            Mode::C64BitmapMulticolour => true,
            _ => false,
        }
    }

    /// The values a pixel can be drawn with, and each one's colour, in these colours.
    pub fn pens(&self, colours: Colours, char: usize) -> Vec<(u8, Color32)> {
        let n = match self.mode {
            Mode::Bits(bits, _) => 1usize << bits,
            _ if self.wide(colours) => 4,
            _ => 2,
        };
        (0..n as u16).map(|v| (v as u8, self.pen_colour(v as u8, colours, char))).collect()
    }

    /// The colour a pixel of a value shows in: for the wide pixels of multicolour, the value of the pair.
    pub fn pen_colour(&self, value: u8, colours: Colours, char: usize) -> Color32 {
        let [ram, low, high] = colours;
        let bg = self.backgrounds;
        match self.mode {
            Mode::C64Hires => c64_colour(if value != 0 { ram } else { bg[0] }),
            Mode::C64Multicolour if ram & 8 != 0 => c64_colour([bg[0], bg[1], bg[2], ram & 7][usize::from(value & 3)]),
            Mode::C64Multicolour => c64_colour(if value != 0 { ram & 7 } else { bg[0] }),
            Mode::C64Ecm => c64_colour(if value != 0 { ram } else { bg[(char >> 6) & 3] }),
            Mode::C64BitmapHires => c64_colour(if value != 0 { high } else { low }),
            // 01 shows the screen's high nibble, 10 its low, 11 the colour RAM.
            Mode::C64BitmapMulticolour => c64_colour([bg[0], high, low, ram][usize::from(value & 3)]),
            Mode::Spectrum => {
                let bright = ram & 0x40 != 0;
                crate::screen_designer::colour(if value != 0 { ram & 7 } else { (ram >> 3) & 7 }, bright)
            }
            Mode::Bits(..) => self.palette.get(usize::from(value)).map_or(Color32::BLACK, |c| rgb(*c)),
        }
    }

    /// The pixels of a character as they show, in these colours. In ECM, a character takes its shape from the
    /// first 64.
    pub fn char_rgba(&self, char: usize, colours: Colours) -> [Color32; 64] {
        let shape = if self.mode == Mode::C64Ecm { char & 63 } else { char };
        let Some(c) = self.chars.get(shape) else {
            return [Color32::from_rgb(255, 0, 255); 64];
        };
        let wide = self.wide(colours);
        std::array::from_fn(|i| {
            let value = if wide {
                let left = i & !1;
                (c.pixels[left] << 1) | c.pixels[left + 1]
            } else {
                c.pixels[i]
            };
            self.pen_colour(value, colours, char)
        })
    }

    /// The pixels of a tile as they show, across then down.
    pub fn tile_rgba(&self, tile: usize) -> Vec<Color32> {
        let (tw, th) = self.tile_size;
        let width = tw * CHAR;
        let mut out = vec![Color32::from_rgb(255, 0, 255); width * th * CHAR];
        let Some(t) = self.tiles.get(tile) else {
            return out;
        };
        for (cell, &char) in t.cells.iter().enumerate().take(tw * th) {
            let char = usize::from(char);
            let pixels = self.char_rgba(char, self.colours_for(tile, char));
            let (cx, cy) = ((cell % tw) * CHAR, (cell / tw) * CHAR);
            for (i, p) in pixels.iter().enumerate() {
                out[(cy + i / CHAR) * width + cx + i % CHAR] = *p;
            }
        }
        out
    }

    /// A pixel's value: for wide pixels, the pair's.
    pub fn pixel(&self, char: usize, x: usize, y: usize, wide: bool) -> u8 {
        let Some(c) = self.chars.get(char) else { return 0 };
        if wide {
            let left = y * CHAR + (x & !1);
            (c.pixels[left] << 1) | c.pixels[left + 1]
        } else {
            c.pixels[y * CHAR + x]
        }
    }

    /// Sets a pixel: for wide pixels, both bits of the pair.
    pub fn set_pixel(&mut self, char: usize, x: usize, y: usize, value: u8, wide: bool) {
        let mode = self.mode;
        let Some(c) = self.chars.get_mut(char) else { return };
        if x >= CHAR || y >= CHAR {
            return;
        }
        if wide {
            let left = y * CHAR + (x & !1);
            c.pixels[left] = (value >> 1) & 1;
            c.pixels[left + 1] = value & 1;
        } else {
            c.pixels[y * CHAR + x] = value & (((1u16 << mode.bits()) - 1) as u8);
        }
    }

    /// A character flipped, turned, shifted or inverted. Multicolour characters' pixels are pairs of bits, moved as
    /// pairs; they aren't turned, being twice as wide as they are tall.
    pub fn transform_char(&mut self, char: usize, how: crate::pixel_editor::Transform, wide: bool) {
        use crate::pixel_editor::{Transform, transform};
        let mask = ((1u16 << self.mode.bits()) - 1) as u8;
        let Some(c) = self.chars.get_mut(char) else { return };
        if how == Transform::Invert {
            for p in &mut c.pixels {
                *p ^= mask;
            }
        } else if wide {
            if how == Transform::Rotate {
                return;
            }
            let pairs: Vec<u8> = (0..32).map(|i| (c.pixels[i * 2] << 1) | c.pixels[i * 2 + 1]).collect();
            for (i, v) in transform(&pairs, CHAR / 2, CHAR, how, 0, 4).into_iter().enumerate() {
                c.pixels[i * 2] = (v >> 1) & 1;
                c.pixels[i * 2 + 1] = v & 1;
            }
        } else {
            let moved = transform(&c.pixels, CHAR, CHAR, how, 0, 2);
            c.pixels.copy_from_slice(&moved);
        }
    }

    /// The character a tile has at a cell.
    pub fn tile_char(&self, tile: usize, cell: usize) -> usize {
        self.tiles.get(tile).and_then(|t| t.cells.get(cell)).map_or(0, |c| usize::from(*c))
    }

    // ---------- The map ----------

    pub fn get(&self, layer: usize, x: usize, y: usize) -> u16 {
        let (w, h) = self.map_size;
        if x >= w || y >= h {
            return 0;
        }
        self.layers.get(layer).map_or(0, |l| l.cells[y * w + x])
    }

    pub fn set(&mut self, layer: usize, x: usize, y: usize, tile: u16) {
        let (w, h) = self.map_size;
        if x < w && y < h
            && let Some(l) = self.layers.get_mut(layer)
        {
            l.cells[y * w + x] = tile;
        }
    }

    /// A block painted with its top left at x, y; what falls off the map is left out.
    pub fn stamp(&mut self, layer: usize, x: usize, y: usize, block: &Block) {
        for by in 0..block.height {
            for bx in 0..block.width {
                self.set(layer, x + bx, y + by, block.cells[by * block.width + bx]);
            }
        }
    }

    /// Fills the cells joined to x, y (across and down, not corner to corner) that hold its tile. With a block,
    /// the block's pattern is laid over the filled cells, lined up with the map's own grid from x, y.
    pub fn fill(&mut self, layer: usize, x: usize, y: usize, block: &Block) {
        let (w, h) = self.map_size;
        if x >= w || y >= h || layer >= self.layers.len() {
            return;
        }
        let from = self.get(layer, x, y);
        let pattern = |cx: usize, cy: usize| {
            let bx = (cx + block.width * w - x) % block.width;
            let by = (cy + block.height * h - y) % block.height;
            block.cells[by * block.width + bx]
        };
        // A pattern of only the tile already there changes nothing, and would loop for ever.
        if block.cells.iter().all(|c| *c == from) {
            return;
        }
        let mut seen = vec![false; w * h];
        let mut todo = vec![(x, y)];
        while let Some((cx, cy)) = todo.pop() {
            if seen[cy * w + cx] || self.get(layer, cx, cy) != from {
                continue;
            }
            seen[cy * w + cx] = true;
            if cx > 0 {
                todo.push((cx - 1, cy));
            }
            if cx + 1 < w {
                todo.push((cx + 1, cy));
            }
            if cy > 0 {
                todo.push((cx, cy - 1));
            }
            if cy + 1 < h {
                todo.push((cx, cy + 1));
            }
        }
        for (i, done) in seen.into_iter().enumerate() {
            if done {
                let (cx, cy) = (i % w, i / w);
                self.set(layer, cx, cy, pattern(cx, cy));
            }
        }
    }

    /// A rectangle of the map, cut to its edges.
    pub fn copy(&self, layer: usize, area: Area) -> Block {
        let (x, y, aw, ah) = self.clip(area);
        Block { width: aw, height: ah, cells: (0..ah).flat_map(|by| (0..aw).map(move |bx| (x + bx, y + by))).map(|(cx, cy)| self.get(layer, cx, cy)).collect() }
    }

    /// Sets a rectangle to tile 0.
    pub fn clear(&mut self, layer: usize, area: Area) {
        let (x, y, aw, ah) = self.clip(area);
        self.stamp(layer, x, y, &Block { width: aw, height: ah, cells: vec![0; aw * ah] });
    }

    fn clip(&self, (x, y, aw, ah): Area) -> Area {
        let (w, h) = self.map_size;
        let (x, y) = (x.min(w), y.min(h));
        (x, y, aw.min(w - x).max(usize::from(x < w)), ah.min(h - y).max(usize::from(y < h)))
    }

    /// A new size for the map, what was there kept at the top left, new cells tile 0.
    pub fn resize_map(&mut self, width: usize, height: usize) {
        let (width, height) = (width.clamp(1, 8192), height.clamp(1, 8192));
        let (w, h) = self.map_size;
        for layer in &mut self.layers {
            let mut cells = vec![0; width * height];
            for y in 0..h.min(height) {
                for x in 0..w.min(width) {
                    cells[y * width + x] = layer.cells[y * w + x];
                }
            }
            layer.cells = cells;
        }
        self.map_size = (width, height);
    }

    /// A new tile size: each tile keeps the characters of its top left, new cells character 0.
    pub fn resize_tiles(&mut self, width: usize, height: usize) {
        let (width, height) = (width.clamp(1, 16), height.clamp(1, 16));
        let (tw, th) = self.tile_size;
        for tile in &mut self.tiles {
            let mut cells = vec![0; width * height];
            for y in 0..th.min(height) {
                for x in 0..tw.min(width) {
                    cells[y * width + x] = tile.cells[y * tw + x];
                }
            }
            tile.cells = cells;
        }
        self.tile_size = (width, height);
    }

    pub fn add_layer(&mut self) {
        let n = self.layers.len() + 1;
        self.layers.push(Layer { name: crate::i18n::tf!("Layer {n}"), cells: vec![0; self.map_size.0 * self.map_size.1], shown: true });
    }

    // ---------- Tiles and characters ----------

    /// A blank tile after `at`, all character 0; the map's numbers past it move up one.
    pub fn insert_tile(&mut self, at: usize, tile: Tile) {
        let at = at.min(self.tiles.len());
        self.tiles.insert(at, tile);
        for layer in &mut self.layers {
            for cell in &mut layer.cells {
                if usize::from(*cell) >= at {
                    *cell += 1;
                }
            }
        }
    }

    /// A tile taken out: where the map had it, tile 0; the numbers past it move down one.
    pub fn remove_tile(&mut self, at: usize) {
        if self.tiles.len() <= 1 || at >= self.tiles.len() {
            return;
        }
        self.tiles.remove(at);
        for layer in &mut self.layers {
            for cell in &mut layer.cells {
                let c = usize::from(*cell);
                if c == at {
                    *cell = 0;
                } else if c > at {
                    *cell -= 1;
                }
            }
        }
    }

    pub fn blank_tile(&self) -> Tile {
        Tile { cells: vec![0; self.tile_size.0 * self.tile_size.1], colours: self.colours, tag: 0, name: String::new() }
    }

    /// Characters given to a tile of its own, copies of those it had, added at the end of the set, so drawing on it
    /// changes no other tile.
    pub fn own_chars(&mut self, tile: usize) -> usize {
        let Some(cells) = self.tiles.get(tile).map(|t| t.cells.clone()) else { return 0 };
        let mut made = 0;
        for (i, char) in cells.into_iter().enumerate() {
            let copy = self.chars.get(usize::from(char)).cloned().unwrap_or(Char { pixels: [0; 64], colours: self.colours, material: 0 });
            if self.chars.len() >= 65536 {
                break;
            }
            self.chars.push(copy);
            self.tiles[tile].cells[i] = (self.chars.len() - 1) as u16;
            made += 1;
        }
        made
    }

    /// Characters no tile uses taken out, and those just like another (the same pixels, and colours where each
    /// has its own, and material) made one; the tiles changed to match. Says how many went.
    pub fn tidy_chars(&mut self) -> usize {
        let before = self.chars.len();
        let mut used = vec![false; before];
        for tile in &self.tiles {
            for cell in &tile.cells {
                if let Some(u) = used.get_mut(usize::from(*cell)) {
                    *u = true;
                }
            }
        }
        // ECM shows only the first 64 shapes, and plain character maps (1×1 tiles numbered as the characters)
        // are left alone, as their numbers are the map's.
        if self.mode == Mode::C64Ecm || self.plain_char_map() {
            return 0;
        }
        let colours_matter = self.colouring == Colouring::Char;
        let mut kept: Vec<Char> = Vec::new();
        let mut renumber = vec![0u16; before];
        for (i, char) in self.chars.iter().enumerate() {
            if !used[i] && i != 0 {
                continue;
            }
            let same = kept.iter().position(|k| k.pixels == char.pixels && k.material == char.material && (!colours_matter || k.colours == char.colours));
            renumber[i] = match same {
                Some(at) => at as u16,
                None => {
                    kept.push(char.clone());
                    (kept.len() - 1) as u16
                }
            };
        }
        for tile in &mut self.tiles {
            for cell in &mut tile.cells {
                *cell = renumber.get(usize::from(*cell)).copied().unwrap_or(0);
            }
        }
        self.chars = kept;
        before - self.chars.len()
    }

    /// 1×1 tiles, tile n being character n: a map of characters, as CharPad has it with its tile system off.
    pub fn plain_char_map(&self) -> bool {
        self.tile_size == (1, 1) && self.tiles.len() == self.chars.len() && self.tiles.iter().enumerate().all(|(i, t)| t.cells == [i as u16])
    }

    // ---------- Out ----------

    /// The tables of the parts asked for, labelled from `label`; parts that have nothing in this mode are left out.
    pub fn tables(&self, label: &str, parts: &[Part]) -> Vec<Table> {
        let label = if label.trim().is_empty() { "level" } else { label.trim() };
        let name = |part: Part| format!("{label}_{}", part.suffix());
        let (mw, mh) = self.map_size;
        let (tw, th) = self.tile_size;
        let mut out = Vec::new();
        for &part in Part::ALL.iter().filter(|p| parts.contains(p)) {
            match part {
                Part::Chars => {
                    let values: Vec<i64> = self.chars.iter().flat_map(|c| encode_char(&c.pixels, self.mode)).map(i64::from).collect();
                    out.push(Table::new(name(part), values, self.mode.char_bytes()));
                }
                Part::Tiles => {
                    let values = self.tiles.iter().flat_map(|t| t.cells.iter().map(|c| i64::from(*c))).collect();
                    out.push(Table::new(name(part), values, tw * th));
                }
                Part::Colours => {
                    let each: Vec<Colours> = match self.colouring {
                        Colouring::Project => continue,
                        Colouring::Tile => self.tiles.iter().map(|t| t.colours).collect(),
                        Colouring::Char => self.chars.iter().map(|c| c.colours).collect(),
                    };
                    for (suffix, values) in self.colour_tables(&each) {
                        out.push(Table::new(format!("{}{suffix}", name(part)), values, 16));
                    }
                }
                Part::Materials => {
                    if self.chars.iter().any(|c| c.material != 0) {
                        out.push(Table::new(name(part), self.chars.iter().map(|c| i64::from(c.material & 15)).collect(), 16));
                    }
                }
                Part::Map => {
                    for (n, layer) in self.layers.iter().enumerate() {
                        let label = if n == 0 { name(part) } else { format!("{}{}", name(part), n + 1) };
                        out.push(Table::new(label, layer.cells.iter().map(|c| i64::from(*c)).collect(), mw));
                    }
                }
                Part::Screen | Part::ScreenColours => {
                    let (sw, sh) = (mw * tw, mh * th);
                    let cell = |x: usize, y: usize| {
                        let tile = usize::from(self.get(0, x / tw, y / th));
                        let char = self.tile_char(tile, (y % th) * tw + x % tw);
                        (tile, char)
                    };
                    if part == Part::Screen {
                        let values = (0..sh).flat_map(|y| (0..sw).map(move |x| (x, y))).map(|(x, y)| cell(x, y).1 as i64).collect();
                        out.push(Table::new(name(part), values, sw));
                    } else {
                        let each: Vec<Colours> = (0..sh).flat_map(|y| (0..sw).map(move |x| (x, y))).map(|(x, y)| {
                            let (tile, char) = cell(x, y);
                            self.colours_for(tile, char)
                        }).collect();
                        for (suffix, values) in self.colour_tables(&each) {
                            out.push(Table::new(format!("{}{suffix}", name(part)), values, sw));
                        }
                    }
                }
            }
        }
        out
    }

    /// Colours as the machine takes them: the C64's colour RAM (text, and multicolour bitmap) and screen bytes
    /// (bitmap: the high nibble the colour of set pixels); the Spectrum's attribute bytes. Nothing for the modes of
    /// so many bits, whose colours are their pixels.
    fn colour_tables(&self, each: &[Colours]) -> Vec<(&'static str, Vec<i64>)> {
        let ram = || -> Vec<i64> { each.iter().map(|c| i64::from(c[0] & 15)).collect() };
        let screen = || -> Vec<i64> { each.iter().map(|c| i64::from((c[2] & 15) << 4 | (c[1] & 15))).collect() };
        match self.mode {
            Mode::C64Hires | Mode::C64Multicolour | Mode::C64Ecm => vec![("", ram())],
            Mode::C64BitmapHires => vec![("", screen())],
            Mode::C64BitmapMulticolour => vec![("", screen()), ("_ram", ram())],
            Mode::Spectrum => vec![("", each.iter().map(|c| i64::from(c[0])).collect())],
            Mode::Bits(..) => Vec::new(),
        }
    }

    /// The parts as source in a form: an assembler's data lines, BASIC DATA lines (numbered on from `first_line`),
    /// or a language's arrays.
    pub fn source(&self, label: &str, parts: &[Part], form: Form, first_line: u32) -> String {
        let mut out = String::new();
        let mut line = first_line;
        for table in self.tables(label, parts) {
            // Decimal for BASIC, as every BASIC reads it; hex for the rest.
            let base = if form == Form::Basic { Base::Decimal } else { Base::Hex };
            let text = if form == Form::Basic {
                let options = crate::table_forms::BasicOptions { first: line, ..Default::default() };
                crate::table_forms::as_basic(&table.values, table.size, base, table.per_line.min(16), &table.label, &options)
            } else {
                crate::table_gen::write(&table.values, table.size, false, base, table.per_line, &table.label, form)
            };
            line += text.lines().count() as u32 * 10;
            if !out.is_empty() && form != Form::Basic {
                out.push('\n');
            }
            out.push_str(&text);
        }
        out
    }

    /// The parts as binary files: each one's name (the label and part) and bytes; words little-endian, as the Z80,
    /// 6502 and x86 have them, or big-endian for the 68000.
    pub fn binaries(&self, label: &str, parts: &[Part], big_endian: bool) -> Vec<(String, Vec<u8>)> {
        self.tables(label, parts).into_iter().map(|t| (format!("{}.bin", t.label), crate::table_gen::as_binary(&t.values, t.size, big_endian))).collect()
    }

    // ---------- The editor's own file ----------

    /// The project as the editor's own text file, a `.ltm`. Each line is a keyword and its values:
    ///
    /// ```text
    /// limoniter-tilemap 1
    /// mode c64-multicolour              (or c64-hires, c64-ecm, c64-bitmap-hires, c64-bitmap-multicolour,
    ///                                    spectrum, bits-N-chunky, bits-N-row-planes, bits-N-char-planes)
    /// colouring project                 (or tile, char)
    /// backgrounds 6 11 12 0             ($D021 to $D024)
    /// colours 14 0 0                    (colour RAM, screen low, screen high; the Spectrum's attribute first)
    /// palette 000000 FFFFFF …           (for bits-N modes: each value's colour)
    /// tile-size 2 2
    /// map-size 40 25
    /// char 14,0,0 0 003C666E6E603C00    (each character in turn: colours, material, its bytes in hex as the
    ///                                    mode lays them out)
    /// tile 14,0,0 0 1 2 3 4 | name      (each tile: colours, tag, its characters; a name after a bar)
    /// layer 1 | Ground                  (1 shown, 0 hidden; then a line of tile numbers for each row)
    /// 0 0 1 1 …
    /// charpad 0A000A0040 0              (CharPad's flexi-grid and key map, kept to write back)
    /// ```
    ///
    /// Blank lines and lines starting with # are passed over.
    pub fn to_text(&self) -> String {
        let colours = |c: Colours| format!("{},{},{}", c[0], c[1], c[2]);
        let hex = |bytes: &[u8]| bytes.iter().map(|b| format!("{b:02X}")).collect::<String>();
        let mut out = String::from("limoniter-tilemap 1\n");
        out.push_str(&format!("mode {}\n", self.mode.id()));
        out.push_str(&format!("colouring {}\n", self.colouring.id()));
        let b = self.backgrounds;
        out.push_str(&format!("backgrounds {} {} {} {}\n", b[0], b[1], b[2], b[3]));
        out.push_str(&format!("colours {}\n", self.colours.map(|c| c.to_string()).join(" ")));
        if matches!(self.mode, Mode::Bits(..)) {
            out.push_str(&format!("palette {}\n", self.palette.iter().map(|c| hex(c)).collect::<Vec<_>>().join(" ")));
        }
        out.push_str(&format!("tile-size {} {}\n", self.tile_size.0, self.tile_size.1));
        out.push_str(&format!("map-size {} {}\n", self.map_size.0, self.map_size.1));
        for c in &self.chars {
            out.push_str(&format!("char {} {} {}\n", colours(c.colours), c.material, hex(&encode_char(&c.pixels, self.mode))));
        }
        for t in &self.tiles {
            let cells: Vec<String> = t.cells.iter().map(u16::to_string).collect();
            out.push_str(&format!("tile {} {} {}", colours(t.colours), t.tag, cells.join(" ")));
            if !t.name.is_empty() {
                out.push_str(&format!(" | {}", t.name.replace('\n', " ")));
            }
            out.push('\n');
        }
        let w = self.map_size.0;
        for layer in &self.layers {
            out.push_str(&format!("layer {} | {}\n", u8::from(layer.shown), layer.name.replace('\n', " ")));
            for row in layer.cells.chunks(w) {
                out.push_str(&row.iter().map(u16::to_string).collect::<Vec<_>>().join(" "));
                out.push('\n');
            }
        }
        out.push_str(&format!("charpad {} {}\n", hex(&self.ctm.flexigrid), self.ctm.key_map));
        out
    }

    /// A project from the editor's own file; what's wrong with it, by its line, if it can't be read.
    pub fn from_text(text: &str) -> Result<Project, String> {
        let mut lines = text.lines().enumerate().map(|(n, l)| (n + 1, l.trim())).filter(|(_, l)| !l.is_empty() && !l.starts_with('#'));
        match lines.next() {
            Some((_, first)) if first.starts_with("limoniter-tilemap") => {}
            _ => return Err(crate::i18n::tr("It isn't a Limoniter tile map: those start \"limoniter-tilemap\".").into()),
        }
        let mut p = Project::new(Mode::C64Hires, (1, 1), (1, 1));
        p.chars.clear();
        p.tiles.clear();
        p.layers.clear();
        let mut palette_given = false;
        let bad = |n: usize| crate::i18n::tf!("Line {n} of the tile map can't be read.");
        let number = |s: &str, n: usize| s.parse::<usize>().map_err(|_| bad(n));
        let colours = |s: &str, n: usize| -> Result<Colours, String> {
            let v: Vec<u8> = s.split(',').map(|x| x.parse::<u8>()).collect::<Result<_, _>>().map_err(|_| bad(n))?;
            (v.len() == 3).then(|| [v[0], v[1], v[2]]).ok_or_else(|| bad(n))
        };
        let unhex = |s: &str, n: usize| -> Result<Vec<u8>, String> {
            if !s.len().is_multiple_of(2) {
                return Err(bad(n));
            }
            (0..s.len()).step_by(2).map(|i| u8::from_str_radix(s.get(i..i + 2).unwrap_or("x"), 16).map_err(|_| bad(n))).collect()
        };
        while let Some((n, line)) = lines.next() {
            let (head, name) = match line.split_once(" | ") {
                Some((h, name)) => (h, Some(name.to_string())),
                None => (line.strip_suffix(" |").unwrap_or(line), None),
            };
            let words: Vec<&str> = head.split_whitespace().collect();
            let Some((&key, rest)) = words.split_first() else { continue };
            match key {
                "mode" => p.mode = rest.first().and_then(|m| Mode::from_id(m)).ok_or_else(|| bad(n))?,
                "colouring" => {
                    p.colouring = match rest.first().copied() {
                        Some("project") => Colouring::Project,
                        Some("tile") => Colouring::Tile,
                        Some("char") => Colouring::Char,
                        _ => return Err(bad(n)),
                    }
                }
                "backgrounds" if rest.len() == 4 => {
                    for (i, v) in rest.iter().enumerate() {
                        p.backgrounds[i] = number(v, n)? as u8;
                    }
                }
                "colours" if rest.len() == 3 => {
                    for (i, v) in rest.iter().enumerate() {
                        p.colours[i] = number(v, n)? as u8;
                    }
                }
                "palette" => {
                    p.palette = rest.iter().map(|c| unhex(c, n).and_then(|b| b.try_into().map_err(|_| bad(n)))).collect::<Result<_, _>>()?;
                    palette_given = true;
                }
                "tile-size" if rest.len() == 2 => p.tile_size = (number(rest[0], n)?.clamp(1, 16), number(rest[1], n)?.clamp(1, 16)),
                "map-size" if rest.len() == 2 => p.map_size = (number(rest[0], n)?.clamp(1, 8192), number(rest[1], n)?.clamp(1, 8192)),
                "char" if rest.len() == 3 => p.chars.push(Char { colours: colours(rest[0], n)?, material: number(rest[1], n)? as u8 & 15, pixels: decode_char(&unhex(rest[2], n)?, p.mode) }),
                "tile" if rest.len() >= 2 => {
                    let mut cells: Vec<u16> = rest[2..].iter().map(|c| number(c, n).map(|c| c as u16)).collect::<Result<_, _>>()?;
                    cells.resize(p.tile_size.0 * p.tile_size.1, 0);
                    p.tiles.push(Tile { cells, colours: colours(rest[0], n)?, tag: number(rest[1], n)? as u8, name: name.unwrap_or_default() });
                }
                "layer" => {
                    let (w, h) = p.map_size;
                    let mut cells = Vec::with_capacity(w * h);
                    let mut rows_wanted = h;
                    while rows_wanted > 0 {
                        let Some((n, row)) = lines.next() else { break };
                        let mut values: Vec<u16> = row.split_whitespace().map(|c| number(c, n).map(|c| c as u16)).collect::<Result<_, _>>()?;
                        values.resize(w, 0);
                        cells.extend(values);
                        rows_wanted -= 1;
                    }
                    cells.resize(w * h, 0);
                    p.layers.push(Layer { name: name.unwrap_or_default(), cells, shown: rest.first() != Some(&"0") });
                }
                "charpad" if rest.len() == 2 => {
                    let grid = unhex(rest[0], n)?;
                    p.ctm.flexigrid = grid.try_into().map_err(|_| bad(n))?;
                    p.ctm.key_map = number(rest[1], n)? as u8 & 15;
                }
                // Lines a later version adds are passed over.
                _ => {}
            }
        }
        if p.chars.is_empty() {
            p.chars.push(Char { pixels: [0; 64], colours: p.colours, material: 0 });
        }
        if p.tiles.is_empty() {
            p.tiles.push(p.blank_tile());
        }
        if p.layers.is_empty() {
            p.add_layer();
        }
        if !palette_given {
            p.palette = p.mode.default_palette();
        }
        Ok(p)
    }

    /// The project changed to another mode: the pixels kept as far as the new one holds them (a 2-bit picture
    /// keeps its low bit in a 1-bit mode), and the colours started afresh if the machine changes.
    pub fn change_mode(&mut self, mode: Mode) {
        if mode == self.mode {
            return;
        }
        let mask = ((1u16 << mode.bits()) - 1) as u8;
        for c in &mut self.chars {
            for p in &mut c.pixels {
                *p &= mask;
            }
        }
        let same_machine = (mode.c64() && self.mode.c64()) || (matches!(mode, Mode::Bits(..)) && matches!(self.mode, Mode::Bits(..)));
        if !same_machine {
            let fresh = Project::new(mode, (1, 1), (1, 1));
            self.colours = fresh.colours;
            self.colouring = fresh.colouring;
            for c in &mut self.chars {
                c.colours = fresh.colours;
            }
            for t in &mut self.tiles {
                t.colours = fresh.colours;
            }
        }
        if mode.bits() != self.mode.bits() || !matches!(mode, Mode::Bits(..)) {
            self.palette = mode.default_palette();
        }
        self.mode = mode;
    }
}

/// A small C64 level to show what the editor does: sky, clouds, brick platforms, ladders and grass, in
/// multicolour, each tile its own colour.
pub fn example() -> Project {
    let mut p = Project::new(Mode::C64Multicolour, (2, 2), (32, 12));
    p.backgrounds = [14, 2, 12, 0];
    p.colouring = Colouring::Tile;
    // Each character as rows of four pairs: 0 the sky, 1 red, 2 grey, 3 the tile's own colour.
    let chars: [[&str; 8]; 7] = [
        ["2222", "1112", "1112", "2222", "1211", "1211", "1211", "2222"],
        ["3333", "3333", "3131", "1111", "1111", "1211", "1111", "1111"],
        ["3003", "3333", "3003", "3003", "3333", "3003", "3003", "3333"],
        ["0000", "0003", "0033", "0333", "3333", "3333", "0333", "0000"],
        ["0000", "3300", "3330", "3333", "3333", "3333", "3330", "0000"],
        ["1111", "1211", "1111", "1112", "1111", "2111", "1111", "1121"],
        ["0000", "0000", "0000", "0000", "0000", "0000", "0000", "0000"],
    ];
    p.chars.truncate(8);
    for (n, rows) in chars.iter().enumerate() {
        let c = &mut p.chars[n + 1];
        for (y, row) in rows.iter().enumerate() {
            for (x, pair) in row.bytes().enumerate() {
                let v = pair - b'0';
                c.pixels[y * CHAR + x * 2] = (v >> 1) & 1;
                c.pixels[y * CHAR + x * 2 + 1] = v & 1;
            }
        }
    }
    let tiles: [([u16; 4], u8, &str); 6] = [([0, 0, 0, 0], 8 | 1, "sky"), ([1, 1, 1, 1], 8 | 1, "bricks"), ([2, 2, 6, 6], 8 | 5, "grass"), ([3, 3, 3, 3], 8 | 7, "ladder"), ([4, 5, 0, 0], 8 | 1, "cloud"), ([6, 6, 6, 6], 8 | 1, "earth")];
    p.tiles = tiles.iter().map(|(cells, colour, name)| Tile { cells: cells.to_vec(), colours: [*colour, 0, 0], tag: 0, name: (*name).into() }).collect();
    p.tiles[1].tag = 1;
    p.tiles[2].tag = 1;
    p.tiles[3].tag = 2;
    let (w, h) = p.map_size;
    for x in 0..w {
        p.set(0, x, h - 2, 2);
        p.set(0, x, h - 1, 5);
    }
    for (x0, x1, y) in [(3, 9, 6), (14, 22, 4), (24, 30, 7)] {
        for x in x0..x1 {
            p.set(0, x, y, 1);
        }
    }
    for (x, y0, y1) in [(7, 6, 10), (18, 4, 10), (26, 7, 10)] {
        for y in y0..y1 {
            p.set(0, x, y, 3);
        }
    }
    for (x, y) in [(2, 1), (11, 2), (25, 1)] {
        p.set(0, x, y, 4);
    }
    p
}

#[cfg(test)]
mod tests {
    use super::*;

    fn small() -> Project {
        let mut p = Project::new(Mode::C64Multicolour, (2, 2), (6, 4));
        p.chars.truncate(4);
        p.chars[1].pixels = [1; 64];
        p.chars[2].pixels = std::array::from_fn(|i| (i % 2) as u8);
        p.tiles.truncate(3);
        p.tiles[1].cells = vec![1, 1, 1, 1];
        p.tiles[2].cells = vec![0, 1, 2, 3];
        p
    }

    #[test]
    fn characters_go_to_bytes_and_back_in_every_layout() {
        let pixels: [u8; 64] = std::array::from_fn(|i| (i * 7 % 256) as u8);
        for mode in Mode::ALL {
            let mask = ((1u16 << mode.bits()) - 1) as u8;
            let masked = pixels.map(|p| p & mask);
            let bytes = encode_char(&masked, mode);
            assert_eq!(bytes.len(), mode.char_bytes(), "{mode:?}");
            assert_eq!(decode_char(&bytes, mode), masked, "{mode:?}");
        }
        // The Game Boy: each row a byte of the low bits, then a byte of the high.
        let mut gb = [0u8; 64];
        gb[0] = 3;
        gb[1] = 1;
        gb[2] = 2;
        assert_eq!(encode_char(&gb, Mode::Bits(2, Layout::RowPlanes))[..2], [0b1100_0000, 0b1010_0000]);
        // The NES: the eight bytes of the low plane, then the eight of the high.
        let nes = encode_char(&gb, Mode::Bits(2, Layout::CharPlanes));
        assert_eq!((nes[0], nes[8]), (0b1100_0000, 0b1010_0000));
        // Chunky: the left pixel in the top bits.
        assert_eq!(encode_char(&gb, Mode::Bits(4, Layout::Chunky))[0], 0x31);
        assert_eq!(encode_char(&gb, Mode::Bits(2, Layout::Chunky))[0], 0b1101_1000);
        // A bit a pixel, the left in the top bit, as the C64 and Spectrum keep them.
        assert_eq!(encode_char(&[1; 64], Mode::Spectrum), [0xFF; 8]);
    }

    #[test]
    fn colours_show_as_each_machine_shows_them() {
        let mut p = small();
        p.backgrounds = [6, 11, 12, 0];
        // A multicolour cell (colour 8 and up): pairs of bits, 11 the colour & 7.
        assert!(p.wide([14 | 8, 0, 0]));
        assert_eq!(p.pen_colour(3, [8 | 2, 0, 0], 0), c64_colour(2));
        assert_eq!(p.pen_colour(1, [8 | 2, 0, 0], 0), c64_colour(11));
        assert_eq!(p.pen_colour(0, [8 | 2, 0, 0], 0), c64_colour(6));
        // Below 8 the cell is hires, in one of the first eight.
        assert!(!p.wide([5, 0, 0]));
        assert_eq!(p.pen_colour(1, [5, 0, 0], 0), c64_colour(5));
        let rgba = p.char_rgba(2, [8 | 2, 0, 0]);
        // Char 2 is 0,1,0,1…: each pair 01, $D022's colour.
        assert_eq!(rgba[0], c64_colour(11));
        assert_eq!(rgba[1], c64_colour(11));
        // ECM: the background is chosen by the top two bits of the character's number, its shape by the rest.
        p.change_mode(Mode::C64Ecm);
        assert_eq!(p.pen_colour(0, [1, 0, 0], 64 + 2), c64_colour(11));
        p.chars.resize(130, p.chars[0].clone());
        assert_eq!(p.char_rgba(64 + 1, [1, 0, 0])[0], c64_colour(1), "char 65 has char 1's shape");
        // Bitmap: set pixels the screen's high nibble.
        p.change_mode(Mode::C64BitmapHires);
        assert_eq!((p.pen_colour(1, [0, 3, 4], 0), p.pen_colour(0, [0, 3, 4], 0)), (c64_colour(4), c64_colour(3)));
        p.change_mode(Mode::C64BitmapMulticolour);
        assert_eq!(p.pens([7, 3, 4], 0).iter().map(|(_, c)| *c).collect::<Vec<_>>(), [c64_colour(6), c64_colour(4), c64_colour(3), c64_colour(7)]);
        // The Spectrum: bright red ink on blue paper.
        p.change_mode(Mode::Spectrum);
        let attribute = 0x40 | (1 << 3) | 2;
        assert_eq!(p.pen_colour(1, [attribute, 0, 0], 0), Color32::from_rgb(255, 0, 0));
        assert_eq!(p.pen_colour(0, [attribute, 0, 0], 0), Color32::from_rgb(0, 0, 255));
        // So many bits: the palette.
        p.change_mode(Mode::Bits(2, Layout::RowPlanes));
        assert_eq!(p.pens(p.colours, 0).len(), 4);
        assert_eq!(p.pen_colour(3, p.colours, 0), Color32::from_rgb(0x08, 0x18, 0x20));
    }

    #[test]
    fn wide_pixels_are_drawn_a_pair_at_a_time() {
        let mut p = small();
        p.set_pixel(3, 5, 2, 2, true);
        assert_eq!(p.chars[3].pixels[2 * 8 + 4..2 * 8 + 6], [1, 0]);
        assert_eq!(p.pixel(3, 4, 2, true), 2);
        assert_eq!(p.pixel(3, 4, 2, false), 1);
        p.set_pixel(3, 7, 7, 1, false);
        assert_eq!(p.chars[3].pixels[63], 1);
        // Flipped, a pair stays a pair: 10 at the left of the row goes to its right, still 10.
        use crate::pixel_editor::Transform;
        let mut q = small();
        q.set_pixel(3, 0, 0, 2, true);
        q.transform_char(3, Transform::FlipAcross, true);
        assert_eq!(q.chars[3].pixels[..8], [0, 0, 0, 0, 0, 0, 1, 0]);
        q.transform_char(3, Transform::FlipAcross, false);
        assert_eq!(q.chars[3].pixels[..8], [0, 1, 0, 0, 0, 0, 0, 0]);
        q.transform_char(3, Transform::Rotate, true);
        assert_eq!(q.chars[3].pixels[1], 1, "multicolour isn't turned");
        q.transform_char(3, Transform::Invert, true);
        assert_eq!(q.chars[3].pixels.iter().filter(|p| **p == 1).count(), 63);
        q.change_mode(Mode::Bits(4, Layout::Chunky));
        q.transform_char(3, Transform::Invert, false);
        assert_eq!(q.chars[3].pixels[1], 15);
    }

    #[test]
    fn the_map_is_painted_filled_copied_and_pasted() {
        let mut p = small();
        p.stamp(0, 1, 1, &Block::one(2));
        assert_eq!(p.get(0, 1, 1), 2);
        // A block painted at the edge is cut off there.
        p.stamp(0, 5, 3, &Block { width: 2, height: 2, cells: vec![1, 1, 1, 1] });
        assert_eq!(p.get(0, 5, 3), 1);
        // Filling the zeros joined to the top left: all but the 2 and the 1.
        p.fill(0, 0, 0, &Block::one(1));
        let count = |p: &Project, t: u16| p.layers[0].cells.iter().filter(|c| **c == t).count();
        assert_eq!((count(&p, 1), count(&p, 2), count(&p, 0)), (23, 1, 0));
        // A fill that's walled in stops at the wall.
        let mut walled = small();
        for (x, y) in [(1, 0), (0, 1), (1, 1)] {
            walled.set(0, x, y, 2);
        }
        walled.fill(0, 0, 0, &Block::one(1));
        assert_eq!(walled.get(0, 0, 0), 1);
        assert_eq!(walled.get(0, 3, 3), 0);
        // Filling with what's there already does nothing (and ends).
        walled.fill(0, 3, 3, &Block::one(0));
        // Filled with a 2×1 pattern, it repeats from where the fill started.
        let mut patterned = small();
        patterned.fill(0, 1, 0, &Block { width: 2, height: 1, cells: vec![1, 2] });
        assert_eq!(&patterned.layers[0].cells[..6], [2, 1, 2, 1, 2, 1]);
        // Copied, cleared and pasted back.
        let block = p.copy(0, (0, 0, 3, 2));
        assert_eq!((block.width, block.height), (3, 2));
        assert_eq!(block.cells, [1, 1, 1, 1, 2, 1]);
        p.clear(0, (0, 0, 3, 2));
        assert_eq!(p.copy(0, (0, 0, 3, 2)).cells, [0; 6]);
        p.stamp(0, 3, 2, &block);
        assert_eq!(p.copy(0, (3, 2, 3, 2)).cells, [1, 1, 1, 1, 2, 1]);
        // Copying past the edge takes what's there.
        assert_eq!(p.copy(0, (5, 3, 4, 4)).cells.len(), 1);
    }

    #[test]
    fn maps_and_tiles_are_resized_and_tiles_added_and_taken_out() {
        let mut p = small();
        p.set(0, 5, 3, 2);
        p.set(0, 1, 0, 1);
        p.resize_map(8, 5);
        assert_eq!(p.get(0, 5, 3), 2);
        assert_eq!(p.layers[0].cells.len(), 40);
        p.resize_map(3, 2);
        assert_eq!(p.get(0, 1, 0), 1);
        // Inserting before tile 1 moves the map's 1s and 2s up.
        p.insert_tile(1, p.blank_tile());
        assert_eq!(p.get(0, 1, 0), 2);
        p.remove_tile(2);
        assert_eq!(p.get(0, 1, 0), 0, "where the tile was, tile 0");
        // Tiles made 3×1 keep their top left.
        p.resize_tiles(3, 1);
        assert_eq!(p.tiles[2].cells, [0, 1, 0]);
        // A tile given its own characters, which can then be drawn on alone.
        let before = p.chars.len();
        assert_eq!(p.own_chars(2), 3);
        assert_eq!(p.chars.len(), before + 3);
        assert_eq!(p.chars[p.tiles[2].cells[1] as usize].pixels, [1; 64]);
        // Tidied: the copies are found to be like the others, and the unused ones go.
        let gone = p.tidy_chars();
        assert_eq!(p.chars.len(), before + 3 - gone);
        assert_eq!(p.tiles[2].cells, [0, 1, 0]);
        assert!(p.chars.len() <= 2, "only blank and solid are left: {}", p.chars.len());
    }

    #[test]
    fn the_editors_own_file_keeps_everything() {
        let mut p = small();
        p.colouring = Colouring::Char;
        p.chars[2].colours = [13, 1, 2];
        p.chars[2].material = 5;
        p.tiles[2].tag = 7;
        p.tiles[2].name = "the wall | with a bar".into();
        p.set(0, 3, 2, 2);
        p.add_layer();
        p.layers[1].shown = false;
        p.set(1, 0, 3, 1);
        p.ctm.key_map = 9;
        let text = p.to_text();
        assert!(text.starts_with("limoniter-tilemap 1\nmode c64-multicolour\ncolouring char\n"), "{text}");
        assert_eq!(Project::from_text(&text), Ok(p.clone()));
        // And for the other machines, with their own palettes.
        for mode in Mode::ALL {
            let mut q = p.clone();
            q.change_mode(mode);
            q.palette[0] = [1, 2, 3];
            if !matches!(mode, Mode::Bits(..)) {
                q.palette = mode.default_palette();
            }
            assert_eq!(Project::from_text(&q.to_text()), Ok(q), "{mode:?}");
        }
        assert!(Project::from_text("something else").is_err());
        assert!(Project::from_text("limoniter-tilemap 1\nmode c64-hires\nchar 1,2 0 00\n").unwrap_err().contains('3'));
    }

    #[test]
    fn the_parts_go_out_as_source_in_each_form_and_as_binary_files() {
        let mut p = small();
        p.resize_map(3, 2);
        p.set(0, 0, 0, 1);
        p.set(0, 2, 1, 2);
        let parts = [Part::Chars, Part::Tiles, Part::Map];
        let z80 = p.source("level", &parts, Form::Z80, 1000);
        assert!(z80.starts_with("level_chars:\n        db      $00,$00"), "{z80}");
        assert!(z80.contains("level_tiles:\n        db      $00,$00,$00,$00\n        db      $01,$01,$01,$01\n        db      $00,$01,$02,$03\n"), "{z80}");
        assert!(z80.ends_with("level_map:\n        db      $01,$00,$00\n        db      $00,$00,$02\n"), "{z80}");
        // The assembler makes the bytes the binary files hold.
        let assembly = crate::asm::assemble(&crate::asm::z80::Z80, std::path::Path::new("t.asm"), &format!("  org $8000\n{z80}"), &|_| Err(crate::i18n::tr("no").into()));
        let binaries = p.binaries("level", &parts, false);
        let all: Vec<u8> = binaries.iter().flat_map(|(_, b)| b.clone()).collect();
        assert_eq!(assembly.image(0x8000, 0x8000 + all.len() as i64), all);
        assert_eq!(binaries.iter().map(|(n, _)| n.as_str()).collect::<Vec<_>>(), ["level_chars.bin", "level_tiles.bin", "level_map.bin"]);
        let acme = p.source("level", &[Part::Map], Form::Acme, 1000);
        assert_eq!(acme, "level_map\n        !byte   $01,$00,$00\n        !byte   $00,$00,$02\n");
        let c = p.source("level", &[Part::Map], Form::C, 1000);
        assert!(c.starts_with("const uint8_t level_map[6] = {\n    0x01, 0x00, 0x00,\n    0x00, 0x00, 0x02\n};"), "{c}");
        // BASIC DATA lines carry on numbering from one table to the next.
        let basic = p.source("level", &[Part::Tiles, Part::Map], Form::Basic, 2000);
        assert!(basic.starts_with("2000 REM level_tiles\n2010 DATA 0,0,0,0\n"), "{basic}");
        assert!(basic.contains("2040 REM level_map\n2050 DATA 1,0,0\n2060 DATA 0,0,2\n"), "{basic}");
        // Every form writes something with the map's numbers in it.
        for form in Form::ALL {
            let text = p.source("level", &[Part::Map], form, 1000);
            assert!(text.contains('2'), "{form:?}: {text}");
        }
        // More than 256 tiles: words, little-endian or big.
        p.tiles.resize(300, p.blank_tile());
        p.set(0, 1, 0, 299);
        let words = p.source("level", &[Part::Map], Form::Motorola, 1000);
        assert!(words.contains("dc.w    $0001,$012B,$0000"), "{words}");
        assert_eq!(p.binaries("level", &[Part::Map], true)[0].1[..4], [0, 1, 1, 0x2B]);
        assert_eq!(p.binaries("level", &[Part::Map], false)[0].1[..4], [1, 0, 0x2B, 1]);
    }

    #[test]
    fn the_map_goes_out_as_a_screen_of_characters_and_its_colours() {
        let mut p = small();
        p.resize_map(2, 1);
        p.set(0, 1, 0, 2);
        p.colouring = Colouring::Tile;
        p.tiles[2].colours = [9, 0, 0];
        let tables = p.tables("x", &[Part::Screen, Part::ScreenColours, Part::Colours]);
        let screen = tables.iter().find(|t| t.label == "x_screen").unwrap();
        assert_eq!(screen.values, [0, 0, 0, 1, 0, 0, 2, 3]);
        assert_eq!(screen.per_line, 4);
        let colours = tables.iter().find(|t| t.label == "x_screen_colours").unwrap();
        let first = i64::from(p.tiles[0].colours[0]);
        assert_eq!(colours.values, [first, first, 9, 9, first, first, 9, 9]);
        let per_tile = tables.iter().find(|t| t.label == "x_colours").unwrap();
        assert_eq!(per_tile.values.len(), 3);
        // The Spectrum's attributes; a C64 bitmap's screen bytes, high nibble the set pixels' colour.
        p.change_mode(Mode::C64BitmapMulticolour);
        p.tiles[2].colours = [7, 3, 4];
        let bitmap = p.tables("x", &[Part::Colours]);
        assert_eq!((bitmap[0].values[2], bitmap[1].values[2]), (0x43, 7));
        assert_eq!(bitmap[1].label, "x_colours_ram");
    }
}
