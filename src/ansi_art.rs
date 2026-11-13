//! PC ANSI art, as the BBS scene drew it: CP437 text with ANSI.SYS escape sequences for colour and cursor
//! movement, and the SAUCE record that art packs add at the end of a file to say who made it and how wide it is.
//! Also BinaryText (`.BIN`), the plain character-and-attribute pairs of the VGA text screen.
//!
//! Sources: the SAUCE 00.5 specification (acid.org/info/sauce/sauce.htm), and ECMA-48 with ANSI.SYS's subset of it.
//! Colours are the standard VGA text palette. This only reads the file: nothing here changes a byte of it.

use crate::charset::{self, Charset};
use crate::screen::{Cell, Screen, rgb};
use eframe::egui::Color32;

/// The VGA text palette, in ANSI order: black, red, green, brown, blue, magenta, cyan, grey, then the bright ones.
pub const VGA: [Color32; 16] = [
    rgb(0x000000),
    rgb(0xAA0000),
    rgb(0x00AA00),
    rgb(0xAA5500),
    rgb(0x0000AA),
    rgb(0xAA00AA),
    rgb(0x00AAAA),
    rgb(0xAAAAAA),
    rgb(0x555555),
    rgb(0xFF5555),
    rgb(0x55FF55),
    rgb(0xFFFF55),
    rgb(0x5555FF),
    rgb(0xFF55FF),
    rgb(0x55FFFF),
    rgb(0xFFFFFF),
];

/// The VGA attribute byte's colour order (blue first) in ANSI's order (red first).
const VGA_TO_ANSI: [usize; 8] = [0, 4, 2, 6, 1, 5, 3, 7];

/// CP437's pictures for the control range, which ANSI art uses as characters.
const CP437_LOW: [char; 32] = [
    ' ', '\u{263A}', '\u{263B}', '\u{2665}', '\u{2666}', '\u{2663}', '\u{2660}', '\u{2022}', '\u{25D8}', '\u{25CB}', '\u{25D9}', '\u{2642}', '\u{2640}', '\u{266A}', '\u{266B}', '\u{263C}', '\u{25BA}', '\u{25C4}', '\u{2195}', '\u{203C}',
    '\u{B6}', '\u{A7}', '\u{25AC}', '\u{21A8}', '\u{2191}', '\u{2193}', '\u{2192}', '\u{2190}', '\u{221F}', '\u{2194}', '\u{25B2}', '\u{25BC}',
];

/// What a byte shows as on a PC text screen in this character set.
pub fn glyph(set: &Charset, byte: u8) -> char {
    if byte < 0x20 && set.id.starts_with("cp") {
        return CP437_LOW[usize::from(byte)];
    }
    if byte == 0x7F && set.id.starts_with("cp") {
        return '\u{2302}';
    }
    set.entry(0, byte).and_then(|entry| entry.unicode).filter(|c| !c.is_control()).unwrap_or(' ')
}

/// The 128-byte record at the end of a file, with its comment lines.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Sauce {
    pub title: String,
    pub author: String,
    pub group: String,
    pub date: String,
    pub data_type: u8,
    pub file_type: u8,
    pub width: u16,
    pub height: u16,
    /// Bit 0 set: the blink bit means bright background instead (iCE colours).
    pub flags: u8,
    pub font: String,
    pub comments: Vec<String>,
    /// Where the art itself stops: before the end-of-file mark, the comments and the record.
    pub content_end: usize,
}

impl Sauce {
    pub fn ice_colours(&self) -> bool {
        self.flags & 1 != 0
    }

    pub fn kind_name(&self) -> &'static str {
        match (self.data_type, self.file_type) {
            (1, 0) => "ASCII",
            (1, 1) => "ANSi",
            (1, 2) => "ANSiMation",
            (1, 3) => "RIPScript",
            (1, 4) => "PCBoard",
            (1, 5) => "Avatar",
            (1, 6) => "HTML",
            (1, 7) => "Source",
            (1, 8) => "TundraDraw",
            (5, _) => "BinaryText",
            (6, _) => "XBin",
            (2, _) => "Bitmap",
            (3, _) => "Vector",
            (4, _) => "Audio",
            (7, _) => "Archive",
            (8, _) => "Executable",
            _ => "Unknown",
        }
    }
}

fn field(bytes: &[u8]) -> String {
    let text: String = bytes.iter().map(|b| sauce_char(*b)).collect();
    text.trim_end_matches(['\0', ' ']).to_string()
}

/// SAUCE text is CP437; anything outside printable ASCII is shown as its CP437 character.
fn sauce_char(byte: u8) -> char {
    match byte {
        0 => '\0',
        0x20..=0x7E => byte as char,
        _ => glyph(charset::known("cp437"), byte),
    }
}

/// Finds a SAUCE record, if the file has one. A damaged one is ignored rather than guessed at.
pub fn sauce(data: &[u8]) -> Option<Sauce> {
    if data.len() < 128 {
        return None;
    }
    let start = data.len() - 128;
    let record = &data[start..];
    if &record[..5] != b"SAUCE" {
        return None;
    }
    let word = |at: usize| u16::from_le_bytes([record[at], record[at + 1]]);
    let comment_count = usize::from(record[104]);
    let mut comments = Vec::new();
    let mut content_end = start;
    let block = 5 + comment_count * 64;
    if comment_count > 0 && start >= block && &data[start - block..start - block + 5] == b"COMNT" {
        for line in data[start - block + 5..start].chunks(64) {
            comments.push(field(line));
        }
        content_end = start - block;
    }
    if content_end > 0 && data[content_end - 1] == 0x1A {
        content_end -= 1;
    }
    let date = field(&record[82..90]);
    let date = if date.len() == 8 && date.chars().all(|c| c.is_ascii_digit()) { format!("{}-{}-{}", &date[..4], &date[4..6], &date[6..]) } else { date };
    Some(Sauce {
        title: field(&record[7..42]),
        author: field(&record[42..62]),
        group: field(&record[62..82]),
        date,
        data_type: record[94],
        file_type: record[95],
        width: word(96),
        height: word(98),
        flags: record[105],
        font: field(&record[106..128]),
        comments,
        content_end,
    })
}

