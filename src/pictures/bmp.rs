//! Windows and OS/2 bitmaps (.bmp), as the Spectrum Next's `.bmpload` and its graphics tools take them: a 14-byte
//! file header ("BM", the size, where the pixels start), an information header (40 bytes or more; 12 for OS/2's),
//! a palette of blue, green, red (and a spare byte, but for OS/2's) for 1, 4 and 8 bits a pixel, then the rows,
//! each padded to four bytes, from the bottom up unless the height is negative. Uncompressed, and 8- and 4-bit
//! run-length packed (RLE8, RLE4); 16, 24 and 32 bits a pixel as blue, green and red.

use super::Picture;

/// A bitmap's pixels as indices into its palette, for those that have one.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Indexed {
    pub width: usize,
    pub height: usize,
    /// Row by row from the top.
    pub pixels: Vec<u8>,
    pub palette: Vec<[u8; 3]>,
    pub bits: usize,
}

fn le16(b: &[u8], at: usize) -> usize {
    usize::from(u16::from_le_bytes([b[at], b[at + 1]]))
}

fn le32(b: &[u8], at: usize) -> u32 {
    u32::from_le_bytes([b[at], b[at + 1], b[at + 2], b[at + 3]])
}

pub fn looks_like(bytes: &[u8]) -> bool {
    bytes.len() >= 26 && bytes.starts_with(b"BM") && matches!(le32(bytes, 14), 12 | 40 | 52 | 56 | 64 | 108 | 124)
}

struct Header {
    width: usize,
    height: usize,
    top_down: bool,
    bits: usize,
    compression: u32,
    colours: usize,
    palette_at: usize,
    entry: usize,
    pixels_at: usize,
}

fn header(bytes: &[u8]) -> Result<Header, String> {
    if !looks_like(bytes) {
        return Err(crate::i18n::tr("It isn't a BMP: those start with BM and a header of a known size.").into());
    }
    let size = le32(bytes, 14) as usize;
    let (width, height, bits, compression, colours) = if size == 12 {
        (le16(bytes, 18) as i64, le16(bytes, 20) as i64, le16(bytes, 24), 0, 0)
    } else {
        if bytes.len() < 54 {
            return Err(crate::i18n::tr("The BMP's header is cut short.").into());
        }
        (i64::from(le32(bytes, 18) as i32), i64::from(le32(bytes, 22) as i32), le16(bytes, 28), le32(bytes, 30), le32(bytes, 46) as usize)
    };
    if width <= 0 || height == 0 || width > 16384 || height.abs() > 16384 {
        return Err(crate::i18n::tf!("A BMP of {width} by {height} pixels can't be shown."));
    }
    if !matches!(bits, 1 | 4 | 8 | 16 | 24 | 32) {
        return Err(crate::i18n::tf!("BMPs of {bits} bits a pixel aren't read."));
    }
    // Packed with bit fields (3) is read as the usual layout of 16 and 32 bits.
    if !matches!(compression, 0 | 3) && !(compression == 1 && bits == 8) && !(compression == 2 && bits == 4) {
        return Err(crate::i18n::tr("This BMP is packed in a way that isn't read (only RLE4 and RLE8 are).").into());
    }
    let entry = if size == 12 { 3 } else { 4 };
    let colours = if bits <= 8 { if colours == 0 || colours > 1 << bits { 1 << bits } else { colours } } else { 0 };
    Ok(Header { width: width as usize, height: height.unsigned_abs() as usize, top_down: height < 0, bits, compression, colours, palette_at: 14 + size, entry, pixels_at: le32(bytes, 10) as usize })
}

