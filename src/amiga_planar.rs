//! Turning a picture into what an Amiga program shows it from: bit planes, a palette, and the source to show them.
//!
//! The picture is scaled to the screen's size (with the pixel's shape in mind), given a palette (found from the
//! picture by median cut refined by k-means in Oklab, a perceptual colour space, with some colours kept as they
//! are), and each pixel given its colour register, with dithering if asked. Extra half-brite and hold-and-modify
//! (HAM6, HAM8) are made too. The pixels' values then go into bit planes, a plane after another or a row of each
//! plane in turn, and out come the palette as words, copper moves and a C array, an IFF ILBM, and assembler source
//! with a copper list. The other way, raw planes ripped from a program are drawn as a picture to check them.

pub use crate::quantise::{Dither, Filter, Fit, Image, identifier, oklab, read_picture};
use crate::quantise::{as_f32, average_error, bayer, diffuse, distance, nearest, ordered_spread};
use std::fmt::Write as _;

/// The pixel's width on the screen.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Resolution {
    Lores,
    Hires,
    SuperHires,
}

impl Resolution {
    pub const ALL: [Self; 3] = [Self::Lores, Self::Hires, Self::SuperHires];

    pub fn name(self) -> &'static str {
        match self {
            Self::Lores => "Lo-res",
            Self::Hires => "Hi-res",
            Self::SuperHires => "Super-hi-res",
        }
    }

    /// A pixel's width for its height, as a PAL screen shows it.
    pub fn aspect(self, interlaced: bool) -> f32 {
        let across = match self {
            Self::Lores => 1.0,
            Self::Hires => 0.5,
            Self::SuperHires => 0.25,
        };
        if interlaced { across * 2.0 } else { across }
    }

    /// How many lo-res pixels one of these is wide: the display window and data fetch count in them.
    fn per_lores(self) -> usize {
        match self {
            Self::Lores => 1,
            Self::Hires => 2,
            Self::SuperHires => 4,
        }
    }
}

/// How pixels' values become colours.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    /// A colour register each.
    Plain,
    /// Extra half-brite: six planes, values from 32 the first 32 colours at half brightness.
    Ehb,
    /// Hold and modify with six planes: 16 colours, or the last pixel's with red, green or blue changed.
    Ham6,
    /// AGA's hold and modify with eight planes: 64 colours, or the top six bits of one level changed.
    Ham8,
}

impl Mode {
    pub const ALL: [Self; 4] = [Self::Plain, Self::Ehb, Self::Ham6, Self::Ham8];

    pub fn name(self) -> &'static str {
        match self {
            Self::Plain => "Colour registers",
            Self::Ehb => "Extra half-brite (64 colours)",
            Self::Ham6 => "HAM6 (hold and modify)",
            Self::Ham8 => "HAM8 (AGA hold and modify)",
        }
    }

    /// The planes the mode needs, if it needs a number.
    pub fn planes(self) -> Option<usize> {
        match self {
            Self::Plain => None,
            Self::Ehb | Self::Ham6 => Some(6),
            Self::Ham8 => Some(8),
        }
    }

    /// How many colour registers it uses with this many planes.
    pub fn registers(self, planes: usize) -> usize {
        match self {
            Self::Plain => 1 << planes,
            Self::Ehb => 32,
            Self::Ham6 => 16,
            Self::Ham8 => 64,
        }
    }
}

/// The colour registers' precision.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Chipset {
    /// OCS and ECS: 4 bits a level, 4096 colours.
    Ocs,
    /// AGA: 8 bits a level.
    Aga,
}

/// How the planes' rows are rounded: to a word (16 pixels), or to what AGA's faster fetch modes read at a time.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Fetch {
    Words,
    /// FMODE 1 or 2: 32 pixels.
    Longs,
    /// FMODE 3: 64 pixels.
    Quads,
}

impl Fetch {
    pub const ALL: [Self; 3] = [Self::Words, Self::Longs, Self::Quads];

    pub fn pixels(self) -> usize {
        match self {
            Self::Words => 16,
            Self::Longs => 32,
            Self::Quads => 64,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Self::Words => "16 pixels (word, any chipset)",
            Self::Longs => "32 pixels (AGA FMODE 1 or 2)",
            Self::Quads => "64 pixels (AGA FMODE 3)",
        }
    }

    fn fmode(self) -> u16 {
        match self {
            Self::Words => 0,
            Self::Longs => 1,
            Self::Quads => 3,
        }
    }
}

/// Everything the conversion is asked to do.
#[derive(Clone, Debug, PartialEq)]
pub struct Settings {
    pub width: usize,
    pub height: usize,
    pub resolution: Resolution,
    pub interlaced: bool,
    pub fit: Fit,
    pub filter: Filter,
    pub planes: usize,
    pub mode: Mode,
    pub chipset: Chipset,
    pub dither: Dither,
    /// How strongly to dither, from 0 to 1.
    pub strength: f32,
    /// Colour 0 kept black, as the background and border usually are.
    pub black_background: bool,
    /// A palette brought in, and how many of its colours to keep (the rest found from the picture).
    pub given: Vec<[u8; 3]>,
    pub keep_given: usize,
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            width: 320,
            height: 256,
            resolution: Resolution::Lores,
            interlaced: false,
            fit: Fit::Fill,
            filter: Filter::Lanczos,
            planes: 5,
            mode: Mode::Plain,
            chipset: Chipset::Ocs,
            dither: Dither::FloydSteinberg,
            strength: 0.8,
            black_background: true,
            given: Vec::new(),
            keep_given: 0,
        }
    }
}

impl Settings {
    /// The settings made consistent: planes for the mode, AGA where only AGA can show it.
    pub fn settled(&self) -> Settings {
        let mut s = self.clone();
        s.width = s.width.clamp(1, 4096);
        s.height = s.height.clamp(1, 4096);
        s.planes = s.mode.planes().unwrap_or(s.planes).clamp(1, 8);
        if s.planes > 6 || s.mode == Mode::Ham8 || s.resolution == Resolution::SuperHires && s.planes > 2 {
            s.chipset = Chipset::Aga;
        }
        // HAM6 changes four bits a level, OCS's colour, whichever chipset shows it.
        if s.mode == Mode::Ham6 {
            s.chipset = Chipset::Ocs;
        }
        s.strength = s.strength.clamp(0.0, 1.0);
        s
    }
}

/// A converted picture: each pixel's value, the palette (as the hardware shows it) and the result as it looks.
#[derive(Clone, Debug, PartialEq)]
pub struct Converted {
    pub width: usize,
    pub height: usize,
    pub planes: usize,
    pub mode: Mode,
    pub chipset: Chipset,
    pub resolution: Resolution,
    pub interlaced: bool,
    /// The colour registers, each level in 8 bits (an OCS one's 4 bits doubled up, as the screen shows it).
    pub palette: Vec<[u8; 3]>,
    pub values: Vec<u8>,
    /// As the Amiga shows it.
    pub rgba: Vec<u8>,
    /// The average difference from the scaled picture, in Oklab (about 0.01 is just noticeable).
    pub error: f32,
}

/// A level as a colour register holds it: OCS's four bits, shown doubled up (0xA as 0xAA).
fn register_level(level: f32, chipset: Chipset) -> u8 {
    let level = level.round().clamp(0.0, 255.0);
    match chipset {
        Chipset::Ocs => ((level / 17.0).round() as u8) * 17,
        Chipset::Aga => level as u8,
    }
}

/// A colour at half brightness, as extra half-brite makes it: each level's bits shifted down one.
fn half(colour: [u8; 3], chipset: Chipset) -> [u8; 3] {
    match chipset {
        Chipset::Ocs => colour.map(|v| ((v >> 4) >> 1) * 17),
        Chipset::Aga => colour.map(|v| v >> 1),
    }
}

/// The picture scaled to the screen, as sRGB levels, its see-through parts on black.
pub fn scale(image: &Image, s: &Settings) -> Vec<[f32; 3]> {
    crate::quantise::scale(image, s.width, s.height, s.resolution.aspect(s.interlaced), s.fit, s.filter)
}

/// A palette for the picture: `slots` long, the given colours where a slot has one, the rest found from the picture.
/// With extra half-brite, each colour also stands for its half-bright one. Levels are the registers' own.
pub fn find_palette(pixels: &[[f32; 3]], slots: &[Option<[u8; 3]>], chipset: Chipset, ehb: bool) -> Vec<[u8; 3]> {
    let snap = |c: [f32; 3]| c.map(|v| register_level(v, chipset));
    let halved = |c: [u8; 3]| half(c, chipset);
    crate::quantise::find_palette(pixels, slots, &snap, if ehb { Some(&halved) } else { None })
}

