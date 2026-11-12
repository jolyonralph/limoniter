//! ZX Spectrum screens (.scr): screen memory saved as it is, 6144 bytes of bitmap and 768 of attributes, as
//! `SAVE "name" SCREEN$` wrote it. The bitmap's rows are in the Spectrum's interleaved order: a third of the
//! screen at a time, and within it the first pixel row of each character row, then the second, and so on. Each
//! attribute colours an 8x8 cell: ink in its low three bits, paper in the next three, then BRIGHT and FLASH.
//! A file kept on a +3 disk may have its 128-byte +3DOS header in front, and a bitmap alone is shown in black on
//! white.

use super::Picture;

const BITMAP: usize = 6144;
const SCREEN: usize = BITMAP + 768;
const PLUS3_HEADER: usize = 128;

/// The Spectrum's colours, normal and bright, as most emulators show them.
fn colour(index: u8, bright: bool) -> [u8; 3] {
    let level = if bright { 0xFF } else { 0xD7 };
    let on = |bit: u8| if index & bit != 0 { level } else { 0 };
    // The colour number's bits are blue, red and green, from the lowest.
    [on(2), on(4), on(1)]
}

/// The screen's bytes, without a +3DOS header in front.
fn body(bytes: &[u8]) -> &[u8] {
    if bytes.starts_with(b"PLUS3DOS") && bytes.len() >= PLUS3_HEADER + BITMAP { &bytes[PLUS3_HEADER..] } else { bytes }
}

pub fn looks_like(ending: &str, bytes: &[u8]) -> bool {
    ending == "scr" && matches!(body(bytes).len(), BITMAP | SCREEN)
}

pub fn decode(bytes: &[u8]) -> Result<Picture, String> {
    let data = body(bytes);
    if data.len() != SCREEN && data.len() != BITMAP {
        return Err(format!("A Spectrum screen is {SCREEN} bytes, and this is {}.", data.len()));
    }
    let attributes = data.get(BITMAP..SCREEN);
    let mut picture = Picture::new(256, 192);
    let mut flashing = false;
    let mut bright = false;
    for y in 0..192 {
        let row = (y >> 6) * 2048 + (y & 7) * 256 + ((y >> 3) & 7) * 32;
        for column in 0..32 {
            let pixels = data[row + column];
            let attribute = attributes.map_or(0x38, |a| a[(y >> 3) * 32 + column]);
            flashing |= attribute & 0x80 != 0;
            bright |= attribute & 0x40 != 0;
            let ink = colour(attribute & 7, attribute & 0x40 != 0);
            let paper = colour((attribute >> 3) & 7, attribute & 0x40 != 0);
            for bit in 0..8 {
                picture.set(column * 8 + bit, y, if pixels & (0x80 >> bit) != 0 { ink } else { paper });
            }
        }
    }
    picture.note("Size", "256\u{D7}192");
    if attributes.is_none() {
        picture.note("Colour", "None: the file has the bitmap alone, shown as black on white");
    } else {
        picture.note("Colour", if bright { "8\u{D7}8 cells, some BRIGHT" } else { "8\u{D7}8 cells" });
    }
    if flashing {
        picture.note("FLASH", "Some cells flash; they're shown as they look before swapping ink and paper");
    }
    if bytes.starts_with(b"PLUS3DOS") {
        picture.note("Header", "+3DOS");
    }
    Ok(picture)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_rows_are_interleaved_and_the_cells_coloured() {
        let mut screen = vec![0u8; SCREEN];
        // The second pixel row on the screen is 256 bytes on; the ninth (the next character row) is 32 bytes on.
        screen[256] = 0x80;
        screen[32] = 0x01;
        // Cell (0,0): bright red ink on blue paper.
        screen[BITMAP] = 0x40 | (1 << 3) | 2;
        let picture = decode(&screen).unwrap();
        let pixel = |x: usize, y: usize| picture.rgba[(y * 256 + x) * 4..(y * 256 + x) * 4 + 3].to_vec();
        assert_eq!(pixel(0, 1), [0xFF, 0, 0], "ink");
        assert_eq!(pixel(1, 1), [0, 0, 0xFF], "paper");
        assert_eq!(pixel(7, 8), [0, 0, 0], "black ink on black paper in the next cell row");
        assert_eq!(pixel(0, 0), [0, 0, 0xFF]);
    }

    #[test]
    fn a_plus3_header_is_passed_over_and_a_wrong_size_isnt_a_screen() {
        let mut file = b"PLUS3DOS".to_vec();
        file.resize(PLUS3_HEADER, 0);
        file.extend(vec![0xFFu8; BITMAP]);
        file.extend(vec![0x38u8; 768]);
        assert!(looks_like("scr", &file));
        let picture = decode(&file).unwrap();
        assert_eq!(&picture.rgba[..3], &[0, 0, 0], "black ink everywhere");
        assert!(!looks_like("scr", &[0; 1000]));
        assert!(!looks_like("bin", &vec![0; SCREEN]));
    }
}