/// A bitmap of 1, 4 or 8 bits a pixel, its indices and palette.
pub fn indexed(bytes: &[u8]) -> Result<Indexed, String> {
    let h = header(bytes)?;
    if h.bits > 8 {
        return Err(crate::i18n::tf!("This BMP is {} bits a pixel, without a palette.", h.bits));
    }
    let mut palette = Vec::with_capacity(h.colours);
    for i in 0..h.colours {
        let at = h.palette_at + i * h.entry;
        let entry = bytes.get(at..at + 3).ok_or(crate::i18n::tr("The BMP's palette is cut short."))?;
        palette.push([entry[2], entry[1], entry[0]]);
    }
    let mut pixels = vec![0u8; h.width * h.height];
    let data = bytes.get(h.pixels_at..).ok_or(crate::i18n::tr("The BMP's pixels are missing."))?;
    // Row y from the file's start: the bottom row first, unless it's top-down.
    let place = |row: usize| if h.top_down { row } else { h.height - 1 - row };
    if h.compression == 1 || h.compression == 2 {
        unpack_rle(data, &h, &mut pixels, &place);
    } else {
        let stride = (h.width * h.bits).div_ceil(32) * 4;
        for row in 0..h.height {
            let line = data.get(row * stride..row * stride + stride).ok_or(crate::i18n::tr("The BMP's pixels are cut short."))?;
            let y = place(row);
            for x in 0..h.width {
                let bit = x * h.bits;
                let byte = line[bit / 8];
                pixels[y * h.width + x] = (byte >> (8 - h.bits - bit % 8)) & ((1u16 << h.bits) - 1) as u8;
            }
        }
    }
    Ok(Indexed { width: h.width, height: h.height, pixels, palette, bits: h.bits })
}

/// RLE8 and RLE4: a count and a value (for RLE4, two pixels taken in turn), or 0 and an escape: 0 the line's end,
/// 1 the picture's, 2 a move right and up, more a run of that many pixels as they are, padded to a word.
fn unpack_rle(data: &[u8], h: &Header, pixels: &mut [u8], place: &dyn Fn(usize) -> usize) {
    let four = h.compression == 2;
    let (mut x, mut row, mut at) = (0usize, 0usize, 0usize);
    let mut put = |x: usize, row: usize, value: u8| {
        if x < h.width && row < h.height {
            pixels[place(row) * h.width + x] = value;
        }
    };
    while at + 1 < data.len() {
        let (count, value) = (usize::from(data[at]), data[at + 1]);
        at += 2;
        if count > 0 {
            for i in 0..count {
                put(x, row, if four { if i % 2 == 0 { value >> 4 } else { value & 15 } } else { value });
                x += 1;
            }
            continue;
        }
        match value {
            0 => {
                x = 0;
                row += 1;
            }
            1 => break,
            2 => {
                if at + 1 >= data.len() {
                    break;
                }
                x += usize::from(data[at]);
                row += usize::from(data[at + 1]);
                at += 2;
            }
            n => {
                let n = usize::from(n);
                let length = if four { n.div_ceil(2) } else { n };
                for i in 0..n {
                    let Some(&byte) = data.get(at + if four { i / 2 } else { i }) else { break };
                    put(x, row, if four { if i % 2 == 0 { byte >> 4 } else { byte & 15 } } else { byte });
                    x += 1;
                }
                at += length + (length & 1);
            }
        }
    }
}

pub fn decode(bytes: &[u8]) -> Result<Picture, String> {
    let h = header(bytes)?;
    let mut picture = Picture::new(h.width, h.height);
    if h.bits <= 8 {
        let image = indexed(bytes)?;
        for (i, &index) in image.pixels.iter().enumerate() {
            picture.set(i % h.width, i / h.width, image.palette.get(usize::from(index)).copied().unwrap_or([0, 0, 0]));
        }
        picture.note("Colours", format!("{} bits a pixel, {} in its palette", h.bits, image.palette.len()));
    } else {
        let data = bytes.get(h.pixels_at..).ok_or(crate::i18n::tr("The BMP's pixels are missing."))?;
        let step = h.bits / 8;
        let stride = (h.width * h.bits).div_ceil(32) * 4;
        for row in 0..h.height {
            let line = data.get(row * stride..row * stride + h.width * step).ok_or(crate::i18n::tr("The BMP's pixels are cut short."))?;
            let y = if h.top_down { row } else { h.height - 1 - row };
            for x in 0..h.width {
                let p = &line[x * step..x * step + step];
                let rgb = if step == 2 {
                    // 5 bits each of red, green and blue.
                    let v = u16::from_le_bytes([p[0], p[1]]);
                    let scale = |c: u16| ((c & 31) * 255 / 31) as u8;
                    [scale(v >> 10), scale(v >> 5), scale(v)]
                } else {
                    [p[2], p[1], p[0]]
                };
                picture.set(x, y, rgb);
            }
        }
        picture.note("Colours", format!("{} bits a pixel", h.bits));
    }
    picture.note("Size", format!("{} \u{D7} {} pixels", h.width, h.height));
    Ok(picture)
}

