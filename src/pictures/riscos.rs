//! RISC OS sprite files (file type &FF9): a sprite area as it's saved, without its first word. Then three words:
//! how many sprites, where the first is (16, counted from the area's missing start) and where the free space
//! starts. Each sprite has a 44-byte header: the offset to the next, a 12-byte name, its width in words less one,
//! height less one, the first and last bits used in a row, where its image and mask are, and its screen mode.
//! Between the header and the image may be a palette, two words a colour, the first 0xBBGGRR00.
//!
//! A mode below 256 is an old screen mode number, which gives the bits a pixel and the pixel's shape. A larger odd
//! one is a new-style sprite type: the type in its top five bits (1, 2, 4, 8, 16 or 32 bits a pixel) and the dots
//! per inch across and down. Rows are words, the leftmost pixel in a word's lowest bits. An old sprite's mask is
//! like its image, a pixel showing where it isn't 0; a new one's is a bit a pixel.
//!
//! A file of one sprite is shown as that sprite; a file of many as a sheet of them all, each with its pixels made
//! square, on a see-through background.

use super::Picture;

fn le32(b: &[u8], at: usize) -> Option<usize> {
    b.get(at..at + 4).map(|w| u32::from_le_bytes([w[0], w[1], w[2], w[3]]) as usize)
}

/// The bits a pixel, and the pixel's shape as eigen factors (a pixel is 2^eig OS units), of the old screen modes.
fn old_mode(mode: usize) -> Option<(usize, u32, u32)> {
    const MODES: [(usize, u32, u32); 50] = [
        (1, 1, 2), (2, 2, 2), (4, 3, 2), (1, 1, 2), (1, 2, 2), (2, 3, 2), (1, 2, 2), (4, 2, 2), (2, 1, 2), (4, 2, 2),
        (8, 3, 2), (2, 1, 2), (4, 1, 2), (8, 2, 2), (4, 1, 2), (8, 1, 2), (4, 1, 2), (4, 1, 2), (1, 1, 1), (2, 1, 1),
        (4, 1, 1), (8, 1, 1), (4, 0, 1), (1, 1, 1), (8, 1, 2), (1, 1, 1), (2, 1, 1), (4, 1, 1), (8, 1, 1), (1, 1, 1),
        (2, 1, 1), (4, 1, 1), (8, 1, 1), (1, 1, 2), (2, 1, 2), (4, 1, 2), (8, 1, 2), (1, 1, 1), (2, 1, 1), (4, 1, 1),
        (8, 1, 1), (1, 1, 2), (2, 1, 2), (4, 1, 2), (1, 1, 2), (2, 1, 2), (4, 1, 2), (8, 2, 2), (4, 2, 1), (8, 2, 1),
    ];
    MODES.get(mode).copied()
}

/// The desktop's 16 colours, the default for a 4-bit sprite with no palette.
const DESKTOP: [[u8; 3]; 16] = [
    [0xFF, 0xFF, 0xFF],
    [0xDD, 0xDD, 0xDD],
    [0xBB, 0xBB, 0xBB],
    [0x99, 0x99, 0x99],
    [0x77, 0x77, 0x77],
    [0x55, 0x55, 0x55],
    [0x33, 0x33, 0x33],
    [0x00, 0x00, 0x00],
    [0x00, 0x44, 0x99],
    [0xEE, 0xEE, 0x00],
    [0x00, 0xCC, 0x00],
    [0xDD, 0x00, 0x00],
    [0xEE, 0xEE, 0xBB],
    [0x55, 0x88, 0x00],
    [0xFF, 0xBB, 0x00],
    [0x00, 0xBB, 0xFF],
];

fn default_palette(bits: usize) -> Vec<[u8; 3]> {
    match bits {
        1 => vec![DESKTOP[0], DESKTOP[7]],
        2 => vec![DESKTOP[0], DESKTOP[2], DESKTOP[4], DESKTOP[7]],
        4 => DESKTOP.to_vec(),
        // The VIDC's 256: each byte is B3 G3 G2 R3 B2 R2 T1 T0, the two tint bits the low two of every level.
        _ => (0..256usize)
            .map(|i| {
                let tint = i & 3;
                let level = |high: usize, low: usize| (((i >> high & 1) << 3 | (i >> low & 1) << 2 | tint) * 17) as u8;
                [level(4, 2), level(6, 5), level(7, 3)]
            })
            .collect(),
    }
}

/// One sprite, drawn with its pixels made square.
struct Sprite {
    name: String,
    width: usize,
    height: usize,
    rgba: Vec<u8>,
    detail: String,
}

