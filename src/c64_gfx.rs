//! Commodore 64 bitmap pictures made from any picture: hires (320x200, two colours in each 8x8 cell), multicolour
//! (160x200 with pixels twice as wide, three colours in each cell and a background), and their FLI forms, where the
//! screen RAM changes every line (AFLI for hires, FLI for multicolour). The colours of each cell, or each line of a
//! cell, are the ones that leave the least difference from the picture (in Oklab), and dithering keeps to them.
//!
//! What's made is saved as the painting programs saved it (Koala Painter, OCP Art Studio and Advanced Art Studio,
//! FLI Designer, AFLI), as raw bitmap, screen and colour RAM, as assembler source (ACME) with the data, and as a
//! program that shows it, whose display code is Limoniter's own.

use crate::quantise::{self, Dither, Filter, Fit, Image, as_f32, oklab};
use std::fmt::Write as _;

/// Philip Timmermann's ("Pepto") measurements of a PAL VIC-II, as VICE's pepto-pal.vpl and RECOIL have them.
pub const PEPTO: [[u8; 3]; 16] = [
    [0x00, 0x00, 0x00],
    [0xFF, 0xFF, 0xFF],
    [0x68, 0x37, 0x2B],
    [0x70, 0xA4, 0xB2],
    [0x6F, 0x3D, 0x86],
    [0x58, 0x8D, 0x43],
    [0x35, 0x28, 0x79],
    [0xB8, 0xC7, 0x6F],
    [0x6F, 0x4F, 0x25],
    [0x43, 0x39, 0x00],
    [0x9A, 0x67, 0x59],
    [0x44, 0x44, 0x44],
    [0x6C, 0x6C, 0x6C],
    [0x9A, 0xD2, 0x84],
    [0x6C, 0x5E, 0xB5],
    [0x95, 0x95, 0x95],
];

/// Pepto's later, idealised palette ("Colodore"), as VICE's colodore.vpl has it.
pub const COLODORE: [[u8; 3]; 16] = [
    [0x00, 0x00, 0x00],
    [0xFF, 0xFF, 0xFF],
    [0x96, 0x28, 0x2E],
    [0x5B, 0xD6, 0xCE],
    [0x9F, 0x2D, 0xAD],
    [0x41, 0xB9, 0x36],
    [0x27, 0x24, 0xC4],
    [0xEF, 0xF3, 0x47],
    [0x9F, 0x48, 0x15],
    [0x5E, 0x35, 0x00],
    [0xDA, 0x5F, 0x66],
    [0x47, 0x47, 0x47],
    [0x78, 0x78, 0x78],
    [0x91, 0xFF, 0x84],
    [0x68, 0x64, 0xFF],
    [0xAE, 0xAE, 0xAE],
];

/// The colours VICE 3.8's x64sc shows by default, from its own calculated palette (measured from its screenshots
/// with no CRT filter).
pub const VICE: [[u8; 3]; 16] = [
    [0x00, 0x00, 0x00],
    [0xFF, 0xFF, 0xFF],
    [0xB5, 0x61, 0x48],
    [0x99, 0xE6, 0xF9],
    [0xC1, 0x61, 0xC9],
    [0x79, 0xD5, 0x70],
    [0x60, 0x49, 0xED],
    [0xF7, 0xFF, 0x6C],
    [0xBA, 0x86, 0x20],
    [0x83, 0x70, 0x00],
    [0xE7, 0x9A, 0x84],
    [0x7A, 0x7A, 0x7A],
    [0xA8, 0xA8, 0xA8],
    [0xC0, 0xFF, 0xB9],
    [0xA2, 0x8F, 0xFF],
    [0xD2, 0xD2, 0xD2],
];

/// The colours' names, as the C64's manual gives them.
pub const NAMES: [&str; 16] = ["black", "white", "red", "cyan", "purple", "green", "blue", "yellow", "orange", "brown", "light red", "dark grey", "grey", "light green", "light blue", "light grey"];

/// Which measurements of the C64's colours pictures are matched against.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Palette {
    #[default]
    Pepto,
    Colodore,
    Vice,
}

impl Palette {
    pub const ALL: [Self; 3] = [Self::Pepto, Self::Colodore, Self::Vice];

    pub fn name(self) -> &'static str {
        match self {
            Self::Pepto => "Pepto (PAL, measured)",
            Self::Colodore => "Colodore",
            Self::Vice => "VICE 3.8's own",
        }
    }

    pub fn colours(self) -> &'static [[u8; 3]; 16] {
        match self {
            Self::Pepto => &PEPTO,
            Self::Colodore => &COLODORE,
            Self::Vice => &VICE,
        }
    }
}

/// How wide a hires pixel is for its height on a PAL C64.
pub const PIXEL_ASPECT: f32 = 0.9365;

/// The kind of bitmap made.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    Hires,
    Multicolour,
    /// Hires, with the screen RAM changed every line: two colours in each 8x1.
    Afli,
    /// Multicolour, with the screen RAM changed every line: the background, colour RAM for the cell, and two more
    /// for each 4x1.
    Fli,
}

impl Mode {
    pub const ALL: [Self; 4] = [Self::Multicolour, Self::Hires, Self::Fli, Self::Afli];

    pub fn name(self) -> &'static str {
        match self {
            Self::Hires => "Hires bitmap (320\u{D7}200)",
            Self::Multicolour => "Multicolour bitmap (160\u{D7}200)",
            Self::Afli => "AFLI (hires, colours each line)",
            Self::Fli => "FLI (multicolour, colours each line)",
        }
    }

    /// Whether pixels are twice as wide, two bits each.
    pub fn multicolour(self) -> bool {
        matches!(self, Self::Multicolour | Self::Fli)
    }

    /// Whether the screen RAM changes every line.
    pub fn fli(self) -> bool {
        matches!(self, Self::Afli | Self::Fli)
    }

    /// Pixels across, as the mode counts them.
    pub fn width(self) -> usize {
        if self.multicolour() { 160 } else { 320 }
    }

    pub fn aspect(self) -> f32 {
        if self.multicolour() { PIXEL_ASPECT * 2.0 } else { PIXEL_ASPECT }
    }
}

