//! The Spectrum Next's Layer 2 painter: a whole screen drawn as the Next shows it, at 256×192 or 320×256 in 256
//! colours, 640×256 in 16, or LoRes's 128×96, through its palette of 9-bit colours, which is edited here too. A
//! pencil and brush, lines, boxes and ellipses (filled or not), a flood fill, text in the 48K ROM's letters, a
//! colour picker, and a selection to copy, cut, paste (with a colour left see-through), flip, turn and recolour;
//! the screen scrolls, and every change can be undone and redone. What's drawn is kept in `NextScreen`, which saves
//! it as an `.nxi`, the pixels and palette apart, a BMP, a PNG, and a `.nex` that shows it.

use crate::next_screen::{self, NextMode, NextScreen};
use crate::screen_designer::glyph;
use eframe::egui::{self, Color32, Rect, Sense, Ui, pos2, vec2};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tool {
    Pencil,
    Line,
    Rectangle,
    FilledRectangle,
    Ellipse,
    FilledEllipse,
    Fill,
    Text,
    Picker,
    Select,
}

impl Tool {
    const ALL: [(Tool, &'static str, &'static str); 10] = [
        (Tool::Pencil, "Pencil", "The left button draws in the left colour, the right in the right; the brush's size below"),
        (Tool::Line, "Line", "Drag from one end to the other"),
        (Tool::Rectangle, "Box", "Drag from corner to corner"),
        (Tool::FilledRectangle, "Filled box", "Drag from corner to corner"),
        (Tool::Ellipse, "Ellipse", "Drag from the centre; hold Shift for a circle"),
        (Tool::FilledEllipse, "Filled ellipse", "Drag from the centre; hold Shift for a circle"),
        (Tool::Fill, "Fill", "Fills the pixels joined to the one clicked that are its colour"),
        (Tool::Text, "Text", "Click to write in the Spectrum's letters: the left button in the left colour alone, the right on the right colour"),
        (Tool::Picker, "Picker", "Click to take a pixel's colour: the left button for the left colour, the right for the right"),
        (Tool::Select, "Select", "Drag across the screen, then copy, cut, flip or recolour it"),
    ];
}

/// What the painter asks the window to do.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Asked {
    Open,
    OpenPalette,
    BringIn,
    SaveNxi,
    SavePixels,
    SavePalette,
    SaveBmp,
    SavePng,
    SaveNex,
    SaveAll,
    Close,
}

/// Pixels copied.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Clip {
    pub w: usize,
    pub h: usize,
    pub pixels: Vec<u8>,
}

impl Clip {
    fn flip_across(&mut self) {
        for row in self.pixels.chunks_mut(self.w) {
            row.reverse();
        }
    }

    fn flip_down(&mut self) {
        let rows: Vec<Vec<u8>> = self.pixels.chunks(self.w).rev().map(<[u8]>::to_vec).collect();
        self.pixels = rows.concat();
    }

