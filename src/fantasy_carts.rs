//! PICO-8 and TIC-80 carts kept as text: a PICO-8 .p8, and a TIC-80 cart saved as its code (a .lua with its
//! graphics, map and sounds in comments at the end). Each is edited as the text it is, and saved back as it was;
//! this reads the parts that aren't code, to be shown beside it: the sprite sheet, the map and the sounds.
//!
//! The .p8 format is as PICO-8's manual and the Lexaloffle wiki give it; TIC-80's text carts as its own source
//! writes them (studio/project.c: each row "-- NNN:" and its bytes in hex, low four bits first).

/// Which fantasy console a cart is for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Console {
    Pico8,
    Tic80,
}

impl Console {
    pub fn name(self) -> &'static str {
        match self {
            Console::Pico8 => "PICO-8",
            Console::Tic80 => "TIC-80",
        }
    }
}

/// PICO-8's sixteen colours, as its manual gives them.
pub const PICO8_PALETTE: [[u8; 3]; 16] = [
    [0x00, 0x00, 0x00],
    [0x1D, 0x2B, 0x53],
    [0x7E, 0x25, 0x53],
    [0x00, 0x87, 0x51],
    [0xAB, 0x52, 0x36],
    [0x5F, 0x57, 0x4F],
    [0xC2, 0xC3, 0xC7],
    [0xFF, 0xF1, 0xE8],
    [0xFF, 0x00, 0x4D],
    [0xFF, 0xA3, 0x00],
    [0xFF, 0xEC, 0x27],
    [0x00, 0xE4, 0x36],
    [0x29, 0xAD, 0xFF],
    [0x83, 0x76, 0x9C],
    [0xFF, 0x77, 0xA8],
    [0xFF, 0xCC, 0xAA],
];

/// TIC-80's colours when a cart doesn't give its own: Sweetie 16, its default.
pub const TIC80_PALETTE: [[u8; 3]; 16] = [
    [0x1A, 0x1C, 0x2C],
    [0x5D, 0x27, 0x5D],
    [0xB1, 0x3E, 0x53],
    [0xEF, 0x7D, 0x57],
    [0xFF, 0xCD, 0x75],
    [0xA7, 0xF0, 0x70],
    [0x38, 0xB7, 0x64],
    [0x25, 0x71, 0x79],
    [0x29, 0x36, 0x6F],
    [0x3B, 0x5D, 0xC9],
    [0x41, 0xA6, 0xF6],
    [0x73, 0xEF, 0xF7],
    [0xF4, 0xF4, 0xF4],
    [0x94, 0xB0, 0xC2],
    [0x56, 0x6C, 0x86],
    [0x33, 0x3C, 0x57],
];

/// A sound effect: its speed (PICO-8: frames a note lasts, 1/128 s each; TIC-80: its speed setting) and its notes.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Sound {
    pub number: usize,
    pub speed: u8,
    pub loop_start: u8,
    pub loop_end: u8,
    pub notes: Vec<Note>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Note {
    /// 0 to 63 on PICO-8 (C0 upwards); on TIC-80 a step of the sound's pitch envelope.
    pub pitch: u8,
    pub wave: u8,
    /// 0 (silent) to 7 on PICO-8, 0 to 15 on TIC-80.
    pub volume: u8,
    pub effect: u8,
}

/// A piece of music: which sound each of the four channels plays (None when it's off), and its flags.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Pattern {
    pub number: usize,
    pub flags: u8,
    pub channels: [Option<u8>; 4],
}

/// What a cart holds besides its code.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Cart {
    pub console: Console,
    pub palette: [[u8; 3]; 16],
    /// The sprite sheet, one colour (0 to 15) a pixel, `sheet_width` wide. PICO-8's is 128 by 128; TIC-80's is its
    /// tiles (128 by 128) with its sprites below (128 by 128 more).
    pub sheet: Vec<u8>,
    pub sheet_width: usize,
    /// The map: a sprite's number in each cell, `map_width` cells a row. PICO-8's is 128 by 64 (its lower half
    /// shared with the sheet's lower half), TIC-80's 240 by 136.
    pub map: Vec<u8>,
    pub map_width: usize,
    pub sounds: Vec<Sound>,
    pub music: Vec<Pattern>,
    /// The line each part starts on, for going to it in the text.
    pub sections: Vec<(String, usize)>,
}

impl Cart {
    pub fn sheet_height(&self) -> usize {
        self.sheet.len() / self.sheet_width
    }

    pub fn map_height(&self) -> usize {
        self.map.len() / self.map_width
    }