/// How a picture is turned into a bitmap.
#[derive(Clone, Debug, PartialEq)]
pub struct Settings {
    pub mode: Mode,
    pub palette: Palette,
    pub fit: Fit,
    pub filter: Filter,
    pub dither: Dither,
    pub strength: f32,
    /// The background colour ($D021) for multicolour, or `None` to find the best. FLI's is always black.
    pub background: Option<u8>,
    pub border: u8,
}

impl Default for Settings {
    fn default() -> Self {
        Settings { mode: Mode::Multicolour, palette: Palette::Pepto, fit: Fit::Fill, filter: Filter::Lanczos, dither: Dither::None, strength: 0.6, background: None, border: 0 }
    }
}

/// The columns at the left of an FLI picture that the VIC-II can't give colours ("the FLI bug").
pub const FLI_BUG_COLUMNS: usize = 3;

/// A bitmap picture as the C64 holds it.
#[derive(Clone, Debug, PartialEq)]
pub struct Bitmap {
    pub mode: Mode,
    /// 8000 bytes: 8x8 cells left to right and down, a byte a row.
    pub bitmap: Vec<u8>,
    /// Screen RAM, 1000 bytes: one, or eight for FLI (the one shown on each line of a cell).
    pub screens: Vec<Vec<u8>>,
    /// Colour RAM, 1000 nibbles, for multicolour.
    pub colour: Vec<u8>,
    pub background: u8,
    pub border: u8,
    /// The average difference from the picture, in Oklab.
    pub error: f32,
}

/// Which screen RAM a line of an FLI picture shows. The VIC-II can't fetch new screen RAM past raster line $F7, so
/// the last three lines keep what line 196 fetched.
pub fn fli_screen_of(y: usize) -> usize {
    if y >= 196 { 4 } else { y & 7 }
}

impl Bitmap {
    /// The colour (0 to 15) of each of the 320x200 pixels, as the VIC-II shows them; a multicolour pixel twice.
    pub fn indices(&self) -> Vec<u8> {
        let mut out = vec![0u8; 320 * 200];
        for y in 0..200 {
            let screen = if self.mode.fli() { &self.screens[fli_screen_of(y)] } else { &self.screens[0] };
            for column in 0..40 {
                let cell = (y / 8) * 40 + column;
                let byte = self.bitmap[cell * 8 + (y & 7)];
                // The FLI bug: the first three columns' screen RAM reads as $FF on every line after the first.
                let bugged = self.mode.fli() && column < FLI_BUG_COLUMNS && y > 0;
                let screen_byte = if bugged { 0xFF } else { screen[cell] };
                let colour_byte = if bugged { 0x0F } else { self.colour.get(cell).copied().unwrap_or(0) };
                for x in 0..8 {
                    out[y * 320 + column * 8 + x] = if self.mode.multicolour() {
                        match (byte >> (6 - (x & 6))) & 3 {
                            0 => self.background,
                            1 => screen_byte >> 4,
                            2 => screen_byte & 15,
                            _ => colour_byte & 15,
                        }
                    } else if byte & (0x80 >> x) != 0 {
                        screen_byte >> 4
                    } else {
                        screen_byte & 15
                    };
                }
            }
        }
        out
    }

    /// The picture as RGBA in these colours, 320x200.
    pub fn rgba(&self, colours: &[[u8; 3]; 16]) -> Vec<u8> {
        self.indices().iter().flat_map(|&i| {
            let c = colours[usize::from(i & 15)];
            [c[0], c[1], c[2], 255]
        }).collect()
    }
}

// Choosing colours.

fn distance(a: [f32; 3], b: [f32; 3]) -> f32 {
    quantise::distance(a, b)
}

/// How far a colour is from the line between two others (squared), where dithering between them can reach.
fn segment_distance(p: [f32; 3], a: [f32; 3], b: [f32; 3]) -> (f32, f32) {
    let ab = [b[0] - a[0], b[1] - a[1], b[2] - a[2]];
    let length = ab[0] * ab[0] + ab[1] * ab[1] + ab[2] * ab[2];
    if length < 1e-9 {
        return (distance(p, a), 0.0);
    }
    let t = (((p[0] - a[0]) * ab[0] + (p[1] - a[1]) * ab[1] + (p[2] - a[2]) * ab[2]) / length).clamp(0.0, 1.0);
    (distance(p, [a[0] + ab[0] * t, a[1] + ab[1] * t, a[2] + ab[2] * t]), t)
}

/// What dithering between two colours costs beyond their mix's own difference: mixing far-apart ones looks noisy.
const MIX_PENALTY: f32 = 0.08;

/// What showing these pixels with these colours costs: each pixel's distance from its nearest colour, or, when
/// dithering, from a mix of two of them.
pub(crate) fn cost(pixels: &[[f32; 3]], set: &[[f32; 3]], mix: bool) -> f32 {
    pixels
        .iter()
        .map(|&p| {
            let mut best = set.iter().map(|&c| distance(p, c)).fold(f32::MAX, f32::min);
            if mix {
                for i in 0..set.len() {
                    for j in i + 1..set.len() {
                        best = best.min(segment_distance(p, set[i], set[j]).0 + MIX_PENALTY * distance(set[i], set[j]));
                    }
                }
            }
            best
        })
        .sum()
}

/// The colours worth trying for these pixels: each pixel's nearest two, most used first, at most `most`.
pub(crate) fn candidates(pixels: &[[f32; 3]], labs: &[[f32; 3]; 16], most: usize) -> Vec<u8> {
    let mut weight = [0.0f32; 16];
    for &p in pixels {
        let mut order: [(f32, usize); 16] = std::array::from_fn(|c| (distance(p, labs[c]), c));
        order.sort_by(|a, b| a.0.total_cmp(&b.0));
        weight[order[0].1] += 1.0;
        weight[order[1].1] += 0.4;
    }
    let mut colours: Vec<u8> = (0..16u8).filter(|&c| weight[usize::from(c)] > 0.0).collect();
    colours.sort_by(|a, b| weight[usize::from(*b)].total_cmp(&weight[usize::from(*a)]));
    colours.truncate(most.max(1));
    colours
}

