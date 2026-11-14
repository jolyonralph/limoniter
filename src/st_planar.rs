//! Turning a picture into what an Atari ST, STE, Falcon or TT program shows it from.
//!
//! The picture is scaled, given a palette and each pixel its colour with the code shared with the Amiga converter
//! (`quantise`), then laid out as the machine's screen: bit planes interleaved a 16-bit word at a time (for each 16
//! pixels, a word of each plane in turn), or the Falcon's 16-bit true colour words. Low resolution is 320x200 in 16
//! colours (four planes), medium 640x200 in 4 (two), high 640x400 in black and white (one); the Falcon adds 256
//! colours (eight planes) and true colour, the TT 320x480 in 256. Spectrum 512 is in `st_spectrum`.
//!
//! A palette word is $0RGB. The ST has three bits a level (512 colours); the STE (and the Falcon and TT in their ST
//! modes) a fourth, kept as the level's lowest bit at the top of each nibble, so that an ST's words mean nearly the
//! same on it (4096 colours). The TT's own 256 colour palette has four bits a level in the plain order; the Falcon's
//! has six, a long word a colour: red in bits 26 to 31, green in 18 to 23, blue in 2 to 7.
//!
//! Out come the raw screen, the palette as words and C, Degas (.PI1-3, packed .PC1-3), NEOchrome, IFF ILBM and
//! assembler source; and the other way, raw planes ripped from a program are drawn as a picture.

use crate::amiga_planar as planar;
use crate::quantise::{self, Dither, Filter, Fit, Image, bayer, identifier};
use std::fmt::Write as _;

/// The machine, which decides the palette's precision and the screens there are.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Machine {
    St,
    Ste,
    Falcon,
    Tt,
}

impl Machine {
    pub const ALL: [Self; 4] = [Self::St, Self::Ste, Self::Falcon, Self::Tt];

    pub fn name(self) -> &'static str {
        match self {
            Self::St => "ST (512 colours)",
            Self::Ste => "STE (4096 colours)",
            Self::Falcon => "Falcon",
            Self::Tt => "TT",
        }
    }

    pub fn screens(self) -> &'static [Screen] {
        match self {
            Self::St | Self::Ste => &[Screen::Low, Screen::Medium, Screen::High, Screen::Spectrum],
            Self::Falcon => &[Screen::Low, Screen::Medium, Screen::High, Screen::Falcon256, Screen::FalconTrue],
            Self::Tt => &[Screen::Low, Screen::Medium, Screen::High, Screen::TtLow],
        }
    }

    /// The precision of the ST-compatible palette: only the plain ST has three bits a level.
    pub fn st_levels(self) -> Levels {
        if self == Self::St { Levels::St } else { Levels::Ste }
    }
}

/// The screen made.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Screen {
    Low,
    Medium,
    High,
    /// Spectrum 512 (Spectrum 4096 on the STE): 48 colours a line, the palette changed as the line is drawn.
    Spectrum,
    /// The Falcon's 256 colours, eight planes.
    Falcon256,
    /// The Falcon's 16-bit true colour: a word a pixel, 5 bits red, 6 green, 5 blue.
    FalconTrue,
    /// The TT's 320x480 in 256 colours.
    TtLow,
}

impl Screen {
    pub fn name(self, machine: Machine) -> &'static str {
        match self {
            Self::Low => "Low resolution, 16 colours",
            Self::Medium => "Medium resolution, 4 colours",
            Self::High => "High resolution, black and white",
            Self::Spectrum if machine == Machine::Ste => "Spectrum 4096 (48 colours a line)",
            Self::Spectrum => "Spectrum 512 (48 colours a line)",
            Self::Falcon256 => "256 colours (8 planes)",
            Self::FalconTrue => "True colour (16-bit)",
            Self::TtLow => "TT low, 256 colours",
        }
    }

    /// The screen's own size.
    pub fn size(self) -> (usize, usize) {
        match self {
            Self::Low | Self::Falcon256 | Self::FalconTrue => (320, 200),
            Self::Medium => (640, 200),
            Self::High => (640, 400),
            Self::Spectrum => (320, 199),
            Self::TtLow => (320, 480),
        }
    }

    /// Bit planes; 16 is the Falcon's true colour word.
    pub fn planes(self) -> usize {
        match self {
            Self::Low | Self::Spectrum => 4,
            Self::Medium => 2,
            Self::High => 1,
            Self::Falcon256 | Self::TtLow => 8,
            Self::FalconTrue => 16,
        }
    }

    /// A pixel's width for its height.
    pub fn aspect(self) -> f32 {
        match self {
            Self::Medium => 0.5,
            Self::TtLow => 2.0,
            _ => 1.0,
        }
    }

    /// The resolution word Degas and NEOchrome keep, for the ST's own three.
    pub fn st_resolution(self) -> Option<u16> {
        match self {
            Self::Low => Some(0),
            Self::Medium => Some(1),
            Self::High => Some(2),
            _ => None,
        }
    }
}

/// How finely the palette holds a level.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Levels {
    /// Three bits, the ST's.
    St,
    /// Four bits in the STE's order.
    Ste,
    /// Four bits in the plain order, the TT's 256-colour palette.
    Tt,
    /// Six bits, the Falcon's.
    Falcon,
}

impl Levels {
    /// The nearest level the palette holds, as the screen shows it.
    pub fn snap(self, level: f32) -> u8 {
        let v = level.round().clamp(0.0, 255.0);
        match self {
            Self::St => level3((v * 7.0 / 255.0).round() as u8),
            Self::Ste | Self::Tt => ((v / 17.0).round() as u8) * 17,
            Self::Falcon => level6((v * 63.0 / 255.0).round() as u8),
        }
    }

    pub fn snap_colour(self, c: [f32; 3]) -> [u8; 3] {
        c.map(|v| self.snap(v))
    }

    /// How many steps a level has.
    pub fn steps(self) -> usize {
        match self {
            Self::St => 8,
            Self::Ste | Self::Tt => 16,
            Self::Falcon => 64,
        }
    }

    /// Every level there is, darkest first.
    pub fn all(self) -> Vec<u8> {
        (0..self.steps()).map(|n| self.snap(n as f32 * 255.0 / (self.steps() - 1) as f32)).collect()
    }
}

/// A three-bit level spread to eight bits, as the ST's three bits are shown.
pub fn level3(n: u8) -> u8 {
    ((u16::from(n & 7) * 255 + 3) / 7) as u8
}