/// The 256 colours of xterm: the sixteen above, a 6x6x6 cube, then 24 greys.
fn xterm(index: u16) -> Color32 {
    match index {
        0..=15 => VGA[usize::from(index)],
        16..=231 => {
            let n = index - 16;
            let level = |v: u16| if v == 0 { 0 } else { (55 + v * 40) as u8 };
            Color32::from_rgb(level(n / 36), level(n / 6 % 6), level(n % 6))
        }
        _ => {
            let v = (8 + (index.min(255) - 232) * 10) as u8;
            Color32::from_rgb(v, v, v)
        }
    }
}

/// The Amiga's colours for ANSI text: sixteen pens, each as the Amiga keeps a colour, four bits each of red,
/// green and blue (0xF80 is orange). The console's colour codes choose pens, so ESC[30m is pen 0, which on a
/// Workbench 1.3 screen is its blue; text is pen 1 on pen 0.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AmigaPalette {
    pub pens: [u16; 16],
    /// Bold text in pens 8 to 15, as terminal programs on 16-colour screens drew it, rather than the same pen.
    pub bold_high: bool,
}

/// graphics.library's own colours for pens 4 to 15, which Workbench leaves as they are.
const KICKSTART_UPPER: [u16; 12] = [0x00F, 0xF0F, 0x0FF, 0xFFF, 0x620, 0xE50, 0x9F1, 0xEB0, 0x55F, 0x92F, 0x0F8, 0xCCC];

const fn with_workbench(first: [u16; 4]) -> [u16; 16] {
    let mut pens = [0u16; 16];
    let mut i = 0;
    while i < 16 {
        pens[i] = if i < 4 { first[i] } else { KICKSTART_UPPER[i - 4] };
        i += 1;
    }
    pens
}

/// Palettes to start from: Workbench's own, and ANSI's usual colours.
pub const AMIGA_PRESETS: [(&str, [u16; 16]); 3] = [
    ("Workbench 1.0\u{2013}1.3", with_workbench([0x05A, 0xFFF, 0x002, 0xF80])),
    ("Workbench 2.04\u{2013}3.x", with_workbench([0xAAA, 0x000, 0xFFF, 0x68B])),
    ("Standard ANSI", [0x000, 0xA00, 0x0A0, 0xA50, 0x00A, 0xA0A, 0x0AA, 0xAAA, 0x555, 0xF55, 0x5F5, 0xFF5, 0x55F, 0xF5F, 0x5FF, 0xFFF]),
];

impl Default for AmigaPalette {
    fn default() -> Self {
        AmigaPalette { pens: AMIGA_PRESETS[0].1, bold_high: true }
    }
}

impl AmigaPalette {
    /// For the settings file: the pens as three hex digits each, then whether bold is pens 8 to 15.
    pub fn to_setting(self) -> String {
        let pens: Vec<String> = self.pens.iter().map(|pen| format!("{pen:03X}")).collect();
        format!("amiga_palette={}\namiga_bold_pens={}\n", pens.join(","), self.bold_high)
    }

    pub fn from_settings(settings: &str) -> Self {
        let mut palette = AmigaPalette::default();
        for line in settings.lines() {
            match line.split_once('=') {
                Some(("amiga_palette", value)) => {
                    let pens: Vec<u16> = value.split(',').filter_map(|pen| u16::from_str_radix(pen.trim(), 16).ok()).filter(|pen| *pen <= 0xFFF).collect();
                    if let Ok(pens) = <[u16; 16]>::try_from(pens) {
                        palette.pens = pens;
                    }
                }
                Some(("amiga_bold_pens", value)) => palette.bold_high = value.trim() != "false",
                _ => {}
            }
        }
        palette
    }
}

/// An Amiga colour (0xRGB, four bits each) as the screen shows it.
pub fn amiga_colour(pen: u16) -> Color32 {
    let level = |shift: u16| ((pen >> shift) & 0xF) as u8 * 17;
    Color32::from_rgb(level(8), level(4), level(0))
}

static AMIGA: std::sync::Mutex<Option<AmigaPalette>> = std::sync::Mutex::new(None);

/// The palette the Amiga previews are drawn in from now on.
pub fn set_amiga_palette(palette: AmigaPalette) {
    *AMIGA.lock().unwrap_or_else(std::sync::PoisonError::into_inner) = Some(palette);
}

fn amiga_palette() -> AmigaPalette {
    AMIGA.lock().unwrap_or_else(std::sync::PoisonError::into_inner).unwrap_or_default()
}

#[derive(Clone, Copy)]
enum Ink {
    Index(usize),
    True(Color32),
}

struct Pen {
    fg: Ink,
    bg: Ink,
    /// The Amiga's pens, for Amiga console text.
    amiga: Option<AmigaPalette>,
    bold: bool,
    blink: bool,
    reverse: bool,
    underline: bool,
    conceal: bool,
}

impl Pen {
    fn new() -> Self {
        Self { fg: Ink::Index(7), bg: Ink::Index(0), amiga: None, bold: false, blink: false, reverse: false, underline: false, conceal: false }
    }

    /// The pen text starts with on an Amiga: pen 1 on pen 0.
    fn amiga(palette: AmigaPalette) -> Self {
        Self { fg: Ink::Index(1), bg: Ink::Index(0), amiga: Some(palette), ..Self::new() }
    }

    /// As it is after ESC[0m.
    fn reset(&self) -> Self {
        self.amiga.map_or_else(Self::new, Self::amiga)
    }

    /// The colours 39 and 49 go back to.
    fn default_inks(&self) -> (Ink, Ink) {
        if self.amiga.is_some() { (Ink::Index(1), Ink::Index(0)) } else { (Ink::Index(7), Ink::Index(0)) }
    }

    /// A VGA attribute byte: foreground in the low four bits (blue first), background in the next three, blink on top.
    fn from_attribute(attribute: u8) -> Self {
        let colour = |bits: u8| VGA_TO_ANSI[usize::from(bits & 7)] + if bits & 8 != 0 { 8 } else { 0 };
        Self { fg: Ink::Index(colour(attribute & 0x0F)), bg: Ink::Index(colour((attribute >> 4) & 7)), blink: attribute & 0x80 != 0, ..Self::new() }
    }

