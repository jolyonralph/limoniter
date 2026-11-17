//! Designing a ZX Spectrum screen: 256x192 pixels, each 8x8 cell with one INK and one PAPER colour (and BRIGHT and
//! FLASH), as the Spectrum keeps it, so what's drawn is what the Spectrum can show. There's a pencil, lines,
//! rectangles, ellipses, a flood fill, text in the Spectrum's own letters, the cells' colours, and a selection of
//! cells to copy, flip, invert or move; drawing can leave the cells' colours as they are. It's saved as a .scr (6144
//! bytes of bitmap in the Spectrum's order, then 768 of attributes), as a tape that shows it, or as a PNG, and a
//! picture of any kind is brought in through the picture converter.

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

const NAMES: [&str; 8] = ["black", "blue", "red", "magenta", "green", "cyan", "yellow", "white"];

/// A character's eight rows in the 48K ROM's font, as PRINT draws it: space to ©, with £ for ` and © for 127.
pub fn glyph(c: char) -> [u8; 8] {
    let code = match c {
        '£' => 0x60,
        '©' => 0x7F,
        ' '..='\u{7F}' => c as usize,
        _ => b'?' as usize,
    };
    let rom = rustzx_core::zx::roms::ROM_48K;
    let at = 0x3D00 + (code - 32) * 8;
    std::array::from_fn(|row| rom[at + row])
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

    /// A picture brought in as the picture converter makes it for the Spectrum, as it starts.
    pub fn from_image(image: &crate::quantise::Image) -> Screen {
        crate::retro_convert::convert(image, &crate::retro_convert::Settings::default()).spectrum_screen().unwrap_or_default()
    }

    /// The cells from (cx, cy), w by h, with their pixels.
    fn cut_out(&self, (cx, cy, w, h): Cells) -> Clip {
        let mut clip = Clip { w, h, pixels: Vec::with_capacity(w * h * 64), attributes: Vec::with_capacity(w * h) };
        for y in cy * 8..(cy + h) * 8 {
            for x in cx * 8..(cx + w) * 8 {
                clip.pixels.push(self.pixels[y * 256 + x]);
            }
        }
        for y in cy..cy + h {
            for x in cx..cx + w {
                clip.attributes.push(self.attributes[y * 32 + x]);
            }
        }
        clip
    }

    /// Cells put down at (cx, cy), as much of them as is on the screen.
    fn put(&mut self, clip: &Clip, cx: usize, cy: usize) {
        for y in 0..clip.h * 8 {
            for x in 0..clip.w * 8 {
                let (sx, sy) = (cx * 8 + x, cy * 8 + y);
                if sx < 256 && sy < 192 {
                    self.pixels[sy * 256 + sx] = clip.pixels[y * clip.w * 8 + x];
                }
            }
        }
        for y in 0..clip.h {
            for x in 0..clip.w {
                if cx + x < 32 && cy + y < 24 {
                    self.attributes[(cy + y) * 32 + cx + x] = clip.attributes[y * clip.w + x];
                }
            }
        }
    }
}

/// Cells: the first's column and row, and how many across and down.
type Cells = (usize, usize, usize, usize);

/// Cells copied: their pixels, row by row, and their attributes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Clip {
    w: usize,
    h: usize,
    pixels: Vec<bool>,
    attributes: Vec<u8>,
}

impl Clip {
    fn flip_across(&mut self) {
        let width = self.w * 8;
        for row in self.pixels.chunks_mut(width) {
            row.reverse();
        }
        for row in self.attributes.chunks_mut(self.w) {
            row.reverse();
        }
    }

    fn flip_down(&mut self) {
        let width = self.w * 8;
        let rows: Vec<Vec<bool>> = self.pixels.chunks(width).rev().map(<[bool]>::to_vec).collect();
        self.pixels = rows.concat();
        let rows: Vec<Vec<u8>> = self.attributes.chunks(self.w).rev().map(<[u8]>::to_vec).collect();
        self.attributes = rows.concat();
    }
}

