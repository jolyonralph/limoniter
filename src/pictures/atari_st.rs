//! Atari ST pictures: Degas and Degas Elite (.pi1-3, and .pc1-3 packed), NEOchrome (.neo), and Spectrum 512
//! (.spu, and .spc and .sps packed).
//!
//! Both keep the ST's screen as it is, 32000 bytes, with a header before it:
//! - Degas: a word for the resolution (0 low, 320x200 in 16 colours; 1 medium, 640x200 in 4; 2 high, 640x400 in
//!   black and white), 16 palette words, the screen. Degas Elite adds 32 bytes of colour cycling after it: four
//!   ranges' left and right colours, directions (0 left, 1 off, 2 right) and delays (128 less the 60ths of a second
//!   a step takes). Packed pictures set the resolution word's top bit and pack each row of each plane in turn with
//!   PackBits, the cycling at the very end.
//! - NEOchrome: a flag word, the resolution, 16 palette words, a file name, its colour cycling (the range in one
//!   word, its top bit on when it cycles; the speed and direction in the next), then the screen at byte 128.
//!
//! Spectrum 512 changes the palette 48 times a line, three palettes of 16, and is read as `st_spectrum` describes.
//!
//! A palette word is $0RGB, three bits a level on the ST; the STE added a fourth, as the level's lowest bit kept
//! at the top of each nibble. The screen interleaves its bit planes a 16-bit word at a time: for each 16 pixels, a
//! word of each plane.

use super::Picture;
use super::ilbm::Cycling;

const SCREEN: usize = 32000;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Kind {
    Degas,
    DegasPacked,
    Neochrome,
    SpectrumPlain,
    SpectrumPacked,
    SpectrumSmooshed,
}

fn be16(b: &[u8], at: usize) -> u16 {
    u16::from_be_bytes([b[at], b[at + 1]])
}

fn kind(ending: &str, bytes: &[u8]) -> Option<Kind> {
    if bytes.len() < 34 {
        return None;
    }
    if bytes.len() == 51104 && matches!(ending, "spu" | "") {
        return Some(Kind::SpectrumPlain);
    }
    // Picture files read with no name are tried as packed Degas: a Spectrum header can't be one of those.
    if bytes.starts_with(b"SP\0\0") {
        return match ending {
            "spc" | "pc1" => Some(Kind::SpectrumPacked),
            "sps" => Some(Kind::SpectrumSmooshed),
            _ => None,
        };
    }
    let first = be16(bytes, 0);
    match bytes.len() {
        32034 | 32066 if first <= 2 => Some(Kind::Degas),
        32128 if first == 0 && be16(bytes, 2) <= 2 => Some(Kind::Neochrome),
        _ if matches!(ending, "pc1" | "pc2" | "pc3") && first & 0x8000 != 0 && first & 0x7FFF <= 2 => Some(Kind::DegasPacked),
        _ => None,
    }
}

pub fn looks_like(ending: &str, bytes: &[u8]) -> bool {
    kind(ending, bytes).is_some()
}

/// The 16 palette words as colours. Only an STE picture uses the fourth bit; an ST one's 3-bit levels are spread
/// to the full range.
fn palette(words: &[u8]) -> (Vec<[u8; 3]>, bool) {
    let words: Vec<u16> = (0..16).map(|i| be16(words, i * 2)).collect();
    let ste = words.iter().any(|w| w & 0x888 != 0);
    let level = |n: u16| -> u8 {
        if ste {
            (((n & 7) << 1 | (n >> 3) & 1) * 17) as u8
        } else {
            (((n & 7) * 255 + 3) / 7) as u8
        }
    };
    (words.iter().map(|w| [level(w >> 8 & 15), level(w >> 4 & 15), level(w & 15)]).collect(), ste)
}

