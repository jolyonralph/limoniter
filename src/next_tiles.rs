//! The Spectrum Next's tilemap and sprites, made from a picture.
//!
//! **Tilemap**: 40×32 (or 80×32) places of 8×8 tiles, 4 bits a pixel, up to 256 tiles of 32 bytes; each place two
//! bytes, the tile's number and its attribute: the palette it's shown in (bits 7 to 4, one of 16 of 16 colours),
//! mirrored across (bit 3) or down (bit 2), and turned a quarter clockwise (bit 1, before the mirroring). The
//! picture's places are shared out among the palettes (each found from the places it serves, 15 colours and the
//! 16th see-through), the pixels given their palette's colours, the same tiles (turned and mirrored too, if
//! allowed) kept once, and, if there are still more than may be, the most alike made one, k-means over the places
//! with what each place's own pixels would look like in each tile.
//!
//! **Sprites**: 16×16 patterns cut from a picture at its own size, 8 bits a pixel (64 of them, in the Next's own
//! palette or one found from the picture, $E3 see-through) or 4 (128, in 15 colours found and 3 see-through), what's
//! see-through from the picture's alpha or the colour of its top left corner.
//!
//! Each is saved as the Next keeps it, with its palette, sjasmplus source that shows it, and that assembled into a
//! `.nex`.

use crate::next_screen::{default_palette, rgb};
use crate::quantise::{self, Dither, Image, as_f32, oklab};
use std::collections::HashMap;

/// Where in a tile's definition the pixel shown at `p` (row by row) comes from, for an attribute's bits 3 to 1:
/// turned a quarter clockwise first, then mirrored.
pub fn source_index(p: usize, attribute: u8) -> usize {
    let (mut x, mut y) = (p % 8, p / 8);
    if attribute & 8 != 0 {
        x = 7 - x;
    }
    if attribute & 4 != 0 {
        y = 7 - y;
    }
    // Turned clockwise, the definition's top row is the right-hand column: the pixel shown at (x, y) is the
    // definition's at column y, row 7 - x.
    if attribute & 2 != 0 { (7 - x) * 8 + y } else { y * 8 + x }
}

/// The eight ways a tile can be shown, as attribute bits.
const ORIENTATIONS: [u8; 8] = [0, 8, 4, 12, 2, 10, 6, 14];

/// The Next's colours, in Oklab, and the nearest of them to a colour.
struct Gamut {
    labs: Vec<[f32; 3]>,
}

impl Gamut {
    fn new() -> Gamut {
        Gamut { labs: (0..512u16).map(|v| oklab(as_f32(rgb(v)))).collect() }
    }

    fn nearest(&self, colour: [f32; 3]) -> u16 {
        quantise::nearest(&self.labs, oklab(colour)).0 as u16
    }
}

/// Each pixel's place in the colours its pixel may be (`bank_of` says which list, or none to leave it), dithered as
/// asked; 255 where there's no list.
fn dither(pixels: &[[f32; 3]], width: usize, height: usize, bank_of: &dyn Fn(usize, usize) -> Option<usize>, banks: &[Vec<[u8; 3]>], how: Dither, strength: f32) -> Vec<u8> {
    let labs: Vec<Vec<[f32; 3]>> = banks.iter().map(|b| b.iter().map(|&c| oklab(as_f32(c))).collect()).collect();
    let mut values = vec![255u8; width * height];
    let spread = quantise::ordered_spread(banks.iter().map(Vec::len).max().unwrap_or(2), strength);
    let mut work = pixels.to_vec();
    for y in 0..height {
        let backwards = how == Dither::FloydSteinberg && y % 2 == 1;
        for i in 0..width {
            let x = if backwards { width - 1 - i } else { i };
            let Some(bank) = bank_of(x, y) else { continue };
            let p = match how {
                Dither::None => pixels[y * width + x],
                Dither::Ordered => {
                    let offset = ((quantise::bayer(x, y) + 0.5) / 64.0 - 0.5) * spread;
                    pixels[y * width + x].map(|v| (v + offset).clamp(0.0, 255.0))
                }
                Dither::FloydSteinberg => work[y * width + x].map(|v| v.clamp(0.0, 255.0)),
            };
            let chosen = quantise::nearest(&labs[bank], oklab(p)).0;
            values[y * width + x] = chosen as u8;
            if how == Dither::FloydSteinberg {
                let shown = banks[bank][chosen];
                let error: [f32; 3] = std::array::from_fn(|c| (p[c] - f32::from(shown[c])) * strength);
                quantise::diffuse(&mut work, width, height, x, y, backwards, error);
            }
        }
    }
    values
}

