//! Spectrum 512: a picture of 320x199 in up to 48 colours a line, by changing the palette as each line is drawn.
//!
//! A display routine timed to the cycle writes 48 palette words a line, three palettes of 16. Which one a pixel's
//! colour comes from depends on where along the line it is: colour c is the first palette's until pixel x1 =
//! 10c + 1 (10c - 5 when c is odd), the second's for the 160 pixels from there, and the third's after that. So
//! each colour register is three colours in turn, changing at a different place for each register.
//!
//! The files: .SPU is the 32000-byte screen (its first line empty, the picture on the 199 after it) and then 199
//! lines of 48 words, 51104 bytes in all. .SPC packs both: "SP", a word of 0, the packed screen's length and the
//! packed palettes' length (longs), then each plane's words from the second line down, packed (a byte n under 128
//! copies n + 1 bytes, otherwise the next byte is repeated 258 - n times), then each palette as a word of bits (bit
//! i set when colour i isn't 0, colour 15 never) and the words that aren't 0. Colours 0 and 15 of every palette are
//! kept black here, as the packed formats need: .SPC can't hold colour 15, and .SPS (read, not written) neither 0
//! nor 15.
//!
//! On the STE the palette words have the fourth bit a level, and the same layout makes "Spectrum 4096".

use crate::quantise::{self, Dither, as_f32, bayer, oklab};
use crate::st_planar::{Converted, Levels, st_word};

pub const WIDTH: usize = 320;
pub const HEIGHT: usize = 199;

/// Where colour `c` turns from one palette to the next along a line.
fn turn(c: usize) -> usize {
    c * 10 + 1 - (c & 1) * 6
}

/// Which of a line's 48 colours a pixel of colour `c` at `x` shows: 16 times the palette, plus `c`.
pub fn slot(x: usize, c: usize) -> usize {
    let x1 = turn(c);
    if x >= x1 + 160 {
        32 + c
    } else if x >= x1 {
        16 + c
    } else {
        c
    }
}

/// The pixels a line's colour `s` (of 48) shows on.
fn span(s: usize) -> std::ops::Range<usize> {
    let (palette, c) = (s / 16, s % 16);
    let x1 = turn(c);
    match palette {
        0 => 0..x1,
        1 => x1..x1 + 160,
        _ => (x1 + 160).min(WIDTH)..WIDTH,
    }
}

/// Colours 0 and 15 are black in every palette.
fn fixed(s: usize) -> bool {
    matches!(s % 16, 0 | 15)
}

/// Every colour the palette can hold, and where each is in Oklab.
struct Grid {
    levels: Vec<u8>,
    labs: Vec<[f32; 3]>,
}

impl Grid {
    fn new(levels: Levels) -> Self {
        let levels = levels.all();
        let n = levels.len();
        let labs = (0..n * n * n).map(|i| oklab(as_f32([levels[i / (n * n)], levels[i / n % n], levels[i % n]]))).collect();
        Grid { levels, labs }
    }

    fn colour(&self, i: usize) -> [u8; 3] {
        let n = self.levels.len();
        [self.levels[i / (n * n)], self.levels[i / n % n], self.levels[i % n]]
    }

    /// The grid colour nearest a colour, a level at a time.
    fn index(&self, c: [f32; 3]) -> usize {
        let n = self.levels.len();
        let near = |v: f32| (0..n).min_by(|&a, &b| (f32::from(self.levels[a]) - v).abs().total_cmp(&(f32::from(self.levels[b]) - v).abs())).unwrap_or(0);
        (near(c[0]) * n + near(c[1])) * n + near(c[2])
    }
}

fn dist(a: [f32; 3], b: [f32; 3]) -> f32 {
    (a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2) + (a[2] - b[2]).powi(2)
}

