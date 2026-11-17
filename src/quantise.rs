//! Turning a picture into fewer colours, shared by the Amiga and Atari ST converters: scaling it to the screen
//! (with the pixel's shape in mind), finding a palette (median cut refined by k-means in Oklab, a perceptual colour
//! space, with some colours kept as they are), and giving each pixel its colour, dithered if asked.

/// A picture to convert: red, green, blue and alpha, row by row.
#[derive(Clone, Debug, PartialEq)]
pub struct Image {
    pub width: usize,
    pub height: usize,
    pub rgba: Vec<u8>,
}

/// How the picture is made to fit the screen.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Fit {
    /// All of it, stretched to fill the screen.
    Stretch,
    /// All of it, keeping its shape, with black round it.
    Whole,
    /// Filling the screen, keeping its shape, its edges cut off.
    Fill,
}

impl Fit {
    pub const ALL: [Self; 3] = [Self::Fill, Self::Whole, Self::Stretch];

    pub fn name(self) -> &'static str {
        match self {
            Self::Stretch => "Stretch to fit",
            Self::Whole => "Fit inside, black round it",
            Self::Fill => "Fill, cutting the edges off",
        }
    }
}

/// How pixels are worked out when the picture is scaled.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Filter {
    Nearest,
    /// The average of the pixels each covers.
    Box,
    Bilinear,
    Lanczos,
}

impl Filter {
    pub const ALL: [Self; 4] = [Self::Lanczos, Self::Bilinear, Self::Box, Self::Nearest];

    pub fn name(self) -> &'static str {
        match self {
            Self::Nearest => "Nearest (sharp, blocky)",
            Self::Box => "Box (averaging)",
            Self::Bilinear => "Bilinear",
            Self::Lanczos => "Lanczos 3 (sharpest smooth)",
        }
    }

    /// How far the filter reaches, in pixels of the side it's measured on, and its weight at a distance.
    fn support(self) -> f32 {
        match self {
            Self::Nearest | Self::Box => 0.5,
            Self::Bilinear => 1.0,
            Self::Lanczos => 3.0,
        }
    }

    fn weight(self, x: f32) -> f32 {
        let x = x.abs();
        match self {
            Self::Nearest | Self::Box => {
                if x <= 0.5 {
                    1.0
                } else {
                    0.0
                }
            }
            Self::Bilinear => (1.0 - x).max(0.0),
            Self::Lanczos => {
                if x < 1e-6 {
                    1.0
                } else if x < 3.0 {
                    let p = std::f32::consts::PI * x;
                    3.0 * p.sin() * (p / 3.0).sin() / (p * p)
                } else {
                    0.0
                }
            }
        }
    }
}

/// How colours between the palette's are made.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Dither {
    None,
    /// A Bayer 8x8 pattern.
    Ordered,
    FloydSteinberg,
}

impl Dither {
    pub const ALL: [Self; 3] = [Self::None, Self::Ordered, Self::FloydSteinberg];

    pub fn name(self) -> &'static str {
        match self {
            Self::None => "None",
            Self::Ordered => "Ordered (Bayer)",
            Self::FloydSteinberg => "Floyd\u{2013}Steinberg",
        }
    }
}

// Colour spaces.

pub fn linear_of(level: f32) -> f32 {
    let c = (level / 255.0).clamp(0.0, 1.0);
    if c <= 0.04045 { c / 12.92 } else { ((c + 0.055) / 1.055).powf(2.4) }
}

pub fn srgb_of(linear: f32) -> f32 {
    let c = linear.clamp(0.0, 1.0);
    255.0 * if c <= 0.003_130_8 { c * 12.92 } else { 1.055 * c.powf(1.0 / 2.4) - 0.055 }
}

/// sRGB levels (0 to 255) in Björn Ottosson's Oklab, where distances follow how different colours look.
pub fn oklab(rgb: [f32; 3]) -> [f32; 3] {
    let [r, g, b] = rgb.map(linear_of);
    let l = (0.412_221_46 * r + 0.536_332_55 * g + 0.051_445_995 * b).cbrt();
    let m = (0.211_903_5 * r + 0.680_699_5 * g + 0.107_396_96 * b).cbrt();
    let s = (0.088_302_46 * r + 0.281_718_85 * g + 0.629_978_7 * b).cbrt();
    [
        0.210_454_26 * l + 0.793_617_8 * m - 0.004_072_047 * s,
        1.977_998_5 * l - 2.428_592_2 * m + 0.450_593_7 * s,
        0.025_904_037 * l + 0.782_771_77 * m - 0.808_675_77 * s,
    ]
}