/// How the mouse draws.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tool {
    /// Pixels in INK (the right button: PAPER).
    Pencil,
    Line,
    Rectangle,
    FilledRectangle,
    /// From its centre; with Shift, a circle.
    Ellipse,
    /// The pixels joined to the one clicked that are the same as it, made INK (the right button: PAPER).
    Fill,
    /// Text in the ROM's letters: the left button as PRINT writes it, the right only its INK.
    Text,
    /// A cell's colours, its pixels left as they are.
    Colours,
    /// Cells to copy, cut, flip and invert.
    Select,
}

impl Tool {
    const ALL: [(Tool, &'static str, &'static str); 9] = [
        (Tool::Pencil, "Pencil", "The left button draws in INK, the right in PAPER"),
        (Tool::Line, "Line", "Drag from one end to the other: the left button in INK, the right in PAPER"),
        (Tool::Rectangle, "Box", "Drag from corner to corner"),
        (Tool::FilledRectangle, "Filled box", "Drag from corner to corner"),
        (Tool::Ellipse, "Ellipse", "Drag from the centre; hold Shift for a circle"),
        (Tool::Fill, "Fill", "Fills the pixels joined to the one clicked that are the same as it: the left button with INK, the right with PAPER"),
        (Tool::Text, "Text", "Click to write the text in the Spectrum's own letters: the left button as PRINT does, the right with only its INK (as OVER 1 on PAPER)"),
        (Tool::Colours, "Colours", "Gives cells the colours below, leaving their pixels as they are"),
        (Tool::Select, "Select", "Drag across cells, then copy, cut, flip or invert them"),
    ];
}

/// What the designer asks the window to do.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Asked {
    Open,
    /// A picture of any kind, made into a screen in the picture converter.
    BringIn,
    Save,
    SaveTap,
    SavePng,
    Close,
}

/// A drag in progress: where it started, where it is, and which button.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Drag {
    from: (i32, i32),
    to: (i32, i32),
    ink: bool,
}

pub struct Designer {
    pub screen: Screen,
    pub tool: Tool,
    pub ink: u8,
    pub paper: u8,
    pub bright: bool,
    pub flash: bool,
    pub grid: bool,
    /// Whether drawing gives the cells it touches the colours chosen, or leaves them as they are.
    pub colour_cells: bool,
    pub zoom: f32,
    pub text: String,
    /// Text put at the cell clicked, as PRINT AT does, rather than at the pixel.
    pub text_in_cells: bool,
    /// Whether scrolling moves a cell (pixels and colours) at a time or a pixel.
    pub scroll_cells: bool,
    undo: Vec<Screen>,
    redo: Vec<Screen>,
    stroke: Option<(i32, i32)>,
    drag: Option<Drag>,
    pub selection: Option<Cells>,
    clipboard: Option<Clip>,
    pasting: bool,
}

impl Default for Designer {
    fn default() -> Self {
        Designer {
            screen: Screen::default(),
            tool: Tool::Pencil,
            ink: 0,
            paper: 7,
            bright: false,
            flash: false,
            grid: true,
            colour_cells: true,
            zoom: 2.0,
            text: "HELLO".into(),
            text_in_cells: true,
            scroll_cells: false,
            undo: Vec::new(),
            redo: Vec::new(),
            stroke: None,
            drag: None,
            selection: None,
            clipboard: None,
            pasting: false,
        }
    }
}

/// The points of a line, Bresenham's way.
pub(crate) fn line_points((x0, y0): (i32, i32), (x1, y1): (i32, i32)) -> Vec<(i32, i32)> {
    let (dx, dy) = ((x1 - x0).abs(), -(y1 - y0).abs());
    let (sx, sy) = (if x0 < x1 { 1 } else { -1 }, if y0 < y1 { 1 } else { -1 });
    let (mut x, mut y, mut error) = (x0, y0, dx + dy);
    let mut points = Vec::new();
    loop {
        points.push((x, y));
        if x == x1 && y == y1 {
            break;
        }
        let twice = 2 * error;
        if twice >= dy {
            error += dy;
            x += sx;
        }
        if twice <= dx {
            error += dx;
            y += sy;
        }
    }
    points
}

