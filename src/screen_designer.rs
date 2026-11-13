//! Designing a ZX Spectrum screen: 256x192 pixels, each 8x8 cell with one INK and one PAPER colour (and BRIGHT and
//! FLASH), as the Spectrum keeps it, so what's drawn is what the Spectrum can show. It's saved as a .scr (6144
//! bytes of bitmap in the Spectrum's order, then 768 of attributes), and a picture can be brought in, each cell
//! given the two colours most of it is.

use eframe::egui::{self, Color32, Rect, Sense, Ui, pos2, vec2};

pub const BITMAP: usize = 6144;
pub const SCR: usize = BITMAP + 768;

/// Where a pixel row starts in the bitmap: a third of the screen at a time, and within it each character row's
/// first pixel row, then their second, and so on.
pub fn row_offset(y: usize) -> usize {
    ((y & 0xC0) << 5) | ((y & 7) << 8) | ((y & 0x38) << 2)
}

/// A Spectrum colour (0 to 7: black, blue, red, magenta, green, cyan, yellow, white), normal or bright.
pub fn colour(index: u8, bright: bool) -> Color32 {
    let level = if bright { 0xFF } else { 0xD7 };
    let on = |bit: u8| if index & bit != 0 { level } else { 0 };
    Color32::from_rgb(on(2), on(4), on(1))
}

/// A screen: a pixel set or not, and each cell's attribute (ink, paper << 3, BRIGHT 64, FLASH 128).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Screen {
    pub pixels: Vec<bool>,
    pub attributes: Vec<u8>,
}

impl Default for Screen {
    fn default() -> Self {
        // Black ink on white paper, as the Spectrum starts.
        Screen { pixels: vec![false; 256 * 192], attributes: vec![0x38; 768] }
    }
}

impl Screen {
    /// From a .scr: with or without a +3DOS header, and a bitmap without attributes in black on white.
    pub fn from_scr(bytes: &[u8]) -> Option<Screen> {
        let data = if bytes.starts_with(b"PLUS3DOS") && bytes.len() >= 128 + BITMAP { &bytes[128..] } else { bytes };
        if data.len() < BITMAP {
            return None;
        }
        let mut screen = Screen::default();
        for y in 0..192 {
            for x in 0..256 {
                screen.pixels[y * 256 + x] = data[row_offset(y) + x / 8] & (0x80 >> (x % 8)) != 0;
            }
        }
        if let Some(attributes) = data.get(BITMAP..SCR) {
            screen.attributes.copy_from_slice(attributes);
        }
        Some(screen)
    }

    pub fn to_scr(&self) -> Vec<u8> {
        let mut bytes = vec![0u8; SCR];
        for y in 0..192 {
            for x in 0..256 {
                if self.pixels[y * 256 + x] {
                    bytes[row_offset(y) + x / 8] |= 0x80 >> (x % 8);
                }
            }
        }
        bytes[BITMAP..].copy_from_slice(&self.attributes);
        bytes
    }

    /// The colour a pixel shows (FLASH shown as it is first, without swapping).
    pub fn shown(&self, x: usize, y: usize) -> Color32 {
        let attribute = self.attributes[(y / 8) * 32 + x / 8];
        let bright = attribute & 64 != 0;
        if self.pixels[y * 256 + x] { colour(attribute & 7, bright) } else { colour((attribute >> 3) & 7, bright) }
    }