fn level6(n: u8) -> u8 {
    (n << 2) | (n >> 4)
}

/// A colour as an ST palette word: three bits a level, or with the STE's fourth bit at the top of each nibble.
pub fn st_word(c: [u8; 3], levels: Levels) -> u16 {
    let nibble = |v: u8| -> u16 {
        match levels {
            Levels::St => u16::from((f32::from(v) * 7.0 / 255.0).round() as u8),
            Levels::Tt => u16::from(v / 17),
            _ => {
                let n = u16::from(v / 17);
                (n >> 1) | (n & 1) << 3
            }
        }
    };
    nibble(c[0]) << 8 | nibble(c[1]) << 4 | nibble(c[2])
}

/// A palette word as a colour. With `ste`, the fourth bit is read in the STE's order; otherwise a plain ST's.
pub fn colour_of_word(w: u16, ste: bool) -> [u8; 3] {
    let level = |n: u16| -> u8 { if ste { ((((n & 7) << 1) | (n >> 3) & 1) * 17) as u8 } else { level3((n & 7) as u8) } };
    [level(w >> 8 & 15), level(w >> 4 & 15), level(w & 15)]
}

/// A colour as the Falcon's palette register: RRRRRRxx GGGGGGxx 00000000 BBBBBBxx.
pub fn falcon_long(c: [u8; 3]) -> u32 {
    let six = |v: u8| u32::from(v >> 2);
    six(c[0]) << 26 | six(c[1]) << 18 | six(c[2]) << 2
}

/// A colour as the Falcon's true colour word: RRRRRGGGGGGBBBBB.
pub fn rgb565(c: [u8; 3]) -> u16 {
    u16::from(c[0] >> 3) << 11 | u16::from(c[1] >> 2) << 5 | u16::from(c[2] >> 3)
}

pub fn colour_of_565(w: u16) -> [u8; 3] {
    let (r, g, b) = ((w >> 11) as u8 & 31, (w >> 5) as u8 & 63, w as u8 & 31);
    [(r << 3) | (r >> 2), (g << 2) | (g >> 4), (b << 3) | (b >> 2)]
}

/// Everything the conversion is asked to do.
#[derive(Clone, Debug, PartialEq)]
pub struct Settings {
    pub machine: Machine,
    pub screen: Screen,
    pub width: usize,
    pub height: usize,
    pub fit: Fit,
    pub filter: Filter,
    pub dither: Dither,
    pub strength: f32,
    pub black_background: bool,
    pub given: Vec<[u8; 3]>,
    pub keep_given: usize,
}

impl Default for Settings {
    fn default() -> Self {
        Settings { machine: Machine::St, screen: Screen::Low, width: 320, height: 200, fit: Fit::Fill, filter: Filter::Lanczos, dither: Dither::FloydSteinberg, strength: 0.8, black_background: true, given: Vec::new(), keep_given: 0 }
    }
}

impl Settings {
    /// The settings made consistent: a screen the machine has, Spectrum's fixed size.
    pub fn settled(&self) -> Settings {
        let mut s = self.clone();
        if !s.machine.screens().contains(&s.screen) {
            s.screen = Screen::Low;
        }
        s.width = s.width.clamp(1, 2048);
        s.height = s.height.clamp(1, 2048);
        if s.screen == Screen::Spectrum {
            (s.width, s.height) = Screen::Spectrum.size();
        }
        s.strength = s.strength.clamp(0.0, 1.0);
        s
    }

    /// The palette's precision for the screen.
    pub fn levels(&self) -> Levels {
        match self.screen {
            Screen::Falcon256 => Levels::Falcon,
            Screen::TtLow => Levels::Tt,
            _ => self.machine.st_levels(),
        }
    }
}

/// A converted picture.
#[derive(Clone, Debug, PartialEq)]
pub struct Converted {
    pub machine: Machine,
    pub screen: Screen,
    pub levels: Levels,
    pub width: usize,
    pub height: usize,
    /// The colours (each level as the screen shows it); for Spectrum the first line's, for true colour none.
    pub palette: Vec<[u8; 3]>,
    /// Each pixel's colour number (for Spectrum, 0 to 15 in the line's palettes); for true colour, none.
    pub values: Vec<u8>,
    /// Spectrum's 48 colours for each line: three palettes of 16, used as `st_spectrum::slot` says.
    pub lines: Vec<[[u8; 3]; 48]>,
    /// The Falcon's true colour words.
    pub words: Vec<u16>,
    pub rgba: Vec<u8>,
    /// The average difference from the scaled picture, in Oklab.
    pub error: f32,
}

impl Converted {
    pub fn planes(&self) -> usize {
        self.screen.planes()
    }

    /// Whether this is the whole of an ST screen, as Degas and NEOchrome keep.
    pub fn is_st_screen(&self) -> bool {
        self.screen.st_resolution().is_some() && (self.width, self.height) == self.screen.size()
    }

    /// The palette words for the ST-compatible palette.
    pub fn st_words(&self) -> Vec<u16> {
        self.palette.iter().map(|&c| st_word(c, self.levels)).collect()
    }
}

/// The whole conversion.
pub fn convert(image: &Image, settings: &Settings) -> Converted {
    let s = settings.settled();
    let levels = s.levels();
    let pixels = quantise::scale(image, s.width, s.height, s.screen.aspect(), s.fit, s.filter);
    let mut c = Converted { machine: s.machine, screen: s.screen, levels, width: s.width, height: s.height, palette: Vec::new(), values: Vec::new(), lines: Vec::new(), words: Vec::new(), rgba: Vec::new(), error: 0.0 };
    match s.screen {
        Screen::Spectrum => {
            let (values, lines) = crate::st_spectrum::encode(&pixels, levels, s.dither, s.strength);
            c.rgba = crate::st_spectrum::render(&values, &lines);
            c.palette = lines.first().map(|l| l[..16].to_vec()).unwrap_or_default();
            c.values = values;
            c.lines = lines;
        }
        Screen::FalconTrue => {
            c.words = true_colour(&pixels, s.width, s.height, s.dither, s.strength);
            c.rgba = c.words.iter().flat_map(|&w| {
                let [r, g, b] = colour_of_565(w);
                [r, g, b, 255]
            }).collect();
        }
        Screen::High => {
            // The monochrome monitor: 0 white, 1 black.
            c.palette = vec![[255, 255, 255], [0, 0, 0]];
            c.values = quantise::choose(&pixels, s.width, s.height, &c.palette, s.dither, s.strength);
            c.rgba = render(&c.values, &c.palette);
        }
        _ => {
            let count = 1usize << s.screen.planes();
            let mut slots = vec![None; count];
            for (slot, colour) in slots.iter_mut().zip(s.given.iter().take(s.keep_given)) {
                *slot = Some(colour.map(|v| levels.snap(f32::from(v))));
            }
            if s.black_background && slots[0].is_none() {
                slots[0] = Some([0, 0, 0]);
            }
            let snap = |c: [f32; 3]| levels.snap_colour(c);
            c.palette = quantise::find_palette(&pixels, &slots, &snap, None);
            c.values = quantise::choose(&pixels, s.width, s.height, &c.palette, s.dither, s.strength);
            c.rgba = render(&c.values, &c.palette);
        }
    }
    c.error = quantise::average_error(&pixels, &c.rgba);
    c
}

