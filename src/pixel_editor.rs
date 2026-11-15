//! Drawing characters and sprites: the Spectrum's UDGs and fonts (8x8, one colour, a byte a row), the Next's
//! sprites (16x16, a byte a pixel in the Next's 256 colours, $E3 transparent), and the Commodore 64's characters (a
//! whole set of 256, hires or multicolour) and sprites (24x21 hires, 12x21 multicolour). They're drawn with the
//! mouse, turned and shifted, and come and go as files (.udg, .chr, .spr, .bin, .prg with its load address,
//! SpritePad's .spd) or as DB lines in source.

use crate::c64_chars::{SpriteSet, sprite_block, sprite_pixels};
use crate::c64_gfx::{NAMES, PEPTO};
use crate::theme;
use eframe::egui::{self, Color32, Pos2, Rect, Sense, Ui, Vec2, pos2, vec2};

/// What's being drawn.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    /// 8x8, a bit a pixel: UDGs and fonts.
    Characters,
    /// 16x16, a byte a pixel: the Next's 8-bit sprites.
    Sprites,
    /// The C64's 8x8 characters, a bit a pixel.
    C64Characters,
    /// The C64's multicolour characters: 4x8, two bits a pixel twice as wide.
    C64MultiCharacters,
    /// The C64's hires sprites: 24x21, a bit a pixel.
    C64Sprites,
    /// The C64's multicolour sprites: 12x21, two bits a pixel twice as wide.
    C64MultiSprites,
}

impl Kind {
    pub const ALL: [Self; 6] = [Self::Characters, Self::Sprites, Self::C64Characters, Self::C64MultiCharacters, Self::C64Sprites, Self::C64MultiSprites];

    /// Pixels across, as the kind counts them (multicolour ones twice as wide).
    pub fn width(self) -> usize {
        match self {
            Kind::Characters | Kind::C64Characters => 8,
            Kind::C64MultiCharacters => 4,
            Kind::Sprites => 16,
            Kind::C64Sprites => 24,
            Kind::C64MultiSprites => 12,
        }
    }

    pub fn height(self) -> usize {
        match self {
            Kind::Characters | Kind::C64Characters | Kind::C64MultiCharacters => 8,
            Kind::Sprites => 16,
            Kind::C64Sprites | Kind::C64MultiSprites => 21,
        }
    }

    /// How many times wider than tall a pixel is drawn.
    pub fn pixel_width(self) -> f32 {
        if self.multicolour() { 2.0 } else { 1.0 }
    }

    pub fn c64(self) -> bool {
        !matches!(self, Kind::Characters | Kind::Sprites)
    }

    pub fn multicolour(self) -> bool {
        matches!(self, Kind::C64MultiCharacters | Kind::C64MultiSprites)
    }

    pub fn c64_sprite(self) -> bool {
        matches!(self, Kind::C64Sprites | Kind::C64MultiSprites)
    }

    /// Bytes each one takes.
    pub fn bytes(self) -> usize {
        match self {
            Kind::Sprites => 256,
            Kind::C64Sprites | Kind::C64MultiSprites => 64,
            _ => 8,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Kind::Characters => "8\u{D7}8 characters (UDGs, fonts)",
            Kind::Sprites => "Next sprites (16\u{D7}16, 256 colours)",
            Kind::C64Characters => "C64 characters",
            Kind::C64MultiCharacters => "C64 multicolour characters",
            Kind::C64Sprites => "C64 sprites (24\u{D7}21)",
            Kind::C64MultiSprites => "C64 multicolour sprites (12\u{D7}21)",
        }
    }

    fn blank(self) -> u8 {
        if self == Kind::Sprites { TRANSPARENT } else { 0 }
    }
}

/// The Next's transparent colour, as sprites start.
pub const TRANSPARENT: u8 = 0xE3;

/// A set of characters or sprites, each its pixels as colour numbers (0 or 1 for characters, 0 to 3 for the C64's
/// multicolour ones).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Sheet {
    pub kind: Kind,
    pub items: Vec<Vec<u8>>,
    /// For the C64: the background ($D021, or see-through for sprites), the two shared multicolours ($D022 and
    /// $D023 for characters, $D025 and $D026 for sprites), and the characters' colour.
    pub colours: [u8; 4],
    /// For C64 sprites: each one's colour, as the 64th byte holds it.
    pub inks: Vec<u8>,
}

/// The colours a new C64 sheet starts with: blue background, grey and white shared, light blue ink.
const C64_COLOURS: [u8; 4] = [6, 11, 1, 14];

