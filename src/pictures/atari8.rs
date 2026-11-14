//! Atari 8-bit (400, 800, XL, XE) screens saved as they are in memory: Graphics 8 (.gr8), Graphics 15 and
//! MicroPainter (.mic), Graphics 9 (.gr9) and Graphics 7 (.gr7). Each row is 40 bytes, top row first.
//!
//! Graphics 8 is 320×192 in two colours: each bit a dot, set dots in the luminance of colour register 1 on the hue
//! of register 2. Graphics 15 is 160×192 and Graphics 7 160×96, both four colours: two bits a dot, 0 the
//! background (COLBK), 1 to 3 colour registers 0 to 2; MicroPainter files may carry those four registers after the
//! screen, background first. Graphics 9 is 80×192 in 16 luminances of one hue (the background register's), four
//! bits a dot.
//!
//! A colour is a byte: its top four bits a hue, the next three a luminance. The hues are as an NTSC Atari shows
//! them: 0 grey, then round the colour wheel from the colour burst's gold, a fifteenth of a turn less than a full
//! circle for the fifteen, so that hue 15 comes back close to hue 1. A dot is not square: on an NTSC set a Graphics 8
//! dot is 6/7 as wide as a scan line is tall, and the coarser modes' dots are two or four times wider than that.

use super::Picture;

/// The size of a whole Graphics 7, 8, 9 or 15 screen.
const SCREEN: usize = 7680;
/// The width of a Graphics 8 dot for its height, on an NTSC set.
const HIRES_ASPECT: f32 = 6.0 / 7.0;
/// The operating system's colours when a program starts: COLOR0 to COLOR2, and the background, COLOR4.
const DEFAULT_COLOURS: [u8; 4] = [0x28, 0xCA, 0x94, 0x00];

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Mode {
    Gr8,
    Gr15,
    Gr9,
    Gr7,
}

fn mode_of(ending: &str) -> Option<Mode> {
    match ending {
        "gr8" => Some(Mode::Gr8),
        // MicroPainter pictures were often saved as .PIC, and as .KMP by the ABBUC disks, at the same size.
        "mic" | "gr15" | "pic" | "kmp" => Some(Mode::Gr15),
        "gr9" => Some(Mode::Gr9),
        "gr7" => Some(Mode::Gr7),
        _ => None,
    }
}

/// Whether a file is one of these screens: its ending, and a size that's a screen and at most its colours.
pub fn looks_like(ending: &str, bytes: &[u8]) -> bool {
    match mode_of(ending) {
        Some(Mode::Gr7) => (SCREEN / 2..=SCREEN / 2 + 5).contains(&bytes.len()),
        Some(_) => (SCREEN..=SCREEN + 5).contains(&bytes.len()),
        None => false,
    }
}

/// An NTSC Atari colour as red, green and blue: the luminance as brightness, the hue as an angle on the colour
/// wheel in YUV, hue 1 at the colour burst's phase and each after it 360/14 degrees on.
pub fn colour(byte: u8) -> [u8; 3] {
    let hue = byte >> 4;
    let luma = f32::from(byte & 15) / 15.0;
    let (u, v) = if hue == 0 {
        (0.0, 0.0)
    } else {
        let angle = (180.0 - f32::from(hue - 1) * 360.0 / 14.0).to_radians();
        (0.2 * angle.cos(), 0.2 * angle.sin())
    };
    let scale = |x: f32| (x.clamp(0.0, 1.0) * 255.0).round() as u8;
    [scale(luma + 1.140 * v), scale(luma - 0.395 * u - 0.581 * v), scale(luma + 2.032 * u)]
}

