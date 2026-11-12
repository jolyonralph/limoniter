//! A grid of coloured character cells, the way a retro machine or a terminal would show a file, and the code
//! that draws it in the preview. The emulators for each format (`ansi_art`, `retro_screens`, `basic`) only fill
//! in a `Screen`; nothing here knows about any one format.
//!
//! Glyphs are stretched to fill their cell, because the fonts here are narrower than the cells of most of these
//! machines. Block elements and teletext mosaics are drawn as rectangles instead, so that art made of them
//! joins up with no gaps.

use crate::theme;
use eframe::egui::{self, Align2, Color32, FontId, Pos2, Rect, RichText, Sense, Ui, pos2, vec2};

/// Teletext's two-by-three blocks. Bit 0 is top left, bit 1 top right, then the middle pair and the bottom pair.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Mosaic {
    pub bits: u8,
    pub separated: bool,
}

/// Which part of a double-height character a cell shows.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Height {
    #[default]
    Normal,
    Top,
    Bottom,
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Cell {
    pub ch: char,
    pub fg: Color32,
    pub bg: Color32,
    pub mosaic: Option<Mosaic>,
    pub height: Height,
    pub underline: bool,
    /// Shown steadily here; marked so the emulators' tests can see it.
    pub flash: bool,
}

impl Cell {
    pub fn blank(fg: Color32, bg: Color32) -> Self {
        Self { ch: ' ', fg, bg, mosaic: None, height: Height::Normal, underline: false, flash: false }
    }
}

/// What a format's emulator made of a file.
#[derive(Clone, Debug)]
pub struct Screen {
    pub cols: usize,
    pub rows: Vec<Vec<Cell>>,
    /// A cell's width over its height, from the machine's own screen.
    pub aspect: f32,
    /// The border round the picture, for machines that have one.
    pub border: Option<Color32>,
    /// What an empty cell shows.
    pub paper: Cell,
    /// Facts about the file, such as a SAUCE record's title and author, shown above the picture.
    pub info: Vec<(String, String)>,
    /// What the preview leaves out or couldn't follow, so it's never taken for the whole story.
    pub notes: Vec<String>,
}

/// The most rows any preview keeps, so a runaway file can't use up memory.
pub const MOST_ROWS: usize = 10_000;

impl Screen {
    pub fn new(cols: usize, aspect: f32, paper: Cell) -> Self {
        Self { cols: cols.max(1), rows: Vec::new(), aspect, border: None, paper, info: Vec::new(), notes: Vec::new() }
    }

    /// Makes sure a row exists, growing the screen downwards. False once the screen is as long as it can get.
    pub fn ensure_row(&mut self, row: usize) -> bool {
        if row >= MOST_ROWS {
            return false;
        }
        while self.rows.len() <= row {
            self.rows.push(vec![self.paper; self.cols]);
        }
        true
    }

    pub fn put(&mut self, row: usize, col: usize, cell: Cell) {
        if col < self.cols && self.ensure_row(row) {
            self.rows[row][col] = cell;
        }
    }

    pub fn get(&self, row: usize, col: usize) -> Option<&Cell> {
        self.rows.get(row).and_then(|r| r.get(col))
    }

    /// The text of a row, with trailing spaces taken off, for the tests.
    #[cfg(test)]
    pub fn row_text(&self, row: usize) -> String {
        self.rows.get(row).map_or_else(String::new, |cells| cells.iter().map(|c| c.ch).collect::<String>().trim_end().to_string())
    }

    pub fn note(&mut self, text: impl Into<String>) {
        let text = text.into();
        if !self.notes.contains(&text) {
            self.notes.push(text);
        }
    }
}

/// A colour from its 0xRRGGBB value.
pub const fn rgb(value: u32) -> Color32 {
    Color32::from_rgb((value >> 16) as u8, (value >> 8) as u8, value as u8)
}

/// How much of the foreground colour a shade character has.
fn shade(ch: char) -> Option<f32> {
    match ch {
        '\u{2591}' => Some(0.25),
        '\u{2592}' => Some(0.5),
        '\u{2593}' => Some(0.75),
        _ => None,
    }
}