/// Each pixel's colour register, from a palette (and its half-bright colours with extra half-brite).
pub fn choose(pixels: &[[f32; 3]], width: usize, height: usize, palette: &[[u8; 3]], chipset: Chipset, ehb: bool, dither: Dither, strength: f32) -> Vec<u8> {
    let mut colours: Vec<[u8; 3]> = palette.to_vec();
    if ehb {
        colours.extend(palette.iter().map(|&c| half(c, chipset)));
    }
    crate::quantise::choose(pixels, width, height, &colours, dither, strength)
}

/// The slots a palette is found into: colour 0 black if asked, the given palette's first colours kept.
fn slots(s: &Settings, count: usize) -> Vec<Option<[u8; 3]>> {
    let mut slots = vec![None; count];
    for (slot, colour) in slots.iter_mut().zip(s.given.iter().take(s.keep_given)) {
        *slot = Some(colour.map(|v| register_level(f32::from(v), s.chipset)));
    }
    if s.black_background && count > 0 && slots[0].is_none() {
        slots[0] = Some([0, 0, 0]);
    }
    slots
}

/// Hold and modify: each pixel a base colour, or the last pixel's with one level changed, whichever is nearest.
/// HAM6 has 16 base colours and sets a level's four bits; HAM8 64, setting a level's top six bits.
pub fn ham_encode(pixels: &[[f32; 3]], width: usize, height: usize, palette: &[[u8; 3]], ham8: bool, dither: Dither, strength: f32) -> Vec<u8> {
    let bits = if ham8 { 6 } else { 4 };
    let labs: Vec<[f32; 3]> = palette.iter().map(|&c| oklab(as_f32(c))).collect();
    let mut values = vec![0u8; width * height];
    let mut work: Vec<[f32; 3]> = pixels.to_vec();
    let spread = ordered_spread(1 << (bits + 2), strength);
    for y in 0..height {
        let mut held = palette.first().copied().unwrap_or([0, 0, 0]);
        for x in 0..width {
            let mut target = work[y * width + x];
            if dither == Dither::Ordered {
                let offset = ((bayer(x, y) + 0.5) / 64.0 - 0.5) * spread;
                target = target.map(|v| v + offset);
            }
            let target = target.map(|v| v.clamp(0.0, 255.0));
            let lab = oklab(target);
            let (index, mut best) = nearest(&labs, lab);
            let (mut value, mut colour) = (index as u8, palette[index]);
            // Red is changed with control bits 2, green 3, blue 1.
            for (level, control) in [(0usize, 2u8), (1, 3), (2, 1)] {
                let (data, shown) = if ham8 {
                    let low = held[level] & 3;
                    let data = ((target[level] - f32::from(low)) / 4.0).round().clamp(0.0, 63.0) as u8;
                    (data, (data << 2) | low)
                } else {
                    let data = (target[level] / 17.0).round().clamp(0.0, 15.0) as u8;
                    (data, data * 17)
                };
                let mut candidate = held;
                candidate[level] = shown;
                let d = distance(oklab(as_f32(candidate)), lab);
                if d < best {
                    best = d;
                    value = control << bits | data;
                    colour = candidate;
                }
            }
            values[y * width + x] = value;
            held = colour;
            if dither == Dither::FloydSteinberg {
                let error: [f32; 3] = std::array::from_fn(|c| (target[c] - f32::from(colour[c])) * strength);
                diffuse(&mut work, width, height, x, y, false, error);
            }
        }
    }
    values
}

/// Pixels' values drawn as the Amiga shows them, each row of a HAM picture starting from colour 0.
pub fn render(values: &[u8], width: usize, palette: &[[u8; 3]], mode: Mode, chipset: Chipset) -> Vec<u8> {
    let colour = |i: usize| palette.get(i).copied().unwrap_or([0, 0, 0]);
    let mut rgba = Vec::with_capacity(values.len() * 4);
    for row in values.chunks(width.max(1)) {
        let mut held = colour(0);
        for &value in row {
            let shown = match mode {
                Mode::Plain => colour(usize::from(value)),
                Mode::Ehb if value >= 32 => half(colour(usize::from(value) - 32), chipset),
                Mode::Ehb => colour(usize::from(value)),
                Mode::Ham6 | Mode::Ham8 => {
                    let bits = if mode == Mode::Ham8 { 6 } else { 4 };
                    let data = value & ((1 << bits) - 1);
                    let set = |old: u8| if bits == 6 { (data << 2) | (old & 3) } else { data * 17 };
                    match value >> bits {
                        0 => held = colour(usize::from(data)),
                        1 => held[2] = set(held[2]),
                        2 => held[0] = set(held[0]),
                        _ => held[1] = set(held[1]),
                    }
                    held
                }
            };
            rgba.extend_from_slice(&[shown[0], shown[1], shown[2], 255]);
        }
    }
    rgba
}

/// The whole conversion.
pub fn convert(image: &Image, settings: &Settings) -> Converted {
    let s = settings.settled();
    let pixels = scale(image, &s);
    let registers = s.mode.registers(s.planes);
    let slots = slots(&s, registers);
    let palette = find_palette(&pixels, &slots, s.chipset, s.mode == Mode::Ehb);
    let values = match s.mode {
        Mode::Plain | Mode::Ehb => choose(&pixels, s.width, s.height, &palette, s.chipset, s.mode == Mode::Ehb, s.dither, s.strength),
        Mode::Ham6 | Mode::Ham8 => ham_encode(&pixels, s.width, s.height, &palette, s.mode == Mode::Ham8, s.dither, s.strength),
    };
    let rgba = render(&values, s.width, &palette, s.mode, s.chipset);
    let error = average_error(&pixels, &rgba);
    Converted { width: s.width, height: s.height, planes: s.planes, mode: s.mode, chipset: s.chipset, resolution: s.resolution, interlaced: s.interlaced, palette, values, rgba, error }
}

// Bit planes.

/// The bytes in a row of one plane: the width rounded up to the fetch's size.
pub fn row_bytes(width: usize, fetch: Fetch) -> usize {
    width.div_ceil(fetch.pixels()) * fetch.pixels() / 8
}

/// Pixels' values as bit planes: plane 0 (the lowest bit) first, each whole; or interleaved, a row of each plane in
/// turn. Bits past the width are 0.
pub fn to_planes(values: &[u8], width: usize, height: usize, planes: usize, interleaved: bool, fetch: Fetch) -> Vec<u8> {
    let row = row_bytes(width, fetch);
    let mut out = vec![0u8; row * height * planes];
    for y in 0..height {
        for plane in 0..planes {
            let start = if interleaved { (y * planes + plane) * row } else { (plane * height + y) * row };
            for x in 0..width {
                if values[y * width + x] >> plane & 1 != 0 {
                    out[start + x / 8] |= 0x80 >> (x % 8);
                }
            }
        }
    }
    out
}

/// Bit planes back to pixels' values; what isn't there is 0.
pub fn from_planes(data: &[u8], width: usize, height: usize, planes: usize, interleaved: bool, fetch: Fetch) -> Vec<u8> {
    let row = row_bytes(width, fetch);
    let mut values = vec![0u8; width * height];
    for y in 0..height {
        for plane in 0..planes {
            let start = if interleaved { (y * planes + plane) * row } else { (plane * height + y) * row };
            for x in 0..width {
                if data.get(start + x / 8).is_some_and(|b| b & (0x80 >> (x % 8)) != 0) {
                    values[y * width + x] |= 1 << plane;
                }
            }
        }
    }
    values
}

// The palette written out.

/// A colour as OCS's register word, $0RGB.
pub fn word12(c: [u8; 3]) -> u16 {
    (u16::from(c[0] >> 4) << 8) | (u16::from(c[1] >> 4) << 4) | u16::from(c[2] >> 4)
}

/// The low four bits of each level as a word, which AGA sets with BPLCON3's LOCT on.
fn word_low(c: [u8; 3]) -> u16 {
    (u16::from(c[0] & 15) << 8) | (u16::from(c[1] & 15) << 4) | u16::from(c[2] & 15)
}

/// The palette as raw big-endian words: OCS's $0RGB a colour; AGA's high halves of the levels then the low halves.
pub fn palette_words(palette: &[[u8; 3]], chipset: Chipset) -> Vec<u8> {
    let mut out: Vec<u8> = palette.iter().flat_map(|&c| word12(c).to_be_bytes()).collect();
    if chipset == Chipset::Aga {
        out.extend(palette.iter().flat_map(|&c| word_low(c).to_be_bytes()));
    }
    out
}

/// The palette as red, green, blue bytes.
pub fn palette_rgb(palette: &[[u8; 3]]) -> Vec<u8> {
    palette.iter().flatten().copied().collect()
}