    fn colours(&self, ice: bool) -> (Color32, Color32) {
        if let Some(palette) = &self.amiga {
            let pen = |ink: Ink, bright: bool| match ink {
                Ink::Index(i) if i < 8 && bright => amiga_colour(palette.pens[i + 8]),
                Ink::Index(i) if i < 16 => amiga_colour(palette.pens[i]),
                Ink::Index(i) => xterm(i as u16),
                Ink::True(c) => c,
            };
            let (fg, bg) = (pen(self.fg, self.bold && palette.bold_high), pen(self.bg, false));
            let (fg, bg) = if self.reverse { (bg, fg) } else { (fg, bg) };
            return (if self.conceal { bg } else { fg }, bg);
        }
        let fg = match self.fg {
            Ink::Index(i) if i < 8 && self.bold => VGA[i + 8],
            Ink::Index(i) => xterm(i as u16),
            Ink::True(c) => c,
        };
        let bg = match self.bg {
            Ink::Index(i) if i < 8 && self.blink && ice => VGA[i + 8],
            Ink::Index(i) => xterm(i as u16),
            Ink::True(c) => c,
        };
        let (fg, bg) = if self.reverse { (bg, fg) } else { (fg, bg) };
        (if self.conceal { bg } else { fg }, bg)
    }
}

struct Terminal {
    screen: Screen,
    row: usize,
    col: usize,
    saved: (usize, usize),
    pen: Pen,
    ice: bool,
    unknown: usize,
}

impl Terminal {
    fn print(&mut self, ch: char) {
        if self.col >= self.screen.cols {
            self.col = 0;
            self.row += 1;
        }
        let (fg, bg) = self.pen.colours(self.ice);
        let cell = Cell { ch, fg, bg, underline: self.pen.underline, flash: self.pen.blink && !self.ice, ..self.screen.paper };
        self.screen.put(self.row, self.col, cell);
        self.col += 1;
    }

    fn erase(&mut self, row: usize, from: usize, to: usize) {
        let (_, bg) = self.pen.colours(self.ice);
        if self.screen.ensure_row(row) {
            for col in from..to.min(self.screen.cols) {
                self.screen.rows[row][col] = Cell { bg, ..self.screen.paper };
            }
        }
    }

    fn sgr(&mut self, params: &[u16]) {
        let mut i = 0;
        if params.is_empty() {
            self.pen = self.pen.reset();
        }
        while i < params.len() {
            match params[i] {
                0 => self.pen = self.pen.reset(),
                1 => self.pen.bold = true,
                2 | 22 => self.pen.bold = false,
                4 => self.pen.underline = true,
                24 => self.pen.underline = false,
                5 | 6 => self.pen.blink = true,
                25 => self.pen.blink = false,
                7 => self.pen.reverse = true,
                27 => self.pen.reverse = false,
                8 => self.pen.conceal = true,
                28 => self.pen.conceal = false,
                n @ 30..=37 => self.pen.fg = Ink::Index(usize::from(n - 30)),
                39 => self.pen.fg = self.pen.default_inks().0,
                n @ 40..=47 => self.pen.bg = Ink::Index(usize::from(n - 40)),
                49 => self.pen.bg = self.pen.default_inks().1,
                n @ 90..=97 => self.pen.fg = Ink::Index(usize::from(n - 90 + 8)),
                n @ 100..=107 => self.pen.bg = Ink::Index(usize::from(n - 100 + 8)),
                which @ (38 | 48) => {
                    let ink = match params.get(i + 1) {
                        Some(5) => {
                            i += 2;
                            params.get(i).map(|n| Ink::Index(usize::from((*n).min(255))))
                        }
                        Some(2) => {
                            i += 4;
                            let part = |k: usize| params.get(k).map_or(0, |v| (*v).min(255) as u8);
                            Some(Ink::True(Color32::from_rgb(part(i - 2), part(i - 1), part(i))))
                        }
                        _ => None,
                    };
                    if let Some(ink) = ink {
                        if which == 38 {
                            self.pen.fg = ink;
                        } else {
                            self.pen.bg = ink;
                        }
                    }
                }
                _ => self.unknown += 1,
            }
            i += 1;
        }
    }

    fn control(&mut self, params: &[u16], private: bool, last: u8) {
        let n = |i: usize| params.get(i).copied().filter(|v| *v > 0).unwrap_or(1) as usize;
        match last {
            b'm' => self.sgr(params),
            b'A' => self.row = self.row.saturating_sub(n(0)),
            b'B' => self.row += n(0),
            b'C' => self.col = (self.col + n(0)).min(self.screen.cols - 1),
            b'D' => self.col = self.col.min(self.screen.cols).saturating_sub(n(0)),
            b'E' => {
                self.row += n(0);
                self.col = 0;
            }
            b'F' => {
                self.row = self.row.saturating_sub(n(0));
                self.col = 0;
            }
            b'G' => self.col = (n(0) - 1).min(self.screen.cols - 1),
            b'H' | b'f' => {
                self.row = n(0) - 1;
                self.col = (n(1) - 1).min(self.screen.cols - 1);
            }
            b's' => self.saved = (self.row, self.col),
            b'u' => (self.row, self.col) = self.saved,
            b'J' => match params.first().copied().unwrap_or(0) {
                2 => {
                    self.screen.rows.clear();
                    self.row = 0;
                    self.col = 0;
                }
                0 => {
                    let (row, col) = (self.row, self.col);
                    self.erase(row, col, self.screen.cols);
                    self.screen.rows.truncate(row + 1);
                }
                _ => self.unknown += 1,
            },
            b'K' => {
                let (row, col, cols) = (self.row, self.col, self.screen.cols);
                match params.first().copied().unwrap_or(0) {
                    0 => self.erase(row, col, cols),
                    1 => self.erase(row, 0, col + 1),
                    _ => self.erase(row, 0, cols),
                }
            }
            // PabloDraw's 24-bit colour: ESC [ 0 ; r ; g ; b t for the background, 1 for the foreground.
            b't' if params.len() == 4 => {
                let colour = Color32::from_rgb(params[1].min(255) as u8, params[2].min(255) as u8, params[3].min(255) as u8);
                if params[0] == 1 {
                    self.pen.fg = Ink::True(colour);
                } else {
                    self.pen.bg = Ink::True(colour);
                }
            }
            // Modes such as ESC [ ? 7 h (line wrap) and ESC [ ? 25 l (hide the cursor) change nothing that's drawn.
            b'h' | b'l' if private => {}
            _ => self.unknown += 1,
        }
    }
}

