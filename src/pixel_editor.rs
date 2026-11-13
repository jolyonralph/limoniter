//! Drawing characters and sprites: the Spectrum's UDGs and fonts (8x8, one colour, a byte a row) and the Next's
//! sprites (16x16, a byte a pixel in the Next's 256 colours, $E3 transparent). They're drawn with the mouse, turned
//! and shifted, and come and go as files (.udg, .chr, .spr) or as DB lines in source.

use crate::theme;
use eframe::egui::{self, Color32, Pos2, Rect, Sense, Ui, Vec2, pos2, vec2};

/// What's being drawn.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    /// 8x8, a bit a pixel: UDGs and fonts.
    Characters,
    /// 16x16, a byte a pixel: the Next's 8-bit sprites.
    Sprites,
}

impl Kind {
    pub fn size(self) -> usize {
        match self {
            Kind::Characters => 8,
            Kind::Sprites => 16,
        }
    }

}

/// The Next's transparent colour, as sprites start.
pub const TRANSPARENT: u8 = 0xE3;

/// A set of characters or sprites, each its pixels as colour numbers (0 or 1 for characters).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Sheet {
    pub kind: Kind,
    pub items: Vec<Vec<u8>>,
}

impl Sheet {
    pub fn new(kind: Kind, count: usize) -> Self {
        let blank = if kind == Kind::Sprites { TRANSPARENT } else { 0 };
        Sheet { kind, items: vec![vec![blank; kind.size() * kind.size()]; count.max(1)] }
    }

    /// From bytes as they're kept: 8 to a character (a row a byte, the left pixel the top bit), 256 to a sprite.
    pub fn from_bytes(kind: Kind, bytes: &[u8]) -> Self {
        let items: Vec<Vec<u8>> = match kind {
            Kind::Characters => bytes.chunks(8).map(|rows| (0..64).map(|i| u8::from(rows.get(i / 8).is_some_and(|r| r & (0x80 >> (i % 8)) != 0))).collect()).collect(),
            Kind::Sprites => bytes.chunks(256).map(|s| (0..256).map(|i| s.get(i).copied().unwrap_or(TRANSPARENT)).collect()).collect(),
        };
        if items.is_empty() { Sheet::new(kind, 1) } else { Sheet { kind, items } }
    }

    pub fn to_bytes(&self) -> Vec<u8> {
        match self.kind {
            Kind::Characters => self.items.iter().flat_map(|c| (0..8).map(move |row| (0..8).fold(0u8, |byte, x| byte | (c[row * 8 + x] & 1) << (7 - x)))).collect(),
            Kind::Sprites => self.items.concat(),
        }
    }

