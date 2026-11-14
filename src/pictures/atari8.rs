//! Atari 8-bit (400, 800, XL, XE) screens saved as they are in memory: Graphics 8 (.gr8), Graphics 15 and
//! MicroPainter (.mic), Graphics 9 (.gr9) and Graphics 7 (.gr7). Each row is 40 bytes, top row first. Koala
//! MicroIllustrator's packed pictures and HIP's pairs of screens, further down, are unpacked into the same.
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
    if bytes.starts_with(&KOALA_MAGIC) && matches!(ending, "pic" | "kpi" | "") {
        return true;
    }
    if ending == "hip" {
        return hip_frames(bytes).is_some();
    }
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
    if bytes.starts_with(&KOALA_MAGIC) {
        return decode_koala(bytes);
    }
    if ending == "hip" {
        return decode_hip(bytes);
    }
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
    let colours = match saved {
        Some(c) => [c[1], c[2], c[3], c[0]],
        None => DEFAULT_COLOURS,
    };
    let [pf0, pf1, pf2, bak] = colours;
    let described = match (mode, saved) {
        (Mode::Gr8, _) => "Two: the operating system's starting colours, light blue dots on blue".to_string(),
        (Mode::Gr9, _) if bak & 0xF0 == 0 => "16 shades of grey".to_string(),
        (Mode::Gr9, _) => format!("16 shades of hue {}", bak >> 4),
        (_, Some(_)) => format!("Four, as saved with it: background ${bak:02X}, ${pf0:02X}, ${pf1:02X}, ${pf2:02X}"),
        (_, None) => "Four: the operating system's starting colours, as the file doesn't say".to_string(),
    };
    Ok(paint(mode, bytes, colours, described))
}

/// A screen in one of the modes, in the colours of playfields 0 to 2 and the background, top row first.
fn paint(mode: Mode, bytes: &[u8], [pf0, pf1, pf2, bak]: [u8; 4], described: String) -> Picture {
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
    picture.note("Colours", described);
    picture
}

// ---------- Koala MicroIllustrator ----------
//
// The KoalaPad's Micro Illustrator saved its pictures packed, in .PIC files: &FF &80 &C9 &C7, the header's length
// (two bytes, low first), 1, how the screen is packed (0 not at all, 1 a column at a time, 2 a row at a time), the
// ANTIC mode (&0E for Graphics 15, &0F for Graphics 8 or 9), 0, 40 (the bytes a row), 0, the number of rows, the
// colours of playfields 0 to 3 and the background, two bytes for the packed length, the GTIA mode (0, or 1 for
// Graphics 9) and 0; the packed screen follows the header. Packing is in runs: a byte under &80 is a count of
// one byte repeated, the byte next; &80 and over, &80 less than a count of bytes as they are. A count of 0 is
// followed by the count in two bytes, high first. Packed a column at a time, each of the 40 columns is the even
// rows' bytes, then the odd rows'. This is as the RECOIL picture decoder (recoil.sourceforge.net) reads them,
// which the files tried here were checked against.

const KOALA_MAGIC: [u8; 4] = [0xFF, 0x80, 0xC9, 0xC7];

/// Bytes from packed runs, one at a time.
struct Runs<'a> {
    bytes: &'a [u8],
    at: usize,
    left: usize,
    repeated: Option<u8>,
}

impl Runs<'_> {
    fn next(&mut self) -> Option<u8> {
        while self.left == 0 {
            let command = *self.bytes.get(self.at)?;
            self.at += 1;
            let count = usize::from(command & 0x7F);
            self.left = if count == 0 {
                let wide = self.bytes.get(self.at..self.at + 2)?;
                self.at += 2;
                usize::from(u16::from_be_bytes([wide[0], wide[1]]))
            } else {
                count
            };
            self.repeated = None;
            if command < 0x80 {
                self.repeated = Some(*self.bytes.get(self.at)?);
                self.at += 1;
            }
        }
        self.left -= 1;
        match self.repeated {
            Some(byte) => Some(byte),
            None => {
                self.at += 1;
                self.bytes.get(self.at - 1).copied()
            }
        }
    }
}

