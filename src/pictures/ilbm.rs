//! Amiga IFF pictures: ILBM ("InterLeaved BitMap") and PBM, from Electronic Arts' "EA IFF 85" and "ILBM IFF
//! Interleaved Bitmap" documents (1985-86) and Commodore's additions to them (CAMG, HAM8, deep ILBMs).
//!
//! A FORM holds chunks, each a four-letter name, a big-endian length, and its data padded to an even length:
//! - BMHD: the size, the number of bit planes, whether there's a mask plane, the compression, and the pixel's shape.
//! - CMAP: the palette, as red, green, blue bytes.
//! - CAMG: the Amiga display mode: HAM (hold and modify), EHB (extra half-brite), hires, interlace.
//! - BODY: the pixels: for each row, a row of each plane in turn (and the mask's), each a whole number of 16-bit
//!   words; or in a PBM, a byte a pixel. Compression 1 is ByteRun1 (PackBits).
//! - CRNG (Deluxe Paint), CCRT (Graphicraft) and DRNG (Deluxe Paint IV): colour cycling, ranges of the palette
//!   rotated as the picture shows, which the preview can play.
//! - ANNO, AUTH, NAME, "(c) " and the rest are only read to describe the picture.

use super::Picture;

const CAMG_HAM: u32 = 0x800;
const CAMG_EHB: u32 = 0x80;
const CAMG_HIRES: u32 = 0x8000;
const CAMG_SUPERHIRES: u32 = 0x20;
const CAMG_LACE: u32 = 0x4;

pub fn looks_like(bytes: &[u8]) -> bool {
    bytes.len() >= 12 && &bytes[..4] == b"FORM" && matches!(&bytes[8..12], b"ILBM" | b"PBM ")
}

struct Header {
    width: usize,
    height: usize,
    planes: usize,
    masking: u8,
    compression: u8,
    transparent: u16,
    x_aspect: u8,
    y_aspect: u8,
}

/// How a pixel's value becomes a colour.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Mode {
    Plain,
    /// Extra half-brite: values from 32 are the first 32 colours at half brightness.
    Ehb,
    /// Hold and modify, with this many bits of data under the two control bits.
    Ham(u32),
}

/// A part of the palette that cycles.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Range {
    low: usize,
    high: usize,
    steps_per_second: f32,
    /// Colours move down the range rather than up it.
    reverse: bool,
}

/// A picture whose palette cycles: its pixels' values and palette kept, to draw it at any moment.
#[derive(Clone, Debug, PartialEq)]
pub struct Cycling {
    width: usize,
    values: Vec<u8>,
    palette: Vec<[u8; 3]>,
    mode: Mode,
    ranges: Vec<Range>,
}

impl Cycling {
    /// The picture this many seconds into cycling, into `rgba`. Each range steps on at its own speed: forward moves
    /// each colour one place up the range, the top one coming round to the bottom, as Deluxe Paint does.
    pub fn frame(&self, seconds: f64, rgba: &mut [u8]) {
        let mut palette = self.palette.clone();
        for range in &self.ranges {
            if palette.len() <= range.high {
                palette.resize(range.high + 1, [0, 0, 0]);
            }
            let length = range.high - range.low + 1;
            let steps = (seconds * f64::from(range.steps_per_second)) as usize % length;
            let part = &mut palette[range.low..=range.high];
            if range.reverse {
                part.rotate_left(steps);
            } else {
                part.rotate_right(steps);
            }
        }
        render(&self.values, &palette, self.mode, self.width, rgba);
    }

    /// Cycling for a picture of palette colours alone, from other formats' ranges: (lowest colour, highest, steps a
    /// second, backwards). Ranges that couldn't cycle are left out; None if none are left.
    pub(super) fn plain(width: usize, values: Vec<u8>, palette: Vec<[u8; 3]>, ranges: &[(usize, usize, f32, bool)]) -> Option<Box<Self>> {
        let ranges: Vec<Range> = ranges.iter().filter(|r| r.1 > r.0 && r.2 > 0.0).map(|&(low, high, steps_per_second, reverse)| Range { low, high, steps_per_second, reverse }).collect();
        (!ranges.is_empty()).then(|| Box::new(Self { width, values, palette, mode: Mode::Plain, ranges }))
    }