/// The 48 colours for a line, as grid places, found for its pixels: first one at a time, each the colour that
/// helps the pixels it can show on most, then each tried again against the rest a few times over.
fn line_palette(target: &[[f32; 3]], grid: &Grid, black: usize) -> [usize; 48] {
    let labs: Vec<[f32; 3]> = target.iter().map(|&p| oklab(p.map(|v| v.clamp(0.0, 255.0)))).collect();
    let snapped: Vec<usize> = target.iter().map(|&p| grid.index(p)).collect();
    let mut chosen: [Option<usize>; 48] = [None; 48];
    for (s, c) in chosen.iter_mut().enumerate() {
        if fixed(s) {
            *c = Some(black);
        }
    }
    // The best any other chosen colour does for each pixel of a span.
    let others = |chosen: &[Option<usize>; 48], s: usize| -> Vec<f32> {
        span(s)
            .map(|x| {
                (0..16)
                    .map(|c| slot(x, c))
                    .filter(|&o| o != s)
                    .filter_map(|o| chosen[o])
                    .map(|g| dist(grid.labs[g], labs[x]))
                    .fold(f32::MAX, f32::min)
            })
            .collect()
    };
    let best_for = |chosen: &[Option<usize>; 48], s: usize| -> usize {
        let range = span(s);
        let base = others(chosen, s);
        // The candidates: the colours of the pixels served worst, and the average of those this colour would serve.
        let mut order: Vec<usize> = (0..base.len()).collect();
        order.sort_by(|&a, &b| base[b].total_cmp(&base[a]));
        let mut candidates: Vec<usize> = Vec::new();
        for &i in &order {
            let g = snapped[range.start + i];
            if !candidates.contains(&g) {
                candidates.push(g);
                if candidates.len() >= 24 {
                    break;
                }
            }
        }
        if let Some(current) = chosen[s] {
            let served: Vec<usize> = (0..base.len()).filter(|&i| dist(grid.labs[current], labs[range.start + i]) < base[i]).collect();
            if !served.is_empty() {
                let mut sum = [0.0f32; 3];
                for &i in &served {
                    for (k, v) in sum.iter_mut().enumerate() {
                        *v += target[range.start + i][k];
                    }
                }
                candidates.push(grid.index(sum.map(|v| v / served.len() as f32)));
            }
            candidates.push(current);
        }
        let cost = |g: usize| -> f32 { base.iter().enumerate().map(|(i, &b)| b.min(dist(grid.labs[g], labs[range.start + i]))).sum() };
        candidates.into_iter().map(|g| (g, cost(g))).min_by(|a, b| a.1.total_cmp(&b.1)).map_or(black, |(g, _)| g)
    };
    // The middle palette's colours first: each covers half the line.
    let order: Vec<usize> = (16..32).chain(0..16).chain(32..48).filter(|&s| !fixed(s)).collect();
    for &s in &order {
        chosen[s] = Some(best_for(&chosen, s));
    }
    for _ in 0..2 {
        for &s in &order {
            chosen[s] = Some(best_for(&chosen, s));
        }
    }
    chosen.map(|c| c.unwrap_or(black))
}

/// The picture (320x199) as Spectrum colours: each pixel's colour number, and each line's 48 colours.
pub fn encode(pixels: &[[f32; 3]], levels: Levels, dither: Dither, strength: f32) -> (Vec<u8>, Vec<[[u8; 3]; 48]>) {
    let grid = Grid::new(levels);
    let black = grid.index([0.0; 3]);
    let mut values = vec![0u8; WIDTH * HEIGHT];
    let mut lines = Vec::with_capacity(HEIGHT);
    let mut carried = vec![[0.0f32; 3]; WIDTH + 2];
    let spread = quantise::ordered_spread(48, strength);
    for y in 0..HEIGHT {
        let row = &pixels[y * WIDTH..(y + 1) * WIDTH];
        let mut target: Vec<[f32; 3]> = row.to_vec();
        if dither == Dither::FloydSteinberg {
            for (x, p) in target.iter_mut().enumerate() {
                for k in 0..3 {
                    p[k] += carried[x + 1][k];
                }
            }
        }
        let palette = line_palette(&target, &grid, black);
        let colours: [[u8; 3]; 48] = palette.map(|g| grid.colour(g));
        let labs: [[f32; 3]; 48] = palette.map(|g| grid.labs[g]);
        let mut next = vec![[0.0f32; 3]; WIDTH + 2];
        let mut ahead = [0.0f32; 3];
        for x in 0..WIDTH {
            let mut p = target[x];
            match dither {
                Dither::Ordered => {
                    let offset = ((bayer(x, y) + 0.5) / 64.0 - 0.5) * spread;
                    p = p.map(|v| v + offset);
                }
                Dither::FloydSteinberg => {
                    for k in 0..3 {
                        p[k] += ahead[k];
                    }
                }
                Dither::None => {}
            }
            let p = p.map(|v| v.clamp(0.0, 255.0));
            let lab = oklab(p);
            let c = (0..16).min_by(|&a, &b| dist(labs[slot(x, a)], lab).total_cmp(&dist(labs[slot(x, b)], lab))).unwrap_or(0);
            values[y * WIDTH + x] = c as u8;
            if dither == Dither::FloydSteinberg {
                let shown = colours[slot(x, c)];
                let error: [f32; 3] = std::array::from_fn(|k| (p[k] - f32::from(shown[k])) * strength);
                // 7/16 on, 3/16 below behind, 5/16 below, 1/16 below on.
                ahead = error.map(|e| e * 7.0 / 16.0);
                for k in 0..3 {
                    next[x][k] += error[k] * 3.0 / 16.0;
                    next[x + 1][k] += error[k] * 5.0 / 16.0;
                    next[x + 2][k] += error[k] / 16.0;
                }
            }
        }
        carried = next;
        lines.push(colours);
    }
    (values, lines)
}