impl Sheet {
    pub fn new(kind: Kind, count: usize) -> Self {
        let count = count.max(1);
        Sheet { kind, items: vec![vec![kind.blank(); kind.width() * kind.height()]; count], colours: C64_COLOURS, inks: vec![C64_COLOURS[3]; count] }
    }

    /// From bytes as they're kept: 8 to a character (a row a byte, the left pixel the top bit), 256 to a Next sprite,
    /// 64 to a C64 sprite (3 bytes a row, the last byte its colour).
    pub fn from_bytes(kind: Kind, bytes: &[u8]) -> Self {
        let size = kind.bytes();
        let mut sheet = Sheet::new(kind, 1);
        sheet.items = bytes
            .chunks(size)
            .map(|chunk| {
                let mut b = chunk.to_vec();
                b.resize(size, if kind == Kind::Sprites { TRANSPARENT } else { 0 });
                match kind {
                    Kind::Characters | Kind::C64Characters => (0..64).map(|i| (b[i / 8] >> (7 - i % 8)) & 1).collect(),
                    Kind::C64MultiCharacters => (0..32).map(|i| (b[i / 4] >> (6 - 2 * (i % 4))) & 3).collect(),
                    Kind::Sprites => b,
                    Kind::C64Sprites | Kind::C64MultiSprites => sprite_pixels(&b, kind.multicolour()),
                }
            })
            .collect();
        if kind.c64_sprite() {
            sheet.inks = bytes.chunks(size).map(|c| c.get(63).map_or(C64_COLOURS[3], |b| b & 15)).collect();
        }
        if sheet.items.is_empty() {
            return Sheet::new(kind, 1);
        }
        sheet.inks.resize(sheet.items.len(), C64_COLOURS[3]);
        sheet
    }

    pub fn to_bytes(&self) -> Vec<u8> {
        let kind = self.kind;
        self.items
            .iter()
            .enumerate()
            .flat_map(|(n, c)| -> Vec<u8> {
                match kind {
                    Kind::Characters | Kind::C64Characters => (0..8).map(|row| (0..8).fold(0u8, |byte, x| byte | (c[row * 8 + x] & 1) << (7 - x))).collect(),
                    Kind::C64MultiCharacters => (0..8).map(|row| (0..4).fold(0u8, |byte, x| byte | (c[row * 4 + x] & 3) << (6 - 2 * x))).collect(),
                    Kind::Sprites => c.clone(),
                    Kind::C64Sprites | Kind::C64MultiSprites => sprite_block(c, kind.multicolour(), self.ink(n), false).to_vec(),
                }
            })
            .collect()
    }

    /// A C64 sprite's colour.
    pub fn ink(&self, item: usize) -> u8 {
        self.inks.get(item).copied().unwrap_or(self.colours[3])
    }

    pub fn insert(&mut self, at: usize, pixels: Vec<u8>, ink: u8) {
        self.items.insert(at, pixels);
        self.inks.resize(self.items.len() - 1, self.colours[3]);
        self.inks.insert(at, ink);
    }

    pub fn remove(&mut self, at: usize) {
        self.items.remove(at);
        if at < self.inks.len() {
            self.inks.remove(at);
        }
    }

    /// The C64 colour (0 to 15) a pixel value shows in, for an item; `None` where it's see-through.
    pub fn c64_colour(&self, value: u8, item: usize) -> Option<u8> {
        let [background, first, second, ink] = self.colours;
        let ink = if self.kind.c64_sprite() { self.ink(item) } else { ink };
        match (self.kind, value) {
            (Kind::C64Sprites | Kind::C64MultiSprites, 0) => None,
            (Kind::C64Sprites, _) => Some(ink),
            (Kind::C64MultiSprites, 1) => Some(first),
            (Kind::C64MultiSprites, 2) => Some(ink),
            (Kind::C64MultiSprites, _) => Some(second),
            (_, 0) => Some(background),
            (Kind::C64MultiCharacters, 1) => Some(first),
            (Kind::C64MultiCharacters, 2) => Some(second),
            // A multicolour character's own colour is one of the first eight.
            (Kind::C64MultiCharacters, _) => Some(ink & 7),
            _ => Some(ink),
        }
    }