// The tilemap.

#[derive(Clone, Debug, PartialEq)]
pub struct TileSettings {
    /// 80 columns rather than 40.
    pub wide: bool,
    /// At most this many tiles (up to 256).
    pub most: usize,
    /// At most this many palettes of 16 (up to 16).
    pub palettes: usize,
    /// Tiles that are another turned or mirrored kept once.
    pub turns: bool,
    pub black_first: bool,
}

impl Default for TileSettings {
    fn default() -> Self {
        TileSettings { wide: false, most: 256, palettes: 16, turns: true, black_first: true }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Tilemap {
    pub wide: bool,
    /// Each tile's 64 pixels, row by row, 0 to 14 (15 is see-through).
    pub tiles: Vec<[u8; 64]>,
    /// Each place's tile and attribute.
    pub map: Vec<(u8, u8)>,
    /// 16 palettes of 16, as 9-bit colours.
    pub palette: Vec<u16>,
    /// How many different places there were before any were made to share a tile.
    pub different: usize,
    /// How many palettes are used.
    pub palettes: usize,
}

const CLEAR: u8 = 15;

pub fn tilemap(pixels: &[[f32; 3]], s: &TileSettings, how: Dither, strength: f32) -> Tilemap {
    let columns = if s.wide { 80 } else { 40 };
    let (width, height) = (columns * 8, 256);
    let places = columns * 32;
    let place_of = |x: usize, y: usize| (y / 8) * columns + x / 8;
    let gamut = Gamut::new();
    let labs: Vec<[f32; 3]> = pixels.iter().map(|&p| oklab(p)).collect();
    let mut members: Vec<Vec<usize>> = vec![Vec::new(); places];
    for y in 0..height {
        for x in 0..width {
            members[place_of(x, y)].push(y * width + x);
        }
    }

    // The places shared out among the palettes: first by their average colour, then each palette found from its
    // places' pixels and each place given the palette that serves it best, a few times over.
    let count = s.palettes.clamp(1, 16).min(places);
    let means: Vec<[f32; 3]> = members.iter().map(|m| {
        let mut sum = [0.0f32; 3];
        for &i in m {
            for c in 0..3 {
                sum[c] += labs[i][c];
            }
        }
        sum.map(|v| v / m.len() as f32)
    }).collect();
    let mut centres = vec![means[0]];
    while centres.len() < count {
        let far = (0..places).max_by(|&a, &b| {
            let d = |i: usize| centres.iter().map(|&c| quantise::distance(means[i], c)).fold(f32::MAX, f32::min);
            d(a).total_cmp(&d(b))
        }).unwrap_or(0);
        centres.push(means[far]);
    }
    let mut bank: Vec<usize> = means.iter().map(|&m| quantise::nearest(&centres, m).0).collect();
    let mut banks: Vec<Vec<[u8; 3]>> = vec![Vec::new(); count];
    for round in 0..4 {
        for (b, colours) in banks.iter_mut().enumerate() {
            let chosen: Vec<[f32; 3]> = (0..places).filter(|&p| bank[p] == b).flat_map(|p| members[p].iter().map(|&i| pixels[i])).collect();
            let slots: Vec<Option<[u8; 3]>> = (0..15).map(|i| (i == 0 && b == 0 && s.black_first).then_some([0, 0, 0])).collect();
            *colours = if chosen.is_empty() { vec![[0, 0, 0]; 15] } else { quantise::find_palette(&chosen, &slots, &|c| rgb(gamut.nearest(c)), None) };
        }
        if round == 3 {
            break;
        }
        let bank_labs: Vec<Vec<[f32; 3]>> = banks.iter().map(|b| b.iter().map(|&c| oklab(as_f32(c))).collect()).collect();
        let cost = |place: usize, b: usize| -> f32 { members[place].iter().map(|&i| quantise::nearest(&bank_labs[b], labs[i]).1).sum() };
        let all: Vec<usize> = (0..places).collect();
        bank = crate::media::flux::in_parallel(&all, |&place| (0..count).min_by(|&a, &b| cost(place, a).total_cmp(&cost(place, b))).unwrap_or(0));
        // A palette nobody chose takes the place served worst.
        for b in 0..count {
            if !bank.contains(&b)
                && let Some(worst) = (0..places).max_by(|&x, &y| cost(x, bank[x]).total_cmp(&cost(y, bank[y])))
            {
                bank[worst] = b;
            }
        }
    }

    let values = dither(pixels, width, height, &|x, y| Some(bank[place_of(x, y)]), &banks, how, strength);
    let shown: Vec<[u8; 64]> = (0..places).map(|place| std::array::from_fn(|p| values[members[place][p]])).collect();

    // The same tiles kept once: each place's tile as the definition that, shown its way, looks like it, the least
    // of the ways allowed.
    let orientations: &[u8] = if s.turns { &ORIENTATIONS } else { &ORIENTATIONS[..1] };
    let definition = |tile: &[u8; 64], o: u8| -> [u8; 64] {
        let mut d = [0u8; 64];
        for (p, &v) in tile.iter().enumerate() {
            d[source_index(p, o)] = v;
        }
        d
    };
    let mut unique: Vec<[u8; 64]> = Vec::new();
    let mut known: HashMap<[u8; 64], usize> = HashMap::new();
    let mut placed: Vec<(usize, u8)> = Vec::with_capacity(places);
    for tile in &shown {
        let (d, o) = orientations.iter().map(|&o| (definition(tile, o), o)).min().unwrap_or((*tile, 0));
        let index = *known.entry(d).or_insert_with(|| {
            unique.push(d);
            unique.len() - 1
        });
        placed.push((index, o));
    }
    let different = unique.len();
    let most = s.most.clamp(1, 256);
    let (tiles, placed) = if unique.len() <= most {
        (unique, placed)
    } else {
        share(&unique, &placed, &members, &labs, &banks, &bank, most, orientations)
    };
    let mut palette = vec![0u16; 256];
    for (b, colours) in banks.iter().enumerate() {
        for (i, &c) in colours.iter().enumerate() {
            palette[b * 16 + i] = crate::next_screen::nearest_value(c);
        }
    }
    let map = placed.iter().zip(&bank).map(|(&(t, o), &b)| (t as u8, (b as u8) << 4 | o)).collect();
    Tilemap { wide: s.wide, tiles, map, palette, different, palettes: count }
}

/// More tiles than may be, made into `most`: k-means over the places, each place given the tile and way of showing
/// it that comes nearest its own pixels in its palette, each tile the pixels that suit its places best.
#[allow(clippy::too_many_arguments)]
fn share(unique: &[[u8; 64]], placed: &[(usize, u8)], members: &[Vec<usize>], labs: &[[f32; 3]], banks: &[Vec<[u8; 3]>], bank: &[usize], most: usize, orientations: &[u8]) -> (Vec<[u8; 64]>, Vec<(usize, u8)>) {
    let bank_labs: Vec<Vec<[f32; 3]>> = banks.iter().map(|b| b.iter().map(|&c| oklab(as_f32(c))).collect()).collect();
    // What each place's pixel would cost in each of its palette's colours.
    let costs: Vec<Vec<[f32; 15]>> = members.iter().zip(bank).map(|(m, &b)| m.iter().map(|&i| std::array::from_fn(|v| quantise::distance(labs[i], bank_labs[b][v]))).collect()).collect();
    let source: Vec<[usize; 64]> = orientations.iter().map(|&o| std::array::from_fn(|p| source_index(p, o))).collect();
    let tile_cost = |place: usize, tile: &[u8; 64], way: usize, limit: f32| -> f32 {
        let c = &costs[place];
        let mut sum = 0.0;
        for p in 0..64 {
            sum += c[p][usize::from(tile[source[way][p]])];
            if sum >= limit {
                return sum;
            }
        }
        sum
    };
    // The first tiles: the most used, then each the one least like those chosen.
    let mut uses = vec![0usize; unique.len()];
    for &(u, _) in placed {
        uses[u] += 1;
    }
    let looks = |u: usize| -> Vec<[f32; 3]> {
        let place = placed.iter().position(|&(t, _)| t == u).unwrap_or(0);
        unique[u].iter().map(|&v| bank_labs[bank[place]][usize::from(v)]).collect()
    };
    let looks_all: Vec<Vec<[f32; 3]>> = (0..unique.len()).map(looks).collect();
    let first = (0..unique.len()).max_by_key(|&u| uses[u]).unwrap_or(0);
    let mut centres = vec![unique[first]];
    let mut nearest: Vec<f32> = looks_all.iter().map(|l| l.iter().zip(&looks_all[first]).map(|(a, b)| quantise::distance(*a, *b)).sum()).collect();
    while centres.len() < most {
        let far = (0..unique.len()).max_by(|&a, &b| nearest[a].total_cmp(&nearest[b])).unwrap_or(0);
        centres.push(unique[far]);
        for u in 0..unique.len() {
            let d: f32 = looks_all[u].iter().zip(&looks_all[far]).map(|(a, b)| quantise::distance(*a, *b)).sum();
            nearest[u] = nearest[u].min(d);
        }
    }
    let mut chosen: Vec<(usize, usize, f32)> = vec![(0, 0, 0.0); members.len()];
    for round in 0..5 {
        // Each place to its best tile and way, on all the computer's cores; starting from the one it had, which
        // is likely near, so the rest can be given up on sooner.
        let all: Vec<usize> = (0..members.len()).collect();
        chosen = crate::media::flux::in_parallel(&all, |&place| {
            let (k0, way0, _) = chosen[place];
            let mut b = (k0, way0, tile_cost(place, &centres[k0], way0, f32::MAX));
            for (k, tile) in centres.iter().enumerate() {
                for way in 0..orientations.len() {
                    let c = tile_cost(place, tile, way, b.2);
                    if c < b.2 {
                        b = (k, way, c);
                    }
                }
            }
            b
        });
        if round == 4 {
            break;
        }
        // Each tile's pixels: for each, the value its places would rather.
        let mut sums = vec![[[0.0f32; 15]; 64]; centres.len()];
        for (place, &(k, way, _)) in chosen.iter().enumerate() {
            for p in 0..64 {
                let q = source[way][p];
                for v in 0..15 {
                    sums[k][q][v] += costs[place][p][v];
                }
            }
        }
        let mut used = vec![false; centres.len()];
        for &(k, _, _) in &chosen {
            used[k] = true;
        }
        for (k, tile) in centres.iter_mut().enumerate() {
            if used[k] {
                for q in 0..64 {
                    tile[q] = (0..15).min_by(|&a, &b| sums[k][q][a].total_cmp(&sums[k][q][b])).unwrap_or(0) as u8;
                }
            }
        }
        // A tile no place chose becomes the tile of the place served worst.
        for k in (0..centres.len()).filter(|&k| !used[k]) {
            if let Some(worst) = (0..chosen.len()).max_by(|&a, &b| chosen[a].2.total_cmp(&chosen[b].2)) {
                centres[k] = unique[placed[worst].0];
                chosen[worst].2 = 0.0;
            }
        }
    }
    let placed = chosen.iter().map(|&(k, way, _)| (k, orientations[way])).collect();
    (centres, placed)
}

impl Tilemap {
    pub fn columns(&self) -> usize {
        if self.wide { 80 } else { 40 }
    }

    pub fn width(&self) -> usize {
        self.columns() * 8
    }

    /// A pixel's colour as the Next shows it.
    pub fn shown(&self, x: usize, y: usize) -> [u8; 3] {
        let (tile, attribute) = self.map[(y / 8) * self.columns() + x / 8];
        let value = self.tiles[usize::from(tile)][source_index((y % 8) * 8 + x % 8, attribute & 14)];
        rgb(self.palette[usize::from(attribute >> 4) * 16 + usize::from(value)])
    }

    pub fn rgba(&self) -> Vec<u8> {
        let width = self.width();
        (0..width * 256).flat_map(|i| {
            let [r, g, b] = self.shown(i % width, i / width);
            [r, g, b, 255]
        }).collect()
    }

    /// The tiles as the Next keeps them: 32 bytes each, two pixels a byte, the left in the high half.
    pub fn tile_bytes(&self) -> Vec<u8> {
        self.tiles.iter().flat_map(|t| t.chunks(2).map(|p| p[0] << 4 | p[1]).collect::<Vec<u8>>()).collect()
    }

    /// The map: each place's tile, then its attribute.
    pub fn map_bytes(&self) -> Vec<u8> {
        self.map.iter().flat_map(|&(t, a)| [t, a]).collect()
    }

    pub fn palette_bytes(&self) -> Vec<u8> {
        self.palette.iter().flat_map(|&v| crate::next_screen::split(v)).collect()
    }

    pub fn source(&self, name: &str) -> String {
        let columns = self.columns();
        let mut s = crate::i18n::tf!("; {name}: a tilemap for the ZX Spectrum Next, {columns}\u{D7}32 places of {} tiles, made by Limoniter.\n; Assemble with sjasmplus (or F9 in Limoniter), with {name}.til, {name}.map and {name}.nxp beside it:\n; it writes {name}.nex, which shows it until the Next is reset.\n\n",
            self.tiles.len()
        );
        s += "        DEVICE ZXSPECTRUMNEXT\n        org $8000\nstart:\n        di\n        nextreg $07,3                   ; 28 MHz\n";
        s += "        nextreg $68,%10000000           ; the ULA off: the tiles and map are where its screen is\n";
        s += "        nextreg $4a,0                   ; black where nothing's shown\n";
        s += "        nextreg $6e,$20                 ; the map at $6000\n        nextreg $6f,$00                 ; the tiles at $4000\n";
        s += &format!("        nextreg $4c,{CLEAR}                  ; colour {CLEAR} of each palette see-through\n");
        s += "        nextreg $2f,0                   ; not scrolled\n        nextreg $30,0\n        nextreg $31,0\n";
        s += "        nextreg $1c,%00001000           ; the tilemap's clip window, from its first value:\n        nextreg $1b,0                   ; left and right, in 2s (4s at 80 columns)\n        nextreg $1b,159\n        nextreg $1b,0                   ; top and bottom\n        nextreg $1b,255\n";
        s += "        nextreg $43,%00110000           ; the tilemap's first palette to be written\n        nextreg $40,0                   ; from colour 0, on by itself after each\n";
        s += "        ld hl,palette\n        ld b,0\n.colour:\n        ld a,(hl)\n        inc hl\n        nextreg $44,a                   ; RRRGGGBB\n        ld a,(hl)\n        inc hl\n        nextreg $44,a                   ; then the lowest blue bit\n        djnz .colour\n";
        s += &format!("        nextreg $6b,%{}000001           ; the tilemap on, {columns} columns, two bytes a place\n", if self.wide { "11" } else { "10" });
        s += "wait:   jr wait\n\npalette:\n";
        s += &format!("        incbin \"{name}.nxp\"\n\n        org $4000\n        incbin \"{name}.til\"           ; {} tiles of 32 bytes\n        org $6000\n        incbin \"{name}.map\"           ; each place's tile and attribute\n\n", self.tiles.len());
        s += &format!("        SAVENEX OPEN \"{name}.nex\",start,$bffe\n        SAVENEX CORE 3,0,0\n        SAVENEX CFG 0\n        SAVENEX BANK 5,2\n        SAVENEX CLOSE\n");
        s
    }

    pub fn everything(&self, name: &str) -> Vec<(String, Vec<u8>)> {
        let mut files = vec![
            (format!("{name}.til"), self.tile_bytes()),
            (format!("{name}.map"), self.map_bytes()),
            (format!("{name}.nxp"), self.palette_bytes()),
            (format!("{name}.asm"), self.source(name).into_bytes()),
        ];
        if let Ok(nex) = crate::next_screen::assemble_nex(name, &self.source(name), &files) {
            files.push((format!("{name}.nex"), nex));
        }
        let picture = crate::pictures::Picture { width: self.width(), height: 256, rgba: self.rgba(), aspect: if self.wide { 0.5 } else { 1.0 }, info: Vec::new(), cycling: None };
        if let Ok(png) = crate::pictures::to_png(&picture) {
            files.push((format!("{name}.png"), png));
        }
        files
    }
}

// Sprites.

/// What's see-through in a sprite.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SeeThrough {
    /// Where the picture is (alpha under half).
    Alpha,
    /// The colour of the picture's top left corner, wherever it is (and where the picture is see-through).
    Corner,
    /// Nothing.
    Nothing,
}

impl SeeThrough {
    pub const ALL: [SeeThrough; 3] = [SeeThrough::Alpha, SeeThrough::Corner, SeeThrough::Nothing];

