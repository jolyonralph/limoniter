//! Commodore 64 bitmap pictures, saved as the painting programs left them in memory: a two-byte load address, then
//! the 8000-byte bitmap and its colours. They're known by their size and load address together:
//! - Koala Painter (load $6000, 10003 bytes): multicolour bitmap, screen RAM, colour RAM, background colour.
//! - OCP Art Studio (load $2000, 9009 bytes): hires bitmap, screen RAM, border colour.
//! - OCP Advanced Art Studio (load $2000, 10018 bytes): multicolour bitmap, screen RAM, border, background, 14
//!   spare bytes, colour RAM.
//! - Doodle! (load $5C00, 9218 bytes): screen RAM in a 1024-byte block, then a hires bitmap.
//! - FLI Designer (load $3C00, 17409 bytes): colour RAM, eight screen RAMs (one for each line of a cell), each in a
//!   1024-byte block, then a multicolour bitmap; the background is black.
//! - AFLI (load $4000, 16385 bytes): eight screen RAMs, then a hires bitmap.
//!
//! In the FLI ones the VIC-II can't show the first three columns' colours (the "FLI bug"), so they're left out, as
//! RECOIL leaves them out: the picture is 296 pixels across.
//!
//! The bitmap is in 8x8 cells, left to right and down, each cell's 8 bytes its rows. In hires each bit is a pixel,
//! the screen RAM's high nibble for 1 and low nibble for 0. In multicolour each pair of bits is a pixel twice as
//! wide: 00 the background, 01 the screen RAM's high nibble, 10 its low nibble, 11 colour RAM. The colours are the
//! ones the preview's PETSCII screens use, Pepto's measurements of the VIC-II.

use super::Picture;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Kind {
    Koala,
    ArtStudio,
    AdvancedArtStudio,
    Doodle,
    Fli,
    Afli,
}

fn kind(bytes: &[u8]) -> Option<Kind> {
    let load = u16::from_le_bytes([*bytes.first()?, *bytes.get(1)?]);
    match (load, bytes.len()) {
        // Some copies have a byte or three of padding at the end.
        (0x6000, 10003..=10006) => Some(Kind::Koala),
        (0x2000, 9009) => Some(Kind::ArtStudio),
        (0x2000, 10018) => Some(Kind::AdvancedArtStudio),
        (0x5C00, 9218) => Some(Kind::Doodle),
        (0x3C00, 17409) => Some(Kind::Fli),
        (0x4000, 16385) => Some(Kind::Afli),
        _ => None,
    }
}

pub fn looks_like(bytes: &[u8]) -> bool {
    kind(bytes).is_some()
}

fn colour(index: u8) -> [u8; 3] {
    let c = crate::retro_screens::C64[usize::from(index & 15)];
    [c.r(), c.g(), c.b()]
}

pub fn decode(bytes: &[u8]) -> Result<Picture, String> {
    let kind = kind(bytes).ok_or("This isn't a Commodore 64 picture Limoniter knows.")?;
    if matches!(kind, Kind::Fli | Kind::Afli) {
        return Ok(decode_fli(bytes, kind == Kind::Fli));
    }
    // Where the bitmap, screen RAM and colour RAM are, the background colour, and whether it's multicolour.
    let (bitmap, screen, colours, background, multicolour, name) = match kind {
        Kind::Koala => (2, 8002, Some(9002), bytes[10002], true, "Koala Painter"),
        Kind::ArtStudio => (2, 8002, None, 0, false, "OCP Art Studio"),
        Kind::AdvancedArtStudio => (2, 8002, Some(9018), bytes[9003], true, "OCP Advanced Art Studio"),
        Kind::Doodle | Kind::Fli | Kind::Afli => (1026, 2, None, 0, false, "Doodle!"),
    };
    let mut picture = Picture::new(320, 200);
    for cell_y in 0..25 {
        for cell_x in 0..40 {
            let cell = cell_y * 40 + cell_x;
            let screen_byte = bytes[screen + cell];
            let colour_byte = colours.map_or(0, |at| bytes[at + cell]);
            for row in 0..8 {
                let pixels = bytes[bitmap + cell * 8 + row];
                let y = cell_y * 8 + row;
                for x in 0..8 {
                    let index = if multicolour {
                        match (pixels >> (6 - (x & 6))) & 3 {
                            0 => background,
                            1 => screen_byte >> 4,
                            2 => screen_byte & 15,
                            _ => colour_byte,
                        }
                    } else if pixels & (0x80 >> x) != 0 {
                        screen_byte >> 4
                    } else {
                        screen_byte & 15
                    };
                    picture.set(cell_x * 8 + x, y, colour(index));
                }
            }
        }
    }
    picture.note("Size", if multicolour { "160\u{D7}200, each pixel twice as wide (multicolour)" } else { "320\u{D7}200 (hires)" });
    picture.note("Colours", if multicolour { "4 in each 4\u{D7}8 cell, from the C64's 16" } else { "2 in each 8\u{D7}8 cell, from the C64's 16" });
    picture.note("Saved by", name);
    Ok(picture)
}