/// The screen's pixels as palette values, from the ST's interleaved planes.
fn interleaved(screen: &[u8], width: usize, height: usize, planes: usize) -> Vec<u8> {
    let mut values = vec![0u8; width * height];
    for y in 0..height {
        for group in 0..width / 16 {
            // A row is 160 bytes in low and medium resolution, 80 in high.
            let at = y * (width * planes / 8) + group * planes * 2;
            for plane in 0..planes {
                let word = be16(screen, at + plane * 2);
                for i in 0..16 {
                    if word & (0x8000 >> i) != 0 {
                        values[y * width + group * 16 + i] |= 1 << plane;
                    }
                }
            }
        }
    }
    values
}

/// Degas Elite's packed screen, each row of each plane in turn, into the ST's own interleaved layout.
fn unpack_degas(data: &[u8], width: usize, height: usize, planes: usize) -> (Vec<u8>, usize, bool) {
    let row = width / 8;
    let mut planar = Vec::with_capacity(SCREEN);
    let mut at = 0;
    while planar.len() < SCREEN && at < data.len() {
        let n = data[at] as i8;
        at += 1;
        match n {
            0..=127 => {
                let end = (at + n as usize + 1).min(data.len());
                planar.extend_from_slice(&data[at..end]);
                at = end;
            }
            -127..=-1 => {
                if let Some(&b) = data.get(at) {
                    planar.extend(std::iter::repeat_n(b, (1 - i16::from(n)) as usize));
                }
                at += 1;
            }
            -128 => {}
        }
    }
    let short = planar.len() < SCREEN;
    planar.resize(SCREEN, 0);
    let mut screen = vec![0u8; SCREEN];
    for y in 0..height {
        for plane in 0..planes {
            for byte in 0..row {
                let from = planar[(y * planes + plane) * row + byte];
                // Byte `byte` of the plane's row is in word byte/2 of it, which is in group byte/2.
                screen[y * row * planes + (byte / 2) * planes * 2 + plane * 2 + byte % 2] = from;
            }
        }
    }
    (screen, at, short)
}