fn mix(a: Color32, b: Color32, t: f32) -> Color32 {
    let m = |x: u8, y: u8| (f32::from(x) * (1.0 - t) + f32::from(y) * t).round() as u8;
    Color32::from_rgb(m(a.r(), b.r()), m(a.g(), b.g()), m(a.b(), b.b()))
}

/// The parts of a cell a block element fills, as fractions of its width and height.
fn block_parts(ch: char) -> Option<Vec<[f32; 4]>> {
    let eighth = |n: u32| n as f32 / 8.0;
    let quad = |bits: u8| {
        let mut parts = Vec::new();
        for (bit, part) in [(1, [0.0, 0.0, 0.5, 0.5]), (2, [0.5, 0.0, 1.0, 0.5]), (4, [0.0, 0.5, 0.5, 1.0]), (8, [0.5, 0.5, 1.0, 1.0])] {
            if bits & bit != 0 {
                parts.push(part);
            }
        }
        parts
    };
    let value = ch as u32;
    Some(match value {
        0x2580 => vec![[0.0, 0.0, 1.0, 0.5]],
        0x2581..=0x2588 => vec![[0.0, 1.0 - eighth(value - 0x2580), 1.0, 1.0]],
        0x2589..=0x258F => vec![[0.0, 0.0, eighth(0x2590 - value), 1.0]],
        0x2590 => vec![[0.5, 0.0, 1.0, 1.0]],
        0x2594 => vec![[0.0, 0.0, 1.0, 0.125]],
        0x2595 => vec![[0.875, 0.0, 1.0, 1.0]],
        0x2596 => quad(4),
        0x2597 => quad(8),
        0x2598 => quad(1),
        0x2599 => quad(1 | 4 | 8),
        0x259A => quad(1 | 8),
        0x259B => quad(1 | 2 | 4),
        0x259C => quad(1 | 2 | 8),
        0x259D => quad(2),
        0x259E => quad(2 | 4),
        0x259F => quad(2 | 4 | 8),
        _ => return None,
    })
}

fn sub_rect(cell: Rect, part: [f32; 4]) -> Rect {
    let (w, h) = (cell.width(), cell.height());
    Rect::from_min_max(pos2(cell.min.x + part[0] * w, cell.min.y + part[1] * h), pos2(cell.min.x + part[2] * w, cell.min.y + part[3] * h))
}

/// Draws a glyph stretched to fill `target`, which may be larger than the cell for double height.
fn stretched_glyph(painter: &egui::Painter, ch: char, target: Rect, font: &FontId, colour: Color32) {
    let galley = painter.layout_no_wrap(ch.to_string(), font.clone(), colour);
    let size = galley.size();
    if size.x <= 0.0 || size.y <= 0.0 {
        return;
    }
    let (sx, sy) = (target.width() / size.x, target.height() / size.y);
    // A laid-out row's mesh has its texture places in texels; a mesh painted as it is needs them as fractions.
    let [tex_w, tex_h] = painter.ctx().fonts(|fonts| fonts.font_image_size());
    let (u, v) = (1.0 / tex_w.max(1) as f32, 1.0 / tex_h.max(1) as f32);
    for row in &galley.rows {
        let mut mesh = row.visuals.mesh.clone();
        if mesh.is_empty() {
            continue;
        }
        for vertex in &mut mesh.vertices {
            vertex.pos = pos2(target.min.x + (vertex.pos.x + row.pos.x) * sx, target.min.y + (vertex.pos.y + row.pos.y) * sy);
            vertex.uv = pos2(vertex.uv.x * u, vertex.uv.y * v);
            vertex.color = colour;
        }
        painter.add(egui::Shape::mesh(mesh));
    }
}