/// Pixels' colour numbers drawn in a palette.
pub fn render(values: &[u8], palette: &[[u8; 3]]) -> Vec<u8> {
    values.iter().flat_map(|&v| {
        let [r, g, b] = palette.get(usize::from(v)).copied().unwrap_or([0, 0, 0]);
        [r, g, b, 255]
    }).collect()
}

/// The Falcon's true colour, dithered if asked: each level to five bits (six for green).
fn true_colour(pixels: &[[f32; 3]], width: usize, height: usize, dither: Dither, strength: f32) -> Vec<u16> {
    let snap = |p: [f32; 3]| -> [u8; 3] {
        let five = |v: f32| ((v.clamp(0.0, 255.0) * 31.0 / 255.0).round() as u8).min(31);
        let six = |v: f32| ((v.clamp(0.0, 255.0) * 63.0 / 255.0).round() as u8).min(63);
        let (r, g, b) = (five(p[0]), six(p[1]), five(p[2]));
        [(r << 3) | (r >> 2), (g << 2) | (g >> 4), (b << 3) | (b >> 2)]
    };
    let mut out = vec![0u16; width * height];
    let mut work = pixels.to_vec();
    for y in 0..height {
        for x in 0..width {
            let mut p = work[y * width + x];
            if dither == Dither::Ordered {
                let offset = ((bayer(x, y) + 0.5) / 64.0 - 0.5) * strength * 8.0;
                p = p.map(|v| v + offset);
            }
            let chosen = snap(p);
            out[y * width + x] = rgb565(chosen);
            if dither == Dither::FloydSteinberg {
                let p = p.map(|v| v.clamp(0.0, 255.0));
                let error: [f32; 3] = std::array::from_fn(|c| (p[c] - f32::from(chosen[c])) * strength);
                quantise::diffuse(&mut work, width, height, x, y, false, error);
            }
        }
    }
    out
}

// The screen's layout.

/// Bytes in a row: the width rounded up to 16 pixels, a word of each plane for each 16.
pub fn row_bytes(width: usize, planes: usize) -> usize {
    width.div_ceil(16) * 2 * planes
}

/// Pixels' values as the ST lays planes out: for each 16 pixels, a word of each plane, plane 0 (the lowest bit)
/// first. Bits past the width are 0.
pub fn to_interleaved(values: &[u8], width: usize, height: usize, planes: usize) -> Vec<u8> {
    let row = row_bytes(width, planes);
    let mut out = vec![0u8; row * height];
    for y in 0..height {
        for x in 0..width {
            let v = values[y * width + x];
            let at = y * row + (x / 16) * planes * 2 + (x % 16) / 8;
            for plane in 0..planes {
                if v >> plane & 1 != 0 {
                    out[at + plane * 2] |= 0x80 >> (x % 8);
                }
            }
        }
    }
    out
}

/// The ST's interleaved planes back to values; what isn't there is 0.
pub fn from_interleaved(data: &[u8], width: usize, height: usize, planes: usize) -> Vec<u8> {
    let row = row_bytes(width, planes);
    let mut values = vec![0u8; width * height];
    for y in 0..height {
        for x in 0..width {
            let at = y * row + (x / 16) * planes * 2 + (x % 16) / 8;
            let mut v = 0u8;
            for plane in 0..planes {
                if data.get(at + plane * 2).is_some_and(|b| b & (0x80 >> (x % 8)) != 0) {
                    v |= 1 << plane;
                }
            }
            values[y * width + x] = v;
        }
    }
    values
}

/// The screen as the program shows it: interleaved planes, or the Falcon's true colour words. Spectrum's is the
/// 32000-byte screen with its first line empty, as the display routine expects.
pub fn screen_bytes(c: &Converted) -> Vec<u8> {
    match c.screen {
        Screen::FalconTrue => c.words.iter().flat_map(|w| w.to_be_bytes()).collect(),
        Screen::Spectrum => {
            let mut out = vec![0u8; 160];
            out.extend(to_interleaved(&c.values, 320, 199, 4));
            out
        }
        _ => to_interleaved(&c.values, c.width, c.height, c.planes()),
    }
}

// The palette written out.

/// The palette as raw big-endian words (the ST's, STE's or TT's), or the Falcon's longs.
pub fn palette_raw(c: &Converted) -> Vec<u8> {
    match c.levels {
        Levels::Falcon => c.palette.iter().flat_map(|&p| falcon_long(p).to_be_bytes()).collect(),
        _ => palette_words(c).iter().flat_map(|w| w.to_be_bytes()).collect(),
    }
}

/// The ST palette's 16 words (4 used in medium resolution; high resolution's white and black).
fn palette_words(c: &Converted) -> Vec<u16> {
    let mut words = c.st_words();
    if c.screen == Screen::High {
        words = vec![0x777, 0x000];
    }
    if words.len() < 16 && c.levels != Levels::Falcon && c.levels != Levels::Tt {
        words.resize(16, 0);
    }
    words
}

