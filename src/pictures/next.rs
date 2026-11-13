//! ZX Spectrum Next pictures: Layer 2 screens, a byte a pixel through the Next's palette (.sl2 and .nxi, 256×192,
//! or 320×256 and 640×256, whose pixels go down each column first), LoRes screens (.slr, 128×96, each pixel
//! shown 2×2), and palettes (.nxp, .pal). A palette is 256 colours of two bytes, RRRGGGBB then a ninth, lowest blue
//! bit; an .nxi may have one in front of its pixels. Without one, the Next's own: each pixel's byte is its RRRGGGBB.

use super::Picture;

const L2: usize = 256 * 192;
const L2_BIG: usize = 320 * 256;
const LORES: usize = 128 * 96;
const PALETTE: usize = 512;

/// A colour from the Next's 9-bit palette entry: RRRGGGBB, and the lowest blue bit.
pub fn colour(first: u8, second: u8) -> [u8; 3] {
    let scale = |v: u32| (v * 255 / 7) as u8;
    let blue = u32::from((first & 3) << 1) | u32::from(second & 1);
    [scale(u32::from(first >> 5)), scale(u32::from((first >> 2) & 7)), scale(blue)]
}

/// The Next's palette at reset: each index's own RRRGGGBB, the lowest blue bit set when either other is.
pub fn default_palette() -> Vec<u8> {
    (0..256u32).flat_map(|i| [i as u8, u8::from(i & 3 != 0)]).collect()
}

pub fn looks_like(ending: &str, bytes: &[u8]) -> bool {
    match ending {
        "sl2" => matches!(bytes.len(), L2 | L2_BIG),
        "nxi" => matches!(bytes.len(), L2 | L2_BIG) || bytes.len() == L2 + PALETTE || bytes.len() == L2_BIG + PALETTE,
        "slr" => matches!(bytes.len(), LORES) || bytes.len() == LORES + PALETTE,
        "nxp" => bytes.len() == PALETTE || bytes.len() == 256,
        _ => false,
    }
}

/// A Layer 2 or LoRes screen, from its pixels and palette, as the Next shows it; also what a .nex's loading screen
/// is shown with. `kind` is the file's ending.
pub fn decode(kind: &str, bytes: &[u8]) -> Result<Picture, String> {
    if kind == "nxp" {
        return Ok(palette_picture(bytes));
    }
    let (palette, pixels) = match bytes.len() {
        n if n == L2 + PALETTE || n == L2_BIG + PALETTE || n == LORES + PALETTE => (bytes[..PALETTE].to_vec(), &bytes[PALETTE..]),
        _ => (default_palette(), bytes),
    };
    let lookup = |index: u8| colour(palette[usize::from(index) * 2], palette[usize::from(index) * 2 + 1]);
    let mut picture;
    match pixels.len() {
        L2 => {
            picture = Picture::new(256, 192);
            for (i, index) in pixels.iter().enumerate() {
                picture.set(i % 256, i / 256, lookup(*index));
            }
            picture.note("Mode", "Layer 2, 256\u{D7}192, 256 colours");
        }
        L2_BIG if kind == "nxi640" => {
            // 640×256 at 4 bits a pixel: each byte two pixels, left in its high half, down each column.
            picture = Picture::new(640, 256);
            for (i, byte) in pixels.iter().enumerate() {
                let (x, y) = ((i / 256) * 2, i % 256);
                picture.set(x, y, lookup(byte >> 4));
                picture.set(x + 1, y, lookup(byte & 15));
            }
            picture.aspect = 0.5;
            picture.note("Mode", "Layer 2, 640\u{D7}256, 16 colours");
        }
        L2_BIG => {
            picture = Picture::new(320, 256);
            for (i, index) in pixels.iter().enumerate() {
                picture.set(i / 256, i % 256, lookup(*index));
            }
            picture.note("Mode", "Layer 2, 320\u{D7}256, 256 colours");
        }
        LORES => {
            picture = Picture::new(256, 192);
            for (i, index) in pixels.iter().enumerate() {
                let (x, y) = ((i % 128) * 2, (i / 128) * 2);
                let c = lookup(*index);
                for (dx, dy) in [(0, 0), (1, 0), (0, 1), (1, 1)] {
                    picture.set(x + dx, y + dy, c);
                }
            }
            picture.note("Mode", "LoRes, 128\u{D7}96, 256 colours");
        }
        n => return Err(format!("A Next screen is {L2}, {L2_BIG} or {LORES} bytes of pixels, and this is {n}.")),
    }
    picture.note("Palette", if bytes.len() > pixels.len() { "The file's own" } else { "The Next's at reset" });
    Ok(picture)
}

/// A palette shown as its 256 colours, 16 a row.
fn palette_picture(bytes: &[u8]) -> Picture {
    let mut picture = Picture::new(256, 256);
    for index in 0..256 {
        // 256 bytes: RRRGGGBB alone; 512: with the ninth bit.
        let c = if bytes.len() >= PALETTE { colour(bytes[index * 2], bytes[index * 2 + 1]) } else { colour(bytes[index], u8::from(bytes[index] & 3 != 0)) };
        for y in 0..16 {
            for x in 0..16 {
                picture.set((index % 16) * 16 + x, (index / 16) * 16 + y, c);
            }
        }
    }
    picture.note("Palette", "256 colours, 16 a row");
    picture
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn layer2_lores_and_palettes_show_in_the_nexts_colours() {
        assert_eq!(colour(0xE0, 0), [255, 0, 0]);
        assert_eq!(colour(0x03, 1), [0, 0, 255]);
        assert_eq!(colour(0xFF, 1), [255, 255, 255]);
        let mut pixels = vec![0u8; L2];
        pixels[1] = 0xE0;
        let picture = decode("sl2", &pixels).unwrap();
        assert_eq!((picture.width, picture.height), (256, 192));
        assert_eq!(&picture.rgba[4..8], &[255, 0, 0, 255]);
        // Its own palette in front: index 0 is green.
        let mut nxi = default_palette();
        nxi[0] = 0x1C;
        nxi.extend(vec![0u8; L2]);
        assert!(looks_like("nxi", &nxi));
        assert_eq!(&decode("nxi", &nxi).unwrap().rgba[..4], &[0, 255, 0, 255]);
        // 320×256 goes down the columns: the second byte is the pixel under the first.
        let mut big = vec![0u8; L2_BIG];
        big[1] = 0x03;
        let picture = decode("sl2", &big).unwrap();
        assert_eq!(&picture.rgba[320 * 4..320 * 4 + 4], &[0, 0, 255, 255]);
        let lores = decode("slr", &vec![0xFFu8; LORES]).unwrap();
        assert_eq!((lores.width, lores.height), (256, 192));
    }
}
