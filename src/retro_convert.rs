//! Any picture made into the screens of the 8-bit machines that keep colour by the cell or by a palette: the ZX
//! Spectrum (an INK and a PAPER for each 8×8 cell sharing BRIGHT; the Timex machines' hi-colour, the same for each
//! 8×1, and hi-res, 512×192 in one of eight pairs), the Spectrum Next (Layer 2 at 256×192, 320×256 and 640×256, and
//! LoRes, through its 9-bit palette: see `next_screen`), the Amstrad CPC (modes 0, 1 and 2,
//! 16, 4 or 2 of its 27 colours), the MSX's Screen 2 (two of the TMS9918's 15 colours for each 8×1), and the BBC
//! Micro (modes 0, 1, 2, 4 and 5, of its 8 colours).
//!
//! The picture is scaled to the screen with the pixel's shape in mind, its brightness, contrast, saturation and
//! gamma set, and then given its colours: for cells, each cell's pair is the one that leaves the least difference
//! from the picture in Oklab (as the C64 converter chooses), dithering kept within it; for a palette, the colours
//! the picture needs are found from those the machine has, and every pixel given its nearest, dithered if asked.
//! What comes out is saved as each machine loads it: a .scr or a tape with a loader for the Spectrum, an .nxi for
//! the Next, a .scr with AMSDOS's header for the CPC, a BSAVEd .sc2 for the MSX, a screen dump for *LOAD on the
//! BBC, each with a BASIC loader or assembler source to show it, and a PNG.

use crate::c64_gfx;
use crate::next_screen::{NextMode, NextScreen};
use crate::next_tiles::{SpriteSettings, Sprites, TileSettings, Tilemap};
use crate::quantise::{self, Dither, Filter, Fit, Image, as_f32, oklab};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Machine {
    Spectrum,
    Next,
    Cpc,
    Msx,
    Bbc,
}

impl Machine {
    pub const ALL: [Machine; 5] = [Machine::Spectrum, Machine::Next, Machine::Cpc, Machine::Msx, Machine::Bbc];

    pub fn name(self) -> &'static str {
        match self {
            Machine::Spectrum => "ZX Spectrum",
            Machine::Next => "Spectrum Next",
            Machine::Cpc => "Amstrad CPC",
            Machine::Msx => "MSX",
            Machine::Bbc => "BBC Micro",
        }
    }

    pub fn modes(self) -> &'static [Mode] {
        match self {
            Machine::Spectrum => &[Mode::Spectrum, Mode::Timex, Mode::TimexHires],
            Machine::Next => &[Mode::Layer2, Mode::Layer2Wide, Mode::Layer2Hi, Mode::LoRes, Mode::Tiles, Mode::TilesWide, Mode::Sprites],
            Machine::Cpc => &[Mode::Cpc0, Mode::Cpc1, Mode::Cpc2],
            Machine::Msx => &[Mode::Msx2],
            Machine::Bbc => &[Mode::Bbc0, Mode::Bbc1, Mode::Bbc2, Mode::Bbc4, Mode::Bbc5],
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    /// 256×192, an INK and a PAPER for each 8×8 cell, both BRIGHT or neither.
    Spectrum,
    /// The Timex TC2048's and TS2068's hi-colour: the same for each 8×1.
    Timex,
    /// The Timex machines' hi-res (and the Next's): 512×192 in one of eight pairs of colours.
    TimexHires,
    /// The Spectrum Next's Layer 2: 256×192, a byte a pixel, 256 colours of 512.
    Layer2,
    /// Layer 2 at 320×256.
    Layer2Wide,
    /// Layer 2 at 640×256, 16 colours.
    Layer2Hi,
    /// The Next's LoRes: 128×96 in 256 colours.
    LoRes,
    /// The Next's tilemap, 40×32 tiles of 8×8.
    Tiles,
    /// The tilemap at 80×32.
    TilesWide,
    /// The Next's 16×16 sprites, cut from the picture at its own size.
    Sprites,
    Cpc0,
    Cpc1,
    Cpc2,
    /// Screen 2: 256×192, two colours for each 8×1.
    Msx2,
    Bbc0,
    Bbc1,
    Bbc2,
    Bbc4,
    Bbc5,
}

/// How a mode keeps its colours.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Kind {
    /// Two colours for each cell of this size.
    Cells(usize, usize),
    /// Every pixel any of this many colours, found from the machine's.
    Palette(usize),
    /// The whole screen one of the Timex hi-res mode's eight pairs.
    Pair,
    /// The Next's tiles and map.
    Tiles,
    /// The Next's sprites.
    Sprites,
}

impl Mode {
    pub fn machine(self) -> Machine {
        match self {
            Mode::Spectrum | Mode::Timex | Mode::TimexHires => Machine::Spectrum,
            Mode::Layer2 | Mode::Layer2Wide | Mode::Layer2Hi | Mode::LoRes | Mode::Tiles | Mode::TilesWide | Mode::Sprites => Machine::Next,
            Mode::Cpc0 | Mode::Cpc1 | Mode::Cpc2 => Machine::Cpc,
            Mode::Msx2 => Machine::Msx,
            _ => Machine::Bbc,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Mode::Spectrum => "Spectrum: 256\u{D7}192, two colours each 8\u{D7}8",
            Mode::Timex => "Timex hi-colour: two colours each 8\u{D7}1",
            Mode::TimexHires => "Timex hi-res: 512\u{D7}192, two colours",
            Mode::Layer2 | Mode::Layer2Wide | Mode::Layer2Hi | Mode::LoRes => self.next_mode().map_or("", NextMode::name),
            Mode::Tiles => "Tilemap: 40\u{D7}32 tiles of 8\u{D7}8, 16 colours each",
            Mode::TilesWide => "Tilemap: 80\u{D7}32 tiles of 8\u{D7}8, 16 colours each",
            Mode::Sprites => "Sprites: 16\u{D7}16, cut from the picture",
            Mode::Cpc0 => "Mode 0: 160\u{D7}200, 16 colours",
            Mode::Cpc1 => "Mode 1: 320\u{D7}200, 4 colours",
            Mode::Cpc2 => "Mode 2: 640\u{D7}200, 2 colours",
            Mode::Msx2 => "Screen 2: 256\u{D7}192, two colours each 8\u{D7}1",
            Mode::Bbc0 => "MODE 0: 640\u{D7}256, 2 colours",
            Mode::Bbc1 => "MODE 1: 320\u{D7}256, 4 colours",
            Mode::Bbc2 => "MODE 2: 160\u{D7}256, 8 colours",
            Mode::Bbc4 => "MODE 4: 320\u{D7}256, 2 colours",
            Mode::Bbc5 => "MODE 5: 160\u{D7}256, 4 colours",
        }
    }

