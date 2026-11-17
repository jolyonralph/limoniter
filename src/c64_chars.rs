//! Commodore 64 characters and sprites made from a picture.
//!
//! Characters: the picture cut into 8x8 cells, each given its colour (colour RAM) and a character whose bits draw
//! it, the same characters shared so that the set fits 256 (128 with their inverses taking the other half, or 64 in
//! extended background colour mode). In multicolour each pair of bits is a pixel twice as wide: 00 the background,
//! 01 and 10 the two shared colours ($D022, $D023), 11 the cell's own, one of the first eight. PETSCII matches each
//! cell to a character already in the ROM (a character ROM file is needed for its shapes).
//!
//! Sprites: 24x21 frames cut from the picture, each 63 bytes (and a 64th, SpritePad's, for its colour and kind).
//! Hires sprites have one colour; multicolour ones are 12x21 with pixels twice as wide, their own colour (10) and
//! two shared by every sprite (01 $D025, 11 $D026). A hires sprite can lie on a multicolour one for detail.
//! SpritePad's .spd files are read and written.

use crate::c64_gfx::{self as gfx, Allowed, Palette, assign, basic_stub, best_set, candidates, data};
use crate::quantise::{self, Dither, Filter, Fit, Image, as_f32, oklab};
use std::fmt::Write as _;

// Characters.

/// What kind of character screen is made.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CharMode {
    /// One colour in each cell on the background.
    Hires,
    /// Pixels twice as wide: the background, two shared colours and the cell's.
    Multicolour,
    /// Extended background colour: 64 characters, each cell one of four backgrounds and its own colour.
    Extended,
    /// The ROM's characters, as PETSCII art is drawn.
    Petscii,
}

impl CharMode {
    pub const ALL: [Self; 4] = [Self::Hires, Self::Multicolour, Self::Extended, Self::Petscii];