    /// A picture brought in: scaled to 256 by 192, each pixel to the nearest Spectrum colour, and each cell given
    /// the two colours most of it is (BRIGHT if most of it is), its pixels set where they're nearer the ink.
    pub fn from_picture(width: usize, height: usize, rgba: &[u8]) -> Screen {
        let mut screen = Screen::default();
        let palette: Vec<(u8, bool, Color32)> = (0..8u8).flat_map(|i| [(i, false, colour(i, false)), (i, true, colour(i, true))]).collect();
        let distance = |a: [u8; 3], b: Color32| -> u32 { (0..3).map(|i| (i32::from(a[i]) - i32::from([b.r(), b.g(), b.b()][i])).pow(2) as u32).sum() };
        let at = |x: usize, y: usize| -> [u8; 3] {
            let (sx, sy) = (x * width / 256, y * height / 192);
            let i = (sy * width + sx) * 4;
            [rgba.get(i).copied().unwrap_or(0), rgba.get(i + 1).copied().unwrap_or(0), rgba.get(i + 2).copied().unwrap_or(0)]
        };
        for cy in 0..24 {
            for cx in 0..32 {
                let mut counts = [0u32; 8];
                let mut bright = 0i32;
                for y in 0..8 {
                    for x in 0..8 {
                        let pixel = at(cx * 8 + x, cy * 8 + y);
                        let (index, is_bright, _) = palette.iter().min_by_key(|(_, _, c)| distance(pixel, *c)).copied().unwrap_or((0, false, Color32::BLACK));
                        counts[usize::from(index)] += 1;
                        bright += if is_bright && index != 0 { 1 } else { -1 };
                    }
                }
                let mut order: Vec<usize> = (0..8).collect();
                order.sort_by_key(|i| std::cmp::Reverse(counts[*i]));
                let (paper, ink) = (order[0] as u8, if counts[order[1]] == 0 { order[0] as u8 } else { order[1] as u8 });
                let bright = bright > 0;
                screen.attributes[cy * 32 + cx] = ink | paper << 3 | if bright { 64 } else { 0 };
                for y in 0..8 {
                    for x in 0..8 {
                        let pixel = at(cx * 8 + x, cy * 8 + y);
                        let near_ink = distance(pixel, colour(ink, bright)) < distance(pixel, colour(paper, bright));
                        screen.pixels[(cy * 8 + y) * 256 + cx * 8 + x] = ink != paper && near_ink;
                    }
                }
            }
        }
        screen
    }
}

/// How the mouse draws.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tool {
    /// Pixels in INK (the right button: PAPER); the cell takes the colours chosen.
    Pencil,
    /// A cell's colours, its pixels left as they are.
    Colours,
}

/// What the designer asks the window to do.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Asked {
    Open,
    Save,
    SavePng,
    Close,
}

pub struct Designer {
    pub screen: Screen,
    pub tool: Tool,
    pub ink: u8,
    pub paper: u8,
    pub bright: bool,
    pub flash: bool,
    pub grid: bool,
    undo: Vec<Screen>,
    stroke: bool,
}

impl Default for Designer {
    fn default() -> Self {
        Designer { screen: Screen::default(), tool: Tool::Pencil, ink: 0, paper: 7, bright: false, flash: false, grid: true, undo: Vec::new(), stroke: false }
    }
}

impl Designer {
    fn attribute(&self) -> u8 {
        self.ink | self.paper << 3 | if self.bright { 64 } else { 0 } | if self.flash { 128 } else { 0 }
    }

    pub fn remember(&mut self) {
        self.undo.push(self.screen.clone());
        if self.undo.len() > 50 {
            self.undo.remove(0);
        }
    }