pub fn distance(a: [f32; 3], b: [f32; 3]) -> f32 {
    (a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2) + (a[2] - b[2]).powi(2)
}

pub fn as_f32(c: [u8; 3]) -> [f32; 3] {
    c.map(f32::from)
}

// Scaling.

/// The picture scaled to a screen `tw` by `th` whose pixels are `aspect` as wide as they're tall, as sRGB levels,
/// its see-through parts on black.
pub fn scale(image: &Image, tw: usize, th: usize, aspect: f32, fit: Fit, filter: Filter) -> Vec<[f32; 3]> {
    let mut out = vec![[0.0f32; 3]; tw * th];
    if image.width == 0 || image.height == 0 {
        return out;
    }
    let (sw, sh) = (image.width as f32, image.height as f32);
    // The screen's shape in square units, and where on it and from where in the picture the picture goes.
    let (shown_w, shown_h) = (tw as f32 * aspect, th as f32);
    let (mut src, mut dst) = ([0.0, 0.0, sw, sh], [0usize, 0, tw, th]);
    match fit {
        Fit::Stretch => {}
        Fit::Whole => {
            let k = (shown_w / sw).min(shown_h / sh);
            let (w, h) = (((sw * k / aspect).round() as usize).clamp(1, tw), ((sh * k).round() as usize).clamp(1, th));
            dst = [(tw - w) / 2, (th - h) / 2, w, h];
        }
        Fit::Fill => {
            let k = (shown_w / sw).max(shown_h / sh);
            let (w, h) = (shown_w / k, shown_h / k);
            src = [(sw - w) / 2.0, (sh - h) / 2.0, w, h];
        }
    }
    // In linear light, so that averaging light and dark doesn't come out too dark.
    let linear: Vec<[f32; 3]> = image
        .rgba
        .as_chunks::<4>()
        .0
        .iter()
        .map(|p| {
            let alpha = f32::from(p[3]) / 255.0;
            [linear_of(f32::from(p[0])) * alpha, linear_of(f32::from(p[1])) * alpha, linear_of(f32::from(p[2])) * alpha]
        })
        .collect();
    let across = weights(filter, src[0], src[2], dst[2], image.width);
    let down = weights(filter, src[1], src[3], dst[3], image.height);
    // Across first, for each source row, then down.
    let mut wide = vec![[0.0f32; 3]; dst[2] * image.height];
    for y in 0..image.height {
        for (x, taps) in across.iter().enumerate() {
            let mut sum = [0.0f32; 3];
            for &(at, w) in taps {
                let p = linear[y * image.width + at];
                for c in 0..3 {
                    sum[c] += p[c] * w;
                }
            }
            wide[y * dst[2] + x] = sum;
        }
    }
    for (y, taps) in down.iter().enumerate() {
        for x in 0..dst[2] {
            let mut sum = [0.0f32; 3];
            for &(at, w) in taps {
                let p = wide[at * dst[2] + x];
                for c in 0..3 {
                    sum[c] += p[c] * w;
                }
            }
            out[(dst[1] + y) * tw + dst[0] + x] = sum.map(srgb_of);
        }
    }
    out
}

/// For each of `count` new pixels covering `length` old ones from `start`, the old pixels it takes and how much.
fn weights(filter: Filter, start: f32, length: f32, count: usize, size: usize) -> Vec<Vec<(usize, f32)>> {
    let step = length / count as f32;
    // Shrinking, the filter is widened to cover every old pixel; enlarging, it stays its own width.
    let stretch = step.max(1.0);
    let reach = filter.support() * stretch;
    (0..count)
        .map(|i| {
            let centre = start + (i as f32 + 0.5) * step;
            if filter == Filter::Nearest {
                return vec![((centre.floor().max(0.0) as usize).min(size - 1), 1.0)];
            }
            let first = (centre - reach - 0.5).floor().max(0.0) as usize;
            let last = ((centre + reach + 0.5).ceil() as usize).min(size);
            let mut taps: Vec<(usize, f32)> = (first..last).map(|at| (at, filter.weight((at as f32 + 0.5 - centre) / stretch))).filter(|&(_, w)| w != 0.0).collect();
            let total: f32 = taps.iter().map(|t| t.1).sum();
            if taps.is_empty() || total.abs() < 1e-6 {
                return vec![((centre.floor().max(0.0) as usize).min(size - 1), 1.0)];
            }
            for tap in &mut taps {
                tap.1 /= total;
            }
            taps
        })
        .collect()
}