/// The best `count` colours from `from` to go with the fixed ones, and what they cost.
pub(crate) fn best_set(pixels: &[[f32; 3]], labs: &[[f32; 3]; 16], fixed: &[u8], from: &[u8], count: usize, mix: bool) -> (Vec<u8>, f32) {
    let mut best = (vec![fixed.first().copied().unwrap_or(0); count], f32::MAX);
    let mut chosen = Vec::with_capacity(count);
    fn walk(start: usize, from: &[u8], count: usize, chosen: &mut Vec<u8>, f: &mut dyn FnMut(&[u8])) {
        if chosen.len() == count || start >= from.len() {
            f(chosen);
            return;
        }
        for i in start..from.len() {
            chosen.push(from[i]);
            walk(i + 1, from, count, chosen, f);
            chosen.pop();
        }
    }
    walk(0, from, count, &mut chosen, &mut |set: &[u8]| {
        let all: Vec<[f32; 3]> = fixed.iter().chain(set).map(|&c| labs[usize::from(c)]).collect();
        let c = cost(pixels, &all, mix);
        if c < best.1 {
            let mut set = set.to_vec();
            // Fewer to choose from than places: the rest repeat the first.
            while set.len() < count {
                set.push(set.first().or(fixed.first()).copied().unwrap_or(0));
            }
            best = (set, c);
        }
    });
    best
}

/// What each place in the bitmap may be: the bit value and the colour it shows.
pub(crate) type Allowed = Vec<(u8, u8)>;

/// Gives each pixel its bit value from the colours its place allows, dithering if asked.
#[allow(clippy::too_many_arguments)]
pub(crate) fn assign(pixels: &[[f32; 3]], width: usize, height: usize, unit_of: &dyn Fn(usize, usize) -> usize, allowed: &[Allowed], colours: &[[u8; 3]; 16], dither: Dither, strength: f32) -> Vec<u8> {
    let labs: [[f32; 3]; 16] = std::array::from_fn(|c| oklab(as_f32(colours[c])));
    let mut values = vec![0u8; width * height];
    let nearest = |p: [f32; 3], allowed: &Allowed| -> (u8, u8) {
        let lab = oklab(p);
        *allowed.iter().min_by(|a, b| distance(lab, labs[usize::from(a.1)]).total_cmp(&distance(lab, labs[usize::from(b.1)]))).unwrap_or(&(0, 0))
    };
    match dither {
        Dither::None => {
            for y in 0..height {
                for x in 0..width {
                    values[y * width + x] = nearest(pixels[y * width + x], &allowed[unit_of(x, y)]).0;
                }
            }
        }
        Dither::Ordered => {
            for y in 0..height {
                for x in 0..width {
                    let set = &allowed[unit_of(x, y)];
                    let lab = oklab(pixels[y * width + x]);
                    let near = nearest(pixels[y * width + x], set);
                    let mut chosen = near.0;
                    let mut best = distance(lab, labs[usize::from(near.1)]);
                    // The pair whose mix comes nearest, if nearer than either alone: which of the two by the pattern.
                    for i in 0..set.len() {
                        for j in i + 1..set.len() {
                            let (a, b) = (labs[usize::from(set[i].1)], labs[usize::from(set[j].1)]);
                            let (d, t) = segment_distance(lab, a, b);
                            if d + MIX_PENALTY * distance(a, b) * (1.0 - strength) < best {
                                best = d;
                                let threshold = 0.5 + ((quantise::bayer(x, y) + 0.5) / 64.0 - 0.5) * strength;
                                chosen = if t > threshold { set[j].0 } else { set[i].0 };
                            }
                        }
                    }
                    values[y * width + x] = chosen;
                }
            }
        }
        Dither::FloydSteinberg => {
            let mut work = pixels.to_vec();
            for y in 0..height {
                let backwards = y % 2 == 1;
                for i in 0..width {
                    let x = if backwards { width - 1 - i } else { i };
                    let p = work[y * width + x].map(|v| v.clamp(0.0, 255.0));
                    let (value, colour) = nearest(p, &allowed[unit_of(x, y)]);
                    values[y * width + x] = value;
                    let shown = colours[usize::from(colour)];
                    let error: [f32; 3] = std::array::from_fn(|c| (p[c] - f32::from(shown[c])) * strength);
                    quantise::diffuse(&mut work, width, height, x, y, backwards, error);
                }
            }
        }
    }
    values
}

/// The pixels of each place, gathered.
pub(crate) fn gather(labs: &[[f32; 3]], width: usize, height: usize, units: usize, unit_of: &dyn Fn(usize, usize) -> usize) -> Vec<Vec<[f32; 3]>> {
    let mut out = vec![Vec::new(); units];
    for y in 0..height {
        for x in 0..width {
            out[unit_of(x, y)].push(labs[y * width + x]);
        }
    }
    out
}

/// The background most worth trying: the colours nearest most pixels.
pub(crate) fn backgrounds(labs: &[[f32; 3]], palette: &[[f32; 3]; 16], count: usize) -> Vec<u8> {
    let mut used = [0usize; 16];
    for &p in labs {
        used[quantise::nearest(palette, p).0] += 1;
    }
    let mut order: Vec<u8> = (0..16).collect();
    order.sort_by_key(|&c| std::cmp::Reverse(used[usize::from(c)]));
    order.truncate(count);
    order
}

/// The picture made into a bitmap.
pub fn convert(image: &Image, settings: &Settings) -> Bitmap {
    let mode = settings.mode;
    let width = mode.width();
    let pixels = quantise::scale(image, width, 200, mode.aspect(), settings.fit, settings.filter);
    convert_pixels(&pixels, settings)
}