/// A bitmap of 4 or 8 bits a pixel, uncompressed and from the bottom up, as `.bmpload` and most programs take.
pub fn encode(width: usize, height: usize, pixels: &[u8], palette: &[[u8; 3]], bits: usize) -> Vec<u8> {
    let colours = 1usize << bits;
    let stride = (width * bits).div_ceil(32) * 4;
    let pixels_at = 14 + 40 + colours * 4;
    let size = pixels_at + stride * height;
    let mut out = Vec::with_capacity(size);
    out.extend_from_slice(b"BM");
    out.extend_from_slice(&(size as u32).to_le_bytes());
    out.extend_from_slice(&[0; 4]);
    out.extend_from_slice(&(pixels_at as u32).to_le_bytes());
    out.extend_from_slice(&40u32.to_le_bytes());
    out.extend_from_slice(&(width as i32).to_le_bytes());
    out.extend_from_slice(&(height as i32).to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes());
    out.extend_from_slice(&(bits as u16).to_le_bytes());
    out.extend_from_slice(&0u32.to_le_bytes());
    out.extend_from_slice(&((stride * height) as u32).to_le_bytes());
    out.extend_from_slice(&2835u32.to_le_bytes());
    out.extend_from_slice(&2835u32.to_le_bytes());
    out.extend_from_slice(&(colours as u32).to_le_bytes());
    out.extend_from_slice(&0u32.to_le_bytes());
    for i in 0..colours {
        let [r, g, b] = palette.get(i).copied().unwrap_or([0, 0, 0]);
        out.extend_from_slice(&[b, g, r, 0]);
    }
    for y in (0..height).rev() {
        let mut line = vec![0u8; stride];
        for x in 0..width {
            let value = pixels[y * width + x];
            if bits == 8 {
                line[x] = value;
            } else {
                line[x / 2] |= (value & 15) << if x % 2 == 0 { 4 } else { 0 };
            }
        }
        out.extend(line);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bitmaps_written_read_back() {
        let palette: Vec<[u8; 3]> = (0..256).map(|i| [i as u8, 255 - i as u8, (i * 7) as u8]).collect();
        let pixels: Vec<u8> = (0..13 * 5).map(|i| (i * 11) as u8).collect();
        let bmp = encode(13, 5, &pixels, &palette, 8);
        let back = indexed(&bmp).unwrap();
        assert_eq!((back.width, back.height, back.bits), (13, 5, 8));
        assert_eq!(back.pixels, pixels);
        assert_eq!(back.palette, palette);
        let picture = decode(&bmp).unwrap();
        assert_eq!(&picture.rgba[4..8], &[11, 244, 77, 255]);
        // Four bits: two pixels a byte, the left in the high half.
        let small: Vec<u8> = (0..7 * 3).map(|i| (i % 16) as u8).collect();
        let bmp = encode(7, 3, &small, &palette[..16], 4);
        assert_eq!(indexed(&bmp).unwrap().pixels, small);
    }

    #[test]
    fn packed_and_true_colour_bitmaps_are_read() {
        // RLE8, 4 by 2: three pixels as they are (5, 5, 5, padded to a word) and a run of one 9, the end of the
        // line; a run of two 1s; the end.
        let mut bmp = encode(4, 2, &[0; 8], &[[0, 0, 0]; 2], 8);
        bmp[30] = 1;
        let at = le32(&bmp, 10) as usize;
        bmp.truncate(at);
        bmp.extend_from_slice(&[0, 3, 5, 5, 5, 0, 1, 9, 0, 0, 2, 1, 0, 1]);
        let image = indexed(&bmp).unwrap();
        assert_eq!(image.pixels, vec![1, 1, 0, 0, 5, 5, 5, 9], "the bottom row first");
        // 24 bits, 2 by 1: blue, green, red each pixel, and the row padded to four bytes.
        let mut bmp = encode(2, 1, &[0, 0], &[], 8);
        bmp.truncate(54);
        bmp[10..14].copy_from_slice(&54u32.to_le_bytes());
        bmp[28] = 24;
        bmp.extend_from_slice(&[1, 2, 3, 4, 5, 6, 0, 0]);
        let picture = decode(&bmp).unwrap();
        assert_eq!(&picture.rgba, &[3, 2, 1, 255, 6, 5, 4, 255]);
    }
}