// Finding a palette.

/// A colour seen in the picture, how often, and where in Oklab.
#[derive(Clone, Copy)]
struct Sample {
    rgb: [f32; 3],
    lab: [f32; 3],
    weight: f32,
}

/// The picture's colours, gathered five bits a level, each its colours' average.
fn samples(pixels: &[[f32; 3]]) -> Vec<Sample> {
    let mut bins: std::collections::HashMap<u32, ([f64; 3], u32)> = std::collections::HashMap::new();
    for p in pixels {
        let q = p.map(|v| (v.clamp(0.0, 255.0) as u32) >> 3);
        let entry = bins.entry(q[0] << 10 | q[1] << 5 | q[2]).or_insert(([0.0; 3], 0));
        for c in 0..3 {
            entry.0[c] += f64::from(p[c]);
        }
        entry.1 += 1;
    }
    let mut keys: Vec<u32> = bins.keys().copied().collect();
    keys.sort_unstable();
    keys.into_iter()
        .map(|k| {
            let (sum, n) = bins[&k];
            let rgb = sum.map(|v| (v / f64::from(n)) as f32);
            Sample { rgb, lab: oklab(rgb), weight: n as f32 }
        })
        .collect()
}

/// `count` colours for these samples by median cut in Oklab: the box with most spread split at its median.
fn median_cut(samples: &[Sample], count: usize) -> Vec<[f32; 3]> {
    if samples.is_empty() || count == 0 {
        return vec![[0.0; 3]; count];
    }
    let spread = |b: &[Sample]| -> (f32, usize) {
        let mut best = (0.0, 0);
        for axis in 0..3 {
            let lo = b.iter().map(|s| s.lab[axis]).fold(f32::MAX, f32::min);
            let hi = b.iter().map(|s| s.lab[axis]).fold(f32::MIN, f32::max);
            let weight: f32 = b.iter().map(|s| s.weight).sum();
            let score = (hi - lo) * weight.sqrt();
            if score > best.0 {
                best = (score, axis);
            }
        }
        best
    };
    let mut boxes: Vec<Vec<Sample>> = vec![samples.to_vec()];
    while boxes.len() < count {
        let Some((at, (score, axis))) = boxes.iter().enumerate().filter(|(_, b)| b.len() > 1).map(|(i, b)| (i, spread(b))).max_by(|a, b| a.1.0.total_cmp(&b.1.0)) else {
            break;
        };
        if score <= 0.0 {
            break;
        }
        let mut b = boxes.swap_remove(at);
        b.sort_by(|p, q| p.lab[axis].total_cmp(&q.lab[axis]));
        let total: f32 = b.iter().map(|s| s.weight).sum();
        let mut run = 0.0;
        let mut cut = 1;
        for (i, s) in b.iter().enumerate() {
            run += s.weight;
            if run >= total / 2.0 {
                cut = (i + 1).clamp(1, b.len() - 1);
                break;
            }
        }
        let rest = b.split_off(cut);
        boxes.push(b);
        boxes.push(rest);
    }
    let mut colours: Vec<[f32; 3]> = boxes
        .iter()
        .map(|b| {
            let total: f32 = b.iter().map(|s| s.weight).sum();
            let mut c = [0.0; 3];
            for s in b {
                for i in 0..3 {
                    c[i] += s.rgb[i] * s.weight / total;
                }
            }
            c
        })
        .collect();
    // Fewer colours in the picture than asked for: the rest black, unused.
    colours.resize(count, [0.0; 3]);
    colours
}