/// Pixels already at the mode's size (160 or 320 by 200, sRGB levels) made into a bitmap.
pub fn convert_pixels(pixels: &[[f32; 3]], settings: &Settings) -> Bitmap {
    let mode = settings.mode;
    let colours = settings.palette.colours();
    let palette: [[f32; 3]; 16] = std::array::from_fn(|c| oklab(as_f32(colours[c])));
    let width = mode.width();
    let labs: Vec<[f32; 3]> = pixels.iter().map(|&p| oklab(p)).collect();
    let mix = settings.dither != Dither::None;
    let per_cell = if mode.multicolour() { 4 } else { 8 };
    let cell_unit = move |x: usize, y: usize| (y / 8) * 40 + x / per_cell;
    // In FLI the place is a line of a cell; the last three lines go with line 196.
    let line_unit = move |x: usize, y: usize| y.min(196) * 40 + x / per_cell;
    let mut screens = vec![vec![0u8; 1000]; if mode.fli() { 8 } else { 1 }];
    let mut colour = vec![0u8; 1000];
    let (allowed, background, unit_of): (Vec<Allowed>, u8, &dyn Fn(usize, usize) -> usize) = match mode {
        Mode::Hires => {
            let units = gather(&labs, width, 200, 1000, &cell_unit);
            let allowed = units
                .iter()
                .enumerate()
                .map(|(cell, px)| {
                    let (set, _) = best_set(px, &palette, &[], &candidates(px, &palette, 7), 2, mix);
                    screens[0][cell] = set[0] << 4 | set[1];
                    vec![(1, set[0]), (0, set[1])]
                })
                .collect();
            (allowed, 0, &cell_unit)
        }
        Mode::Multicolour => {
            let units = gather(&labs, width, 200, 1000, &cell_unit);
            let tries = match settings.background {
                Some(b) => vec![b & 15],
                None => backgrounds(&labs, &palette, 3),
            };
            let cands: Vec<Vec<u8>> = units.iter().map(|px| candidates(px, &palette, 6)).collect();
            let mut best: Option<(f32, u8, Vec<Vec<u8>>)> = None;
            for background in tries {
                let mut total = 0.0;
                let sets: Vec<Vec<u8>> = units
                    .iter()
                    .zip(&cands)
                    .map(|(px, from)| {
                        let from: Vec<u8> = from.iter().copied().filter(|&c| c != background).collect();
                        let (set, c) = best_set(px, &palette, &[background], &from, 3, mix);
                        total += c;
                        set
                    })
                    .collect();
                if best.as_ref().is_none_or(|b| total < b.0) {
                    best = Some((total, background, sets));
                }
            }
            let (_, background, sets) = best.unwrap_or_default();
            let allowed = sets
                .iter()
                .enumerate()
                .map(|(cell, set)| {
                    screens[0][cell] = set[0] << 4 | set[1];
                    colour[cell] = set[2];
                    vec![(0, background), (1, set[0]), (2, set[1]), (3, set[2])]
                })
                .collect();
            (allowed, background, &cell_unit)
        }
        Mode::Afli => {
            let units = gather(&labs, width, 200, 197 * 40, &line_unit);
            let allowed = units
                .iter()
                .enumerate()
                .map(|(unit, px)| {
                    let (line, cell_x) = (unit / 40, unit % 40);
                    if cell_x < FLI_BUG_COLUMNS {
                        return vec![(0, 15)];
                    }
                    let (set, _) = best_set(px, &palette, &[], &candidates(px, &palette, 6), 2, mix);
                    let cell = (line / 8) * 40 + cell_x;
                    for screen in if line == 196 { 4..8 } else { line & 7..(line & 7) + 1 } {
                        screens[screen][cell] = set[0] << 4 | set[1];
                    }
                    vec![(1, set[0]), (0, set[1])]
                })
                .collect();
            (allowed, 0, &line_unit)
        }
        Mode::Fli => {
            let background = 0;
            let units = gather(&labs, width, 200, 197 * 40, &line_unit);
            let mut allowed = vec![vec![(0, background)]; 197 * 40];
            for cell in 0..1000 {
                let (row, cell_x) = (cell / 40, cell % 40);
                if cell_x < FLI_BUG_COLUMNS {
                    continue;
                }
                let lines: Vec<usize> = (row * 8..(row * 8 + 8).min(197)).collect();
                let all: Vec<[f32; 3]> = lines.iter().flat_map(|&l| units[l * 40 + cell_x].iter().copied()).collect();
                let from: Vec<u8> = candidates(&all, &palette, 6).into_iter().filter(|&c| c != background).collect();
                // The colour RAM is the cell's; each line then has its own two.
                let mut best: Option<(f32, u8, Vec<Vec<u8>>)> = None;
                for &shared in from.iter().take(4).chain(std::iter::once(&background)) {
                    let mut total = 0.0;
                    let sets: Vec<Vec<u8>> = lines
                        .iter()
                        .map(|&l| {
                            let px = &units[l * 40 + cell_x];
                            let others: Vec<u8> = candidates(px, &palette, 5).into_iter().filter(|&c| c != background && c != shared).collect();
                            let (set, c) = best_set(px, &palette, &[background, shared], &others, 2, mix);
                            total += c;
                            set
                        })
                        .collect();
                    if best.as_ref().is_none_or(|b| total < b.0) {
                        best = Some((total, shared, sets));
                    }
                }
                let (_, shared, sets) = best.unwrap_or_default();
                colour[cell] = shared;
                for (&line, set) in lines.iter().zip(&sets) {
                    for screen in if line == 196 { 4..8 } else { line & 7..(line & 7) + 1 } {
                        screens[screen][cell] = set[0] << 4 | set[1];
                    }
                    allowed[line * 40 + cell_x] = vec![(0, background), (1, set[0]), (2, set[1]), (3, shared)];
                }
            }
            (allowed, background, &line_unit)
        }
    };
    if mode.fli() {
        // The bug columns can't show their screen RAM: $FF there, so that the first line, which can, matches.
        for screen in &mut screens {
            for row in 0..25 {
                for column in 0..FLI_BUG_COLUMNS {
                    screen[row * 40 + column] = 0xFF;
                }
            }
        }
    }
    let values = assign(pixels, width, 200, unit_of, &allowed, colours, settings.dither, settings.strength);
    let mut bitmap = vec![0u8; 8000];
    for y in 0..200 {
        for x in 0..width {
            let cell = (y / 8) * 40 + x / per_cell;
            let shift = if mode.multicolour() { 2 * (3 - x % 4) } else { 7 - x % 8 };
            bitmap[cell * 8 + (y & 7)] |= values[y * width + x] << shift;
        }
    }
    let mut out = Bitmap { mode, bitmap, screens, colour, background, border: settings.border & 15, error: 0.0 };
    // The difference, at the mode's own pixels.
    let shown = out.rgba(colours);
    let step = 320 / width;
    let sampled: Vec<u8> = (0..200 * width).flat_map(|i| {
        let at = ((i / width) * 320 + (i % width) * step) * 4;
        [shown[at], shown[at + 1], shown[at + 2], 255]
    }).collect();
    out.error = quantise::average_error(pixels, &sampled);
    out
}

// Saving.

