//! Hardware sprites and blitter objects (bobs) from a picture, for the Amiga bitplane converter.
//!
//! A sprite is 16 pixels wide (32 or 64 with AGA's wider fetch) and as tall as wanted: two control words (SPRxPOS
//! and SPRxCTL, its place on the screen), then each row as two words (DATA, the low bit of each pixel's colour, and
//! DATB, the high bit), then two words of 0 to end it. Its three colours are shared with the sprite beside it:
//! sprites 0 and 1 use COLOR17 to 19, 2 and 3 COLOR21 to 23, and so on; 0 is see-through. Two sprites attached (the
//! odd one with SPRxCTL's ATTACH bit) make one of 15 colours, COLOR17 to 31. A picture wider than a sprite is cut
//! into columns, a sprite (or an attached pair) each.
//!
//! A bob is bit planes the blitter copies onto the screen, with a mask plane to cut its shape out of what's there.
//! Its rows are a whole number of words with an extra word on the right, room for the blitter's shift to move it to
//! any pixel.

use crate::amiga_planar::{self as planar, Chipset, Dither, Fetch, Image};
use std::fmt::Write as _;

/// Which pixels are see-through.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Transparency {
    /// Those the picture says are (alpha under a half).
    Alpha,
    /// Those of this colour (near enough).
    Colour([u8; 3]),
    /// None.
    Opaque,
}

/// A picture cut to size: its colours, and which pixels show.
#[derive(Clone, Debug, PartialEq)]
pub struct Cut {
    pub width: usize,
    pub height: usize,
    pub pixels: Vec<[f32; 3]>,
    pub opaque: Vec<bool>,
}

/// The picture scaled to `width` by `height` (stretched), and which pixels show: each the nearest source pixel's.
pub fn cut(image: &Image, width: usize, height: usize, filter: planar::Filter, transparency: Transparency) -> Cut {
    let (width, height) = (width.clamp(1, 1024), height.clamp(1, 1024));
    let settings = planar::Settings { width, height, fit: planar::Fit::Stretch, filter, ..planar::Settings::default() };
    let mut pixels = planar::scale(image, &settings);
    let mut opaque = vec![true; width * height];
    if image.width > 0 && image.height > 0 {
        for y in 0..height {
            for x in 0..width {
                let (sx, sy) = (x * image.width / width, y * image.height / height);
                let p = &image.rgba[(sy * image.width + sx) * 4..][..4];
                opaque[y * width + x] = match transparency {
                    Transparency::Alpha => p[3] >= 128,
                    Transparency::Colour(c) => (0..3).any(|i| p[i].abs_diff(c[i]) > 12),
                    Transparency::Opaque => true,
                };
            }
        }
    }
    // See-through pixels' colours don't pull the palette or the dithering about.
    let shown: Vec<[f32; 3]> = pixels.iter().zip(&opaque).filter(|(_, o)| **o).map(|(p, _)| *p).collect();
    let fill = shown.first().copied().unwrap_or([0.0; 3]);
    for (p, o) in pixels.iter_mut().zip(&opaque) {
        if !*o {
            *p = fill;
        }
    }
    Cut { width, height, pixels, opaque }
}

/// Colours for the shown pixels: `count` of them found from those pixels (or the given ones kept), each shown pixel
/// given one from 1 up and the see-through ones 0. The palette returned has the unused colour 0 in front.
fn colour_in(cut: &Cut, which: impl Fn(usize) -> bool, count: usize, given: &[[u8; 3]], chipset: Chipset, dither: Dither, strength: f32) -> (Vec<[u8; 3]>, Vec<u8>) {
    let shown: Vec<[f32; 3]> = (0..cut.pixels.len()).filter(|&i| which(i) && cut.opaque[i]).map(|i| cut.pixels[i]).collect();
    let slots: Vec<Option<[u8; 3]>> = (0..count).map(|i| given.get(i).copied()).collect();
    let colours = planar::find_palette(&shown, &slots, chipset, false);
    let values = planar::choose(&cut.pixels, cut.width, cut.height, &colours, chipset, false, dither, strength);
    let values = values.iter().enumerate().map(|(i, &v)| if which(i) && cut.opaque[i] { v + 1 } else { 0 }).collect();
    let mut palette = vec![[0, 0, 0]];
    palette.extend(colours);
    (palette, values)
}

