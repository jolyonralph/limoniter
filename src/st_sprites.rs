//! Software sprites and 1-plane fonts for the Atari ST, from a picture.
//!
//! The ST has no hardware sprites: a demo draws one by cutting its shape out of the screen with a mask (AND) and
//! putting its colours in (OR), 16 pixels a word, in the screen's own interleaved layout. To put it at any pixel
//! without shifting at run time, it's kept pre-shifted: 16 copies, each a pixel further right and a word wider. The
//! mask is kept a word for each plane beside the planes' words (so that `and.l` and `or.l` do two planes at once),
//! or as a single plane after the picture's.
//!
//! A font for a scroller is cut from a picture of a grid of characters, 8x8 (a byte a row) or 16x16 (a word a
//! row), one plane.

use crate::amiga_sprites::{Cut, Transparency, cut};
use crate::quantise::{self, Dither, Filter, Image, identifier};
use crate::st_planar::{self as st, Levels};
use std::fmt::Write as _;

/// Where the mask is kept.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MaskKind {
    /// A mask word for each plane, then the planes' words, for each 16 pixels.
    PerPlane,
    /// A single plane after the picture's planes.
    Single,
    None,
}

impl MaskKind {
    pub const ALL: [Self; 3] = [Self::PerPlane, Self::Single, Self::None];