impl Bitmap {
    /// Koala Painter (.koa): load address $6000, bitmap, screen RAM, colour RAM, background. Multicolour only.
    pub fn koala(&self) -> Option<Vec<u8>> {
        (self.mode == Mode::Multicolour).then(|| {
            let mut out = vec![0x00, 0x60];
            out.extend(&self.bitmap);
            out.extend(&self.screens[0]);
            out.extend(&self.colour);
            out.push(self.background);
            out
        })
    }

    /// OCP Art Studio (.art): load address $2000, bitmap, screen RAM, border, to 9009 bytes. Hires only.
    pub fn art_studio(&self) -> Option<Vec<u8>> {
        (self.mode == Mode::Hires).then(|| {
            let mut out = vec![0x00, 0x20];
            out.extend(&self.bitmap);
            out.extend(&self.screens[0]);
            out.push(self.border);
            // Art Studio saved six more bytes, unused.
            out.resize(9009, 0);
            out
        })
    }

    /// OCP Advanced Art Studio (.ocp): load address $2000, bitmap, screen RAM, border, background, 14 spare bytes,
    /// colour RAM. Multicolour only.
    pub fn advanced_art_studio(&self) -> Option<Vec<u8>> {
        (self.mode == Mode::Multicolour).then(|| {
            let mut out = vec![0x00, 0x20];
            out.extend(&self.bitmap);
            out.extend(&self.screens[0]);
            out.push(self.border);
            out.push(self.background);
            out.extend([0u8; 14]);
            out.extend(&self.colour);
            out
        })
    }

    /// FLI Designer's .fli: load address $3C00, colour RAM in a 1K block, the eight screens' 1K blocks from $4000,
    /// and the bitmap from $6000, to 17409 bytes. The background is black.
    pub fn fli_file(&self) -> Option<Vec<u8>> {
        (self.mode == Mode::Fli).then(|| {
            let mut out = vec![0x00, 0x3C];
            out.extend(&self.colour);
            out.resize(2 + 1024, 0);
            for screen in &self.screens {
                let at = out.len();
                out.extend(screen);
                out.resize(at + 1024, 0);
            }
            out.extend(&self.bitmap);
            out.resize(17409, 0);
            out
        })
    }

    /// AFLI (.afl): load address $4000, the eight screens' 1K blocks, then the bitmap from $6000, to 16385 bytes.
    pub fn afli_file(&self) -> Option<Vec<u8>> {
        (self.mode == Mode::Afli).then(|| {
            let mut out = vec![0x00, 0x40];
            for screen in &self.screens {
                let at = out.len();
                out.extend(screen);
                out.resize(at + 1024, 0);
            }
            out.extend(&self.bitmap);
            out.resize(16385, 0);
            out
        })
    }

    /// The picture file this mode is usually kept as, with its ending.
    pub fn picture_file(&self) -> (&'static str, Vec<u8>) {
        match self.mode {
            Mode::Multicolour => ("koa", self.koala().unwrap_or_default()),
            Mode::Hires => ("art", self.art_studio().unwrap_or_default()),
            Mode::Fli => ("fli", self.fli_file().unwrap_or_default()),
            Mode::Afli => ("afl", self.afli_file().unwrap_or_default()),
        }
    }

    /// The screen RAM as saved raw: 1000 bytes, or for FLI the eight 1K blocks.
    pub fn screen_bytes(&self) -> Vec<u8> {
        if self.screens.len() == 1 {
            return self.screens[0].clone();
        }
        self.screens.iter().flat_map(|s| {
            let mut s = s.clone();
            s.resize(1024, 0);
            s
        }).collect()
    }

    /// Assembler source (ACME) for a program showing the picture, with its data.
    pub fn viewer_source(&self, name: &str) -> String {
        self.viewer_source_waiting(name, FLI_DELAY)
    }

    fn viewer_source_waiting(&self, name: &str, wait: u32) -> String {
        let label = quantise::identifier(name);
        let mut s = String::new();
        let _ = writeln!(s, "; {name}: a Commodore 64 {} picture and a program to show it.", match self.mode {
            Mode::Hires => "hires bitmap",
            Mode::Multicolour => "multicolour bitmap",
            Mode::Afli => "AFLI",
            Mode::Fli => "FLI",
        });
        s.push_str("; Made by Limoniter's C64 picture converter, for ACME (acme -f cbm -o out.prg this.asm).\n\n");
        basic_stub(&mut s);
        let _ = writeln!(s, "        sei\n        lda #${:02X}            ; border\n        sta $D020\n        lda #${:02X}            ; background\n        sta $D021", self.border, self.background);
        let multicolour = self.mode.multicolour();
        if !self.mode.fli() {
            s.push_str("        ldx #0\ncopy:\n");
            for offset in ["$000", "$100", "$200", "$2E8"] {
                let _ = writeln!(s, "        lda {label}_screen+{offset},x\n        sta $0400+{offset},x");
                if multicolour {
                    let _ = writeln!(s, "        lda {label}_colour+{offset},x\n        sta $D800+{offset},x");
                }
            }
            s.push_str("        inx\n        bne copy\n");
            let _ = writeln!(s, "        lda #$3B            ; bitmap on\n        sta $D011\n        lda #${:02X}            ; {}\n        sta $D016", if multicolour { 0xD8 } else { 0xC8 }, if multicolour { "multicolour" } else { "hires" });
            s.push_str("        lda #$18            ; screen RAM at $0400, bitmap at $2000\n        sta $D018\n        lda $DD00\n        ora #3              ; VIC bank 0\n        sta $DD00\nhold:\n        jmp hold\n\n");
            data(&mut s, &format!("{label}_screen"), &self.screens[0]);
            if multicolour {
                data(&mut s, &format!("{label}_colour"), &self.colour);
            }
            s.push_str("\n        * = $2000\n");
            data(&mut s, &format!("{label}_bitmap"), &self.bitmap);
            return s;
        }
        // FLI: the colour RAM copied, then every frame a raster made stable and the screen RAM changed each line.
        s.push_str("        lda #$7F            ; no interrupts\n        sta $DC0D\n        sta $DD0D\n        lda $DC0D\n        lda $DD0D\n        lda #0\n        sta $D015           ; no sprites\n");
        if multicolour {
            s.push_str("        ldx #0\ncopy:\n");
            for offset in ["$000", "$100", "$200", "$2E8"] {
                let _ = writeln!(s, "        lda {label}_colour+{offset},x\n        sta $D800+{offset},x");
            }
            s.push_str("        inx\n        bne copy\n");
        }
        let _ = writeln!(s, "        lda $DD00\n        and #$FC\n        ora #2              ; VIC bank 1: $4000-$7FFF\n        sta $DD00\n        lda #${:02X}\n        sta $D016\n        jmp frame\n", if multicolour { 0xD8 } else { 0xC8 });
        if multicolour {
            data(&mut s, &format!("{label}_colour"), &self.colour);
        }
        s.push_str("\n        * = $1000           ; timed code: no branch here may cross a page\n");
        fli_frame(&mut s, wait);
        s.push_str("\n        * = $4000           ; the eight screens, one for each line of a cell\n");
        for (i, screen) in self.screens.iter().enumerate() {
            let mut block = screen.clone();
            block.resize(1024, 0);
            data(&mut s, &format!("{label}_screen{i}"), &block);
        }
        s.push_str("\n        * = $6000\n");
        data(&mut s, &format!("{label}_bitmap"), &self.bitmap);
        s
    }