/// A palette for the picture: `slots` long, the given colours where a slot has one, the rest found from the picture.
/// `snap` turns a colour into the nearest the hardware's palette holds. With `half` (the Amiga's extra half-brite),
/// each colour also stands for the one it makes.
pub fn find_palette(pixels: &[[f32; 3]], slots: &[Option<[u8; 3]>], snap: &dyn Fn([f32; 3]) -> [u8; 3], half: Option<&dyn Fn([u8; 3]) -> [u8; 3]>) -> Vec<[u8; 3]> {
    let ehb = half.is_some();
    let samples = samples(pixels);
    let free: Vec<usize> = (0..slots.len()).filter(|&i| slots[i].is_none()).collect();
    let mut centres: Vec<[f32; 3]> = slots.iter().map(|s| s.map_or([0.0; 3], as_f32)).collect();
    // With extra half-brite, dark colours can be the half-bright ones of colours twice as bright, so the colours
    // are first found as if they were.
    let seeds = if ehb {
        samples
            .iter()
            .map(|s| {
                let rgb = if s.rgb.iter().all(|&v| v < 128.0) { s.rgb.map(|v| v * 2.0) } else { s.rgb };
                Sample { rgb, lab: oklab(rgb), weight: s.weight }
            })
            .collect()
    } else {
        samples.clone()
    };
    for (&slot, colour) in free.iter().zip(median_cut(&seeds, free.len())) {
        centres[slot] = colour;
    }
    // Lloyd's k-means: each sample to its nearest colour, each free colour moved to its samples' average.
    for _ in 0..16 {
        let mut candidates: Vec<[f32; 3]> = centres.iter().map(|&c| oklab(c)).collect();
        if let Some(half) = half {
            candidates.extend(centres.iter().map(|&c| oklab(as_f32(half(snap(c))))));
        }
        let n = centres.len();
        let mut sums = vec![([0.0f64; 3], 0.0f64); n];
        let mut worst = (0.0f32, None);
        for s in &samples {
            let (best, d) = nearest(&candidates, s.lab);
            if d * s.weight > worst.0 {
                worst = (d * s.weight, Some(s.rgb));
            }
            // A half-bright colour's sample counts towards its full colour at twice its levels.
            let (slot, k) = if best >= n { (best - n, 2.0) } else { (best, 1.0) };
            for c in 0..3 {
                sums[slot].0[c] += f64::from((s.rgb[c] * k).min(255.0) * s.weight);
            }
            sums[slot].1 += f64::from(s.weight);
        }
        let mut moved = 0.0f32;
        let mut reseeded = false;
        for &slot in &free {
            let (sum, w) = sums[slot];
            let next = if w > 0.0 {
                sum.map(|v| (v / w) as f32)
            } else if let (false, Some(colour)) = (reseeded, worst.1) {
                // An unused colour goes where the picture is worst served.
                reseeded = true;
                colour
            } else {
                centres[slot]
            };
            moved = moved.max(distance(next, centres[slot]));
            centres[slot] = next;
        }
        if moved < 0.25 {
            break;
        }
    }
    let mut palette: Vec<[u8; 3]> = centres.into_iter().map(snap).collect();
    // The found colours in order of brightness, round the kept ones.
    let mut found: Vec<[u8; 3]> = free.iter().map(|&i| palette[i]).collect();
    found.sort_by(|a, b| oklab(as_f32(*a))[0].total_cmp(&oklab(as_f32(*b))[0]));
    for (&slot, colour) in free.iter().zip(found) {
        palette[slot] = colour;
    }
    palette
}

/// The nearest of `colours` (in Oklab) and its distance squared.
pub fn nearest(colours: &[[f32; 3]], lab: [f32; 3]) -> (usize, f32) {
    let mut best = (0, f32::MAX);
    for (i, c) in colours.iter().enumerate() {
        let d = distance(*c, lab);
        if d < best.1 {
            best = (i, d);
        }
    }
    best
}

// Choosing each pixel's value.

/// Bayer's 8x8 ordered dither, thresholds from 0 to 63: each 2x2 block's place (0, 2 / 3, 1) at the finest
/// scale weighs most, so that neighbouring thresholds are far apart.
pub fn bayer(x: usize, y: usize) -> f32 {
    let mut v = 0;
    for bit in 0..3 {
        let (xb, yb) = ((x >> bit) & 1, (y >> bit) & 1);
        v += (((xb ^ yb) << 1) | yb) << (2 * (2 - bit));
    }
    v as f32
}

/// How far the ordered dither moves a level, for a palette of this many colours.
pub fn ordered_spread(colours: usize, strength: f32) -> f32 {
    strength * 255.0 / (colours.max(2) as f32).cbrt()
}