/// The BBS colour codes a file uses as well as, or instead of, ANSI. From Synchronet's "Color Codes" reference
/// (wiki.synchro.net/custom:colors) and, for Avatar, George Stanislav's AVATAR.DOC (AVT/0, FidoNet FSC-0025).
#[derive(Clone, Copy, Default, PartialEq, Eq, Debug)]
pub struct Codes {
    /// PCBoard's @X followed by the background and foreground digits, with @X00 and @XFF to save and restore.
    pub pcboard: bool,
    /// Wildcat!'s @ background foreground @.
    pub wildcat: bool,
    /// WWIV's heart (Ctrl-C) followed by a digit.
    pub wwiv: bool,
    /// Renegade's and Telegard's pipe followed by two digits, 00 to 31.
    pub pipes: bool,
    /// Avatar's Ctrl-V commands, Ctrl-L and Ctrl-Y.
    pub avatar: bool,
}

impl Codes {
    /// Which codes a file uses: what SAUCE says, or what turns up at least twice in it.
    pub fn of(body: &[u8], record: Option<&Sauce>) -> Self {
        let hex = |b: &u8| b.is_ascii_hexdigit();
        let count = |found: &dyn Fn(&[u8]) -> bool| (0..body.len()).filter(|&i| found(&body[i..])).take(2).count() >= 2;
        let from_sauce = record.map(|s| (s.data_type, s.file_type));
        Self {
            pcboard: from_sauce == Some((1, 4)) || count(&|b| b.len() >= 4 && b[0] == b'@' && b[1] == b'X' && hex(&b[2]) && hex(&b[3])),
            wildcat: count(&|b| b.len() >= 4 && b[0] == b'@' && hex(&b[1]) && hex(&b[2]) && b[3] == b'@'),
            wwiv: count(&|b| b.len() >= 2 && b[0] == 0x03 && b[1].is_ascii_digit()),
            pipes: count(&|b| b.len() >= 3 && b[0] == b'|' && b[1].is_ascii_digit() && b[2].is_ascii_digit() && (b[1] - b'0') * 10 + (b[2] - b'0') <= 31),
            avatar: from_sauce == Some((1, 5)) || count(&|b| b.len() >= 2 && b[0] == 0x16 && (1..=8).contains(&b[1])),
        }
    }

    fn any(self) -> bool {
        self.pcboard || self.wildcat || self.wwiv || self.pipes || self.avatar
    }
}

/// WWIV's ten colours, as attributes, from Synchronet's table: normal, bright cyan, bright yellow, magenta,
/// bright white on blue, green, bright blinking red, bright blue, blue and cyan.
const WWIV: [u8; 10] = [0x07, 0x0B, 0x0E, 0x05, 0x1F, 0x02, 0x8C, 0x09, 0x01, 0x03];

impl Terminal {
    fn set_attribute(&mut self, attribute: u8) {
        self.pen = Pen::from_attribute(attribute);
    }

    /// Reads a BBS colour code at `i` if there is one, and says how many bytes it took.
    fn bbs_code(&mut self, body: &[u8], i: usize, codes: Codes, saved: &mut Option<u8>) -> Option<usize> {
        let at = &body[i..];
        let hex = |b: u8| char::from(b).to_digit(16).map(|d| d as u8);
        match at.first()? {
            b'@' if codes.pcboard && at.get(1) == Some(&b'X') => {
                let attribute = hex(*at.get(2)?)? << 4 | hex(*at.get(3)?)?;
                match attribute {
                    0x00 => *saved = Some(self.pen_attribute()),
                    0xFF => self.set_attribute(saved.unwrap_or(0x07)),
                    _ => self.set_attribute(attribute),
                }
                Some(4)
            }
            b'@' if codes.pcboard && at.starts_with(b"@CLS@") => {
                self.screen.rows.clear();
                (self.row, self.col) = (0, 0);
                Some(5)
            }
            b'@' if codes.wildcat && at.get(3) == Some(&b'@') => {
                self.set_attribute(hex(*at.get(1)?)? << 4 | hex(*at.get(2)?)?);
                Some(4)
            }
            0x03 if codes.wwiv => {
                let digit = at.get(1).filter(|d| d.is_ascii_digit())?;
                self.set_attribute(WWIV[usize::from(digit - b'0')]);
                Some(2)
            }
            b'|' if codes.pipes => {
                let (a, b) = (at.get(1).filter(|d| d.is_ascii_digit())?, at.get(2).filter(|d| d.is_ascii_digit())?);
                let n = (a - b'0') * 10 + (b - b'0');
                let attribute = self.pen_attribute();
                match n {
                    0..=15 => self.set_attribute((attribute & 0xF0) | n),
                    16..=23 => self.set_attribute((attribute & 0x8F) | (n - 16) << 4),
                    24..=31 => self.set_attribute((attribute & 0x0F) | 0x80 | (n - 24) << 4),
                    _ => return None,
                }
                Some(3)
            }
            0x0C if codes.avatar => {
                self.screen.rows.clear();
                (self.row, self.col) = (0, 0);
                self.set_attribute(0x03);
                Some(1)
            }
            0x19 if codes.avatar => {
                let (ch, times) = (*at.get(1)?, *at.get(2)?);
                for _ in 0..times {
                    self.print(glyph(charset::known("cp437"), ch));
                }
                Some(3)
            }
            0x16 if codes.avatar => {
                let used = match at.get(1)? {
                    0x01 => {
                        self.set_attribute(at.get(2)? & 0x7F);
                        3
                    }
                    0x02 => {
                        self.pen.blink = true;
                        2
                    }
                    0x03 => {
                        self.row = self.row.saturating_sub(1);
                        2
                    }
                    0x04 => {
                        self.row += 1;
                        2
                    }
                    0x05 => {
                        self.col = self.col.saturating_sub(1);
                        2
                    }
                    0x06 => {
                        self.col = (self.col + 1).min(self.screen.cols - 1);
                        2
                    }
                    0x07 => {
                        let (row, col, cols) = (self.row, self.col, self.screen.cols);
                        self.erase(row, col, cols);
                        2
                    }
                    0x08 => {
                        self.row = usize::from(*at.get(2)?).saturating_sub(1);
                        self.col = usize::from(*at.get(3)?).saturating_sub(1).min(self.screen.cols - 1);
                        4
                    }
                    _ => {
                        self.unknown += 1;
                        2
                    }
                };
                Some(used)
            }
            _ => None,
        }
    }