    /// The program that shows the picture: the source above, assembled.
    pub fn viewer(&self, name: &str) -> Result<Vec<u8>, String> {
        assemble_prg(&self.viewer_source(name))
    }

    /// Everything the picture is saved as, by name.
    pub fn everything(&self, name: &str) -> Vec<(String, Vec<u8>)> {
        let mut files = Vec::new();
        let (ending, picture) = self.picture_file();
        files.push((format!("{name}.{ending}"), picture));
        if let Some(ocp) = self.advanced_art_studio() {
            files.push((format!("{name}.ocp"), ocp));
        }
        files.push((format!("{name}_bitmap.bin"), self.bitmap.clone()));
        files.push((format!("{name}_screen.bin"), self.screen_bytes()));
        if self.mode.multicolour() {
            files.push((format!("{name}_colour.bin"), self.colour.clone()));
        }
        files.push((format!("{name}.asm"), self.viewer_source(name).into_bytes()));
        if let Ok(prg) = self.viewer(name) {
            files.push((format!("{name}_viewer.prg"), prg));
        }
        files
    }
}

// Programs.

/// The BASIC line `10 SYS2061` at $0801, so that RUN starts the code after it.
pub fn basic_stub(s: &mut String) {
    s.push_str("        * = $0801\n        !word basic_end, 10\n        !byte $9E\n        !text \"2061\"\n        !byte 0\nbasic_end:\n        !word 0\nstart:                      ; $080D\n");
}

/// Bytes as `!byte` lines, sixteen to a line, under a label.
pub fn data(s: &mut String, label: &str, bytes: &[u8]) {
    let _ = writeln!(s, "{label}:");
    for line in bytes.chunks(16) {
        let items: Vec<String> = line.iter().map(|b| format!("${b:02X}")).collect();
        let _ = writeln!(s, "        !byte {}", items.join(","));
    }
}

/// Source assembled by Limoniter's own 6502 assembler into a .prg.
pub fn assemble_prg(source: &str) -> Result<Vec<u8>, String> {
    let assembly = crate::asm::assemble(&crate::asm::m6502::M6502, std::path::Path::new("viewer.asm"), source, &|_| Err(crate::i18n::tr("no files").into()));
    if let Some(problem) = assembly.problems.first() {
        return Err(crate::i18n::tf!("The viewer didn't assemble: line {}: {}", problem.line + 1, problem.message));
    }
    crate::asm::output::commodore_program(&assembly)
}

/// Code taking exactly this many cycles (at least 2), using X.
pub(crate) fn delay(s: &mut String, mut cycles: u32) {
    assert!(cycles >= 2, "a delay can't be one cycle");
    // A loop of LDX #k, DEX, BNE takes 5k+1 cycles.
    while cycles >= 12 {
        let mut k = ((cycles - 1) / 5).min(255);
        let mut rest = cycles - (5 * k + 1);
        if rest == 1 {
            k -= 1;
            rest += 5;
        }
        let label = format!("delay{}", s.len());
        let _ = writeln!(s, "        ldx #{k}\n{label}:\n        dex\n        bne {label}");
        cycles = rest;
    }
    if cycles % 2 == 1 {
        s.push_str("        bit $EA\n");
        cycles -= 3;
    }
    for _ in 0..cycles / 2 {
        s.push_str("        nop\n");
    }
}

/// Cycles from the stable raster's last step (line $16) to the first line of the picture's halt, found by running
/// the viewer in VICE 3.8 (x64sc) over a range of waits: at 1829 every line shows exactly, and a cycle less breaks it.
pub const FLI_DELAY: u32 = 1829;

/// An FLI frame: a stable raster made by polling, then on every line from $34 the screen RAM changed and a bad line
/// forced, 23 cycles a line, each line's halt for the bad line keeping it in step.
fn fli_frame(s: &mut String, wait: u32) {
    s.push_str("frame:\n        lda #$3B            ; the first bad line at $33, as usual\n        sta $D011\n        lda #$08            ; the first screen, bitmap at $6000\n        sta $D018\nbottom:                     ; past line 255, then into the next frame: line $10 and $110 read alike\n        bit $D011\n        bpl bottom\ntop:\n        bit $D011\n        bmi top\n        ldx #$10\nwait:\n        cpx $D012\n        bne wait\n");
    // Up to 6 cycles late: each of six lines, read a cycle earlier unless the line had already begun.
    delay(s, 52);
    for line in 0x11..=0x16 {
        let _ = writeln!(s, "        lda $D012\n        cmp #${line:02X}\n        bne step{line:02X}\nstep{line:02X}:");
        delay(s, 56);
    }
    delay(s, wait - 56);
    // Line $33 is a bad line of its own; its halt ends at cycle 55, and from there each line takes 23 cycles: 11
    // waiting, then the screen chosen and the next line made a bad line by its write at cycle 14.
    for line in 0x34..=0xF7u32 {
        s.push_str("        nop\n        nop\n        nop\n        nop\n        bit $EA\n");
        let _ = writeln!(s, "        lda #${:02X}\n        sta $D018\n        lda #${:02X}\n        sta $D011", ((line - 0x33) & 7) << 4 | 0x08, 0x38 | (line & 7));
    }
    s.push_str("        jmp frame\n");
}