    pub fn name(self) -> &'static str {
        match self {
            Self::Hires => "Hires characters",
            Self::Multicolour => "Multicolour characters",
            Self::Extended => "Extended background colour (64 characters)",
            Self::Petscii => "PETSCII (the ROM's characters)",
        }
    }

    fn most(self) -> usize {
        if self == Self::Extended { 64 } else { 256 }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct CharSettings {
    pub mode: CharMode,
    pub palette: Palette,
    pub fit: Fit,
    pub filter: Filter,
    pub dither: Dither,
    pub strength: f32,
    /// Cells across and down.
    pub columns: usize,
    pub rows: usize,
    /// The background ($D021), or `None` to find it.
    pub background: Option<u8>,
    /// At most this many characters (no more than the mode allows).
    pub most: usize,
    /// Characters 128 to 255 the inverses of 0 to 127, so that a cell can use a character's inverse.
    pub inverses: bool,
    /// For PETSCII: the lower case set rather than upper case and graphics.
    pub lower_case: bool,
}

impl Default for CharSettings {
    fn default() -> Self {
        CharSettings { mode: CharMode::Hires, palette: Palette::Pepto, fit: Fit::Fill, filter: Filter::Lanczos, dither: Dither::None, strength: 0.6, columns: 40, rows: 25, background: None, most: 256, inverses: false, lower_case: false }
    }
}

/// A character screen as the C64 holds it.
#[derive(Clone, Debug, PartialEq)]
pub struct CharScreen {
    pub mode: CharMode,
    pub columns: usize,
    pub rows: usize,
    /// The characters, 8 bytes each (none for PETSCII, which uses the ROM's).
    pub charset: Vec<[u8; 8]>,
    /// Each cell's screen code.
    pub screen: Vec<u8>,
    /// Each cell's colour RAM.
    pub colour: Vec<u8>,
    /// $D021 to $D024: the background, then multicolour's two shared colours or extended mode's other backgrounds.
    pub backgrounds: [u8; 4],
    pub border: u8,
    /// How many cells there were with different characters before they were shared out.
    pub unique: usize,
    pub error: f32,
    /// PETSCII: which ROM set.
    pub lower_case: bool,
}

/// The shapes of a character ROM: 2048 bytes a set, upper case and graphics first, then lower case.
pub fn rom_set(rom: &[u8], lower_case: bool) -> Option<Vec<[u8; 8]>> {
    let start = if lower_case { 2048 } else { 0 };
    let set = rom.get(start..start + 2048)?;
    Some(set.chunks(8).map(|c| std::array::from_fn(|i| c[i])).collect())
}

impl CharScreen {
    /// The colour of each pixel, `columns * 8` by `rows * 8`, as the VIC-II shows it. `rom` gives PETSCII's shapes.
    pub fn indices(&self, rom: Option<&[[u8; 8]]>) -> Vec<u8> {
        let width = self.columns * 8;
        let mut out = vec![self.backgrounds[0]; width * self.rows * 8];
        for cell in 0..self.columns * self.rows {
            let (cx, cy) = (cell % self.columns, cell / self.columns);
            let code = self.screen[cell];
            let colour = self.colour[cell] & 15;
            let (shape, background) = match self.mode {
                CharMode::Extended => (self.charset.get(usize::from(code & 63)), self.backgrounds[usize::from(code >> 6)]),
                CharMode::Petscii => (rom.and_then(|r| r.get(usize::from(code))), self.backgrounds[0]),
                _ => (self.charset.get(usize::from(code)), self.backgrounds[0]),
            };
            let Some(shape) = shape else { continue };
            for (row, &byte) in shape.iter().enumerate() {
                for x in 0..8 {
                    let index = if self.mode == CharMode::Multicolour && colour & 8 != 0 {
                        match (byte >> (6 - (x & 6))) & 3 {
                            0 => self.backgrounds[0],
                            1 => self.backgrounds[1],
                            2 => self.backgrounds[2],
                            _ => colour & 7,
                        }
                    } else if byte & (0x80 >> x) != 0 {
                        if self.mode == CharMode::Multicolour { colour & 7 } else { colour }
                    } else {
                        background
                    };
                    out[(cy * 8 + row) * width + cx * 8 + x] = index;
                }
            }
        }
        out
    }

    pub fn charset_bytes(&self) -> Vec<u8> {
        self.charset.concat()
    }

    /// Source (ACME) for a program showing the screen, with the characters, the screen and its colours.
    pub fn viewer_source(&self, name: &str) -> String {
        let label = quantise::identifier(name);
        let mut s = String::new();
        let _ = writeln!(s, "; {name}: a Commodore 64 character screen ({}), {} by {} characters.", self.mode.name(), self.columns, self.rows);
        s.push_str("; Made by Limoniter's C64 converter, for ACME (acme -f cbm -o out.prg this.asm).\n");
        if self.mode != CharMode::Petscii {
            let _ = writeln!(s, "; {} characters at $2000; the screen RAM's codes are their numbers.", self.charset.len());
        }
        s.push('\n');
        basic_stub(&mut s);
        let _ = writeln!(s, "        sei\n        lda #${:02X}\n        sta $D020", self.border);
        for (i, b) in self.backgrounds.iter().enumerate() {
            let _ = writeln!(s, "        lda #${b:02X}\n        sta $D02{}", i + 1);
        }
        // Cells off the given size are spaces in the background, and in PETSCII spaces are code 32.
        s.push_str("        ldx #0\ncopy:\n");
        for offset in ["$000", "$100", "$200", "$2E8"] {
            let _ = writeln!(s, "        lda {label}_screen+{offset},x\n        sta $0400+{offset},x\n        lda {label}_colour+{offset},x\n        sta $D800+{offset},x");
        }
        s.push_str("        inx\n        bne copy\n");
        let (d011, d016, d018) = match self.mode {
            CharMode::Hires => (0x1B, 0xC8, 0x18),
            CharMode::Multicolour => (0x1B, 0xD8, 0x18),
            CharMode::Extended => (0x5B, 0xC8, 0x18),
            CharMode::Petscii => (0x1B, 0xC8, if self.lower_case { 0x16 } else { 0x14 }),
        };
        let _ = writeln!(s, "        lda #${d011:02X}\n        sta $D011\n        lda #${d016:02X}\n        sta $D016\n        lda #${d018:02X}\n        sta $D018\n        lda $DD00\n        ora #3              ; VIC bank 0\n        sta $DD00\nhold:\n        jmp hold\n");
        let (screen, colour) = self.full_screen();
        data(&mut s, &format!("{label}_screen"), &screen);
        data(&mut s, &format!("{label}_colour"), &colour);
        if self.mode != CharMode::Petscii {
            s.push_str("\n        * = $2000\n");
            data(&mut s, &format!("{label}_charset"), &self.charset_bytes());
        }
        s
    }

    /// The screen and colour RAM, 40 by 25, with the cells off the picture blank.
    pub fn full_screen(&self) -> (Vec<u8>, Vec<u8>) {
        let blank = match self.mode {
            CharMode::Petscii => 32,
            _ => self.blank_code(),
        };
        let mut screen = vec![blank; 1000];
        let mut colour = vec![self.backgrounds[0]; 1000];
        for cell in 0..self.columns.min(40) * self.rows.min(25) {
            let (cx, cy) = (cell % self.columns.min(40), cell / self.columns.min(40));
            screen[cy * 40 + cx] = self.screen[cy * self.columns + cx];
            colour[cy * 40 + cx] = self.colour[cy * self.columns + cx];
        }
        (screen, colour)
    }

    /// A character with no bits set, if there is one.
    fn blank_code(&self) -> u8 {
        self.charset.iter().position(|c| c.iter().all(|&b| b == 0)).unwrap_or(0) as u8
    }

    pub fn viewer(&self, name: &str) -> Result<Vec<u8>, String> {
        gfx::assemble_prg(&self.viewer_source(name))
    }

    pub fn everything(&self, name: &str) -> Vec<(String, Vec<u8>)> {
        let mut files = Vec::new();
        if self.mode != CharMode::Petscii {
            files.push((format!("{name}_charset.bin"), self.charset_bytes()));
        }
        files.push((format!("{name}_screen.bin"), self.screen.clone()));
        files.push((format!("{name}_colour.bin"), self.colour.clone()));
        files.push((format!("{name}.asm"), self.viewer_source(name).into_bytes()));
        if let Ok(prg) = self.viewer(name) {
            files.push((format!("{name}_viewer.prg"), prg));
        }
        files
    }
}

/// Bits that differ between two characters.
fn difference(a: &[u8; 8], b: &[u8; 8], multicolour: bool) -> u32 {
    (0..8)
        .map(|i| {
            let x = a[i] ^ b[i];
            if multicolour {
                // A pixel is a pair of bits.
                ((x | x >> 1) & 0x55).count_ones()
            } else {
                x.count_ones()
            }
        })
        .sum()
}

/// The cells' characters shared out into at most `most`: the same ones once, and if there are still too many, the
/// most used kept and each of the others turned into the one nearest it, each kept one then made what its cells
/// have most.
fn share(cells: &[[u8; 8]], most: usize, multicolour: bool) -> (Vec<[u8; 8]>, Vec<usize>, usize) {
    let mut found: Vec<([u8; 8], usize)> = Vec::new();
    let mut index = std::collections::HashMap::new();
    for c in cells {
        let at = *index.entry(*c).or_insert_with(|| {
            found.push((*c, 0));
            found.len() - 1
        });
        found[at].1 += 1;
    }
    let unique = found.len();
    if unique <= most {
        let set: Vec<[u8; 8]> = found.iter().map(|f| f.0).collect();
        let of = cells.iter().map(|c| index[c]).collect();
        return (set, of, unique);
    }
    let mut order: Vec<usize> = (0..unique).collect();
    order.sort_by_key(|&i| std::cmp::Reverse(found[i].1));
    let mut kept: Vec<[u8; 8]> = order[..most].iter().map(|&i| found[i].0).collect();
    let mut to = vec![0usize; unique];
    for _ in 0..6 {
        for (i, f) in found.iter().enumerate() {
            to[i] = (0..kept.len()).min_by_key(|&k| difference(&f.0, &kept[k], multicolour)).unwrap_or(0);
        }
        // Each kept one becomes, bit by bit (or pixel by pixel), what most of its cells have.
        for (k, shape) in kept.iter_mut().enumerate() {
            let mut votes = [[0usize; 4]; 32];
            let mut any = false;
            for (i, f) in found.iter().enumerate().filter(|(i, _)| to[*i] == k) {
                let _ = i;
                any = true;
                for row in 0..8 {
                    for p in 0..4 {
                        votes[row * 4 + p][usize::from((f.0[row] >> (6 - 2 * p)) & 3)] += f.1;
                    }
                }
            }
            if !any {
                continue;
            }
            for row in 0..8 {
                let mut byte = 0u8;
                for p in 0..4 {
                    let v = &votes[row * 4 + p];
                    let pair = if multicolour {
                        (0..4).max_by_key(|&c| v[c]).unwrap_or(0) as u8
                    } else {
                        // Each bit of the pair counted on its own.
                        let high = v[2] + v[3] > v[0] + v[1];
                        let low = v[1] + v[3] > v[0] + v[2];
                        u8::from(high) << 1 | u8::from(low)
                    };
                    byte |= pair << (6 - 2 * p);
                }
                shape[row] = byte;
            }
        }
    }
    let of = cells.iter().map(|c| to[index[c]]).collect();
    (kept, of, unique)
}

/// The picture made into characters. `rom` is the character ROM for PETSCII.
pub fn convert_chars(image: &Image, settings: &CharSettings, rom: Option<&[u8]>) -> Result<CharScreen, String> {
    let (columns, rows) = (settings.columns.clamp(1, 40), settings.rows.clamp(1, 25));
    let multicolour = settings.mode == CharMode::Multicolour;
    let width = columns * if multicolour { 4 } else { 8 };
    let height = rows * 8;
    let aspect = if multicolour { gfx::PIXEL_ASPECT * 2.0 } else { gfx::PIXEL_ASPECT };
    let pixels = quantise::scale(image, width, height, aspect, settings.fit, settings.filter);
    convert_char_pixels(&pixels, columns, rows, settings, rom)
}

/// Pixels already at the size (4 or 8 a cell across, 8 down) made into characters.
pub fn convert_char_pixels(pixels: &[[f32; 3]], columns: usize, rows: usize, settings: &CharSettings, rom: Option<&[u8]>) -> Result<CharScreen, String> {
    let mode = settings.mode;
    let colours = settings.palette.colours();
    let palette: [[f32; 3]; 16] = std::array::from_fn(|c| oklab(as_f32(colours[c])));
    let multicolour = mode == CharMode::Multicolour;
    let per_cell = if multicolour { 4 } else { 8 };
    let width = columns * per_cell;
    let height = rows * 8;
    let labs: Vec<[f32; 3]> = pixels.iter().map(|&p| oklab(p)).collect();
    let cell_of = move |x: usize, y: usize| (y / 8) * columns + x / per_cell;
    let cells = gfx::gather(&labs, width, height, columns * rows, &cell_of);
    let mix = settings.dither != Dither::None;
    let mut used = gfx::backgrounds(&labs, &palette, 16);
    let background = settings.background.map_or(used[0], |b| b & 15);
    used.retain(|&c| c != background);
    let mut backgrounds = [background, 0, 0, 0];
    let mut colour = vec![0u8; columns * rows];
    let mut screen = vec![0u8; columns * rows];
    if mode == CharMode::Petscii {
        let rom = rom.ok_or(crate::i18n::tr("PETSCII needs the shapes of the C64's characters: open a character ROM file (the 4096-byte chargen, as VICE keeps it)."))?;
        let set = rom_set(rom, settings.lower_case).ok_or(crate::i18n::tr("The character ROM is too short: it should be 4096 bytes (or 2048 for one set)."))?;
        let mut total = 0.0;
        for (cell, px) in cells.iter().enumerate() {
            let away: Vec<f32> = px.iter().map(|&p| quantise::distance(p, palette[usize::from(background)])).collect();
            let base: f32 = away.iter().sum();
            let mut best = (f32::MAX, 32u8, 1u8);
            for fg in candidates(px, &palette, 4).into_iter().chain([1]) {
                let gain: Vec<f32> = px.iter().zip(&away).map(|(&p, &a)| quantise::distance(p, palette[usize::from(fg)]) - a).collect();
                for (code, shape) in set.iter().enumerate() {
                    let mut score = base;
                    for (row, &byte) in shape.iter().enumerate() {
                        let mut bits = byte;
                        while bits != 0 {
                            let x = bits.leading_zeros() as usize;
                            score += gain[row * 8 + x];
                            bits &= !(0x80 >> x);
                        }
                    }
                    if score < best.0 {
                        best = (score, code as u8, fg);
                    }
                }
            }
            total += best.0;
            screen[cell] = best.1;
            colour[cell] = best.2;
        }
        let _ = total;
        let mut out = CharScreen { mode, columns, rows, charset: Vec::new(), screen, colour, backgrounds, border: background, unique: 0, error: 0.0, lower_case: settings.lower_case };
        out.error = error_of(&out, pixels, colours, Some(&set));
        return Ok(out);
    }
    // The colours each cell may use.
    let allowed: Vec<Allowed> = match mode {
        CharMode::Hires => cells
            .iter()
            .enumerate()
            .map(|(cell, px)| {
                let from: Vec<u8> = candidates(px, &palette, 6).into_iter().filter(|&c| c != background).collect();
                let (set, _) = best_set(px, &palette, &[background], &from, 1, mix);
                colour[cell] = set[0];
                vec![(0, background), (1, set[0])]
            })
            .collect(),
        CharMode::Multicolour => {
            backgrounds[1] = used.first().copied().unwrap_or(1);
            backgrounds[2] = used.get(1).copied().unwrap_or(2);
            let shared = [background, backgrounds[1], backgrounds[2]];
            cells
                .iter()
                .enumerate()
                .map(|(cell, px)| {
                    let from: Vec<u8> = candidates(px, &palette, 6).into_iter().filter(|&c| c < 8).collect();
                    let from = if from.is_empty() { vec![0] } else { from };
                    let (set, _) = best_set(px, &palette, &shared, &from, 1, mix);
                    colour[cell] = set[0] | 8;
                    vec![(0, shared[0]), (1, shared[1]), (2, shared[2]), (3, set[0])]
                })
                .collect()
        }
        _ => {
            for (i, &c) in used.iter().take(3).enumerate() {
                backgrounds[i + 1] = c;
            }
            cells
                .iter()
                .enumerate()
                .map(|(cell, px)| {
                    let mut best = (f32::MAX, 0usize, 0u8);
                    for (b, &bg) in backgrounds.iter().enumerate() {
                        let from: Vec<u8> = candidates(px, &palette, 6).into_iter().filter(|&c| c != bg).collect();
                        let (set, c) = best_set(px, &palette, &[bg], &from, 1, mix);
                        if c < best.0 {
                            best = (c, b, set[0]);
                        }
                    }
                    colour[cell] = best.2;
                    // The background's number goes in the screen code's top two bits.
                    screen[cell] = (best.1 as u8) << 6;
                    vec![(0, backgrounds[best.1]), (1, best.2)]
                })
                .collect()
        }
    };
    let values = assign(pixels, width, height, &cell_of, &allowed, colours, settings.dither, settings.strength);
    let mut shapes = vec![[0u8; 8]; columns * rows];
    for y in 0..height {
        for x in 0..width {
            let shift = if multicolour { 2 * (3 - x % 4) } else { 7 - x % 8 };
            shapes[cell_of(x, y)][y % 8] |= values[y * width + x] << shift;
        }
    }
    let most = settings.most.clamp(1, mode.most());
    let inverses = settings.inverses && mode == CharMode::Hires;
    let (charset, of, unique) = if inverses {
        // Each cell's shape or its inverse, whichever is smaller, so that a shape and its inverse are one.
        let flipped: Vec<bool> = shapes.iter().map(|s| s.map(|b| !b) < *s).collect();
        let plain: Vec<[u8; 8]> = shapes.iter().zip(&flipped).map(|(s, &f)| if f { s.map(|b| !b) } else { *s }).collect();
        let (mut set, of, unique) = share(&plain, most.min(128), false);
        set.resize(128, [0; 8]);
        let inverse: Vec<[u8; 8]> = set.iter().map(|s| s.map(|b| !b)).collect();
        set.extend(inverse);
        let of: Vec<usize> = of.iter().zip(&flipped).map(|(&i, &f)| if f { i + 128 } else { i }).collect();
        (set, of, unique)
    } else {
        share(&shapes, most, multicolour)
    };
    for (cell, &code) in of.iter().enumerate() {
        screen[cell] |= code as u8;
    }
    let mut out = CharScreen { mode, columns, rows, charset, screen, colour, backgrounds, border: background, unique, error: 0.0, lower_case: false };
    out.error = error_of(&out, pixels, colours, None);
    Ok(out)
}

fn error_of(screen: &CharScreen, pixels: &[[f32; 3]], colours: &[[u8; 3]; 16], rom: Option<&[[u8; 8]]>) -> f32 {
    let shown = screen.indices(rom);
    let full = screen.columns * 8;
    let width = if screen.mode == CharMode::Multicolour { screen.columns * 4 } else { full };
    let step = full / width;
    let rgba: Vec<u8> = (0..pixels.len()).flat_map(|i| {
        let c = colours[usize::from(shown[(i / width) * full + (i % width) * step])];
        [c[0], c[1], c[2], 255]
    }).collect();
    quantise::average_error(pixels, &rgba)
}

// Sprites.

/// A sprite's 63 bytes as pixels: 24x21 bits, or for multicolour 12x21 values 0 to 3 (0 see-through, 1 $D025, 2
/// the sprite's colour, 3 $D026).
pub fn sprite_pixels(block: &[u8], multicolour: bool) -> Vec<u8> {
    let at = |i: usize| block.get(i).copied().unwrap_or(0);
    if multicolour {
        (0..12 * 21).map(|i| (at((i / 12) * 3 + (i % 12) / 4) >> (6 - 2 * (i % 4))) & 3).collect()
    } else {
        (0..24 * 21).map(|i| (at((i / 24) * 3 + (i % 24) / 8) >> (7 - i % 8)) & 1).collect()
    }
}

/// Pixels made into a sprite's 64 bytes, the last SpritePad's: the colour, bit 4 for a sprite with a hires one
/// lying on it, bit 7 for multicolour.
pub fn sprite_block(pixels: &[u8], multicolour: bool, colour: u8, overlaid: bool) -> [u8; 64] {
    let mut block = [0u8; 64];
    if multicolour {
        for (i, &p) in pixels.iter().take(12 * 21).enumerate() {
            block[(i / 12) * 3 + (i % 12) / 4] |= (p & 3) << (6 - 2 * (i % 4));
        }
    } else {
        for (i, &p) in pixels.iter().take(24 * 21).enumerate() {
            block[(i / 24) * 3 + (i % 24) / 8] |= (p & 1) << (7 - i % 8);
        }
    }
    block[63] = (colour & 15) | if overlaid { 0x10 } else { 0 } | if multicolour { 0x80 } else { 0 };
    block
}

/// A set of sprites as SpritePad keeps them.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SpriteSet {
    pub background: u8,
    pub multicolour1: u8,
    pub multicolour2: u8,
    /// 64 bytes each: 63 of pixels, then the colour and kind.
    pub sprites: Vec<[u8; 64]>,
}