    /// How many ranges cycle.
    pub fn ranges(&self) -> usize {
        self.ranges.len()
    }

    /// The fastest range's steps a second, to know how often the picture changes.
    pub fn fastest(&self) -> f32 {
        self.ranges.iter().map(|r| r.steps_per_second).fold(0.0, f32::max)
    }
}

/// Draws pixels' values in a palette, row by row.
fn render(values: &[u8], palette: &[[u8; 3]], mode: Mode, width: usize, rgba: &mut [u8]) {
    let look_up = |index: usize| -> [u8; 3] {
        match mode {
            Mode::Ehb if index >= 32 => palette.get(index - 32).map_or([0, 0, 0], |c| c.map(|v| v >> 1)),
            _ => palette.get(index).copied().unwrap_or([0, 0, 0]),
        }
    };
    for (row, out) in values.chunks_exact(width).zip(rgba.chunks_exact_mut(width * 4)) {
        let mut put = |x: usize, [r, g, b]: [u8; 3]| out[x * 4..x * 4 + 4].copy_from_slice(&[r, g, b, 255]);
        match mode {
            Mode::Ham(bits) => {
                // Hold and modify: the top two bits say whether a pixel is a palette colour or the last pixel's with
                // one of its levels changed. Each row starts from the background colour.
                let mut colour = look_up(0);
                for (x, &value) in row.iter().enumerate() {
                    let control = u32::from(value) >> bits;
                    let data = value & ((1 << bits) - 1) as u8;
                    // A changed level is the new bits at the top, keeping the old level's bits below them.
                    let level = |old: u8| if bits == 4 { widen4(data) } else { (data << 2) | (old & 3) };
                    match control {
                        0 => colour = look_up(usize::from(data)),
                        1 => colour[2] = level(colour[2]),
                        2 => colour[0] = level(colour[0]),
                        _ => colour[1] = level(colour[1]),
                    }
                    put(x, colour);
                }
            }
            _ => {
                for (x, &value) in row.iter().enumerate() {
                    put(x, look_up(usize::from(value)));
                }
            }
        }
    }
}

fn be16(b: &[u8], at: usize) -> u16 {
    u16::from_be_bytes([b[at], b[at + 1]])
}

fn be32(b: &[u8], at: usize) -> u32 {
    u32::from_be_bytes([b[at], b[at + 1], b[at + 2], b[at + 3]])
}

/// Unpacks ByteRun1 until `size` bytes are out: n from 0 to 127 copies the next n+1 bytes, -1 to -127 repeats the
/// next byte 1-n times, and -128 does nothing. Some programs packed across rows, so the whole body is unpacked at once.
fn unpack_byterun1(data: &[u8], size: usize) -> (Vec<u8>, bool) {
    let mut out = Vec::with_capacity(size);
    let mut at = 0;
    while out.len() < size && at < data.len() {
        let n = data[at] as i8;
        at += 1;
        match n {
            0..=127 => {
                let count = n as usize + 1;
                let end = (at + count).min(data.len());
                out.extend_from_slice(&data[at..end]);
                at = end;
            }
            -127..=-1 => {
                if let Some(&byte) = data.get(at) {
                    out.extend(std::iter::repeat_n(byte, (1 - i16::from(n)) as usize));
                }
                at += 1;
            }
            -128 => {}
        }
    }
    let short = out.len() < size;
    out.resize(size, 0);
    (out, short)
}

/// A 4-bit Amiga colour level, made 8-bit the way the screen shows it.
fn widen4(level: u8) -> u8 {
    (level << 4) | level
}