#[cfg(test)]
mod tests {
    use super::*;

    fn gradient(width: usize, height: usize) -> Image {
        let rgba = (0..width * height)
            .flat_map(|i| {
                let (x, y) = (i % width, i / width);
                [(x * 255 / width) as u8, (y * 255 / height) as u8, ((x + y) * 128 / (width + height)) as u8 + 64, 255]
            })
            .collect();
        Image { width, height, rgba }
    }

    /// What Limoniter's own C64 picture reader makes of a file, as colour numbers in Pepto's palette.
    fn read_back(name: &str, bytes: &[u8]) -> (usize, usize, Vec<u8>) {
        let format = crate::pictures::detect(name, bytes).expect("known");
        let picture = crate::pictures::decode_named(format, name, bytes).unwrap();
        let indices = picture.rgba.chunks(4).map(|p| PEPTO.iter().position(|c| c[..] == p[..3]).expect("a C64 colour") as u8).collect();
        (picture.width, picture.height, indices)
    }

    #[test]
    fn palettes_agree_with_their_sources() {
        // Pepto's is the one the rest of Limoniter shows C64 screens in.
        for (i, c) in crate::retro_screens::C64.iter().enumerate() {
            assert_eq!([c.r(), c.g(), c.b()], PEPTO[i]);
        }
        assert_eq!(COLODORE[6], [0x27, 0x24, 0xC4]);
        assert_eq!(VICE[2], [0xB5, 0x61, 0x48]);
    }

    #[test]
    fn every_mode_reads_back_pixel_for_pixel() {
        let image = gradient(200, 120);
        for mode in Mode::ALL {
            for dither in Dither::ALL {
                let settings = Settings { mode, dither, palette: Palette::Colodore, ..Settings::default() };
                let bitmap = convert(&image, &settings);
                let shown = bitmap.indices();
                let (ending, file) = bitmap.picture_file();
                let (width, height, read) = read_back(&format!("p.{ending}"), &file);
                if mode.fli() {
                    // Readers leave out the bug's columns.
                    assert_eq!((width, height), (296, 200));
                    for y in 0..200 {
                        assert_eq!(read[y * 296..y * 296 + 296], shown[y * 320 + 24..y * 320 + 320], "{mode:?} {dither:?} line {y}");
                    }
                } else {
                    assert_eq!((width, height), (320, 200));
                    assert_eq!(read, shown, "{mode:?} {dither:?}");
                }
                println!("{mode:?} {dither:?}: average difference {:.4}", bitmap.error);
                assert!(bitmap.error < if mode.fli() { 0.15 } else { 0.1 }, "{mode:?} {dither:?}: {}", bitmap.error);
            }
        }
    }

    #[test]
    fn cells_keep_to_their_colours() {
        let image = gradient(160, 100);
        let bitmap = convert(&image, &Settings { mode: Mode::Multicolour, dither: Dither::FloydSteinberg, ..Settings::default() });
        let shown = bitmap.indices();
        for cell in 0..1000 {
            let (cx, cy) = (cell % 40, cell / 40);
            let mut seen: Vec<u8> = (0..64).map(|i| shown[(cy * 8 + i / 8) * 320 + cx * 8 + i % 8]).collect();
            seen.sort_unstable();
            seen.dedup();
            assert!(seen.len() <= 4);
            assert!(seen.iter().all(|&c| c == bitmap.background || c == bitmap.screens[0][cell] >> 4 || c == bitmap.screens[0][cell] & 15 || c == bitmap.colour[cell]));
        }
        // An AFLI line of a cell has two.
        let afli = convert(&image, &Settings { mode: Mode::Afli, ..Settings::default() });
        let shown = afli.indices();
        for y in 0..200 {
            for cx in 3..40 {
                let mut seen: Vec<u8> = shown[y * 320 + cx * 8..y * 320 + cx * 8 + 8].to_vec();
                seen.sort_unstable();
                seen.dedup();
                assert!(seen.len() <= 2);
            }
        }
    }

    #[test]
    fn a_picture_in_c64_colours_comes_through_exactly() {
        // Each 8x8 cell two of the C64's colours: hires shows it with no difference at all.
        let mut pixels = vec![[0.0f32; 3]; 320 * 200];
        for y in 0..200 {
            for x in 0..320 {
                let cell = (y / 8) * 40 + x / 8;
                let c = if (x ^ y) & 1 == 0 { cell % 16 } else { (cell / 16 + 3) % 16 };
                pixels[y * 320 + x] = as_f32(PEPTO[c]);
            }
        }
        let bitmap = convert_pixels(&pixels, &Settings { mode: Mode::Hires, ..Settings::default() });
        assert!(bitmap.error < 1e-4, "{}", bitmap.error);
        let file = bitmap.art_studio().unwrap();
        assert_eq!(file.len(), 9009);
        assert_eq!(bitmap.koala(), None);
    }

    #[test]
    fn files_are_their_formats_sizes() {
        let image = gradient(64, 40);
        let mc = convert(&image, &Settings::default());
        assert_eq!(mc.koala().unwrap().len(), 10003);
        assert_eq!(mc.advanced_art_studio().unwrap().len(), 10018);
        let (_, _, a) = read_back("p.ocp", &mc.advanced_art_studio().unwrap());
        assert_eq!(a, mc.indices(), "Advanced Art Studio reads back too");
        let fli = convert(&image, &Settings { mode: Mode::Fli, ..Settings::default() });
        assert_eq!(fli.fli_file().unwrap().len(), 17409);
        assert_eq!(fli.screen_bytes().len(), 8192);
        let names: Vec<String> = mc.everything("pic").into_iter().map(|f| f.0).collect();
        assert_eq!(names, ["pic.koa", "pic.ocp", "pic_bitmap.bin", "pic_screen.bin", "pic_colour.bin", "pic.asm", "pic_viewer.prg"]);
    }