    /// The pen as a VGA attribute, for codes that change only part of it.
    fn pen_attribute(&self) -> u8 {
        let index = |ink: Ink, fallback: usize| match ink {
            Ink::Index(i) if i < 16 => i,
            _ => fallback,
        };
        let to_vga = |ansi: usize| (VGA_TO_ANSI.iter().position(|a| *a == ansi % 8).unwrap_or(0) + if ansi >= 8 { 8 } else { 0 }) as u8;
        let fg = to_vga(index(self.pen.fg, 7) + if self.pen.bold && index(self.pen.fg, 7) < 8 { 8 } else { 0 });
        let bg = to_vga(index(self.pen.bg, 0)) & 7;
        fg | bg << 4 | if self.pen.blink { 0x80 } else { 0 }
    }
}

/// Draws ANSI art. `set` gives the glyphs, normally CP437.
pub fn render(data: &[u8], set: &Charset) -> Screen {
    if data.starts_with(b"XBIN\x1A") {
        return xbin(data, set);
    }
    render_with(data, set, None)
}

/// Draws Amiga console text: ANSI with the single-byte CSI (0x9B) too, in the Amiga's ISO-8859-1.
pub fn render_amiga(data: &[u8]) -> Screen {
    render_with(data, charset::known("amiga-text"), Some(amiga_palette()))
}

fn render_with(data: &[u8], set: &Charset, amiga: Option<AmigaPalette>) -> Screen {
    let record = sauce(data);
    let end = record.as_ref().map_or_else(|| data.iter().position(|b| *b == 0x1A).unwrap_or(data.len()), |s| s.content_end.min(data.len()));
    let body = &data[..end];
    if record.as_ref().is_some_and(|s| s.data_type == 5) {
        return binary_text(body, record, set);
    }
    let width = record.as_ref().map(|s| usize::from(s.width)).filter(|w| (1..=1000).contains(w) && record.as_ref().is_some_and(|s| s.data_type == 1)).unwrap_or(80);
    let ice = record.as_ref().is_some_and(Sauce::ice_colours);
    let (pen, paper) = match amiga {
        Some(palette) => (Pen::amiga(palette), Cell::blank(amiga_colour(palette.pens[1]), amiga_colour(palette.pens[0]))),
        None => (Pen::new(), Cell::blank(VGA[7], VGA[0])),
    };
    let mut term = Terminal { screen: Screen::new(width, 0.5, paper), row: 0, col: 0, saved: (0, 0), pen, ice, unknown: 0 };
    let codes = if amiga.is_some() { Codes::default() } else { Codes::of(body, record.as_ref()) };
    let mut saved_attribute = None;
    let mut i = 0;
    while i < body.len() {
        if codes.any()
            && let Some(used) = term.bbs_code(body, i, codes, &mut saved_attribute)
        {
            i += used;
            continue;
        }
        let byte = body[i];
        i += 1;
        // The Amiga's console takes the single byte 0x9B for ESC [.
        let csi = (byte == 0x1B && body.get(i) == Some(&b'[')) || (amiga.is_some() && byte == 0x9B);
        match byte {
            0x0D => term.col = 0,
            0x0A => {
                term.row += 1;
                term.col = 0;
            }
            0x09 => {
                let next = (term.col / 8 + 1) * 8;
                while term.col < next.min(term.screen.cols) {
                    term.print(' ');
                }
            }
            _ if csi => {
                let mut j = if byte == 0x9B { i } else { i + 1 };
                let private = body.get(j) == Some(&b'?');
                if private {
                    j += 1;
                }
                let mut params: Vec<u16> = Vec::new();
                let mut current: Option<u16> = None;
                while j < body.len() && (body[j].is_ascii_digit() || body[j] == b';') {
                    if body[j] == b';' {
                        params.push(current.take().unwrap_or(0));
                    } else {
                        current = Some(current.unwrap_or(0).saturating_mul(10).saturating_add(u16::from(body[j] - b'0')));
                    }
                    j += 1;
                }
                if let Some(value) = current {
                    params.push(value);
                }
                match body.get(j) {
                    Some(&last) if (0x40..=0x7E).contains(&last) => {
                        term.control(&params, private, last);
                        i = j + 1;
                    }
                    // An unfinished sequence is drawn as what it is, so nothing is hidden.
                    _ => term.print(glyph(set, byte)),
                }
            }
            _ => term.print(glyph(set, byte)),
        }
        if term.row >= crate::screen::MOST_ROWS {
            break;
        }
    }
    let mut screen = term.screen;
    if let Some(record) = record {
        add_sauce_info(&mut screen, &record);
    }
    if term.unknown > 0 {
        screen.note(format!("{} sequence(s) aren't drawn, because this preview doesn't know them.", term.unknown));
    }
    if screen.rows.iter().flatten().any(|cell| cell.flash) {
        screen.note("Blinking text is shown steadily.");
    }
    for (used, name) in [(codes.pcboard, "PCBoard"), (codes.wildcat, "Wildcat!"), (codes.wwiv, "WWIV"), (codes.pipes, "Renegade pipe"), (codes.avatar, "Avatar")] {
        if used {
            screen.info.push(("Colour codes".to_string(), name.to_string()));
        }
    }
    screen
}