    pub fn name(self) -> &'static str {
        match self {
            SeeThrough::Alpha => "Where the picture is see-through",
            SeeThrough::Corner => "The top left corner's colour",
            SeeThrough::Nothing => "Nothing",
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct SpriteSettings {
    pub across: usize,
    pub down: usize,
    /// Where the first sprite's top left is in the picture.
    pub x: usize,
    pub y: usize,
    /// 4 bits a pixel (128 sprites, 16 colours) rather than 8 (64, 256).
    pub four_bit: bool,
    /// For 8 bits: a palette found from the picture, rather than the Next's own.
    pub own_palette: bool,
    pub see_through: SeeThrough,
}

impl Default for SpriteSettings {
    fn default() -> Self {
        SpriteSettings { across: 4, down: 4, x: 0, y: 0, four_bit: false, own_palette: false, see_through: SeeThrough::Alpha }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Sprites {
    pub across: usize,
    pub down: usize,
    pub four_bit: bool,
    pub own_palette: bool,
    /// Each sprite's 256 pixels, row by row: palette places, the see-through one where it's see-through.
    pub patterns: Vec<[u8; 256]>,
    pub palette: Vec<u16>,
}

/// The index that's see-through at reset: $E3, and for 4-bit sprites its low half.
pub fn clear_index(four_bit: bool) -> u8 {
    if four_bit { 3 } else { 0xE3 }
}

pub fn cut_sprites(image: &Image, s: &SpriteSettings, how: Dither, strength: f32) -> Sprites {
    let most = if s.four_bit { 128 } else { 64 };
    let across = s.across.clamp(1, 16);
    let down = s.down.clamp(1, most / across);
    let (width, height) = (across * 16, down * 16);
    let at = |x: usize, y: usize| -> Option<[u8; 4]> {
        let (sx, sy) = (s.x + x, s.y + y);
        (sx < image.width && sy < image.height).then(|| {
            let i = (sy * image.width + sx) * 4;
            [image.rgba[i], image.rgba[i + 1], image.rgba[i + 2], image.rgba[i + 3]]
        })
    };
    let corner = at(0, 0).map(|c| [c[0], c[1], c[2]]);
    let mut pixels = vec![[0.0f32; 3]; width * height];
    let mut solid = vec![false; width * height];
    for y in 0..height {
        for x in 0..width {
            if let Some(c) = at(x, y) {
                pixels[y * width + x] = as_f32([c[0], c[1], c[2]]);
                solid[y * width + x] = match s.see_through {
                    SeeThrough::Nothing => true,
                    SeeThrough::Alpha => c[3] >= 128,
                    SeeThrough::Corner => c[3] >= 128 && Some([c[0], c[1], c[2]]) != corner,
                };
            }
        }
    }
    let clear = clear_index(s.four_bit);
    let gamut = Gamut::new();
    let opaque: Vec<[f32; 3]> = pixels.iter().zip(&solid).filter(|(_, s)| **s).map(|(p, _)| *p).collect();
    // The colours a pixel can be, and the palette place each is in.
    let (colours, places, palette): (Vec<[u8; 3]>, Vec<u8>, Vec<u16>) = if s.four_bit || s.own_palette {
        let count = if s.four_bit { 15 } else { 255 };
        let slots = vec![None; count];
        let found = if opaque.is_empty() { vec![[0, 0, 0]; count] } else { quantise::find_palette(&opaque, &slots, &|c| rgb(gamut.nearest(c)), None) };
        let places: Vec<u8> = (0..=255u8).filter(|&i| i != clear).take(count).collect();
        let mut palette = if s.four_bit { vec![0u16; 256] } else { default_palette() };
        for (&place, &c) in places.iter().zip(&found) {
            palette[usize::from(place)] = crate::next_screen::nearest_value(c);
        }
        (found, places, palette)
    } else {
        let palette = default_palette();
        let places: Vec<u8> = (0..=255u8).filter(|&i| i != clear).collect();
        (places.iter().map(|&i| rgb(palette[usize::from(i)])).collect(), places, palette)
    };
    let values = dither(&pixels, width, height, &|x, y| solid[y * width + x].then_some(0), &[colours], how, strength);
    let patterns = (0..across * down)
        .map(|n| {
            let (sx, sy) = ((n % across) * 16, (n / across) * 16);
            std::array::from_fn(|p| {
                let v = values[(sy + p / 16) * width + sx + p % 16];
                if v == 255 { clear } else { places[usize::from(v)] }
            })
        })
        .collect();
    Sprites { across, down, four_bit: s.four_bit, own_palette: s.own_palette || s.four_bit, patterns, palette }
}

impl Sprites {
    pub fn width(&self) -> usize {
        self.across * 16
    }

    pub fn height(&self) -> usize {
        self.down * 16
    }

    /// A pixel of the sheet: its colour, or none where it's see-through.
    pub fn shown(&self, x: usize, y: usize) -> Option<[u8; 3]> {
        let value = self.patterns[(y / 16) * self.across + x / 16][(y % 16) * 16 + x % 16];
        (value != clear_index(self.four_bit)).then(|| rgb(self.palette[usize::from(value)]))
    }

    pub fn rgba(&self) -> Vec<u8> {
        let width = self.width();
        (0..width * self.height()).flat_map(|i| match self.shown(i % width, i / width) {
            Some([r, g, b]) => [r, g, b, 255],
            None => [0, 0, 0, 0],
        }).collect()
    }

    /// The patterns as the Next's sprite memory takes them: 256 bytes each, or 128 at 4 bits (two pixels a
    /// byte, the left in the high half), filled out to whole 256-byte slots.
    pub fn pattern_bytes(&self) -> Vec<u8> {
        let mut out: Vec<u8> = if self.four_bit {
            self.patterns.iter().flat_map(|p| p.chunks(2).map(|c| c[0] << 4 | c[1]).collect::<Vec<u8>>()).collect()
        } else {
            self.patterns.iter().flatten().copied().collect()
        };
        out.resize(out.len().div_ceil(256) * 256, 0);
        out
    }

    pub fn palette_bytes(&self) -> Vec<u8> {
        self.palette.iter().flat_map(|&v| crate::next_screen::split(v)).collect()
    }

    /// Each sprite's attributes, to show them in rows of 16 on the screen: 4 bytes each, 5 for 4-bit ones.
    fn attributes(&self) -> Vec<u8> {
        let mut out = Vec::new();
        for n in 0..self.patterns.len() {
            let (x, y) = (32 + (n % 16) * 18, 32 + (n / 16) * 18);
            out.push((x & 255) as u8);
            out.push(y as u8);
            out.push((x >> 8) as u8);
            if self.four_bit {
                out.push(0x80 | 0x40 | (n >> 1) as u8);
                out.push(0x80 | ((n & 1) as u8) << 6);
            } else {
                out.push(0x80 | n as u8);
            }
        }
        out
    }

    pub fn source(&self, name: &str) -> String {
        let count = self.patterns.len();
        let bits = if self.four_bit { 4 } else { 8 };
        let colours = if self.four_bit { 16 } else { 256 };
        let patterns = self.pattern_bytes();
        let attributes = self.attributes();
        let mut s = crate::i18n::tf!("; {name}: {count} sprites for the ZX Spectrum Next, 16\u{D7}16 at {bits} bits a pixel, made by Limoniter.\n; Assemble with sjasmplus (or F9 in Limoniter), with {name}.spr and {name}.nxp beside it:\n; it writes {name}.nex, which shows them in rows of 16 until the Next is reset.\n\n"
        );
        s += "        DEVICE ZXSPECTRUMNEXT\n        org $8000\nstart:\n        di\n        nextreg $07,3                   ; 28 MHz\n";
        s += "        ld hl,$4000                     ; the ULA's screen black\n        ld de,$4001\n        ld bc,6911\n        ld (hl),0\n        ldir\n";
        s += "        nextreg $43,%00100000           ; the sprites' first palette to be written\n        nextreg $40,0                   ; from colour 0, on by itself after each\n";
        s += &format!("        ld hl,palette\n        ld b,{}\n.colour:\n        ld a,(hl)\n        inc hl\n        nextreg $44,a                   ; RRRGGGBB\n        ld a,(hl)\n        inc hl\n        nextreg $44,a                   ; then the lowest blue bit\n        djnz .colour\n", colours & 255);
        s += &format!(
            "        ld bc,$303b                     ; from pattern slot 0\n        xor a\n        out (c),a\n        ld hl,patterns\n        ld e,{}                         ; 256-byte slots\n.slot:  ld bc,$005b                     ; 256 bytes to port $5b\n        otir\n        dec e\n        jr nz,.slot\n",
            patterns.len() / 256
        );
        s += &format!(
            "        ld bc,$303b                     ; from sprite 0\n        xor a\n        out (c),a\n        ld hl,attributes\n        ld de,{}\n.attribute:\n        ld a,(hl)\n        out ($57),a\n        inc hl\n        dec de\n        ld a,d\n        or e\n        jr nz,.attribute\n",
            attributes.len()
        );
        s += "        nextreg $15,%00000011           ; sprites shown, over the border too\nwait:   jr wait\n\n";
        s += &format!("palette:\n        incbin \"{name}.nxp\",0,{}\npatterns:\n        incbin \"{name}.spr\"\n", colours * 2);
        s += &format!("attributes:                             ; x, y, x's top bit, shown and the pattern{}\n", if self.four_bit { ", 4-bit and which half" } else { "" });
        let per = if self.four_bit { 5 } else { 4 };
        for sprite in attributes.chunks(per) {
            s += &format!("        db {}\n", sprite.iter().map(|b| format!("${b:02x}")).collect::<Vec<_>>().join(","));
        }
        // The code and data from $8000; more than bank 2 holds goes on into bank 0.
        let end = 0x8000 + 0x200 + colours * 2 + patterns.len() + attributes.len();
        let banks = if end > 0xC000 { "2,0" } else { "2" };
        s += &format!("\n        SAVENEX OPEN \"{name}.nex\",start,$5ffe\n        SAVENEX CORE 3,0,0\n        SAVENEX CFG 0\n        SAVENEX BANK {banks}\n        SAVENEX CLOSE\n");
        s
    }

    pub fn everything(&self, name: &str) -> Vec<(String, Vec<u8>)> {
        let mut files = vec![(format!("{name}.spr"), self.pattern_bytes()), (format!("{name}.nxp"), self.palette_bytes()), (format!("{name}.asm"), self.source(name).into_bytes())];
        if let Ok(nex) = crate::next_screen::assemble_nex(name, &self.source(name), &files) {
            files.push((format!("{name}.nex"), nex));
        }
        let picture = crate::pictures::Picture { width: self.width(), height: self.height(), rgba: self.rgba(), aspect: 1.0, info: Vec::new(), cycling: None };
        if let Ok(png) = crate::pictures::to_png(&picture) {
            files.push((format!("{name}.png"), png));
        }
        files
    }
}

#[cfg(test)]
mod tests;