impl SpriteSet {
    /// SpritePad 2's .spd: "SPD", version 1, the number of sprites less one, of animations less one, the three
    /// shared colours, the sprites, and one animation of the first sprite.
    pub fn to_spd(&self) -> Vec<u8> {
        let mut out = b"SPD".to_vec();
        out.extend([1, (self.sprites.len().clamp(1, 256) - 1) as u8, 0, self.background, self.multicolour1, self.multicolour2]);
        for s in self.sprites.iter().take(256) {
            out.extend(s);
        }
        // The animation: its first and last sprite, its speed and its flags.
        out.extend([0, 0, 1, 0]);
        out
    }

    /// A SpritePad file: the newer kind with "SPD" first, or the older, which is just the three colours and the
    /// sprites.
    pub fn from_spd(bytes: &[u8]) -> Option<SpriteSet> {
        let (colours, count) = if bytes.starts_with(b"SPD") {
            (*bytes.get(3)? >= 1).then_some(())?;
            (6, usize::from(*bytes.get(4)?) + 1)
        } else {
            if bytes.len() < 3 + 64 || !(bytes.len() - 3).is_multiple_of(64) {
                return None;
            }
            (0, (bytes.len() - 3) / 64)
        };
        let start = colours + 3;
        let sprites: Vec<[u8; 64]> = (0..count).map_while(|i| bytes.get(start + i * 64..start + i * 64 + 64).map(|b| std::array::from_fn(|j| b[j]))).collect();
        if sprites.is_empty() {
            return None;
        }
        Some(SpriteSet { background: bytes[colours] & 15, multicolour1: bytes[colours + 1] & 15, multicolour2: bytes[colours + 2] & 15, sprites })
    }

