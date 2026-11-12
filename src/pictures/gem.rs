//! GEM bit images (.img), from Digital Research's GEM and the Atari ST's: a header of big-endian words (version,
//! header length in words, planes, pattern length, a pixel's width and height in microns, width and height in
//! pixels), then the rows packed. Each row is its planes in turn, each a whole number of bytes, packed as:
//! - 00 00 FF n: the row that follows is to be used n times.
//! - 00 n, then a pattern of the pattern length: the pattern n times.
//! - 80 n, then n bytes: those bytes.
//! - Any other byte: its low seven bits' worth of bytes, all set if its top bit is, all clear if not.
//!
//! A colour picture may carry its palette in an XIMG header extension, levels from 0 to 1000; without one, the
//! colours are the Atari ST desktop's. Set bits in a one-plane picture are black.

use super::Picture;

fn be16(b: &[u8], at: usize) -> usize {
    usize::from(u16::from_be_bytes([b[at], b[at + 1]]))
}

struct Header {
    words: usize,
    planes: usize,
    pattern: usize,
    micron_w: usize,
    micron_h: usize,
    width: usize,
    height: usize,
}

fn header(bytes: &[u8]) -> Option<Header> {
    if bytes.len() < 16 {
        return None;
    }
    let h = Header { words: be16(bytes, 2), planes: be16(bytes, 4), pattern: be16(bytes, 6), micron_w: be16(bytes, 8), micron_h: be16(bytes, 10), width: be16(bytes, 12), height: be16(bytes, 14) };
    let sane = matches!(be16(bytes, 0), 1..=3) && (8..=1024).contains(&h.words) && matches!(h.planes, 1..=8) && (1..=8).contains(&h.pattern) && h.width > 0 && h.height > 0 && h.width <= 16384 && h.height <= 16384;
    (sane && bytes.len() > h.words * 2).then_some(h)
}

pub fn looks_like(ending: &str, bytes: &[u8]) -> bool {
    matches!(ending, "img" | "ximg") && header(bytes).is_some()
}

/// The Atari ST desktop's colours, as the VDI's pens are laid on the hardware's colour numbers.
const DESKTOP: [[u8; 3]; 16] = [
    [255, 255, 255],
    [255, 0, 0],
    [0, 255, 0],
    [255, 255, 0],
    [0, 0, 255],
    [255, 0, 255],
    [0, 255, 255],
    [182, 182, 182],
    [109, 109, 109],
    [255, 109, 109],
    [109, 255, 109],
    [255, 255, 109],
    [109, 109, 255],
    [255, 109, 255],
    [109, 255, 255],
    [0, 0, 0],
];