fn paint_cell(painter: &egui::Painter, cell: &Cell, rect: Rect, font: &FontId) {
    painter.rect_filled(rect, 0.0, cell.bg);
    if let Some(mosaic) = cell.mosaic {
        let gap_x = if mosaic.separated { (rect.width() / 6.0).max(1.0) } else { 0.0 };
        let gap_y = if mosaic.separated { (rect.height() / 9.0).max(1.0) } else { 0.0 };
        let (w, h) = (rect.width() / 2.0, rect.height() / 3.0);
        for (bit, (x, y)) in [(0, (0, 0)), (1, (1, 0)), (2, (0, 1)), (3, (1, 1)), (4, (0, 2)), (6, (1, 2))] {
            if mosaic.bits & (1 << bit) != 0 {
                let min = pos2(rect.min.x + x as f32 * w + gap_x, rect.min.y + y as f32 * h);
                let max = pos2(rect.min.x + (x + 1) as f32 * w, rect.min.y + (y + 1) as f32 * h - gap_y);
                let block = Rect::from_min_max(min, max);
                // Double height mosaics show their own half of a block twice the size.
                let block = match cell.height {
                    Height::Normal => block,
                    Height::Top => Rect::from_min_max(pos2(block.min.x, rect.min.y + (block.min.y - rect.min.y) * 2.0), pos2(block.max.x, rect.min.y + (block.max.y - rect.min.y) * 2.0)),
                    Height::Bottom => Rect::from_min_max(pos2(block.min.x, rect.min.y + (block.min.y - rect.min.y) * 2.0 - rect.height()), pos2(block.max.x, rect.min.y + (block.max.y - rect.min.y) * 2.0 - rect.height())),
                };
                painter.rect_filled(block.intersect(rect), 0.0, cell.fg);
            }
        }
        return;
    }
    if let Some(t) = shade(cell.ch) {
        painter.rect_filled(rect, 0.0, mix(cell.bg, cell.fg, t));
    } else if let Some(parts) = block_parts(cell.ch) {
        for part in parts {
            painter.rect_filled(sub_rect(rect, part), 0.0, cell.fg);
        }
    } else if cell.ch != ' ' {
        let target = match cell.height {
            Height::Normal => rect,
            Height::Top => Rect::from_min_size(rect.min, vec2(rect.width(), rect.height() * 2.0)),
            Height::Bottom => Rect::from_min_size(pos2(rect.min.x, rect.min.y - rect.height()), vec2(rect.width(), rect.height() * 2.0)),
        };
        stretched_glyph(&painter.with_clip_rect(rect.intersect(painter.clip_rect())), cell.ch, target, font, cell.fg);
    }
    if cell.underline {
        painter.hline(rect.x_range(), rect.max.y - 1.0, egui::Stroke::new(1.0, cell.fg));
    }
}

/// How wide a cell is drawn, fitting the screen to the room there is, within readable limits.
fn cell_width(available: f32, cols: usize, border: bool) -> f32 {
    let room = available - if border { 24.0 } else { 0.0 };
    (room / cols as f32).clamp(3.0, 24.0).floor().max(3.0)
}