    /// The sprites' bytes, 64 each, as kept in memory.
    pub fn data(&self) -> Vec<u8> {
        self.sprites.concat()
    }
}

/// Which pixels of a picture are see-through.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SeeThrough {
    /// Where the picture is.
    Alpha,
    /// Where it's the colour of its top left pixel.
    TopLeft,
    Nowhere,
}

#[derive(Clone, Debug, PartialEq)]
pub struct SpriteSettings {
    pub multicolour: bool,
    /// A hires sprite on each multicolour one, for detail in one more colour.
    pub overlay: bool,
    pub expand_x: bool,
    pub expand_y: bool,
    pub see_through: SeeThrough,
    pub palette: Palette,
    pub dither: Dither,
    pub strength: f32,
    /// $D025 and $D026, or found from the picture.
    pub shared: Option<(u8, u8)>,
    /// Where the first sprite goes in memory, for its pointer.
    pub address: u16,
    /// Where the screen RAM is, whose last 8 bytes are the pointers.
    pub screen: u16,
}

impl Default for SpriteSettings {
    fn default() -> Self {
        SpriteSettings { multicolour: true, overlay: false, expand_x: false, expand_y: false, see_through: SeeThrough::Alpha, palette: Palette::Pepto, dither: Dither::None, strength: 0.6, shared: None, address: 0x2000, screen: 0x0400 }
    }
}