pub fn decode(bytes: &[u8]) -> Result<Picture, String> {
    if !looks_like(bytes) {
        return Err("This isn't an IFF ILBM picture.".into());
    }
    let chunky = &bytes[8..12] == b"PBM ";
    let end = (8 + be32(bytes, 4) as usize).min(bytes.len());
    let mut header = None;
    let mut palette: Vec<[u8; 3]> = Vec::new();
    let mut camg = None;
    let mut body = None;
    let mut texts = Vec::new();
    // Deluxe Paint's CRNG and Graphicraft's CCRT; DRNG only when neither is there, as DPaint IV writes both.
    let (mut ranges, mut drng, mut range_chunks) = (Vec::new(), Vec::new(), 0);
    let mut unread = Vec::new();
    let mut at = 12;
    while at + 8 <= end {
        let id = &bytes[at..at + 4];
        let len = be32(bytes, at + 4) as usize;
        let data = &bytes[at + 8..(at + 8 + len).min(end)];
        match id {
            b"BMHD" if data.len() >= 20 => {
                header = Some(Header {
                    width: be16(data, 0) as usize,
                    height: be16(data, 2) as usize,
                    planes: data[8] as usize,
                    masking: data[9],
                    compression: data[10],
                    transparent: be16(data, 12),
                    x_aspect: data[14],
                    y_aspect: data[15],
                })
            }
            b"CMAP" => palette = data.as_chunks::<3>().0.to_vec(),
            b"CAMG" if data.len() >= 4 => camg = Some(be32(data, 0)),
            b"BODY" => body = Some(data),
            b"ANNO" | b"AUTH" | b"NAME" | b"(c) " | b"FVER" => {
                let label = match id {
                    b"ANNO" => "Note",
                    b"AUTH" => "Author",
                    b"NAME" => "Name",
                    b"(c) " => "Copyright",
                    _ => "Version",
                };
                let text: String = data.iter().take_while(|&&b| b != 0).map(|&b| char::from(b)).collect();
                if !text.trim().is_empty() {
                    texts.push((label, text.trim().to_string()));
                }
            }
            // Speed 16384 is 60 steps a second; bit 0 of the flags is on, bit 1 backwards.
            b"CRNG" | b"DRNG" if data.len() >= 8 => {
                range_chunks += 1;
                let (rate, flags) = (be16(data, 2) as i16, be16(data, 4));
                let (low, high) = if id == b"CRNG" { (data[6], data[7]) } else { (data[0], data[1]) };
                if flags & 1 != 0 && rate > 0 && high > low {
                    let range = Range { low: low.into(), high: high.into(), steps_per_second: f32::from(rate) * 60.0 / 16384.0, reverse: flags & 2 != 0 };
                    if id == b"CRNG" { ranges.push(range) } else { drng.push(range) }
                }
            }
            // Graphicraft: a direction (0 off, 1 forward, -1 back), the range, and the time each step takes.
            b"CCRT" if data.len() >= 12 => {
                range_chunks += 1;
                let direction = be16(data, 0) as i16;
                let delay = f64::from(be32(data, 4)) + f64::from(be32(data, 8)) / 1_000_000.0;
                if direction != 0 && delay > 0.0 && data[3] > data[2] {
                    ranges.push(Range { low: data[2].into(), high: data[3].into(), steps_per_second: (1.0 / delay) as f32, reverse: direction < 0 });
                }
            }
            b"PCHG" | b"SHAM" | b"CTBL" | b"BEAM" => unread.push(String::from_utf8_lossy(id).trim().to_string()),
            _ => {}
        }
        at += 8 + len + (len & 1);
    }
    let header = header.ok_or("This IFF picture has no BMHD chunk, so its size isn't known.")?;
    let body = body.ok_or("This IFF picture has no pixels (no BODY chunk); it may hold a palette or settings alone.")?;
    if header.width == 0 || header.height == 0 {
        return Err("This picture is 0 pixels across or down.".into());
    }
    if header.width * header.height > 64 * 1024 * 1024 {
        return Err(format!("This picture says it's {}\u{D7}{}, larger than any real one.", header.width, header.height));
    }
    let camg = camg.unwrap_or(0);
    let planes = header.planes;
    let ham = camg & CAMG_HAM != 0 && !chunky && matches!(planes, 5..=8);
    let ehb = camg & CAMG_EHB != 0 && !chunky && planes == 6 && !ham;
    if !chunky && !matches!(planes, 1..=8 | 24 | 32) {
        return Err(format!("This picture has {planes} bit planes, which isn't a kind Limoniter can show."));
    }
    if chunky && planes != 8 {
        return Err(format!("This PBM picture has {planes} bits a pixel; PBM pictures have 8."));
    }

    // Each row of each plane is a whole number of 16-bit words; a PBM's rows are a whole number of bytes, made even.
    let row_bytes = if chunky { header.width + (header.width & 1) } else { header.width.div_ceil(16) * 2 };
    let rows_per_line = if chunky { 1 } else { planes + usize::from(header.masking == 1) };
    let size = row_bytes * rows_per_line * header.height;
    let (pixels, short) = match header.compression {
        0 => {
            let mut raw = body.to_vec();
            let short = raw.len() < size;
            raw.resize(size, 0);
            (raw, short)
        }
        1 => unpack_byterun1(body, size),
        2 => return Err("This picture is packed with the Atari ST's vertical RLE (VDAT), which Limoniter doesn't unpack yet.".into()),
        other => return Err(format!("This picture is packed in a way (compression {other}) Limoniter doesn't know.")),
    };

    // Old programs saved the palette's 4-bit levels in the high nibble alone; they're shown as the Amiga showed them.
    let four_bit = !palette.is_empty() && palette.iter().flatten().all(|v| v & 0x0F == 0);
    if four_bit {
        for colour in &mut palette {
            for level in colour.iter_mut() {
                *level = widen4(*level >> 4);
            }
        }
    }
    let colours = 1usize << planes.min(8);
    // With no palette, shades of grey.
    if palette.is_empty() {
        palette = (0..colours).map(|i| [(i * 255 / (colours - 1).max(1)) as u8; 3]).collect();
    }
    // The second 32 of an extra half-brite picture are the first 32 at half brightness, as the screen makes them.
    if ehb {
        palette.resize(32, [0, 0, 0]);
    }
    let mode = if ham {
        Mode::Ham(planes as u32 - 2)
    } else if ehb {
        Mode::Ehb
    } else {
        Mode::Plain
    };

    let mut picture = Picture::new(header.width, header.height);
    let mut values = if planes < 24 { vec![0u8; header.width * header.height] } else { Vec::new() };
    let mut indices = vec![0u32; header.width];
    for y in 0..header.height {
        let line = &pixels[y * row_bytes * rows_per_line..(y + 1) * row_bytes * rows_per_line];
        if chunky {
            for (x, index) in indices.iter_mut().enumerate() {
                *index = u32::from(line[x]);
            }
        } else {
            indices.fill(0);
            for plane in 0..planes {
                let row = &line[plane * row_bytes..(plane + 1) * row_bytes];
                for (x, index) in indices.iter_mut().enumerate() {
                    if row[x >> 3] & (0x80 >> (x & 7)) != 0 {
                        *index |= 1 << plane;
                    }
                }
            }
        }
        if planes >= 24 {
            // Deep ILBMs: eight planes each of red, green and blue, low bits first.
            for (x, &value) in indices.iter().enumerate() {
                picture.set(x, y, [value as u8, (value >> 8) as u8, (value >> 16) as u8]);
            }
            continue;
        }
        for (x, &value) in indices.iter().enumerate() {
            values[y * header.width + x] = value as u8;
        }
    }
    if planes < 24 {
        render(&values, &palette, mode, header.width, &mut picture.rgba);
    }
    if ranges.is_empty() {
        ranges = drng;
    }
    let running = ranges.len();
    if running > 0 && planes < 24 {
        picture.cycling = Some(Box::new(Cycling { width: header.width, values, palette, mode, ranges }));
    }

    // The pixel's shape: from BMHD if it says, otherwise from the display mode.
    picture.aspect = if header.x_aspect > 0 && header.y_aspect > 0 {
        f32::from(header.x_aspect) / f32::from(header.y_aspect)
    } else {
        let across: f32 = if camg & CAMG_SUPERHIRES != 0 {
            0.25
        } else if camg & CAMG_HIRES != 0 {
            0.5
        } else {
            1.0
        };
        if camg & CAMG_LACE != 0 { across * 2.0 } else { across }
    };
    // Real displays' aspect ratios are near 1:1, 1:2 or 2:1, a quarter at the most; anything else is a broken header.
    if !(0.2..=5.0).contains(&picture.aspect) {
        picture.aspect = 1.0;
    }

    picture.note("Size", format!("{}\u{D7}{}", header.width, header.height));
    let kind = if chunky {
        "PBM, 256 colours, a byte a pixel".to_string()
    } else if planes >= 24 {
        format!("{planes}-bit colour (deep ILBM)")
    } else if ham {
        format!("HAM{planes} (hold and modify)")
    } else if ehb {
        "64 colours, extra half-brite".to_string()
    } else {
        format!("{} colours, {planes} bit plane{}", colours, if planes == 1 { "" } else { "s" })
    };
    picture.note("Colours", kind);
    let mut mode = Vec::new();
    if camg & CAMG_SUPERHIRES != 0 {
        mode.push("super-hires");
    } else if camg & CAMG_HIRES != 0 {
        mode.push("hires");
    } else if camg != 0 {
        mode.push("lores");
    }
    if camg & CAMG_LACE != 0 {
        mode.push("interlaced");
    }
    if !mode.is_empty() {
        picture.note("Screen mode", mode.join(", "));
    }
    picture.note(
        "Packing",
        match header.compression {
            0 => "None",
            _ => "ByteRun1",
        },
    );
    match header.masking {
        1 => picture.note("Mask", "A mask plane, not used here"),
        2 => picture.note("Mask", format!("Colour {} is see-through, shown as itself here", header.transparent)),
        3 => picture.note("Mask", "Lasso"),
        _ => {}
    }
    if four_bit {
        picture.note("Palette", "4 bits a level (OCS), widened");
    }
    if range_chunks > 0 {
        picture.note("Colour cycling", if running > 0 { format!("{running} of {range_chunks} range(s) on; play it to see them cycle") } else { format!("{range_chunks} range(s), all off") });
    }
    if !unread.is_empty() {
        picture.note("Not shown", format!("{} (palette changes down the screen), so colours may be off", unread.join(", ")));
    }
    for (label, text) in texts {
        picture.note(label, text);
    }
    if short {
        picture.note("Damaged", "The pixel data ends early; the rest is shown black");
    }
    Ok(picture)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn chunk(id: &[u8; 4], data: &[u8]) -> Vec<u8> {
        let mut out = id.to_vec();
        out.extend((data.len() as u32).to_be_bytes());
        out.extend(data);
        if data.len() & 1 == 1 {
            out.push(0);
        }
        out
    }

    fn form(kind: &[u8; 4], chunks: &[Vec<u8>]) -> Vec<u8> {
        let body: Vec<u8> = kind.iter().copied().chain(chunks.iter().flatten().copied()).collect();
        let mut out = b"FORM".to_vec();
        out.extend((body.len() as u32).to_be_bytes());
        out.extend(body);
        out
    }

    fn bmhd(width: u16, height: u16, planes: u8, masking: u8, compression: u8) -> Vec<u8> {
        let mut data = Vec::new();
        data.extend(width.to_be_bytes());
        data.extend(height.to_be_bytes());
        data.extend([0, 0, 0, 0, planes, masking, compression, 0, 0, 0, 10, 11]);
        data.extend(width.to_be_bytes());
        data.extend(height.to_be_bytes());
        chunk(b"BMHD", &data)
    }

    fn pixel(picture: &Picture, x: usize, y: usize) -> [u8; 3] {
        let at = (y * picture.width + x) * 4;
        [picture.rgba[at], picture.rgba[at + 1], picture.rgba[at + 2]]
    }

    #[test]
    fn planes_make_palette_colours_and_byterun1_unpacks() {
        // 2 planes, 16 pixels wide, 1 row: plane 0 = 1100..., plane 1 = 1010...
        let palette = chunk(b"CMAP", &[0, 0, 0, 255, 0, 0, 0, 255, 0, 0, 0, 255]);
        // Packed: a literal of 2 bytes for each plane's row.
        let body = chunk(b"BODY", &[1, 0b1100_0000, 0, 1, 0b1010_0000, 0]);
        let picture = decode(&form(b"ILBM", &[bmhd(16, 1, 2, 0, 1), palette, body])).unwrap();
        assert_eq!(pixel(&picture, 0, 0), [0, 0, 255], "both planes: colour 3");
        assert_eq!(pixel(&picture, 1, 0), [255, 0, 0], "plane 0: colour 1");
        assert_eq!(pixel(&picture, 2, 0), [0, 255, 0], "plane 1: colour 2");
        assert_eq!(pixel(&picture, 3, 0), [0, 0, 0]);
        assert!((picture.aspect - 10.0 / 11.0).abs() < 0.001, "the shape BMHD gives");
    }

    #[test]
    fn a_repeat_run_and_a_mask_plane_are_passed_over_rightly() {
        let palette = chunk(b"CMAP", &[0, 0, 0, 0xF0, 0xF0, 0xF0]);
        // One plane plus a mask, 16 wide, 2 rows; the run -1 repeats 0xFF twice.
        let body = chunk(b"BODY", &[0xFF, 0xFF, 1, 0x12, 0x34, 1, 0, 0, 1, 0xAA, 0xAA]);
        let picture = decode(&form(b"ILBM", &[bmhd(16, 2, 1, 1, 1), palette, body])).unwrap();
        assert_eq!(pixel(&picture, 0, 0), [0xFF; 3], "the old 4-bit palette is widened");
        assert_eq!(pixel(&picture, 0, 1), [0, 0, 0], "row 2 starts after row 1's mask");
        assert_eq!(pixel(&picture, 1, 1), [0, 0, 0]);
        assert_eq!(pixel(&picture, 7, 1), [0, 0, 0]);
        assert!(picture.info.iter().any(|(label, _)| label == "Mask"));
    }

    #[test]
    fn ham6_holds_and_modifies() {
        // 6 planes, 16 wide: pixel 0 is palette colour 1; pixel 1 sets red to 15; pixel 2 sets green to 8.
        let mut planes = [[0u8; 2]; 6];
        planes[0][0] |= 0x80; // pixel 0: value 0b00_0001
        for plane in planes.iter_mut().take(4) {
            plane[0] |= 0x40; // pixel 1: data 15
        }
        planes[5][0] |= 0x40; // control 10: red
        planes[3][0] |= 0x20; // pixel 2: data 8
        planes[4][0] |= 0x20;
        planes[5][0] |= 0x20; // control 11: green
        let body: Vec<u8> = planes.iter().flatten().copied().collect();
        let mut palette = vec![0u8; 16 * 3];
        palette[3..6].copy_from_slice(&[0x12, 0x23, 0x34]);
        let picture = decode(&form(b"ILBM", &[bmhd(16, 1, 6, 0, 0), chunk(b"CMAP", &palette), chunk(b"CAMG", &CAMG_HAM.to_be_bytes()), chunk(b"BODY", &body)])).unwrap();
        assert_eq!(pixel(&picture, 0, 0), [0x12, 0x23, 0x34]);
        assert_eq!(pixel(&picture, 1, 0), [0xFF, 0x23, 0x34]);
        assert_eq!(pixel(&picture, 2, 0), [0xFF, 0x88, 0x34]);
        assert_eq!(pixel(&picture, 3, 0), [0, 0, 0], "control 00 with data 0: colour 0");
    }

    #[test]
    fn ehb_makes_the_half_bright_colours() {
        let mut palette = vec![0u8; 32 * 3];
        palette[3..6].copy_from_slice(&[200, 100, 50]);
        // Pixel 0 is colour 33: plane 0 and plane 5.
        let mut planes = [[0u8; 2]; 6];
        planes[0][0] = 0x80;
        planes[5][0] = 0x80;
        let body: Vec<u8> = planes.iter().flatten().copied().collect();
        let picture = decode(&form(b"ILBM", &[bmhd(16, 1, 6, 0, 0), chunk(b"CMAP", &palette), chunk(b"CAMG", &CAMG_EHB.to_be_bytes()), chunk(b"BODY", &body)])).unwrap();
        assert_eq!(pixel(&picture, 0, 0), [100, 50, 25]);
    }

    #[test]
    fn a_pbm_is_a_byte_a_pixel_with_even_rows() {
        let palette = chunk(b"CMAP", &[0, 0, 0, 9, 9, 9, 7, 7, 7]);
        // 3 wide, so each row has a pad byte.
        let body = chunk(b"BODY", &[1, 2, 0, 0, 2, 1, 1, 0]);
        let picture = decode(&form(b"PBM ", &[bmhd(3, 2, 8, 0, 0), palette, body])).unwrap();
        assert_eq!(pixel(&picture, 0, 0), [9; 3]);
        assert_eq!(pixel(&picture, 1, 0), [7; 3]);
        assert_eq!(pixel(&picture, 0, 1), [7; 3]);
    }

    #[test]
    fn its_text_is_described_and_a_short_body_says_so() {
        let picture = decode(&form(b"ILBM", &[bmhd(16, 4, 1, 0, 0), chunk(b"AUTH", b"Dan Silva"), chunk(b"BODY", &[0xFF, 0xFF])])).unwrap();
        assert!(picture.info.contains(&("Author".to_string(), "Dan Silva".to_string())));
        assert!(picture.info.iter().any(|(label, _)| label == "Damaged"));
        assert_eq!(pixel(&picture, 0, 0), [255; 3], "no palette: grey levels");
    }

    #[test]
    fn a_cycling_range_rotates_the_palette() {
        // Colours 1 to 3 cycle forward at 60 steps a second; pixels 0..3 are colours 0..3.
        let palette = chunk(b"CMAP", &[0, 0, 0, 10, 10, 10, 20, 20, 20, 30, 30, 30]);
        let mut crng = vec![0, 0];
        crng.extend(16384i16.to_be_bytes());
        crng.extend(1i16.to_be_bytes());
        crng.extend([1, 3]);
        let body = chunk(b"BODY", &[0b0101_0000, 0, 0b0011_0000, 0]);
        let picture = decode(&form(b"ILBM", &[bmhd(16, 1, 2, 0, 0), palette, chunk(b"CRNG", &crng), body])).unwrap();
        let cycling = picture.cycling.as_ref().expect("the range is on");
        let mut frame = picture.rgba.clone();
        let level = |frame: &[u8], x: usize| frame[x * 4];
        cycling.frame(0.0, &mut frame);
        assert_eq!(frame, picture.rgba, "at the start it's the picture as saved");
        cycling.frame(1.0 / 60.0 + 0.001, &mut frame);
        assert_eq!([level(&frame, 0), level(&frame, 1), level(&frame, 2), level(&frame, 3)], [0, 30, 10, 20], "one step: each colour up one, the top round to the bottom");
        assert!(picture.info.iter().any(|(label, value)| label == "Colour cycling" && value.starts_with("1 of 1")));
    }

    #[test]
    fn a_range_thats_off_isnt_played() {
        let mut crng = vec![0, 0];
        crng.extend(16384i16.to_be_bytes());
        crng.extend(0i16.to_be_bytes());
        crng.extend([0, 1]);
        let picture = decode(&form(b"ILBM", &[bmhd(16, 1, 1, 0, 0), chunk(b"CRNG", &crng), chunk(b"BODY", &[0, 0])])).unwrap();
        assert!(picture.cycling.is_none());
    }

    #[test]
    fn what_isnt_a_picture_is_refused() {
        assert!(decode(b"FORM\0\0\0\x04ILBM").is_err());
        assert!(!looks_like(b"FORM\0\0\0\x04SMUS"));
    }
}