    /// C64 sprites as SpritePad keeps them.
    pub fn to_spd(&self) -> Vec<u8> {
        let blocks = self.to_bytes();
        SpriteSet { background: self.colours[0], multicolour1: self.colours[1], multicolour2: self.colours[2], sprites: blocks.chunks(64).map(|b| std::array::from_fn(|i| b[i])).collect() }.to_spd()
    }

    /// From a SpritePad file: multicolour if its first sprite is.
    pub fn from_spd(bytes: &[u8]) -> Option<Self> {
        let set = SpriteSet::from_spd(bytes)?;
        let kind = if set.sprites[0][63] & 0x80 != 0 { Kind::C64MultiSprites } else { Kind::C64Sprites };
        let mut sheet = Sheet::from_bytes(kind, &set.data());
        sheet.colours = [set.background, set.multicolour1, set.multicolour2, sheet.ink(0)];
        Some(sheet)
    }

    /// DB lines for source: a line a character, or a row of a sprite a line, labelled if a label's given. The C64's
    /// are `!byte`, as ACME has it.
    pub fn to_db(&self, label: &str) -> String {
        let bytes = self.to_bytes();
        let (per_line, word) = match self.kind {
            Kind::Sprites => (16, "db"),
            Kind::C64Sprites | Kind::C64MultiSprites => (16, "!byte"),
            Kind::Characters => (8, "db"),
            _ => (8, "!byte"),
        };
        let mut out = String::new();
        if !label.is_empty() {
            out.push_str(&format!("{label}:\n"));
        }
        for line in bytes.chunks(per_line) {
            let items: Vec<String> = line.iter().map(|b| format!("${b:02X}")).collect();
            out.push_str(&format!("        {word} {}\n", items.join(",")));
        }
        out
    }

    /// From the numbers in DB lines (or any text of numbers), as many characters or sprites as they make.
    pub fn from_text(kind: Kind, text: &str) -> Option<Self> {
        let mut bytes = Vec::new();
        for line in text.lines() {
            let code = line.split(';').next().unwrap_or("");
            let lower = code.to_ascii_lowercase();
            let Some(at) = ["db", "defb", "byte", "!byte", "fcb", ".byte"].iter().find_map(|w| lower.find(&format!("{w} ")).map(|i| i + w.len() + 1)) else {
                continue;
            };
            for item in code[at..].split(',') {
                let item = item.trim();
                let value = if let Some(h) = item.strip_prefix('$').or_else(|| item.strip_prefix('#')).or_else(|| item.strip_prefix("0x")) {
                    u8::from_str_radix(h, 16).ok()
                } else if let Some(b) = item.strip_prefix('%') {
                    u8::from_str_radix(b, 2).ok()
                } else {
                    item.parse().ok()
                };
                bytes.push(value?);
            }
        }
        (!bytes.is_empty()).then(|| Sheet::from_bytes(kind, &bytes))
    }
}

/// One character or sprite, turned or shifted. Turning needs it square; otherwise it's left as it is.
pub fn transform(pixels: &[u8], width: usize, height: usize, how: Transform, blank: u8, values: u8) -> Vec<u8> {
    let at = |x: usize, y: usize| pixels[y * width + x];
    (0..width * height)
        .map(|i| {
            let (x, y) = (i % width, i / width);
            match how {
                Transform::FlipAcross => at(width - 1 - x, y),
                Transform::FlipDown => at(x, height - 1 - y),
                Transform::Rotate if width == height => at(y, width - 1 - x),
                Transform::Rotate => at(x, y),
                Transform::Left => if x + 1 < width { at(x + 1, y) } else { blank },
                Transform::Right => if x > 0 { at(x - 1, y) } else { blank },
                Transform::Up => if y + 1 < height { at(x, y + 1) } else { blank },
                Transform::Down => if y > 0 { at(x, y - 1) } else { blank },
                Transform::Invert => values - 1 - at(x, y).min(values - 1),
            }
        })
        .collect()
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Transform {
    FlipAcross,
    FlipDown,
    Rotate,
    Left,
    Right,
    Up,
    Down,
    Invert,
}

/// One of the Next's 256 colours (RRRGGGBB, blue's third bit the OR of the other two), as it shows by default.
pub fn next_colour(index: u8) -> Color32 {
    let r = (index >> 5) & 7;
    let g = (index >> 2) & 7;
    let b2 = index & 3;
    let b = (b2 << 1) | u8::from(b2 != 0);
    let scale = |v: u8| (u16::from(v) * 255 / 7) as u8;
    Color32::from_rgb(scale(r), scale(g), scale(b))
}

fn c64_colour32(index: u8) -> Color32 {
    let c = PEPTO[usize::from(index & 15)];
    Color32::from_rgb(c[0], c[1], c[2])
}

/// What the editor is showing: the sheet, which item, the colour drawn with, and what was there before a stroke.
pub struct Editor {
    pub sheet: Sheet,
    pub current: usize,
    pub colour: u8,
    /// A label for DB lines.
    pub label: String,
    /// Where a C64 .prg of them loads.
    pub load_address: u16,
    /// What it was before each change, to undo.
    undo: Vec<Sheet>,
    stroke: bool,
}

/// What the editor asks the window to do.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Asked {
    Load,
    Save,
    Insert,
    FromSelection,
    UseAsUdgs,
    Close,
}