/// Sprites made from a picture, with what's needed to show them.
#[derive(Clone, Debug, PartialEq)]
pub struct Sprites {
    pub set: SpriteSet,
    /// Frames across and down, as cut from the picture.
    pub across: usize,
    pub down: usize,
    pub overlay: bool,
    pub expand_x: bool,
    pub expand_y: bool,
    pub address: u16,
    pub screen: u16,
    pub error: f32,
}

impl Sprites {
    /// The pointer of the first sprite: its place in its 16K VIC bank in 64s.
    pub fn first_pointer(&self) -> u8 {
        ((self.address & 0x3FFF) / 64) as u8
    }

    /// The VIC bank the sprites are in (0 for $0000-$3FFF).
    pub fn bank(&self) -> u8 {
        (self.address >> 14) as u8
    }

    /// One frame as RGBA, 24x21, see-through clear. With an overlay, the pair drawn together.
    pub fn frame_rgba(&self, frame: usize, colours: &[[u8; 3]; 16]) -> Vec<u8> {
        let mut out = vec![0u8; 24 * 21 * 4];
        let per = if self.overlay { 2 } else { 1 };
        for layer in (0..per).rev() {
            let i = frame * per + layer;
            let Some(block) = self.set.sprites.get(i) else { continue };
            let multicolour = block[63] & 0x80 != 0;
            let pixels = sprite_pixels(block, multicolour);
            for y in 0..21 {
                for x in 0..24 {
                    let value = if multicolour { pixels[y * 12 + x / 2] } else { pixels[y * 24 + x] };
                    let c = match (multicolour, value) {
                        (_, 0) => continue,
                        (false, _) | (true, 2) => block[63] & 15,
                        (true, 1) => self.set.multicolour1,
                        _ => self.set.multicolour2,
                    };
                    let rgb = colours[usize::from(c)];
                    out[(y * 24 + x) * 4..(y * 24 + x) * 4 + 4].copy_from_slice(&[rgb[0], rgb[1], rgb[2], 255]);
                }
            }
        }
        out
    }

    /// The frames side by side as they were cut, for showing.
    pub fn sheet_rgba(&self, colours: &[[u8; 3]; 16]) -> (usize, usize, Vec<u8>) {
        let (w, h) = (self.across * 24, self.down * 21);
        let mut out = vec![0u8; w * h * 4];
        for frame in 0..self.across * self.down {
            let rgba = self.frame_rgba(frame, colours);
            let (fx, fy) = (frame % self.across, frame / self.across);
            for y in 0..21 {
                let at = ((fy * 21 + y) * w + fx * 24) * 4;
                out[at..at + 96].copy_from_slice(&rgba[y * 96..y * 96 + 96]);
            }
        }
        (w, h, out)
    }