/// BPLCON3 with a colour bank chosen and LOCT on or off, its other bits as AGA starts ($0C00).
fn bplcon3(bank: usize, low: bool) -> u16 {
    0x0C00 | ((bank as u16) << 13) | if low { 0x0200 } else { 0 }
}

/// The palette as copper moves (`dc.w register,value`). AGA's 24-bit colours go a bank of 32 at a time, the high
/// halves of the levels with BPLCON3's LOCT off and then the low halves with it on.
pub fn palette_copper(palette: &[[u8; 3]], chipset: Chipset) -> String {
    let registers: Vec<(usize, [u8; 3])> = palette.iter().copied().enumerate().collect();
    copper_moves(&registers, chipset)
}

/// Colour registers (by number, 0 to 255) set by copper moves, as `palette_copper` does.
pub fn copper_moves(registers: &[(usize, [u8; 3])], chipset: Chipset) -> String {
    let mut out = String::new();
    let moves = |out: &mut String, word: &dyn Fn([u8; 3]) -> u16, bank: usize| {
        for &(i, c) in registers.iter().filter(|r| r.0 / 32 == bank) {
            let _ = writeln!(out, "        dc.w    ${:04x},${:04x}    ; COLOR{:02}", 0x180 + (i % 32) * 2, word(c), i % 32);
        }
    };
    let mut banks: Vec<usize> = registers.iter().map(|r| r.0 / 32).collect();
    banks.dedup();
    match chipset {
        Chipset::Ocs => moves(&mut out, &word12, 0),
        Chipset::Aga => {
            for &bank in &banks {
                let _ = writeln!(out, "        dc.w    $0106,${:04x}    ; BPLCON3: bank {bank}, high halves", bplcon3(bank, false));
                moves(&mut out, &word12, bank);
                let _ = writeln!(out, "        dc.w    $0106,${:04x}    ; BPLCON3: bank {bank}, low halves (LOCT)", bplcon3(bank, true));
                moves(&mut out, &word_low, bank);
            }
            let _ = writeln!(out, "        dc.w    $0106,${:04x}    ; BPLCON3 as it was", bplcon3(0, false));
        }
    }
    out
}

/// The palette as a C array: OCS's register words, or AGA's 0x00RRGGBB with a table for LoadRGB32().
pub fn palette_c(palette: &[[u8; 3]], chipset: Chipset, name: &str) -> String {
    let name = identifier(name);
    let mut out = String::new();
    let rows = |items: Vec<String>| items.chunks(8).map(|c| format!("    {}", c.join(", "))).collect::<Vec<_>>().join(",\n");
    match chipset {
        Chipset::Ocs => {
            let _ = writeln!(out, "/* {} colours, as OCS colour registers: 0x0RGB */", palette.len());
            let _ = writeln!(out, "UWORD {name}_palette[{}] = {{\n{}\n}};", palette.len(), rows(palette.iter().map(|&c| format!("0x{:04X}", word12(c))).collect()));
        }
        Chipset::Aga => {
            let _ = writeln!(out, "/* {} colours, 24-bit: 0x00RRGGBB */", palette.len());
            let _ = writeln!(out, "ULONG {name}_palette[{}] = {{\n{}\n}};", palette.len(), rows(palette.iter().map(|c| format!("0x00{:02X}{:02X}{:02X}", c[0], c[1], c[2])).collect()));
            let _ = writeln!(out, "\n/* For graphics.library's LoadRGB32(): the count and first colour, 32 bits a level, then 0 */");
            let mut table = vec![format!("{}UL << 16 | 0", palette.len())];
            for c in palette {
                table.extend(c.iter().map(|&v| format!("0x{:08X}", u32::from(v) * 0x0101_0101)));
            }
            table.push("0".into());
            let _ = writeln!(out, "ULONG {name}_rgb32[{}] = {{\n{}\n}};", table.len(), rows(table));
        }
    }
    out
}

// Files.

/// Packs a row with ByteRun1, as ILBM's compression 1: runs of three or more the same as a repeat, the rest copied.
pub fn pack_byterun1(row: &[u8], out: &mut Vec<u8>) {
    let mut at = 0;
    while at < row.len() {
        let mut run = 1;
        while at + run < row.len() && run < 128 && row[at + run] == row[at] {
            run += 1;
        }
        if run >= 3 {
            out.push((1 - run as i16) as i8 as u8);
            out.push(row[at]);
            at += run;
            continue;
        }
        // Copy up to where a run of three begins.
        let start = at;
        while at < row.len() && at - start < 128 {
            if at + 2 < row.len() && row[at] == row[at + 1] && row[at] == row[at + 2] {
                break;
            }
            at += 1;
        }
        out.push((at - start - 1) as u8);
        out.extend_from_slice(&row[start..at]);
    }
}

/// The picture as an IFF ILBM, packed with ByteRun1 or not.
pub fn to_ilbm(c: &Converted, packed: bool) -> Vec<u8> {
    fn chunk(out: &mut Vec<u8>, id: &[u8; 4], data: &[u8]) {
        out.extend_from_slice(id);
        out.extend_from_slice(&(data.len() as u32).to_be_bytes());
        out.extend_from_slice(data);
        if data.len() % 2 == 1 {
            out.push(0);
        }
    }
    let aspect = c.resolution.aspect(c.interlaced);
    let (x_aspect, y_aspect) = if aspect >= 1.0 { ((10.0 * aspect).round() as u8, 10u8) } else { (10, (10.0 / aspect).round() as u8) };
    let mut bmhd = Vec::new();
    bmhd.extend_from_slice(&(c.width as u16).to_be_bytes());
    bmhd.extend_from_slice(&(c.height as u16).to_be_bytes());
    bmhd.extend_from_slice(&[0, 0, 0, 0, c.planes as u8, 0, u8::from(packed), 0, 0, 0, x_aspect, y_aspect]);
    bmhd.extend_from_slice(&(c.width as u16).to_be_bytes());
    bmhd.extend_from_slice(&(c.height as u16).to_be_bytes());
    let mut camg = 0u32;
    match c.resolution {
        Resolution::Lores => {}
        Resolution::Hires => camg |= 0x8000,
        Resolution::SuperHires => camg |= 0x8020,
    }
    if c.interlaced {
        camg |= 0x4;
    }
    match c.mode {
        Mode::Ehb => camg |= 0x80,
        Mode::Ham6 | Mode::Ham8 => camg |= 0x800,
        Mode::Plain => {}
    }
    let planes = to_planes(&c.values, c.width, c.height, c.planes, true, Fetch::Words);
    let body = if packed {
        let mut body = Vec::new();
        for row in planes.chunks(row_bytes(c.width, Fetch::Words)) {
            pack_byterun1(row, &mut body);
        }
        body
    } else {
        planes
    };
    let mut inner = b"ILBM".to_vec();
    chunk(&mut inner, b"BMHD", &bmhd);
    chunk(&mut inner, b"CMAP", &palette_rgb(&c.palette));
    chunk(&mut inner, b"CAMG", &camg.to_be_bytes());
    chunk(&mut inner, b"BODY", &body);
    let mut out = b"FORM".to_vec();
    out.extend_from_slice(&(inner.len() as u32).to_be_bytes());
    out.extend(inner);
    out
}

/// BPLCON0 for the picture: the planes, HAM, the resolution and interlace, and COLOR on.
pub fn bplcon0(c: &Converted) -> u16 {
    let mut v = if c.planes == 8 { 0x0010 } else { (c.planes as u16) << 12 } | 0x0200;
    match c.resolution {
        Resolution::Lores => {}
        Resolution::Hires => v |= 0x8000,
        Resolution::SuperHires => v |= 0x0040,
    }
    if matches!(c.mode, Mode::Ham6 | Mode::Ham8) {
        v |= 0x0800;
    }
    if c.interlaced {
        v |= 0x0004;
    }
    v
}

/// The sizes a program needs for the planes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Layout {
    /// Bytes in a row of one plane.
    pub row: usize,
    /// Bytes in one whole plane.
    pub plane: usize,
    /// From one plane's start to the next's.
    pub step: usize,
    /// BPL1MOD and BPL2MOD: what's skipped after each row shown.
    pub modulo: usize,
    pub total: usize,
}

pub fn layout(c: &Converted, interleaved: bool, fetch: Fetch) -> Layout {
    let row = row_bytes(c.width, fetch);
    let plane = row * c.height;
    let (step, mut modulo) = if interleaved { (row, row * (c.planes - 1)) } else { (plane, 0) };
    // An interlaced screen shows every other row each field.
    if c.interlaced {
        modulo += if interleaved { row * c.planes } else { row };
    }
    Layout { row, plane, step, modulo, total: plane * c.planes }
}