impl Editor {
    pub fn new(kind: Kind) -> Self {
        let count = match kind {
            Kind::Characters => 21,
            Kind::C64Characters | Kind::C64MultiCharacters => 256,
            Kind::Sprites => 4,
            _ => 8,
        };
        let label = match kind {
            Kind::Characters => "udgs",
            Kind::C64Characters | Kind::C64MultiCharacters => "charset",
            _ => "sprites",
        };
        let colour = match kind {
            Kind::Sprites => 0xE0,
            Kind::C64MultiCharacters | Kind::C64MultiSprites => 2,
            _ => 1,
        };
        Editor { sheet: Sheet::new(kind, count), current: 0, colour, label: label.into(), load_address: 0x2000, undo: Vec::new(), stroke: false }
    }

    fn remember(&mut self) {
        self.undo.push(self.sheet.clone());
        if self.undo.len() > 100 {
            self.undo.remove(0);
        }
    }

    /// The bytes saved as a .prg: the load address first.
    pub fn prg(&self) -> Vec<u8> {
        let mut out = self.load_address.to_le_bytes().to_vec();
        out.extend(self.sheet.to_bytes());
        out
    }

    /// The window's contents. `spectrum_basic` offers giving the listing in front these as its UDG pictures.
    pub fn draw(&mut self, ui: &mut Ui, spectrum_basic: bool) -> Option<Asked> {
        let mut asked = None;
        let kind = self.sheet.kind;
        let (width, height) = (kind.width(), kind.height());
        let blank = kind.blank();
        ui.horizontal(|ui| {
            let mut chosen = kind;
            ui.label("Drawing");
            egui::ComboBox::from_id_salt("pixel-kind").selected_text(kind.name()).show_ui(ui, |ui| {
                for k in Kind::ALL {
                    ui.selectable_value(&mut chosen, k, k.name());
                }
            });
            if chosen != kind {
                let colours = self.sheet.colours;
                *self = Editor::new(chosen);
                self.sheet.colours = colours;
            }
        });
        ui.separator();
        ui.horizontal_top(|ui| {
            // The list of them, small.
            ui.vertical(|ui| {
                ui.set_width(150.0);
                egui::ScrollArea::vertical().id_salt("pixel-list").max_height(380.0).show(ui, |ui| {
                    egui::Grid::new("pixel-thumbs").num_columns(3).spacing([4.0, 4.0]).show(ui, |ui| {
                        for index in 0..self.sheet.items.len() {
                            let (rect, response) = ui.allocate_exact_size(vec2(40.0, 40.0), Sense::click());
                            paint(ui, rect, &self.sheet, index);
                            let label = if kind == Kind::Characters && self.sheet.items.len() == 21 { char::from(b'A' + index as u8).to_string() } else { index.to_string() };
                            ui.painter().text(rect.right_bottom() - vec2(2.0, 1.0), egui::Align2::RIGHT_BOTTOM, label, egui::FontId::proportional(9.0), theme::text_faint());
                            if index == self.current {
                                ui.painter().rect_stroke(rect, 0.0, egui::Stroke::new(2.0, theme::accent()), egui::StrokeKind::Outside);
                            }
                            if response.clicked() {
                                self.current = index;
                            }
                            if index % 3 == 2 {
                                ui.end_row();
                            }
                        }
                    });
                });
                ui.horizontal(|ui| {
                    if ui.small_button("Add").clicked() {
                        self.remember();
                        let ink = self.sheet.ink(self.current);
                        self.sheet.insert(self.current + 1, vec![blank; width * height], ink);
                        self.current += 1;
                    }
                    if ui.add_enabled(self.sheet.items.len() > 1, egui::Button::new("Remove").small()).clicked() {
                        self.remember();
                        self.sheet.remove(self.current);
                        self.current = self.current.min(self.sheet.items.len() - 1);
                    }
                    if ui.small_button("Copy").on_hover_text("Adds a copy of this one after it").clicked() {
                        self.remember();
                        let copy = self.sheet.items[self.current].clone();
                        let ink = self.sheet.ink(self.current);
                        self.sheet.insert(self.current + 1, copy, ink);
                        self.current += 1;
                    }
                });
            });
            ui.add_space(12.0);
            // The one being drawn, big: left button draws, right button rubs out.
            ui.vertical(|ui| {
                let cell = match kind {
                    Kind::Characters | Kind::C64Characters => 36.0,
                    Kind::C64MultiCharacters => 36.0,
                    Kind::Sprites => 20.0,
                    _ => 16.0,
                };
                let cell_x = cell * kind.pixel_width();
                let (rect, response) = ui.allocate_exact_size(vec2(cell_x * width as f32, cell * height as f32), Sense::click_and_drag());
                paint(ui, rect, &self.sheet, self.current);
                let grid = egui::Stroke::new(0.5, theme::whitespace().gamma_multiply(0.6));
                let strong = egui::Stroke::new(1.0, theme::whitespace().gamma_multiply(0.6));
                for i in 0..=width {
                    let at = i as f32 * cell_x;
                    let edge = i % if kind.c64_sprite() { 24 / kind.pixel_width() as usize / 3 } else { 8 } == 0;
                    ui.painter().line_segment([pos2(rect.min.x + at, rect.min.y), pos2(rect.min.x + at, rect.max.y)], if edge { strong } else { grid });
                }
                for i in 0..=height {
                    let at = i as f32 * cell;
                    ui.painter().line_segment([pos2(rect.min.x, rect.min.y + at), pos2(rect.max.x, rect.min.y + at)], if i % 8 == 0 { strong } else { grid });
                }
                let (primary, secondary) = ui.input(|i| (i.pointer.primary_down(), i.pointer.secondary_down()));
                if let Some(p) = response.interact_pointer_pos().filter(|_| primary || secondary) {
                    if !self.stroke {
                        self.remember();
                        self.stroke = true;
                    }
                    let (x, y) = (((p.x - rect.min.x) / cell_x) as usize, ((p.y - rect.min.y) / cell) as usize);
                    if x < width && y < height {
                        self.sheet.items[self.current][y * width + x] = if secondary { blank } else { self.colour };
                    }
                } else {
                    self.stroke = false;
                }
                let values = match kind {
                    Kind::Sprites => 0,
                    k if k.multicolour() => 4,
                    _ => 2,
                };
                ui.horizontal_wrapped(|ui| {
                    for (label, how, tip) in [
                        ("\u{2194}", Transform::FlipAcross, "Flip left to right"),
                        ("\u{2195}", Transform::FlipDown, "Flip top to bottom"),
                        ("\u{21BB}", Transform::Rotate, "Turn a quarter clockwise"),
                        ("\u{2190}", Transform::Left, "Move left a pixel"),
                        ("\u{2192}", Transform::Right, "Move right a pixel"),
                        ("\u{2191}", Transform::Up, "Move up a pixel"),
                        ("\u{2193}", Transform::Down, "Move down a pixel"),
                        ("Invert", Transform::Invert, "Swaps set and clear pixels (the colour's opposite for sprites)"),
                    ] {
                        if how == Transform::Rotate && width != height {
                            continue;
                        }
                        if ui.button(label).on_hover_text(tip).clicked() {
                            self.remember();
                            let item = &mut self.sheet.items[self.current];
                            *item = if kind == Kind::Sprites && how == Transform::Invert { item.iter().map(|v| v ^ 0xFF).collect() } else { transform(item, width, height, how, blank, values.max(2)) };
                        }
                    }
                    if ui.button("Clear").clicked() {
                        self.remember();
                        self.sheet.items[self.current] = vec![blank; width * height];
                    }
                    if ui.add_enabled(!self.undo.is_empty(), egui::Button::new("Undo")).clicked()
                        && let Some(before) = self.undo.pop()
                    {
                        self.sheet = before;
                        self.current = self.current.min(self.sheet.items.len() - 1);
                    }
                });
                if kind == Kind::Sprites {
                    // The Next's colours, to draw sprites with.
                    ui.add_space(4.0);
                    let swatch = 11.0;
                    let (palette, response) = ui.allocate_exact_size(vec2(swatch * 32.0, swatch * 8.0), Sense::click());
                    for index in 0..=255u8 {
                        let (x, y) = (f32::from(index % 32), f32::from(index / 32));
                        let r = Rect::from_min_size(palette.min + vec2(x * swatch, y * swatch), Vec2::splat(swatch));
                        ui.painter().rect_filled(r, 0.0, next_colour(index));
                        if index == self.colour {
                            ui.painter().rect_stroke(r, 0.0, egui::Stroke::new(2.0, Color32::WHITE), egui::StrokeKind::Inside);
                        }
                    }
                    if let Some(p) = response.interact_pointer_pos().filter(|_| response.clicked()) {
                        let (x, y) = (((p.x - palette.min.x) / swatch) as u8, ((p.y - palette.min.y) / swatch) as u8);
                        self.colour = y.min(7) * 32 + x.min(31);
                    }
                    ui.label(egui::RichText::new(format!("Drawing with colour {} (${:02X}){}; the right button rubs out to ${TRANSPARENT:02X}, transparent.", self.colour, self.colour, if self.colour == TRANSPARENT { ", transparent" } else { "" })).size(11.5).color(theme::text_dim()));
                } else if kind.c64() {
                    self.c64_colours(ui);
                } else {
                    ui.label(egui::RichText::new("The left button draws, the right rubs out.").size(11.5).color(theme::text_dim()));
                }
            });
        });
        ui.separator();
        ui.horizontal(|ui| {
            ui.label("Label for DB lines");
            ui.add(egui::TextEdit::singleline(&mut self.label).desired_width(120.0));
            if ui.button("Put in the source").on_hover_text("As DB lines at the cursor").clicked() {
                asked = Some(Asked::Insert);
            }
            if ui.button("From the selection").on_hover_text("Reads the numbers in the DB lines selected").clicked() {
                asked = Some(Asked::FromSelection);
            }
        });
        ui.horizontal(|ui| {
            let hint = if kind.c64() { ".bin, .prg (its load address first), .spd for sprites, or any file of bytes" } else { ".udg, .chr, a font, .spr, or any file of bytes" };
            if ui.button("Open a file\u{2026}").on_hover_text(hint).clicked() {
                asked = Some(Asked::Load);
            }
            if ui.button("Save as\u{2026}").on_hover_text(if kind.c64() { ".bin raw, .prg with the load address, or .spd for sprites" } else { "" }).clicked() {
                asked = Some(Asked::Save);
            }
            if kind.c64() {
                ui.label("Loads at");
                ui.add(egui::DragValue::new(&mut self.load_address).hexadecimal(4, false, true)).on_hover_text("The load address a .prg is saved with");
            }
            if kind == Kind::Characters && spectrum_basic && ui.button("Use as this listing's UDGs").on_hover_text("Draws the listing's UDGs with these, A from the first").clicked() {
                asked = Some(Asked::UseAsUdgs);
            }
            if ui.button("Close").clicked() {
                asked = Some(Asked::Close);
            }
        });
        asked
    }