fn decode_koala(bytes: &[u8]) -> Result<Picture, String> {
    let header = bytes.get(..22).ok_or("This Koala picture stops in its header.")?;
    let length = usize::from(u16::from_le_bytes([header[4], header[5]]));
    if header[6] != 1 || header[9..12] != [0, 40, 0] || header[21] != 0 || length < 26 {
        return Err("This file starts as a Koala picture does, but its header isn't one.".into());
    }
    let rows = usize::from(header[12]);
    let mode = match (header[20], header[8], rows) {
        (0, 0x0E, 192) => Mode::Gr15,
        (1, 0x0F, 192) => Mode::Gr9,
        (0, 0x02, _) => return Err("This Koala file is a text screen, which isn't shown as a picture.".into()),
        _ => return Err(format!("This Koala picture is in a mode (ANTIC {}, GTIA {}, {rows} rows) that isn't read.", header[8], header[20])),
    };
    let mut screen = vec![0u8; SCREEN];
    let packed = bytes.get(length + 1..).unwrap_or_default();
    let short = || "This Koala picture's packed screen stops before its end, so the file is cut short or damaged.".to_string();
    match header[7] {
        0 if packed.len() == SCREEN => screen.copy_from_slice(packed),
        0 => return Err(short()),
        packing @ (1 | 2) => {
            let mut runs = Runs { bytes: packed, at: 0, left: 0, repeated: None };
            let order: Vec<usize> = if packing == 2 { (0..SCREEN).collect() } else { (0..40).flat_map(|x| [x, x + 40]).flat_map(|first| (first..SCREEN).step_by(80)).collect() };
            for at in order {
                screen[at] = runs.next().ok_or_else(short)?;
            }
        }
        other => return Err(format!("This Koala picture is packed in a way ({other}) that isn't known.")),
    }
    let colours = [header[13], header[14], header[15], header[17]];
    let [pf0, pf1, pf2, bak] = colours;
    let described = match mode {
        Mode::Gr9 if bak & 0xF0 == 0 => "16 shades of grey".to_string(),
        Mode::Gr9 => format!("16 shades of hue {}", bak >> 4),
        _ => format!("Four, as saved with it: background ${bak:02X}, ${pf0:02X}, ${pf1:02X}, ${pf2:02X}"),
    };
    let mut picture = paint(mode, &screen, colours, described);
    picture.info.insert(0, ("Format".into(), "Koala MicroIllustrator (packed)".into()));
    Ok(picture)
}

// ---------- HIP ----------
//
// A HIP picture (from the Polish demo scene, around 1995) is two screens shown on alternate frames, so that the
// eye mixes them: one in Graphics 9 (16 shades of grey) and one in Graphics 10 (nine colour registers, here set
// to shades of grey), shifted half a Graphics 9 dot from each other, giving twice the detail across and more
// shades. A .HIP file is either both screens as a binary load file, each with its &FF &FF and addresses (the
// Graphics 10 one first), or the Graphics 9 screen then the Graphics 10 one, perhaps followed by the nine
// Graphics 10 colours. Each row is 40 bytes, two dots a byte, high half first.

/// The Graphics 10 colours a HIP picture is shown in when it doesn't give its own: black, then shades of grey.
const HIP_SHADES: [u8; 9] = [0x00, 0x00, 0x02, 0x04, 0x06, 0x08, 0x0A, 0x0C, 0x0E];

/// A HIP file's Graphics 9 screen, its Graphics 10 screen, its number of rows and its Graphics 10 colours.
fn hip_frames(bytes: &[u8]) -> Option<(&[u8], &[u8], usize, [u8; 9])> {
    let load_length = |at: usize| -> Option<usize> {
        let header = bytes.get(at..at + 6)?;
        let (start, end) = (u16::from_le_bytes([header[2], header[3]]), u16::from_le_bytes([header[4], header[5]]));
        (header[..2] == [0xFF, 0xFF] && end >= start).then(|| usize::from(end - start) + 1)
    };
    if let Some(length) = load_length(0).filter(|&n| n % 40 == 0 && 12 + 2 * n == bytes.len() && load_length(6 + n) == Some(n)) {
        let rows = length / 40;
        return (1..=240).contains(&rows).then(|| (&bytes[12 + length..], &bytes[6..6 + length], rows, HIP_SHADES));
    }
    let rows = bytes.len() / 80;
    if !(1..=240).contains(&rows) || !matches!(bytes.len() % 80, 0 | 9) {
        return None;
    }
    let colours = match bytes.len() % 80 {
        9 => bytes[bytes.len() - 9..].try_into().ok()?,
        _ => HIP_SHADES,
    };
    Some((&bytes[..rows * 40], &bytes[rows * 40..rows * 80], rows, colours))
}