pub fn decode(ending: &str, bytes: &[u8]) -> Result<Picture, String> {
    let mut mode = mode_of(ending).unwrap_or(if bytes.len() < SCREEN { Mode::Gr7 } else { Mode::Gr15 });
    let needed = if mode == Mode::Gr7 { SCREEN / 2 } else { SCREEN };
    if bytes.len() < needed {
        return Err(format!("An Atari {} screen is {needed} bytes; this file has {}.", name(mode), bytes.len()));
    }
    // MicroPainter's colours after the screen: background, then registers 0 to 2.
    let saved = bytes.get(needed..needed + 4).filter(|_| matches!(mode, Mode::Gr15 | Mode::Gr7));
    // Four saved colours all as dark as black would show nothing: such a file is a Graphics 9 picture (a digitised
    // photograph, often) kept the MicroPainter way, its first colour giving the hue its shades are in.
    if mode == Mode::Gr15 && saved.is_some_and(|c| c.iter().all(|b| b & 0x0E == 0) && c.iter().any(|b| b & 0xF0 != 0)) {
        mode = Mode::Gr9;
    }
    let [pf0, pf1, pf2, bak] = match saved {
        Some(c) => [c[1], c[2], c[3], c[0]],
        None => DEFAULT_COLOURS,
    };
    let (width, height) = match mode {
        Mode::Gr8 => (320, 192),
        Mode::Gr15 => (160, 192),
        Mode::Gr9 => (80, 192),
        Mode::Gr7 => (160, 96),
    };
    let mut picture = Picture::new(width, height);
    for y in 0..height {
        let row = &bytes[y * 40..y * 40 + 40];
        for x in 0..width {
            let rgb = match mode {
                Mode::Gr8 => {
                    let on = row[x / 8] >> (7 - x % 8) & 1 != 0;
                    colour(if on { pf2 & 0xF0 | pf1 & 0x0E } else { pf2 & 0xFE })
                }
                // Outside Graphics 9 the lowest bit of a colour's luminance makes no difference.
                Mode::Gr15 | Mode::Gr7 => colour([bak, pf0, pf1, pf2][usize::from(row[x / 4] >> (6 - 2 * (x % 4)) & 3)] & 0xFE),
                Mode::Gr9 => colour(bak & 0xF0 | row[x / 2] >> (4 - 4 * (x % 2)) & 15),
            };
            picture.set(x, y, rgb);
        }
    }
    picture.aspect = match mode {
        Mode::Gr8 => HIRES_ASPECT,
        Mode::Gr15 => HIRES_ASPECT * 2.0,
        Mode::Gr9 => HIRES_ASPECT * 4.0,
        Mode::Gr7 => HIRES_ASPECT,
    };
    picture.note("Mode", name(mode));
    picture.note("Size", format!("{width}\u{D7}{height}"));
    let colours = match (mode, saved) {
        (Mode::Gr8, _) => "Two: the operating system's starting colours, light blue dots on blue".to_string(),
        (Mode::Gr9, _) if bak & 0xF0 == 0 => "16 shades of grey".to_string(),
        (Mode::Gr9, _) => format!("16 shades of hue {}", bak >> 4),
        (_, Some(_)) => format!("Four, as saved with it: background ${bak:02X}, ${pf0:02X}, ${pf1:02X}, ${pf2:02X}"),
        (_, None) => "Four: the operating system's starting colours, as the file doesn't say".to_string(),
    };
    picture.note("Colours", colours);
    Ok(picture)
}

fn name(mode: Mode) -> &'static str {
    match mode {
        Mode::Gr8 => "Graphics 8, 320\u{D7}192 in two colours",
        Mode::Gr15 => "Graphics 15 (MicroPainter), 160\u{D7}192 in four colours",
        Mode::Gr9 => "Graphics 9, 80\u{D7}192 in 16 shades",
        Mode::Gr7 => "Graphics 7, 160\u{D7}96 in four colours",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pixel(picture: &Picture, x: usize, y: usize) -> [u8; 3] {
        let at = (y * picture.width + x) * 4;
        [picture.rgba[at], picture.rgba[at + 1], picture.rgba[at + 2]]
    }

    #[test]
    fn the_palette_has_grey_and_the_hues_round_the_wheel() {
        assert_eq!(colour(0x00), [0, 0, 0]);
        assert_eq!(colour(0x0F), [255, 255, 255]);
        let [r, g, b] = colour(0x46);
        assert!(r > g && r > b, "hue 4 is red: {r} {g} {b}");
        let [r, g, b] = colour(0x86);
        assert!(b > r && b > g, "hue 8 is blue: {r} {g} {b}");
        let [r, g, b] = colour(0xC6);
        assert!(g > r && g > b, "hue 12 is green: {r} {g} {b}");
        let [r, g, b] = colour(0x16);
        assert!(r > b && g > b, "hue 1 is gold: {r} {g} {b}");
    }

    #[test]
    fn each_mode_reads_its_dots() {
        let mut screen = vec![0u8; SCREEN];
        screen[0] = 0b1000_0000;
        screen[40] = 0b0110_1100;
        let gr8 = decode("gr8", &screen).unwrap();
        assert_eq!((gr8.width, gr8.height), (320, 192));
        assert_eq!(pixel(&gr8, 0, 0), colour(0x9A));
        assert_eq!(pixel(&gr8, 1, 0), colour(0x94));
        let mut mic = screen.clone();
        mic.extend([0x00, 0x46, 0x86, 0xC6]);
        assert!(looks_like("mic", &mic));
        let gr15 = decode("mic", &mic).unwrap();
        assert_eq!((gr15.width, gr15.height), (160, 192));
        assert_eq!([0, 1, 2, 3].map(|x| pixel(&gr15, x, 1)), [colour(0x46), colour(0x86), colour(0xC6), colour(0x00)]);
        let gr9 = decode("gr9", &screen).unwrap();
        assert_eq!(pixel(&gr9, 0, 0), colour(0x08));
        assert_eq!(pixel(&gr9, 1, 1), colour(0x0C));
        let mut photo = screen.clone();
        photo.extend([0x10, 0xF0, 0xC0, 0x20]);
        let photo = decode("pic", &photo).unwrap();
        assert_eq!(photo.width, 80, "four black colours mean Graphics 9 shades");
        assert_eq!(pixel(&photo, 0, 0), colour(0x18));
        let gr7 = decode("gr7", &screen[..3840]).unwrap();
        assert_eq!((gr7.width, gr7.height), (160, 96));
        assert!(decode("gr8", &screen[..100]).is_err());
        assert!(!looks_like("gr8", &screen[..100]));
        assert!(!looks_like("bin", &screen));
    }
}