    pub fn size(self) -> (usize, usize) {
        match self {
            Mode::Spectrum | Mode::Timex | Mode::Msx2 => (256, 192),
            Mode::TimexHires => (512, 192),
            Mode::Layer2 | Mode::Layer2Wide | Mode::Layer2Hi | Mode::LoRes => self.next_mode().map_or((256, 192), NextMode::size),
            Mode::Tiles => (320, 256),
            Mode::TilesWide => (640, 256),
            // Its size is the sprites', from the settings.
            Mode::Sprites => (16, 16),
            Mode::Cpc0 => (160, 200),
            Mode::Cpc1 => (320, 200),
            Mode::Cpc2 => (640, 200),
            Mode::Bbc0 => (640, 256),
            Mode::Bbc1 | Mode::Bbc4 => (320, 256),
            Mode::Bbc2 | Mode::Bbc5 => (160, 256),
        }
    }

    /// How wide a pixel is for its height on a 4:3 screen.
    pub fn aspect(self) -> f32 {
        match self {
            Mode::Spectrum | Mode::Timex | Mode::Msx2 | Mode::Cpc1 => 1.0,
            Mode::TimexHires => 0.5,
            Mode::Layer2 | Mode::Layer2Wide | Mode::Layer2Hi | Mode::LoRes => self.next_mode().map_or(1.0, NextMode::aspect),
            Mode::Tiles | Mode::Sprites => 1.0,
            Mode::TilesWide => 0.5,
            Mode::Cpc0 => 2.0,
            Mode::Cpc2 => 0.5,
            Mode::Bbc1 | Mode::Bbc4 => 1.0667,
            Mode::Bbc0 => 0.5333,
            Mode::Bbc2 | Mode::Bbc5 => 2.1333,
        }
    }

    fn kind(self) -> Kind {
        match self {
            Mode::Spectrum => Kind::Cells(8, 8),
            Mode::Timex | Mode::Msx2 => Kind::Cells(8, 1),
            Mode::TimexHires => Kind::Pair,
            Mode::Layer2 | Mode::Layer2Wide | Mode::LoRes => Kind::Palette(256),
            Mode::Layer2Hi => Kind::Palette(16),
            Mode::Tiles | Mode::TilesWide => Kind::Tiles,
            Mode::Sprites => Kind::Sprites,
            Mode::Cpc0 => Kind::Palette(16),
            Mode::Cpc1 | Mode::Bbc1 | Mode::Bbc5 => Kind::Palette(4),
            Mode::Cpc2 | Mode::Bbc0 | Mode::Bbc4 => Kind::Palette(2),
            Mode::Bbc2 => Kind::Palette(8),
        }
    }

    /// The Next's own mode, for its modes.
    pub fn next_mode(self) -> Option<NextMode> {
        match self {
            Mode::Layer2 => Some(NextMode::Layer2),
            Mode::Layer2Wide => Some(NextMode::Layer2Wide),
            Mode::Layer2Hi => Some(NextMode::Layer2Hi),
            Mode::LoRes => Some(NextMode::LoRes),
            _ => None,
        }
    }

    /// The colours the machine has, to choose from.
    pub fn gamut(self) -> Vec<[u8; 3]> {
        match self.machine() {
            Machine::Next => (0..512u16).map(crate::next_screen::rgb).collect(),
            Machine::Spectrum => SPECTRUM.to_vec(),
            Machine::Cpc => CPC.to_vec(),
            Machine::Msx => TMS9918.to_vec(),
            Machine::Bbc => BBC.to_vec(),
        }
    }
}

/// The Spectrum's colours: 0 to 7 normal, 8 to 15 BRIGHT, as most emulators show them.
pub const SPECTRUM: [[u8; 3]; 16] = {
    let mut c = [[0u8; 3]; 16];
    let mut i = 0;
    while i < 16 {
        let level = if i < 8 { 0xD7 } else { 0xFF };
        let index = i % 8;
        c[i] = [if index & 2 != 0 { level } else { 0 }, if index & 4 != 0 { level } else { 0 }, if index & 1 != 0 { level } else { 0 }];
        i += 1;
    }
    c
};

/// The CPC's 27 colours, by the firmware's numbers: green, red and blue each off, half or full, nine at a time.
pub const CPC: [[u8; 3]; 27] = {
    let mut c = [[0u8; 3]; 27];
    let levels = [0x00, 0x80, 0xFF];
    let mut i = 0;
    while i < 27 {
        c[i] = [levels[(i / 3) % 3], levels[i / 9], levels[i % 3]];
        i += 1;
    }
    c
};