    pub fn draw(&mut self, ui: &mut Ui) -> Option<Asked> {
        let mut asked = None;
        ui.horizontal(|ui| {
            ui.selectable_value(&mut self.tool, Tool::Pencil, "Pencil").on_hover_text("The left button draws in INK, the right in PAPER; the cell takes the colours below");
            ui.selectable_value(&mut self.tool, Tool::Colours, "Colours").on_hover_text("Gives cells the colours below, leaving their pixels as they are");
            ui.separator();
            ui.checkbox(&mut self.grid, "Cells");
            if ui.add_enabled(!self.undo.is_empty(), egui::Button::new("Undo")).clicked()
                && let Some(before) = self.undo.pop()
            {
                self.screen = before;
            }
            if ui.button("Clear").on_hover_text("PAPER everywhere, in the colours below").clicked() {
                self.remember();
                self.screen = Screen { pixels: vec![false; 256 * 192], attributes: vec![self.attribute(); 768] };
            }
        });
        let swatches = |ui: &mut Ui, label: &str, value: &mut u8, bright: bool| {
            ui.horizontal(|ui| {
                ui.label(label);
                for index in 0..8u8 {
                    let (rect, response) = ui.allocate_exact_size(vec2(22.0, 18.0), Sense::click());
                    ui.painter().rect_filled(rect, 2.0, colour(index, bright));
                    if *value == index {
                        ui.painter().rect_stroke(rect, 2.0, egui::Stroke::new(2.0, crate::theme::accent()), egui::StrokeKind::Outside);
                    }
                    if response.clicked() {
                        *value = index;
                    }
                }
            });
        };
        swatches(ui, "INK  ", &mut self.ink, self.bright);
        swatches(ui, "PAPER", &mut self.paper, self.bright);
        ui.horizontal(|ui| {
            ui.checkbox(&mut self.bright, "BRIGHT");
            ui.checkbox(&mut self.flash, "FLASH");
        });
        // The screen, twice its size, with the border.
        let scale = 2.0;
        let (rect, response) = ui.allocate_exact_size(vec2(256.0 * scale, 192.0 * scale), Sense::click_and_drag());
        let painter = ui.painter_at(rect);
        for y in 0..192 {
            // A run of the same colour is drawn as one rectangle.
            let mut x = 0;
            while x < 256 {
                let c = self.screen.shown(x, y);
                let mut end = x + 1;
                while end < 256 && self.screen.shown(end, y) == c {
                    end += 1;
                }
                painter.rect_filled(Rect::from_min_size(rect.min + vec2(x as f32 * scale, y as f32 * scale), vec2((end - x) as f32 * scale, scale)), 0.0, c);
                x = end;
            }
        }
        if self.grid {
            let stroke = egui::Stroke::new(0.5, Color32::from_black_alpha(60));
            for i in 0..=32 {
                let x = rect.min.x + i as f32 * 8.0 * scale;
                painter.line_segment([pos2(x, rect.min.y), pos2(x, rect.max.y)], stroke);
            }
            for i in 0..=24 {
                let y = rect.min.y + i as f32 * 8.0 * scale;
                painter.line_segment([pos2(rect.min.x, y), pos2(rect.max.x, y)], stroke);
            }
        }
        let (primary, secondary) = ui.input(|i| (i.pointer.primary_down(), i.pointer.secondary_down()));
        if let Some(p) = response.interact_pointer_pos().filter(|_| primary || secondary) {
            if !self.stroke {
                self.remember();
                self.stroke = true;
            }
            let (x, y) = (((p.x - rect.min.x) / scale) as usize, ((p.y - rect.min.y) / scale) as usize);
            if x < 256 && y < 192 {
                let attribute = self.attribute();
                if self.tool == Tool::Pencil {
                    self.screen.pixels[y * 256 + x] = primary;
                }
                self.screen.attributes[(y / 8) * 32 + x / 8] = attribute;
            }
        } else {
            self.stroke = false;
        }
        if let Some(p) = response.hover_pos() {
            let (x, y) = (((p.x - rect.min.x) / scale) as usize, ((p.y - rect.min.y) / scale) as usize);
            if x < 256 && y < 192 {
                let a = self.screen.attributes[(y / 8) * 32 + x / 8];
                response.clone().on_hover_text(format!("x {x}, y {y}; cell {},{}: INK {} PAPER {}{}{}; attribute address {}", x / 8, y / 8, a & 7, (a >> 3) & 7, if a & 64 != 0 { " BRIGHT" } else { "" }, if a & 128 != 0 { " FLASH" } else { "" }, 22528 + (y / 8) * 32 + x / 8));
            }
        }
        ui.horizontal(|ui| {
            if ui.button("Open\u{2026}").on_hover_text("A .scr, or a picture (PNG) to bring in").clicked() {
                asked = Some(Asked::Open);
            }
            if ui.button("Save as .scr\u{2026}").clicked() {
                asked = Some(Asked::Save);
            }
            if ui.button("Save as PNG\u{2026}").clicked() {
                asked = Some(Asked::SavePng);
            }
            if ui.button("Close").clicked() {
                asked = Some(Asked::Close);
            }
        });
        asked
    }
}