pub fn decode(ending: &str, bytes: &[u8]) -> Result<Picture, String> {
    let kind = kind(ending, bytes).ok_or(crate::i18n::tr("This isn't an Atari ST picture Limoniter knows."))?;
    if matches!(kind, Kind::SpectrumPlain | Kind::SpectrumPacked | Kind::SpectrumSmooshed) {
        return decode_spectrum(kind, bytes);
    }
    let (resolution, palette_at) = match kind {
        Kind::Neochrome => (be16(bytes, 2), 4),
        _ => (be16(bytes, 0) & 3, 2),
    };
    let (width, height, planes) = match resolution {
        0 => (320, 200, 4),
        1 => (640, 200, 2),
        _ => (640, 400, 1),
    };
    let (mut colours, ste) = palette(&bytes[palette_at..palette_at + 32]);
    if planes == 1 {
        // The ST's monochrome monitor: set bits are black.
        colours = vec![[255, 255, 255], [0, 0, 0]];
    }
    let mut short = false;
    let (screen, cycling_at) = match kind {
        Kind::Degas => (bytes[34..34 + SCREEN].to_vec(), (bytes.len() == 32066).then_some(34 + SCREEN)),
        Kind::Neochrome => (bytes[128..128 + SCREEN].to_vec(), None),
        Kind::DegasPacked => {
            let (screen, used, ended_early) = unpack_degas(&bytes[34..], width, height, planes);
            short = ended_early;
            (screen, (bytes.len() >= 34 + used + 32).then_some(bytes.len() - 32))
        }
        Kind::SpectrumPlain | Kind::SpectrumPacked | Kind::SpectrumSmooshed => unreachable!("read by decode_spectrum"),
    };
    let values = interleaved(&screen, width, height, planes);
    let mut picture = Picture::new(width, height);
    for (pixel, &value) in picture.rgba.as_chunks_mut::<4>().0.iter_mut().zip(&values) {
        let [r, g, b] = colours[usize::from(value)];
        pixel.copy_from_slice(&[r, g, b, 255]);
    }
    // Medium resolution's pixels are half as wide as they're tall.
    picture.aspect = if resolution == 1 { 0.5 } else { 1.0 };

    // Colour cycling: Degas Elite's four ranges, or NEOchrome's one.
    let mut ranges = Vec::new();
    if let Some(at) = cycling_at {
        for i in 0..4 {
            let word = |table: usize| be16(bytes, at + table * 8 + i * 2);
            let (left, right, direction, delay) = (word(0), word(1), word(2), word(3));
            let sixtieths = 128i32 - i32::from(delay as i16);
            if direction != 1 && sixtieths > 0 && left < 16 && right < 16 {
                ranges.push((usize::from(left), usize::from(right), 60.0 / sixtieths as f32, direction == 0));
            }
        }
    }
    if kind == Kind::Neochrome {
        let limits = be16(bytes, 48);
        let speed = be16(bytes, 50);
        let vblanks = speed & 0xFF;
        if limits & 0x8000 != 0 && vblanks > 0 {
            ranges.push((usize::from(limits >> 4 & 15), usize::from(limits & 15), 60.0 / f32::from(vblanks), speed & 0x8000 != 0));
        }
    }
    picture.cycling = if planes > 1 { Cycling::plain(width, values, colours, &ranges) } else { None };

    let format = match kind {
        Kind::Degas if bytes.len() == 32066 => "Degas Elite",
        Kind::Degas => "Degas",
        Kind::DegasPacked => "Degas Elite, packed",
        Kind::Neochrome => "NEOchrome",
        _ => "Spectrum 512",
    };
    picture.note("Size", format!("{width}\u{D7}{height}"));
    picture.note(
        "Colours",
        match planes {
            4 => "16 colours (low resolution)",
            2 => "4 colours (medium resolution)",
            _ => "Black and white (high resolution)",
        },
    );
    picture.note("Saved by", format);
    if ste && planes > 1 {
        picture.note("Palette", "4 bits a level (STE)");
    }
    if kind == Kind::Neochrome {
        let name: String = bytes[36..48].iter().take_while(|&&b| b != 0).map(|&b| char::from(b)).collect();
        // An unnamed picture has spaces and a dot there.
        if name.chars().any(|c| c.is_ascii_alphanumeric()) {
            picture.note("Name", name.trim());
        }
    }
    if let Some(cycling) = &picture.cycling {
        picture.note("Colour cycling", format!("{} range(s) on; play it to see them cycle", cycling.ranges()));
    }
    if short {
        picture.note("Damaged", "The packed screen ends early; the rest is shown in colour 0");
    }
    Ok(picture)
}

/// A run-length stream as Spectrum's packed files use it: a command, then bytes copied or one repeated.
struct Runs<'a> {
    data: &'a [u8],
    at: usize,
    count: usize,
    repeat: Option<u8>,
    smooshed: bool,
}

impl Runs<'_> {
    fn next(&mut self) -> Option<u8> {
        while self.count == 0 {
            let b = *self.data.get(self.at)?;
            self.at += 1;
            // .SPC: under 128 copies b + 1, otherwise repeats 258 - b. .SPS: under 128 repeats b + 3, otherwise
            // copies b - 127.
            let repeats = (b >= 128) != self.smooshed;
            self.count = match (self.smooshed, repeats) {
                (false, false) => usize::from(b) + 1,
                (false, true) => 258 - usize::from(b),
                (true, true) => usize::from(b) + 3,
                (true, false) => usize::from(b) - 127,
            };
            self.repeat = if repeats {
                let v = *self.data.get(self.at)?;
                self.at += 1;
                Some(v)
            } else {
                None
            };
        }
        self.count -= 1;
        match self.repeat {
            Some(v) => Some(v),
            None => {
                let v = *self.data.get(self.at)?;
                self.at += 1;
                Some(v)
            }
        }
    }
}