    /// The C64's colours: what each pixel value shows, each chosen from the 16, and which is drawn with.
    fn c64_colours(&mut self, ui: &mut Ui) {
        let kind = self.sheet.kind;
        let roles: &[(u8, &str, usize)] = match kind {
            Kind::C64Characters => &[(1, "Colour", 3), (0, "Background $D021", 0)],
            Kind::C64MultiCharacters => &[(3, "Colour (0 to 7)", 3), (1, "$D022", 1), (2, "$D023", 2), (0, "Background $D021", 0)],
            Kind::C64Sprites => &[(1, "Sprite's colour", 4), (0, "See-through (shown as $D021)", 0)],
            _ => &[(2, "Sprite's colour", 4), (1, "$D025", 1), (3, "$D026", 2), (0, "See-through (shown as $D021)", 0)],
        };
        ui.add_space(4.0);
        egui::Grid::new("c64-roles").num_columns(2).spacing([8.0, 3.0]).show(ui, |ui| {
            for &(value, label, slot) in roles {
                ui.radio_value(&mut self.colour, value, label);
                let current = if slot == 4 { self.sheet.ink(self.current) } else { self.sheet.colours[slot] };
                let mut chosen = current;
                egui::ComboBox::from_id_salt(("c64-role", slot)).selected_text(egui::RichText::new(format!("\u{25A0} {current}: {}", NAMES[usize::from(current & 15)])).color(c64_colour32(current))).show_ui(ui, |ui| {
                    for c in 0..16u8 {
                        ui.selectable_value(&mut chosen, c, egui::RichText::new(format!("\u{25A0} {c}: {}", NAMES[usize::from(c)])).color(c64_colour32(c)));
                    }
                });
                if chosen != current {
                    self.remember();
                    if slot == 4 {
                        self.sheet.inks.resize(self.sheet.items.len(), self.sheet.colours[3]);
                        self.sheet.inks[self.current] = chosen;
                    } else {
                        self.sheet.colours[slot] = chosen;
                    }
                }
                ui.end_row();
            }
        });
        ui.label(egui::RichText::new("The left button draws with the one chosen, the right rubs out.").size(11.5).color(theme::text_dim()));
    }
}