    pub fn name(self) -> &'static str {
        match self {
            Self::PerPlane => "A word a plane, beside the planes",
            Self::Single => "One plane, after the picture",
            Self::None => "None",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SpriteSettings {
    pub planes: usize,
    pub preshifted: bool,
    pub mask: MaskKind,
    /// Mask bits set where the background shows (ready to AND), rather than where the sprite does.
    pub mask_background: bool,
}

impl Default for SpriteSettings {
    fn default() -> Self {
        SpriteSettings { planes: 4, preshifted: true, mask: MaskKind::PerPlane, mask_background: true }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Sprite {
    pub width: usize,
    pub height: usize,
    pub settings: SpriteSettings,
    pub palette: Vec<[u8; 3]>,
    pub values: Vec<u8>,
    pub opaque: Vec<bool>,
    pub rgba: Vec<u8>,
}

/// A sprite from a picture cut to size: its colours found from it (colour 0 left for what's behind) or the screen
/// picture's palette used.
pub fn make_sprite(cut: &Cut, settings: SpriteSettings, screen_palette: &[[u8; 3]], levels: Levels, dither: Dither, strength: f32) -> Sprite {
    let planes = settings.planes.clamp(1, 4);
    let settings = SpriteSettings { planes, ..settings };
    let shown: Vec<[f32; 3]> = cut.pixels.iter().zip(&cut.opaque).filter(|(_, o)| **o).map(|(p, _)| *p).collect();
    let (palette, values) = if screen_palette.is_empty() {
        let snap = |c: [f32; 3]| levels.snap_colour(c);
        let colours = quantise::find_palette(&shown, &vec![None; (1 << planes) - 1], &snap, None);
        let values = quantise::choose(&cut.pixels, cut.width, cut.height, &colours, dither, strength);
        let mut palette = vec![[0, 0, 0]];
        palette.extend(colours);
        (palette, values.iter().zip(&cut.opaque).map(|(&v, &o)| if o { v + 1 } else { 0 }).collect::<Vec<u8>>())
    } else {
        let palette: Vec<[u8; 3]> = screen_palette.iter().copied().take(1 << planes).collect();
        let values = quantise::choose(&cut.pixels, cut.width, cut.height, &palette, dither, strength);
        (palette, values.iter().zip(&cut.opaque).map(|(&v, &o)| if o { v } else { 0 }).collect::<Vec<u8>>())
    };
    let rgba = values.iter().zip(&cut.opaque).flat_map(|(&v, &o)| {
        let [r, g, b] = palette[usize::from(v)];
        [r, g, b, if o { 255 } else { 0 }]
    }).collect();
    Sprite { width: cut.width, height: cut.height, settings, palette, values, opaque: cut.opaque.clone(), rgba }
}

impl Sprite {
    /// Words a row of one plane in each copy: the width rounded up to 16, and one more when pre-shifted.
    pub fn words(&self) -> usize {
        self.width.div_ceil(16) + usize::from(self.settings.preshifted)
    }

    pub fn copies(&self) -> usize {
        if self.settings.preshifted { 16 } else { 1 }
    }

    /// One copy, moved `shift` pixels right: its values and where it shows, `words` * 16 wide.
    fn shifted(&self, shift: usize) -> (Vec<u8>, Vec<bool>) {
        let wide = self.words() * 16;
        let mut values = vec![0u8; wide * self.height];
        let mut shows = vec![false; wide * self.height];
        for y in 0..self.height {
            for x in 0..self.width {
                values[y * wide + x + shift] = self.values[y * self.width + x];
                shows[y * wide + x + shift] = self.opaque[y * self.width + x];
            }
        }
        (values, shows)
    }

    /// Bytes in one copy.
    pub fn copy_bytes(&self) -> usize {
        let planes = self.settings.planes + if self.settings.mask == MaskKind::None { 0 } else if self.settings.mask == MaskKind::PerPlane { self.settings.planes } else { 1 };
        self.words() * 2 * planes * self.height
    }

    /// Every copy, one after another.
    pub fn data(&self) -> Vec<u8> {
        let (planes, words, wide) = (self.settings.planes, self.words(), self.words() * 16);
        let mut out = Vec::with_capacity(self.copy_bytes() * self.copies());
        for shift in 0..self.copies() {
            let (values, shows) = self.shifted(shift);
            let mask: Vec<u8> = shows.iter().map(|&s| u8::from(s != self.settings.mask_background)).collect();
            let picture = st::to_interleaved(&values, wide, self.height, planes);
            let single = st::to_interleaved(&mask, wide, self.height, 1);
            match self.settings.mask {
                MaskKind::PerPlane => {
                    for y in 0..self.height {
                        for group in 0..words {
                            let m = &single[(y * words + group) * 2..][..2];
                            for _ in 0..planes {
                                out.extend_from_slice(m);
                            }
                            out.extend_from_slice(&picture[(y * words + group) * planes * 2..][..planes * 2]);
                        }
                    }
                }
                MaskKind::Single => {
                    out.extend(picture);
                    out.extend(single);
                }
                MaskKind::None => out.extend(picture),
            }
        }
        out
    }
}

/// `dc.w` lines, eight words to a line.
pub fn dc_words(bytes: &[u8]) -> String {
    let mut out = String::new();
    for chunk in bytes.chunks(16) {
        let words: Vec<String> = chunk.chunks(2).map(|w| format!("${:02x}{:02x}", w[0], w.get(1).copied().unwrap_or(0))).collect();
        let _ = writeln!(out, "        dc.w    {}", words.join(","));
    }
    out
}

/// Source for a sprite: its sizes, how it's laid out, a routine drawing it, and its data (as `dc.w` or INCBIN).
pub fn sprite_source(s: &Sprite, name: &str, levels: Levels, file: Option<&str>) -> String {
    let id = identifier(name);
    let mut out = String::new();
    let set = &s.settings;
    let _ = writeln!(out, "; {} by {}, {} plane{}, {} words a row{}.", s.width, s.height, set.planes, if set.planes == 1 { "" } else { "s" }, s.words(), if set.preshifted { ", 16 copies each shifted a pixel further right" } else { "" });
    let _ = writeln!(out, "; {}", match set.mask {
        MaskKind::PerPlane => "For each 16 pixels: the mask word once for each plane, then each plane's word.",
        MaskKind::Single => "Each copy: the planes interleaved (a word of each plane for each 16 pixels), then the mask, one plane.",
        MaskKind::None => "The planes interleaved, a word of each plane for each 16 pixels; no mask.",
    });
    if set.mask != MaskKind::None {
        let _ = writeln!(out, "; The mask's bits are set where {} shows.", if set.mask_background { "the background" } else { "the sprite" });
    }
    let _ = writeln!(out);
    let _ = writeln!(out, "{id}_WIDTH     equ {}", s.width);
    let _ = writeln!(out, "{id}_HEIGHT    equ {}", s.height);
    let _ = writeln!(out, "{id}_PLANES    equ {}", set.planes);
    let _ = writeln!(out, "{id}_WORDS     equ {}    ; words a row of one plane", s.words());
    let _ = writeln!(out, "{id}_COPY      equ {}    ; bytes in one copy", s.copy_bytes());
    let _ = writeln!(out, "{id}_COPIES    equ {}", s.copies());
    let _ = writeln!(out);
    if set.mask == MaskKind::PerPlane && set.planes == 4 && set.mask_background {
        let _ = writeln!(out, "; Draws it on a low resolution screen: a0 the screen, d0 x, d1 y. Uses d0-d3/a0-a1.");
        let _ = writeln!(out, "{id}_draw:");
        let _ = writeln!(out, "        mulu    #160,d1");
        let _ = writeln!(out, "        add.l   d1,a0");
        let _ = writeln!(out, "        move.w  d0,d1");
        let _ = writeln!(out, "        and.w   #$fff0,d1");
        let _ = writeln!(out, "        lsr.w   #1,d1               ; 8 bytes for each 16 pixels");
        let _ = writeln!(out, "        add.w   d1,a0");
        let _ = writeln!(out, "        lea     {id}_data,a1");
        if set.preshifted {
            let _ = writeln!(out, "        and.w   #15,d0");
            let _ = writeln!(out, "        mulu    #{id}_COPY,d0");
            let _ = writeln!(out, "        add.l   d0,a1");
        }
        let _ = writeln!(out, "        move.w  #{id}_HEIGHT-1,d2");
        let _ = writeln!(out, ".row:   move.w  #{id}_WORDS-1,d3");
        let _ = writeln!(out, ".word:  move.l  (a1)+,d0            ; the mask for planes 0 and 1");
        let _ = writeln!(out, "        addq.l  #4,a1               ; the same for planes 2 and 3");
        let _ = writeln!(out, "        and.l   d0,(a0)");
        let _ = writeln!(out, "        and.l   d0,4(a0)");
        let _ = writeln!(out, "        move.l  (a1)+,d0");
        let _ = writeln!(out, "        or.l    d0,(a0)+");
        let _ = writeln!(out, "        move.l  (a1)+,d0");
        let _ = writeln!(out, "        or.l    d0,(a0)+");
        let _ = writeln!(out, "        dbf     d3,.word");
        let _ = writeln!(out, "        lea     160-{id}_WORDS*8(a0),a0");
        let _ = writeln!(out, "        dbf     d2,.row");
        let _ = writeln!(out, "        rts");
        let _ = writeln!(out);
    }
    let _ = writeln!(out, "        section data");
    let words: Vec<u16> = s.palette.iter().map(|&c| st::st_word(c, levels)).collect();
    let _ = writeln!(out, "{id}_palette:");
    for chunk in words.chunks(8) {
        let _ = writeln!(out, "        dc.w    {}", chunk.iter().map(|w| format!("${w:04x}")).collect::<Vec<_>>().join(","));
    }
    let data = s.data();
    let _ = writeln!(out, "{id}_data:");
    match file {
        Some(file) => {
            let _ = writeln!(out, "        incbin  \"{file}\"    ; {} bytes", data.len());
        }
        None => out.push_str(&dc_words(&data)),
    }
    out
}

/// Which pixels of a font's picture are set.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Ink {
    /// Brighter than half.
    Bright,
    /// Not the top left pixel's colour.
    NotBackground,
    /// Where the picture isn't see-through.
    Opaque,
}

impl Ink {
    pub const ALL: [Self; 3] = [Self::NotBackground, Self::Bright, Self::Opaque];

    pub fn name(self) -> &'static str {
        match self {
            Self::Bright => "Brighter than half",
            Self::NotBackground => "Not the top left pixel's colour",
            Self::Opaque => "Not see-through",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FontSettings {
    /// 8 or 16, square.
    pub size: usize,
    /// The first character's code, for the source's notes.
    pub first: u8,
    /// How many characters, at most.
    pub count: usize,
    pub ink: Ink,
}

impl Default for FontSettings {
    fn default() -> Self {
        FontSettings { size: 8, first: 32, count: 96, ink: Ink::NotBackground }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Font {
    pub settings: FontSettings,
    /// Each character's rows, a bit a pixel from the left.
    pub glyphs: Vec<Vec<u16>>,
}

/// A font from a picture of a grid of characters, left to right and down, at the picture's own pixels.
pub fn make_font(image: &Image, settings: FontSettings) -> Font {
    let size = if settings.size >= 16 { 16 } else { 8 };
    let settings = FontSettings { size, ..settings };
    let (across, down) = (image.width / size, image.height / size);
    let pixel = |x: usize, y: usize| -> [u8; 4] {
        let at = (y * image.width + x) * 4;
        [image.rgba[at], image.rgba[at + 1], image.rgba[at + 2], image.rgba[at + 3]]
    };
    let background = if image.width > 0 && image.height > 0 { pixel(0, 0) } else { [0; 4] };
    let set = |p: [u8; 4]| -> bool {
        match settings.ink {
            Ink::Bright => u32::from(p[0]) * 299 + u32::from(p[1]) * 587 + u32::from(p[2]) * 114 >= 128 * 1000 && p[3] >= 128,
            Ink::NotBackground => (0..3).any(|i| p[i].abs_diff(background[i]) > 12) || p[3].abs_diff(background[3]) > 127,
            Ink::Opaque => p[3] >= 128,
        }
    };
    let mut glyphs = Vec::new();
    'cells: for row in 0..down {
        for column in 0..across {
            if glyphs.len() >= settings.count {
                break 'cells;
            }
            let glyph = (0..size)
                .map(|y| {
                    (0..size).fold(0u16, |bits, x| {
                        let on = set(pixel(column * size + x, row * size + y));
                        bits | u16::from(on) << (size - 1 - x)
                    })
                })
                .collect();
            glyphs.push(glyph);
        }
    }
    Font { settings, glyphs }
}

impl Font {
    /// The characters one after another: a byte a row (8x8) or a word (16x16).
    pub fn data(&self) -> Vec<u8> {
        self.glyphs.iter().flatten().flat_map(|&row| if self.settings.size == 8 { vec![row as u8] } else { row.to_be_bytes().to_vec() }).collect()
    }

    /// The font drawn, 16 characters a row, white on black.
    pub fn picture(&self) -> (usize, usize, Vec<u8>) {
        let size = self.settings.size;
        let across = 16;
        let down = self.glyphs.len().div_ceil(across).max(1);
        let (width, height) = (across * size, down * size);
        let mut rgba = vec![0u8; width * height * 4];
        for (i, glyph) in self.glyphs.iter().enumerate() {
            let (gx, gy) = (i % across * size, i / across * size);
            for (y, &row) in glyph.iter().enumerate() {
                for x in 0..size {
                    let on = row >> (size - 1 - x) & 1 != 0;
                    let shade = if on { 255 } else if (i + i / across) % 2 == 0 { 0 } else { 40 };
                    let at = ((gy + y) * width + gx + x) * 4;
                    rgba[at..at + 4].copy_from_slice(&[shade, shade, shade, 255]);
                }
            }
        }
        (width, height, rgba)
    }
}

/// Source for a font: its sizes and characters (as `dc.b`/`dc.w` or INCBIN).
pub fn font_source(f: &Font, name: &str, file: Option<&str>) -> String {
    let id = identifier(name);
    let size = f.settings.size;
    let mut out = String::new();
    let _ = writeln!(out, "; {} characters of {size}x{size}, one plane: {} a row, the top row first.", f.glyphs.len(), if size == 8 { "a byte" } else { "a word" });
    let _ = writeln!(out, "; The first is character {}; a character's rows start at {id}_font + (code - {id}_FIRST) * {id}_BYTES.", f.settings.first);
    let _ = writeln!(out);
    let _ = writeln!(out, "{id}_SIZE      equ {size}");
    let _ = writeln!(out, "{id}_FIRST     equ {}", f.settings.first);
    let _ = writeln!(out, "{id}_COUNT     equ {}", f.glyphs.len());
    let _ = writeln!(out, "{id}_BYTES     equ {}    ; bytes a character", size * size / 8);
    let _ = writeln!(out);
    let _ = writeln!(out, "        section data");
    let _ = writeln!(out, "{id}_font:");
    match file {
        Some(file) => {
            let _ = writeln!(out, "        incbin  \"{file}\"    ; {} bytes", f.data().len());
        }
        None => {
            for (i, glyph) in f.glyphs.iter().enumerate() {
                let code = usize::from(f.settings.first) + i;
                let shown = char::from_u32(code as u32).filter(|c| c.is_ascii_graphic()).map_or(String::new(), |c| format!(" '{c}'"));
                let items: Vec<String> = glyph.iter().map(|&row| if size == 8 { format!("${:02x}", row as u8) } else { format!("${row:04x}") }).collect();
                let _ = writeln!(out, "        dc.{}    {}    ; {code}{shown}", if size == 8 { 'b' } else { 'w' }, items.join(","));
            }
        }
    }
    out
}

/// The picture cut to a sprite's size, as the Amiga converter cuts it.
pub fn cut_sprite(image: &Image, width: usize, height: usize, filter: Filter, transparency: Transparency) -> Cut {
    cut(image, width, height, filter, transparency)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ball(width: usize, height: usize) -> Image {
        let mut rgba = Vec::new();
        for y in 0..height {
            for x in 0..width {
                let (dx, dy) = (x as f32 / width as f32 - 0.5, y as f32 / height as f32 - 0.5);
                let inside = dx * dx + dy * dy < 0.2;
                rgba.extend_from_slice(&[(x * 8) as u8, (y * 8) as u8, 200, if inside { 255 } else { 0 }]);
            }
        }
        Image { width, height, rgba }
    }

    #[test]
    fn preshifted_copies_move_a_pixel_each_and_the_mask_matches() {
        let c = cut_sprite(&ball(20, 10), 20, 10, Filter::Box, Transparency::Alpha);
        for mask in MaskKind::ALL {
            let s = make_sprite(&c, SpriteSettings { planes: 4, preshifted: true, mask, mask_background: true }, &[], Levels::St, Dither::None, 0.0);
            assert_eq!(s.words(), 3);
            let data = s.data();
            assert_eq!(data.len(), s.copy_bytes() * 16);
            for shift in [0, 1, 7, 15] {
                let copy = &data[shift * s.copy_bytes()..][..s.copy_bytes()];
                // Read the planes (and the mask) back from the copy.
                let (picture, mask_plane): (Vec<u8>, Vec<u8>) = match mask {
                    MaskKind::PerPlane => {
                        let (mut p, mut m) = (Vec::new(), Vec::new());
                        for group in copy.chunks(16) {
                            m.extend_from_slice(&group[..2]);
                            assert!(group[..8].chunks(2).all(|w| w == &group[..2]), "the mask word for each plane");
                            p.extend_from_slice(&group[8..]);
                        }
                        (p, m)
                    }
                    MaskKind::Single => {
                        let size = s.words() * 8 * 10;
                        (copy[..size].to_vec(), copy[size..].to_vec())
                    }
                    MaskKind::None => (copy.to_vec(), Vec::new()),
                };
                let values = st::from_interleaved(&picture, 48, 10, 4);
                let shows = if mask_plane.is_empty() { Vec::new() } else { st::from_interleaved(&mask_plane, 48, 10, 1) };
                for y in 0..10 {
                    for x in 0..20 {
                        assert_eq!(values[y * 48 + x + shift], s.values[y * 20 + x]);
                        if !shows.is_empty() {
                            assert_eq!(shows[y * 48 + x + shift] == 0, s.opaque[y * 20 + x], "mask set on the background");
                        }
                    }
                    if !shows.is_empty() {
                        assert!((0..shift).all(|x| shows[y * 48 + x] == 1));
                    }
                }
            }
        }
    }

    #[test]
    fn a_font_is_cut_from_a_grid() {
        // Two 8x8 characters side by side: the first a vertical bar in column 0, the second its top row.
        let (w, h) = (16, 8);
        let mut rgba = vec![0u8; w * h * 4];
        for y in 0..h {
            for x in 0..w {
                let on = (x == 1 && y > 0) || (x >= 8 && y == 0);
                let v = if on { 255 } else { 0 };
                rgba[(y * w + x) * 4..][..4].copy_from_slice(&[v, v, v, 255]);
            }
        }
        let font = make_font(&Image { width: w, height: h, rgba }, FontSettings::default());
        assert_eq!(font.glyphs.len(), 2);
        assert_eq!(font.glyphs[0], vec![0, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40]);
        assert_eq!(font.glyphs[1][0], 0xFF);
        assert_eq!(font.data().len(), 16);
        assert!(font_source(&font, "font", None).contains("dc.b    $00,$40"));
    }
}