// Sprites.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SpriteWidth {
    W16,
    /// AGA, FMODE's sprite bits 1.
    W32,
    /// AGA, FMODE's sprite bits 3.
    W64,
}

impl SpriteWidth {
    pub const ALL: [Self; 3] = [Self::W16, Self::W32, Self::W64];

    pub fn pixels(self) -> usize {
        match self {
            Self::W16 => 16,
            Self::W32 => 32,
            Self::W64 => 64,
        }
    }

    /// Words in each of DATA and DATB a row (and in each control word, padded).
    fn words(self) -> usize {
        self.pixels() / 16
    }

    pub fn name(self) -> &'static str {
        match self {
            Self::W16 => "16 pixels (any chipset)",
            Self::W32 => "32 pixels (AGA, FMODE $4)",
            Self::W64 => "64 pixels (AGA, FMODE $C)",
        }
    }
}

/// What sprites are made.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SpriteSettings {
    /// Pairs attached for 15 colours, rather than 3 a sprite.
    pub attached: bool,
    pub width: SpriteWidth,
    /// Where the top left goes: the first line (VSTART) and lo-res pixel (HSTART) of the screen, as the hardware counts.
    pub vstart: u16,
    pub hstart: u16,
}

impl Default for SpriteSettings {
    fn default() -> Self {
        SpriteSettings { attached: false, width: SpriteWidth::W16, vstart: 0x2C, hstart: 0x81 }
    }
}

/// Sprites made from a picture.
#[derive(Clone, Debug, PartialEq)]
pub struct Sprites {
    pub width: usize,
    pub height: usize,
    pub settings: SpriteSettings,
    pub chipset: Chipset,
    /// COLOR16 to COLOR31.
    pub palette: Vec<[u8; 3]>,
    /// Each pixel's colour, 0 see-through, 1 to 3 (or 15 attached), in the column's own colours.
    pub values: Vec<u8>,
    /// Each hardware sprite's words, control words to the end, in order of sprite number.
    pub sprites: Vec<Vec<u8>>,
    pub rgba: Vec<u8>,
}

impl Sprites {
    /// Columns across: a sprite each, or an attached pair each.
    pub fn columns(&self) -> usize {
        self.width.div_ceil(self.settings.width.pixels())
    }

    /// How many of the eight hardware sprites it takes.
    pub fn needed(&self) -> usize {
        self.sprites.len()
    }

    /// The colour registers it uses, by number.
    pub fn registers(&self) -> Vec<(usize, [u8; 3])> {
        let used: Vec<usize> = if self.settings.attached { (1..16).collect() } else { (0..4.min(self.columns().div_ceil(2))).flat_map(|g| (1..4).map(move |i| g * 4 + i)).collect() };
        used.into_iter().map(|i| (16 + i, self.palette[i])).collect()
    }
}

/// SPRxPOS and SPRxCTL for a sprite at this line and lo-res pixel, this tall.
pub fn control_words(vstart: u16, hstart: u16, height: usize, attach: bool) -> (u16, u16) {
    let vstop = vstart.wrapping_add(height as u16);
    let pos = (vstart & 0xFF) << 8 | (hstart >> 1) & 0xFF;
    let ctl = (vstop & 0xFF) << 8 | u16::from(attach) << 7 | (vstart >> 8 & 1) << 2 | (vstop >> 8 & 1) << 1 | hstart & 1;
    (pos, ctl)
}