/// Each plane's words from the second line down, a plane after another, into the screen.
fn unpack_plane_words(runs: &mut Runs, screen: &mut [u8]) -> Option<()> {
    for plane in 0..4 {
        let mut at = 160 + plane * 2;
        while at < SCREEN {
            screen[at] = runs.next()?;
            screen[at + 1] = runs.next()?;
            at += 8;
        }
    }
    Some(())
}

/// Spectrum 512's screen and 199 lines of 48 palette words, unpacked: the .SPU's layout.
fn unpack_spectrum(kind: Kind, bytes: &[u8]) -> Option<Vec<u8>> {
    if kind == Kind::SpectrumPlain {
        return Some(bytes.to_vec());
    }
    let mut out = vec![0u8; 51104];
    let palettes_at = 12usize.checked_add(u32::from_be_bytes([bytes[4], bytes[5], bytes[6], bytes[7]]) as usize)?;
    let mut runs = Runs { data: bytes.get(..palettes_at.min(bytes.len()))?, at: 12, count: 0, repeat: None, smooshed: kind == Kind::SpectrumSmooshed };
    if kind == Kind::SpectrumPacked || bytes.last()? & 1 == 1 {
        unpack_plane_words(&mut runs, &mut out)?;
    } else {
        // .SPS's other layout: each plane's bytes a column at a time, down the screen.
        for plane in 0..4 {
            for x in 0..40 {
                let mut at = 160 + ((x & !1) << 2) + plane * 2 + (x & 1);
                while at < SCREEN {
                    out[at] = runs.next()?;
                    at += 160;
                }
            }
        }
    }
    let mut at = SCREEN;
    let data = bytes.get(palettes_at..)?;
    if kind == Kind::SpectrumPacked {
        let mut from = 0;
        while at < out.len() {
            let bits = u16::from(*data.get(from)? & 0x7F) << 8 | u16::from(*data.get(from + 1)?);
            from += 2;
            for i in 0..16 {
                if bits >> i & 1 != 0 {
                    out[at] = *data.get(from)?;
                    out[at + 1] = *data.get(from + 1)?;
                    from += 2;
                }
                at += 2;
            }
        }
    } else {
        // Bits, the first the highest: 14 saying which of colours 1 to 14 are there, then 9 (RRRGGGBBB) for each.
        let mut bit = 0usize;
        let mut read = |count: usize| -> Option<u16> {
            let mut v = 0u16;
            for _ in 0..count {
                let byte = *data.get(bit / 8)?;
                v = v << 1 | u16::from(byte >> (7 - bit % 8) & 1);
                bit += 1;
            }
            Some(v)
        };
        while at < out.len() {
            let there = read(14)? << 1;
            for i in (0..16).rev() {
                let rgb = if there >> i & 1 != 0 { read(9)? } else { 0 };
                out[at] = (rgb >> 6) as u8;
                out[at + 1] = ((rgb & 0x3F) + (rgb & 0x38)) as u8;
                at += 2;
            }
        }
    }
    Some(out)
}