/// The palette as `dc.w` (or the Falcon's `dc.l`) lines, eight to a line.
pub fn palette_asm(c: &Converted, name: &str) -> String {
    let mut out = String::new();
    let id = identifier(name);
    let what = match c.levels {
        Levels::St => "ST palette words, $0RGB with three bits a level",
        Levels::Ste => "STE palette words, $0RGB with each level's lowest bit at the top of its nibble",
        Levels::Tt => "TT palette words, $0RGB with four bits a level",
        Levels::Falcon => "Falcon palette registers, RRRRRRxx GGGGGGxx 00000000 BBBBBBxx",
    };
    let _ = writeln!(out, "; {} colours: {what}", c.palette.len().max(palette_words(c).len().min(16)));
    let _ = writeln!(out, "{id}_palette:");
    match c.levels {
        Levels::Falcon => {
            for chunk in c.palette.chunks(4) {
                let items: Vec<String> = chunk.iter().map(|&p| format!("${:08x}", falcon_long(p))).collect();
                let _ = writeln!(out, "        dc.l    {}", items.join(","));
            }
        }
        _ => {
            for chunk in palette_words(c).chunks(8) {
                let items: Vec<String> = chunk.iter().map(|w| format!("${w:04x}")).collect();
                let _ = writeln!(out, "        dc.w    {}", items.join(","));
            }
        }
    }
    out
}

/// The palette as a C array.
pub fn palette_c(c: &Converted, name: &str) -> String {
    let id = identifier(name);
    let mut out = String::new();
    let rows = |items: Vec<String>| items.chunks(8).map(|c| format!("    {}", c.join(", "))).collect::<Vec<_>>().join(",\n");
    match c.levels {
        Levels::Falcon => {
            let _ = writeln!(out, "/* {} colours, as the Falcon's palette registers */", c.palette.len());
            let _ = writeln!(out, "unsigned long {id}_palette[{}] = {{\n{}\n}};", c.palette.len(), rows(c.palette.iter().map(|&p| format!("0x{:08X}UL", falcon_long(p))).collect()));
        }
        _ => {
            let words = palette_words(c);
            let kind = match c.levels {
                Levels::St => "ST",
                Levels::Ste => "STE",
                _ => "TT",
            };
            let _ = writeln!(out, "/* {} colours, as {kind} palette words: 0x0RGB */", words.len());
            let _ = writeln!(out, "unsigned short {id}_palette[{}] = {{\n{}\n}};", words.len(), rows(words.iter().map(|w| format!("0x{w:04X}")).collect()));
        }
    }
    out
}

// Files.

/// A Degas picture (.PI1-3), or Degas Elite's packed one (.PC1-3) with its colour cycling switched off.
pub fn to_degas(c: &Converted, packed: bool) -> Option<Vec<u8>> {
    let resolution = c.screen.st_resolution()?;
    if !c.is_st_screen() {
        return None;
    }
    let mut out = (resolution | if packed { 0x8000 } else { 0 }).to_be_bytes().to_vec();
    let words = palette_words(c);
    for i in 0..16 {
        out.extend(words.get(i).copied().unwrap_or(0).to_be_bytes());
    }
    let screen = screen_bytes(c);
    if !packed {
        out.extend(screen);
        return Some(out);
    }
    // Each row of each plane in turn, each packed on its own.
    let planes = c.planes();
    let plane_row = c.width / 8;
    let row = row_bytes(c.width, planes);
    for y in 0..c.height {
        for plane in 0..planes {
            let bytes: Vec<u8> = (0..plane_row).map(|byte| screen[y * row + (byte / 2) * planes * 2 + plane * 2 + byte % 2]).collect();
            planar::pack_byterun1(&bytes, &mut out);
        }
    }
    // Colour cycling: four ranges, all off (direction 1).
    out.extend([0u8; 16]);
    for _ in 0..4 {
        out.extend(1u16.to_be_bytes());
    }
    out.extend([0u8; 8]);
    Some(out)
}

/// A NEOchrome picture.
pub fn to_neo(c: &Converted) -> Option<Vec<u8>> {
    let resolution = c.screen.st_resolution()?;
    if !c.is_st_screen() {
        return None;
    }
    let mut out = vec![0, 0];
    out.extend(resolution.to_be_bytes());
    let words = palette_words(c);
    for i in 0..16 {
        out.extend(words.get(i).copied().unwrap_or(0).to_be_bytes());
    }
    out.extend(b"        .   ");
    out.resize(58, 0);
    // Width and height, as NEOchrome writes them.
    out.extend(320u16.to_be_bytes());
    out.extend(200u16.to_be_bytes());
    out.resize(128, 0);
    out.extend(screen_bytes(c));
    Some(out)
}

/// An IFF ILBM of an indexed picture, through the Amiga converter's writer: its palette as 8-bit levels.
pub fn to_ilbm(c: &Converted, packed: bool) -> Option<Vec<u8>> {
    if matches!(c.screen, Screen::Spectrum | Screen::FalconTrue) {
        return None;
    }
    let (resolution, interlaced) = match c.screen {
        Screen::Medium => (planar::Resolution::Hires, false),
        Screen::High => (planar::Resolution::Hires, true),
        _ => (planar::Resolution::Lores, false),
    };
    let palette = if c.screen == Screen::High { vec![[255, 255, 255], [0, 0, 0]] } else { c.palette.clone() };
    let amiga = planar::Converted { width: c.width, height: c.height, planes: c.planes(), mode: planar::Mode::Plain, chipset: planar::Chipset::Aga, resolution, interlaced, palette, values: c.values.clone(), rgba: Vec::new(), error: 0.0 };
    Some(planar::to_ilbm(&amiga, packed))
}

