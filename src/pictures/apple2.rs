//! Apple II hi-res screens: the 8192 bytes of a hi-res page saved as they are (often 8184, the last 8 never shown).
//! 192 rows of 40 bytes, kept in the Apple's interleaved order: a row's place is 1024 times its line within a group
//! of 8, 128 times its group within a third of the screen, and 40 times the third. Each byte is 7 dots, lowest bit
//! leftmost; its top bit shifts the dots half a dot, which on a colour monitor changes their colours.
//!
//! The colours are as an NTSC monitor made them: two dots on next to each other are white; a dot on alone is
//! violet in an even column or green in an odd one, or blue and orange when its byte's top bit is set.

use super::Picture;

const WIDTH: usize = 280;
const HEIGHT: usize = 192;
const BLACK: [u8; 3] = [0, 0, 0];
const WHITE: [u8; 3] = [255, 255, 255];
const VIOLET: [u8; 3] = [0xDD, 0x22, 0xDD];
const GREEN: [u8; 3] = [0x11, 0xDD, 0x00];
const BLUE: [u8; 3] = [0x22, 0x22, 0xFF];
const ORANGE: [u8; 3] = [0xFF, 0x66, 0x00];

pub fn looks_like(ending: &str, bytes: &[u8]) -> bool {
    matches!(ending, "hgr" | "hgr1" | "hgr2" | "a2hgr") && (8184..=8192).contains(&bytes.len())
}

fn row_offset(y: usize) -> usize {
    (y & 7) * 0x400 + ((y >> 3) & 7) * 0x80 + (y >> 6) * 0x28
}

pub fn decode(bytes: &[u8]) -> Result<Picture, String> {
    if bytes.len() < 8184 {
        return Err("An Apple II hi-res screen is 8192 bytes.".into());
    }
    let mut picture = Picture::new(WIDTH, HEIGHT);
    for y in 0..HEIGHT {
        let row = &bytes[row_offset(y)..row_offset(y) + 40];
        let dot = |x: usize| x < WIDTH && row[x / 7] >> (x % 7) & 1 != 0;
        for x in 0..WIDTH {
            let colour = if !dot(x) {
                BLACK
            } else if (x > 0 && dot(x - 1)) || dot(x + 1) {
                WHITE
            } else {
                match (row[x / 7] & 0x80 != 0, x % 2 == 0) {
                    (false, true) => VIOLET,
                    (false, false) => GREEN,
                    (true, true) => BLUE,
                    (true, false) => ORANGE,
                }
            };
            picture.set(x, y, colour);
        }
    }
    picture.note("Size", "280\u{D7}192");
    picture.note("Colours", "Hi-res: black, white, and violet, green, blue and orange as an NTSC monitor showed them");
    Ok(picture)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rows_are_interleaved_and_dots_coloured_by_column() {
        let mut screen = vec![0u8; 8192];
        // Row 1 is 1024 bytes on; row 8 is 128 on. A lone dot in column 0 is violet, in column 1 green.
        screen[0x400] = 0b0000_0001;
        screen[0x80] = 0b1000_0010;
        screen[0] = 0b0000_0011;
        let picture = decode(&screen).unwrap();
        let pixel = |x: usize, y: usize| picture.rgba[(y * WIDTH + x) * 4..(y * WIDTH + x) * 4 + 3].to_vec();
        assert_eq!(pixel(0, 1), VIOLET);
        assert_eq!(pixel(1, 8), ORANGE, "the top bit makes an odd column's dot orange");
        assert_eq!(pixel(0, 0), WHITE, "two dots together are white");
        assert_eq!(pixel(2, 0), BLACK);
        assert!(looks_like("hgr", &screen));
        assert!(!looks_like("bin", &screen));
    }
}