/// The TMS9918's colours as the MSX shows them (0 is see-through, shown black).
pub const TMS9918: [[u8; 3]; 16] = [
    [0x00, 0x00, 0x00],
    [0x00, 0x00, 0x00],
    [0x21, 0xC8, 0x42],
    [0x5E, 0xDC, 0x78],
    [0x54, 0x55, 0xED],
    [0x7D, 0x76, 0xFC],
    [0xD4, 0x52, 0x4D],
    [0x42, 0xEB, 0xF5],
    [0xFC, 0x55, 0x54],
    [0xFF, 0x79, 0x78],
    [0xD4, 0xC1, 0x54],
    [0xE6, 0xCE, 0x80],
    [0x21, 0xB0, 0x3B],
    [0xC9, 0x5B, 0xBA],
    [0xCC, 0xCC, 0xCC],
    [0xFF, 0xFF, 0xFF],
];

/// The BBC Micro's eight: black, red, green, yellow, blue, magenta, cyan, white.
pub const BBC: [[u8; 3]; 8] = [[0, 0, 0], [255, 0, 0], [0, 255, 0], [255, 255, 0], [0, 0, 255], [255, 0, 255], [0, 255, 255], [255, 255, 255]];

/// Whether a Spectrum picture may use BRIGHT.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Bright {
    /// Each cell's own choice.
    Choose,
    Never,
    Always,
}

impl Bright {
    pub const ALL: [Bright; 3] = [Bright::Choose, Bright::Never, Bright::Always];