/// Assembler source that shows the picture: its sizes, a routine setting the screen and palette through the XBIOS,
/// the palette, and INCBIN of the screen.
pub fn asm_source(c: &Converted, name: &str, screen_file: &str) -> String {
    let id = identifier(name);
    let mut out = String::new();
    let planes = c.planes();
    let row = if c.screen == Screen::FalconTrue { c.width * 2 } else { row_bytes(c.width, planes) };
    let size = screen_bytes(c).len();
    let _ = writeln!(out, "; {} by {}, {} on the {}.", c.width, c.height, c.screen.name(c.machine).to_lowercase(), match c.machine {
        Machine::St => "ST",
        Machine::Ste => "STE",
        Machine::Falcon => "Falcon",
        Machine::Tt => "TT",
    });
    if c.screen != Screen::FalconTrue {
        let _ = writeln!(out, "; The planes are interleaved: for each 16 pixels, a word of each plane.");
    }
    let _ = writeln!(out);
    let _ = writeln!(out, "{id}_WIDTH     equ {}", c.width);
    let _ = writeln!(out, "{id}_HEIGHT    equ {}", c.height);
    let _ = writeln!(out, "{id}_PLANES    equ {planes}");
    let _ = writeln!(out, "{id}_ROWBYTES  equ {row}    ; one row of every plane");
    let _ = writeln!(out, "{id}_SIZE      equ {size}");
    let _ = writeln!(out);
    let whole = (c.width, c.height) == c.screen.size() || c.screen == Screen::Spectrum;
    if c.screen == Screen::Spectrum {
        let _ = writeln!(out, "; Spectrum: the screen's first line is empty; then 199 lines of 48 palette words, three palettes of 16");
        let _ = writeln!(out, "; a line. Colour c of a line shows from the first palette until pixel 10c+1 (10c-5 when c is odd),");
        let _ = writeln!(out, "; from the second for 160 pixels after that, then from the third. A display routine timed to the");
        let _ = writeln!(out, "; cycle writes them to $ffff8240 as each line is drawn.");
        let _ = writeln!(out);
    } else if whole {
        let _ = writeln!(out, "; Shows the picture; call it from user mode. The screen's address goes to the shifter, which on");
        let _ = writeln!(out, "; a plain ST must be a multiple of 256.");
        let _ = writeln!(out, "{id}_show:");
        match c.screen {
            Screen::Low | Screen::Medium | Screen::High | Screen::TtLow => {
                let rez = match c.screen {
                    Screen::Low => 0,
                    Screen::Medium => 1,
                    Screen::High => 2,
                    _ => 7,
                };
                let _ = writeln!(out, "        move.w  #{rez},-(sp)          ; resolution");
                let _ = writeln!(out, "        pea     {id}_screen          ; physical screen");
                let _ = writeln!(out, "        pea     {id}_screen          ; logical screen");
                let _ = writeln!(out, "        move.w  #5,-(sp)            ; Setscreen");
                let _ = writeln!(out, "        trap    #14");
                let _ = writeln!(out, "        lea     12(sp),sp");
                if c.screen == Screen::TtLow {
                    let _ = writeln!(out, "        pea     {id}_palette");
                    let _ = writeln!(out, "        move.w  #256,-(sp)          ; colours");
                    let _ = writeln!(out, "        clr.w   -(sp)               ; from colour 0");
                    let _ = writeln!(out, "        move.w  #84,-(sp)           ; EsetPalette");
                    let _ = writeln!(out, "        trap    #14");
                    let _ = writeln!(out, "        lea     10(sp),sp");
                } else {
                    let _ = writeln!(out, "        pea     {id}_palette");
                    let _ = writeln!(out, "        move.w  #6,-(sp)            ; Setpalette (from the next vertical blank)");
                    let _ = writeln!(out, "        trap    #14");
                    let _ = writeln!(out, "        addq.l  #6,sp");
                }
            }
            Screen::Falcon256 | Screen::FalconTrue => {
                let mode = if c.screen == Screen::Falcon256 { 0x0023 } else { 0x0024 };
                let _ = writeln!(out, "; VsetMode ${mode:04x}: {}, 40 columns, PAL on an RGB monitor or TV. On VGA use ${:04x} (line doubled).", if mode == 0x23 { "8 planes" } else { "true colour" }, mode & 7 | 0x110);
                let _ = writeln!(out, "        move.w  #${mode:04x},-(sp)        ; mode");
                let _ = writeln!(out, "        move.w  #3,-(sp)            ; 3: the mode is given");
                let _ = writeln!(out, "        pea     {id}_screen");
                let _ = writeln!(out, "        pea     {id}_screen");
                let _ = writeln!(out, "        move.w  #5,-(sp)            ; Setscreen");
                let _ = writeln!(out, "        trap    #14");
                let _ = writeln!(out, "        lea     14(sp),sp");
                if c.screen == Screen::Falcon256 {
                    let _ = writeln!(out, "        pea     {id}_rgb");
                    let _ = writeln!(out, "        move.w  #256,-(sp)          ; colours");
                    let _ = writeln!(out, "        clr.w   -(sp)               ; from colour 0");
                    let _ = writeln!(out, "        move.w  #93,-(sp)           ; VsetRGB");
                    let _ = writeln!(out, "        trap    #14");
                    let _ = writeln!(out, "        lea     10(sp),sp");
                }
            }
            Screen::Spectrum => {}
        }
        let _ = writeln!(out, "        rts");
        let _ = writeln!(out);
    }
    let _ = writeln!(out, "        section data");
    if c.screen != Screen::FalconTrue {
        out.push_str(&palette_asm(c, name));
    }
    if c.screen == Screen::Falcon256 {
        let _ = writeln!(out, "; The same colours as VsetRGB takes them: $00RRGGBB.");
        let _ = writeln!(out, "{id}_rgb:");
        for chunk in c.palette.chunks(4) {
            let items: Vec<String> = chunk.iter().map(|p| format!("${:02x}{:02x}{:02x}", p[0], p[1], p[2])).collect();
            let _ = writeln!(out, "        dc.l    {}", items.join(","));
        }
    }
    if c.screen == Screen::Spectrum {
        let _ = writeln!(out, "        even");
        let _ = writeln!(out, "{id}_screen:");
        let _ = writeln!(out, "        incbin  \"{screen_file}\"    ; {} bytes: the screen, then the palettes", size + 199 * 96);
        return out;
    }
    let _ = writeln!(out, "        cnop    0,256");
    let _ = writeln!(out, "{id}_screen:");
    let _ = writeln!(out, "        incbin  \"{screen_file}\"    ; {size} bytes");
    out
}

// Bringing palettes in.