/// Assembler source that brings in the planes and shows them: their sizes, a copper list with the display window,
/// data fetch, BPLCON0, modulos, the palette and the plane pointers, and a routine to set the pointers.
pub fn asm_source(c: &Converted, name: &str, planes_file: &str, interleaved: bool, fetch: Fetch) -> String {
    let id = identifier(name);
    let l = layout(c, interleaved, fetch);
    let mut out = String::new();
    let shown_rows = if c.interlaced { c.height / 2 } else { c.height };
    let lores_wide = c.width.div_ceil(c.resolution.per_lores());
    let (diw_start_v, diw_start_h) = (0x2Cusize, 0x81usize);
    let diwstrt = (diw_start_v & 0xFF) << 8 | diw_start_h;
    let diwstop = ((diw_start_v + shown_rows) & 0xFF) << 8 | ((diw_start_h + lores_wide) & 0xFF);
    let ddfstrt = match c.resolution {
        Resolution::Lores => 0x38,
        _ => 0x3C,
    };
    let ddfstop = ddfstrt + 8 * (lores_wide.div_ceil(16).max(1) - 1);
    let _ = writeln!(out, "; {} by {}, {} bit plane{}, {}{}.", c.width, c.height, c.planes, if c.planes == 1 { "" } else { "s" }, c.mode.name(), if c.interlaced { ", interlaced" } else { "" });
    let _ = writeln!(out, "; The planes are {}.", if interleaved { "interleaved: a row of each plane in turn" } else { "one after another" });
    let _ = writeln!(out);
    let _ = writeln!(out, "{id}_WIDTH     equ {}", c.width);
    let _ = writeln!(out, "{id}_HEIGHT    equ {}", c.height);
    let _ = writeln!(out, "{id}_PLANES    equ {}", c.planes);
    let _ = writeln!(out, "{id}_ROWBYTES  equ {}    ; one row of one plane", l.row);
    let _ = writeln!(out, "{id}_PLANESIZE equ {}    ; one whole plane", l.plane);
    let _ = writeln!(out, "{id}_PLANESTEP equ {}    ; from one plane's start to the next's", l.step);
    let _ = writeln!(out, "{id}_MODULO    equ {}    ; BPL1MOD and BPL2MOD", l.modulo);
    let _ = writeln!(out, "{id}_SIZE      equ {}", l.total);
    let _ = writeln!(out);
    let _ = writeln!(out, "; Points the copper list's plane pointers at the planes. Call it before starting the copper list.");
    let _ = writeln!(out, "{id}_setplanes:");
    let _ = writeln!(out, "        lea     {id}_bplpt+2,a0");
    let _ = writeln!(out, "        move.l  #{id}_planes,d0");
    let _ = writeln!(out, "        moveq   #{id}_PLANES-1,d1");
    let _ = writeln!(out, ".next:  swap    d0");
    let _ = writeln!(out, "        move.w  d0,(a0)         ; BPLxPTH");
    let _ = writeln!(out, "        swap    d0");
    let _ = writeln!(out, "        move.w  d0,4(a0)        ; BPLxPTL");
    let _ = writeln!(out, "        add.l   #{id}_PLANESTEP,d0");
    let _ = writeln!(out, "        addq.l  #8,a0");
    let _ = writeln!(out, "        dbf     d1,.next");
    let _ = writeln!(out, "        rts");
    let _ = writeln!(out);
    let _ = writeln!(out, "        section {id}_copper,data_c");
    let _ = writeln!(out, "{id}_copper:");
    let _ = writeln!(out, "        dc.w    $008e,${diwstrt:04x}    ; DIWSTRT");
    let _ = writeln!(out, "        dc.w    $0090,${diwstop:04x}    ; DIWSTOP");
    let _ = writeln!(out, "        dc.w    $0092,${ddfstrt:04x}    ; DDFSTRT");
    let _ = writeln!(out, "        dc.w    $0094,${ddfstop:04x}    ; DDFSTOP");
    let _ = writeln!(out, "        dc.w    $0100,${:04x}    ; BPLCON0", bplcon0(c));
    let _ = writeln!(out, "        dc.w    $0102,$0000    ; BPLCON1");
    let _ = writeln!(out, "        dc.w    $0104,$0000    ; BPLCON2");
    let _ = writeln!(out, "        dc.w    $0108,{id}_MODULO    ; BPL1MOD");
    let _ = writeln!(out, "        dc.w    $010a,{id}_MODULO    ; BPL2MOD");
    if c.chipset == Chipset::Aga || fetch != Fetch::Words {
        if fetch != Fetch::Words {
            let _ = writeln!(out, "; With FMODE's wider fetch, the hardware fetches whole {}-pixel units: if DDFSTOP makes it fetch past", fetch.pixels());
            let _ = writeln!(out, "; the end of a row, take the extra bytes off the modulo.");
        }
        let _ = writeln!(out, "        dc.w    $01fc,${:04x}    ; FMODE", fetch.fmode());
    }
    out.push_str(&palette_copper(&c.palette, c.chipset));
    let _ = writeln!(out, "{id}_bplpt:");
    for plane in 0..c.planes {
        let _ = writeln!(out, "        dc.w    ${:04x},0,${:04x},0    ; BPL{}PTH, BPL{}PTL", 0xE0 + plane * 4, 0xE2 + plane * 4, plane + 1, plane + 1);
    }
    if c.interlaced {
        let _ = writeln!(out, "; Interlaced: the other field starts a row further on; a second copper list, or a change to the");
        let _ = writeln!(out, "; pointers each frame, shows it.");
    }
    let _ = writeln!(out, "        dc.w    $ffff,$fffe    ; the end");
    let _ = writeln!(out);
    let _ = writeln!(out, "        section {id}_chip,data_c");
    let _ = writeln!(out, "        cnop    0,8");
    let _ = writeln!(out, "{id}_planes:");
    let _ = writeln!(out, "        incbin  \"{planes_file}\"    ; {} bytes", l.total);
    out
}

/// A palette from a file: an IFF's CMAP, a JASC or GIMP palette, assembler or C numbers ($0RGB words, or
/// $RRGGBB), raw OCS words, or red, green, blue bytes.
pub fn read_palette(bytes: &[u8]) -> Option<Vec<[u8; 3]>> {
    if bytes.len() >= 12 && &bytes[..4] == b"FORM" {
        let mut at = 12;
        while at + 8 <= bytes.len() {
            let len = u32::from_be_bytes([bytes[at + 4], bytes[at + 5], bytes[at + 6], bytes[at + 7]]) as usize;
            if &bytes[at..at + 4] == b"CMAP" {
                let data = bytes.get(at + 8..at + 8 + len)?;
                let mut colours: Vec<[u8; 3]> = data.as_chunks::<3>().0.to_vec();
                // Old programs kept OCS's four bits in the high half alone.
                if colours.iter().flatten().all(|v| v & 15 == 0) {
                    colours.iter_mut().for_each(|c| *c = c.map(|v| (v >> 4) * 17));
                }
                return (!colours.is_empty()).then_some(colours);
            }
            at += 8 + len + (len & 1);
        }
        return None;
    }
    if let Ok(text) = std::str::from_utf8(bytes)
        && text.chars().all(|c| !c.is_control() || c.is_ascii_whitespace())
    {
        return palette_from_text(text);
    }
    // Raw OCS words when every word fits in twelve bits, otherwise red, green and blue bytes.
    if bytes.len().is_multiple_of(2) && bytes.len() <= 512 && bytes.chunks(2).all(|w| w[0] < 0x10) {
        return Some(bytes.chunks(2).map(|w| expand12(u16::from_be_bytes([w[0], w[1]]))).collect());
    }
    (bytes.len().is_multiple_of(3) && bytes.len() <= 768 && !bytes.is_empty()).then(|| bytes.as_chunks::<3>().0.to_vec())
}

fn expand12(w: u16) -> [u8; 3] {
    [((w >> 8) & 15) as u8 * 17, ((w >> 4) & 15) as u8 * 17, (w & 15) as u8 * 17]
}