    /// DB lines for source: a line a character, or a row of a sprite a line, labelled if a label's given.
    pub fn to_db(&self, label: &str) -> String {
        let bytes = self.to_bytes();
        let per_line = if self.kind == Kind::Characters { 8 } else { 16 };
        let mut out = String::new();
        if !label.is_empty() {
            out.push_str(&format!("{label}:\n"));
        }
        for line in bytes.chunks(per_line) {
            let items: Vec<String> = line.iter().map(|b| format!("${b:02X}")).collect();
            out.push_str(&format!("        db {}\n", items.join(",")));
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

/// One character or sprite, turned or shifted.
pub fn transform(pixels: &[u8], size: usize, how: Transform, blank: u8) -> Vec<u8> {
    let at = |x: usize, y: usize| pixels[y * size + x];
    (0..size * size)
        .map(|i| {
            let (x, y) = (i % size, i / size);
            match how {
                Transform::FlipAcross => at(size - 1 - x, y),
                Transform::FlipDown => at(x, size - 1 - y),
                Transform::Rotate => at(y, size - 1 - x),
                Transform::Left => if x + 1 < size { at(x + 1, y) } else { blank },
                Transform::Right => if x > 0 { at(x - 1, y) } else { blank },
                Transform::Up => if y + 1 < size { at(x, y + 1) } else { blank },
                Transform::Down => if y > 0 { at(x, y - 1) } else { blank },
                Transform::Invert => if size == 8 { 1 - (at(x, y) & 1) } else { at(x, y) ^ 0xFF },
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

/// What the editor is showing: the sheet, which item, the colour drawn with, and what was there before a stroke.
pub struct Editor {
    pub sheet: Sheet,
    pub current: usize,
    pub colour: u8,
    /// A label for DB lines.
    pub label: String,
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
        let count = if kind == Kind::Characters { 21 } else { 4 };
        Editor { sheet: Sheet::new(kind, count), current: 0, colour: if kind == Kind::Characters { 1 } else { 0xE0 }, label: if kind == Kind::Characters { "udgs".into() } else { "sprites".into() }, undo: Vec::new(), stroke: false }
    }

    fn remember(&mut self) {
        self.undo.push(self.sheet.clone());
        if self.undo.len() > 100 {
            self.undo.remove(0);
        }
    }

    /// The window's contents. `spectrum_basic` offers giving the listing in front these as its UDG pictures.
    pub fn draw(&mut self, ui: &mut Ui, spectrum_basic: bool) -> Option<Asked> {
        let mut asked = None;
        let kind = self.sheet.kind;
        let size = kind.size();
        let blank = if kind == Kind::Sprites { TRANSPARENT } else { 0 };
        ui.horizontal(|ui| {
            let mut chosen = kind;
            ui.radio_value(&mut chosen, Kind::Characters, "8\u{D7}8 characters (UDGs, fonts)");
            ui.radio_value(&mut chosen, Kind::Sprites, "Next sprites (16\u{D7}16, 256 colours)");
            if chosen != kind {
                *self = Editor::new(chosen);
            }
        });
        ui.separator();
        ui.horizontal_top(|ui| {
            // The list of them, small.
            ui.vertical(|ui| {
                ui.set_width(150.0);
                egui::ScrollArea::vertical().id_salt("pixel-list").max_height(380.0).show(ui, |ui| {
                    egui::Grid::new("pixel-thumbs").num_columns(3).spacing([4.0, 4.0]).show(ui, |ui| {
                        for (index, item) in self.sheet.items.iter().enumerate() {
                            let (rect, response) = ui.allocate_exact_size(vec2(40.0, 40.0), Sense::click());
                            paint(ui, rect, item, size, kind);
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
                        self.sheet.items.insert(self.current + 1, vec![blank; size * size]);
                        self.current += 1;
                    }
                    if ui.add_enabled(self.sheet.items.len() > 1, egui::Button::new("Remove").small()).clicked() {
                        self.remember();
                        self.sheet.items.remove(self.current);
                        self.current = self.current.min(self.sheet.items.len() - 1);
                    }
                    if ui.small_button("Copy").on_hover_text("Adds a copy of this one after it").clicked() {
                        self.remember();
                        let copy = self.sheet.items[self.current].clone();
                        self.sheet.items.insert(self.current + 1, copy);
                        self.current += 1;
                    }
                });
            });
            ui.add_space(12.0);
            // The one being drawn, big: left button draws, right button rubs out.
            ui.vertical(|ui| {
                let cell = if kind == Kind::Characters { 36.0 } else { 20.0 };
                let (rect, response) = ui.allocate_exact_size(Vec2::splat(cell * size as f32), Sense::click_and_drag());
                paint(ui, rect, &self.sheet.items[self.current], size, kind);
                for i in 0..=size {
                    let at = i as f32 * cell;
                    let stroke = egui::Stroke::new(if i % 8 == 0 { 1.0 } else { 0.5 }, theme::whitespace().gamma_multiply(0.6));
                    ui.painter().line_segment([pos2(rect.min.x + at, rect.min.y), pos2(rect.min.x + at, rect.max.y)], stroke);
                    ui.painter().line_segment([pos2(rect.min.x, rect.min.y + at), pos2(rect.max.x, rect.min.y + at)], stroke);
                }
                let (primary, secondary) = ui.input(|i| (i.pointer.primary_down(), i.pointer.secondary_down()));
                if let Some(p) = response.interact_pointer_pos().filter(|_| primary || secondary) {
                    if !self.stroke {
                        self.remember();
                        self.stroke = true;
                    }
                    let (x, y) = (((p.x - rect.min.x) / cell) as usize, ((p.y - rect.min.y) / cell) as usize);
                    if x < size && y < size {
                        self.sheet.items[self.current][y * size + x] = if secondary { blank } else { self.colour };
                    }
                } else {
                    self.stroke = false;
                }
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
                        if ui.button(label).on_hover_text(tip).clicked() {
                            self.remember();
                            let item = &mut self.sheet.items[self.current];
                            *item = transform(item, size, how, blank);
                        }
                    }
                    if ui.button("Clear").clicked() {
                        self.remember();
                        self.sheet.items[self.current] = vec![blank; size * size];
                    }
                    if ui.add_enabled(!self.undo.is_empty(), egui::Button::new("Undo")).clicked()
                        && let Some(before) = self.undo.pop()
                    {
                        self.sheet = before;
                        self.current = self.current.min(self.sheet.items.len() - 1);
                    }
                });
                // The Next's colours, to draw sprites with.
                if kind == Kind::Sprites {
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
            if ui.button("Open a file\u{2026}").on_hover_text(".udg, .chr, a font, .spr, or any file of bytes").clicked() {
                asked = Some(Asked::Load);
            }
            if ui.button("Save as\u{2026}").clicked() {
                asked = Some(Asked::Save);
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
}

/// A character or sprite, drawn into a square.
fn paint(ui: &Ui, rect: Rect, pixels: &[u8], size: usize, kind: Kind) {
    let cell = rect.width() / size as f32;
    ui.painter().rect_filled(rect, 0.0, if kind == Kind::Characters { Color32::from_rgb(205, 205, 205) } else { Color32::from_gray(60) });
    for (i, value) in pixels.iter().enumerate() {
        let at: Pos2 = rect.min + vec2((i % size) as f32 * cell, (i / size) as f32 * cell);
        let colour = match kind {
            Kind::Characters if *value != 0 => Color32::BLACK,
            Kind::Characters => continue,
            Kind::Sprites if *value == TRANSPARENT => {
                // A checkerboard where it's see-through.
                if ((i % size) + (i / size)).is_multiple_of(2) { Color32::from_gray(75) } else { continue }
            }
            Kind::Sprites => next_colour(*value),
        };
        ui.painter().rect_filled(Rect::from_min_size(at, Vec2::splat(cell)), 0.0, colour);
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
        assert_eq!(transform(&p, 8, Transform::FlipAcross, 0)[7], 1);
        assert_eq!(transform(&p, 8, Transform::FlipDown, 0)[56], 1);
        assert_eq!(transform(&p, 8, Transform::Rotate, 0)[7], 1, "the top left goes to the top right");
        assert_eq!(transform(&p, 8, Transform::Right, 0)[1], 1);
        assert_eq!(transform(&p, 8, Transform::Left, 0)[0], 0, "off the edge");
        assert_eq!(transform(&p, 8, Transform::Invert, 0).iter().filter(|v| **v == 1).count(), 63);
        let sprite = Sheet::new(Kind::Sprites, 1);
        assert!(sprite.items[0].iter().all(|v| *v == TRANSPARENT));
        assert_eq!(sprite.to_bytes().len(), 256);
        assert_eq!(next_colour(0xE0), Color32::from_rgb(255, 0, 0));
        assert_eq!(next_colour(0x03), Color32::from_rgb(0, 0, 255));
        assert_eq!(next_colour(0xFF), Color32::from_rgb(255, 255, 255));
    }
}