/// XBin, as Tasmaniac's XBIN specification (ACiD, 1996) has it: an 11-byte header, an optional palette and font,
/// then character and attribute pairs, run-length compressed a row at a time when the header says so.
pub fn xbin(data: &[u8], set: &Charset) -> Screen {
    let word = |at: usize| usize::from(u16::from_le_bytes([data[at], data[at + 1]]));
    let mut screen = Screen::new(1, 0.5, Cell::blank(VGA[7], VGA[0]));
    if data.len() < 11 {
        screen.note("The XBin header is cut short.");
        return screen;
    }
    let (width, height, font_height, flags) = (word(5).max(1), word(7), usize::from(data[9]), data[10]);
    let mut at = 11;
    let mut palette = VGA;
    if flags & 0x01 != 0 && at + 48 <= data.len() {
        // Sixteen colours of red, green and blue, each 0 to 63, in VGA's order.
        let six = |v: u8| ((u16::from(v & 0x3F) * 255 + 31) / 63) as u8;
        for vga in 0..16 {
            let ansi = VGA_TO_ANSI[vga & 7] + (vga & 8);
            let rgb = &data[at + vga * 3..at + vga * 3 + 3];
            palette[ansi] = Color32::from_rgb(six(rgb[0]), six(rgb[1]), six(rgb[2]));
        }
        at += 48;
    }
    if flags & 0x02 != 0 {
        at += font_height * if flags & 0x10 != 0 { 512 } else { 256 };
        screen.note("The file's own font isn't drawn; the characters are shown in the usual DOS set.");
    }
    let ice = flags & 0x08 != 0;
    let mut cells: Vec<(u8, u8)> = Vec::with_capacity(width * height);
    let image = data.get(at..).unwrap_or(&[]);
    if flags & 0x04 == 0 {
        cells.extend(image.as_chunks::<2>().0.iter().map(|&[c, a]| (c, a)));
    } else {
        let mut i = 0;
        'rows: for _ in 0..height {
            let mut filled = 0;
            while filled < width {
                let Some(&counter) = image.get(i) else {
                    screen.note("The compressed picture ends early.");
                    break 'rows;
                };
                i += 1;
                let count = usize::from(counter & 0x3F) + 1;
                let take = |i: &mut usize| {
                    let byte = image.get(*i).copied().unwrap_or(0);
                    *i += 1;
                    byte
                };
                match counter >> 6 {
                    0 => (0..count).for_each(|_| {
                        let c = take(&mut i);
                        cells.push((c, take(&mut i)));
                    }),
                    1 => {
                        let c = take(&mut i);
                        (0..count).for_each(|_| cells.push((c, take(&mut i))));
                    }
                    2 => {
                        let a = take(&mut i);
                        (0..count).for_each(|_| cells.push((take(&mut i), a)));
                    }
                    _ => {
                        let (c, a) = (take(&mut i), take(&mut i));
                        cells.extend(std::iter::repeat_n((c, a), count));
                    }
                }
                filled += count;
            }
        }
    }
    // A picture wider than any screen is drawn to 1,000 columns, as ANSI art is.
    let shown = width.min(1000);
    if shown < width {
        screen.note(format!("Only the first {shown} of its {width} columns are drawn."));
    }
    screen = Screen { notes: screen.notes, ..Screen::new(shown, 0.5, Cell::blank(palette[7], palette[0])) };
    for (index, (byte, attribute)) in cells.into_iter().enumerate().take(width * height.max(1)) {
        let (row, col) = (index / width, index % width);
        if row >= crate::screen::MOST_ROWS {
            break;
        }
        if col >= shown {
            continue;
        }
        let fg = palette[VGA_TO_ANSI[usize::from(attribute & 7)] + if attribute & 8 != 0 { 8 } else { 0 }];
        let blink = attribute & 0x80 != 0;
        let bg = palette[VGA_TO_ANSI[usize::from((attribute >> 4) & 7)] + if blink && ice { 8 } else { 0 }];
        screen.put(row, col, Cell { ch: glyph(set, byte), fg, bg, flash: blink && !ice, ..screen.paper });
    }
    screen.info.push(("XBin".to_string(), format!("{width} by {height}{}{}", if flags & 0x01 != 0 { ", own palette" } else { "" }, if ice { ", iCE colours" } else { "" })));
    screen
}

fn add_sauce_info(screen: &mut Screen, record: &Sauce) {
    for (key, value) in [("Title", &record.title), ("Author", &record.author), ("Group", &record.group), ("Date", &record.date), ("Font", &record.font)] {
        if !value.is_empty() {
            screen.info.push((key.to_string(), value.clone()));
        }
    }
    let mut kind = record.kind_name().to_string();
    if record.data_type == 1 && record.width > 0 {
        kind.push_str(&format!(", {} columns", record.width));
    }
    if record.ice_colours() {
        kind.push_str(", iCE colours");
    }
    screen.info.push(("SAUCE".to_string(), kind));
    for comment in &record.comments {
        if !comment.trim().is_empty() {
            screen.info.push(("Comment".to_string(), comment.clone()));
        }
    }
}