/// Sprites from a picture cut to size: the colours found for each pair of columns (or for all, attached).
pub fn make_sprites(cut: &Cut, settings: SpriteSettings, chipset: Chipset, dither: Dither, strength: f32) -> Sprites {
    let wide = settings.width.pixels();
    let columns = cut.width.div_ceil(wide);
    let column_of = |i: usize| (i % cut.width) / wide;
    let mut palette = vec![[0u8, 0, 0]; 16];
    let mut values = vec![0u8; cut.width * cut.height];
    if settings.attached {
        let (colours, found) = colour_in(cut, |_| true, 15, &[], chipset, dither, strength);
        palette.copy_from_slice(&colours);
        values = found;
    } else {
        // Sprites 0 and 1 share colours, 2 and 3, and so on; past eight sprites the colours come round again.
        for group in 0..4.min(columns.div_ceil(2)) {
            let (colours, found) = colour_in(cut, |i| (column_of(i) / 2) % 4 == group, 3, &[], chipset, dither, strength);
            palette[group * 4 + 1..group * 4 + 4].copy_from_slice(&colours[1..]);
            for (v, f) in values.iter_mut().zip(found) {
                if f != 0 {
                    *v = f;
                }
            }
        }
    }
    let words = settings.width.words();
    let mut sprites = Vec::new();
    for column in 0..columns {
        let hstart = settings.hstart.wrapping_add((column * wide) as u16);
        // Attached, the even sprite has the low two bits and the odd one the high two.
        let halves: &[(u8, bool)] = if settings.attached { &[(0, false), (2, true)] } else { &[(0, false)] };
        for &(shift, attach) in halves {
            let (pos, ctl) = control_words(settings.vstart, hstart, cut.height, attach);
            let mut out = Vec::new();
            let push = |out: &mut Vec<u8>, w: u16| out.extend_from_slice(&w.to_be_bytes());
            push(&mut out, pos);
            (1..words).for_each(|_| push(&mut out, 0));
            push(&mut out, ctl);
            (1..words).for_each(|_| push(&mut out, 0));
            for y in 0..cut.height {
                for bit in [shift, shift + 1] {
                    for word in 0..words {
                        let mut w = 0u16;
                        for b in 0..16 {
                            let x = column * wide + word * 16 + b;
                            if x < cut.width && values[y * cut.width + x] >> bit & 1 != 0 {
                                w |= 0x8000 >> b;
                            }
                        }
                        push(&mut out, w);
                    }
                }
            }
            (0..2 * words).for_each(|_| push(&mut out, 0));
            sprites.push(out);
        }
    }
    let mut rgba = Vec::with_capacity(values.len() * 4);
    for (i, &v) in values.iter().enumerate() {
        let colour = if settings.attached { palette[usize::from(v)] } else { palette[(column_of(i) / 2 % 4) * 4 + usize::from(v)] };
        rgba.extend_from_slice(&[colour[0], colour[1], colour[2], if v == 0 { 0 } else { 255 }]);
    }
    Sprites { width: cut.width, height: cut.height, settings, chipset, palette, values, sprites, rgba }
}

/// A sprite's words read back: its place, height, whether attached, and each pixel's two bits.
#[cfg(test)]
pub fn read_sprite(words: &[u8], width: SpriteWidth) -> Option<(u16, u16, usize, bool, Vec<u8>)> {
    let n = width.words();
    let word = |i: usize| words.get(i * 2..i * 2 + 2).map(|b| u16::from_be_bytes([b[0], b[1]]));
    let (pos, ctl) = (word(0)?, word(n)?);
    let vstart = (pos >> 8) | (ctl >> 2 & 1) << 8;
    let vstop = (ctl >> 8) | (ctl >> 1 & 1) << 8;
    let hstart = (pos & 0xFF) << 1 | ctl & 1;
    let height = usize::from(vstop.wrapping_sub(vstart) & 0x1FF);
    let pixels = width.pixels();
    let mut values = vec![0u8; pixels * height];
    for y in 0..height {
        for bit in 0..2 {
            for w in 0..n {
                let value = word(2 * n + (y * 2 + bit) * n + w)?;
                for b in 0..16 {
                    if value & (0x8000 >> b) != 0 {
                        values[y * pixels + w * 16 + b] |= 1 << bit;
                    }
                }
            }
        }
    }
    let end = 2 * n + height * 2 * n;
    if (end..end + 2 * n).any(|i| word(i) != Some(0)) {
        return None;
    }
    Some((vstart, hstart, height, ctl & 0x80 != 0, values))
}

/// Words as `dc.w` lines, eight a line.
pub fn dc_words(bytes: &[u8]) -> String {
    let mut out = String::new();
    for line in bytes.chunks(16) {
        let words: Vec<String> = line.chunks(2).map(|w| format!("${:02x}{:02x}", w[0], w.get(1).copied().unwrap_or(0))).collect();
        let _ = writeln!(out, "        dc.w    {}", words.join(","));
    }
    out
}