fn decode_hip(bytes: &[u8]) -> Result<Picture, String> {
    let (gr9, gr10, rows, colours) = hip_frames(bytes).ok_or("This isn't a HIP picture: it isn't two screens of 40-byte rows, with or without load headers.")?;
    // A dot's value, from 320 hires columns; Graphics 9's and 10's dots are four columns wide.
    let dot = |screen: &[u8], y: usize, column: usize| screen[y * 40 + column / 8] >> (if column & 4 == 0 { 4 } else { 0 }) & 15;
    // Graphics 10's sixteen values: the four players' colours, the four playfields', the background, then the
    // background again and the playfields again. A colour's lowest bit makes no difference outside Graphics 9.
    let gr10_colour = |value: u8| {
        let register = match value {
            0..=8 => value,
            9..=11 => 8,
            _ => value - 8,
        };
        colours[usize::from(register)] & 0xFE
    };
    let mut picture = Picture::new(320, rows);
    for y in 0..rows {
        for x in 0..320 {
            // The Graphics 9 screen is a column to the left and the Graphics 10 screen a column to the right, so
            // they're half a dot apart; at the edges black and player 0's colour show.
            let first = if x + 1 < 320 { dot(gr9, y, x + 1) } else { 0 };
            let second = if x == 0 { colours[0] & 0xFE } else { gr10_colour(dot(gr10, y, x - 1)) };
            let (a, b) = (colour(first), colour(second));
            picture.set(x, y, [0, 1, 2].map(|i| ((u16::from(a[i]) + u16::from(b[i])) / 2) as u8));
        }
    }
    picture.aspect = HIRES_ASPECT;
    picture.note("Format", "HIP: Graphics 9 and 10 screens on alternate frames, mixed");
    picture.note("Size", format!("320\u{D7}{rows}"));
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

    /// A Koala picture of a Graphics 15 screen, packed in runs as Micro Illustrator packs it, a column at a time
    /// (`packing` 1) or a row at a time (2).
    fn koala(screen: &[u8], packing: u8) -> Vec<u8> {
        let mut file = KOALA_MAGIC.to_vec();
        file.extend(26u16.to_le_bytes());
        file.extend([1, packing, 0x0E, 0, 40, 0, 192, 0x46, 0x86, 0xC6, 0x00, 0x00, 0, 0, 0, 0]);
        file.resize(27, 0);
        let order: Vec<usize> = if packing == 2 { (0..SCREEN).collect() } else { (0..40).flat_map(|x| [x, x + 40]).flat_map(|first| (first..SCREEN).step_by(80)).collect() };
        let bytes: Vec<u8> = order.iter().map(|&at| screen[at]).collect();
        // Runs of one byte where they're long, a long count after a 0, and the rest as literals.
        let mut at = 0;
        while at < bytes.len() {
            let run = bytes[at..].iter().take_while(|&&b| b == bytes[at]).count();
            if run >= 300 {
                file.extend([0, (run >> 8) as u8, run as u8, bytes[at]]);
                at += run;
            } else if run >= 3 {
                let run = run.min(127);
                file.extend([run as u8, bytes[at]]);
                at += run;
            } else {
                let count = (bytes.len() - at).min(100);
                file.push(0x80 | count as u8);
                file.extend(&bytes[at..at + count]);
                at += count;
            }
        }
        file
    }

    #[test]
    fn koala_pictures_unpack_in_either_order() {
        let mut screen = vec![0u8; SCREEN];
        screen[40] = 0b0110_1100;
        for (at, byte) in screen.iter_mut().enumerate().skip(4000).take(500) {
            *byte = (at * 37) as u8;
        }
        let plain = {
            let mut mic = screen.clone();
            mic.extend([0x00, 0x46, 0x86, 0xC6]);
            decode("mic", &mic).unwrap()
        };
        for packing in [1, 2] {
            let file = koala(&screen, packing);
            assert!(looks_like("pic", &file));
            let picture = decode("pic", &file).unwrap();
            assert_eq!(picture.rgba, plain.rgba, "packed {packing}");
            assert_eq!(picture.info[0].1, "Koala MicroIllustrator (packed)");
            for cut in (0..file.len()).step_by(7) {
                let _ = decode("pic", &file[..cut]);
            }
        }
        let short = koala(&screen, 2);
        assert!(decode("pic", &short[..short.len() - 20]).unwrap_err().contains("cut short"));
    }

    #[test]
    fn hip_pictures_mix_their_two_screens() {
        let mut gr9 = vec![0u8; 40 * 2];
        let mut gr10 = vec![0u8; 40 * 2];
        gr9[0] = 0xF0;
        gr10[0] = 0x80;
        let mut raw = gr9.clone();
        raw.extend(&gr10);
        assert!(looks_like("hip", &raw) && !looks_like("hip", &raw[..100]));
        let picture = decode("hip", &raw).unwrap();
        assert_eq!((picture.width, picture.height), (320, 2));
        // Column 1: Graphics 9's first dot (15) and Graphics 10's first dot (register 8, the grey &0E), mixed.
        let mix = |a: u8, b: u8| [0, 1, 2].map(|i| ((u16::from(colour(a)[i]) + u16::from(colour(b)[i])) / 2) as u8);
        assert_eq!(pixel(&picture, 1, 0), mix(0x0F, 0x0E));
        assert_eq!(pixel(&picture, 0, 0), mix(0x0F, 0x00), "player 0's colour shows at the left edge");
        assert_eq!(pixel(&picture, 319, 1), mix(0x00, 0x00));
        // The same as a binary load file: the Graphics 10 screen first, each with its header.
        let mut load = vec![0xFF, 0xFF, 0x10, 0x60, 0x5F, 0x60];
        load.extend(&gr10);
        load.extend([0xFF, 0xFF, 0x10, 0x80, 0x5F, 0x80]);
        load.extend(&gr9);
        assert_eq!(decode("hip", &load).unwrap().rgba, picture.rgba);
        assert!(decode("hip", &raw[..79]).is_err());
    }
}