    /// Source (ACME): the sprites' pointers and registers as constants, a program showing the first ones, and the
    /// sprites.
    pub fn source(&self, name: &str) -> String {
        let label = quantise::identifier(name);
        let mut s = String::new();
        let count = self.set.sprites.len();
        let _ = writeln!(s, "; {name}: {count} Commodore 64 sprites{}, 64 bytes each.", if self.overlay { " (each frame a multicolour sprite and a hires one on it)" } else { "" });
        s.push_str("; Made by Limoniter's C64 converter, for ACME (acme -f cbm -o out.prg this.asm).\n");
        let _ = writeln!(s, "; Byte 63 of each is SpritePad's: the colour, bit 4 a hires sprite on it, bit 7 multicolour.\n");
        let _ = writeln!(s, "{label}_address = ${:04X}       ; in VIC bank {} (${:04X}-${:04X})", self.address, self.bank(), u32::from(self.bank()) * 0x4000, u32::from(self.bank()) * 0x4000 + 0x3FFF);
        let _ = writeln!(s, "{label}_pointer = ${:02X}          ; the first sprite's pointer; the next is one more", self.first_pointer());
        let _ = writeln!(s, "{label}_pointers = ${:04X}      ; where the eight pointers go: the screen RAM + $3F8", self.screen.wrapping_add(0x3F8));
        let _ = writeln!(s, "{label}_count = {count}");
        let _ = writeln!(s, "{label}_multicolour1 = ${:02X}     ; $D025\n{label}_multicolour2 = ${:02X}     ; $D026", self.set.multicolour1, self.set.multicolour2);
        let _ = writeln!(s, "{label}_expand_x = {}           ; $D01D\n{label}_expand_y = {}           ; $D017\n", u8::from(self.expand_x), u8::from(self.expand_y));
        // The viewer shows them from $2000 in bank 0 with the screen at $0400, wherever they're meant to go.
        basic_stub(&mut s);
        let shown = (count.min(8) / if self.overlay { 2 } else { 1 }).max(1);
        let gap = if self.expand_x { 56 } else { 32 };
        s.push_str("        sei\n        lda #0\n        sta $D020\n");
        let _ = writeln!(s, "        lda #${:02X}\n        sta $D021", self.set.background);
        let _ = writeln!(s, "        lda #{label}_multicolour1\n        sta $D025\n        lda #{label}_multicolour2\n        sta $D026");
        let (mut enable, mut multi) = (0u8, 0u8);
        for i in 0..count.min(8) {
            let frame = if self.overlay { i / 2 } else { i };
            if frame >= shown {
                break;
            }
            enable |= 1 << i;
            let block = &self.set.sprites[i];
            if block[63] & 0x80 != 0 {
                multi |= 1 << i;
            }
            let x = 40 + frame * gap;
            let _ = writeln!(s, "        lda #${:02X}\n        sta $07F8+{i}\n        lda #${:02X}\n        sta $D027+{i}\n        lda #${:02X}\n        sta $D000+{}\n        lda #120\n        sta $D001+{}", 0x80 + i, block[63] & 15, x & 0xFF, i * 2, i * 2);
        }
        let high: u8 = (0..count.min(8)).filter(|&i| 40 + (if self.overlay { i / 2 } else { i }) * gap > 255).fold(0, |m, i| m | 1 << i);
        let _ = writeln!(s, "        lda #${high:02X}\n        sta $D010\n        lda #${multi:02X}\n        sta $D01C\n        lda #${:02X}\n        sta $D01D\n        lda #${:02X}\n        sta $D017\n        lda #${enable:02X}\n        sta $D015", if self.expand_x { enable } else { 0 }, if self.expand_y { enable } else { 0 });
        s.push_str("        lda #$93            ; clear the screen\n        jsr $FFD2\nhold:\n        jmp hold\n\n        * = $2000           ; pointer $80 in bank 0\n");
        let data_bytes: Vec<u8> = self.set.sprites.iter().take(128).flat_map(|b| b.iter().copied()).collect();
        data(&mut s, &format!("{label}_sprites"), &data_bytes);
        s
    }

    pub fn viewer(&self, name: &str) -> Result<Vec<u8>, String> {
        gfx::assemble_prg(&self.source(name))
    }

    pub fn everything(&self, name: &str) -> Vec<(String, Vec<u8>)> {
        let mut files = vec![(format!("{name}_sprites.bin"), self.set.data()), (format!("{name}.spd"), self.set.to_spd()), (format!("{name}_sprites.asm"), self.source(name).into_bytes())];
        if let Ok(prg) = self.viewer(name) {
            files.push((format!("{name}_sprites.prg"), prg));
        }
        files
    }
}