/// FLI and AFLI: the screen RAM for each line is the block for its place in the cell.
fn decode_fli(bytes: &[u8], multicolour: bool) -> Picture {
    let (colours, screens, bitmap) = if multicolour { (2, 0x402, 0x2402) } else { (0, 2, 0x2002) };
    let at = |i: usize| bytes.get(i).copied().unwrap_or(0);
    let mut picture = Picture::new(296, 200);
    for y in 0..200 {
        for x in 24..320 {
            let cell = (y / 8) * 40 + x / 8;
            let pixels = at(bitmap + cell * 8 + (y & 7));
            let screen_byte = at(screens + (y & 7) * 1024 + cell);
            let index = if multicolour {
                match (pixels >> (6 - (x & 6))) & 3 {
                    0 => 0,
                    1 => screen_byte >> 4,
                    2 => screen_byte & 15,
                    _ => at(colours + cell),
                }
            } else if pixels & (0x80 >> (x & 7)) != 0 {
                screen_byte >> 4
            } else {
                screen_byte & 15
            };
            picture.set(x - 24, y, colour(index));
        }
    }
    picture.note("Size", if multicolour { "296\u{D7}200, each pixel twice as wide (FLI; the first 3 columns can't be shown)" } else { "296\u{D7}200 (AFLI; the first 3 columns can't be shown)" });
    picture.note("Colours", if multicolour { "Black, one for the cell and 2 more in each 4\u{D7}1, from the C64's 16" } else { "2 in each 8\u{D7}1, from the C64's 16" });
    picture.note("Saved by", if multicolour { "FLI Designer" } else { "AFLI editor" });
    picture
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pixel(picture: &Picture, x: usize, y: usize) -> [u8; 3] {
        let at = (y * 320 + x) * 4;
        [picture.rgba[at], picture.rgba[at + 1], picture.rgba[at + 2]]
    }

    #[test]
    fn koala_multicolour_picks_its_four_colours() {
        let mut file = vec![0x00, 0x60];
        file.resize(10003, 0);
        // Cell 0's first row: pixel pairs 00, 01, 10, 11.
        file[2] = 0b00_01_10_11;
        file[8002] = 0x25; // screen RAM: red (2) high, green (5) low
        file[9002] = 0x07; // colour RAM: yellow
        file[10002] = 6; // background: blue
        let picture = decode(&file).unwrap();
        assert_eq!(pixel(&picture, 0, 0), colour(6));
        assert_eq!(pixel(&picture, 1, 0), colour(6), "a multicolour pixel is two wide");
        assert_eq!(pixel(&picture, 2, 0), colour(2));
        assert_eq!(pixel(&picture, 4, 0), colour(5));
        assert_eq!(pixel(&picture, 6, 0), colour(7));
        // The second cell along starts 8 bytes on.
        assert_eq!(pixel(&picture, 8, 0), colour(6));
    }

    #[test]
    fn doodle_hires_has_its_screen_ram_first() {
        let mut file = vec![0x00, 0x5C];
        file.resize(9218, 0);
        file[2] = 0x10; // white on black
        file[1026] = 0x80;
        let picture = decode(&file).unwrap();
        assert_eq!(pixel(&picture, 0, 0), colour(1));
        assert_eq!(pixel(&picture, 1, 0), colour(0));
        assert!(!looks_like(&[0x00, 0x60, 0, 0]));
    }
}