fn sprite(data: &[u8], at: usize) -> Result<Sprite, String> {
    let word = |offset: usize| le32(data, at + offset).ok_or_else(|| "a sprite runs past the end of the file".to_string());
    let name: String = data.get(at + 4..at + 16).unwrap_or(&[]).iter().take_while(|&&b| b != 0).map(|&b| char::from(b)).collect();
    let (words, rows, first_bit, last_bit, image, mask, mode) = (word(16)? + 1, word(20)? + 1, word(24)?, word(28)?, word(32)?, word(36)?, word(40)?);
    let (bits, x_eig, y_eig, new_style) = if mode < 256 {
        let (bits, x, y) = old_mode(mode).ok_or_else(|| format!("sprite \"{name}\" is in screen mode {mode}, which Limoniter doesn't know"))?;
        (bits, x, y, false)
    } else if mode & 1 == 1 {
        let bits = match mode >> 27 {
            1 => 1,
            2 => 2,
            3 => 4,
            4 => 8,
            5 => 16,
            6 => 32,
            other => return Err(format!("sprite \"{name}\" is of a type ({other}) Limoniter doesn't know")),
        };
        // 90 dots an inch is an eigen factor of 1, 45 of 2, 180 of 0.
        let eig = |dpi: usize| match dpi {
            0..=60 => 2,
            61..=120 => 1,
            _ => 0,
        };
        (bits, eig(mode >> 1 & 0x1FFF), eig(mode >> 14 & 0x1FFF), true)
    } else {
        return Err(format!("sprite \"{name}\" names its mode by a mode selector, which isn't saved with it"));
    };
    if words > 4096 || rows > 8192 || first_bit >= 32 || last_bit >= 32 {
        return Err(format!("sprite \"{name}\" has a header that can't be right"));
    }
    let row_bits = words * 32 - first_bit - (31 - last_bit);
    let width = row_bits / bits;
    let stride = words * 4;
    let pixels = data.get(at + image..at + image + stride * rows).ok_or_else(|| format!("sprite \"{name}\" runs past the end of the file"))?;
    // A palette between the header and the image (or the mask, if that comes first).
    let palette_end = image.min(if mask != image { mask } else { image });
    let palette: Vec<[u8; 3]> = if palette_end > 44 && bits <= 8 {
        (0..(palette_end - 44) / 8).filter_map(|i| le32(data, at + 44 + i * 8)).map(|c| [(c >> 8) as u8, (c >> 16) as u8, (c >> 24) as u8]).collect()
    } else {
        default_palette(bits)
    };
    let value = |row: &[u8], bit: usize, size: usize| -> usize {
        let mut v = 0;
        for i in 0..size {
            let b = bit + i;
            if row.get(b / 8).is_some_and(|&byte| byte >> (b % 8) & 1 != 0) {
                v |= 1 << i;
            }
        }
        v
    };
    let mask_rows = (mask != image).then(|| {
        let mask_stride = if new_style { width.div_ceil(32) * 4 } else { stride };
        (data.get(at + mask..at + mask + mask_stride * rows), mask_stride)
    });
    // Pixels made square: the narrower or shorter side repeated.
    let (x_rep, y_rep) = (1usize << x_eig.saturating_sub(y_eig), 1usize << y_eig.saturating_sub(x_eig));
    let (out_w, out_h) = (width * x_rep, rows * y_rep);
    let mut rgba = vec![0u8; out_w * out_h * 4];
    for y in 0..rows {
        let row = &pixels[y * stride..(y + 1) * stride];
        for x in 0..width {
            let v = value(row, first_bit + x * bits, bits);
            let colour = match bits {
                16 => [((v & 31) * 255 / 31) as u8, ((v >> 5 & 31) * 255 / 31) as u8, ((v >> 10 & 31) * 255 / 31) as u8],
                32 => [v as u8, (v >> 8) as u8, (v >> 16) as u8],
                _ => palette.get(v).copied().unwrap_or([0, 0, 0]),
            };
            let shown = match &mask_rows {
                Some((Some(masks), mask_stride)) => {
                    let mask_row = &masks[y * mask_stride..(y + 1) * mask_stride];
                    if new_style { value(mask_row, x, 1) != 0 } else { value(mask_row, first_bit + x * bits, bits) != 0 }
                }
                _ => true,
            };
            let pixel = [colour[0], colour[1], colour[2], if shown { 255 } else { 0 }];
            for dy in 0..y_rep {
                for dx in 0..x_rep {
                    let o = ((y * y_rep + dy) * out_w + x * x_rep + dx) * 4;
                    rgba[o..o + 4].copy_from_slice(&pixel);
                }
            }
        }
    }
    let colours = if bits <= 8 { format!("{} colours", 1usize << bits) } else { format!("{bits}-bit colour") };
    let detail = format!("{width}\u{D7}{rows}, {colours}{}{}", if mode < 256 { format!(", mode {mode}") } else { String::new() }, if mask != image { ", masked" } else { "" });
    Ok(Sprite { name, width: out_w, height: out_h, rgba, detail })
}

fn header(bytes: &[u8]) -> Option<(usize, usize, usize)> {
    let (count, first, free) = (le32(bytes, 0)?, le32(bytes, 4)?, le32(bytes, 8)?);
    (count > 0 && count <= 10_000 && first == 16 && free >= first && free <= bytes.len() + 4 && bytes.len() >= 12 + 44).then_some((count, first, free))
}

