//! What Commodore and teletext files look like on their own screens, for the preview.
//!
//! - PETSCII streams (`.SEQ` files, printed text): the C64, VIC-20 and PET's own handling of colour codes, reverse,
//!   cursor movement and clearing, read from the Commodore 64 Programmer's Reference Guide's table of codes.
//! - Commodore screen codes: screen memory as it is, one byte a cell, with the reversed half drawn reversed.
//! - Teletext and BBC Mode 7: level 1 spacing attributes as ETS 300 706 describes them, with set-at and
//!   set-after controls, held mosaics, double height, concealing and the background controls.
//!
//! The glyphs come from the same tables the editor uses, so the national sets and both Commodore sets match.

use crate::charset::Charset;
use crate::screen::{Cell, Height, Mosaic, Screen, rgb};
use eframe::egui::Color32;

/// The C64's colours, as measured by Philip Timmermann ("Pepto").
pub const C64: [Color32; 16] = [
    rgb(0x000000),
    rgb(0xFFFFFF),
    rgb(0x68372B),
    rgb(0x70A4B2),
    rgb(0x6F3D86),
    rgb(0x588D43),
    rgb(0x352879),
    rgb(0xB8C76F),
    rgb(0x6F4F25),
    rgb(0x433900),
    rgb(0x9A6759),
    rgb(0x444444),
    rgb(0x6C6C6C),
    rgb(0x9AD284),
    rgb(0x6C5EB5),
    rgb(0x959595),
];

/// The VIC-20's first eight colours, which are the ones its colour codes reach.
pub const VIC20: [Color32; 8] = [rgb(0x000000), rgb(0xFFFFFF), rgb(0x782922), rgb(0x87D6DD), rgb(0xAA5FB6), rgb(0x55A049), rgb(0x40318D), rgb(0xBFCE72)];

/// Teletext's eight full-intensity colours.
pub const TELETEXT: [Color32; 8] = [rgb(0x000000), rgb(0xFF0000), rgb(0x00FF00), rgb(0xFFFF00), rgb(0x0000FF), rgb(0xFF00FF), rgb(0x00FFFF), rgb(0xFFFFFF)];

const PET_GREEN: Color32 = rgb(0x41FF00);

/// The ZX Spectrum's eight colours, at normal and BRIGHT brightness.
pub const SPECTRUM: [Color32; 8] = [rgb(0x000000), rgb(0x0000D7), rgb(0xD70000), rgb(0xD700D7), rgb(0x00D700), rgb(0x00D7D7), rgb(0xD7D700), rgb(0xD7D7D7)];
pub const SPECTRUM_BRIGHT: [Color32; 8] = [rgb(0x000000), rgb(0x0000FF), rgb(0xFF0000), rgb(0xFF00FF), rgb(0x00FF00), rgb(0x00FFFF), rgb(0xFFFF00), rgb(0xFFFFFF)];