/// The sprites as raw data: each sprite's words, one after another.
pub fn sprites_raw(s: &Sprites) -> Vec<u8> {
    s.sprites.concat()
}

/// Source for the sprites: their words (as `dc.w`, or brought in from the raw file), a table of where each starts,
/// and the copper moves for their colours.
pub fn sprites_source(s: &Sprites, name: &str, raw_file: Option<&str>) -> String {
    let id = planar::identifier(name);
    let mut out = String::new();
    let wide = s.settings.width.pixels();
    let _ = writeln!(out, "; {} by {}: {} sprite{} {} pixels wide{}, at line ${:x}, lo-res pixel ${:x}.", s.width, s.height, s.needed(), if s.needed() == 1 { "" } else { "s" }, wide, if s.settings.attached { ", attached in pairs for 15 colours" } else { "" }, s.settings.vstart, s.settings.hstart);
    if s.needed() > 8 {
        let _ = writeln!(out, "; That's more than the eight sprites there are: the rest would have to be reused further down the screen.");
    }
    if s.settings.width != SpriteWidth::W16 {
        let _ = writeln!(out, "; AGA: FMODE's sprite bits must be set for {wide}-pixel sprites: dc.w $01fc,${:04x}", if wide == 32 { 0x4 } else { 0xC });
    }
    let _ = writeln!(out, "{id}_HEIGHT  equ {}", s.height);
    let _ = writeln!(out, "{id}_SPRITES equ {}", s.needed());
    let _ = writeln!(out);
    let _ = writeln!(out, "; Where each sprite starts, for SPR0PT onwards.");
    let _ = writeln!(out, "{id}_table:");
    for n in 0..s.needed() {
        let _ = writeln!(out, "        dc.l    {id}_spr{n}");
    }
    let _ = writeln!(out);
    let _ = writeln!(out, "; The sprites' colours (COLOR16 to 31), for the copper list.");
    let _ = writeln!(out, "{id}_colours:");
    out.push_str(&planar::copper_moves(&s.registers(), s.chipset));
    let _ = writeln!(out);
    let _ = writeln!(out, "        section {id}_data,data_c");
    let align = if wide == 64 { 8 } else { 4 };
    let mut offset = 0;
    for (n, words) in s.sprites.iter().enumerate() {
        let _ = writeln!(out, "        cnop    0,{align}");
        match raw_file {
            Some(file) => {
                let _ = writeln!(out, "{id}_spr{n}:\n        incbin  \"{file}\",{offset},{}    ; sprite {n}", words.len());
            }
            None => {
                let _ = writeln!(out, "{id}_spr{n}:    ; sprite {n}{}", if s.settings.attached && n % 2 == 1 { ", attached" } else { "" });
                out.push_str(&dc_words(words));
            }
        }
        offset += words.len();
    }
    out
}

// Bobs.

/// What a bob is made as.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BobSettings {
    pub planes: usize,
    pub interleaved: bool,
    /// A word on the right of each row for the blitter's shift.
    pub extra_word: bool,
}