pub fn looks_like(ending: &str, bytes: &[u8]) -> bool {
    // Without the file type, the free space must start exactly at the file's end, as RISC OS saves it.
    header(bytes).is_some_and(|(_, _, free)| matches!(ending, "ff9" | "spr" | "sprite") || free == bytes.len() + 4)
}

pub fn decode(bytes: &[u8]) -> Result<Picture, String> {
    let (count, first, _) = header(bytes).ok_or("This isn't a RISC OS sprite file.")?;
    let mut sprites = Vec::new();
    let mut problems = Vec::new();
    // Offsets in the area count from its first word, which the file leaves out.
    let mut at = first - 4;
    for _ in 0..count {
        match sprite(bytes, at) {
            Ok(s) => sprites.push(s),
            Err(problem) => problems.push(problem),
        }
        match le32(bytes, at) {
            Some(next) if next >= 44 => at += next,
            _ => break,
        }
    }
    if sprites.is_empty() {
        return Err(problems.first().map_or_else(|| "This sprite file has no sprites in it.".to_string(), |p| format!("The sprites can't be shown: {p}.")));
    }
    let mut picture = if sprites.len() == 1 {
        let s = &sprites[0];
        let mut picture = Picture::new(s.width, s.height);
        picture.rgba.copy_from_slice(&s.rgba);
        picture.note("Size", format!("{}\u{D7}{}", s.width, s.height));
        picture.note("Sprite", format!("\"{}\": {}", s.name, s.detail));
        picture
    } else {
        // A sheet: rows of sprites, 8 pixels apart, as wide as the widest or 640.
        const GAP: usize = 8;
        let across = sprites.iter().map(|s| s.width).max().unwrap_or(0).max(640);
        let (mut x, mut y, mut line, mut placed) = (0, 0, 0, Vec::new());
        for s in &sprites {
            if x > 0 && x + s.width > across {
                (x, y, line) = (0, y + line + GAP, 0);
            }
            placed.push((x, y));
            x += s.width + GAP;
            line = line.max(s.height);
        }
        let (width, height) = (across, (y + line).max(1));
        let mut picture = Picture::new(width, height);
        for (s, &(px, py)) in sprites.iter().zip(&placed) {
            for row in 0..s.height {
                let from = &s.rgba[row * s.width * 4..(row + 1) * s.width * 4];
                let to = ((py + row) * width + px) * 4;
                picture.rgba[to..to + s.width * 4].copy_from_slice(from);
            }
        }
        picture.note("Size", format!("{width}\u{D7}{height}, a sheet of {} sprites", sprites.len()));
        for s in sprites.iter().take(40) {
            picture.note("Sprite", format!("\"{}\": {}", s.name, s.detail));
        }
        if sprites.len() > 40 {
            picture.note("Sprite", format!("and {} more", sprites.len() - 40));
        }
        picture
    };
    picture.info.insert(1, ("Colours".into(), "RISC OS sprites".into()));
    for problem in problems {
        picture.note("Not shown", problem);
    }
    Ok(picture)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A file of one 8x2 sprite in mode 12 (4 bits a pixel, pixels twice as tall), with a mask.
    fn one_sprite() -> Vec<u8> {
        let mut sprite = Vec::new();
        sprite.extend(60u32.to_le_bytes()); // next: the 44-byte header, 8 of image, 8 of mask
        sprite.extend(b"test\0\0\0\0\0\0\0\0");
        for w in [0u32, 1, 0, 31, 44, 52, 12] {
            sprite.extend(w.to_le_bytes());
        }
        // Image: two rows of one word: pixel 0 is colour 11 (red), pixel 1 colour 7 (black).
        sprite.extend(0x0000_007Bu32.to_le_bytes());
        sprite.extend(0u32.to_le_bytes());
        // Mask: pixel 0 shown, the rest not.
        sprite.extend(0x0000_000Fu32.to_le_bytes());
        sprite.extend(0u32.to_le_bytes());
        let mut file = Vec::new();
        file.extend(1u32.to_le_bytes());
        file.extend(16u32.to_le_bytes());
        file.extend(((16 + sprite.len()) as u32).to_le_bytes());
        file.extend(sprite);
        file
    }

    #[test]
    fn a_sprite_is_drawn_little_end_first_square_and_masked() {
        let file = one_sprite();
        assert!(looks_like("", &file), "the free space is at the end");
        let picture = decode(&file).unwrap();
        assert_eq!((picture.width, picture.height), (8, 4), "mode 12's tall pixels are doubled down");
        assert_eq!(&picture.rgba[..4], &[0xDD, 0, 0, 255]);
        assert_eq!(&picture.rgba[4..8], &[0, 0, 0, 0], "masked out");
        assert_eq!(&picture.rgba[32..36], &[0xDD, 0, 0, 255], "the row repeated");
    }

    #[test]
    fn the_256_colour_default_palette_has_its_tints() {
        let palette = default_palette(8);
        assert_eq!(palette[0], [0, 0, 0]);
        assert_eq!(palette[255], [255, 255, 255]);
        assert_eq!(palette[3], [51, 51, 51], "the tint bits alone are a grey");
    }
}