/// A palette from a file: a Degas or NEOchrome picture's, ST palette words in assembler or C ($0RGB, read with
/// the STE's fourth bit when any colour has it) or raw, or anything the Amiga converter reads.
pub fn read_palette(bytes: &[u8]) -> Option<Vec<[u8; 3]>> {
    let words_at = |at: usize| -> Vec<u16> { (0..16).map(|i| u16::from_be_bytes([bytes[at + i * 2], bytes[at + i * 2 + 1]])).collect() };
    let from_words = |words: Vec<u16>| -> Vec<[u8; 3]> {
        let ste = words.iter().any(|w| w & 0x888 != 0);
        words.into_iter().map(|w| colour_of_word(w, ste)).collect()
    };
    match bytes.len() {
        32034 | 32066 if bytes[0] == 0 && bytes[1] <= 2 => return Some(from_words(words_at(2))),
        32128 if bytes[0] == 0 && bytes[1] == 0 => return Some(from_words(words_at(4))),
        _ => {}
    }
    if let Ok(text) = std::str::from_utf8(bytes)
        && text.chars().all(|c| !c.is_control() || c.is_ascii_whitespace())
        && !text.starts_with("JASC-PAL")
        && !text.starts_with("GIMP Palette")
    {
        let mut words = Vec::new();
        for line in text.lines() {
            let code = line.split([';', '*']).next().unwrap_or_default();
            let code = code.split("//").next().unwrap_or_default();
            for word in code.split(|c: char| !(c.is_ascii_alphanumeric() || c == '$')) {
                let Some(digits) = word.strip_prefix('$').or_else(|| word.strip_prefix("0x")).or_else(|| word.strip_prefix("0X")) else {
                    continue;
                };
                if digits.len() <= 4
                    && let Ok(value) = u16::from_str_radix(digits, 16)
                    && value <= 0xFFF
                {
                    words.push(value);
                }
            }
        }
        if !words.is_empty() {
            return Some(from_words(words));
        }
    }
    if bytes.len().is_multiple_of(2) && !bytes.is_empty() && bytes.len() <= 512 && bytes.chunks(2).all(|w| w[0] < 0x10) {
        return Some(from_words(bytes.chunks(2).map(|w| u16::from_be_bytes([w[0], w[1]])).collect()));
    }
    planar::read_palette(bytes)
}

// Raw planes shown.

/// Raw ST planes drawn as a picture, for checking planes ripped from a program.
#[derive(Clone, Debug, PartialEq)]
pub struct RawView {
    pub offset: usize,
    pub width: usize,
    pub height: usize,
    /// 1, 2, 4 or 8 interleaved planes, or 16 for the Falcon's true colour.
    pub planes: usize,
    /// Where 16 palette words are in the file, if they are.
    pub palette_at: Option<usize>,
}

impl Default for RawView {
    fn default() -> Self {
        RawView { offset: 0, width: 320, height: 200, planes: 4, palette_at: None }
    }
}

impl RawView {
    pub fn row(&self) -> usize {
        if self.planes >= 16 { self.width.max(1) * 2 } else { row_bytes(self.width.max(1), self.planes.clamp(1, 8)) }
    }

    /// The rows that fit in the file after the offset.
    pub fn rows_in(&self, length: usize) -> usize {
        length.saturating_sub(self.offset) / self.row()
    }

    /// The palette words in the file, if it's said where they are.
    pub fn file_palette(&self, bytes: &[u8]) -> Option<Vec<[u8; 3]>> {
        let at = self.palette_at?;
        let data = bytes.get(at..at + 32)?;
        let words: Vec<u16> = data.chunks(2).map(|w| u16::from_be_bytes([w[0], w[1]])).collect();
        let ste = words.iter().any(|w| w & 0x888 != 0);
        Some(words.into_iter().map(|w| colour_of_word(w, ste)).collect())
    }