fn palette_from_text(text: &str) -> Option<Vec<[u8; 3]>> {
    let mut lines = text.lines().map(str::trim);
    let first = lines.next().unwrap_or_default();
    // JASC-PAL: the header, a version, a count, then "r g b" lines. GIMP: "GIMP Palette", then "r g b name" lines.
    if first == "JASC-PAL" || first == "GIMP Palette" {
        let colours: Vec<[u8; 3]> = text
            .lines()
            .skip(1)
            .filter_map(|line| {
                let numbers: Vec<u8> = line.split_whitespace().take(3).map_while(|n| n.parse().ok()).collect();
                (numbers.len() == 3).then(|| [numbers[0], numbers[1], numbers[2]])
            })
            .collect();
        return (!colours.is_empty()).then_some(colours);
    }
    // Numbers in assembler or C: $0RGB words or $RRGGBB (0x too); copper moves to COLORxx have their values taken.
    let mut colours = Vec::new();
    for line in text.lines() {
        let code = line.split([';', '*']).next().unwrap_or_default();
        let code = code.split("//").next().unwrap_or_default();
        let numbers: Vec<(u32, usize)> = code
            .split(|c: char| !(c.is_ascii_alphanumeric() || c == '$'))
            .filter_map(|word| {
                let digits = word.strip_prefix('$').or_else(|| word.strip_prefix("0x")).or_else(|| word.strip_prefix("0X"))?;
                let digits = digits.trim_end_matches(['U', 'L', 'u', 'l']);
                u32::from_str_radix(digits, 16).ok().map(|v| (v, digits.len()))
            })
            .collect();
        let mut i = 0;
        while i < numbers.len() {
            let (value, len) = numbers[i];
            // A copper move: a colour register then its value.
            if (0x180..0x1C0).contains(&value) && i + 1 < numbers.len() {
                colours.push(expand12(numbers[i + 1].0 as u16));
                i += 2;
                continue;
            }
            if len > 4 {
                colours.push([(value >> 16) as u8, (value >> 8) as u8, value as u8]);
            } else if value <= 0xFFF {
                colours.push(expand12(value as u16));
            }
            i += 1;
        }
    }
    (!colours.is_empty()).then_some(colours)
}

/// Raw planes drawn as a picture, for checking planes ripped from a program.
#[derive(Clone, Debug, PartialEq)]
pub struct RawView {
    pub offset: usize,
    pub width: usize,
    pub height: usize,
    pub planes: usize,
    pub interleaved: bool,
    pub fetch: Fetch,
    pub mode: Mode,
}

impl Default for RawView {
    fn default() -> Self {
        RawView { offset: 0, width: 320, height: 256, planes: 5, interleaved: false, fetch: Fetch::Words, mode: Mode::Plain }
    }
}

impl RawView {
    /// The rows that fit in the file after the offset, at this width and number of planes.
    pub fn rows_in(&self, length: usize) -> usize {
        let per_row = row_bytes(self.width.max(1), self.fetch) * self.planes.max(1);
        length.saturating_sub(self.offset) / per_row
    }