/// Character and attribute pairs, as in VGA text memory. Without SAUCE it's taken to be 160 columns wide.
pub fn binary_text(body: &[u8], record: Option<Sauce>, set: &Charset) -> Screen {
    // SAUCE gives BinaryText's width in its file type, as half the number of columns.
    let width = record.as_ref().map(|s| usize::from(s.file_type) * 2).filter(|w| *w > 0).unwrap_or(160);
    let ice = record.as_ref().is_some_and(Sauce::ice_colours);
    let mut screen = Screen::new(width, 0.5, Cell::blank(VGA[7], VGA[0]));
    for (index, &[byte, attribute]) in body.as_chunks::<2>().0.iter().enumerate() {
        let fg = VGA[VGA_TO_ANSI[usize::from(attribute & 7)] + if attribute & 8 != 0 { 8 } else { 0 }];
        let back = usize::from((attribute >> 4) & 7);
        let blink = attribute & 0x80 != 0;
        let bg = VGA[VGA_TO_ANSI[back] + if blink && ice { 8 } else { 0 }];
        let (row, col) = (index / width, index % width);
        if row >= crate::screen::MOST_ROWS {
            break;
        }
        screen.put(row, col, Cell { ch: glyph(set, byte), fg, bg, flash: blink && !ice, ..screen.paper });
    }
    if let Some(record) = record {
        add_sauce_info(&mut screen, &record);
    }
    screen
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cp437() -> &'static Charset {
        charset::known("cp437")
    }

    fn with_sauce(body: &[u8], width: u16, flags: u8, comments: &[&str]) -> Vec<u8> {
        let mut data = body.to_vec();
        data.push(0x1A);
        if !comments.is_empty() {
            data.extend_from_slice(b"COMNT");
            for comment in comments {
                let mut line = comment.as_bytes().to_vec();
                line.resize(64, b' ');
                data.extend_from_slice(&line);
            }
        }
        let mut record = vec![0u8; 128];
        record[..7].copy_from_slice(b"SAUCE00");
        let mut put = |at: usize, text: &str, len: usize| {
            let mut bytes = text.as_bytes().to_vec();
            bytes.resize(len, b' ');
            record[at..at + len].copy_from_slice(&bytes);
        };
        put(7, "Dark Tower", 35);
        put(42, "Someone", 20);
        put(62, "ACiD", 20);
        put(82, "19961231", 8);
        record[94] = 1;
        record[95] = 1;
        record[96..98].copy_from_slice(&width.to_le_bytes());
        record[104] = comments.len() as u8;
        record[105] = flags;
        record[106..114].copy_from_slice(b"IBM VGA\0");
        data.extend_from_slice(&record);
        data
    }

    #[test]
    fn a_sauce_record_is_read_with_its_comments_and_leaves_the_art_alone() {
        let data = with_sauce(b"hello", 80, 1, &["first comment"]);
        let record = sauce(&data).unwrap();
        assert_eq!(record.title, "Dark Tower");
        assert_eq!(record.author, "Someone");
        assert_eq!(record.group, "ACiD");
        assert_eq!(record.date, "1996-12-31");
        assert_eq!(record.width, 80);
        assert!(record.ice_colours());
        assert_eq!(record.font, "IBM VGA");
        assert_eq!(record.comments, vec!["first comment".to_string()]);
        assert_eq!(record.content_end, 5);
        assert_eq!(record.kind_name(), "ANSi");
        assert_eq!(sauce(b"no record here"), None);
    }

    #[test]
    fn colours_bold_and_reset_follow_sgr() {
        let screen = render(b"\x1b[1;31mA\x1b[0;44mB\x1b[37;40mC", cp437());
        let row = &screen.rows[0];
        assert_eq!((row[0].ch, row[0].fg), ('A', VGA[9]), "bold red is bright red");
        assert_eq!((row[1].ch, row[1].fg, row[1].bg), ('B', VGA[7], VGA[4]));
        assert_eq!((row[2].fg, row[2].bg), (VGA[7], VGA[0]));
    }

    #[test]
    fn the_cursor_moves_and_lines_wrap_at_the_width() {
        let screen = render(b"\x1b[3;5HX\x1b[2AY\x1b[1;1H\x1b[10CZ", cp437());
        assert_eq!(screen.get(2, 4).unwrap().ch, 'X');
        assert_eq!(screen.get(0, 5).unwrap().ch, 'Y');
        assert_eq!(screen.get(0, 10).unwrap().ch, 'Z');
        let long = vec![b'#'; 85];
        let wrapped = render(&long, cp437());
        assert_eq!(wrapped.rows.len(), 2);
        assert_eq!(wrapped.row_text(1), "#####");
    }

    #[test]
    fn cp437_pictures_and_blocks_are_characters() {
        let screen = render(&[0x01, 0xDB, 0xB0, 0xC4, b'\r', b'\n', 0x03], cp437());
        assert_eq!(screen.row_text(0), "\u{263A}\u{2588}\u{2591}\u{2500}");
        assert_eq!(screen.row_text(1), "\u{2665}");
    }

    #[test]
    fn sauce_sets_the_width_and_ice_colours_turn_blink_into_bright_backgrounds() {
        let data = with_sauce(b"\x1b[5;41mX", 40, 1, &[]);
        let screen = render(&data, cp437());
        assert_eq!(screen.cols, 40);
        assert_eq!(screen.rows[0][0].bg, VGA[9]);
        assert!(!screen.rows[0][0].flash);
        assert!(screen.info.iter().any(|(k, v)| k == "Title" && v == "Dark Tower"));
        let plain = render(b"\x1b[5;41mX", cp437());
        assert_eq!(plain.rows[0][0].bg, VGA[1]);
        assert!(plain.rows[0][0].flash);
    }

    #[test]
    fn extended_colours_and_unknown_sequences() {
        let screen = render(b"\x1b[38;5;196mA\x1b[48;2;1;2;3mB\x1b[1;255;128;0tC\x1b[5zD", cp437());
        assert_eq!(screen.rows[0][0].fg, Color32::from_rgb(255, 0, 0));
        assert_eq!(screen.rows[0][1].bg, Color32::from_rgb(1, 2, 3));
        assert_eq!(screen.rows[0][2].fg, Color32::from_rgb(255, 128, 0));
        assert_eq!(screen.row_text(0), "ABCD");
        assert_eq!(screen.notes.len(), 1, "the unknown one is reported");
    }

    #[test]
    fn erasing_and_clearing() {
        let screen = render(b"abcdef\x1b[1;3H\x1b[K\r\nxy\x1b[2Jz", cp437());
        assert_eq!(screen.rows.len(), 1);
        assert_eq!(screen.row_text(0), "z");
        let erased = render(b"abcdef\x1b[1;3H\x1b[K", cp437());
        assert_eq!(erased.row_text(0), "ab");
    }

    #[test]
    fn an_end_of_file_mark_stops_the_art_and_odd_input_never_goes_wrong() {
        assert_eq!(render(b"ab\x1acd", cp437()).row_text(0), "ab");
        for data in [&b"\x1b["[..], b"\x1b[99999999999999m", b"\x1b[;;;;H", b"\x1b[0A\x1b[0D\x1b[999D", b"\x1b[u\x1b[J"] {
            let _ = render(data, cp437());
        }
    }

    #[test]
    fn bbs_colour_codes_set_the_colours() {
        let screen = render(b"@X1FA@X0EB@X4FC", cp437());
        assert_eq!((screen.rows[0][0].fg, screen.rows[0][0].bg), (VGA[15], VGA[4]), "PCBoard @X1F: bright white on blue");
        assert_eq!(screen.rows[0][1].fg, VGA[11], "@X0E: yellow");
        assert_eq!(screen.row_text(0), "ABC");
        assert!(screen.info.iter().any(|(k, v)| k == "Colour codes" && v == "PCBoard"));
        let pipes = render(b"|14|17A|04B", cp437());
        assert_eq!((pipes.rows[0][0].fg, pipes.rows[0][0].bg), (VGA[11], VGA[4]), "|14 yellow, |17 blue background");
        assert_eq!((pipes.rows[0][1].fg, pipes.rows[0][1].bg), (VGA[1], VGA[4]), "|04 red, keeping the background");
        let wwiv = render(b"\x031A\x034B", cp437());
        assert_eq!(wwiv.rows[0][0].fg, VGA[14], "heart 1: bright cyan");
        assert_eq!((wwiv.rows[0][1].fg, wwiv.rows[0][1].bg), (VGA[15], VGA[4]), "heart 4: bright white on blue");
        let wildcat = render(b"@1F@A@0C@B", cp437());
        assert_eq!(wildcat.rows[0][1].fg, VGA[9], "@0C@: bright red");
        // Avatar: set the attribute, repeat a character, move the cursor.
        let avatar = render(b"\x16\x01\x1eX\x19-\x03\x16\x08\x03\x05Y\x16\x01\x0fZ", cp437());
        assert_eq!((avatar.rows[0][0].fg, avatar.rows[0][0].bg), (VGA[11], VGA[4]), "^V^A 0x1E: yellow on blue");
        assert_eq!(avatar.row_text(0), "X---");
        assert_eq!(avatar.get(2, 4).unwrap().ch, 'Y', "^V^H 3,5");
        // A pipe or an @ that isn't a code stays text.
        assert_eq!(render(b"a|b @home", cp437()).row_text(0), "a|b @home");
    }

    #[test]
    fn xbin_pictures_with_their_palette_and_compression() {
        // 3 by 2, its own palette with colour 1 pure red, compressed: [11,2]A\x01 then [00,2] B\x07 C\x07 [01,0]D\x01.
        let mut data = b"XBIN\x1a".to_vec();
        data.extend([3, 0, 2, 0, 16, 0x01 | 0x04]);
        let mut palette = vec![0u8; 48];
        palette[3..6].copy_from_slice(&[63, 0, 0]);
        data.extend(palette);
        data.extend([0xC2, b'A', 0x01, 0x01, b'B', 0x07, b'C', 0x07, 0x40, b'D', 0x01]);
        let screen = render(&data, cp437());
        assert_eq!(screen.cols, 3);
        assert_eq!(screen.row_text(0), "AAA");
        assert_eq!(screen.row_text(1), "BCD");
        assert_eq!(screen.rows[0][0].fg, Color32::from_rgb(255, 0, 0), "colour 1 of the file's palette");
        let _ = xbin(b"XBIN\x1a\xff\xff\xff\xff\x10\x04\xc5", cp437());
    }

    #[test]
    fn amiga_console_text_takes_the_single_byte_csi() {
        let screen = render_with(b"\x9b31mR\x1b[0mN \xe9", charset::known("amiga-text"), Some(AmigaPalette::default()));
        assert_eq!(screen.row_text(0), "RN \u{e9}");
    }

    #[test]
    fn amiga_colour_codes_choose_pens_in_the_workbench_palette() {
        let palette = AmigaPalette::default();
        let amiga = |data: &[u8], palette: AmigaPalette| render_with(data, charset::known("amiga-text"), Some(palette));
        // Plain text is pen 1 on pen 0: Workbench 1.3's white on blue.
        let screen = amiga(b"A\x1b[33mB\x1b[42mC\x1b[1;33mD\x1b[0mE", palette);
        let cells = &screen.rows[0];
        assert_eq!((cells[0].fg, cells[0].bg), (amiga_colour(0xFFF), amiga_colour(0x05A)));
        assert_eq!(cells[1].fg, amiga_colour(0xF80), "33 is pen 3, Workbench's orange");
        assert_eq!(cells[2].bg, amiga_colour(0x002), "42 is pen 2, Workbench 1.3's black");
        assert_eq!(cells[3].fg, amiga_colour(0xEB0), "bold 33 is pen 11");
        assert_eq!((cells[4].fg, cells[4].bg), (amiga_colour(0xFFF), amiga_colour(0x05A)), "0 goes back to pen 1 on pen 0");
        assert_eq!(screen.paper.bg, amiga_colour(0x05A));
        let same_pen = amiga(b"\x1b[1;33mD", AmigaPalette { bold_high: false, ..palette });
        assert_eq!(same_pen.rows[0][0].fg, amiga_colour(0xF80), "without bold pens, bold keeps its pen");
        let wb2 = amiga(b"A", AmigaPalette { pens: AMIGA_PRESETS[1].1, ..palette });
        assert_eq!((wb2.rows[0][0].fg, wb2.rows[0][0].bg), (amiga_colour(0x000), amiga_colour(0xAAA)), "2.04's black on grey");
    }

    #[test]
    fn the_amiga_palette_is_kept_in_the_settings() {
        let mut palette = AmigaPalette::default();
        palette.pens[5] = 0xF8F;
        palette.bold_high = false;
        assert_eq!(AmigaPalette::from_settings(&palette.to_setting()), palette);
        assert_eq!(AmigaPalette::from_settings("amiga_palette=1,2,3\n"), AmigaPalette::default(), "a broken one is let go");
        assert_eq!(amiga_colour(0xF80), Color32::from_rgb(0xFF, 0x88, 0x00));
    }

    #[test]
    fn binary_text_is_character_and_attribute_pairs() {
        let screen = binary_text(&[b'A', 0x1E, b'B', 0x4F], None, cp437());
        assert_eq!(screen.cols, 160);
        assert_eq!((screen.rows[0][0].fg, screen.rows[0][0].bg), (VGA[11], VGA[4]), "yellow on blue");
        assert_eq!((screen.rows[0][1].fg, screen.rows[0][1].bg), (VGA[15], VGA[1]), "white on red");
    }
}