impl Default for BobSettings {
    fn default() -> Self {
        BobSettings { planes: 4, interleaved: true, extra_word: true }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Bob {
    pub width: usize,
    pub height: usize,
    pub settings: BobSettings,
    pub chipset: Chipset,
    pub palette: Vec<[u8; 3]>,
    /// 0 where it's see-through.
    pub values: Vec<u8>,
    pub opaque: Vec<bool>,
    pub rgba: Vec<u8>,
}

impl Bob {
    /// Words in a row of one plane.
    pub fn words(&self) -> usize {
        self.width.div_ceil(16) + usize::from(self.settings.extra_word)
    }

    /// The rows of data the blitter copies in one go: every plane's at once, interleaved.
    pub fn blit_rows(&self) -> usize {
        if self.settings.interleaved { self.height * self.settings.planes } else { self.height }
    }

    /// BLTSIZE for one blit (all the planes interleaved, otherwise one plane): rows times 64, plus words.
    pub fn bltsize(&self) -> Option<u16> {
        let (rows, words) = (self.blit_rows(), self.words());
        (rows <= 1024 && words <= 64).then_some(((rows % 1024) << 6 | (words % 64)) as u16)
    }

    /// The planes, rows padded to the words; the extra word is 0.
    pub fn planes(&self) -> Vec<u8> {
        let padded = self.words() * 16;
        let values = pad(&self.values, self.width, padded);
        planar::to_planes(&values, padded, self.height, self.settings.planes, self.settings.interleaved, Fetch::Words)
    }

    /// The mask: a bit set where the bob shows. Interleaved, each row is there once a plane, so that one blit
    /// cuts every plane.
    pub fn mask(&self) -> Vec<u8> {
        let padded = self.words() * 16;
        let shows: Vec<u8> = self.opaque.iter().map(|&o| u8::from(o)).collect();
        let single = planar::to_planes(&pad(&shows, self.width, padded), padded, self.height, 1, false, Fetch::Words);
        if !self.settings.interleaved {
            return single;
        }
        single.chunks(self.words() * 2).flat_map(|row| row.repeat(self.settings.planes)).collect()
    }
}

/// Rows widened with 0s on the right.
fn pad(values: &[u8], width: usize, padded: usize) -> Vec<u8> {
    values.chunks(width).flat_map(|row| row.iter().copied().chain(std::iter::repeat_n(0, padded - width))).collect()
}

/// A bob from a picture cut to size: its colours found from it (colour 0 left for what's behind), or the given
/// palette's kept.
pub fn make_bob(cut: &Cut, settings: BobSettings, given: &[[u8; 3]], chipset: Chipset, dither: Dither, strength: f32) -> Bob {
    let planes = settings.planes.clamp(1, 8);
    let settings = BobSettings { planes, ..settings };
    let count = (1usize << planes) - 1;
    let kept: Vec<[u8; 3]> = given.iter().skip(1).copied().collect();
    let (palette, values) = colour_in(cut, |_| true, count, &kept, chipset, dither, strength);
    let palette = match given.first() {
        Some(&background) => std::iter::once(background).chain(palette.into_iter().skip(1)).collect(),
        None => palette,
    };
    let rgba = values.iter().map(|&v| palette[usize::from(v)]).zip(&cut.opaque).flat_map(|(c, &o)| [c[0], c[1], c[2], if o { 255 } else { 0 }]).collect();
    Bob { width: cut.width, height: cut.height, settings, chipset, palette, values, opaque: cut.opaque.clone(), rgba }
}

/// Source for a bob: its sizes, BLTSIZE, and its planes and mask (as `dc.w`, or brought in from raw files).
pub fn bob_source(b: &Bob, name: &str, files: Option<(&str, &str)>) -> String {
    let id = planar::identifier(name);
    let mut out = String::new();
    let row = b.words() * 2;
    let _ = writeln!(out, "; {} by {}, {} bit plane{}{}, {} words a row{}.", b.width, b.height, b.settings.planes, if b.settings.planes == 1 { "" } else { "s" }, if b.settings.interleaved { ", interleaved" } else { ", one plane after another" }, b.words(), if b.settings.extra_word { " (one of them spare, for shifting)" } else { "" });
    let _ = writeln!(out, "{id}_WIDTH     equ {}", b.width);
    let _ = writeln!(out, "{id}_HEIGHT    equ {}", b.height);
    let _ = writeln!(out, "{id}_PLANES    equ {}", b.settings.planes);
    let _ = writeln!(out, "{id}_WORDS     equ {}    ; words a row of one plane", b.words());
    let _ = writeln!(out, "{id}_ROWBYTES  equ {row}");
    let _ = writeln!(out, "{id}_PLANESTEP equ {}    ; from one plane's start to the next's", if b.settings.interleaved { row } else { row * b.height });
    match b.bltsize() {
        Some(size) => {
            let _ = writeln!(out, "{id}_BLTSIZE   equ ${size:04x}    ; {} rows by {} words{}", b.blit_rows(), b.words(), if b.settings.interleaved { ", every plane in one blit" } else { ", one plane a blit" });
        }
        None => {
            let _ = writeln!(out, "; {} rows by {} words is more than BLTSIZE holds (1024 by 64): use ECS's BLTSIZV and BLTSIZH.", b.blit_rows(), b.words());
        }
    }
    let _ = writeln!(out, "; On a screen whose planes' rows are SCREENROW bytes, the destination's modulo is SCREENROW-{row}{}.", if b.settings.interleaved { " for a screen interleaved too, with SCREENROW the bytes of all its planes' rows" } else { "" });
    let _ = writeln!(out);
    let _ = writeln!(out, "        section {id}_data,data_c");
    let (planes, mask) = (b.planes(), b.mask());
    match files {
        Some((planes_file, mask_file)) => {
            let _ = writeln!(out, "{id}_planes:\n        incbin  \"{planes_file}\"    ; {} bytes", planes.len());
            let _ = writeln!(out, "{id}_mask:\n        incbin  \"{mask_file}\"    ; {} bytes", mask.len());
        }
        None => {
            let _ = writeln!(out, "{id}_planes:");
            out.push_str(&dc_words(&planes));
            let _ = writeln!(out, "{id}_mask:");
            out.push_str(&dc_words(&mask));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A ball: a shaded disc on see-through, 40 by 30.
    fn ball(width: usize, height: usize) -> Image {
        let mut rgba = Vec::new();
        for y in 0..height {
            for x in 0..width {
                let (dx, dy) = (x as f32 / width as f32 - 0.5, y as f32 / height as f32 - 0.5);
                let inside = dx * dx + dy * dy < 0.2;
                let light = (255.0 * (1.0 - (dx + 0.2).hypot(dy + 0.2))).clamp(0.0, 255.0) as u8;
                rgba.extend_from_slice(&[light, light / 2, (x * 6) as u8, if inside { 255 } else { 0 }]);
            }
        }
        Image { width, height, rgba }
    }

    #[test]
    fn control_words_are_as_the_hardware_reads_them() {
        // The usual top left of a PAL screen.
        assert_eq!(control_words(0x2C, 0x80, 16, false), (0x2C40, 0x3C00));
        assert_eq!(control_words(0x2C, 0x81, 16, false), (0x2C40, 0x3C01));
        // Past line 255 the ninth bits go in SPRxCTL; ATTACH is bit 7.
        assert_eq!(control_words(0xF8, 0x1FF, 16, true), (0xF8FF, 0x0883));
    }

    #[test]
    fn sprites_go_out_and_come_back() {
        let image = ball(40, 30);
        for attached in [false, true] {
            for width in SpriteWidth::ALL {
                let c = cut(&image, 40, 30, planar::Filter::Box, Transparency::Alpha);
                let settings = SpriteSettings { attached, width, vstart: 0x40, hstart: 0x90 };
                let s = make_sprites(&c, settings, Chipset::Ocs, Dither::FloydSteinberg, 0.8);
                let wide = width.pixels();
                assert_eq!(s.needed(), 40usize.div_ceil(wide) * if attached { 2 } else { 1 });
                // Each sprite's words read back give the pixels' bits in its column.
                for (n, words) in s.sprites.iter().enumerate() {
                    let (column, high) = if attached { (n / 2, n % 2 == 1) } else { (n, false) };
                    let (vstart, hstart, height, attach, bits) = read_sprite(words, width).unwrap();
                    assert_eq!((vstart, hstart, height, attach), (0x40, 0x90 + (column * wide) as u16, 30, high));
                    assert_eq!(words.len(), (2 + 30 * 2 + 2) * width.words() * 2);
                    for y in 0..30 {
                        for x in 0..wide {
                            let at = column * wide + x;
                            let shift = if high { 2 } else { 0 };
                            let want = if at < 40 { (s.values[y * 40 + at] >> shift) & 3 } else { 0 };
                            assert_eq!(bits[y * wide + x], want, "sprite {n} at {x},{y}");
                        }
                    }
                }
                // The see-through pixels are 0, the rest not.
                assert!(s.values.iter().zip(&c.opaque).all(|(&v, &o)| (v != 0) == o));
                assert!(s.values.iter().all(|&v| v <= if attached { 15 } else { 3 }));
            }
        }
    }

    #[test]
    fn the_sprites_colours_go_in_their_registers() {
        let c = cut(&ball(40, 30), 40, 30, planar::Filter::Box, Transparency::Alpha);
        let s = make_sprites(&c, SpriteSettings::default(), Chipset::Ocs, Dither::None, 0.0);
        // Three columns: sprites 0 and 1 in COLOR17 to 19, sprite 2 in COLOR21 to 23.
        assert_eq!(s.registers().iter().map(|r| r.0).collect::<Vec<_>>(), [17, 18, 19, 21, 22, 23]);
        let source = sprites_source(&s, "ball", None);
        assert!(source.contains("dc.w    $01a2,"), "{source}");
        assert!(source.contains("ball_spr2:    ; sprite 2\n        dc.w    $2c50,$4a01,"), "{source}");
        let attached = make_sprites(&c, SpriteSettings { attached: true, ..SpriteSettings::default() }, Chipset::Ocs, Dither::None, 0.0);
        assert_eq!(attached.registers().len(), 15);
        let source = sprites_source(&attached, "ball", Some("ball.spr"));
        assert!(source.contains("ball_spr1:\n        incbin  \"ball.spr\",128,128    ; sprite 1"), "{source}");
        assert!(dc_words(&[0x12, 0x34, 0xAB, 0xCD]).contains("dc.w    $1234,$abcd"));
    }

    #[test]
    fn a_bob_goes_out_and_comes_back_with_its_mask() {
        let image = ball(40, 30);
        for planes in 1..=5 {
            for interleaved in [false, true] {
                for extra_word in [false, true] {
                    let c = cut(&image, 40, 30, planar::Filter::Bilinear, Transparency::Alpha);
                    let b = make_bob(&c, BobSettings { planes, interleaved, extra_word }, &[], Chipset::Ocs, Dither::Ordered, 0.6);
                    let words = if extra_word { 4 } else { 3 };
                    assert_eq!(b.words(), words);
                    let data = b.planes();
                    assert_eq!(data.len(), words * 2 * 30 * planes);
                    let back = planar::from_planes(&data, words * 16, 30, planes, interleaved, Fetch::Words);
                    for y in 0..30 {
                        for x in 0..words * 16 {
                            let want = if x < 40 { b.values[y * 40 + x] } else { 0 };
                            assert_eq!(back[y * words * 16 + x], want);
                        }
                    }
                    let mask = b.mask();
                    let mask_rows = if interleaved { 30 * planes } else { 30 };
                    assert_eq!(mask.len(), words * 2 * mask_rows);
                    for row in 0..mask_rows {
                        let y = if interleaved { row / planes } else { row };
                        for x in 0..words * 16 {
                            let set = mask[row * words * 2 + x / 8] & (0x80 >> (x % 8)) != 0;
                            assert_eq!(set, x < 40 && c.opaque[y * 40 + x]);
                        }
                    }
                    assert!(b.values.iter().zip(&b.opaque).all(|(&v, &o)| (v != 0) == o));
                    let size = b.bltsize().unwrap();
                    assert_eq!((usize::from(size >> 6), usize::from(size & 63)), (mask_rows, words));
                }
            }
        }
    }

    #[test]
    fn a_bob_keeps_the_screens_palette_and_says_its_sizes() {
        let screen = vec![[0, 0, 0], [255, 0, 0], [0, 255, 0], [0, 0, 255]];
        let c = cut(&ball(32, 16), 32, 16, planar::Filter::Box, Transparency::Colour([0, 0, 0]));
        let b = make_bob(&c, BobSettings { planes: 2, interleaved: true, extra_word: true }, &screen, Chipset::Ocs, Dither::None, 0.0);
        assert_eq!(b.palette, screen);
        let source = bob_source(&b, "ball", None);
        assert!(source.contains("ball_BLTSIZE   equ $0803    ; 32 rows by 3 words, every plane in one blit"), "{source}");
        assert!(source.contains("ball_PLANESTEP equ 6"));
        let source = bob_source(&b, "ball", Some(("ball.bpl", "ball.msk")));
        assert!(source.contains("incbin  \"ball.msk\"    ; 192 bytes"), "{source}");
    }
}