/// The colours shown.
pub fn render(values: &[u8], lines: &[[[u8; 3]; 48]]) -> Vec<u8> {
    let mut rgba = Vec::with_capacity(values.len() * 4);
    for (y, row) in values.chunks(WIDTH).enumerate() {
        for (x, &c) in row.iter().enumerate() {
            let [r, g, b] = lines.get(y).map_or([0, 0, 0], |l| l[slot(x, usize::from(c))]);
            rgba.extend([r, g, b, 255]);
        }
    }
    rgba
}

/// The palette words of every line, 199 times 48.
fn palette_words(c: &Converted) -> Vec<u16> {
    let levels = if c.levels == Levels::St { Levels::St } else { Levels::Ste };
    c.lines.iter().flat_map(|line| line.iter().map(move |&colour| st_word(colour, levels))).collect()
}

/// An uncompressed .SPU.
pub fn to_spu(c: &Converted) -> Vec<u8> {
    let mut out = crate::st_planar::screen_bytes(c);
    out.resize(32000, 0);
    out.extend(palette_words(c).iter().flat_map(|w| w.to_be_bytes()));
    out
}

/// Packs bytes as .SPC does: runs of three or more as a repeat (up to 130), the rest copied (up to 128).
fn pack_spc(data: &[u8], out: &mut Vec<u8>) {
    let mut at = 0;
    while at < data.len() {
        let mut run = 1;
        while at + run < data.len() && run < 130 && data[at + run] == data[at] {
            run += 1;
        }
        if run >= 3 {
            out.push((258 - run) as u8);
            out.push(data[at]);
            at += run;
            continue;
        }
        let start = at;
        while at < data.len() && at - start < 128 {
            if at + 2 < data.len() && data[at] == data[at + 1] && data[at] == data[at + 2] {
                break;
            }
            at += 1;
        }
        out.push((at - start - 1) as u8);
        out.extend_from_slice(&data[start..at]);
    }
}