    /// The picture's colours, with a palette (shades of grey where it runs out).
    pub fn draw(&self, bytes: &[u8], palette: &[[u8; 3]]) -> Vec<u8> {
        let (width, height) = (self.width.clamp(1, 4096), self.height.clamp(1, 4096));
        let data = bytes.get(self.offset..).unwrap_or_default();
        if self.planes >= 16 {
            return (0..width * height)
                .flat_map(|i| {
                    let w = data.get(i * 2..i * 2 + 2).map_or(0, |w| u16::from_be_bytes([w[0], w[1]]));
                    let [r, g, b] = colour_of_565(w);
                    [r, g, b, 255]
                })
                .collect();
        }
        let planes = self.planes.clamp(1, 8);
        let values = from_interleaved(data, width, height, planes);
        let registers = 1usize << planes;
        let mut colours: Vec<[u8; 3]> = palette.iter().copied().take(registers).collect();
        let have = colours.len();
        colours.extend((have..registers).map(|i| [(i * 255 / (registers - 1).max(1)) as u8; 3]));
        render(&values, &colours)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Smooth gradients and a few hard-edged shapes.
    pub fn test_card(width: usize, height: usize) -> Image {
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

    fn settings(machine: Machine, screen: Screen, dither: Dither) -> Settings {
        let (width, height) = screen.size();
        Settings { machine, screen, width, height, dither, fit: Fit::Stretch, filter: Filter::Box, ..Settings::default() }
    }

    #[test]
    fn planes_interleave_a_word_at_a_time() {
        let mut values = vec![0u8; 32];
        values[0] = 1;
        values[1] = 2;
        values[16] = 15;
        let data = to_interleaved(&values, 32, 1, 4);
        assert_eq!(data, [0x80, 0, 0x40, 0, 0, 0, 0, 0, 0x80, 0, 0x80, 0, 0x80, 0, 0x80, 0]);
        for planes in [1, 2, 4, 8] {
            let (w, h) = (37, 5);
            let values: Vec<u8> = (0..w * h).map(|i| ((i * 7 + i / 5) % (1 << planes)) as u8).collect();
            let data = to_interleaved(&values, w, h, planes);
            assert_eq!(data.len(), 3 * 2 * planes * h);
            assert_eq!(from_interleaved(&data, w, h, planes), values);
        }
    }

    #[test]
    fn palette_words_keep_the_stes_fourth_bit_at_the_top() {
        assert_eq!(st_word([255, 238, 17], Levels::Ste), 0xF78);
        assert_eq!(st_word([255, 238, 17], Levels::Tt), 0xFE1);
        assert_eq!(st_word([255, 36, 0], Levels::St), 0x710);
        for w in 0..0x1000u16 {
            assert_eq!(st_word(colour_of_word(w, true), Levels::Ste), w);
            if w & 0x888 == 0 {
                assert_eq!(st_word(colour_of_word(w, false), Levels::St), w);
            }
        }
        assert_eq!(Levels::St.all(), [0, 36, 73, 109, 146, 182, 219, 255]);
        assert_eq!(falcon_long([255, 128, 4]), 0xFC80_0004);
        assert_eq!(colour_of_565(rgb565([255, 130, 8])), [255, 130, 8]);
    }

    /// Writes Degas (plain and packed) and NEOchrome, reads them back with Limoniter's own reader, and compares
    /// every pixel. Returns the pixels compared.
    fn st_files_round_trip(c: &Converted) -> usize {
        let resolution = c.screen.st_resolution().unwrap();
        // An STE palette without the fourth bit anywhere is read as an ST one, as every reader does.
        let ste_unseen = c.levels == Levels::Ste && c.st_words().iter().all(|w| w & 0x888 == 0) && c.screen != Screen::High;
        let mut compared = 0;
        for (name, file) in [(format!("t.pi{}", resolution + 1), to_degas(c, false).unwrap()), (format!("t.pc{}", resolution + 1), to_degas(c, true).unwrap()), ("t.neo".to_string(), to_neo(c).unwrap())] {
            let format = crate::pictures::detect(&name, &file).unwrap();
            let picture = crate::pictures::decode_named(format, &name, &file).unwrap();
            assert_eq!((picture.width, picture.height), (c.width, c.height));
            if !ste_unseen {
                assert_eq!(picture.rgba, c.rgba, "{:?} {:?} {name}", c.machine, c.screen);
                compared += c.width * c.height;
            }
            // The raw view shows the same, given where the palette is.
            let at = if name.ends_with("neo") { 128 } else { 34 };
            if !name.contains(".pc") && c.screen != Screen::High && !ste_unseen {
                let view = RawView { offset: at, width: c.width, height: c.height, planes: c.planes(), palette_at: Some(if at == 128 { 4 } else { 2 }) };
                assert_eq!(view.draw(&file, &view.file_palette(&file).unwrap()), c.rgba);
            }
        }
        assert_eq!(to_degas(c, false).unwrap().len(), 32034);
        assert_eq!(to_neo(c).unwrap().len(), 32128);
        compared
    }

    #[test]
    fn every_st_screen_goes_through_degas_and_neochrome_unchanged() {
        let mut compared = 0;
        for machine in [Machine::St, Machine::Ste, Machine::Falcon] {
            for screen in [Screen::Low, Screen::Medium, Screen::High] {
                for dither in Dither::ALL {
                    let (w, h) = screen.size();
                    let c = convert(&test_card(w / 2, h / 2), &settings(machine, screen, dither));
                    compared += st_files_round_trip(&c);
                }
            }
        }
        assert!(compared > 20 * 64000, "{compared}");
    }

    #[test]
    fn indexed_screens_go_through_an_ilbm_unchanged() {
        for (machine, screen) in [(Machine::St, Screen::Low), (Machine::Ste, Screen::Medium), (Machine::St, Screen::High), (Machine::Falcon, Screen::Falcon256), (Machine::Tt, Screen::TtLow)] {
            let s = Settings { width: 97, height: 61, ..settings(machine, screen, Dither::FloydSteinberg) };
            let c = convert(&test_card(120, 80), &s);
            for packed in [false, true] {
                let picture = crate::pictures::decode(crate::pictures::Format::Ilbm, &to_ilbm(&c, packed).unwrap()).unwrap();
                assert_eq!(picture.rgba, c.rgba, "{machine:?} {screen:?}");
            }
            assert_eq!(from_interleaved(&screen_bytes(&c), 97, 61, c.planes()), c.values);
        }
    }

    #[test]
    fn levels_are_the_machines_and_more_colours_come_closer() {
        let image = test_card(160, 100);
        let st = convert(&image, &settings(Machine::St, Screen::Low, Dither::None));
        assert!(st.palette.iter().flatten().all(|v| Levels::St.all().contains(v)));
        assert_eq!(st.palette[0], [0, 0, 0]);
        let falcon = convert(&image, &settings(Machine::Falcon, Screen::Falcon256, Dither::None));
        assert!(falcon.palette.iter().flatten().all(|v| Levels::Falcon.all().contains(v)));
        let tt = convert(&image, &settings(Machine::Tt, Screen::TtLow, Dither::None));
        assert!(tt.palette.iter().flatten().all(|v| v % 17 == 0));
        let true_colour = convert(&image, &settings(Machine::Falcon, Screen::FalconTrue, Dither::None));
        assert!(falcon.error < st.error && true_colour.error < falcon.error, "{} {} {}", st.error, falcon.error, true_colour.error);
        assert_eq!(screen_bytes(&true_colour).len(), 320 * 200 * 2);
        let view = RawView { planes: 16, ..RawView::default() };
        assert_eq!(view.draw(&screen_bytes(&true_colour), &[]), true_colour.rgba);
        // The source has what it needs.
        let source = asm_source(&st, "pic", "pic.raw");
        assert!(source.contains("move.w  #5,-(sp)") && source.contains("incbin  \"pic.raw\"") && source.contains("pic_palette:"));
        assert!(palette_c(&st, "pic").contains("unsigned short pic_palette[16]"));
        assert!(asm_source(&falcon, "pic", "pic.raw").contains("VsetRGB"));
    }

    #[test]
    fn palettes_are_read_from_st_files() {
        let c = convert(&test_card(160, 100), &settings(Machine::St, Screen::Low, Dither::None));
        assert_eq!(read_palette(&to_degas(&c, false).unwrap()).unwrap(), c.palette);
        assert_eq!(read_palette(&to_neo(&c).unwrap()).unwrap(), c.palette);
        assert_eq!(read_palette(palette_asm(&c, "x").as_bytes()).unwrap(), c.palette);
        assert_eq!(read_palette(&palette_raw(&c)).unwrap(), c.palette);
        assert_eq!(read_palette(b"dc.w $0f00,$0008").unwrap(), [[255, 0, 0], [0, 0, 17]]);
    }

    fn ppm(width: usize, height: usize, rgba: &[u8]) -> Vec<u8> {
        let mut out = format!("P6\n{width} {height}\n255\n").into_bytes();
        out.extend(rgba.chunks(4).flat_map(|p| [p[0], p[1], p[2]]));
        out
    }

    fn read_ppm(bytes: &[u8]) -> (usize, usize, Vec<u8>) {
        let mut fields = Vec::new();
        let mut at = 0;
        while fields.len() < 4 {
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
            while !bytes[at].is_ascii_whitespace() {
                at += 1;
            }
            fields.push(String::from_utf8_lossy(&bytes[start..at]).into_owned());
        }
        at += 1;
        let (w, h, most): (usize, usize, u32) = (fields[1].parse().unwrap(), fields[2].parse().unwrap(), fields[3].parse().unwrap());
        // Netpbm's ST readers write levels out of 7 (or 15): spread to 8 bits, rounding to the nearest.
        let level = |v: u8| ((u32::from(v) * 255 + most / 2) / most) as u8;
        (w, h, bytes[at..at + w * h * 3].chunks(3).flat_map(|p| [level(p[0]), level(p[1]), level(p[2]), 255]).collect())
    }

    /// How many pixels differ, and by at most how much in a level.
    fn compare(a: &[u8], b: &[u8]) -> (usize, u8) {
        let differing = a.chunks(4).zip(b.chunks(4)).filter(|(p, q)| p != q).count();
        let most = a.iter().zip(b).map(|(x, y)| x.abs_diff(*y)).max().unwrap_or(0);
        (differing, most)
    }

    /// Our files read by Netpbm (pi1toppm, neotoppm, sputoppm, spctoppm) and RECOIL (recoil2png), and Netpbm's
    /// ppmtopi1's read by ours. Skipped where they aren't installed; LIMONITER_RECOIL names recoil2png.
    #[test]
    fn reference_tools_agree() {
        use std::process::Command;
        let dir = std::env::temp_dir().join(format!("limoniter-stpics-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let run = |tool: &str, args: &[&str]| -> Option<Vec<u8>> {
            let out = Command::new(tool).args(args).output().ok()?;
            out.status.success().then_some(out.stdout)
        };
        let recoil = std::env::var("LIMONITER_RECOIL").unwrap_or_else(|_| "recoil2png".into());
        let image = test_card(320, 200);
        let mut cases: Vec<(String, Converted)> = Vec::new();
        for machine in [Machine::St, Machine::Ste] {
            for screen in [Screen::Low, Screen::Medium, Screen::High] {
                cases.push((format!("{machine:?}-{screen:?}"), convert(&image, &settings(machine, screen, Dither::FloydSteinberg))));
            }
        }
        let mut report = Vec::new();
        for (label, c) in &cases {
            let resolution = c.screen.st_resolution().unwrap() + 1;
            for (ending, file) in [(format!("pi{resolution}"), to_degas(c, false).unwrap()), (format!("pc{resolution}"), to_degas(c, true).unwrap()), ("neo".into(), to_neo(c).unwrap())] {
                let path = dir.join(format!("{label}.{ending}"));
                std::fs::write(&path, &file).unwrap();
                let netpbm = match ending.as_str() {
                    "pi1" => Some("pi1toppm"),
                    "neo" if c.screen == Screen::Low => Some("neotoppm"),
                    _ => None,
                };
                if let Some(tool) = netpbm
                    && let Some(out) = run(tool, &[path.to_str().unwrap()])
                {
                    let (w, h, rgba) = read_ppm(&out);
                    assert_eq!((w, h), (c.width, c.height));
                    let (differing, most) = compare(&rgba, &c.rgba);
                    report.push(format!("{tool} {label}.{ending}: {differing} of {} pixels differ, by at most {most} a level", w * h));
                    // Netpbm reads three bits a level only, so an STE palette comes out as an ST one.
                    if c.levels == Levels::St {
                        assert_eq!(differing, 0, "{tool} {label}");
                    }
                }
                let png = dir.join(format!("{label}-{ending}.png"));
                if run(&recoil, &["-o", png.to_str().unwrap(), path.to_str().unwrap()]).is_some() {
                    let (w, h, rgba) = crate::screen_designer::read_png(&std::fs::read(&png).unwrap()).unwrap();
                    // RECOIL doubles medium resolution's height to show its pixels' shape.
                    let rgba: Vec<u8> = if h == c.height * 2 { rgba.chunks(w * 4).step_by(2).flatten().copied().collect() } else { rgba };
                    let (differing, most) = compare(&rgba, &c.rgba);
                    report.push(format!("recoil2png {label}.{ending}: {w}x{h}, {differing} of {} pixels differ, by at most {most} a level", c.width * c.height));
                    assert_eq!(differing, 0, "recoil2png {label}.{ending}");
                }
            }
            // Netpbm's own, from our pixels: low resolution only, and the ST's levels.
            if c.screen == Screen::Low && c.levels == Levels::St {
                let picture = dir.join(format!("{label}.ppm"));
                std::fs::write(&picture, ppm(c.width, c.height, &c.rgba)).unwrap();
                // (Netpbm 10's ppmtoneo writes a 32036-byte file, not NEOchrome's 32128, so it isn't tried.)
                if let Some(theirs) = run("ppmtopi1", &[picture.to_str().unwrap()]) {
                    let decoded = crate::pictures::decode_named(crate::pictures::Format::AtariSt, "t.pi1", &theirs).unwrap();
                    let (differing, most) = compare(&decoded.rgba, &c.rgba);
                    report.push(format!("ppmtopi1 {label}, read by Limoniter: {differing} of 64000 pixels differ, by at most {most} a level"));
                    assert_eq!(differing, 0, "ppmtopi1");
                }
            }
        }
        // Spectrum, both machines.
        for machine in [Machine::St, Machine::Ste] {
            let c = convert(&image, &settings(machine, Screen::Spectrum, Dither::FloydSteinberg));
            for (ending, file) in [("spu", crate::st_spectrum::to_spu(&c)), ("spc", crate::st_spectrum::to_spc(&c))] {
                let path = dir.join(format!("{machine:?}.{ending}"));
                std::fs::write(&path, &file).unwrap();
                let tool = if ending == "spu" { "sputoppm" } else { "spctoppm" };
                if let Some(out) = run(tool, &[path.to_str().unwrap()]) {
                    let (w, h, rgba) = read_ppm(&out);
                    // Netpbm shows the empty first line too.
                    let rgba: Vec<u8> = if h == 200 { rgba[w * 4..].to_vec() } else { rgba };
                    let (differing, most) = compare(&rgba, &c.rgba);
                    report.push(format!("{tool} {machine:?}.{ending}: {w}x{h}, {differing} of {} pixels differ, by at most {most} a level", 320 * 199));
                    if machine == Machine::St {
                        assert_eq!(differing, 0, "{tool}");
                    }
                }
                let png = dir.join(format!("{machine:?}-{ending}.png"));
                if run(&recoil, &["-o", png.to_str().unwrap(), path.to_str().unwrap()]).is_some() {
                    let (w, h, rgba) = crate::screen_designer::read_png(&std::fs::read(&png).unwrap()).unwrap();
                    let (differing, most) = compare(&rgba, &c.rgba);
                    report.push(format!("recoil2png {machine:?}.{ending}: {w}x{h}, {differing} of {} pixels differ, by at most {most} a level", 320 * 199));
                    assert_eq!(differing, 0, "recoil2png {machine:?}.{ending}");
                }
            }
        }
        for line in &report {
            eprintln!("{line}");
        }
        let _ = std::fs::remove_dir_all(&dir);
    }
}