pub fn decode(bytes: &[u8]) -> Result<Picture, String> {
    let h = header(bytes).ok_or("This isn't a GEM bit image.")?;
    let line = h.width.div_ceil(8);
    let mut planes_data = vec![0u8; h.height * h.planes * line];
    let mut at = h.words * 2;
    let mut y = 0;
    let mut short = false;
    let next = |at: &mut usize| -> Option<u8> {
        let b = bytes.get(*at).copied();
        *at += 1;
        b
    };
    'rows: while y < h.height {
        let mut repeat = 1;
        let mut row = vec![0u8; h.planes * line];
        for plane in 0..h.planes {
            let out = &mut row[plane * line..(plane + 1) * line];
            let mut x = 0;
            while x < line {
                let Some(b) = next(&mut at) else {
                    short = true;
                    break 'rows;
                };
                match b {
                    0 => {
                        let n = next(&mut at).unwrap_or(0);
                        if n == 0 {
                            // 00 00 FF n: the row repeated, said before it starts.
                            let _ = next(&mut at);
                            repeat = usize::from(next(&mut at).unwrap_or(1)).max(1);
                            continue;
                        }
                        let pattern: Vec<u8> = (0..h.pattern).map(|_| next(&mut at).unwrap_or(0)).collect();
                        for _ in 0..n {
                            for &p in &pattern {
                                if x < line {
                                    out[x] = p;
                                    x += 1;
                                }
                            }
                        }
                    }
                    0x80 => {
                        let n = next(&mut at).unwrap_or(0);
                        for _ in 0..n {
                            let value = next(&mut at).unwrap_or(0);
                            if x < line {
                                out[x] = value;
                                x += 1;
                            }
                        }
                    }
                    _ => {
                        let fill = if b & 0x80 != 0 { 0xFF } else { 0 };
                        for _ in 0..(b & 0x7F) {
                            if x < line {
                                out[x] = fill;
                                x += 1;
                            }
                        }
                    }
                }
            }
        }
        for _ in 0..repeat {
            if y < h.height {
                planes_data[y * h.planes * line..(y + 1) * h.planes * line].copy_from_slice(&row);
                y += 1;
            }
        }
    }

    // XIMG: "XIMG", 0 for RGB, then each colour's red, green and blue from 0 to 1000.
    let colours = 1usize << h.planes;
    let ximg = h.words >= 11 + colours * 3 && &bytes[16..20] == b"XIMG" && be16(bytes, 20) == 0;
    let palette: Vec<[u8; 3]> = if ximg {
        (0..colours).map(|i| std::array::from_fn(|c| (be16(bytes, 22 + (i * 3 + c) * 2).min(1000) * 255 / 1000) as u8)).collect()
    } else {
        match h.planes {
            1 => vec![[255; 3], [0; 3]],
            2 => vec![DESKTOP[0], DESKTOP[1], DESKTOP[2], DESKTOP[15]],
            4 => DESKTOP.to_vec(),
            _ => (0..colours).map(|i| [(255 - i * 255 / (colours - 1)) as u8; 3]).collect(),
        }
    };
    let mut picture = Picture::new(h.width, h.height);
    for y in 0..h.height {
        let row = &planes_data[y * h.planes * line..(y + 1) * h.planes * line];
        for x in 0..h.width {
            let index = (0..h.planes).filter(|p| row[p * line + x / 8] & (0x80 >> (x % 8)) != 0).fold(0, |v, p| v | 1 << p);
            picture.set(x, y, palette.get(index).copied().unwrap_or([0, 0, 0]));
        }
    }
    if h.micron_w > 0 && h.micron_h > 0 {
        let aspect = h.micron_w as f32 / h.micron_h as f32;
        picture.aspect = if (0.2..=5.0).contains(&aspect) { aspect } else { 1.0 };
    }
    picture.note("Size", format!("{}\u{D7}{}", h.width, h.height));
    picture.note("Colours", if h.planes == 1 { "Black and white".to_string() } else { format!("{colours} colours, {} planes", h.planes) });
    if h.planes > 1 {
        picture.note("Palette", if ximg { "Its own (XIMG)" } else { "None saved; the Atari ST desktop's colours are used" });
    }
    if h.micron_w > 0 && h.micron_h > 0 {
        picture.note("Pixel", format!("{}\u{D7}{} microns", h.micron_w, h.micron_h));
    }
    if short {
        picture.note("Damaged", "The picture data ends early; the rest is shown in colour 0");
    }
    Ok(picture)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn header_for(planes: u16, width: u16, height: u16) -> Vec<u8> {
        [1u16, 8, planes, 2, 85, 85, width, height].iter().flat_map(|w| w.to_be_bytes()).collect()
    }

    #[test]
    fn runs_patterns_literals_and_repeated_rows() {
        let mut file = header_for(1, 32, 3);
        // Row 0, said twice: a solid run of 1 black byte, a pattern (AA 55) once, then a literal 0F.
        file.extend([0, 0, 0xFF, 2, 0x81, 0, 1, 0xAA, 0x55, 0x80, 1, 0x0F]);
        // Row 2: 4 white bytes.
        file.push(0x04);
        let picture = decode(&file).unwrap();
        let px = |x: usize, y: usize| picture.rgba[(y * 32 + x) * 4];
        assert_eq!(px(0, 0), 0, "set bits are black");
        assert_eq!(px(8, 0), 0, "AA: the first bit set");
        assert_eq!(px(9, 0), 255);
        assert_eq!(px(28, 0), 0, "0F: the low four bits set");
        assert_eq!(px(0, 1), 0, "the row used twice");
        assert_eq!(px(0, 2), 255);
        assert!(!picture.info.iter().any(|(label, _)| label == "Damaged"));
        assert!(looks_like("img", &file));
        assert!(!looks_like("txt", &file));
    }

    #[test]
    fn an_ximg_palette_is_used() {
        let mut file: Vec<u8> = [1u16, 11 + 4 * 3, 2, 1, 85, 85, 8, 1].iter().flat_map(|w| w.to_be_bytes()).collect();
        file.extend(b"XIMG");
        file.extend(0u16.to_be_bytes());
        for colour in [[0u16, 0, 0], [1000, 0, 0], [0, 1000, 0], [0, 0, 1000]] {
            file.extend(colour.iter().flat_map(|c| c.to_be_bytes()));
        }
        // Plane 0: 80 (pixel 0), plane 1: C0 (pixels 0 and 1): pixel 0 is 3, pixel 1 is 2.
        file.extend([0x80, 1, 0x80, 0x80, 1, 0xC0]);
        let picture = decode(&file).unwrap();
        assert_eq!(&picture.rgba[..3], &[0, 0, 255]);
        assert_eq!(&picture.rgba[4..7], &[0, 255, 0]);
    }
}