    /// The picture's values and colours, with a palette (shades of grey where it runs out).
    pub fn draw(&self, bytes: &[u8], palette: &[[u8; 3]], chipset: Chipset) -> (Vec<u8>, Vec<u8>) {
        let planes = self.planes.clamp(1, 8);
        let (width, height) = (self.width.clamp(1, 4096), self.height.clamp(1, 4096));
        let data = bytes.get(self.offset..).unwrap_or_default();
        let values = from_planes(data, width, height, planes, self.interleaved, self.fetch);
        let registers = self.mode.registers(planes);
        let mut colours: Vec<[u8; 3]> = palette.iter().copied().take(registers).collect();
        let have = colours.len();
        colours.extend((have..registers).map(|i| [(i * 255 / (registers - 1).max(1)) as u8; 3]));
        let rgba = render(&values, width, &colours, self.mode, chipset);
        (values, rgba)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A picture with smooth gradients and a few hard-edged shapes, to convert.
    fn test_card(width: usize, height: usize) -> Image {
        let mut rgba = Vec::with_capacity(width * height * 4);
        for y in 0..height {
            for x in 0..width {
                let (fx, fy) = (x as f32 / width as f32, y as f32 / height as f32);
                let mut c = [255.0 * fx, 255.0 * fy, 255.0 * (1.0 - fx) * (0.5 + 0.5 * (fy * 6.0).sin())];
                if (x / 16 + y / 16) % 5 == 0 {
                    c = [250.0, 240.0, 30.0];
                }
                if (fx - 0.7).powi(2) + (fy - 0.3).powi(2) < 0.01 {
                    c = [20.0, 30.0, 200.0];
                }
                rgba.extend_from_slice(&[c[0] as u8, c[1] as u8, c[2] as u8, 255]);
            }
        }
        Image { width, height, rgba }
    }

    fn settings(width: usize, height: usize, planes: usize, mode: Mode, chipset: Chipset, dither: Dither) -> Settings {
        Settings { width, height, planes, mode, chipset, dither, fit: Fit::Stretch, filter: Filter::Box, ..Settings::default() }
    }

    #[test]
    fn planes_go_out_and_come_back_both_ways_at_every_fetch() {
        let (w, h) = (37, 9);
        for planes in 1..=8 {
            let values: Vec<u8> = (0..w * h).map(|i| ((i * 7 + i / 5) % (1 << planes)) as u8).collect();
            for interleaved in [false, true] {
                for fetch in Fetch::ALL {
                    let data = to_planes(&values, w, h, planes, interleaved, fetch);
                    assert_eq!(data.len(), row_bytes(w, fetch) * h * planes);
                    assert_eq!(from_planes(&data, w, h, planes, interleaved, fetch), values);
                }
            }
        }
        assert_eq!((row_bytes(320, Fetch::Words), row_bytes(321, Fetch::Words), row_bytes(321, Fetch::Longs), row_bytes(321, Fetch::Quads)), (40, 42, 44, 48));
    }

    #[test]
    fn the_planar_layout_is_as_the_hardware_reads_it() {
        // Two planes, 16 pixels, two rows: pixel 0 is 1 (plane 0), pixel 1 is 2 (plane 1), pixel 15 of row 1 is 3.
        let mut values = vec![0u8; 32];
        values[0] = 1;
        values[1] = 2;
        values[31] = 3;
        let flat = to_planes(&values, 16, 2, 2, false, Fetch::Words);
        assert_eq!(flat, [0x80, 0, 0, 1, 0x40, 0, 0, 1]);
        let interleaved = to_planes(&values, 16, 2, 2, true, Fetch::Words);
        assert_eq!(interleaved, [0x80, 0, 0x40, 0, 0, 1, 0, 1]);
    }

    /// ByteRun1 unpacked, as EA's document describes it.
    fn unpack(mut data: &[u8]) -> Vec<u8> {
        let mut out = Vec::new();
        while let Some((&n, rest)) = data.split_first() {
            let n = n as i8;
            if n >= 0 {
                out.extend_from_slice(&rest[..n as usize + 1]);
                data = &rest[n as usize + 1..];
            } else if n != -128 {
                out.extend(std::iter::repeat_n(rest[0], (1 - i16::from(n)) as usize));
                data = &rest[1..];
            } else {
                data = rest;
            }
        }
        out
    }

    #[test]
    fn bayer_thresholds_are_each_used_once() {
        let mut seen: Vec<usize> = (0..64).map(|i| bayer(i % 8, i / 8) as usize).collect();
        assert_eq!(&seen[..4], &[0, 32, 8, 40]);
        seen.sort_unstable();
        assert_eq!(seen, (0..64).collect::<Vec<_>>());
    }

    #[test]
    fn byterun1_packs_what_the_reader_unpacks() {
        let rows: [&[u8]; 5] = [&[], &[1], &[5, 5, 5, 5, 1, 2, 3, 3, 9], &[7; 300], &(0..=255).collect::<Vec<u8>>()];
        for row in rows {
            let mut packed = Vec::new();
            pack_byterun1(row, &mut packed);
            assert_eq!(unpack(&packed), row);
        }
        let mut packed = Vec::new();
        pack_byterun1(&[0; 40], &mut packed);
        assert_eq!(packed, [(1 - 40i16) as u8, 0]);
    }

    /// Converts, writes an ILBM, reads it back with Limoniter's own reader, and compares every pixel.
    fn ilbm_round_trip(s: &Settings) -> Converted {
        let c = convert(&test_card(97, 61), s);
        for packed in [true, false] {
            let file = to_ilbm(&c, packed);
            let picture = crate::pictures::decode(crate::pictures::Format::Ilbm, &file).unwrap();
            assert_eq!((picture.width, picture.height), (c.width, c.height));
            assert_eq!(picture.rgba, c.rgba, "{:?} {:?}, {} planes, packed {packed}", c.mode, c.chipset, c.planes);
            assert!((picture.aspect - c.resolution.aspect(c.interlaced)).abs() < 0.01);
        }
        let interleaved = to_planes(&c.values, c.width, c.height, c.planes, true, Fetch::Words);
        assert_eq!(from_planes(&interleaved, c.width, c.height, c.planes, true, Fetch::Words), c.values);
        c
    }

    #[test]
    fn every_kind_goes_through_an_ilbm_and_back_unchanged() {
        for planes in 1..=8 {
            for chipset in [Chipset::Ocs, Chipset::Aga] {
                for dither in Dither::ALL {
                    ilbm_round_trip(&settings(97, 61, planes, Mode::Plain, chipset, dither));
                }
            }
        }
        for dither in Dither::ALL {
            // The reader halves extra half-brite colours' eight bits, as AGA does.
            ilbm_round_trip(&settings(97, 61, 6, Mode::Ehb, Chipset::Aga, dither));
            ilbm_round_trip(&settings(97, 61, 6, Mode::Ham6, Chipset::Ocs, dither));
            ilbm_round_trip(&settings(97, 61, 8, Mode::Ham8, Chipset::Aga, dither));
        }
        let mut hires = settings(97, 61, 4, Mode::Plain, Chipset::Ocs, Dither::None);
        hires.resolution = Resolution::Hires;
        hires.interlaced = true;
        ilbm_round_trip(&hires);
    }

    #[test]
    fn ham_values_decode_as_the_hardware_does() {
        let palette = [[0, 0, 0], [0xFF, 0xFF, 0xFF], [0x11, 0x22, 0x33]];
        // HAM6: colour 2, then red to $A, green to $5, blue to $F, then colour 1.
        let values = [0x02, 0x20 | 0xA, 0x30 | 0x5, 0x10 | 0xF, 0x01];
        let rgba = render(&values, 5, &palette, Mode::Ham6, Chipset::Ocs);
        let shown: Vec<[u8; 3]> = rgba.chunks(4).map(|p| [p[0], p[1], p[2]]).collect();
        assert_eq!(shown, [[0x11, 0x22, 0x33], [0xAA, 0x22, 0x33], [0xAA, 0x55, 0x33], [0xAA, 0x55, 0xFF], [0xFF, 0xFF, 0xFF]]);
        // HAM8: the top six bits change, the low two are kept.
        let palette = [[0x03, 0x02, 0x01]];
        let values = [0x00, 0x80 | 0x3F, 0xC0 | 0x01, 0x40 | 0x20];
        let rgba = render(&values, 4, &palette, Mode::Ham8, Chipset::Aga);
        let shown: Vec<[u8; 3]> = rgba.chunks(4).map(|p| [p[0], p[1], p[2]]).collect();
        assert_eq!(shown, [[0x03, 0x02, 0x01], [0xFF, 0x02, 0x01], [0xFF, 0x06, 0x01], [0xFF, 0x06, 0x81]]);
    }

    #[test]
    fn ham_comes_closer_than_its_base_colours_alone() {
        let image = test_card(160, 100);
        let plain16 = convert(&image, &settings(160, 100, 4, Mode::Plain, Chipset::Ocs, Dither::None));
        let ham6 = convert(&image, &settings(160, 100, 6, Mode::Ham6, Chipset::Ocs, Dither::None));
        let plain64 = convert(&image, &settings(160, 100, 6, Mode::Plain, Chipset::Aga, Dither::None));
        let ham8 = convert(&image, &settings(160, 100, 8, Mode::Ham8, Chipset::Aga, Dither::None));
        eprintln!("Oklab error: 16 colours {:.4}, HAM6 {:.4}, 64 colours {:.4}, HAM8 {:.4}", plain16.error, ham6.error, plain64.error, ham8.error);
        assert!(ham6.error < plain16.error);
        assert!(ham8.error < plain64.error);
        assert!(ham8.error < ham6.error);
    }

    #[test]
    fn more_colours_come_closer_and_ehb_helps() {
        let image = test_card(160, 100);
        let mut last = f32::MAX;
        for planes in 1..=8 {
            let c = convert(&image, &settings(160, 100, planes, Mode::Plain, Chipset::Aga, Dither::None));
            eprintln!("{planes} planes: Oklab error {:.4}", c.error);
            assert!(c.error < last, "{planes} planes");
            last = c.error;
        }
        // Extra half-brite helps most where there's shade: the card's top half again below it, in shadow.
        let mut shaded = image.clone();
        let half_way = shaded.rgba.len() / 2;
        let top = shaded.rgba[..half_way].to_vec();
        shaded.rgba[half_way..].iter_mut().zip(top).enumerate().for_each(|(i, (v, t))| *v = if i % 4 == 3 { 255 } else { t / 2 });
        for (name, image) in [("test card", &image), ("shaded card", &shaded)] {
            let five = convert(image, &settings(160, 100, 5, Mode::Plain, Chipset::Ocs, Dither::None));
            let ehb = convert(image, &settings(160, 100, 6, Mode::Ehb, Chipset::Ocs, Dither::None));
            let six = convert(image, &settings(160, 100, 6, Mode::Plain, Chipset::Aga, Dither::None));
            eprintln!("{name}: 32 colours {:.4}, extra half-brite {:.4}, 64 colours {:.4}", five.error, ehb.error, six.error);
            assert!(ehb.error <= five.error);
        }
    }

    #[test]
    fn kept_colours_stay_and_ocs_levels_are_four_bits() {
        let image = test_card(64, 40);
        let mut s = settings(64, 40, 4, Mode::Plain, Chipset::Ocs, Dither::FloydSteinberg);
        let c = convert(&image, &s);
        assert_eq!(c.palette[0], [0, 0, 0]);
        assert!(c.palette.iter().flatten().all(|v| v % 17 == 0));
        s.given = vec![[0xF0, 0x10, 0x10], [0x22, 0x44, 0x66], [0x80, 0x80, 0x80]];
        s.keep_given = 2;
        let c = convert(&image, &s);
        assert_eq!(&c.palette[..2], &[[0xEE, 0x11, 0x11], [0x22, 0x44, 0x66]]);
        s.chipset = Chipset::Aga;
        s.keep_given = 3;
        let c = convert(&image, &s);
        assert_eq!(&c.palette[..3], &s.given[..]);
    }

    #[test]
    fn a_picture_of_few_colours_keeps_them_exactly() {
        let mut rgba = Vec::new();
        let colours = [[0u8, 0, 0], [255, 0, 0], [0, 255, 0], [0, 0, 255], [255, 255, 255], [136, 68, 34]];
        for i in 0..64 * 32 {
            let c = colours[(i / 7 + i / 64) % colours.len()];
            rgba.extend_from_slice(&[c[0], c[1], c[2], 255]);
        }
        let image = Image { width: 64, height: 32, rgba: rgba.clone() };
        let mut s = settings(64, 32, 3, Mode::Plain, Chipset::Ocs, Dither::FloydSteinberg);
        s.filter = Filter::Nearest;
        let c = convert(&image, &s);
        assert_eq!(c.rgba, rgba);
        assert!(c.error < 1e-4);
    }

    #[test]
    fn the_palette_comes_out_as_words_copper_moves_and_c() {
        let palette = [[0x00, 0x00, 0x00], [0xFF, 0x88, 0x11], [0x12, 0x34, 0x56]];
        assert_eq!(palette_words(&palette[..2], Chipset::Ocs), [0x00, 0x00, 0x0F, 0x81]);
        assert_eq!(palette_words(&palette[1..], Chipset::Aga), [0x0F, 0x81, 0x01, 0x35, 0x0F, 0x81, 0x02, 0x46]);
        let copper = palette_copper(&palette[..2], Chipset::Ocs);
        assert_eq!(copper.lines().nth(1).unwrap().trim(), "dc.w    $0182,$0f81    ; COLOR01");
        let aga = palette_copper(&palette, Chipset::Aga);
        let lines: Vec<&str> = aga.lines().map(str::trim).collect();
        assert_eq!(lines[0], "dc.w    $0106,$0c00    ; BPLCON3: bank 0, high halves");
        assert_eq!(lines[3], "dc.w    $0184,$0135    ; COLOR02");
        assert_eq!(lines[4], "dc.w    $0106,$0e00    ; BPLCON3: bank 0, low halves (LOCT)");
        assert_eq!(lines[7], "dc.w    $0184,$0246    ; COLOR02");
        // A second bank for colours from 32.
        let big = vec![[1, 2, 3]; 64];
        assert!(palette_copper(&big, Chipset::Aga).contains("dc.w    $0106,$2c00    ; BPLCON3: bank 1"));
        assert_eq!(palette_copper(&big, Chipset::Aga).lines().count(), 2 * (2 + 64) + 1);
        let c = palette_c(&palette, Chipset::Ocs, "my pic");
        assert!(c.contains("UWORD my_pic_palette[3] = {\n    0x0000, 0x0F81, 0x0135\n};"));
        let c = palette_c(&palette, Chipset::Aga, "pic");
        assert!(c.contains("0x00FF8811"));
        assert!(c.contains("3UL << 16 | 0, 0x00000000"));
        assert!(c.contains("0xFFFFFFFF, 0x88888888, 0x11111111"));
    }

    #[test]
    fn palettes_are_read_from_many_kinds_of_file() {
        let want = vec![[0x00, 0x00, 0x00], [0xFF, 0x88, 0x11]];
        assert_eq!(read_palette(&palette_words(&want, Chipset::Ocs)), Some(want.clone()));
        assert_eq!(read_palette(palette_copper(&want, Chipset::Ocs).as_bytes()), Some(want.clone()));
        assert_eq!(read_palette(palette_c(&want, Chipset::Ocs, "p").as_bytes()), Some(want.clone()));
        assert_eq!(read_palette(b"JASC-PAL\n0100\n2\n0 0 0\n255 136 17\n"), Some(want.clone()));
        assert_eq!(read_palette(b"GIMP Palette\nName: x\n#\n  0   0   0 Black\n255 136  17 Orange\n"), Some(want.clone()));
        assert_eq!(read_palette(&[0, 0, 0, 255, 136, 17, 1, 2, 3]), Some(vec![[0, 0, 0], [255, 136, 17], [1, 2, 3]]));
        assert_eq!(read_palette(b"dc.l $123456, $abcdef\n"), Some(vec![[0x12, 0x34, 0x56], [0xAB, 0xCD, 0xEF]]));
        let image = test_card(32, 16);
        let c = convert(&image, &settings(32, 16, 2, Mode::Plain, Chipset::Aga, Dither::None));
        assert_eq!(read_palette(&to_ilbm(&c, true)), Some(c.palette.clone()));
    }

    #[test]
    fn the_source_has_the_sizes_and_copper_list() {
        let image = test_card(320, 256);
        let c = convert(&image, &settings(320, 256, 5, Mode::Plain, Chipset::Ocs, Dither::None));
        let source = asm_source(&c, "pic", "pic.raw", true, Fetch::Words);
        for want in ["pic_ROWBYTES  equ 40", "pic_MODULO    equ 160", "pic_PLANESTEP equ 40", "$008e,$2c81", "$0090,$2cc1", "$0092,$0038", "$0094,$00d0", "$0100,$5200", "$00f0,0,$00f2,0    ; BPL5PTH, BPL5PTL", "incbin  \"pic.raw\"    ; 51200 bytes"] {
            assert!(source.contains(want), "{want} in\n{source}");
        }
        assert!(!source.contains("FMODE"));
        let flat = layout(&c, false, Fetch::Words);
        assert_eq!((flat.modulo, flat.step, flat.total), (0, 10240, 51200));
        let mut hires = settings(640, 256, 8, Mode::Ham8, Chipset::Aga, Dither::None);
        hires.resolution = Resolution::Hires;
        let c = convert(&test_card(64, 64), &hires);
        assert_eq!(bplcon0(&c), 0x8000 | 0x0010 | 0x0800 | 0x0200);
        let source = asm_source(&c, "pic", "pic.raw", false, Fetch::Quads);
        assert!(source.contains("$0092,$003c") && source.contains("$0094,$00d4") && source.contains("$01fc,$0003"));
    }

    #[test]
    fn scaling_keeps_the_shape_as_asked() {
        // A white square on black, 100 by 100, into a 64 by 32 lo-res screen.
        let image = Image { width: 100, height: 100, rgba: [255u8; 4].repeat(100 * 100) };
        let mut s = settings(64, 32, 1, Mode::Plain, Chipset::Ocs, Dither::None);
        s.fit = Fit::Whole;
        let pixels = scale(&image, &s);
        let white: usize = pixels.iter().filter(|p| p[0] > 128.0).count();
        assert_eq!(white, 32 * 32);
        assert!(pixels[0][0] < 1.0 && pixels[32][0] > 254.0);
        // On a hi-res screen pixels are half as wide, so it's twice as many across.
        s.resolution = Resolution::Hires;
        let white: usize = scale(&image, &s).iter().filter(|p| p[0] > 128.0).count();
        assert_eq!(white, 64 * 32);
        // Filling, all of the screen is picture.
        s.fit = Fit::Fill;
        s.resolution = Resolution::Lores;
        for filter in Filter::ALL {
            s.filter = filter;
            assert!(scale(&image, &s).iter().all(|p| p[0] > 254.0), "{filter:?}");
        }
    }

    #[test]
    fn raw_planes_are_shown_with_a_palette() {
        let image = test_card(48, 20);
        let c = convert(&image, &settings(48, 20, 3, Mode::Plain, Chipset::Ocs, Dither::None));
        let mut file = vec![0xAA; 10];
        file.extend(to_planes(&c.values, 48, 20, 3, true, Fetch::Words));
        let view = RawView { offset: 10, width: 48, height: 20, planes: 3, interleaved: true, fetch: Fetch::Words, mode: Mode::Plain };
        assert_eq!(view.rows_in(file.len()), 20);
        let (values, rgba) = view.draw(&file, &c.palette, Chipset::Ocs);
        assert_eq!(values, c.values);
        assert_eq!(rgba, c.rgba);
    }

    #[test]
    fn jpeg_and_png_pictures_are_read() {
        let mut png = Vec::new();
        {
            let mut encoder = png::Encoder::new(&mut png, 2, 1);
            encoder.set_color(png::ColorType::Rgb);
            encoder.set_depth(png::BitDepth::Eight);
            encoder.write_header().unwrap().write_image_data(&[255, 0, 0, 0, 0, 255]).unwrap();
        }
        let image = read_picture("a.png", &png).unwrap();
        assert_eq!(image.rgba, [255, 0, 0, 255, 0, 0, 255, 255]);
        // A JPEG 16 by 8, red on the left and blue on the right, as ImageMagick wrote it.
        let jpeg = [
            0xFF, 0xD8, 0xFF, 0xE0, 0x00, 0x10, 0x4A, 0x46, 0x49, 0x46, 0x00, 0x01, 0x01, 0x00, 0x00, 0x01, 0x00, 0x01, 0x00, 0x00, 0xFF, 0xDB, 0x00, 0x43,
            0x00, 0x02, 0x01, 0x01, 0x01, 0x01, 0x01, 0x02, 0x01, 0x01, 0x01, 0x02, 0x02, 0x02, 0x02, 0x02, 0x04, 0x03, 0x02, 0x02, 0x02, 0x02, 0x05, 0x04,
            0x04, 0x03, 0x04, 0x06, 0x05, 0x06, 0x06, 0x06, 0x05, 0x06, 0x06, 0x06, 0x07, 0x09, 0x08, 0x06, 0x07, 0x09, 0x07, 0x06, 0x06, 0x08, 0x0B, 0x08,
            0x09, 0x0A, 0x0A, 0x0A, 0x0A, 0x0A, 0x06, 0x08, 0x0B, 0x0C, 0x0B, 0x0A, 0x0C, 0x09, 0x0A, 0x0A, 0x0A, 0xFF, 0xDB, 0x00, 0x43, 0x01, 0x02, 0x02,
            0x02, 0x02, 0x02, 0x02, 0x05, 0x03, 0x03, 0x05, 0x0A, 0x07, 0x06, 0x07, 0x0A, 0x0A, 0x0A, 0x0A, 0x0A, 0x0A, 0x0A, 0x0A, 0x0A, 0x0A, 0x0A, 0x0A,
            0x0A, 0x0A, 0x0A, 0x0A, 0x0A, 0x0A, 0x0A, 0x0A, 0x0A, 0x0A, 0x0A, 0x0A, 0x0A, 0x0A, 0x0A, 0x0A, 0x0A, 0x0A, 0x0A, 0x0A, 0x0A, 0x0A, 0x0A, 0x0A,
            0x0A, 0x0A, 0x0A, 0x0A, 0x0A, 0x0A, 0x0A, 0x0A, 0x0A, 0x0A, 0x0A, 0x0A, 0x0A, 0x0A, 0xFF, 0xC0, 0x00, 0x11, 0x08, 0x00, 0x08, 0x00, 0x10, 0x03,
            0x01, 0x11, 0x00, 0x02, 0x11, 0x01, 0x03, 0x11, 0x01, 0xFF, 0xC4, 0x00, 0x14, 0x00, 0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
            0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x08, 0xFF, 0xC4, 0x00, 0x14, 0x10, 0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
            0x00, 0x00, 0x00, 0x00, 0x00, 0xFF, 0xC4, 0x00, 0x16, 0x01, 0x01, 0x01, 0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
            0x00, 0x00, 0x0A, 0x08, 0x09, 0xFF, 0xC4, 0x00, 0x14, 0x11, 0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
            0x00, 0x00, 0x00, 0xFF, 0xDA, 0x00, 0x0C, 0x03, 0x01, 0x00, 0x02, 0x11, 0x03, 0x11, 0x00, 0x3F, 0x00, 0x17, 0xA5, 0x36, 0xFE, 0x08, 0x65, 0x50,
            0x35, 0xEF, 0xFF, 0xD9,
        ];
        let image = read_picture("a.jpg", &jpeg).unwrap();
        assert_eq!((image.width, image.height, image.rgba.len()), (16, 8, 16 * 8 * 4));
        let near = |at: usize, want: [u8; 3]| (0..3).all(|c| image.rgba[at * 4 + c].abs_diff(want[c]) < 8);
        assert!(near(0, [255, 0, 0]) && near(16 * 8 - 1, [0, 0, 255]), "{:?}", &image.rgba[..8]);
        assert!(read_picture("nothing.bin", b"nothing").is_err());
    }

    fn ppm(width: usize, height: usize, rgba: &[u8]) -> Vec<u8> {
        let mut out = format!("P6\n{width} {height}\n255\n").into_bytes();
        out.extend(rgba.chunks(4).flat_map(|p| [p[0], p[1], p[2]]));
        out
    }

    /// A P6 PPM's size and pixels as red, green, blue and alpha.
    fn read_ppm(bytes: &[u8]) -> (usize, usize, Vec<u8>) {
        let mut fields = Vec::new();
        let mut at = 2;
        while fields.len() < 3 {
            while bytes[at].is_ascii_whitespace() {
                at += 1;
            }
            if bytes[at] == b'#' {
                while bytes[at] != b'\n' {
                    at += 1;
                }
                continue;
            }
            let start = at;
            while bytes[at].is_ascii_digit() {
                at += 1;
            }
            fields.push(std::str::from_utf8(&bytes[start..at]).unwrap().parse::<usize>().unwrap());
        }
        assert_eq!(fields[2], 255);
        let data = &bytes[at + 1..];
        (fields[0], fields[1], data.chunks(3).flat_map(|p| [p[0], p[1], p[2], 255]).collect())
    }

    fn body(ilbm: &[u8]) -> &[u8] {
        let at = ilbm.windows(4).position(|w| w == b"BODY").unwrap();
        let len = u32::from_be_bytes(ilbm[at + 4..at + 8].try_into().unwrap()) as usize;
        &ilbm[at + 8..at + 8 + len]
    }

    /// Netpbm's ilbmtoppm and ppmtoilbm, where they're installed, as a second opinion: ILBMs written here are read
    /// by ilbmtoppm to the same pixels; ppmtoilbm, given the same palette, lays out the same planes byte for byte;
    /// and its own HAM pictures are read by Limoniter's reader as ilbmtoppm reads them. LIMONITER_NETPBM names the
    /// folder the programs are in, otherwise they're looked for on the path. Skipped without Netpbm; the byte for
    /// byte comparison is passed over with Netpbm 10.0 (as Ubuntu has), whose ppmtoilbm -map fails.
    #[test]
    fn netpbm_agrees() {
        use std::process::Command;
        let tool = |name: &str| std::env::var_os("LIMONITER_NETPBM").map_or_else(|| std::path::PathBuf::from(name), |d| std::path::PathBuf::from(d).join(name));
        if Command::new(tool("ilbmtoppm")).arg("-version").output().is_err() {
            eprintln!("Netpbm isn't installed: skipped");
            return;
        }
        let dir = std::env::temp_dir().join(format!("limoniter-planar-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let try_run = |name: &str, args: &[&str], input: &std::path::Path| -> Result<Vec<u8>, String> {
            let out = Command::new(tool(name)).args(args).arg(input).output().map_err(|e| e.to_string())?;
            if out.status.success() && !out.stdout.is_empty() { Ok(out.stdout) } else { Err(crate::i18n::tf!("{name} {args:?}: {}", String::from_utf8_lossy(&out.stderr))) }
        };
        let run = |name: &str, args: &[&str], input: &std::path::Path| -> Vec<u8> { try_run(name, args, input).unwrap() };
        let image = test_card(97, 61);
        let mut cases = Vec::new();
        for planes in 1..=8 {
            cases.push(settings(97, 61, planes, Mode::Plain, if planes > 5 { Chipset::Aga } else { Chipset::Ocs }, Dither::FloydSteinberg));
        }
        cases.push(settings(97, 61, 6, Mode::Ehb, Chipset::Aga, Dither::Ordered));
        cases.push(settings(97, 61, 6, Mode::Ehb, Chipset::Ocs, Dither::None));
        cases.push(settings(97, 61, 6, Mode::Ham6, Chipset::Ocs, Dither::FloydSteinberg));
        cases.push(settings(97, 61, 8, Mode::Ham8, Chipset::Aga, Dither::FloydSteinberg));
        let (mut pixels, mut differing, mut bodies_same) = (0, 0, 0);
        for (n, s) in cases.iter().enumerate() {
            let c = convert(&image, s);
            // Ours, read by ilbmtoppm.
            let ours = dir.join(format!("{n}.iff"));
            std::fs::write(&ours, to_ilbm(&c, true)).unwrap();
            let (w, h, rgba) = read_ppm(&run("ilbmtoppm", &[], &ours));
            assert_eq!((w, h), (c.width, c.height));
            // ilbmtoppm treats every picture as AGA would: HAM6 sets a level's top four bits, keeping the low four
            // from the pixel before, and half-brite halves all eight. On OCS the four bits are all there is, so
            // where the colours are OCS's, the top four bits are compared.
            let twelve = c.chipset == Chipset::Ocs && c.mode != Mode::Plain;
            let wrong = rgba.chunks(4).zip(c.rgba.chunks(4)).filter(|(a, b)| if twelve { a.iter().zip(b.iter()).any(|(x, y)| x >> 4 != y >> 4) } else { a != b }).count();
            let exact = rgba.chunks(4).zip(c.rgba.chunks(4)).filter(|(a, b)| a != b).count();
            eprintln!("{:?} {:?} {} planes: ilbmtoppm reads {} of {} pixels differently{}", c.mode, c.chipset, c.planes, wrong, w * h, if twelve { format!(" in their top four bits ({exact} in all eight)") } else { String::new() });
            pixels += w * h;
            differing += wrong;
            // Theirs, from the same pixels and palette, when every colour in it is different.
            let mut unique = c.palette.clone();
            unique.sort_unstable();
            unique.dedup();
            if c.mode == Mode::Plain && unique.len() == c.palette.len() {
                let map = dir.join(format!("{n}-map.ppm"));
                let picture = dir.join(format!("{n}.ppm"));
                let palette_rgba: Vec<u8> = c.palette.iter().flat_map(|p| [p[0], p[1], p[2], 255]).collect();
                std::fs::write(&map, ppm(c.palette.len(), 1, &palette_rgba)).unwrap();
                std::fs::write(&picture, ppm(c.width, c.height, &c.rgba)).unwrap();
                let Some(theirs) = try_run("ppmtoilbm", &["-maxplanes", "8", "-map", map.to_str().unwrap(), "-nocompress"], &picture).ok().filter(|t| !body(t).is_empty()) else {
                    eprintln!("ppmtoilbm -map failed: an old Netpbm; the planes aren't compared byte for byte");
                    continue;
                };
                assert_eq!(body(&theirs), to_planes(&c.values, c.width, c.height, c.planes, true, Fetch::Words), "{} planes", c.planes);
                let theirs = crate::pictures::decode(crate::pictures::Format::Ilbm, &run("ppmtoilbm", &["-maxplanes", "8", "-map", map.to_str().unwrap(), "-compress"], &picture)).unwrap();
                assert_eq!(theirs.rgba, c.rgba);
                bodies_same += 1;
            }
        }
        // Their HAM, read by both.
        for (flag, planes) in [("-ham6", 6), ("-ham8", 8)] {
            let picture = dir.join(format!("ham{planes}.ppm"));
            std::fs::write(&picture, ppm(image.width, image.height, &image.rgba)).unwrap();
            let theirs = run("ppmtoilbm", &[flag], &picture);
            let file = dir.join(format!("ham{planes}.iff"));
            std::fs::write(&file, &theirs).unwrap();
            let (_, _, rgba) = read_ppm(&run("ilbmtoppm", &[], &file));
            let ours = crate::pictures::decode(crate::pictures::Format::Ilbm, &theirs).unwrap();
            let shift = if planes == 6 { 4 } else { 0 };
            let wrong = rgba.chunks(4).zip(ours.rgba.chunks(4)).filter(|(a, b)| a.iter().zip(b.iter()).any(|(x, y)| x >> shift != y >> shift)).count();
            eprintln!("ppmtoilbm {flag}: Limoniter reads {wrong} of {} pixels differently from ilbmtoppm{}", image.width * image.height, if shift > 0 { " in their top four bits" } else { "" });
            assert_eq!(wrong, 0);
        }
        eprintln!("{differing} of {pixels} pixels differ; {bodies_same} BODYs byte for byte the same as ppmtoilbm's");
        let _ = std::fs::remove_dir_all(&dir);
        assert_eq!(differing, 0);
    }
}