/// A character or sprite, drawn into a rectangle.
fn paint(ui: &Ui, rect: Rect, sheet: &Sheet, item: usize) {
    let kind = sheet.kind;
    let pixels = &sheet.items[item];
    let (width, height) = (kind.width(), kind.height());
    // The small ones are drawn square; the big one has its pixels their own shape.
    let cell = vec2(rect.width() / width as f32, rect.height() / height as f32);
    let back = match kind {
        Kind::Characters => Color32::from_rgb(205, 205, 205),
        Kind::Sprites => Color32::from_gray(60),
        _ => c64_colour32(sheet.colours[0]),
    };
    ui.painter().rect_filled(rect, 0.0, back);
    for (i, value) in pixels.iter().enumerate() {
        let at: Pos2 = rect.min + vec2((i % width) as f32 * cell.x, (i / width) as f32 * cell.y);
        let colour = match kind {
            Kind::Characters if *value != 0 => Color32::BLACK,
            Kind::Characters => continue,
            Kind::Sprites if *value == TRANSPARENT => {
                // A checkerboard where it's see-through.
                if ((i % width) + (i / width)).is_multiple_of(2) { Color32::from_gray(75) } else { continue }
            }
            Kind::Sprites => next_colour(*value),
            _ => match sheet.c64_colour(*value, item) {
                Some(c) => c64_colour32(c),
                None => continue,
            },
        };
        ui.painter().rect_filled(Rect::from_min_size(at, cell), 0.0, colour);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn characters_go_to_bytes_and_db_lines_and_back() {
        let bytes = [0x18, 0x3C, 0x7E, 0xFF, 0xFF, 0x7E, 0x3C, 0x18, 1, 2, 3, 4, 5, 6, 7, 8];
        let sheet = Sheet::from_bytes(Kind::Characters, &bytes);
        assert_eq!(sheet.items.len(), 2);
        assert_eq!(sheet.items[0][3], 1, "the top row's fourth pixel");
        assert_eq!(sheet.to_bytes(), bytes);
        let db = sheet.to_db("ship");
        assert!(db.starts_with("ship:\n        db $18,$3C,$7E,$FF,$FF,$7E,$3C,$18\n"), "{db}");
        assert_eq!(Sheet::from_text(Kind::Characters, &db), Some(sheet.clone()), "DB lines read back");
        assert_eq!(Sheet::from_text(Kind::Characters, "  defb %00011000, 60, #7E ; a comment\n"), Some(Sheet::from_bytes(Kind::Characters, &[0x18, 60, 0x7E])));
        // The assembler makes the same bytes from the DB lines.
        let assembly = crate::asm::assemble(&crate::asm::z80::Z80, std::path::Path::new("t.asm"), &format!("  org $8000\n{db}"), &|_| Err("no".into()));
        assert_eq!(assembly.image(0x8000, 0x8010), bytes);
    }

    #[test]
    fn pixels_are_turned_and_shifted() {
        let mut p = vec![0u8; 64];
        p[0] = 1;
        assert_eq!(transform(&p, 8, 8, Transform::FlipAcross, 0, 2)[7], 1);
        assert_eq!(transform(&p, 8, 8, Transform::FlipDown, 0, 2)[56], 1);
        assert_eq!(transform(&p, 8, 8, Transform::Rotate, 0, 2)[7], 1, "the top left goes to the top right");
        assert_eq!(transform(&p, 8, 8, Transform::Right, 0, 2)[1], 1);
        assert_eq!(transform(&p, 8, 8, Transform::Left, 0, 2)[0], 0, "off the edge");
        assert_eq!(transform(&p, 8, 8, Transform::Invert, 0, 2).iter().filter(|v| **v == 1).count(), 63);
        let sprite = Sheet::new(Kind::Sprites, 1);
        assert!(sprite.items[0].iter().all(|v| *v == TRANSPARENT));
        assert_eq!(sprite.to_bytes().len(), 256);
        assert_eq!(next_colour(0xE0), Color32::from_rgb(255, 0, 0));
        assert_eq!(next_colour(0x03), Color32::from_rgb(0, 0, 255));
        assert_eq!(next_colour(0xFF), Color32::from_rgb(255, 255, 255));
        // A C64 sprite isn't square: flipping and shifting work on it, turning leaves it.
        let mut s = vec![0u8; 24 * 21];
        s[0] = 1;
        assert_eq!(transform(&s, 24, 21, Transform::FlipAcross, 0, 2)[23], 1);
        assert_eq!(transform(&s, 24, 21, Transform::FlipDown, 0, 2)[20 * 24], 1);
        assert_eq!(transform(&s, 24, 21, Transform::Rotate, 0, 2), s);
        assert_eq!(transform(&[0, 1, 2, 3], 4, 1, Transform::Invert, 0, 4), [3, 2, 1, 0]);
    }

    #[test]
    fn c64_characters_and_sprites_round_trip() {
        // A whole set of 256 hires characters, as bytes.
        let set: Vec<u8> = (0..2048).map(|i| (i * 7 + i / 8) as u8).collect();
        let chars = Sheet::from_bytes(Kind::C64Characters, &set);
        assert_eq!(chars.items.len(), 256);
        assert_eq!(chars.to_bytes(), set);
        // Multicolour: the top two bits are the left pixel.
        let multi = Sheet::from_bytes(Kind::C64MultiCharacters, &set);
        assert_eq!(multi.items[0].len(), 32);
        assert_eq!(multi.items[0][..4], [set[0] >> 6, (set[0] >> 4) & 3, (set[0] >> 2) & 3, set[0] & 3]);
        assert_eq!(multi.to_bytes(), set);
        assert_eq!(multi.c64_colour(3, 0), Some(14 & 7), "a multicolour cell's own colour is one of the first eight");
        // Sprites, with each one's colour in its 64th byte.
        let mut sprites: Vec<u8> = (0..64 * 3).map(|i| (i * 13) as u8).collect();
        for (n, colour) in [(0, 0x87), (1, 0x82), (2, 0x8F)] {
            sprites[n * 64 + 63] = colour;
        }
        let sheet = Sheet::from_bytes(Kind::C64MultiSprites, &sprites);
        assert_eq!(sheet.items.len(), 3);
        assert_eq!(sheet.inks, [7, 2, 15]);
        assert_eq!(sheet.to_bytes(), sprites);
        assert_eq!(sheet.c64_colour(2, 1), Some(2));
        assert_eq!(sheet.c64_colour(0, 1), None, "see-through");
        // SpritePad's file, with the shared colours.
        let mut sheet = sheet;
        sheet.colours = [0, 11, 12, 7];
        let spd = sheet.to_spd();
        let back = Sheet::from_spd(&spd).unwrap();
        assert_eq!(back.kind, Kind::C64MultiSprites);
        assert_eq!((back.items.clone(), back.inks.clone(), back.colours[..3].to_vec()), (sheet.items.clone(), sheet.inks.clone(), vec![0, 11, 12]));
        assert_eq!(back.to_spd(), spd);
        let hires = Sheet::from_bytes(Kind::C64Sprites, &sprites[..64]);
        assert_eq!(hires.items[0].len(), 24 * 21);
        assert_eq!(hires.to_bytes()[..63], sprites[..63]);
        // A .prg has its load address first; !byte lines read back.
        let mut editor = Editor::new(Kind::C64Characters);
        editor.sheet = chars.clone();
        editor.load_address = 0x3800;
        assert_eq!(editor.prg()[..2], [0x00, 0x38]);
        assert_eq!(editor.prg()[2..], set[..]);
        let db = chars.to_db("font");
        assert!(db.contains("        !byte $00,$07,$0E"), "{db}");
        assert_eq!(Sheet::from_text(Kind::C64Characters, &db).unwrap().to_bytes(), set);
        // Adding and removing keep each sprite's colour with it.
        let mut s = Sheet::from_bytes(Kind::C64Sprites, &sprites);
        s.insert(1, vec![0; 24 * 21], 5);
        assert_eq!(s.inks, [7, 5, 2, 15]);
        s.remove(0);
        assert_eq!(s.inks, [5, 2, 15]);
    }
}