    /// A sprite's pixels (8 by 8), from the sheet.
    pub fn sprite(&self, number: usize) -> [u8; 64] {
        let per_row = self.sheet_width / 8;
        let (sx, sy) = ((number % per_row) * 8, (number / per_row) * 8);
        let mut pixels = [0; 64];
        for y in 0..8 {
            for x in 0..8 {
                pixels[y * 8 + x] = self.sheet.get((sy + y) * self.sheet_width + sx + x).copied().unwrap_or(0);
            }
        }
        pixels
    }

    /// Whether a sprite has anything drawn in it.
    pub fn sprite_used(&self, number: usize) -> bool {
        self.sprite(number).iter().any(|p| *p != 0)
    }
}

/// Which console a file is a cart for, from its name and the start of its text.
pub fn console_of(name: &str, text: &str) -> Option<Console> {
    let lower = name.to_ascii_lowercase();
    if lower.ends_with(".p8") || text.starts_with("pico-8 cartridge") {
        return Some(Console::Pico8);
    }
    // A TIC-80 cart saved as text says what's in it at its top (-- title:, -- script:), or has its data at its end.
    let tic_ending = [".lua", ".moon", ".fnl", ".js", ".wren", ".nut", ".rb", ".janet", ".py"].iter().any(|e| lower.ends_with(e));
    let looks = text.lines().take(12).any(|l| l.trim_start().starts_with("-- script:") || l.trim_start().starts_with("// script:") || l.trim_start().starts_with("# script:") || l.trim_start().starts_with(";; script:"))
        || text.contains("-- <TILES>")
        || text.contains("-- <PALETTE>");
    (tic_ending && looks).then_some(Console::Tic80)
}

/// Reads a cart from its text.
pub fn read(console: Console, text: &str) -> Cart {
    match console {
        Console::Pico8 => read_p8(text),
        Console::Tic80 => read_tic(text),
    }
}

fn hex(c: char) -> Option<u8> {
    c.to_digit(16).map(|d| d as u8)
}

/// A .p8: a header line, a version, then __lua__, __gfx__, __gff__, __label__, __map__, __sfx__ and __music__,
/// each starting at a line of its own.
fn read_p8(text: &str) -> Cart {
    let mut cart = Cart { console: Console::Pico8, palette: PICO8_PALETTE, sheet: vec![0; 128 * 128], sheet_width: 128, map: vec![0; 128 * 64], map_width: 128, sounds: Vec::new(), music: Vec::new(), sections: Vec::new() };
    let mut section = String::new();
    let mut row = 0;
    for (number, line) in text.lines().enumerate() {
        let trimmed = line.trim_end();
        if trimmed.len() > 4 && trimmed.starts_with("__") && trimmed.ends_with("__") && !trimmed.contains(' ') {
            section = trimmed.trim_matches('_').to_string();
            cart.sections.push((section.clone(), number));
            row = 0;
            continue;
        }
        match section.as_str() {
            "gfx" if row < 128 => {
                for (x, c) in trimmed.chars().take(128).enumerate() {
                    cart.sheet[row * 128 + x] = hex(c).unwrap_or(0);
                }
                row += 1;
            }
            "map" if row < 32 => {
                let digits: Vec<u8> = trimmed.chars().filter_map(hex).collect();
                for (x, pair) in digits.chunks(2).take(128).enumerate() {
                    cart.map[row * 128 + x] = pair[0] << 4 | pair.get(1).copied().unwrap_or(0);
                }
                row += 1;
            }
            "sfx" if row < 64 => {
                let digits: Vec<u8> = trimmed.chars().filter_map(hex).collect();
                if digits.len() >= 8 {
                    let byte = |at: usize| digits[at] << 4 | digits[at + 1];
                    let notes = digits[8..].chunks(5).filter(|n| n.len() == 5).map(|n| Note { pitch: n[0] << 4 | n[1], wave: n[2], volume: n[3], effect: n[4] }).collect();
                    cart.sounds.push(Sound { number: row, speed: byte(2), loop_start: byte(4), loop_end: byte(6), notes });
                }
                row += 1;
            }
            "music" if row < 64 => {
                // "01 41424344": the flags, then each channel's sound, with bit 6 set when the channel's off.
                let digits: Vec<u8> = trimmed.chars().filter_map(hex).collect();
                if digits.len() >= 10 {
                    let byte = |at: usize| digits[at] << 4 | digits[at + 1];
                    let channels = [byte(2), byte(4), byte(6), byte(8)].map(|c| (c & 0x40 == 0).then_some(c & 0x3F));
                    cart.music.push(Pattern { number: row, flags: byte(0), channels });
                }
                row += 1;
            }
            _ => {}
        }
    }
    // The map's lower half is the sheet's lower half: each byte two pixels, the left one in its low four bits.
    for at in 0..0x1000 {
        let (left, right) = (cart.sheet[0x2000 + at * 2], cart.sheet[0x2000 + at * 2 + 1]);
        cart.map[0x1000 + at] = right << 4 | left;
    }
    cart
}