    #[test]
    fn viewers_assemble_with_the_data_where_it_goes() {
        let image = gradient(64, 40);
        for mode in Mode::ALL {
            let bitmap = convert(&image, &Settings { mode, ..Settings::default() });
            let prg = bitmap.viewer("pic").unwrap();
            assert_eq!(&prg[..2], &[0x01, 0x08]);
            let at = |address: usize| 2 + address - 0x0801;
            let bitmap_at = if mode.fli() { 0x6000 } else { 0x2000 };
            assert_eq!(&prg[at(bitmap_at)..at(bitmap_at) + 8000], &bitmap.bitmap[..], "{mode:?}");
            if mode.fli() {
                for (i, screen) in bitmap.screens.iter().enumerate() {
                    assert_eq!(&prg[at(0x4000 + i * 1024)..at(0x4000 + i * 1024) + 1000], &screen[..]);
                }
                // The timed code's branches stay in one page, or they'd take a cycle more: the waits, the stable
                // raster's steps and its delay loops.
                let assembly = crate::asm::assemble(&crate::asm::m6502::M6502, std::path::Path::new("v.asm"), &bitmap.viewer_source("pic"), &|_| Err(crate::i18n::tr("no").into()));
                let frame = assembly.symbols["frame"];
                assert_eq!(frame, 0x1000);
                assert!(assembly.symbols.iter().filter(|(k, _)| k.starts_with("delay") || k.starts_with("step") || ["bottom", "top", "wait"].contains(&k.as_str())).all(|(_, &v)| v >> 8 == frame >> 8), "{:?}", assembly.symbols);
            }
        }
    }

    #[test]
    fn delays_take_what_they_say() {
        for cycles in 2..1500 {
            let mut s = String::new();
            delay(&mut s, cycles);
            let mut total = 0;
            let mut k = 0;
            for line in s.lines() {
                let line = line.trim();
                if let Some(n) = line.strip_prefix("ldx #") {
                    k = n.parse::<u32>().unwrap();
                    total += 2;
                } else if line == "dex" {
                    total += 5 * k - 1;
                } else if line == "bit $EA" {
                    total += 3;
                } else if line == "nop" {
                    total += 2;
                }
            }
            assert_eq!(total, cycles);
        }
    }

    /// Writes every mode's files for a picture, to be checked against RECOIL and VICE: set LIMONITER_C64_OUT to a
    /// folder holding source.png (and sprites.png). Each picture's own idea of itself goes beside it as a PNG.
    #[test]
    #[ignore]
    fn files_for_checking_elsewhere() {
        let Some(folder) = std::env::var_os("LIMONITER_C64_OUT").map(std::path::PathBuf::from) else { return };
        let image = quantise::read_picture("source.png", &std::fs::read(folder.join("source.png")).unwrap()).unwrap();
        for mode in Mode::ALL {
            for dither in Dither::ALL {
                let bitmap = convert(&image, &Settings { mode, dither, border: 6, ..Settings::default() });
                let name = format!("{mode:?}_{dither:?}").to_lowercase();
                for (file, bytes) in bitmap.everything(&name) {
                    std::fs::write(folder.join(file), bytes).unwrap();
                }
                let picture = crate::pictures::Picture { width: 320, height: 200, rgba: bitmap.rgba(&PEPTO), aspect: 1.0, info: Vec::new(), cycling: None };
                std::fs::write(folder.join(format!("{name}_expected.png")), crate::pictures::to_png(&picture).unwrap()).unwrap();
                println!("{name}: {:.4}", bitmap.error);
                // The FLI viewers waiting a little more and less, to find the wait that shows it exactly.
                if mode.fli() && dither == Dither::None {
                    for wait in FLI_DELAY - 3..=FLI_DELAY + 3 {
                        std::fs::write(folder.join(format!("{name}_wait{wait}.prg")), assemble_prg(&bitmap.viewer_source_waiting(&name, wait)).unwrap()).unwrap();
                    }
                }
            }
        }
        let chars = crate::c64_chars::convert_chars(&image, &crate::c64_chars::CharSettings { mode: crate::c64_chars::CharMode::Multicolour, ..Default::default() }, None).unwrap();
        for (file, bytes) in chars.everything("chars_mc") {
            std::fs::write(folder.join(file), bytes).unwrap();
        }
        let indices = chars.indices(None);
        let rgba: Vec<u8> = indices.iter().flat_map(|&i| { let c = PEPTO[usize::from(i)]; [c[0], c[1], c[2], 255] }).collect();
        std::fs::write(folder.join("chars_mc_expected.png"), crate::pictures::to_png(&crate::pictures::Picture { width: 320, height: 200, rgba, aspect: 1.0, info: Vec::new(), cycling: None }).unwrap()).unwrap();
        if let Ok(rom) = std::fs::read("/root/emu/vice-src/vice-3.8/data/C64/chargen-901225-01.bin") {
            let petscii = crate::c64_chars::convert_chars(&image, &crate::c64_chars::CharSettings { mode: crate::c64_chars::CharMode::Petscii, ..Default::default() }, Some(&rom)).unwrap();
            for (file, bytes) in petscii.everything("petscii") {
                std::fs::write(folder.join(file), bytes).unwrap();
            }
            let set = crate::c64_chars::rom_set(&rom, false).unwrap();
            let rgba: Vec<u8> = petscii.indices(Some(&set)).iter().flat_map(|&i| { let c = PEPTO[usize::from(i)]; [c[0], c[1], c[2], 255] }).collect();
            std::fs::write(folder.join("petscii_expected.png"), crate::pictures::to_png(&crate::pictures::Picture { width: 320, height: 200, rgba, aspect: 1.0, info: Vec::new(), cycling: None }).unwrap()).unwrap();
            println!("petscii: {:.4}", petscii.error);
        }
        if let Ok(bytes) = std::fs::read(folder.join("sprites.png")) {
            let sheet = quantise::read_picture("sprites.png", &bytes).unwrap();
            for (tag, overlay) in [("spr", false), ("spr_overlay", true)] {
                let sprites = crate::c64_chars::convert_sprites(&sheet, &crate::c64_chars::SpriteSettings { overlay, ..Default::default() });
                for (file, bytes) in sprites.everything(tag) {
                    std::fs::write(folder.join(file), bytes).unwrap();
                }
                println!("{tag}: {:.4}", sprites.error);
                for frame in 0..sprites.across * sprites.down {
                    let picture = crate::pictures::Picture { width: 24, height: 21, rgba: sprites.frame_rgba(frame, &PEPTO), aspect: 1.0, info: Vec::new(), cycling: None };
                    std::fs::write(folder.join(format!("{tag}_frame{frame}.png")), crate::pictures::to_png(&picture).unwrap()).unwrap();
                }
            }
        }
    }
}