fn decode_spectrum(kind: Kind, bytes: &[u8]) -> Result<Picture, String> {
    let file = unpack_spectrum(kind, bytes).ok_or(crate::i18n::tr("This Spectrum 512 picture ends early or is damaged."))?;
    let words: Vec<u16> = file[SCREEN..].chunks(2).map(|w| u16::from_be_bytes([w[0], w[1]])).collect();
    let ste = words.iter().any(|w| w & 0x888 != 0);
    let colours: Vec<[u8; 3]> = words.iter().map(|&w| crate::st_planar::colour_of_word(w, ste)).collect();
    let values = interleaved(&file[160..SCREEN], 320, 199, 4);
    let mut picture = Picture::new(320, 199);
    for y in 0..199 {
        for x in 0..320 {
            let c = usize::from(values[y * 320 + x]);
            picture.set(x, y, colours[y * 48 + crate::st_spectrum::slot(x, c)]);
        }
    }
    picture.note("Size", "320\u{D7}199");
    picture.note("Colours", if ste { "48 a line from 4096 (STE)" } else { "48 a line from 512" });
    picture.note(
        "Saved by",
        match kind {
            Kind::SpectrumPacked => "Spectrum 512, packed",
            Kind::SpectrumSmooshed => "Spectrum 512, smooshed",
            _ => "Spectrum 512",
        },
    );
    Ok(picture)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn degas(resolution: u16, palette: &[u16]) -> Vec<u8> {
        let mut file = resolution.to_be_bytes().to_vec();
        for i in 0..16 {
            file.extend(palette.get(i).copied().unwrap_or(0).to_be_bytes());
        }
        file
    }

    fn pixel(picture: &Picture, x: usize, y: usize) -> [u8; 3] {
        let at = (y * picture.width + x) * 4;
        [picture.rgba[at], picture.rgba[at + 1], picture.rgba[at + 2]]
    }

    #[test]
    fn low_resolution_planes_interleave_a_word_at_a_time() {
        let mut file = degas(0, &[0x000, 0x700, 0x070, 0x007]);
        file.resize(32034, 0);
        // First 16 pixels: plane 0 word = 8000 (pixel 0), plane 1 word = 4000 (pixel 1); so pixel 0 is 1, pixel 1 is 2.
        file[34] = 0x80;
        file[36] = 0x40;
        // The second group of 16 starts 8 bytes on: pixel 16 has plane 0 and 1 set: colour 3.
        file[42] = 0x80;
        file[44] = 0x80;
        let picture = decode("pi1", &file).unwrap();
        assert_eq!(pixel(&picture, 0, 0), [255, 0, 0], "3-bit ST levels are spread to the full range");
        assert_eq!(pixel(&picture, 1, 0), [0, 255, 0]);
        assert_eq!(pixel(&picture, 16, 0), [0, 0, 255]);
        assert_eq!(pixel(&picture, 2, 0), [0, 0, 0]);
    }

    #[test]
    fn a_packed_picture_unpacks_by_plane_rows_and_finds_its_cycling() {
        let mut file = degas(0x8000, &[0x000, 0x111, 0x222, 0x333]);
        // Row 0: plane 0 is 40 bytes of FF (a run), planes 1-3 40 bytes of 00; then 199 more rows of zeros.
        file.extend([(-39i8) as u8, 0xFF]);
        for _ in 0..(3 + 199 * 4) {
            file.extend([(-39i8) as u8, 0x00]);
        }
        // Cycling: range 1 colours 0-3, moving right, a step every 2/60 s.
        let mut cycling = vec![0u8; 32];
        cycling[1] = 0; // left[0] = 0
        cycling[9] = 3; // right[0] = 3
        cycling[16..24].copy_from_slice(&[0, 2, 0, 1, 0, 1, 0, 1]); // directions: right, off, off, off
        cycling[25] = 126; // delay[0]: 128 - 2
        file.extend(cycling);
        let picture = decode("pc1", &file).unwrap();
        assert_eq!(pixel(&picture, 0, 0), [36, 36, 36], "colour 1");
        assert_eq!(pixel(&picture, 319, 0), [36, 36, 36]);
        assert_eq!(pixel(&picture, 0, 1), [0, 0, 0]);
        assert_eq!(picture.cycling.as_ref().map(|c| c.ranges()), Some(1));
    }

    #[test]
    fn neochrome_has_its_screen_at_128_and_mono_is_black_on_white() {
        let mut file = vec![0, 0, 0, 2];
        file.resize(32128, 0);
        file[128] = 0x80;
        let picture = decode("neo", &file).unwrap();
        assert_eq!((picture.width, picture.height), (640, 400));
        assert_eq!(pixel(&picture, 0, 0), [0, 0, 0]);
        assert_eq!(pixel(&picture, 1, 0), [255, 255, 255]);
    }
}