/// Prints ZX Spectrum text onto its 32-column screen, following the colour controls PRINT follows: INK, PAPER,
/// FLASH, BRIGHT, INVERSE and OVER each with a value, AT with a row and column, TAB with a column, the comma
/// control and the cursor moves. Keywords print as their words, UDGs as the letters they start out as.
/// From the ZX Spectrum BASIC manual's character set and control codes. Lines run on down the page.
pub fn spectrum(data: &[u8], set: &Charset) -> Screen {
    let paper_of = |paper: usize, bright: bool| if bright { SPECTRUM_BRIGHT[paper] } else { SPECTRUM[paper] };
    let mut screen = Screen::new(32, 1.0, Cell::blank(SPECTRUM[0], SPECTRUM[7]));
    screen.border = Some(SPECTRUM[7]);
    let (mut row, mut col) = (0usize, 0usize);
    let (mut ink, mut paper, mut bright, mut flash, mut inverse) = (0usize, 7usize, false, false, false);
    let mut over = false;
    let mut i = 0;
    let put = |screen: &mut Screen, row: &mut usize, col: &mut usize, ch: char, ink: usize, paper: usize, bright: bool, flash: bool, inverse: bool| {
        if *col >= 32 {
            *col = 0;
            *row += 1;
        }
        let (fg, bg) = (paper_of(ink, bright), paper_of(paper, bright));
        let (fg, bg) = if inverse { (bg, fg) } else { (fg, bg) };
        screen.put(*row, *col, Cell { ch, fg, bg, flash, ..screen.paper });
        *col += 1;
    };
    while i < data.len() && row < crate::screen::MOST_ROWS {
        let byte = data[i];
        i += 1;
        let value = |k: usize| data.get(i + k).copied().unwrap_or(0);
        match byte {
            0x0D => {
                row += 1;
                col = 0;
            }
            0x06 => {
                col = if col < 16 { 16 } else { 32 };
            }
            0x08 => col = col.saturating_sub(1),
            0x09 => col += 1,
            0x10..=0x15 => {
                let v = usize::from(value(0));
                i += 1;
                match byte {
                    0x10 if v < 8 => ink = v,
                    0x11 if v < 8 => paper = v,
                    0x12 => flash = v == 1,
                    0x13 => bright = v == 1,
                    0x14 => inverse = v == 1,
                    0x15 => over = v == 1,
                    _ => {}
                }
            }
            0x16 => {
                (row, col) = (usize::from(value(0)), usize::from(value(1)).min(31));
                i += 2;
            }
            0x17 => {
                let to = usize::from(value(0)) % 32;
                i += 2;
                if to < col {
                    row += 1;
                }
                col = to;
            }
            0x20..=0x8F => {
                let ch = set.entry(0, byte).and_then(|e| e.unicode).unwrap_or(' ');
                put(&mut screen, &mut row, &mut col, ch, ink, paper, bright, flash, inverse);
            }
            0x90..=0xA4 => {
                // A UDG shows the capital it's defined as when the machine starts.
                let ch = set.entry(0, byte).and_then(|e| e.name.chars().last()).unwrap_or(' ');
                put(&mut screen, &mut row, &mut col, ch, ink, paper, bright, flash, inverse);
            }
            _ => {
                let word = set.entry(0, byte).and_then(|e| e.name.strip_prefix("BASIC keyword ")).unwrap_or("");
                let alphabetic = word.starts_with(|c: char| c.is_ascii_alphabetic());
                // As the ROM prints a keyword: a space before it unless there's one already, and one after.
                let after_space = col == 0 || screen.get(row, col - 1).is_none_or(|cell| cell.ch == ' ');
                let before = (alphabetic && !after_space).then_some(' ');
                for ch in before.into_iter().chain(word.chars()).chain(alphabetic.then_some(' ')) {
                    put(&mut screen, &mut row, &mut col, ch, ink, paper, bright, flash, inverse);
                }
            }
        }
    }
    screen.ensure_row(row.min(crate::screen::MOST_ROWS - 1));
    if over {
        screen.note("OVER is followed as normal printing: what's printed replaces what was there.");
    }
    if screen.rows.iter().flatten().any(|cell| cell.flash) {
        screen.note("Flashing text is shown steadily.");
    }
    screen.note("UDGs show the capitals they start out as, since the file doesn't hold their shapes.");
    screen
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Machine {
    C64,
    Vic20,
    Pet,
}

impl Machine {
    pub fn of(set: &Charset) -> Self {
        if set.id.ends_with("vic20") {
            Machine::Vic20
        } else if set.id.ends_with("-pet") {
            Machine::Pet
        } else {
            Machine::C64
        }
    }

    fn cols(self) -> usize {
        if self == Machine::Vic20 { 22 } else { 40 }
    }

    /// Paper, border and ink when the machine is switched on.
    fn start(self) -> (Color32, Color32, Color32) {
        match self {
            Machine::C64 => (C64[6], C64[14], C64[14]),
            Machine::Vic20 => (VIC20[1], VIC20[3], VIC20[6]),
            Machine::Pet => (Color32::BLACK, Color32::BLACK, PET_GREEN),
        }
    }

    fn aspect(self) -> f32 {
        match self {
            // The VIC-20's pixels are wide, so its 22 columns fill a whole screen.
            Machine::Vic20 => 1.5,
            _ => 1.0,
        }
    }

    fn colour_code(self, byte: u8) -> Option<Color32> {
        let index = match byte {
            0x90 => 0,
            0x05 => 1,
            0x1C => 2,
            0x9F => 3,
            0x9C => 4,
            0x1E => 5,
            0x1F => 6,
            0x9E => 7,
            0x81 => 8,
            0x95 => 9,
            0x96 => 10,
            0x97 => 11,
            0x98 => 12,
            0x99 => 13,
            0x9A => 14,
            0x9B => 15,
            _ => return None,
        };
        match self {
            Machine::C64 => Some(C64[index]),
            Machine::Vic20 => VIC20.get(index).copied(),
            Machine::Pet => None,
        }
    }
}

/// Plays a PETSCII stream onto a screen, as PRINTing it would. A clear screen code starts the picture again,
/// so a file of several screens shows the last one.
pub fn petscii(data: &[u8], set: &Charset, start_table: usize) -> Screen {
    let machine = Machine::of(set);
    let (paper, border, ink) = machine.start();
    let mut screen = Screen::new(machine.cols(), machine.aspect(), Cell::blank(ink, paper));
    screen.border = Some(border);
    let cols = screen.cols;
    let (mut row, mut col) = (0usize, 0usize);
    let mut fg = ink;
    let mut reverse = false;
    let mut table = start_table.min(1);
    let mut may_switch = true;
    let mut clears = 0;
    for &byte in data {
        if let Some(colour) = machine.colour_code(byte) {
            fg = colour;
            continue;
        }
        match byte {
            0x0D | 0x8D => {
                row += 1;
                col = 0;
                reverse = false;
            }
            0x93 => {
                screen.rows.clear();
                row = 0;
                col = 0;
                clears += 1;
            }
            0x13 => (row, col) = (0, 0),
            0x11 => row += 1,
            0x91 => row = row.saturating_sub(1),
            0x1D => {
                col += 1;
                if col >= cols {
                    col = 0;
                    row += 1;
                }
            }
            0x9D => {
                if col == 0 {
                    if row > 0 {
                        row -= 1;
                        col = cols - 1;
                    }
                } else {
                    col -= 1;
                }
            }
            0x12 => reverse = true,
            0x92 => reverse = false,
            0x0E if may_switch => table = 1,
            0x8E if may_switch => table = 0,
            0x08 => may_switch = false,
            0x09 => may_switch = true,
            0x14 => {
                // Delete: the cursor steps back and the rest of the line closes up.
                if col > 0 {
                    col -= 1;
                    if screen.ensure_row(row) {
                        let line = &mut screen.rows[row];
                        line.remove(col);
                        line.push(screen.paper);
                    }
                }
            }
            0x94 => {
                if screen.ensure_row(row) {
                    let line = &mut screen.rows[row];
                    line.insert(col, screen.paper);
                    line.truncate(cols);
                }
            }
            0x20..=0x7F | 0xA0..=0xFF => {
                let entry = set.entry(table, byte);
                let ch = entry.and_then(|e| e.unicode).unwrap_or(' ');
                let flipped = reverse != entry.is_some_and(|e| e.inverse);
                let (f, b) = if flipped { (paper, fg) } else { (fg, paper) };
                screen.put(row, col, Cell { ch, fg: f, bg: b, ..screen.paper });
                col += 1;
                if col >= cols {
                    col = 0;
                    row += 1;
                }
            }
            _ => {}
        }
        if row >= crate::screen::MOST_ROWS {
            break;
        }
    }
    screen.ensure_row(row.min(crate::screen::MOST_ROWS - 1));
    if clears > 1 {
        screen.note(format!("The file clears the screen {clears} times; this is the picture after the last clear."));
    }
    screen.note("Lines run on down the page instead of scrolling a 25-row screen.");
    screen
}

/// Screen memory, a byte a cell, in rows of the machine's width.
pub fn screen_codes(data: &[u8], set: &Charset, table: usize) -> Screen {
    let machine = Machine::of(set);
    let (paper, border, ink) = machine.start();
    let mut screen = Screen::new(machine.cols(), machine.aspect(), Cell::blank(ink, paper));
    screen.border = Some(border);
    for (index, &byte) in data.iter().enumerate() {
        let (row, col) = (index / screen.cols, index % screen.cols);
        if row >= crate::screen::MOST_ROWS {
            break;
        }
        let entry = set.entry(table.min(1), byte);
        let ch = entry.and_then(|e| e.unicode).unwrap_or(' ');
        let (fg, bg) = if entry.is_some_and(|e| e.inverse) { (paper, ink) } else { (ink, paper) };
        screen.put(row, col, Cell { ch, fg, bg, ..screen.paper });
    }
    screen.note("Colour memory isn't part of a screen code file, so every cell is in the start-up colour.");
    screen
}

/// The page itself from a teletext file: an EP1 page has a six-byte header and two bytes after its 25 rows.
fn teletext_body(data: &[u8]) -> (&[u8], bool) {
    if data.len() == 1008 && data.starts_with(&[0xFE, 0x01]) {
        return (&data[6..1006], true);
    }
    (data, false)
}

/// Draws teletext rows of 40 cells. With `parity`, bit 7 of each byte is a parity bit and not part of the code.
/// Mode 7 always ignores bit 7, as the SAA5050 does.
pub fn teletext(data: &[u8], set: &Charset, parity: bool, mode7: bool) -> Screen {
    let (body, ep1) = teletext_body(data);
    let mut screen = Screen::new(40, if mode7 { 0.8 } else { 0.6 }, Cell::blank(TELETEXT[7], TELETEXT[0]));
    let rows: Vec<&[u8]> = body.chunks(40).collect();
    let mut out_row = 0;
    let mut index = 0;
    let mut concealed = false;
    let mut flashing = false;
    while index < rows.len() && out_row < crate::screen::MOST_ROWS {
        let (cells, hides) = teletext_row(rows[index], set, parity || mode7);
        let doubled = cells.iter().any(|cell| cell.height == Height::Top);
        concealed |= hides;
        flashing |= cells.iter().any(|cell| cell.flash);
        for (col, cell) in cells.iter().enumerate() {
            screen.put(out_row, col, *cell);
        }
        out_row += 1;
        index += 1;
        // The row under a double-height row shows the bottom halves, and its own bytes aren't drawn.
        if doubled {
            for (col, cell) in cells.iter().enumerate() {
                let lower = if cell.height == Height::Top { Cell { height: Height::Bottom, ..*cell } } else { Cell { bg: cell.bg, ..screen.paper } };
                screen.put(out_row, col, lower);
            }
            out_row += 1;
            index += 1;
        }
    }
    if ep1 {
        screen.info.push(("Format".into(), "EP1 teletext page".into()));
    }
    if flashing {
        screen.note("Flashing text is shown steadily.");
    }
    if concealed {
        screen.note("Concealed text is hidden, as a set does until Reveal is pressed.");
    }
    screen
}

/// Whether text is a TTI teletext page file: lines of a two-letter command, a comma and its values.
pub fn looks_like_tti(data: &[u8]) -> bool {
    let starts = |line: &[u8]| ["DE,", "PN,", "DS,", "SP,", "OL,", "CT,", "PS,", "SC,", "RE,", "MS,", "FL,", "CY,"].iter().any(|c| line.starts_with(c.as_bytes()));
    starts(data) || data.windows(4).any(|w| w == b"\nOL,")
}

/// A TTI page file, as the vbit2 inserter reads it: `OL,row,text` lines, where a control code is written as ESC
/// and the code plus &40, bit 7 is ignored, and each row is filled out with spaces to 40 cells. A file of several
/// subpages shows the first, and says how many there are.
pub fn tti(data: &[u8], set: &Charset) -> Screen {
    let mut rows = vec![[0x20u8; 40]; 25];
    let mut used = 0;
    let (mut pages, mut description, mut number) = (0usize, None, None);
    for line in data.split(|b| *b == b'\n') {
        let line = line.strip_suffix(b"\r").unwrap_or(line);
        if let Some(rest) = line.strip_prefix(b"PN,") {
            pages += 1;
            if pages == 1 {
                number = Some(String::from_utf8_lossy(rest).trim().to_string());
            }
        } else if let Some(rest) = line.strip_prefix(b"DE,") {
            description.get_or_insert_with(|| String::from_utf8_lossy(rest).trim().to_string());
        } else if let Some(rest) = line.strip_prefix(b"OL,")
            && pages <= 1
        {
            let comma = rest.iter().position(|b| *b == b',').unwrap_or(rest.len());
            let Some(row) = std::str::from_utf8(&rest[..comma]).ok().and_then(|n| n.trim().parse::<usize>().ok()).filter(|n| *n < 25) else {
                continue;
            };
            let text = rest.get(comma + 1..).unwrap_or(&[]);
            let mut cells = [0x20u8; 40];
            let (mut i, mut j) = (0, 0);
            while i < text.len() && j < 40 {
                let mut ch = text[i] & 0x7F;
                if text[i] == 0x1B {
                    i += 1;
                    ch = text.get(i).map_or(0x20, |b| b & 0x3F);
                } else if text[i] < 0x20 {
                    break;
                }
                cells[j] = ch;
                j += 1;
                i += 1;
            }
            rows[row] = cells;
            used = used.max(row + 1);
        }
    }
    let bytes: Vec<u8> = rows[..used.max(1)].concat();
    let mut screen = teletext(&bytes, set, false, false);
    if let Some(number) = number {
        screen.info.push(("Page".into(), number));
    }
    if let Some(description) = description.filter(|d| !d.is_empty()) {
        screen.info.push(("Description".into(), description));
    }
    if pages > 1 {
        screen.note(format!("The file has {pages} subpages; the first is shown."));
    }
    screen
}

/// One row of teletext, following its spacing attributes from the left, and whether any of it is concealed.
fn teletext_row(bytes: &[u8], set: &Charset, strip: bool) -> (Vec<Cell>, bool) {
    let mut hides = false;
    let mut out = Vec::with_capacity(40);
    let (mut fg, mut bg) = (TELETEXT[7], TELETEXT[0]);
    let mut mosaic = false;
    let mut separated = false;
    let mut hold = false;
    let mut held: Option<Mosaic> = None;
    let mut flash = false;
    let mut conceal = false;
    let mut double = false;
    for &byte in bytes.iter().take(40) {
        let code = if strip { byte & 0x7F } else { byte };
        // Set-at controls take effect in their own cell.
        match code {
            0x09 => flash = false,
            0x0C => {
                double = false;
                held = None;
            }
            0x18 => conceal = true,
            0x19 => separated = false,
            0x1A => separated = true,
            0x1C => bg = TELETEXT[0],
            0x1D => bg = fg,
            0x1E => hold = true,
            _ => {}
        }
        let mut cell = Cell { ch: ' ', fg, bg, mosaic: None, height: if double { Height::Top } else { Height::Normal }, underline: false, flash };
        if code < 0x20 {
            if hold && mosaic {
                cell.mosaic = held;
            }
        } else if code >= 0x80 {
            // Without parity stripping, the top half isn't teletext at all.
            cell.ch = '\u{FFFD}';
        } else if mosaic && code & 0x20 != 0 {
            let block = Mosaic { bits: code & 0x5F, separated };
            cell.mosaic = Some(block);
            held = Some(block);
        } else {
            cell.ch = set.entry(0, code).and_then(|e| e.unicode).unwrap_or(' ');
        }
        if conceal && (cell.ch != ' ' || cell.mosaic.is_some()) {
            cell.fg = cell.bg;
            hides = true;
        }
        out.push(cell);
        // Set-after controls take effect from the next cell.
        match code {
            0x01..=0x07 => {
                fg = TELETEXT[usize::from(code)];
                mosaic = false;
                conceal = false;
                held = None;
            }
            0x08 => flash = true,
            0x0D => {
                double = true;
                held = None;
            }
            0x11..=0x17 => {
                fg = TELETEXT[usize::from(code - 0x10)];
                mosaic = true;
                conceal = false;
            }
            0x1F => hold = false,
            _ => {}
        }
    }
    while out.len() < 40 {
        out.push(Cell::blank(TELETEXT[7], TELETEXT[0]));
    }
    (out, hides)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::charset::known;

    #[test]
    fn petscii_colour_codes_reverse_and_newlines() {
        let set = known("petscii-c64");
        let screen = petscii(b"\x1cA\x12B\x0dC", set, 0);
        assert_eq!(screen.cols, 40);
        let a = screen.get(0, 0).unwrap();
        assert_eq!((a.ch, a.fg, a.bg), ('A', C64[2], C64[6]));
        let b = screen.get(0, 1).unwrap();
        assert_eq!((b.fg, b.bg), (C64[6], C64[2]), "reverse swaps ink and paper");
        let c = screen.get(1, 0).unwrap();
        assert_eq!((c.ch, c.bg), ('C', C64[6]), "a return ends reverse");
        assert_eq!(screen.border, Some(C64[14]));
    }

    #[test]
    fn petscii_switches_case_moves_the_cursor_and_clears() {
        let set = known("petscii-c64");
        let screen = petscii(b"\x0eA\x8eA", set, 0);
        assert_eq!(screen.row_text(0), "aA");
        let moved = petscii(b"X\x11\x1d\x1dY\x13Z", set, 0);
        assert_eq!(moved.row_text(0), "Z");
        assert_eq!(moved.row_text(1), "   Y");
        let cleared = petscii(b"OLD\x93NEW\x93LAST", set, 0);
        assert_eq!(cleared.row_text(0), "LAST");
        assert!(cleared.notes.iter().any(|n| n.contains("2 times")));
        let locked = petscii(b"\x08\x0eA", set, 0);
        assert_eq!(locked.row_text(0), "A", "case switching is locked out");
    }

    #[test]
    fn the_vic20_and_pet_have_their_own_screens() {
        let vic = petscii(&[b'A'; 23], known("petscii-vic20"), 0);
        assert_eq!(vic.cols, 22);
        assert_eq!(vic.rows.len(), 2, "the 23rd character wraps");
        let pet = petscii(b"\x1cA", known("petscii-pet"), 0);
        assert_eq!(pet.get(0, 0).unwrap().fg, PET_GREEN, "the PET has no colour");
    }

    #[test]
    fn screen_codes_draw_their_reversed_half_reversed() {
        let set = known("screen-c64");
        let screen = screen_codes(&[0x01, 0x81], set, 0);
        let (a, b) = (screen.get(0, 0).unwrap(), screen.get(0, 1).unwrap());
        assert_eq!((a.ch, b.ch), ('A', 'A'));
        assert_eq!((a.fg, b.bg), (C64[14], C64[14]));
    }

    fn row(text: &[u8]) -> Vec<u8> {
        let mut bytes = text.to_vec();
        bytes.resize(40, b' ');
        bytes
    }

    #[test]
    fn teletext_colours_are_set_after_and_backgrounds_set_at() {
        let set = known("teletext-en");
        let screen = teletext(&row(b"\x01A\x1dB\x1cC"), set, false, false);
        let cells = &screen.rows[0];
        assert_eq!(cells[0].ch, ' ');
        assert_eq!((cells[1].ch, cells[1].fg), ('A', TELETEXT[1]));
        assert_eq!(cells[2].bg, TELETEXT[1], "new background works in its own cell");
        assert_eq!((cells[3].ch, cells[3].bg), ('B', TELETEXT[1]));
        assert_eq!(cells[5].bg, TELETEXT[0]);
    }

    #[test]
    fn mosaics_capitals_hold_and_separation() {
        let set = known("teletext-en");
        let screen = teletext(&row(b"\x17\x7fA\x1a\x7f\x1e\x13\x1f\x12"), set, false, false);
        let cells = &screen.rows[0];
        assert_eq!(cells[1].mosaic, Some(Mosaic { bits: 0x5F, separated: false }));
        assert_eq!((cells[2].ch, cells[2].mosaic), ('A', None), "capitals blast through");
        assert_eq!(cells[4].mosaic, Some(Mosaic { bits: 0x5F, separated: true }));
        assert_eq!(cells[6].mosaic, Some(Mosaic { bits: 0x5F, separated: true }), "a held mosaic fills the control cell");
        assert_eq!(cells[8].mosaic, None, "released");
    }

    #[test]
    fn double_height_takes_the_row_below() {
        let set = known("teletext-en");
        let mut data = row(b"\x0dBIG");
        data.extend(row(b"hidden"));
        data.extend(row(b"next"));
        let screen = teletext(&data, set, false, false);
        assert_eq!(screen.rows.len(), 3);
        assert_eq!(screen.rows[0][1].height, Height::Top);
        assert_eq!((screen.rows[1][1].ch, screen.rows[1][1].height), ('B', Height::Bottom));
        assert_eq!(screen.row_text(2), "next");
    }

    #[test]
    fn conceal_parity_national_sets_and_ep1() {
        let set = known("teletext-en");
        let concealed = teletext(&row(b"\x18SECRET"), set, false, false);
        assert_eq!(concealed.rows[0][1].fg, concealed.rows[0][1].bg);
        assert!(!concealed.notes.is_empty());
        let parity: Vec<u8> = row(b"\x01A").iter().map(|b| b | 0x80).collect();
        assert_eq!(teletext(&parity, set, true, false).rows[0][1].ch, 'A');
        assert_eq!(teletext(&row(b"#"), set, false, false).rows[0][0].ch, '\u{A3}', "the English set has the pound sign");
        let mut ep1 = vec![0xFE, 0x01, 0x09, 0, 0, 0];
        ep1.extend(std::iter::repeat_n(b'x', 1000));
        ep1.extend([0, 0]);
        let page = teletext(&ep1, set, false, false);
        assert_eq!(page.rows.len(), 25);
        assert_eq!(page.row_text(0), "x".repeat(40));
    }

    #[test]
    fn spectrum_colour_controls_position_and_keywords() {
        let set = known("zx-spectrum");
        let screen = spectrum(b"\x10\x02\x11\x06A\x13\x01B\x14\x01C\x0d\x16\x05\x0aX\x06Y\xf5\x90", set);
        let a = screen.get(0, 0).unwrap();
        assert_eq!((a.ch, a.fg, a.bg), ('A', SPECTRUM[2], SPECTRUM[6]), "red ink on yellow paper");
        assert_eq!(screen.get(0, 1).unwrap().fg, SPECTRUM_BRIGHT[2], "BRIGHT");
        let c = screen.get(0, 2).unwrap();
        assert_eq!((c.fg, c.bg), (SPECTRUM_BRIGHT[6], SPECTRUM_BRIGHT[2]), "INVERSE swaps them");
        assert_eq!(screen.get(5, 10).unwrap().ch, 'X', "AT 5,10");
        assert_eq!(screen.get(5, 16).unwrap().ch, 'Y', "the comma control goes to the next half of the line");
        assert_eq!(screen.row_text(5).trim(), "X     Y PRINT A");
        assert_eq!(screen.cols, 32);
        assert_eq!(screen.get(0, 3).map(|c| c.ch), Some(' '));
        let _ = spectrum(&[0x10, 0x16, 0x17], set);
    }

    #[test]
    fn tti_pages_are_read_as_vbit2_reads_them() {
        let data = b"DE,News\r\nPN,10000\r\nSC,0000\r\nOL,1,\x1bAHEADLINE\r\nOL,3,\x81RED\r\nPN,10001\r\nOL,1,second subpage\r\n";
        assert!(looks_like_tti(data));
        assert!(!looks_like_tti(b"just some text"));
        let screen = tti(data, known("teletext-en"));
        assert_eq!(screen.rows.len(), 4);
        assert_eq!((screen.rows[1][1].ch, screen.rows[1][1].fg), ('H', TELETEXT[1]), "ESC A is alpha red");
        assert_eq!((screen.rows[3][1].ch, screen.rows[3][1].fg), ('R', TELETEXT[1]), "bit 7 is ignored");
        assert_eq!(screen.row_text(0), "");
        assert!(screen.info.iter().any(|(k, v)| k == "Page" && v == "10000"));
        assert!(screen.info.iter().any(|(k, v)| k == "Description" && v == "News"));
        assert!(screen.notes.iter().any(|n| n.contains("2 subpages")));
    }

    #[test]
    fn mode7_ignores_bit_7() {
        let set = known("bbc-mode7");
        let screen = teletext(&row(&[0x81, 0xC1]), set, false, true);
        assert_eq!((screen.rows[0][1].ch, screen.rows[0][1].fg), ('A', TELETEXT[1]));
    }
}