/// Draws a screen with its facts above it, scrolling both ways once it's bigger than the room it's given.
pub fn draw(ui: &mut Ui, screen: &Screen) {
    for (key, value) in &screen.info {
        ui.horizontal_wrapped(|ui| {
            ui.label(RichText::new(format!("{key}:")).color(theme::text_dim()).size(12.0));
            ui.label(RichText::new(value).size(12.0));
        });
    }
    for note in &screen.notes {
        ui.label(RichText::new(note).color(theme::text_faint()).size(11.5));
    }
    if !screen.info.is_empty() || !screen.notes.is_empty() {
        ui.add_space(4.0);
    }
    let cw = cell_width(ui.available_width(), screen.cols, screen.border.is_some());
    let ch = (cw / screen.aspect).round().max(3.0);
    let pad = if screen.border.is_some() { 12.0 } else { 0.0 };
    let rows = screen.rows.len().max(1);
    let size = vec2(screen.cols as f32 * cw + pad * 2.0, rows as f32 * ch + pad * 2.0);
    let font = FontId::monospace(ch.max(4.0));
    egui::ScrollArea::both().id_salt("screen-preview-scroll").auto_shrink([false, false]).show(ui, |ui| {
        let (area, _) = ui.allocate_exact_size(size, Sense::hover());
        let painter = ui.painter_at(area);
        painter.rect_filled(area, 0.0, screen.border.unwrap_or(screen.paper.bg));
        let origin = area.min + vec2(pad, pad);
        let clip = ui.clip_rect();
        let first = (((clip.min.y - origin.y) / ch).floor().max(0.0)) as usize;
        let last = ((((clip.max.y - origin.y) / ch).ceil().max(0.0)) as usize).min(screen.rows.len());
        let first_col = (((clip.min.x - origin.x) / cw).floor().max(0.0)) as usize;
        let last_col = ((((clip.max.x - origin.x) / cw).ceil().max(0.0)) as usize).min(screen.cols);
        if screen.rows.is_empty() {
            painter.rect_filled(Rect::from_min_size(origin, vec2(screen.cols as f32 * cw, ch)), 0.0, screen.paper.bg);
        }
        for row in first..last {
            for col in first_col..last_col {
                if let Some(cell) = screen.get(row, col) {
                    let rect = Rect::from_min_size(Pos2::new(origin.x + col as f32 * cw, origin.y + row as f32 * ch), vec2(cw, ch));
                    paint_cell(&painter, cell, rect, &font);
                }
            }
        }
    });
    if screen.rows.len() >= MOST_ROWS {
        ui.painter().text(ui.min_rect().left_bottom(), Align2::LEFT_BOTTOM, format!("Only the first {MOST_ROWS} rows are shown."), FontId::proportional(11.0), theme::text_faint());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_screen_grows_down_as_cells_are_written_and_stops_at_its_limit() {
        let mut screen = Screen::new(4, 0.5, Cell::blank(Color32::WHITE, Color32::BLACK));
        screen.put(2, 1, Cell { ch: 'x', ..screen.paper });
        assert_eq!(screen.rows.len(), 3);
        assert_eq!(screen.row_text(2), " x");
        screen.put(0, 9, Cell { ch: 'y', ..screen.paper });
        assert_eq!(screen.row_text(0), "", "a cell past the edge is ignored");
        assert!(!screen.ensure_row(MOST_ROWS));
    }

    #[test]
    fn block_elements_fill_the_right_parts_of_a_cell() {
        assert_eq!(block_parts('\u{2588}'), Some(vec![[0.0, 0.0, 1.0, 1.0]]));
        assert_eq!(block_parts('\u{2584}'), Some(vec![[0.0, 0.5, 1.0, 1.0]]));
        assert_eq!(block_parts('\u{258C}'), Some(vec![[0.0, 0.0, 0.5, 1.0]]));
        assert_eq!(block_parts('\u{259A}').map(|p| p.len()), Some(2));
        assert_eq!(block_parts('A'), None);
        assert_eq!(shade('\u{2592}'), Some(0.5));
    }

    #[test]
    fn screens_draw_in_both_themes_without_going_wrong() {
        let mut screen = Screen::new(8, 0.5, Cell::blank(Color32::WHITE, Color32::BLACK));
        screen.border = Some(Color32::BLUE);
        screen.info.push(("Title".into(), "Test".into()));
        for (col, ch) in ['A', '\u{2588}', '\u{2592}', '\u{259E}', ' '].into_iter().enumerate() {
            screen.put(0, col, Cell { ch, ..screen.paper });
        }
        screen.put(1, 0, Cell { mosaic: Some(Mosaic { bits: 0x7F, separated: true }), height: Height::Top, ..screen.paper });
        screen.put(2, 0, Cell { ch: 'B', height: Height::Bottom, underline: true, ..screen.paper });
        for light in [false, true] {
            theme::set_light(light);
            let ctx = egui::Context::default();
            for _ in 0..2 {
                let raw = egui::RawInput { screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(600.0, 400.0))), ..Default::default() };
                let mut output = ctx.run_ui(raw, |ui| draw(ui, &screen));
                output.textures_delta.clear();
            }
        }
        theme::set_light(false);
    }
}