/// A TIC-80 text cart: its code, then sections such as `-- <TILES>`, each row `-- 001:` and hex, ended by
/// `-- </TILES>`. The comment may be another language's (//, #, ;;).
fn read_tic(text: &str) -> Cart {
    let mut cart = Cart { console: Console::Tic80, palette: TIC80_PALETTE, sheet: vec![0; 128 * 256], sheet_width: 128, map: vec![0; 240 * 136], map_width: 240, sounds: Vec::new(), music: Vec::new(), sections: Vec::new() };
    let mut section = String::new();
    for (number, line) in text.lines().enumerate() {
        let rest = line.trim().trim_start_matches(['-', '/', '#', ';']).trim_start();
        if let Some(tag) = rest.strip_prefix('<').and_then(|r| r.strip_suffix('>')) {
            if tag.starts_with('/') {
                section.clear();
            } else {
                section = tag.to_string();
                cart.sections.push((section.clone(), number));
            }
            continue;
        }
        if section.is_empty() {
            continue;
        }
        let Some((index, data)) = rest.split_once(':') else { continue };
        let Ok(index) = index.trim().parse::<usize>() else { continue };
        let digits: Vec<u8> = data.trim().chars().filter_map(hex).collect();
        match section.as_str() {
            // Each tile's 64 pixels, in order; sprites are the 256 after the tiles.
            "TILES" | "SPRITES" if index < 256 => {
                let number = index + if section == "SPRITES" { 256 } else { 0 };
                let (sx, sy) = ((number % 16) * 8, (number / 16) * 8);
                for (at, pixel) in digits.iter().take(64).enumerate() {
                    cart.sheet[(sy + at / 8) * 128 + sx + at % 8] = *pixel;
                }
            }
            // A row of the map: each cell a byte, written low four bits first.
            "MAP" if index < 136 => {
                for (x, pair) in digits.chunks(2).take(240).enumerate() {
                    cart.map[index * 240 + x] = pair.get(1).copied().unwrap_or(0) << 4 | pair[0];
                }
            }
            // The colours, as RGB bytes, written as they are.
            "PALETTE" if index == 0 => {
                for (colour, rgb) in digits.chunks(6).take(16).enumerate() {
                    if rgb.len() == 6 {
                        cart.palette[colour] = [rgb[0] << 4 | rgb[1], rgb[2] << 4 | rgb[3], rgb[4] << 4 | rgb[5]];
                    }
                }
            }
            // A sound effect: only that it's there. (Its 66 bytes hold an envelope for each of volume, wave,
            // arpeggio and pitch, which this doesn't draw.)
            "SFX" if index < 64 && digits.iter().any(|d| *d != 0) => cart.sounds.push(Sound { number: index, ..Sound::default() }),
            _ => {}
        }
    }
    cart
}

/// A picture of the sprite sheet, as RGB, `sheet_width` wide.
pub fn sheet_rgb(cart: &Cart) -> Vec<u8> {
    cart.sheet.iter().flat_map(|c| cart.palette[usize::from(*c & 15)]).collect()
}

/// A picture of the map, each cell its sprite, as RGB: map_width * 8 pixels wide.
pub fn map_rgb(cart: &Cart) -> Vec<u8> {
    let (width, height) = (cart.map_width * 8, cart.map_height() * 8);
    let mut out = vec![0; width * height * 3];
    let sprites: Vec<[u8; 64]> = (0..cart.sheet.len() / 64).map(|n| cart.sprite(n)).collect();
    for (cell, number) in cart.map.iter().enumerate() {
        let (cx, cy) = ((cell % cart.map_width) * 8, (cell / cart.map_width) * 8);
        let sprite = &sprites[usize::from(*number) % sprites.len()];
        for y in 0..8 {
            for x in 0..8 {
                let at = ((cy + y) * width + cx + x) * 3;
                out[at..at + 3].copy_from_slice(&cart.palette[usize::from(sprite[y * 8 + x] & 15)]);
            }
        }
    }
    out
}