/// A packed .SPC.
pub fn to_spc(c: &Converted) -> Vec<u8> {
    let screen = crate::st_planar::screen_bytes(c);
    let mut bitmap = Vec::new();
    for plane in 0..4 {
        // Each plane's words from the second line down, packed on its own.
        let words: Vec<u8> = (0..(32000 - 160) / 8).flat_map(|i| [screen[160 + i * 8 + plane * 2], screen[160 + i * 8 + plane * 2 + 1]]).collect();
        pack_spc(&words, &mut bitmap);
    }
    if bitmap.len() % 2 == 1 {
        bitmap.push(0);
    }
    let mut palettes = Vec::new();
    for palette in palette_words(c).chunks(16) {
        let mut bits = 0u16;
        for (i, &w) in palette.iter().enumerate().take(15) {
            if w != 0 {
                bits |= 1 << i;
            }
        }
        palettes.extend(bits.to_be_bytes());
        for &w in palette.iter().take(15).filter(|&&w| w != 0) {
            palettes.extend(w.to_be_bytes());
        }
    }
    let mut out = b"SP\0\0".to_vec();
    out.extend((bitmap.len() as u32).to_be_bytes());
    out.extend((palettes.len() as u32).to_be_bytes());
    out.extend(bitmap);
    out.extend(palettes);
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::quantise::Image;
    use crate::st_planar::{Machine, Screen, Settings, convert};

    #[test]
    fn colours_turn_where_the_display_routine_changes_them() {
        // Colour 0 turns at 1 and 161, colour 1 at 5 and 165, colour 15 at 145 and 305.
        assert_eq!((slot(0, 0), slot(1, 0), slot(160, 0), slot(161, 0)), (0, 16, 16, 32));
        assert_eq!((slot(4, 1), slot(5, 1), slot(164, 1), slot(165, 1)), (1, 17, 17, 33));
        assert_eq!((slot(144, 15), slot(145, 15), slot(304, 15), slot(305, 15)), (15, 31, 31, 47));
        // Every pixel can show 16 different colours, and each of the 48 shows somewhere.
        let mut seen = [false; 48];
        for x in 0..WIDTH {
            let mut here: Vec<usize> = (0..16).map(|c| slot(x, c)).collect();
            here.dedup();
            assert_eq!(here.len(), 16);
            here.iter().for_each(|&s| seen[s] = true);
        }
        assert!(seen.iter().all(|&s| s));
        for s in 0..48 {
            assert!(span(s).all(|x| slot(x, s % 16) == s));
        }
    }

    #[test]
    fn spc_packing_reads_back_as_recoil_unpacks_it() {
        let data: Vec<u8> = (0..1000).map(|i| if i % 300 < 200 { 7 } else { (i * 13) as u8 }).collect();
        let mut packed = Vec::new();
        pack_spc(&data, &mut packed);
        let mut out = Vec::new();
        let mut at = 0;
        while at < packed.len() {
            let b = packed[at];
            at += 1;
            if b < 128 {
                out.extend_from_slice(&packed[at..at + usize::from(b) + 1]);
                at += usize::from(b) + 1;
            } else {
                out.extend(std::iter::repeat_n(packed[at], 258 - usize::from(b)));
                at += 1;
            }
        }
        assert_eq!(out, data);
    }

    /// Bands of colour across and down, more than 16 to a line.
    pub fn bands() -> Image {
        let (w, h) = (320, 199);
        let mut rgba = Vec::new();
        for y in 0..h {
            for x in 0..w {
                let band = x / 20;
                let c = [(band * 37 % 8) as u8 * 36, ((band + y / 25) * 53 % 8) as u8 * 36, ((band * 3 + y / 40) % 8) as u8 * 36];
                rgba.extend([c[0], c[1], c[2], 255]);
            }
        }
        Image { width: w, height: h, rgba }
    }

    #[test]
    fn spectrum_goes_through_spu_and_spc_and_back_unchanged() {
        for machine in [Machine::St, Machine::Ste] {
            let s = Settings { machine, screen: Screen::Spectrum, dither: Dither::None, fit: crate::quantise::Fit::Stretch, filter: crate::quantise::Filter::Nearest, ..Settings::default() };
            let c = convert(&bands(), &s);
            assert_eq!((c.width, c.height, c.lines.len()), (320, 199, 199));
            assert!(c.lines.iter().all(|l| (0..48).filter(|s| fixed(*s)).all(|s| l[s] == [0, 0, 0])));
            let spu = to_spu(&c);
            assert_eq!(spu.len(), 51104);
            let spc = to_spc(&c);
            assert!(spc.len() < spu.len());
            for (name, file) in [("t.spu", &spu), ("t.spc", &spc)] {
                let format = crate::pictures::detect(name, file).unwrap();
                let picture = crate::pictures::decode_named(format, name, file).unwrap();
                assert_eq!((picture.width, picture.height), (320, 199));
                if machine == Machine::Ste && !c.lines.iter().flatten().any(|&k| st_word(k, Levels::Ste) & 0x888 != 0) {
                    continue;
                }
                assert_eq!(picture.rgba, c.rgba, "{machine:?} {name}");
            }
            // Sixteen bands a line, each of the ST's own colours, come out close.
            if machine == Machine::St {
                assert!(c.error < 0.01, "{}", c.error);
            }
        }
    }

    #[test]
    fn spectrum_comes_closer_than_sixteen_colours() {
        let mut rgba = Vec::new();
        for y in 0..199 {
            for x in 0..320 {
                let (fx, fy) = (x as f32 / 320.0, y as f32 / 199.0);
                rgba.extend([(255.0 * fx) as u8, (255.0 * fy) as u8, (255.0 * (1.0 - fx) * (0.5 + 0.5 * (fy * 6.0).sin())) as u8, 255]);
            }
        }
        let image = Image { width: 320, height: 199, rgba };
        // The best any picture of the ST's 512 colours could do, without dithering: each pixel its nearest.
        let pixels: Vec<[f32; 3]> = image.rgba.chunks(4).map(|p| [p[0], p[1], p[2]].map(f32::from)).collect();
        let nearest: Vec<u8> = pixels.iter().flat_map(|p| {
            let [r, g, b] = Levels::St.snap_colour(*p);
            [r, g, b, 255]
        }).collect();
        eprintln!("Every pixel its nearest of the 512: {:.4}", quantise::average_error(&pixels, &nearest));
        for dither in Dither::ALL {
            let low = convert(&image, &Settings { width: 320, height: 199, dither, ..Settings::default() });
            let started = std::time::Instant::now();
            let spectrum = convert(&image, &Settings { screen: Screen::Spectrum, dither, ..Settings::default() });
            eprintln!("{dither:?}: 16 colours {:.4}, Spectrum 512 {:.4} (Oklab), in {:?}", low.error, spectrum.error, started.elapsed());
            assert!(spectrum.error < low.error * 0.8, "{dither:?}");
        }
    }
}