    pub fn name(self) -> &'static str {
        match self {
            Bright::Choose => "Each cell as suits it",
            Bright::Never => "Never",
            Bright::Always => "Always",
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Settings {
    pub mode: Mode,
    pub fit: Fit,
    pub filter: Filter,
    pub dither: Dither,
    pub strength: f32,
    pub bright: Bright,
    /// -1 to 1, 0 as it is.
    pub brightness: f32,
    pub contrast: f32,
    pub saturation: f32,
    /// 0.5 to 2, 1 as it is.
    pub gamma: f32,
    /// Colour 0 kept black, for a border and background that are.
    pub black_first: bool,
    /// How many of the Next's 256 palette places a picture may use, the rest left as they are at reset (for
    /// sprites and the other layers).
    pub palette_size: usize,
    /// For the Next's tilemap: how many tiles and palettes, and whether turned tiles are shared.
    pub tiles: TileSettings,
    /// For the Next's sprites: where they're cut from, and how.
    pub sprites: SpriteSettings,
}

impl Default for Settings {
    fn default() -> Self {
        Settings { mode: Mode::Spectrum, fit: Fit::Fill, filter: Filter::Lanczos, dither: Dither::Ordered, strength: 0.6, bright: Bright::Choose, brightness: 0.0, contrast: 0.0, saturation: 0.0, gamma: 1.0, black_first: true, palette_size: 256, tiles: TileSettings::default(), sprites: SpriteSettings::default() }
    }
}

/// A picture made for a mode: each pixel's colour (an index into `palette`), and for cell modes each cell's colours.
#[derive(Clone, Debug, PartialEq)]
pub struct Screen {
    pub mode: Mode,
    pub width: usize,
    pub height: usize,
    /// For a cell mode, 0 for the cell's PAPER (background) and 1 for its INK (foreground); otherwise the colour's
    /// place in `palette`.
    pub values: Vec<u8>,
    /// For a cell mode, each cell's (ink, paper), as the machine's own colour numbers.
    pub cells: Vec<(u8, u8)>,
    /// For a palette mode, the colours chosen, as the machine's own numbers (the CPC's firmware's, the BBC's
    /// physical colours, the Next's 9 bits: RRRGGGBB and the lowest blue bit).
    pub palette: Vec<u16>,
    /// The Next's tilemap, for its modes.
    pub tilemap: Option<Box<Tilemap>>,
    /// The Next's sprites, for its mode.
    pub sprites: Option<Box<Sprites>>,
}

/// The picture's colours changed as the settings say, in sRGB levels.
fn adjust(pixels: &mut [[f32; 3]], s: &Settings) {
    if s.brightness == 0.0 && s.contrast == 0.0 && s.saturation == 0.0 && s.gamma == 1.0 {
        return;
    }
    let contrast = (1.0 + s.contrast).max(0.0);
    let saturation = (1.0 + s.saturation).max(0.0);
    for p in pixels {
        let grey = 0.299 * p[0] + 0.587 * p[1] + 0.114 * p[2];
        for c in p.iter_mut() {
            let mut v = grey + (*c - grey) * saturation;
            v = (v - 128.0) * contrast + 128.0 + s.brightness * 128.0;
            v = 255.0 * (v.clamp(0.0, 255.0) / 255.0).powf(1.0 / s.gamma.max(0.1));
            *c = v;
        }
    }
}

/// The picture made into a screen for the mode.
pub fn convert(image: &Image, s: &Settings) -> Screen {
    if s.mode == Mode::Sprites {
        // Cut at the picture's own size, its colours set as asked.
        let mut pixels: Vec<[f32; 3]> = image.rgba.chunks(4).map(|p| as_f32([p[0], p[1], p[2]])).collect();
        adjust(&mut pixels, s);
        let rgba = pixels.iter().zip(image.rgba.chunks(4)).flat_map(|(p, o)| [p[0].round() as u8, p[1].round() as u8, p[2].round() as u8, o[3]]).collect();
        let sprites = crate::next_tiles::cut_sprites(&Image { width: image.width, height: image.height, rgba }, &s.sprites, s.dither, s.strength);
        return Screen { mode: s.mode, width: sprites.width(), height: sprites.height(), values: Vec::new(), cells: Vec::new(), palette: Vec::new(), tilemap: None, sprites: Some(Box::new(sprites)) };
    }
    let (width, height) = s.mode.size();
    let mut pixels = quantise::scale(image, width, height, s.mode.aspect(), s.fit, s.filter);
    adjust(&mut pixels, s);
    convert_pixels(&pixels, s)
}

pub fn convert_pixels(pixels: &[[f32; 3]], s: &Settings) -> Screen {
    let mode = s.mode;
    let (width, height) = mode.size();
    match mode.kind() {
        Kind::Cells(cw, ch) => cells(pixels, width, height, cw, ch, s),
        Kind::Palette(count) => paletted(pixels, width, height, if count == 256 { s.palette_size.clamp(2, 256) } else { count }, s),
        Kind::Pair => hires(pixels, width, height, s),
        Kind::Tiles => {
            let tiles = crate::next_tiles::tilemap(pixels, &TileSettings { wide: mode == Mode::TilesWide, black_first: s.black_first, ..s.tiles.clone() }, s.dither, s.strength);
            Screen { mode, width, height, values: Vec::new(), cells: Vec::new(), palette: Vec::new(), tilemap: Some(Box::new(tiles)), sprites: None }
        }
        // Cut from the picture itself, in `convert`.
        Kind::Sprites => Screen { mode, width: 16, height: 16, values: vec![0; 256], cells: Vec::new(), palette: Vec::new(), tilemap: None, sprites: None },
    }
}

/// Timex hi-res: the pair of the eight (INK, and PAPER its opposite, both BRIGHT) that comes nearest the picture.
fn hires(pixels: &[[f32; 3]], width: usize, height: usize, s: &Settings) -> Screen {
    let labs: [[f32; 3]; 16] = std::array::from_fn(|c| oklab(as_f32(SPECTRUM[c])));
    let step = (pixels.len() / 3000).max(1);
    let sample: Vec<[f32; 3]> = pixels.iter().step_by(step).map(|&p| oklab(p)).collect();
    let mix = s.dither != Dither::None;
    let (ink, paper) = (0..8u8)
        .map(|i| (8 + i, 15 - i))
        .min_by(|&(a, b), &(c, d)| {
            let cost = |ink: u8, paper: u8| -> f32 { sample.iter().map(|&p| reach(p, &[labs[usize::from(ink)], labs[usize::from(paper)]], mix)).sum() };
            cost(a, b).total_cmp(&cost(c, d))
        })
        .unwrap_or((8, 15));
    let values = c64_gfx::assign(pixels, width, height, &|_, _| 0, &[vec![(0, paper), (1, ink)]], &SPECTRUM, s.dither, s.strength);
    Screen { mode: s.mode, width, height, values, cells: vec![(ink, paper)], palette: Vec::new(), tilemap: None, sprites: None }
}

/// Two colours for each cell. The Spectrum's 16 are two groups of 8 (normal and BRIGHT), a cell's pair from one;
/// the MSX's are one group of 15 (colour 0 is see-through).
fn cells(pixels: &[[f32; 3]], width: usize, height: usize, cw: usize, ch: usize, s: &Settings) -> Screen {
    let spectrum = s.mode.machine() == Machine::Spectrum;
    let colours: [[u8; 3]; 16] = if spectrum { SPECTRUM } else { TMS9918 };
    let labs: [[f32; 3]; 16] = std::array::from_fn(|c| oklab(as_f32(colours[c])));
    let groups: Vec<Vec<u8>> = if spectrum {
        match s.bright {
            Bright::Choose => vec![(0..8).collect(), (8..16).collect()],
            Bright::Never => vec![(0..8).collect()],
            Bright::Always => vec![(8..16).collect()],
        }
    } else {
        vec![(1..16).collect()]
    };
    let across = width / cw;
    let units = across * (height / ch);
    let unit_of = |x: usize, y: usize| (y / ch) * across + x / cw;
    let lab_pixels: Vec<[f32; 3]> = pixels.iter().map(|&p| oklab(p)).collect();
    let gathered = c64_gfx::gather(&lab_pixels, width, height, units, &unit_of);
    let mix = s.dither != Dither::None;
    // Every pair of a group tried on each cell (a colour alone too), as near as its pixels or their mixes come:
    // here hue counts as much as lightness, with only 8 colours to choose from.
    let pairs: Vec<(u8, u8)> = groups.iter().flat_map(|g| g.iter().flat_map(move |&a| g.iter().filter(move |&&b| b >= a).map(move |&b| (a, b)))).collect();
    let mut chosen: Vec<(u8, u8)> = Vec::with_capacity(units);
    for unit in &gathered {
        let mut best = ((0, 0), f32::MAX);
        for &(a, b) in &pairs {
            let set = [labs[usize::from(a)], labs[usize::from(b)]];
            let cost: f32 = unit.iter().map(|&p| reach_weighed(p, if a == b { &set[..1] } else { &set }, mix, 1.0, 0.01)).sum();
            if cost < best.1 {
                best = ((a, b), cost);
            }
        }
        // The colour most of the cell is is the PAPER, the other the INK.
        let (a, b) = best.0;
        let near_a = unit.iter().filter(|&&p| quantise::distance(p, labs[usize::from(a)]) <= quantise::distance(p, labs[usize::from(b)])).count();
        let (paper, ink) = if near_a * 2 >= unit.len() { (a, b) } else { (b, a) };
        chosen.push((ink, paper));
    }
    let allowed: Vec<c64_gfx::Allowed> = chosen.iter().map(|&(ink, paper)| if ink == paper { vec![(0, paper)] } else { vec![(0, paper), (1, ink)] }).collect();
    let values = c64_gfx::assign(pixels, width, height, &unit_of, &allowed, &colours, s.dither, s.strength);
    Screen { mode: s.mode, width, height, values, cells: chosen, palette: Vec::new(), tilemap: None, sprites: None }
}

/// A palette of `count` from the machine's colours, and each pixel's nearest.
fn paletted(pixels: &[[f32; 3]], width: usize, height: usize, count: usize, s: &Settings) -> Screen {
    let gamut = s.mode.gamut();
    let mut gamut_labs: Vec<[f32; 3]> = gamut.iter().map(|&c| oklab(as_f32(c))).collect();
    // The Next's two colours whose RRRGGGBB is $E3 are see-through to the layers behind, at reset: never chosen.
    if s.mode.machine() == Machine::Next {
        gamut_labs[0x1C6] = [9.0; 3];
        gamut_labs[0x1C7] = [9.0; 3];
    }
    let nearest_index = |rgb: [f32; 3]| quantise::nearest(&gamut_labs, oklab(rgb)).0;
    let indices: Vec<usize> = if count >= gamut.len() {
        (0..gamut.len()).collect()
    } else if count <= 4 {
        few(pixels, &gamut_labs, count, s.black_first.then_some(nearest_index([0.0; 3])), s.dither != Dither::None)
    } else {
        let slots: Vec<Option<[u8; 3]>> = (0..count).map(|i| (i == 0 && s.black_first).then_some([0, 0, 0])).collect();
        let snap = |rgb: [f32; 3]| gamut[nearest_index(rgb)];
        let mut indices: Vec<usize> = Vec::with_capacity(count);
        for c in quantise::find_palette(pixels, &slots, &snap, None) {
            let i = nearest_index(as_f32(c));
            if !indices.contains(&i) {
                indices.push(i);
            }
        }
        // Two found colours the same leave places for others: those nearest the most pixels, of the rest.
        if indices.len() < count {
            let mut used = vec![0usize; gamut.len()];
            for &p in pixels.iter().step_by(7) {
                used[nearest_index(p)] += 1;
            }
            let mut rest: Vec<usize> = (0..gamut.len()).filter(|g| !indices.contains(g)).collect();
            rest.sort_by_key(|&g| std::cmp::Reverse(used[g]));
            indices.extend(rest.into_iter().take(count - indices.len()));
        }
        indices
    };
    let colours: Vec<[u8; 3]> = indices.iter().map(|&i| gamut[i]).collect();
    // Up to 16, dithered as the C64's cells are: between the two colours whose mix comes nearest.
    let values = if colours.len() <= 16 {
        let mut sixteen = [[0u8; 3]; 16];
        sixteen[..colours.len()].copy_from_slice(&colours);
        let allowed: c64_gfx::Allowed = (0..colours.len() as u8).map(|i| (i, i)).collect();
        c64_gfx::assign(pixels, width, height, &|_, _| 0, &[allowed], &sixteen, s.dither, s.strength)
    } else {
        quantise::choose(pixels, width, height, &colours, s.dither, s.strength)
    };
    let palette = indices.iter().map(|&i| i as u16).collect();
    Screen { mode: s.mode, width, height, values, cells: Vec::new(), palette, tilemap: None, sprites: None }
}

/// The best `count` of the machine's colours, every choice of them tried on a sample of the picture: with
/// dithering, by what mixes of two of them can reach, so that colours either side of the picture's are chosen
/// rather than its average.
fn few(pixels: &[[f32; 3]], gamut: &[[f32; 3]], count: usize, fixed: Option<usize>, mix: bool) -> Vec<usize> {
    let step = (pixels.len() / 1500).max(1);
    let sample: Vec<[f32; 3]> = pixels.iter().step_by(step).map(|&p| oklab(p)).collect();
    let mut best = (Vec::new(), f32::MAX);
    let mut chosen: Vec<usize> = fixed.into_iter().collect();
    fn walk(start: usize, gamut: &[[f32; 3]], count: usize, chosen: &mut Vec<usize>, sample: &[[f32; 3]], mix: bool, best: &mut (Vec<usize>, f32)) {
        if chosen.len() == count {
            let set: Vec<[f32; 3]> = chosen.iter().map(|&c| gamut[c]).collect();
            let cost: f32 = sample.iter().map(|&p| reach(p, &set, mix)).sum();
            if cost < best.1 {
                *best = (chosen.clone(), cost);
            }
            return;
        }
        for c in start..gamut.len() {
            if !chosen.contains(&c) {
                chosen.push(c);
                walk(c + 1, gamut, count, chosen, sample, mix, best);
                chosen.pop();
            }
        }
    }
    walk(0, gamut, count, &mut chosen, &sample, mix, &mut best);
    // Darkest first, after a colour kept first.
    let keep = usize::from(fixed.is_some());
    best.0[keep..].sort_by(|&a, &b| gamut[a][0].total_cmp(&gamut[b][0]));
    best.0
}

/// How near a pixel comes to one of the colours or, dithering, to a mix of two, lightness counting twice: a
/// dithered picture is its tones first. A little is added for mixing colours far apart, which looks noisier.
/// (The C64 converter's penalty for that is much more: chosen with it, a palette is the picture's average, and
/// a Spectrum cell blue and magenta where black and yellow would do.)
fn reach(p: [f32; 3], set: &[[f32; 3]], mix: bool) -> f32 {
    reach_weighed(p, set, mix, 2.0, 0.01)
}

fn reach_weighed(p: [f32; 3], set: &[[f32; 3]], mix: bool, lightness: f32, penalty: f32) -> f32 {
    let weighed = |a: [f32; 3], b: [f32; 3]| lightness * (a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2) + (a[2] - b[2]).powi(2);
    let mut best = set.iter().map(|&c| weighed(p, c)).fold(f32::MAX, f32::min);
    if mix {
        for (i, &a) in set.iter().enumerate() {
            for &b in &set[i + 1..] {
                let ab: [f32; 3] = std::array::from_fn(|k| b[k] - a[k]);
                let length = ab.iter().map(|v| v * v).sum::<f32>();
                if length == 0.0 {
                    continue;
                }
                let t = ((0..3).map(|k| (p[k] - a[k]) * ab[k]).sum::<f32>() / length).clamp(0.0, 1.0);
                let on: [f32; 3] = std::array::from_fn(|k| a[k] + ab[k] * t);
                best = best.min(weighed(p, on) + penalty * length);
            }
        }
    }
    best
}

impl Screen {
    /// A pixel's colour as the machine shows it.
    pub fn rgb(&self, x: usize, y: usize) -> [u8; 3] {
        if let Some(tiles) = &self.tilemap {
            return tiles.shown(x, y);
        }
        if let Some(sprites) = &self.sprites {
            return sprites.shown(x, y).unwrap_or([0, 0, 0]);
        }
        let value = self.values[y * self.width + x];
        match self.mode.kind() {
            Kind::Cells(cw, ch) => {
                let (ink, paper) = self.cells[(y / ch) * (self.width / cw) + x / cw];
                let c = if value == 1 { ink } else { paper };
                if self.mode.machine() == Machine::Spectrum { SPECTRUM[usize::from(c)] } else { TMS9918[usize::from(c)] }
            }
            Kind::Palette(_) => self.mode.gamut()[usize::from(self.palette[usize::from(value)])],
            Kind::Pair => SPECTRUM[usize::from(if value == 1 { self.cells[0].0 } else { self.cells[0].1 })],
            Kind::Tiles | Kind::Sprites => [0, 0, 0],
        }
    }

    pub fn rgba(&self) -> Vec<u8> {
        if let Some(tiles) = &self.tilemap {
            return tiles.rgba();
        }
        // Sprites keep what's see-through.
        if let Some(sprites) = &self.sprites {
            return sprites.rgba();
        }
        let mut out = Vec::with_capacity(self.width * self.height * 4);
        for y in 0..self.height {
            for x in 0..self.width {
                let [r, g, b] = self.rgb(x, y);
                out.extend_from_slice(&[r, g, b, 255]);
            }
        }
        out
    }

    /// The Spectrum's screen, for the designer: pixels set where they're INK, and each cell's attribute.
    pub fn spectrum_screen(&self) -> Option<crate::screen_designer::Screen> {
        if self.mode != Mode::Spectrum {
            return None;
        }
        let pixels = self.values.iter().map(|&v| v == 1).collect();
        let attributes = self.cells.iter().map(|&(ink, paper)| (ink & 7) | (paper & 7) << 3 | if ink >= 8 || paper >= 8 { 64 } else { 0 }).collect();
        Some(crate::screen_designer::Screen { pixels, attributes })
    }

    /// The Next's screen, for its modes: the colours chosen first in its palette, the rest as at reset (for
    /// 640×256, 0).
    pub fn next_screen(&self) -> Option<NextScreen> {
        let mode = self.mode.next_mode()?;
        let mut palette = if mode == NextMode::Layer2Hi { vec![0; 256] } else { crate::next_screen::default_palette() };
        palette[..self.palette.len()].copy_from_slice(&self.palette);
        Some(NextScreen { mode, pixels: self.values.clone(), palette })
    }

    /// Everything that can be saved, by file name.
    pub fn everything(&self, name: &str) -> Vec<(String, Vec<u8>)> {
        if let Some(next) = self.next_screen() {
            return next.everything(name);
        }
        if let Some(tiles) = &self.tilemap {
            return tiles.everything(name);
        }
        if let Some(sprites) = &self.sprites {
            return sprites.everything(name);
        }
        let mut files = Vec::new();
        match self.mode {
            Mode::Spectrum => {
                let scr = self.spectrum_screen().map(|s| s.to_scr()).unwrap_or_default();
                files.push((format!("{name}.scr"), scr.clone()));
                files.push((format!("{name}.tap"), spectrum_tape(name, &scr)));
                files.push((format!("{name}.asm"), spectrum_source(name, Spectrum::Plain).into_bytes()));
            }
            Mode::Timex => {
                let scr = self.timex();
                files.push((format!("{name}.scr"), scr));
                files.push((format!("{name}.asm"), spectrum_source(name, Spectrum::HiColour).into_bytes()));
            }
            Mode::TimexHires => {
                files.push((format!("{name}.scr"), self.timex_hires()));
                files.push((format!("{name}.asm"), spectrum_source(name, Spectrum::HiRes).into_bytes()));
            }
            Mode::Layer2 | Mode::Layer2Wide | Mode::Layer2Hi | Mode::LoRes | Mode::Tiles | Mode::TilesWide | Mode::Sprites => {}
            Mode::Cpc0 | Mode::Cpc1 | Mode::Cpc2 => {
                files.push((format!("{name}.scr"), amsdos(name, &self.cpc_memory())));
                files.push((format!("{name}.bas"), self.cpc_loader(name).into_bytes()));
            }
            Mode::Msx2 => {
                files.push((format!("{name}.sc2"), self.msx_sc2()));
                files.push((format!("{name}.bas"), format!("10 COLOR 15,1,1:SCREEN 2\r\n20 BLOAD\"{}.SC2\",S\r\n30 GOTO 30\r\n", name.to_uppercase()).into_bytes()));
            }
            _ => {
                files.push((name.to_uppercase(), self.bbc_memory()));
                files.push((format!("{name}.bas"), self.bbc_loader(name).into_bytes()));
            }
        }
        let png = crate::pictures::to_png(&crate::pictures::Picture { width: self.width, height: self.height, rgba: self.rgba(), aspect: self.mode.aspect(), info: Vec::new(), cycling: None });
        if let Ok(png) = png {
            files.push((format!("{name}.png"), png));
        }
        files
    }

    /// Timex hi-colour: the bitmap, then a byte of colour for each byte of it, laid out as the bitmap is.
    fn timex(&self) -> Vec<u8> {
        let mut out = vec![0u8; 12288];
        for y in 0..192 {
            for cx in 0..32 {
                let at = crate::screen_designer::row_offset(y) + cx;
                let mut byte = 0u8;
                for x in 0..8 {
                    if self.values[y * 256 + cx * 8 + x] == 1 {
                        byte |= 0x80 >> x;
                    }
                }
                out[at] = byte;
                let (ink, paper) = self.cells[y * 32 + cx];
                out[6144 + at] = (ink & 7) | (paper & 7) << 3 | if ink >= 8 || paper >= 8 { 64 } else { 0 };
            }
        }
        out
    }

    /// Timex hi-res: the even bytes of each row as a bitmap at 16384, the odd at 24576 (each laid out as the
    /// Spectrum's), then the value for port 255 (6, and the INK in bits 3 to 5): 12289 bytes.
    pub fn timex_hires(&self) -> Vec<u8> {
        let mut out = vec![0u8; 12289];
        for y in 0..192 {
            for column in 0..64 {
                let mut byte = 0u8;
                for x in 0..8 {
                    if self.values[y * 512 + column * 8 + x] == 1 {
                        byte |= 0x80 >> x;
                    }
                }
                out[if column % 2 == 0 { 0 } else { 6144 } + crate::screen_designer::row_offset(y) + column / 2] = byte;
            }
        }
        out[12288] = 6 | (self.cells[0].0 & 7) << 3;
        out
    }

    /// The CPC's screen memory from &C000: a line every &800 bytes within a character row, rows 80 bytes apart.
    pub fn cpc_memory(&self) -> Vec<u8> {
        let mut out = vec![0u8; 16384];
        let per_byte = match self.mode {
            Mode::Cpc0 => 2,
            Mode::Cpc1 => 4,
            _ => 8,
        };
        for y in 0..200 {
            let line = (y / 8) * 80 + (y % 8) * 0x800;
            for b in 0..80 {
                let mut byte = 0u8;
                for p in 0..per_byte {
                    let ink = self.values[y * self.width + b * per_byte + p];
                    byte |= match self.mode {
                        // Bits 7, 3, 5, 1 for the first pixel's ink bits 0 to 3; 6, 2, 4, 0 for the second.
                        Mode::Cpc0 => {
                            let shift = 1 - p;
                            (ink & 1) << (6 + shift) | (ink >> 1 & 1) << (2 + shift) | (ink >> 2 & 1) << (4 + shift) | (ink >> 3 & 1) << shift
                        }
                        // Bits 7 to 4 the low bit of pixels 0 to 3, bits 3 to 0 the high bit.
                        Mode::Cpc1 => (ink & 1) << (7 - p) | (ink >> 1 & 1) << (3 - p),
                        _ => (ink & 1) << (7 - p),
                    };
                }
                out[line + b] = byte;
            }
        }
        out
    }

    fn cpc_loader(&self, name: &str) -> String {
        let mode = match self.mode {
            Mode::Cpc0 => 0,
            Mode::Cpc1 => 1,
            _ => 2,
        };
        let inks: Vec<String> = self.palette.iter().enumerate().map(|(i, c)| format!("INK {i},{c}")).collect();
        format!("10 MODE {mode}:BORDER {}:{}\r\n20 LOAD\"{}.SCR\",&C000\r\n30 CALL &BB18\r\n", self.palette.first().copied().unwrap_or(0), inks.join(":"), name.to_uppercase())
    }

    /// Screen 2's VRAM from 0 to &37FF, BSAVEd: the patterns, a name table in order, no sprites, and the colours.
    fn msx_sc2(&self) -> Vec<u8> {
        let mut vram = vec![0u8; 0x3800];
        for i in 0..768 {
            vram[0x1800 + i] = (i & 255) as u8;
        }
        vram[0x1B00] = 0xD0;
        for cy in 0..24 {
            for cx in 0..32 {
                for row in 0..8 {
                    let y = cy * 8 + row;
                    let at = (cy * 32 + cx) * 8 + row;
                    let mut byte = 0u8;
                    for x in 0..8 {
                        if self.values[y * 256 + cx * 8 + x] == 1 {
                            byte |= 0x80 >> x;
                        }
                    }
                    let (fore, back) = self.cells[y * 32 + cx];
                    vram[at] = byte;
                    vram[0x2000 + at] = fore << 4 | back;
                }
            }
        }
        let mut out = vec![0xFE, 0x00, 0x00, 0xFF, 0x37, 0x00, 0x00];
        out.extend(vram);
        out
    }

    /// The BBC's screen memory: character cells of 8 bytes, a byte a line, left to right, then the next row.
    pub fn bbc_memory(&self) -> Vec<u8> {
        let (per_byte, row_bytes) = match self.mode {
            Mode::Bbc0 => (8, 640),
            Mode::Bbc1 => (4, 640),
            Mode::Bbc2 => (2, 640),
            Mode::Bbc4 => (8, 320),
            _ => (4, 320),
        };
        let mut out = vec![0u8; row_bytes * 32];
        let bytes_across = self.width / per_byte;
        for y in 0..256 {
            for b in 0..bytes_across {
                let mut byte = 0u8;
                for p in 0..per_byte {
                    let c = self.values[y * self.width + b * per_byte + p];
                    byte |= match per_byte {
                        // Bits 7, 5, 3, 1 the first pixel's colour bits 3 to 0; 6, 4, 2, 0 the second's.
                        2 => {
                            let shift = 1 - p;
                            (c >> 3 & 1) << (6 + shift) | (c >> 2 & 1) << (4 + shift) | (c >> 1 & 1) << (2 + shift) | (c & 1) << shift
                        }
                        // Bits 7 to 4 the high bit of pixels 0 to 3, bits 3 to 0 the low bit.
                        4 => (c >> 1 & 1) << (7 - p) | (c & 1) << (3 - p),
                        _ => (c & 1) << (7 - p),
                    };
                }
                out[(y / 8) * row_bytes + b * 8 + y % 8] = byte;
            }
        }
        out
    }

    fn bbc_loader(&self, name: &str) -> String {
        let (mode, address) = match self.mode {
            Mode::Bbc0 => (0, "3000"),
            Mode::Bbc1 => (1, "3000"),
            Mode::Bbc2 => (2, "3000"),
            Mode::Bbc4 => (4, "5800"),
            _ => (5, "5800"),
        };
        let colours: Vec<String> = self.palette.iter().enumerate().map(|(i, c)| format!("VDU 19,{i},{c};0;")).collect();
        format!("10 MODE {mode}\r\n20 VDU 23,1,0;0;0;0;\r\n30 {}\r\n40 *LOAD {} {address}\r\n50 REPEAT UNTIL GET\r\n", colours.join(":"), name.to_uppercase())
    }
}

/// A tape that shows the screen: a BASIC program that LOADs it as SCREEN$ and waits, then the screen.
pub fn spectrum_tape(name: &str, scr: &[u8]) -> Vec<u8> {
    let short: String = name.chars().take(10).collect();
    let program = crate::basic::tokenise_spectrum(&["10 BORDER 0: LOAD \"\" SCREEN$ : PAUSE 0"], crate::basic::ZxDialect::Spectrum).unwrap_or_default();
    let mut tape = crate::basic::spectrum_tape(&program, &short, Some(10));
    let mut header = vec![3u8];
    let mut field = short.as_bytes().to_vec();
    field.resize(10, b' ');
    header.extend_from_slice(&field);
    header.extend_from_slice(&(scr.len() as u16).to_le_bytes());
    header.extend_from_slice(&16384u16.to_le_bytes());
    header.extend_from_slice(&32768u16.to_le_bytes());
    tape.extend(crate::basic::tap_block(0, &header));
    tape.extend(crate::basic::tap_block(0xFF, scr));
    tape
}

/// Which Spectrum screen a viewer's source shows.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Spectrum {
    Plain,
    HiColour,
    HiRes,
}

/// sjasmplus source that shows the screen: for the Timex modes, port 255 set to show it.
fn spectrum_source(name: &str, kind: Spectrum) -> String {
    let machine = match kind {
        Spectrum::Plain => "ZX Spectrum",
        Spectrum::HiColour => "Timex TC2048 or TS2068 (or the Next) in hi-colour",
        Spectrum::HiRes => "Timex TC2048 or TS2068 (or the Next) in hi-res",
    };
    let mut s = crate::i18n::tf!("; {name}: a picture for the {machine}, made by Limoniter.\n; Assemble with sjasmplus (or F9 in Limoniter), with {name}.scr beside it.\n\n");
    s += "        DEVICE ZXSPECTRUM48\n        org $8000\nstart:\n        ld hl,picture\n        ld de,$4000\n";
    match kind {
        Spectrum::Plain => s += "        ld bc,6912\n        ldir                    ; the bitmap and its colours\n        xor a\n        out ($fe),a             ; a black border\n",
        Spectrum::HiColour => s += "        ld bc,6144\n        ldir                    ; the bitmap\n        ld de,$6000\n        ld bc,6144\n        ldir                    ; its colours, a byte for each byte of it\n        ld a,2\n        out ($ff),a             ; hi-colour\n",
        Spectrum::HiRes => s += "        ld bc,6144\n        ldir                    ; the even bytes of each row\n        ld de,$6000\n        ld bc,6144\n        ldir                    ; the odd\n        ld a,(hl)               ; hi-res (6), and the INK in bits 3 to 5\n        out ($ff),a\n",
    }
    s += "wait:   jr wait\n\npicture:\n";
    s += &format!("        incbin \"{name}.scr\"\n\n        SAVESNA \"{name}.sna\",start\n");
    s
}

/// A file with AMSDOS's 128-byte header, as the CPC's disc saves a binary file, loading at &C000.
pub fn amsdos(name: &str, data: &[u8]) -> Vec<u8> {
    let mut header = vec![0u8; 128];
    let base: String = name.to_uppercase().chars().filter(|c| c.is_ascii_alphanumeric()).take(8).collect();
    let mut field = base.into_bytes();
    field.resize(8, b' ');
    header[1..9].copy_from_slice(&field);
    header[9..12].copy_from_slice(b"SCR");
    header[18] = 2;
    header[21..23].copy_from_slice(&0xC000u16.to_le_bytes());
    header[24..26].copy_from_slice(&(data.len() as u16).to_le_bytes());
    header[64..66].copy_from_slice(&(data.len() as u16).to_le_bytes());
    let checksum: u16 = header[..67].iter().map(|&b| u16::from(b)).sum();
    header[67..69].copy_from_slice(&checksum.to_le_bytes());
    let mut out = header;
    out.extend_from_slice(data);
    out
}

#[cfg(test)]
mod tests;