/// A PNG's size and its pixels as red, green, blue and alpha.
pub fn read_png(bytes: &[u8]) -> Result<(usize, usize, Vec<u8>), String> {
    let mut decoder = png::Decoder::new(std::io::Cursor::new(bytes));
    decoder.set_transformations(png::Transformations::EXPAND | png::Transformations::STRIP_16);
    let mut reader = decoder.read_info().map_err(|e| e.to_string())?;
    let mut buffer = vec![0; reader.output_buffer_size().ok_or("The picture is too big.")?];
    let info = reader.next_frame(&mut buffer).map_err(|e| e.to_string())?;
    let (width, height) = (info.width as usize, info.height as usize);
    let rgba = match info.color_type {
        png::ColorType::Rgba => buffer[..width * height * 4].to_vec(),
        png::ColorType::Rgb => buffer[..width * height * 3].chunks(3).flat_map(|p| [p[0], p[1], p[2], 255]).collect(),
        png::ColorType::GrayscaleAlpha => buffer[..width * height * 2].chunks(2).flat_map(|p| [p[0], p[0], p[0], p[1]]).collect(),
        _ => buffer[..width * height].iter().flat_map(|g| [*g, *g, *g, 255]).collect(),
    };
    Ok((width, height, rgba))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_screen_goes_to_a_scr_and_back() {
        let mut screen = Screen::default();
        screen.pixels[0] = true;
        screen.pixels[8 * 256 + 1] = true;
        screen.pixels[191 * 256 + 255] = true;
        screen.attributes[0] = 0x47;
        let scr = screen.to_scr();
        assert_eq!(scr.len(), 6912);
        assert_eq!(scr[0], 0x80, "the top left pixel");
        assert_eq!(scr[0x20], 0x40, "pixel row 8 is the second character row's first: 32 bytes on");
        assert_eq!(scr[6143], 0x01, "the bottom right");
        assert_eq!(scr[6144], 0x47);
        assert_eq!(Screen::from_scr(&scr), Some(screen.clone()));
        // As the built-in Spectrum's screen code reads it, the same picture.
        let picture = crate::pictures::decode(crate::pictures::Format::SpectrumScreen, &scr).unwrap();
        assert_eq!(picture.width, 256);
        assert_eq!(row_offset(1), 256);
        assert_eq!(row_offset(64), 2048);
    }

    #[test]
    fn a_picture_is_brought_in_two_colours_a_cell() {
        // Left half red, right half blue, with a white line through the middle of the screen's height.
        let (w, h) = (512, 384);
        let rgba: Vec<u8> = (0..w * h).flat_map(|i| {
            let (x, y) = (i % w, i / w);
            if y == 192 || y == 193 { [255, 255, 255, 255] } else if x < 256 { [215, 0, 0, 255] } else { [0, 0, 215, 255] }
        }).collect();
        let screen = Screen::from_picture(w, h, &rgba);
        assert_eq!(screen.attributes[0] >> 3 & 7, 2, "red paper on the left");
        assert_eq!(screen.attributes[31] >> 3 & 7, 1, "blue on the right");
        let middle = 12 * 32;
        let a = screen.attributes[middle];
        assert!(a & 7 == 7 || (a >> 3) & 7 == 7, "white in the cells the line goes through: {a:02X}");
    }
}