/// The points of an ellipse's edge round (cx, cy) with radii rx and ry, by the midpoint way.
pub(crate) fn ellipse_points((cx, cy): (i32, i32), rx: i32, ry: i32) -> Vec<(i32, i32)> {
    if rx == 0 || ry == 0 {
        return line_points((cx - rx, cy - ry), (cx + rx, cy + ry));
    }
    let mut points = Vec::new();
    let mut four = |x: i32, y: i32| {
        points.extend([(cx + x, cy + y), (cx - x, cy + y), (cx + x, cy - y), (cx - x, cy - y)]);
    };
    let (a2, b2) = (i64::from(rx * rx), i64::from(ry * ry));
    let (mut x, mut y) = (0i64, i64::from(ry));
    // The flatter part, stepping across, then the steeper, stepping down.
    let mut d = b2 - a2 * i64::from(ry) + a2 / 4;
    while b2 * x <= a2 * y {
        four(x as i32, y as i32);
        if d < 0 {
            d += b2 * (2 * x + 3);
        } else {
            d += b2 * (2 * x + 3) + a2 * (2 - 2 * y);
            y -= 1;
        }
        x += 1;
    }
    let mut d = b2 * (2 * x + 1) * (2 * x + 1) / 4 + a2 * (y - 1) * (y - 1) - a2 * b2;
    while y >= 0 {
        four(x as i32, y as i32);
        if d > 0 {
            d += a2 * (3 - 2 * y);
        } else {
            d += b2 * (2 * x + 2) + a2 * (3 - 2 * y);
            x += 1;
        }
        y -= 1;
    }
    points
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
        self.redo.clear();
    }

    pub fn undo(&mut self) {
        if let Some(before) = self.undo.pop() {
            self.redo.push(std::mem::replace(&mut self.screen, before));
        }
    }

    pub fn redo(&mut self) {
        if let Some(after) = self.redo.pop() {
            self.undo.push(std::mem::replace(&mut self.screen, after));
        }
    }

    /// A pixel set (INK) or cleared (PAPER), and its cell coloured if drawing colours cells.
    fn plot(&self, screen: &mut Screen, x: i32, y: i32, on: bool) {
        if !(0..256).contains(&x) || !(0..192).contains(&y) {
            return;
        }
        let (x, y) = (x as usize, y as usize);
        screen.pixels[y * 256 + x] = on;
        if self.colour_cells {
            screen.attributes[(y / 8) * 32 + x / 8] = self.attribute();
        }
    }

    /// What a drag of the tool draws, onto a screen.
    fn shape(&self, screen: &mut Screen, drag: Drag, circle: bool) {
        let Drag { from, to, ink } = drag;
        let (left, right, top, bottom) = (from.0.min(to.0), from.0.max(to.0), from.1.min(to.1), from.1.max(to.1));
        match self.tool {
            Tool::Line => {
                for (x, y) in line_points(from, to) {
                    self.plot(screen, x, y, ink);
                }
            }
            Tool::Rectangle => {
                for (a, b) in [((left, top), (right, top)), ((right, top), (right, bottom)), ((right, bottom), (left, bottom)), ((left, bottom), (left, top))] {
                    for (x, y) in line_points(a, b) {
                        self.plot(screen, x, y, ink);
                    }
                }
            }
            Tool::FilledRectangle => {
                for y in top..=bottom {
                    for x in left..=right {
                        self.plot(screen, x, y, ink);
                    }
                }
            }
            Tool::Ellipse => {
                let (mut rx, mut ry) = ((to.0 - from.0).abs(), (to.1 - from.1).abs());
                if circle {
                    rx = rx.max(ry);
                    ry = rx;
                }
                for (x, y) in ellipse_points(from, rx, ry) {
                    self.plot(screen, x, y, ink);
                }
            }
            _ => {}
        }
    }

    /// The pixels joined to (x, y) that are the same as it, made INK or PAPER.
    pub fn fill(&mut self, x: usize, y: usize, ink: bool) {
        let target = self.screen.pixels[y * 256 + x];
        if target == ink && !self.colour_cells {
            return;
        }
        let mut seen = vec![false; 256 * 192];
        let mut stack = vec![(x, y)];
        let mut screen = self.screen.clone();
        while let Some((x, y)) = stack.pop() {
            if seen[y * 256 + x] || self.screen.pixels[y * 256 + x] != target {
                continue;
            }
            seen[y * 256 + x] = true;
            self.plot(&mut screen, x as i32, y as i32, ink);
            if x > 0 {
                stack.push((x - 1, y));
            }
            if x < 255 {
                stack.push((x + 1, y));
            }
            if y > 0 {
                stack.push((x, y - 1));
            }
            if y < 191 {
                stack.push((x, y + 1));
            }
        }
        self.screen = screen;
    }

    /// The text written at (x, y): as PRINT writes it, or with only its INK.
    pub fn write(&mut self, x: i32, y: i32, print: bool) {
        let mut screen = self.screen.clone();
        for (i, c) in self.text.chars().enumerate() {
            let rows = glyph(c);
            for (row, bits) in rows.iter().enumerate() {
                for column in 0..8 {
                    let on = bits & (0x80 >> column) != 0;
                    if on || print {
                        self.plot(&mut screen, x + i as i32 * 8 + column, y + row as i32, on);
                    }
                }
            }
        }
        self.screen = screen;
    }

    /// The screen moved by (dx, dy), what goes off one side coming on at the other; a cell at a time moves the
    /// colours too.
    pub fn scroll(&mut self, dx: i32, dy: i32) {
        let step = if self.scroll_cells { 8 } else { 1 };
        let (dx, dy) = (dx * step, dy * step);
        let old = self.screen.clone();
        for y in 0..192 {
            for x in 0..256 {
                let (sx, sy) = ((x - dx).rem_euclid(256) as usize, (y - dy).rem_euclid(192) as usize);
                self.screen.pixels[y as usize * 256 + x as usize] = old.pixels[sy * 256 + sx];
            }
        }
        if self.scroll_cells {
            for y in 0..24 {
                for x in 0..32 {
                    let (sx, sy) = ((x - dx / 8).rem_euclid(32) as usize, (y - dy / 8).rem_euclid(24) as usize);
                    self.screen.attributes[y as usize * 32 + x as usize] = old.attributes[sy * 32 + sx];
                }
            }
        }
    }

    /// The cells selected, or the whole screen.
    fn chosen(&self) -> Cells {
        self.selection.unwrap_or((0, 0, 32, 24))
    }

    /// The cells chosen changed by `change` and put back.
    fn change(&mut self, change: impl FnOnce(&mut Clip)) {
        self.remember();
        let cells = self.chosen();
        let mut clip = self.screen.cut_out(cells);
        change(&mut clip);
        self.screen.put(&clip, cells.0, cells.1);
    }

    pub fn flip_across(&mut self) {
        self.change(Clip::flip_across);
    }

    pub fn flip_down(&mut self) {
        self.change(Clip::flip_down);
    }

    /// The pixels inverted: what was INK is PAPER.
    pub fn invert(&mut self) {
        self.change(|clip| clip.pixels.iter_mut().for_each(|p| *p = !*p));
    }

    /// INK and PAPER swapped and the pixels inverted, so it looks the same: a cell's bitmap the other way up.
    pub fn swap_colours(&mut self) {
        self.change(|clip| {
            clip.pixels.iter_mut().for_each(|p| *p = !*p);
            for a in &mut clip.attributes {
                *a = (*a & 0xC0) | (*a & 7) << 3 | (*a >> 3) & 7;
            }
        });
    }

    pub fn copy(&mut self) {
        self.clipboard = Some(self.screen.cut_out(self.chosen()));
    }

    /// The cells copied, and left as PAPER in the colours chosen.
    pub fn cut(&mut self) {
        self.copy();
        let attribute = self.attribute();
        self.change(|clip| {
            clip.pixels.iter_mut().for_each(|p| *p = false);
            clip.attributes.iter_mut().for_each(|a| *a = attribute);
        });
    }

    pub fn paste_at(&mut self, cx: usize, cy: usize) {
        if let Some(clip) = self.clipboard.clone() {
            self.remember();
            self.screen.put(&clip, cx, cy);
        }
    }

    pub fn draw(&mut self, ui: &mut Ui) -> Option<Asked> {
        let mut asked = None;
        ui.horizontal_top(|ui| {
            ui.vertical(|ui| {
                ui.set_width(250.0);
                self.controls(ui);
            });
            ui.separator();
            ui.vertical(|ui| asked = self.screen_side(ui));
        });
        asked
    }

    /// The tools, colours and what can be done to the cells, down the left.
    fn controls(&mut self, ui: &mut Ui) {
        egui::Grid::new("zx-tools").num_columns(3).spacing([4.0, 4.0]).show(ui, |ui| {
            for (i, (tool, name, tip)) in Tool::ALL.into_iter().enumerate() {
                if ui.selectable_label(self.tool == tool && !self.pasting, crate::i18n::tr(name)).on_hover_text(crate::i18n::tr(tip)).clicked() {
                    self.tool = tool;
                    self.pasting = false;
                }
                if i % 3 == 2 {
                    ui.end_row();
                }
            }
        });
        // What the tool in front needs.
        if self.tool == Tool::Text && !self.pasting {
            ui.horizontal(|ui| {
                ui.label(crate::i18n::tr("Text"));
                ui.add(egui::TextEdit::singleline(&mut self.text).desired_width(200.0).font(egui::TextStyle::Monospace));
            });
            ui.checkbox(&mut self.text_in_cells, crate::i18n::tr("At a cell, as PRINT AT")).on_hover_text(crate::i18n::tr("Off, at any pixel"));
        }
        ui.add_space(4.0);
        let swatches = |ui: &mut Ui, label: &str, value: &mut u8, bright: bool| {
            ui.horizontal(|ui| {
                ui.add_sized(vec2(48.0, 18.0), egui::Label::new(label));
                ui.spacing_mut().item_spacing.x = 3.0;
                for index in 0..8u8 {
                    let (rect, response) = ui.allocate_exact_size(vec2(20.0, 18.0), Sense::click());
                    ui.painter().rect_filled(rect, 2.0, colour(index, bright));
                    if *value == index {
                        ui.painter().rect_stroke(rect, 2.0, egui::Stroke::new(2.0, crate::theme::accent()), egui::StrokeKind::Outside);
                    }
                    if response.on_hover_text(crate::i18n::tf!("{index}: {}", NAMES[usize::from(index)])).clicked() {
                        *value = index;
                    }
                }
            });
        };
        swatches(ui, "INK", &mut self.ink, self.bright);
        swatches(ui, "PAPER", &mut self.paper, self.bright);
        ui.horizontal(|ui| {
            ui.checkbox(&mut self.bright, crate::i18n::tr("BRIGHT"));
            ui.checkbox(&mut self.flash, crate::i18n::tr("FLASH"));
        });
        ui.checkbox(&mut self.colour_cells, crate::i18n::tr("Drawing colours the cells")).on_hover_text(crate::i18n::tr("Off, drawing changes only pixels, and each cell keeps the colours it has"));
        ui.add_space(4.0);
        ui.horizontal(|ui| {
            if ui.add_enabled(!self.undo.is_empty(), egui::Button::new(crate::i18n::tr("Undo"))).clicked() {
                self.undo();
            }
            if ui.add_enabled(!self.redo.is_empty(), egui::Button::new(crate::i18n::tr("Redo"))).clicked() {
                self.redo();
            }
            if ui.button(crate::i18n::tr("Clear")).on_hover_text(crate::i18n::tr("PAPER everywhere, in the colours above")).clicked() {
                self.remember();
                self.screen = Screen { pixels: vec![false; 256 * 192], attributes: vec![self.attribute(); 768] };
            }
        });
        ui.separator();
        ui.label(if self.selection.is_some() { crate::i18n::tr("The cells selected") } else { crate::i18n::tr("The whole screen") });
        ui.horizontal_wrapped(|ui| {
            if ui.small_button(crate::i18n::tr("Flip \u{2194}")).on_hover_text(crate::i18n::tr("Left for right")).clicked() {
                self.flip_across();
            }
            if ui.small_button(crate::i18n::tr("Flip \u{2195}")).on_hover_text(crate::i18n::tr("Top for bottom")).clicked() {
                self.flip_down();
            }
            if ui.small_button(crate::i18n::tr("Invert")).on_hover_text(crate::i18n::tr("INK pixels made PAPER, and PAPER INK")).clicked() {
                self.invert();
            }
            if ui.small_button(crate::i18n::tr("Swap INK and PAPER")).on_hover_text(crate::i18n::tr("The colours swapped and the pixels inverted: it looks the same, kept the other way")).clicked() {
                self.swap_colours();
            }
            if ui.small_button(crate::i18n::tr("Copy")).clicked() {
                self.copy();
            }
            if ui.small_button(crate::i18n::tr("Cut")).on_hover_text(crate::i18n::tr("Copied, and left as PAPER in the colours above")).clicked() {
                self.cut();
            }
            if ui.add_enabled(self.clipboard.is_some(), egui::Button::new(crate::i18n::tr("Paste")).small()).on_hover_text(crate::i18n::tr("Then click where its top left cell goes")).clicked() {
                self.pasting = true;
            }
            if self.selection.is_some() && ui.small_button(crate::i18n::tr("Select none")).clicked() {
                self.selection = None;
            }
        });
        ui.horizontal(|ui| {
            ui.label(crate::i18n::tr("Scroll"));
            for (arrow, dx, dy) in [("\u{2190}", -1, 0), ("\u{2192}", 1, 0), ("\u{2191}", 0, -1), ("\u{2193}", 0, 1)] {
                if ui.small_button(arrow).clicked() {
                    self.remember();
                    self.scroll(dx, dy);
                }
            }
            ui.selectable_value(&mut self.scroll_cells, false, crate::i18n::tr("a pixel"));
            ui.selectable_value(&mut self.scroll_cells, true, crate::i18n::tr("a cell")).on_hover_text(crate::i18n::tr("Pixels and colours together"));
        });
        ui.separator();
        ui.horizontal(|ui| {
            ui.checkbox(&mut self.grid, crate::i18n::tr("Cell grid"));
            ui.label(crate::i18n::tr("Zoom"));
            egui::ComboBox::from_id_salt("zx-zoom").width(48.0).selected_text(crate::i18n::tf!("{}\u{D7}", self.zoom)).show_ui(ui, |ui| {
                for z in [1.0, 2.0, 3.0, 4.0] {
                    ui.selectable_value(&mut self.zoom, z, crate::i18n::tf!("{z}\u{D7}"));
                }
            });
        });
    }

    /// The screen, where the mouse is on it, and the files.
    fn screen_side(&mut self, ui: &mut Ui) -> Option<Asked> {
        let mut asked = None;
        let scale = self.zoom;
        let room = ui.ctx().content_rect().size() - vec2(380.0, 200.0);
        let (width, height) = ((256.0 * scale).min(room.x.max(256.0)), (192.0 * scale).min(room.y.max(192.0)));
        let mut hovered = None;
        egui::ScrollArea::both().min_scrolled_width(width).min_scrolled_height(height).max_width(width).max_height(height).show(ui, |ui| {
            hovered = self.canvas(ui, scale);
        });
        // Where the mouse is, as a program would find it.
        let status = match hovered {
            Some((x, y)) => {
                let a = self.screen.attributes[(y / 8) * 32 + x / 8];
                format!(
                    "x {x}, y {y} \u{B7} AT {},{} \u{B7} INK {} PAPER {}{}{}\nbitmap byte {} \u{B7} attribute {}",
                    y / 8,
                    x / 8,
                    a & 7,
                    (a >> 3) & 7,
                    if a & 64 != 0 { " BRIGHT" } else { "" },
                    if a & 128 != 0 { " FLASH" } else { "" },
                    16384 + row_offset(y) + x / 8,
                    22528 + (y / 8) * 32 + x / 8
                )
            }
            None => "\n".to_string(),
        };
        ui.label(egui::RichText::new(status).monospace().size(11.0).color(crate::theme::text_dim()));
        ui.horizontal_wrapped(|ui| {
            ui.set_max_width(width);
            if ui.button(crate::i18n::tr("Open\u{2026}")).on_hover_text(crate::i18n::tr("A Spectrum screen (.scr)")).clicked() {
                asked = Some(Asked::Open);
            }
            if ui.button(crate::i18n::tr("Bring in a picture\u{2026}")).on_hover_text(crate::i18n::tr("A JPEG, PNG or any picture Limoniter reads, made into a Spectrum screen in the picture converter")).clicked() {
                asked = Some(Asked::BringIn);
            }
            if ui.button(crate::i18n::tr("Save as .scr\u{2026}")).clicked() {
                asked = Some(Asked::Save);
            }
            if ui.button(crate::i18n::tr("Save as a tape\u{2026}")).on_hover_text(crate::i18n::tr("A .tap that loads and shows it: LOAD \"\"")).clicked() {
                asked = Some(Asked::SaveTap);
            }
            if ui.button(crate::i18n::tr("Save as PNG\u{2026}")).clicked() {
                asked = Some(Asked::SavePng);
            }
            if ui.button(crate::i18n::tr("Close")).clicked() {
                asked = Some(Asked::Close);
            }
        });
        asked
    }

    /// The screen drawn and drawn on; where the mouse is over it.
    fn canvas(&mut self, ui: &mut Ui, scale: f32) -> Option<(usize, usize)> {
        let (rect, response) = ui.allocate_exact_size(vec2(256.0 * scale, 192.0 * scale), Sense::click_and_drag());
        let to_pixel = |p: egui::Pos2| (((p.x - rect.min.x) / scale).floor() as i32, ((p.y - rect.min.y) / scale).floor() as i32);
        let (primary, secondary, shift) = ui.input(|i| (i.pointer.primary_down(), i.pointer.secondary_down(), i.modifiers.shift));
        let pointer = response.interact_pointer_pos().or_else(|| response.hover_pos()).map(to_pixel);
        let shapes = matches!(self.tool, Tool::Line | Tool::Rectangle | Tool::FilledRectangle | Tool::Ellipse);
        let clicked = response.clicked() || response.secondary_clicked();
        let ink = !response.secondary_clicked() && !secondary;
        let inside = |(x, y): (i32, i32)| (0..256).contains(&x) && (0..192).contains(&y);
        let cell_of = |(x, y): (i32, i32)| ((x.clamp(0, 255) / 8) as usize, (y.clamp(0, 191) / 8) as usize);

        if self.pasting {
            if clicked && let Some(p) = pointer.filter(|p| inside(*p)) {
                let (cx, cy) = cell_of(p);
                self.paste_at(cx, cy);
                self.pasting = false;
            }
        } else {
            match self.tool {
                Tool::Pencil | Tool::Colours => {
                    if let Some(p) = pointer.filter(|_| response.is_pointer_button_down_on() && (primary || secondary)) {
                        let from = match self.stroke {
                            Some(last) => last,
                            None => {
                                self.remember();
                                p
                            }
                        };
                        let mut screen = self.screen.clone();
                        for (x, y) in line_points(from, p) {
                            if self.tool == Tool::Pencil {
                                self.plot(&mut screen, x, y, primary);
                            } else if inside((x, y)) {
                                screen.attributes[(y as usize / 8) * 32 + x as usize / 8] = self.attribute();
                            }
                        }
                        self.screen = screen;
                        self.stroke = Some(p);
                    } else {
                        self.stroke = None;
                    }
                }
                _ if shapes || self.tool == Tool::Select => {
                    if self.drag.is_none() && response.is_pointer_button_down_on() && (primary || secondary)
                        && let Some(p) = pointer
                    {
                        self.drag = Some(Drag { from: p, to: p, ink: !secondary });
                    }
                    if let (Some(drag), Some(p)) = (self.drag.as_mut(), pointer) {
                        drag.to = p;
                    }
                    let released = self.drag.is_some() && !response.is_pointer_button_down_on() && !primary && !secondary;
                    if released && let Some(drag) = self.drag.take() {
                        if self.tool == Tool::Select {
                            let ((ax, ay), (bx, by)) = (cell_of(drag.from), cell_of(drag.to));
                            self.selection = Some((ax.min(bx), ay.min(by), ax.abs_diff(bx) + 1, ay.abs_diff(by) + 1));
                        } else {
                            self.remember();
                            let mut screen = self.screen.clone();
                            self.shape(&mut screen, drag, shift);
                            self.screen = screen;
                        }
                    }
                }
                Tool::Fill => {
                    if clicked && let Some((x, y)) = pointer.filter(|p| inside(*p)) {
                        self.remember();
                        self.fill(x as usize, y as usize, ink);
                    }
                }
                Tool::Text => {
                    if clicked && let Some(p) = pointer.filter(|p| inside(*p)) {
                        self.remember();
                        let (x, y) = if self.text_in_cells { (p.0 / 8 * 8, p.1 / 8 * 8) } else { p };
                        self.write(x, y, ink);
                    }
                }
                _ => {}
            }
        }

        // The screen, with a shape being dragged shown on it.
        let preview;
        let screen = match self.drag {
            Some(drag) if shapes => {
                let mut s = self.screen.clone();
                self.shape(&mut s, drag, shift);
                preview = s;
                &preview
            }
            _ => &self.screen,
        };
        let painter = ui.painter_at(rect);
        for y in 0..192 {
            // A run of the same colour is drawn as one rectangle.
            let mut x = 0;
            while x < 256 {
                let c = screen.shown(x, y);
                let mut end = x + 1;
                while end < 256 && screen.shown(end, y) == c {
                    end += 1;
                }
                painter.rect_filled(Rect::from_min_size(rect.min + vec2(x as f32 * scale, y as f32 * scale), vec2((end - x) as f32 * scale, scale)), 0.0, c);
                x = end;
            }
        }
        if self.grid && scale >= 2.0 {
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
        let cells_rect = |(cx, cy, w, h): Cells| Rect::from_min_size(rect.min + vec2(cx as f32, cy as f32) * 8.0 * scale, vec2(w as f32, h as f32) * 8.0 * scale);
        let marking = egui::Stroke::new(2.0, crate::theme::accent());
        if let Some(cells) = self.selection {
            painter.rect_stroke(cells_rect(cells), 0.0, marking, egui::StrokeKind::Inside);
        }
        if let (Tool::Select, Some(drag)) = (self.tool, self.drag) {
            let ((ax, ay), (bx, by)) = (cell_of(drag.from), cell_of(drag.to));
            painter.rect_stroke(cells_rect((ax.min(bx), ay.min(by), ax.abs_diff(bx) + 1, ay.abs_diff(by) + 1)), 0.0, marking, egui::StrokeKind::Inside);
        }
        // Where a paste or the text would go.
        if let Some(p) = response.hover_pos().map(to_pixel).filter(|p| inside(*p)) {
            if self.pasting && let Some(clip) = &self.clipboard {
                let (cx, cy) = cell_of(p);
                painter.rect_stroke(cells_rect((cx, cy, clip.w, clip.h)), 0.0, marking, egui::StrokeKind::Inside);
            } else if self.tool == Tool::Text {
                let (x, y) = if self.text_in_cells { (p.0 / 8 * 8, p.1 / 8 * 8) } else { p };
                let size = vec2(self.text.chars().count() as f32 * 8.0, 8.0) * scale;
                painter.rect_stroke(Rect::from_min_size(rect.min + vec2(x as f32, y as f32) * scale, size), 0.0, marking, egui::StrokeKind::Outside);
            }
        }
        response.hover_pos().map(to_pixel).filter(|p| inside(*p)).map(|(x, y)| (x as usize, y as usize))
    }
}

/// A PNG's size and its pixels as red, green, blue and alpha.
pub fn read_png(bytes: &[u8]) -> Result<(usize, usize, Vec<u8>), String> {
    let mut decoder = png::Decoder::new(std::io::Cursor::new(bytes));
    decoder.set_transformations(png::Transformations::EXPAND | png::Transformations::STRIP_16);
    let mut reader = decoder.read_info().map_err(|e| e.to_string())?;
    let mut buffer = vec![0; reader.output_buffer_size().ok_or(crate::i18n::tr("The picture is too big."))?];
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
mod tests;