    /// A quarter turn clockwise.
    fn turn(&mut self) {
        let (w, h) = (self.w, self.h);
        let mut turned = vec![0u8; w * h];
        for y in 0..h {
            for x in 0..w {
                turned[x * h + (h - 1 - y)] = self.pixels[y * w + x];
            }
        }
        *self = Clip { w: h, h: w, pixels: turned };
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Drag {
    from: (i32, i32),
    to: (i32, i32),
    right: bool,
}

/// A rectangle of pixels: its left, top, width and height.
pub type Area = (usize, usize, usize, usize);

pub struct Painter {
    pub screen: NextScreen,
    /// What its files are called.
    pub name: String,
    pub tool: Tool,
    /// The colours on the left and right buttons.
    pub left: u8,
    pub right: u8,
    /// The pencil's size, in pixels square.
    pub brush: usize,
    /// A grid every this many pixels, or none.
    pub grid: usize,
    pub zoom: f32,
    pub text: String,
    /// Text twice its size.
    pub big_text: bool,
    /// Pasting leaves the right colour see-through.
    pub paste_over: bool,
    /// Scrolling a pixel or 8 at a time.
    pub scroll_eight: bool,
    pub selection: Option<Area>,
    pub clipboard: Option<Clip>,
    pasting: bool,
    /// What the palette's buttons asked, for the window.
    pending: Option<Asked>,
    undo: Vec<NextScreen>,
    redo: Vec<NextScreen>,
    stroke: Option<(i32, i32)>,
    drag: Option<Drag>,
    texture: Option<egui::TextureHandle>,
    shown: Option<NextScreen>,
}

impl Default for Painter {
    fn default() -> Self {
        Painter {
            screen: NextScreen::new(NextMode::Layer2),
            name: "picture".into(),
            tool: Tool::Pencil,
            left: 0xFF,
            right: 0,
            brush: 1,
            grid: 0,
            zoom: 2.0,
            text: crate::i18n::tr("HELLO NEXT").into(),
            big_text: false,
            paste_over: true,
            scroll_eight: false,
            selection: None,
            clipboard: None,
            pasting: false,
            pending: None,
            undo: Vec::new(),
            redo: Vec::new(),
            stroke: None,
            drag: None,
            texture: None,
            shown: None,
        }
    }
}

/// The points along a filled ellipse's rows: each row's y and its first and last x.
fn ellipse_rows((cx, cy): (i32, i32), rx: i32, ry: i32) -> Vec<(i32, i32, i32)> {
    if ry == 0 {
        return vec![(cy, cx - rx, cx + rx)];
    }
    (-ry..=ry)
        .map(|dy| {
            let t = f64::from(dy) / f64::from(ry);
            let half = (f64::from(rx) * (1.0 - t * t).max(0.0).sqrt()).round() as i32;
            (cy + dy, cx - half, cx + half)
        })
        .collect()
}

impl Painter {
    pub fn new(screen: NextScreen, name: &str) -> Painter {
        let left = (screen.mode.colours() - 1) as u8;
        Painter { screen, name: name.to_string(), left, ..Painter::default() }
    }

    pub fn remember(&mut self) {
        self.undo.push(self.screen.clone());
        if self.undo.len() > 40 {
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

    fn colour(&self, right: bool) -> u8 {
        if right { self.right } else { self.left }
    }

    fn set(screen: &mut NextScreen, x: i32, y: i32, value: u8) {
        let (w, h) = screen.mode.size();
        if (0..w as i32).contains(&x) && (0..h as i32).contains(&y) {
            screen.pixels[y as usize * w + x as usize] = value;
        }
    }

    /// A dab of the brush, its top left at the point less half its size.
    fn dab(&self, screen: &mut NextScreen, x: i32, y: i32, value: u8) {
        let size = self.brush.max(1) as i32;
        let start = size / 2;
        for dy in 0..size {
            for dx in 0..size {
                Self::set(screen, x - start + dx, y - start + dy, value);
            }
        }
    }

    /// What a drag of the tool draws, onto a screen.
    fn shape(&self, screen: &mut NextScreen, drag: Drag, circle: bool) {
        let Drag { from, to, right } = drag;
        let value = self.colour(right);
        let (left, right_x, top, bottom) = (from.0.min(to.0), from.0.max(to.0), from.1.min(to.1), from.1.max(to.1));
        let (mut rx, mut ry) = ((to.0 - from.0).abs(), (to.1 - from.1).abs());
        if circle {
            rx = rx.max(ry);
            ry = rx;
        }
        match self.tool {
            Tool::Line => {
                for (x, y) in crate::screen_designer::line_points(from, to) {
                    self.dab(screen, x, y, value);
                }
            }
            Tool::Rectangle => {
                for (a, b) in [((left, top), (right_x, top)), ((right_x, top), (right_x, bottom)), ((right_x, bottom), (left, bottom)), ((left, bottom), (left, top))] {
                    for (x, y) in crate::screen_designer::line_points(a, b) {
                        self.dab(screen, x, y, value);
                    }
                }
            }
            Tool::FilledRectangle => {
                for y in top..=bottom {
                    for x in left..=right_x {
                        Self::set(screen, x, y, value);
                    }
                }
            }
            Tool::Ellipse => {
                for (x, y) in crate::screen_designer::ellipse_points(from, rx, ry) {
                    self.dab(screen, x, y, value);
                }
            }
            Tool::FilledEllipse => {
                for (y, a, b) in ellipse_rows(from, rx, ry) {
                    for x in a..=b {
                        Self::set(screen, x, y, value);
                    }
                }
            }
            _ => {}
        }
    }

    /// The pixels joined to (x, y) that are its colour, made `value`.
    pub fn fill(&mut self, x: usize, y: usize, value: u8) {
        let (w, h) = self.screen.mode.size();
        let target = self.screen.pixels[y * w + x];
        if target == value {
            return;
        }
        let mut stack = vec![(x, y)];
        while let Some((x, y)) = stack.pop() {
            if self.screen.pixels[y * w + x] != target {
                continue;
            }
            self.screen.pixels[y * w + x] = value;
            if x > 0 {
                stack.push((x - 1, y));
            }
            if x + 1 < w {
                stack.push((x + 1, y));
            }
            if y > 0 {
                stack.push((x, y - 1));
            }
            if y + 1 < h {
                stack.push((x, y + 1));
            }
        }
    }

    /// The text written at (x, y) in the left colour: alone, or on the right colour.
    pub fn write(&mut self, x: i32, y: i32, on_right: bool) {
        let size = if self.big_text { 2 } else { 1 };
        let mut screen = self.screen.clone();
        for (i, c) in self.text.chars().enumerate() {
            for (row, bits) in glyph(c).iter().enumerate() {
                for column in 0..8 {
                    let on = bits & (0x80 >> column) != 0;
                    if !on && !on_right {
                        continue;
                    }
                    let value = if on { self.left } else { self.right };
                    for dy in 0..size {
                        for dx in 0..size {
                            Self::set(&mut screen, x + (i as i32 * 8 + column) * size + dx, y + row as i32 * size + dy, value);
                        }
                    }
                }
            }
        }
        self.screen = screen;
    }

    /// The area chosen: the selection, or the whole screen.
    fn chosen(&self) -> Area {
        let (w, h) = self.screen.mode.size();
        self.selection.unwrap_or((0, 0, w, h))
    }

    fn cut_out(&self, (x, y, w, h): Area) -> Clip {
        let width = self.screen.width();
        let pixels = (y..y + h).flat_map(|row| self.screen.pixels[row * width + x..row * width + x + w].to_vec()).collect();
        Clip { w, h, pixels }
    }

    /// A copy put down with its top left at (x, y), leaving the right colour see-through if asked.
    fn put(&mut self, clip: &Clip, x: i32, y: i32, over: Option<u8>) {
        for row in 0..clip.h {
            for column in 0..clip.w {
                let value = clip.pixels[row * clip.w + column];
                if Some(value) != over {
                    Self::set(&mut self.screen, x + column as i32, y + row as i32, value);
                }
            }
        }
    }

    fn change(&mut self, change: impl FnOnce(&mut Clip)) {
        self.remember();
        let area = self.chosen();
        let mut clip = self.cut_out(area);
        change(&mut clip);
        self.put(&clip, area.0 as i32, area.1 as i32, None);
    }

    pub fn flip_across(&mut self) {
        self.change(Clip::flip_across);
    }

    pub fn flip_down(&mut self) {
        self.change(Clip::flip_down);
    }

    /// The left colour made the right one, in the area chosen.
    pub fn recolour(&mut self) {
        let (from, to) = (self.left, self.right);
        self.change(|clip| clip.pixels.iter_mut().filter(|p| **p == from).for_each(|p| *p = to));
    }

    pub fn copy(&mut self) {
        self.clipboard = Some(self.cut_out(self.chosen()));
    }

    /// Copied, and left in the right colour.
    pub fn cut(&mut self) {
        self.copy();
        let right = self.right;
        self.change(|clip| clip.pixels.iter_mut().for_each(|p| *p = right));
    }

    pub fn turn_copy(&mut self) {
        if let Some(clip) = &mut self.clipboard {
            clip.turn();
        }
    }

    pub fn paste_at(&mut self, x: i32, y: i32) {
        if let Some(clip) = self.clipboard.clone() {
            self.remember();
            let over = self.paste_over.then_some(self.right);
            self.put(&clip, x, y, over);
        }
    }

    /// The screen moved, what goes off one side coming on at the other.
    pub fn scroll(&mut self, dx: i32, dy: i32) {
        let step = if self.scroll_eight { 8 } else { 1 };
        let (w, h) = self.screen.mode.size();
        let old = self.screen.pixels.clone();
        for y in 0..h as i32 {
            for x in 0..w as i32 {
                let (sx, sy) = ((x - dx * step).rem_euclid(w as i32) as usize, (y - dy * step).rem_euclid(h as i32) as usize);
                self.screen.pixels[y as usize * w + x as usize] = old[sy * w + sx];
            }
        }
    }

    /// How many pixels are each colour.
    pub fn used(&self) -> Vec<usize> {
        let mut used = vec![0usize; 256];
        for &p in &self.screen.pixels {
            used[usize::from(p)] += 1;
        }
        used
    }

    pub fn set_mode(&mut self, mode: NextMode) {
        if mode != self.screen.mode {
            self.remember();
            self.screen.change_mode(mode);
            self.left = self.left.min((mode.colours() - 1) as u8);
            self.right = self.right.min((mode.colours() - 1) as u8);
            self.selection = None;
        }
    }

    pub fn draw(&mut self, ui: &mut Ui) -> Option<Asked> {
        let mut asked = None;
        ui.horizontal_top(|ui| {
            ui.vertical(|ui| {
                ui.set_width(270.0);
                // On a small screen, the tools scroll rather than run off its bottom.
                let most = (ui.ctx().content_rect().height() - 140.0).max(300.0);
                egui::ScrollArea::vertical().id_salt("next-controls").max_height(most).show(ui, |ui| self.controls(ui));
            });
            ui.separator();
            ui.vertical(|ui| asked = self.screen_side(ui));
        });
        asked
    }

    fn controls(&mut self, ui: &mut Ui) {
        ui.horizontal(|ui| {
            ui.label(crate::i18n::tr("Mode"));
            let mut mode = self.screen.mode;
            egui::ComboBox::from_id_salt("next-mode").selected_text(crate::i18n::tr(mode.name())).show_ui(ui, |ui| {
                for m in NextMode::ALL {
                    ui.selectable_value(&mut mode, m, crate::i18n::tr(m.name()));
                }
            });
            self.set_mode(mode);
        });
        egui::Grid::new("next-tools").num_columns(3).spacing([4.0, 4.0]).show(ui, |ui| {
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
        match self.tool {
            Tool::Pencil | Tool::Line | Tool::Rectangle | Tool::Ellipse => {
                ui.add(egui::Slider::new(&mut self.brush, 1..=8).text(crate::i18n::tr("brush")));
            }
            Tool::Text => {
                ui.horizontal(|ui| {
                    ui.label(crate::i18n::tr("Text"));
                    ui.add(egui::TextEdit::singleline(&mut self.text).desired_width(200.0).font(egui::TextStyle::Monospace));
                });
                ui.checkbox(&mut self.big_text, crate::i18n::tr("Twice the size"));
            }
            _ => {}
        }
        ui.separator();
        self.palette_ui(ui);
        ui.separator();
        ui.horizontal(|ui| {
            if ui.add_enabled(!self.undo.is_empty(), egui::Button::new(crate::i18n::tr("Undo"))).clicked() {
                self.undo();
            }
            if ui.add_enabled(!self.redo.is_empty(), egui::Button::new(crate::i18n::tr("Redo"))).clicked() {
                self.redo();
            }
            if ui.button(crate::i18n::tr("Clear")).on_hover_text(crate::i18n::tr("The whole screen in the right colour")).clicked() {
                self.remember();
                let right = self.right;
                self.screen.pixels.iter_mut().for_each(|p| *p = right);
            }
        });
        ui.label(if self.selection.is_some() { crate::i18n::tr("The area selected") } else { crate::i18n::tr("The whole screen") });
        ui.horizontal_wrapped(|ui| {
            if ui.small_button(crate::i18n::tr("Flip \u{2194}")).clicked() {
                self.flip_across();
            }
            if ui.small_button(crate::i18n::tr("Flip \u{2195}")).clicked() {
                self.flip_down();
            }
            if ui.small_button(crate::i18n::tr("Left colour to right")).on_hover_text(crate::i18n::tr("Every pixel of the left colour made the right one")).clicked() {
                self.recolour();
            }
            if ui.small_button(crate::i18n::tr("Copy")).clicked() {
                self.copy();
            }
            if ui.small_button(crate::i18n::tr("Cut")).on_hover_text(crate::i18n::tr("Copied, and left in the right colour")).clicked() {
                self.cut();
            }
            let has = self.clipboard.is_some();
            if ui.add_enabled(has, egui::Button::new(crate::i18n::tr("Paste")).small()).on_hover_text(crate::i18n::tr("Then click where its top left goes")).clicked() {
                self.pasting = true;
            }
            if ui.add_enabled(has, egui::Button::new(crate::i18n::tr("Turn the copy")).small()).on_hover_text(crate::i18n::tr("A quarter turn clockwise")).clicked() {
                self.turn_copy();
            }
            if self.selection.is_some() && ui.small_button(crate::i18n::tr("Select none")).clicked() {
                self.selection = None;
            }
        });
        ui.checkbox(&mut self.paste_over, crate::i18n::tr("Paste leaves the right colour see-through"));
        ui.horizontal(|ui| {
            ui.label(crate::i18n::tr("Scroll"));
            for (arrow, dx, dy) in [("\u{2190}", -1, 0), ("\u{2192}", 1, 0), ("\u{2191}", 0, -1), ("\u{2193}", 0, 1)] {
                if ui.small_button(arrow).clicked() {
                    self.remember();
                    self.scroll(dx, dy);
                }
            }
            ui.selectable_value(&mut self.scroll_eight, false, crate::i18n::tr("1"));
            ui.selectable_value(&mut self.scroll_eight, true, crate::i18n::tr("8"));
        });
        ui.horizontal(|ui| {
            ui.label(crate::i18n::tr("Grid"));
            for (size, label) in [(0, "none"), (8, "8"), (16, "16")] {
                ui.selectable_value(&mut self.grid, size, crate::i18n::tr(label));
            }
            ui.label(crate::i18n::tr("Zoom"));
            egui::ComboBox::from_id_salt("next-zoom").width(44.0).selected_text(crate::i18n::tf!("{}\u{D7}", self.zoom)).show_ui(ui, |ui| {
                for z in [1.0, 2.0, 3.0, 4.0] {
                    ui.selectable_value(&mut self.zoom, z, crate::i18n::tf!("{z}\u{D7}"));
                }
            });
        });
    }

    /// The palette: each colour a swatch (the left button choosing the left colour, the right the right), and the
    /// left colour's red, green and blue.
    fn palette_ui(&mut self, ui: &mut Ui) {
        let count = self.screen.mode.colours();
        let size = 15.0;
        let accent = crate::theme::accent();
        let used = self.used();
        ui.spacing_mut().item_spacing = vec2(1.0, 1.0);
        for row in 0..count.div_ceil(16) {
            ui.horizontal(|ui| {
                for column in 0..16 {
                    let index = row * 16 + column;
                    if index >= count {
                        break;
                    }
                    let [r, g, b] = self.screen.colour_of(index as u8);
                    let (rect, response) = ui.allocate_exact_size(vec2(size, size), Sense::click());
                    ui.painter().rect_filled(rect, 0.0, Color32::from_rgb(r, g, b));
                    if usize::from(self.left) == index {
                        ui.painter().rect_stroke(rect, 0.0, egui::Stroke::new(2.0, accent), egui::StrokeKind::Inside);
                    }
                    if usize::from(self.right) == index {
                        ui.painter().rect_stroke(rect.shrink(4.0), 0.0, egui::Stroke::new(2.0, accent), egui::StrokeKind::Inside);
                    }
                    let value = self.screen.palette[index];
                    let response = response.on_hover_text(crate::i18n::tf!("Colour {index}: {:03b} {:03b} {:03b} (${:03X}); {} pixels", value >> 6, (value >> 3) & 7, value & 7, value, used[index]));
                    if response.clicked() {
                        self.left = index as u8;
                    }
                    if response.secondary_clicked() {
                        self.right = index as u8;
                    }
                }
            });
        }
        ui.spacing_mut().item_spacing = vec2(8.0, 4.0);
        ui.label(crate::i18n::tf!("Left colour {}, right {}", self.left, self.right));
        let index = usize::from(self.left);
        let value = self.screen.palette[index];
        let (mut r, mut g, mut b) = (value >> 6, (value >> 3) & 7, value & 7);
        let before = (r, g, b);
        ui.add(egui::Slider::new(&mut r, 0..=7).text(crate::i18n::tr("red")));
        ui.add(egui::Slider::new(&mut g, 0..=7).text(crate::i18n::tr("green")));
        ui.add(egui::Slider::new(&mut b, 0..=7).text(crate::i18n::tr("blue"))).on_hover_text(crate::i18n::tr("Blue's lowest bit is the ninth, which only some colours need"));
        if (r, g, b) != before {
            if self.undo.last().is_none_or(|u| u.palette == self.screen.palette) {
                self.remember();
            }
            self.screen.palette[index] = r << 6 | g << 3 | b;
        }
        ui.horizontal(|ui| {
            if ui.small_button(crate::i18n::tr("The Next's palette")).on_hover_text(crate::i18n::tr("Every colour as the Next has it at reset")).clicked() {
                self.remember();
                self.screen.palette = if count == 16 { NextScreen::new(NextMode::Layer2Hi).palette } else { next_screen::default_palette() };
            }
            if ui.small_button(crate::i18n::tr("Open\u{2026}")).on_hover_text(crate::i18n::tr("A palette (.nxp, 512 or 256 bytes)")).clicked() {
                self.pending = Some(Asked::OpenPalette);
            }
            if ui.small_button(crate::i18n::tr("Save\u{2026}")).on_hover_text(crate::i18n::tr("The palette alone (.nxp)")).clicked() {
                self.pending = Some(Asked::SavePalette);
            }
        });
    }

    fn screen_side(&mut self, ui: &mut Ui) -> Option<Asked> {
        let mut asked = self.pending.take();
        let mode = self.screen.mode;
        let (w, h) = mode.size();
        let base = if mode == NextMode::LoRes { 2.0 } else { 1.0 };
        let scale = vec2(self.zoom * base * mode.aspect(), self.zoom * base);
        let room = ui.ctx().content_rect().size() - vec2(400.0, 200.0);
        let (width, height) = ((w as f32 * scale.x).min(room.x.max(256.0)), (h as f32 * scale.y).min(room.y.max(192.0)));
        let mut hovered = None;
        egui::ScrollArea::both().min_scrolled_width(width).min_scrolled_height(height).max_width(width).max_height(height).show(ui, |ui| {
            hovered = self.canvas(ui, scale);
        });
        let status = match hovered {
            Some((x, y)) => {
                let index = self.screen.pixels[y * w + x];
                let value = self.screen.palette[usize::from(index)];
                let byte = match mode {
                    NextMode::Layer2 => y * 256 + x,
                    NextMode::Layer2Wide => x * 256 + y,
                    NextMode::Layer2Hi => (x / 2) * 256 + y,
                    NextMode::LoRes => y * 128 + x,
                };
                let place = if mode == NextMode::LoRes {
                    format!("${:04X}", if byte < 6144 { 0x4000 + byte } else { 0x6000 + byte - 6144 })
                } else {
                    format!("16K bank {}, ${:04X}", 9 + byte / 16384, byte % 16384)
                };
                format!("x {x}, y {y} \u{B7} colour {index} (${value:03X})\nbyte {byte}: {place}")
            }
            None => "\n".to_string(),
        };
        ui.label(egui::RichText::new(status).monospace().size(11.0).color(crate::theme::text_dim()));
        ui.horizontal_wrapped(|ui| {
            ui.set_max_width(width.max(420.0));
            for (label, tip, what) in [
                ("Open\u{2026}", "An .nxi, .sl2, .slr or a BMP of a Next screen's size", Asked::Open),
                ("Bring in a picture\u{2026}", "A JPEG, PNG or any picture, made into this mode in the picture converter", Asked::BringIn),
                ("Save as .nxi\u{2026}", "The palette, then the pixels as the Next keeps them", Asked::SaveNxi),
                ("Pixels alone\u{2026}", "An .sl2 (or .slr), in the Next's own order, without the palette", Asked::SavePixels),
                ("BMP\u{2026}", "As .bmpload takes it (at 256\u{D7}192)", Asked::SaveBmp),
                ("PNG\u{2026}", "", Asked::SavePng),
                ("Next program (.nex)\u{2026}", "Shows the picture until the Next is reset", Asked::SaveNex),
                ("Save everything\u{2026}", "Into a folder: the .nxi, pixels, palette, BMP, PNG, the sjasmplus source and the .nex", Asked::SaveAll),
                ("Close", "", Asked::Close),
            ] {
                let button = ui.button(crate::i18n::tr(label));
                let button = if tip.is_empty() { button } else { button.on_hover_text(crate::i18n::tr(tip)) };
                if button.clicked() {
                    asked = Some(what);
                }
            }
        });
        asked
    }

    /// The screen drawn and drawn on; where the mouse is over it.
    fn canvas(&mut self, ui: &mut Ui, scale: egui::Vec2) -> Option<(usize, usize)> {
        let (w, h) = self.screen.mode.size();
        let (rect, response) = ui.allocate_exact_size(vec2(w as f32 * scale.x, h as f32 * scale.y), Sense::click_and_drag());
        let to_pixel = |p: egui::Pos2| (((p.x - rect.min.x) / scale.x).floor() as i32, ((p.y - rect.min.y) / scale.y).floor() as i32);
        let inside = |(x, y): (i32, i32)| (0..w as i32).contains(&x) && (0..h as i32).contains(&y);
        let (primary, secondary, shift) = ui.input(|i| (i.pointer.primary_down(), i.pointer.secondary_down(), i.modifiers.shift));
        let pointer = response.interact_pointer_pos().or_else(|| response.hover_pos()).map(to_pixel);
        let clicked = response.clicked() || response.secondary_clicked();
        let right_click = response.secondary_clicked();
        let shapes = matches!(self.tool, Tool::Line | Tool::Rectangle | Tool::FilledRectangle | Tool::Ellipse | Tool::FilledEllipse);

        if self.pasting {
            if clicked && let Some(p) = pointer.filter(|p| inside(*p)) {
                self.paste_at(p.0, p.1);
                self.pasting = false;
            }
        } else {
            match self.tool {
                Tool::Pencil => {
                    if let Some(p) = pointer.filter(|_| response.is_pointer_button_down_on() && (primary || secondary)) {
                        let from = match self.stroke {
                            Some(last) => last,
                            None => {
                                self.remember();
                                p
                            }
                        };
                        let mut screen = self.screen.clone();
                        let value = self.colour(!primary);
                        for (x, y) in crate::screen_designer::line_points(from, p) {
                            self.dab(&mut screen, x, y, value);
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
                        self.drag = Some(Drag { from: p, to: p, right: secondary });
                    }
                    if let (Some(drag), Some(p)) = (self.drag.as_mut(), pointer) {
                        drag.to = p;
                    }
                    let released = self.drag.is_some() && !response.is_pointer_button_down_on() && !primary && !secondary;
                    if released && let Some(drag) = self.drag.take() {
                        if self.tool == Tool::Select {
                            let clamp = |(x, y): (i32, i32)| (x.clamp(0, w as i32 - 1) as usize, y.clamp(0, h as i32 - 1) as usize);
                            let ((ax, ay), (bx, by)) = (clamp(drag.from), clamp(drag.to));
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
                        let value = self.colour(right_click);
                        self.fill(x as usize, y as usize, value);
                    }
                }
                Tool::Text => {
                    if clicked && let Some((x, y)) = pointer.filter(|p| inside(*p)) {
                        self.remember();
                        self.write(x, y, right_click);
                    }
                }
                Tool::Picker => {
                    if clicked && let Some((x, y)) = pointer.filter(|p| inside(*p)) {
                        let index = self.screen.pixels[y as usize * w + x as usize];
                        if right_click {
                            self.right = index;
                        } else {
                            self.left = index;
                        }
                    }
                }
                _ => {}
            }
        }

        // The screen as a picture, with a shape being dragged shown on it.
        let preview = match self.drag {
            Some(drag) if shapes => {
                let mut s = self.screen.clone();
                self.shape(&mut s, drag, shift);
                s
            }
            _ => self.screen.clone(),
        };
        if self.shown.as_ref() != Some(&preview) || self.texture.is_none() {
            let image = egui::ColorImage::from_rgba_unmultiplied([w, h], &preview.rgba());
            let options = egui::TextureOptions::NEAREST;
            match &mut self.texture {
                Some(texture) => texture.set(image, options),
                None => self.texture = Some(ui.ctx().load_texture("next-painter", image, options)),
            }
            self.shown = Some(preview);
        }
        let painter = ui.painter_at(rect);
        if let Some(texture) = &self.texture {
            painter.image(texture.id(), rect, Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)), Color32::WHITE);
        }
        if self.grid > 0 && scale.y * self.grid as f32 >= 6.0 {
            let stroke = egui::Stroke::new(0.5, Color32::from_white_alpha(70));
            for i in (0..=w).step_by(self.grid) {
                let x = rect.min.x + i as f32 * scale.x;
                painter.line_segment([pos2(x, rect.min.y), pos2(x, rect.max.y)], stroke);
            }
            for i in (0..=h).step_by(self.grid) {
                let y = rect.min.y + i as f32 * scale.y;
                painter.line_segment([pos2(rect.min.x, y), pos2(rect.max.x, y)], stroke);
            }
        }
        let area_rect = |(x, y, aw, ah): Area| Rect::from_min_size(rect.min + vec2(x as f32 * scale.x, y as f32 * scale.y), vec2(aw as f32 * scale.x, ah as f32 * scale.y));
        let marking = egui::Stroke::new(2.0, crate::theme::accent());
        if let Some(area) = self.selection {
            painter.rect_stroke(area_rect(area), 0.0, marking, egui::StrokeKind::Outside);
        }
        if let (Tool::Select, Some(drag)) = (self.tool, self.drag) {
            let (ax, ay) = (drag.from.0.min(drag.to.0).max(0) as usize, drag.from.1.min(drag.to.1).max(0) as usize);
            let (bx, by) = (drag.from.0.max(drag.to.0).max(0) as usize, drag.from.1.max(drag.to.1).max(0) as usize);
            painter.rect_stroke(area_rect((ax, ay, bx - ax + 1, by - ay + 1)), 0.0, marking, egui::StrokeKind::Outside);
        }
        let hover = response.hover_pos().map(to_pixel).filter(|p| inside(*p));
        if let Some((x, y)) = hover {
            if self.pasting && let Some(clip) = &self.clipboard {
                painter.rect_stroke(area_rect((x as usize, y as usize, clip.w, clip.h)), 0.0, marking, egui::StrokeKind::Outside);
            } else if self.tool == Tool::Text {
                let size = if self.big_text { 16 } else { 8 };
                painter.rect_stroke(area_rect((x as usize, y as usize, self.text.chars().count() * size, size)), 0.0, marking, egui::StrokeKind::Outside);
            }
        }
        hover.map(|(x, y)| (x as usize, y as usize))
    }
}

#[cfg(test)]
mod tests;