/// The picture cut into 24x21 frames, left to right and down, at its own pixels, and each made a sprite.
pub fn convert_sprites(image: &Image, settings: &SpriteSettings) -> Sprites {
    let colours = settings.palette.colours();
    let palette: [[f32; 3]; 16] = std::array::from_fn(|c| oklab(as_f32(colours[c])));
    let across = image.width.div_ceil(24).max(1);
    let down = image.height.div_ceil(21).max(1);
    let frames = (across * down).min(if settings.overlay && settings.multicolour { 128 } else { 256 });
    let key = image.rgba.get(..3).map(|c| [c[0], c[1], c[2]]);
    let pixel = |x: usize, y: usize| -> Option<[f32; 3]> {
        if x >= image.width || y >= image.height {
            return None;
        }
        let p = &image.rgba[(y * image.width + x) * 4..(y * image.width + x) * 4 + 4];
        let clear = match settings.see_through {
            SeeThrough::Alpha => p[3] < 128,
            SeeThrough::TopLeft => Some([p[0], p[1], p[2]]) == key,
            SeeThrough::Nowhere => false,
        };
        (!clear).then(|| [f32::from(p[0]), f32::from(p[1]), f32::from(p[2])])
    };
    // Every opaque pixel, to find the shared colours from.
    let mut all = Vec::new();
    for y in 0..down * 21 {
        for x in 0..across * 24 {
            if let Some(p) = pixel(x, y) {
                all.push(oklab(p));
            }
        }
    }
    let (m1, m2) = settings.shared.unwrap_or_else(|| {
        let order = gfx::backgrounds(&all, &palette, 3);
        (order[0], order.get(1).copied().unwrap_or(1))
    });
    let mix = settings.dither != Dither::None;
    let mut set = SpriteSet { background: 0, multicolour1: m1, multicolour2: m2, sprites: Vec::new() };
    let mut total_error = 0.0f64;
    let mut counted = 0usize;
    for frame in 0..frames {
        let (fx, fy) = ((frame % across) * 24, (frame / across) * 21);
        let full: Vec<Option<[f32; 3]>> = (0..24 * 21).map(|i| pixel(fx + i % 24, fy + i / 24)).collect();
        // The hires sprite lying on the multicolour one: the colour that, set where it's nearest, helps most.
        let mut overlay: Option<(u8, Vec<bool>)> = None;
        if settings.multicolour && settings.overlay {
            let labs: Vec<Option<[f32; 3]>> = full.iter().map(|p| p.map(oklab)).collect();
            let mut best = (f32::MAX, 0u8);
            for o in 0..16u8 {
                let score: f32 = labs.iter().flatten().map(|&p| {
                    let shared = quantise::distance(p, palette[usize::from(m1)]).min(quantise::distance(p, palette[usize::from(m2)]));
                    shared.min(quantise::distance(p, palette[usize::from(o)]))
                }).sum();
                if score < best.0 {
                    best = (score, o);
                }
            }
            let o = best.1;
            let mask: Vec<bool> = labs.iter().map(|p| p.is_some_and(|p| {
                let d = quantise::distance(p, palette[usize::from(o)]);
                d < quantise::distance(p, palette[usize::from(m1)]) && d < quantise::distance(p, palette[usize::from(m2)])
            })).collect();
            overlay = Some((o, mask));
        }
        let covered = |i: usize| overlay.as_ref().is_some_and(|(_, m)| m[i]);
        let (width, pixels): (usize, Vec<Option<[f32; 3]>>) = if settings.multicolour {
            (12, (0..12 * 21).map(|i| {
                let (x, y) = (i % 12, i / 12);
                let pair: Vec<[f32; 3]> = [y * 24 + x * 2, y * 24 + x * 2 + 1].iter().filter(|&&j| !covered(j)).filter_map(|&j| full[j]).collect();
                (!pair.is_empty()).then(|| std::array::from_fn(|c| pair.iter().map(|p| p[c]).sum::<f32>() / pair.len() as f32))
            }).collect())
        } else {
            (24, full.clone())
        };
        let opaque: Vec<[f32; 3]> = pixels.iter().flatten().map(|&p| oklab(p)).collect();
        let fixed: Vec<u8> = if settings.multicolour { vec![m1, m2] } else { Vec::new() };
        let from = candidates(&opaque, &palette, 6);
        let (chosen, _) = if opaque.is_empty() { (vec![1], 0.0) } else { best_set(&opaque, &palette, &fixed, &from, 1, mix) };
        let own = chosen[0];
        let allowed: Vec<Allowed> = vec![if settings.multicolour { vec![(1, m1), (2, own), (3, m2)] } else { vec![(1, own)] }];
        let filled: Vec<[f32; 3]> = pixels.iter().map(|p| p.unwrap_or([0.0; 3])).collect();
        let mut values = assign(&filled, width, 21, &|_, _| 0, &allowed, colours, settings.dither, settings.strength);
        for (v, p) in values.iter_mut().zip(&pixels) {
            if p.is_none() {
                *v = 0;
            }
        }
        set.sprites.push(sprite_block(&values, settings.multicolour, own, overlay.is_some()));
        if let Some((o, mask)) = &overlay {
            let bits: Vec<u8> = mask.iter().map(|&m| u8::from(m)).collect();
            set.sprites.push(sprite_block(&bits, false, *o, false));
        }
        // How near the frame comes, over its opaque pixels.
        let shown = Sprites { set: set.clone(), across: 1, down: 1, overlay: overlay.is_some(), expand_x: false, expand_y: false, address: 0, screen: 0, error: 0.0 };
        let rgba = shown.frame_rgba(set.sprites.len() / if overlay.is_some() { 2 } else { 1 } - 1, colours);
        for (i, p) in full.iter().enumerate() {
            if let Some(p) = p {
                let q = &rgba[i * 4..i * 4 + 3];
                total_error += f64::from(quantise::distance(oklab(*p), oklab([q[0], q[1], q[2]].map(f32::from))).sqrt());
                counted += 1;
            }
        }
    }
    Sprites {
        set,
        across: across.min(frames),
        down: frames.div_ceil(across.min(frames).max(1)),
        overlay: settings.multicolour && settings.overlay,
        expand_x: settings.expand_x,
        expand_y: settings.expand_y,
        address: settings.address,
        screen: settings.screen,
        error: if counted > 0 { (total_error / counted as f64) as f32 } else { 0.0 },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::c64_gfx::PEPTO;

    /// A made-up character ROM for the tests: each character's rows its code and a pattern of it.
    fn fake_rom() -> Vec<u8> {
        (0..4096).map(|i| {
            let (code, row) = ((i / 8) % 256, i % 8);
            ((code as u8).rotate_left(row as u32)) ^ if code >= 128 { 0xFF } else { 0 }
        }).collect()
    }

    fn picture_of(width: usize, height: usize, f: impl Fn(usize, usize) -> [u8; 4]) -> Image {
        Image { width, height, rgba: (0..width * height).flat_map(|i| f(i % width, i / width)).collect() }
    }

    #[test]
    fn a_character_screen_shares_its_characters_and_reads_back() {
        // Stripes in two colours on blue: few different cells, all kept exactly.
        let pixels: Vec<[f32; 3]> = (0..320 * 200).map(|i| {
            let (x, y) = (i % 320, i / 320);
            as_f32(if (x / 3 + y / 5) % 4 == 0 { PEPTO[7] } else { PEPTO[6] })
        }).collect();
        let screen = convert_char_pixels(&pixels, 40, 25, &CharSettings::default(), None).unwrap();
        assert_eq!(screen.backgrounds[0], 6);
        assert!(screen.charset.len() <= 256);
        assert!(screen.error < 1e-4, "{}", screen.error);
        let shown = screen.indices(None);
        for (i, p) in pixels.iter().enumerate() {
            assert_eq!(PEPTO[usize::from(shown[i])], p.map(|v| v as u8));
        }
        // Its viewer has the characters at $2000, the screen copied to $0400.
        let prg = screen.viewer("stripes").unwrap();
        let at = 2 + 0x2000 - 0x0801;
        assert_eq!(&prg[at..at + screen.charset.len() * 8], &screen.charset_bytes()[..]);
    }

    #[test]
    fn too_many_characters_are_made_to_fit() {
        // Noise: every cell different.
        let mut seed = 12345u32;
        let pixels: Vec<[f32; 3]> = (0..320 * 200).map(|_| {
            seed = seed.wrapping_mul(1_103_515_245).wrapping_add(12345);
            as_f32(if seed >> 16 & 1 == 0 { PEPTO[0] } else { PEPTO[1] })
        }).collect();
        for (mode, most) in [(CharMode::Hires, 256), (CharMode::Multicolour, 256), (CharMode::Extended, 64), (CharMode::Hires, 100)] {
            // Multicolour's pixels are half as many across.
            let pixels: Vec<[f32; 3]> = if mode == CharMode::Multicolour { pixels.iter().step_by(2).copied().collect() } else { pixels.clone() };
            let screen = convert_char_pixels(&pixels, 40, 25, &CharSettings { mode, most, ..CharSettings::default() }, None).unwrap();
            assert!(screen.unique > most, "{mode:?}: {}", screen.unique);
            assert_eq!(screen.charset.len(), most, "{mode:?}");
            let limit = if mode == CharMode::Extended { 64 } else { 256 };
            assert!(screen.screen.iter().all(|&c| usize::from(if mode == CharMode::Extended { c & 63 } else { c }) < limit.min(most)));
        }
        let inverses = convert_char_pixels(&pixels, 40, 25, &CharSettings { inverses: true, ..CharSettings::default() }, None).unwrap();
        assert_eq!(inverses.charset.len(), 256);
        for i in 0..128 {
            assert_eq!(inverses.charset[i + 128], inverses.charset[i].map(|b| !b));
        }
    }

    #[test]
    fn petscii_finds_the_rom_characters() {
        let rom = fake_rom();
        let set = rom_set(&rom, false).unwrap();
        // A screen drawn with ROM characters comes back as the same characters.
        let columns = 8;
        let codes: Vec<u8> = (0..columns * 2).map(|i| (i * 37 + 5) as u8).collect();
        let mut pixels = vec![[0.0f32; 3]; columns * 8 * 16];
        for (cell, &code) in codes.iter().enumerate() {
            let (cx, cy) = (cell % columns, cell / columns);
            for row in 0..8 {
                for x in 0..8 {
                    let on = set[usize::from(code)][row] & (0x80 >> x) != 0;
                    pixels[(cy * 8 + row) * columns * 8 + cx * 8 + x] = as_f32(if on { PEPTO[1] } else { PEPTO[6] });
                }
            }
        }
        let screen = convert_char_pixels(&pixels, columns, 2, &CharSettings { mode: CharMode::Petscii, background: Some(6), ..CharSettings::default() }, Some(&rom)).unwrap();
        let shown = screen.indices(Some(&set));
        assert!(screen.error < 1e-4);
        for (i, p) in pixels.iter().enumerate() {
            assert_eq!(PEPTO[usize::from(shown[i])], p.map(|v| v as u8));
        }
        assert!(convert_char_pixels(&pixels, columns, 2, &CharSettings { mode: CharMode::Petscii, ..CharSettings::default() }, None).is_err());
        assert!(screen.viewer("petscii").is_ok());
    }

    #[test]
    fn sprites_go_to_bytes_and_back() {
        let pixels: Vec<u8> = (0..24 * 21).map(|i| u8::from((i * 7) % 5 == 0)).collect();
        let block = sprite_block(&pixels, false, 5, false);
        assert_eq!(sprite_pixels(&block, false), pixels);
        assert_eq!(block[63], 5);
        let multi: Vec<u8> = (0..12 * 21).map(|i| (i % 4) as u8).collect();
        let block = sprite_block(&multi, true, 2, true);
        assert_eq!(sprite_pixels(&block, true), multi);
        assert_eq!(block[0], 0b00_01_10_11);
        assert_eq!(block[63], 0x92);
        // SpritePad's file, both kinds.
        let set = SpriteSet { background: 0, multicolour1: 11, multicolour2: 1, sprites: vec![block, sprite_block(&pixels, false, 7, false)] };
        let spd = set.to_spd();
        assert_eq!(&spd[..9], b"SPD\x01\x01\x00\x00\x0B\x01");
        assert_eq!(spd.len(), 9 + 128 + 4);
        assert_eq!(SpriteSet::from_spd(&spd), Some(set.clone()));
        let mut old = vec![0, 11, 1];
        old.extend(set.data());
        assert_eq!(SpriteSet::from_spd(&old), Some(set));
    }

    #[test]
    fn a_picture_is_cut_into_sprites() {
        // 48x21: two frames, a red square on see-through, and a white and grey one.
        let image = picture_of(48, 21, |x, y| {
            if x < 24 {
                if (4..20).contains(&x) && (4..17).contains(&y) { [0x68, 0x37, 0x2B, 255] } else { [0, 0, 0, 0] }
            } else if x < 36 { [255, 255, 255, 255] } else { [0x6C, 0x6C, 0x6C, 255] }
        });
        let hires = convert_sprites(&image, &SpriteSettings { multicolour: false, ..SpriteSettings::default() });
        assert_eq!((hires.across, hires.down, hires.set.sprites.len()), (2, 1, 2));
        assert_eq!(hires.set.sprites[0][63], 2, "red");
        let first = sprite_pixels(&hires.set.sprites[0], false);
        assert_eq!(first.iter().filter(|&&p| p == 1).count(), 16 * 13);
        let multi = convert_sprites(&image, &SpriteSettings::default());
        assert!(multi.error < 0.02, "{}", multi.error);
        assert_eq!(multi.first_pointer(), 0x80);
        let source = multi.source("ship");
        assert!(source.contains("ship_pointer = $80"), "{source}");
        let prg = multi.viewer("ship").unwrap();
        let at = 2 + 0x2000 - 0x0801;
        assert_eq!(&prg[at..at + 128], &multi.set.data()[..]);
        // With a hires sprite on each: twice as many, the multicolour one marked.
        let overlay = convert_sprites(&image, &SpriteSettings { overlay: true, ..SpriteSettings::default() });
        assert_eq!(overlay.set.sprites.len(), 4);
        assert_eq!(overlay.set.sprites[0][63] & 0x90, 0x90);
        assert!(overlay.error <= multi.error + 1e-6);
    }
}