/// A note's name, for PICO-8's pitches: C0 is 0.
pub fn note_name(pitch: u8) -> String {
    const NAMES: [&str; 12] = ["C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B"];
    format!("{}{}", NAMES[usize::from(pitch % 12)], pitch / 12)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p8() -> String {
        let mut text = String::from("pico-8 cartridge // http://www.pico-8.com\nversion 41\n__lua__\nfunction _draw()\n cls(1)\n spr(1,60,60)\nend\n__gfx__\n");
        // Sprite 1: a row of colour 8, then colour 12 at its bottom right.
        text.push_str(&format!("{}{}\n", "0".repeat(8), "8".repeat(8)));
        for row in 1..8 {
            text.push_str(&format!("{}{}\n", "0".repeat(8), if row == 7 { "0000000c" } else { "00000000" }));
        }
        // The lower half of the sheet is the map's lower half: pixels 2 and 1 there are map byte $21... or 0x12.
        for row in 8..64 {
            let _ = row;
            text.push_str(&"0".repeat(128));
            text.push('\n');
        }
        text.push_str(&format!("21{}\n", "0".repeat(126)));
        text.push_str("__map__\n0102\n__sfx__\n");
        // A sound: speed 16, a note of pitch 33 (A2), wave 1, volume 5, no effect, then silence.
        text.push_str(&format!("00100000{}{}\n", "21150", "00000".repeat(31)));
        text.push_str("__music__\n01 00414243\n");
        text
    }

    #[test]
    fn a_pico8_carts_sheet_map_and_sounds_are_read() {
        let text = p8();
        assert_eq!(console_of("game.p8", &text), Some(Console::Pico8));
        let cart = read(Console::Pico8, &text);
        assert_eq!(cart.sprite(1)[..8], [8; 8]);
        assert_eq!(cart.sprite(1)[63], 12);
        assert!(cart.sprite_used(1) && !cart.sprite_used(2));
        assert_eq!(&cart.map[..3], &[1, 2, 0]);
        // Sheet row 64's first two pixels (2 and 1) are the map's row 32, cell 0: the left pixel the low four bits.
        assert_eq!(cart.map[32 * 128], 0x12);
        assert_eq!(cart.sounds.len(), 1);
        let sound = &cart.sounds[0];
        assert_eq!((sound.speed, sound.notes.len()), (16, 32));
        assert_eq!(sound.notes[0], Note { pitch: 33, wave: 1, volume: 5, effect: 0 });
        assert_eq!(note_name(33), "A2");
        assert_eq!(cart.music[0].channels, [Some(0), None, None, None]);
        assert_eq!(cart.sections.iter().map(|(name, _)| name.as_str()).collect::<Vec<_>>(), ["lua", "gfx", "map", "sfx", "music"]);
        // The pictures are the right size.
        assert_eq!(sheet_rgb(&cart).len(), 128 * 128 * 3);
        let map = map_rgb(&cart);
        assert_eq!(map.len(), 1024 * 512 * 3);
        // Cell (0,0) is sprite 1: its top row in colour 8.
        assert_eq!(&map[..3], &PICO8_PALETTE[8]);
    }

    #[test]
    fn a_tic80_text_carts_tiles_map_and_palette_are_read() {
        let mut text = String::from("-- title:  game\n-- author: someone\n-- script: lua\n\nfunction TIC()\n cls(0)\nend\n\n");
        text.push_str("-- <TILES>\n");
        text.push_str(&format!("-- 001:{}{}\n", "c".repeat(8), "0".repeat(56)));
        text.push_str("-- </TILES>\n\n-- <SPRITES>\n");
        text.push_str(&format!("-- 000:{}\n", "3".repeat(64)));
        text.push_str("-- </SPRITES>\n\n-- <MAP>\n-- 002:1021\n-- </MAP>\n\n-- <PALETTE>\n-- 000:");
        text.push_str(&"ff0000".repeat(16));
        text.push_str("\n-- </PALETTE>\n");
        assert_eq!(console_of("game.lua", &text), Some(Console::Tic80));
        assert_eq!(console_of("plain.lua", "print('hello')"), None);
        let cart = read(Console::Tic80, &text);
        assert_eq!(cart.sprite(1)[..8], [12; 8]);
        assert_eq!(cart.sprite(256)[0], 3, "the sprites come after the tiles");
        // Bytes are written low four bits first: "10" is 1, "21" is 0x12.
        assert_eq!(&cart.map[2 * 240..2 * 240 + 2], &[1, 0x12]);
        assert_eq!(cart.palette[5], [0xFF, 0, 0]);
        assert_eq!((cart.map_width, cart.map_height(), cart.sheet_height()), (240, 136, 256));
    }

    /// F9 knows a cart for its console, and leaves other Lua alone.
    #[test]
    fn carts_run_on_their_console() {
        use crate::running::{Machine, Runnable};
        assert_eq!(Runnable::from_file("game.p8", p8().into_bytes()).unwrap().machine, Machine::Pico8);
        let tic = "-- title: game\n-- script: lua\nfunction TIC() end\n";
        assert_eq!(Runnable::from_file("game.lua", tic.as_bytes().to_vec()).unwrap().machine, Machine::Tic80);
        assert!(Runnable::from_file("tool.lua", b"print('hello')".to_vec()).is_none());
        assert!(Runnable::from_file("notes.p8", b"not a cart".to_vec()).is_none());
    }
}