/// Each pixel's colour: the place in `colours` of the one it's given.
pub fn choose(pixels: &[[f32; 3]], width: usize, height: usize, colours: &[[u8; 3]], dither: Dither, strength: f32) -> Vec<u8> {
    let labs: Vec<[f32; 3]> = colours.iter().map(|&c| oklab(as_f32(c))).collect();
    let mut values = vec![0u8; width * height];
    match dither {
        Dither::None => {
            let mut known: std::collections::HashMap<[u32; 3], u8> = std::collections::HashMap::new();
            for (value, p) in values.iter_mut().zip(pixels) {
                *value = *known.entry(p.map(f32::to_bits)).or_insert_with(|| nearest(&labs, oklab(*p)).0 as u8);
            }
        }
        Dither::Ordered => {
            let spread = ordered_spread(colours.len(), strength);
            for y in 0..height {
                for x in 0..width {
                    let offset = ((bayer(x, y) + 0.5) / 64.0 - 0.5) * spread;
                    let p = pixels[y * width + x].map(|v| (v + offset).clamp(0.0, 255.0));
                    values[y * width + x] = nearest(&labs, oklab(p)).0 as u8;
                }
            }
        }
        Dither::FloydSteinberg => {
            let mut work: Vec<[f32; 3]> = pixels.to_vec();
            for y in 0..height {
                // Serpentine: every other row right to left, so the error doesn't drift one way.
                let backwards = y % 2 == 1;
                for i in 0..width {
                    let x = if backwards { width - 1 - i } else { i };
                    let p = work[y * width + x].map(|v| v.clamp(0.0, 255.0));
                    let chosen = nearest(&labs, oklab(p)).0;
                    values[y * width + x] = chosen as u8;
                    let error: [f32; 3] = std::array::from_fn(|c| (p[c] - f32::from(colours[chosen][c])) * strength);
                    diffuse(&mut work, width, height, x, y, backwards, error);
                }
            }
        }
    }
    values
}

/// Floyd and Steinberg's spread of a pixel's error: 7/16 on, 3/16 below behind, 5/16 below, 1/16 below on.
pub fn diffuse(work: &mut [[f32; 3]], width: usize, height: usize, x: usize, y: usize, backwards: bool, error: [f32; 3]) {
    let ahead = |dx: isize| if backwards { x as isize - dx } else { x as isize + dx };
    for (dx, dy, share) in [(1isize, 0usize, 7.0f32), (-1, 1, 3.0), (0, 1, 5.0), (1, 1, 1.0)] {
        let (nx, ny) = (ahead(dx), y + dy);
        if nx < 0 || nx as usize >= width || ny >= height {
            continue;
        }
        let at = &mut work[ny * width + nx as usize];
        for c in 0..3 {
            at[c] += error[c] * share / 16.0;
        }
    }
}

/// The average Oklab difference between pixels and what's shown for them.
pub fn average_error(pixels: &[[f32; 3]], rgba: &[u8]) -> f32 {
    if pixels.is_empty() {
        return 0.0;
    }
    let total: f64 = pixels.iter().zip(rgba.as_chunks::<4>().0).map(|(p, q)| f64::from(distance(oklab(*p), oklab([q[0], q[1], q[2]].map(f32::from))).sqrt())).sum();
    (total / pixels.len() as f64) as f32
}

/// A name usable as a label in C and assembler.
pub fn identifier(name: &str) -> String {
    let mut out: String = name.chars().map(|c| if c.is_ascii_alphanumeric() { c } else { '_' }).collect();
    if out.is_empty() || out.starts_with(|c: char| c.is_ascii_digit()) {
        out.insert(0, '_');
    }
    out
}

// Bringing pictures in.

/// A picture from a PNG, a JPEG, or any of the older machines' formats Limoniter reads.
pub fn read_picture(name: &str, bytes: &[u8]) -> Result<Image, String> {
    if bytes.starts_with(b"\x89PNG") {
        let (width, height, rgba) = crate::screen_designer::read_png(bytes)?;
        return Ok(Image { width, height, rgba });
    }
    if bytes.starts_with(&[0xFF, 0xD8, 0xFF]) {
        return read_jpeg(bytes);
    }
    let format = crate::pictures::detect(name, bytes).ok_or(crate::i18n::tr("It isn't a picture Limoniter knows: PNG, JPEG, IFF and older machines' pictures can be brought in."))?;
    let picture = crate::pictures::decode_named(format, name, bytes)?;
    Ok(Image { width: picture.width, height: picture.height, rgba: picture.rgba })
}

pub(crate) fn read_jpeg(bytes: &[u8]) -> Result<Image, String> {
    use zune_jpeg::zune_core::{bytestream::ZCursor, colorspace::ColorSpace, options::DecoderOptions};
    let options = DecoderOptions::default().jpeg_set_out_colorspace(ColorSpace::RGBA);
    let mut decoder = zune_jpeg::JpegDecoder::new_with_options(ZCursor::new(bytes), options);
    let rgba = decoder.decode().map_err(|e| crate::i18n::tf!("The JPEG couldn't be read: {e:?}"))?;
    let (width, height) = decoder.dimensions().ok_or(crate::i18n::tr("The JPEG has no size."))?;
    if rgba.len() < width * height * 4 {
        return Err(crate::i18n::tr("The JPEG ended early.").into());
    }
    Ok(Image { width, height, rgba })
}
