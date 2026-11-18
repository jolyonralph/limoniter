//! The Atari 2600 graphics editor: player sprites (8 pixels across, a colour a line, frames played as an
//! animation, as wide as NUSIZ makes them) and the playfield (40 blocks across, mirrored, repeated or two halves,
//! rows of any number of lines with a colour and background each), drawn the shape the TV shows them in the TV
//! system's colours. Sprites can be taken from a cartridge's bytes, and a picture made into a playfield. What's
//! drawn goes out as DASM source, as bytes, and as a whole program that shows it. The model is in
//! `atari2600_gfx`.

use crate::atari2600_gfx::{Frame, Halves, MOST_LINES, Order, Playfield, Size, Sprites, Tv};
use eframe::egui::{self, Color32, Rect, Sense, Ui, pos2, vec2};

/// Which of the two is being drawn.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Part {
    Sprites,
    Playfield,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tool {
    Pencil,
    Line,
    Box,
    FilledBox,
    Fill,
}

impl Tool {
    const ALL: [(Tool, &'static str, &'static str); 5] = [
        (Tool::Pencil, "Pencil", "The left button draws, the right rubs out"),
        (Tool::Line, "Line", "Drag from one end to the other"),
        (Tool::Box, "Box", "Drag from corner to corner"),
        (Tool::FilledBox, "Filled box", "Drag from corner to corner"),
        (Tool::Fill, "Fill", "Fills the pixels joined to the one clicked that are as it is"),
    ];
}

/// What the editor asks the window to do.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Asked {
    /// Source to read the sprites or playfield from.
    Open,
    /// A cartridge to take sprites from.
    OpenCartridge,
    /// A picture to make into the playfield.
    BringIn,
    /// The source put in the document at the cursor.
    Insert,
    /// The document's selection read as sprites or a playfield.
    FromSelection,
    SaveSource,
    SaveBytes,
    /// The program that shows them, in a new tab for F9.
    OpenProgram,
    SaveCartridge,
    Close,
}

/// A cartridge's bytes, looked through for sprites.
pub struct Rip {
    pub name: String,
    pub bytes: Vec<u8>,
    pub offset: usize,
    /// Frames taken at once.
    pub count: usize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Drag {
    from: (i32, i32),
    to: (i32, i32),
    on: bool,
}

pub struct Editor {
    pub part: Part,
    pub tv: Tv,
    pub sprites: Sprites,
    /// The frame being drawn.
    pub frame: usize,
    pub field: Playfield,
    pub tool: Tool,
    /// The colour chosen, a TIA value.
    pub colour: u8,
    pub order: Order,
    /// What the tables are called.
    pub label: String,
    /// Bringing a picture in gives every row one background.
    pub one_background: bool,
    pub fit: crate::quantise::Fit,
    pub rip: Option<Rip>,
    /// A frame copied.
    pub copied: Option<Frame>,
    undo: Vec<(Sprites, Playfield)>,
    redo: Vec<(Sprites, Playfield)>,
    stroke: Option<(i32, i32)>,
    drag: Option<Drag>,
    /// Painting colours down a column of swatches: which column, the line the mouse was last on, and whether
    /// what was there before is kept to undo.
    swatching: Option<(usize, Option<usize>, bool)>,
    texture: Option<egui::TextureHandle>,
    shown: Vec<u8>,
    /// The size of a cell on the screen as last drawn.
    cell: egui::Vec2,
}

impl Default for Editor {
    fn default() -> Self {
        Editor {
            part: Part::Sprites,
            tv: Tv::Ntsc,
            sprites: Sprites::default(),
            frame: 0,
            field: Playfield::default(),
            tool: Tool::Pencil,
            colour: 0x1E,
            order: Order::TopFirst,
            label: "graphics".into(),
            one_background: false,
            fit: crate::quantise::Fit::Fill,
            rip: None,
            copied: None,
            undo: Vec::new(),
            redo: Vec::new(),
            stroke: None,
            drag: None,
            swatching: None,
            texture: None,
            shown: Vec::new(),
            cell: vec2(1.0, 1.0),
        }
    }
}

fn rgb32(tv: Tv, value: u8) -> Color32 {
    let [r, g, b] = tv.rgb(value);
    Color32::from_rgb(r, g, b)
}

/// The cells a drag of a tool covers.
fn shape_points(tool: Tool, from: (i32, i32), to: (i32, i32)) -> Vec<(i32, i32)> {
    let (left, right, top, bottom) = (from.0.min(to.0), from.0.max(to.0), from.1.min(to.1), from.1.max(to.1));
    match tool {
        Tool::Line => crate::screen_designer::line_points(from, to),
        Tool::Box => (left..=right).flat_map(|x| [(x, top), (x, bottom)]).chain((top..=bottom).flat_map(|y| [(left, y), (right, y)])).collect(),
        Tool::FilledBox => (top..=bottom).flat_map(|y| (left..=right).map(move |x| (x, y))).collect(),
        _ => Vec::new(),
    }
}

impl Editor {
    pub fn remember(&mut self) {
        self.undo.push((self.sprites.clone(), self.field.clone()));
        if self.undo.len() > 60 {
            self.undo.remove(0);
        }
        self.redo.clear();
    }

    pub fn undo(&mut self) {
        if let Some((sprites, field)) = self.undo.pop() {
            self.redo.push((std::mem::replace(&mut self.sprites, sprites), std::mem::replace(&mut self.field, field)));
            self.frame = self.frame.min(self.sprites.frames.len() - 1);
        }
    }

    pub fn redo(&mut self) {
        if let Some((sprites, field)) = self.redo.pop() {
            self.undo.push((std::mem::replace(&mut self.sprites, sprites), std::mem::replace(&mut self.field, field)));
            self.frame = self.frame.min(self.sprites.frames.len() - 1);
        }
    }

    /// The grid being drawn on: its size in cells.
    pub fn size(&self) -> (usize, usize) {
        match self.part {
            Part::Sprites => (8, self.sprites.height),
            Part::Playfield => (40, self.field.rows),
        }
    }

    pub fn get(&self, x: usize, y: usize) -> bool {
        match self.part {
            Part::Sprites => self.sprites.pixel(self.frame, x, y),
            Part::Playfield => self.field.pixel(x, y),
        }
    }

    pub fn put(&mut self, x: i32, y: i32, on: bool) {
        let (w, h) = self.size();
        if x < 0 || y < 0 || x as usize >= w || y as usize >= h {
            return;
        }
        match self.part {
            Part::Sprites => self.sprites.set(self.frame, x as usize, y as usize, on),
            Part::Playfield => self.field.set(x as usize, y as usize, on),
        }
    }

    /// The cells joined to one that are as it is, made `on`.
    pub fn fill(&mut self, x: usize, y: usize, on: bool) {
        let (w, h) = self.size();
        let target = self.get(x, y);
        if target == on {
            return;
        }
        let mut seen = vec![false; w * h];
        let mut todo = vec![(x, y)];
        while let Some((x, y)) = todo.pop() {
            if seen[y * w + x] || self.get(x, y) != target {
                continue;
            }
            seen[y * w + x] = true;
            self.put(x as i32, y as i32, on);
            if x > 0 {
                todo.push((x - 1, y));
            }
            if x + 1 < w {
                todo.push((x + 1, y));
            }
            if y > 0 {
                todo.push((x, y - 1));
            }
            if y + 1 < h {
                todo.push((x, y + 1));
            }
        }
    }

    /// Every cell moved through a change of where it is (or made the other way, for invert), the grid's size the
    /// same: `from(x, y)` gives the cell each takes its state from, None to be off.
    fn rearrange(&mut self, from: impl Fn(usize, usize, usize, usize) -> Option<(usize, usize)>, invert: bool) {
        self.remember();
        let (w, h) = self.size();
        let before: Vec<bool> = (0..w * h).map(|i| self.get(i % w, i / w)).collect();
        // The playfield's halves are kept by drawing the left half (or all of it when they're apart).
        let across = if self.part == Part::Playfield && self.field.halves != Halves::Asymmetric { 20 } else { w };
        for y in 0..h {
            for x in 0..across {
                let on = from(x, y, w, h).is_some_and(|(fx, fy)| before[fy * w + fx]) != invert;
                self.put(x as i32, y as i32, on);
            }
        }
    }

    pub fn flip_across(&mut self) {
        self.rearrange(|x, y, w, _| Some((w - 1 - x, y)), false);
    }

    pub fn flip_down(&mut self) {
        self.rearrange(|x, y, _, h| Some((x, h - 1 - y)), false);
        if self.part == Part::Sprites {
            self.sprites.frames[self.frame].colours.reverse();
        } else {
            self.field.colours.reverse();
            self.field.backgrounds.reverse();
        }
    }

    pub fn invert(&mut self) {
        self.rearrange(|x, y, _, _| Some((x, y)), true);
    }

    /// Moved a cell, round to the other side.
    pub fn shift(&mut self, dx: i32, dy: i32) {
        self.rearrange(move |x, y, w, h| Some(((x as i32 - dx).rem_euclid(w as i32) as usize, (y as i32 - dy).rem_euclid(h as i32) as usize)), false);
    }

    pub fn clear(&mut self) {
        self.rearrange(|_, _, _, _| None, false);
    }

    /// The frames taken from the cartridge where it's being looked at.
    pub fn take_frames(&mut self) {
        let Some(rip) = &self.rip else { return };
        let height = self.sprites.height;
        let colours = self.sprites.frames[self.frame].colours.clone();
        let frames: Vec<Frame> = (0..rip.count.max(1))
            .map(|n| {
                let start = rip.offset + n * height;
                let mut rows: Vec<u8> = rip.bytes.iter().skip(start).take(height).copied().collect();
                rows.resize(height, 0);
                if self.order == Order::BottomFirst {
                    rows.reverse();
                }
                Frame { rows, colours: colours.clone() }
            })
            .collect();
        self.remember();
        self.sprites.frames = frames;
        self.frame = 0;
    }

    pub fn source(&self) -> String {
        let label = crate::quantise::identifier(&self.label);
        match self.part {
            Part::Sprites => self.sprites.source(&label, self.order),
            Part::Playfield => self.field.source(&label, self.order),
        }
    }

    pub fn bytes(&self) -> Vec<u8> {
        match self.part {
            Part::Sprites => self.sprites.graphics_bytes(self.order).into_iter().chain(self.sprites.colour_bytes(self.order)).collect(),
            Part::Playfield => self.field.bytes(self.order),
        }
    }

    pub fn program(&self) -> String {
        let label = crate::quantise::identifier(&self.label);
        match self.part {
            Part::Sprites => self.sprites.program(self.tv, &label),
            Part::Playfield => self.field.program(self.tv, &label),
        }
    }

    /// Source read into the part being drawn; whether it held any.
    pub fn read_source(&mut self, text: &str) -> bool {
        match self.part {
            Part::Sprites => match Sprites::from_source(text, self.sprites.height) {
                Some(read) => {
                    self.remember();
                    let Sprites { background, size, lines, speed, .. } = self.sprites;
                    self.sprites = Sprites { background, size, lines, speed, ..read };
                    self.frame = 0;
                    true
                }
                None => false,
            },
            Part::Playfield => match Playfield::from_source(text) {
                Some(read) => {
                    self.remember();
                    self.field = read;
                    true
                }
                None => false,
            },
        }
    }

    pub fn draw(&mut self, ui: &mut Ui) -> Option<Asked> {
        let mut asked = None;
        ui.horizontal(|ui| {
            ui.selectable_value(&mut self.part, Part::Sprites, crate::i18n::tr("Player sprites"));
            ui.selectable_value(&mut self.part, Part::Playfield, crate::i18n::tr("Playfield"));
            ui.separator();
            ui.label(crate::i18n::tr("TV"));
            egui::ComboBox::from_id_salt("vcs-tv").width(70.0).selected_text(crate::i18n::tr(self.tv.name())).show_ui(ui, |ui| {
                for tv in Tv::ALL {
                    ui.selectable_value(&mut self.tv, tv, crate::i18n::tr(tv.name()));
                }
            });
            ui.label(crate::i18n::tr("Label"));
            ui.add(egui::TextEdit::singleline(&mut self.label).desired_width(110.0).font(egui::TextStyle::Monospace));
            egui::ComboBox::from_id_salt("vcs-order").selected_text(crate::i18n::tr(self.order.name())).show_ui(ui, |ui| {
                for order in [Order::TopFirst, Order::BottomFirst] {
                    ui.selectable_value(&mut self.order, order, crate::i18n::tr(order.name())).on_hover_text(if order == Order::BottomFirst { crate::i18n::tr("For kernels that count Y down to 0") } else { crate::i18n::tr("") });
                }
            });
        });
        ui.separator();
        ui.horizontal_top(|ui| {
            ui.vertical(|ui| {
                ui.set_width(250.0);
                // On a small screen, the tools scroll rather than push the buttons below off its bottom.
                let most = (ui.ctx().content_rect().height() - 250.0).max(240.0);
                egui::ScrollArea::vertical().id_salt("vcs-controls").max_height(most).show(ui, |ui| {
                    if let Some(a) = self.controls(ui) {
                        asked = Some(a);
                    }
                });
            });
            // Spaces, not separators, which would reach to the screen's bottom in a window that sizes itself.
            ui.add_space(10.0);
            ui.vertical(|ui| {
                let hovered = self.canvas(ui);
                let status = match hovered {
                    Some((x, y)) => self.describe(x, y),
                    None => String::new(),
                };
                ui.label(egui::RichText::new(status).monospace().size(11.0).color(crate::theme::text_dim()));
            });
            if self.part == Part::Sprites {
                ui.add_space(10.0);
                ui.vertical(|ui| self.sprite_side(ui));
            }
        });
        ui.separator();
        ui.horizontal_wrapped(|ui| {
            let mut buttons: Vec<(&str, &str, Asked)> = vec![
                ("Open source\u{2026}", "DASM source with .byte tables, as these are saved", Asked::Open),
                ("Read the selection", "The tables selected in the document", Asked::FromSelection),
                ("Put in the source", "These tables at the document's cursor", Asked::Insert),
                ("Save source\u{2026}", "", Asked::SaveSource),
                ("Save bytes\u{2026}", "The tables one after another, for INCBIN", Asked::SaveBytes),
                ("Program to run", "A whole program that shows them, in a new tab: F9 runs it in Stella or MAME", Asked::OpenProgram),
                ("Save cartridge\u{2026}", "That program built into a 4K cartridge", Asked::SaveCartridge),
            ];
            if self.part == Part::Sprites {
                buttons.insert(2, ("From a cartridge\u{2026}", "Look through a cartridge's bytes for its sprites", Asked::OpenCartridge));
            } else {
                buttons.insert(2, ("Bring in a picture\u{2026}", "A JPEG, PNG or any picture, made into this playfield's rows", Asked::BringIn));
            }
            buttons.push(("Close", "", Asked::Close));
            for (label, tip, what) in buttons {
                let button = ui.button(crate::i18n::tr(label));
                let button = if tip.is_empty() { button } else { button.on_hover_text(crate::i18n::tr(tip)) };
                if button.clicked() {
                    asked = Some(what);
                }
            }
        });
        asked
    }

    /// What the cell under the mouse is in the registers.
    fn describe(&self, x: usize, y: usize) -> String {
        match self.part {
            Part::Sprites => {
                let f = &self.sprites.frames[self.frame];
                crate::i18n::tf!("pixel {x}, line {y}: %{:08b} (${:02X}) \u{B7} colour ${:02X}", f.rows[y], f.rows[y], f.colours[y])
            }
            Part::Playfield => {
                let left = self.field.halves.source_of(x);
                let half = if x >= 20 && self.field.halves == Halves::Asymmetric { "right " } else { "" };
                let (register, bit) = match left % 20 {
                    b @ 0..4 => ("PF0", b + 4),
                    b @ 4..12 => ("PF1", 11 - b),
                    b => ("PF2", b - 12),
                };
                let first = self.field.top(self.tv) + y * self.field.lines;
                crate::i18n::tf!("block {x}, row {y} (lines {first} to {}): {half}{register} bit {bit} \u{B7} colour ${:02X}, background ${:02X}", first + self.field.lines - 1, self.field.colours[y], self.field.backgrounds[y])
            }
        }
    }

    fn controls(&mut self, ui: &mut Ui) -> Option<Asked> {
        egui::Grid::new("vcs-tools").num_columns(3).spacing([4.0, 4.0]).show(ui, |ui| {
            for (i, (tool, name, tip)) in Tool::ALL.into_iter().enumerate() {
                if ui.selectable_label(self.tool == tool, crate::i18n::tr(name)).on_hover_text(crate::i18n::tr(tip)).clicked() {
                    self.tool = tool;
                }
                if i % 3 == 2 {
                    ui.end_row();
                }
            }
        });
        ui.horizontal_wrapped(|ui| {
            if ui.add_enabled(!self.undo.is_empty(), egui::Button::new(crate::i18n::tr("Undo"))).clicked() {
                self.undo();
            }
            if ui.add_enabled(!self.redo.is_empty(), egui::Button::new(crate::i18n::tr("Redo"))).clicked() {
                self.redo();
            }
            if ui.small_button(crate::i18n::tr("Flip \u{2194}")).clicked() {
                self.flip_across();
            }
            if ui.small_button(crate::i18n::tr("Flip \u{2195}")).clicked() {
                self.flip_down();
            }
            if ui.small_button(crate::i18n::tr("Invert")).clicked() {
                self.invert();
            }
            if ui.small_button(crate::i18n::tr("Clear")).clicked() {
                self.clear();
            }
        });
        ui.horizontal(|ui| {
            ui.label(crate::i18n::tr("Move"));
            for (arrow, dx, dy) in [("\u{2190}", -1, 0), ("\u{2192}", 1, 0), ("\u{2191}", 0, -1), ("\u{2193}", 0, 1)] {
                if ui.small_button(arrow).clicked() {
                    self.shift(dx, dy);
                }
            }
        });
        ui.separator();
        self.palette_ui(ui);
        ui.separator();
        match self.part {
            Part::Sprites => self.sprite_controls(ui),
            Part::Playfield => self.playfield_controls(ui),
        }
        None
    }

    /// The TV's colours: hue down, brightness across.
    fn palette_ui(&mut self, ui: &mut Ui) {
        let size = 13.0;
        let accent = crate::theme::accent();
        ui.spacing_mut().item_spacing = vec2(1.0, 1.0);
        for hue in 0..16u8 {
            ui.horizontal(|ui| {
                for lum in 0..8u8 {
                    let value = hue << 4 | lum << 1;
                    let (rect, response) = ui.allocate_exact_size(vec2(size * 1.6, size), Sense::click());
                    ui.painter().rect_filled(rect, 0.0, rgb32(self.tv, value));
                    if self.colour == value {
                        ui.painter().rect_stroke(rect, 0.0, egui::Stroke::new(2.0, accent), egui::StrokeKind::Inside);
                    }
                    if response.on_hover_text(crate::i18n::tf!("${value:02X}")).clicked() {
                        self.colour = value;
                    }
                }
            });
        }
        ui.spacing_mut().item_spacing = vec2(8.0, 4.0);
        ui.label(crate::i18n::tf!("Colour ${:02X}: click a line's colour to give it this; the right button takes a colour", self.colour));
    }

    fn sprite_controls(&mut self, ui: &mut Ui) {
        let mut height = self.sprites.height;
        ui.add(egui::Slider::new(&mut height, 1..=MOST_LINES).text(crate::i18n::tr("lines tall")));
        if height != self.sprites.height {
            if self.undo.last().is_none_or(|(s, _)| s.height == self.sprites.height && s.frames == self.sprites.frames) {
                self.remember();
            }
            self.sprites.set_height(height);
        }
        ui.add(egui::Slider::new(&mut self.sprites.lines, 1..=4).text(crate::i18n::tr("TV lines a line"))).on_hover_text(crate::i18n::tr("2 for a two-line kernel"));
        ui.horizontal(|ui| {
            ui.label(crate::i18n::tr("Width"));
            egui::ComboBox::from_id_salt("vcs-size").selected_text(crate::i18n::tr(self.sprites.size.name())).show_ui(ui, |ui| {
                for size in Size::ALL {
                    ui.selectable_value(&mut self.sprites.size, size, crate::i18n::tr(size.name()));
                }
            });
        });
        ui.add(egui::Slider::new(&mut self.sprites.speed, 0..=60).text(crate::i18n::tr("TV frames a frame"))).on_hover_text(crate::i18n::tr("How fast the frames play; 0 stops them"));
        ui.horizontal_wrapped(|ui| {
            if ui.small_button(crate::i18n::tr("Every line this colour")).clicked() {
                self.remember();
                let colour = self.colour;
                self.sprites.frames[self.frame].colours.iter_mut().for_each(|c| *c = colour);
            }
            if ui.small_button(crate::i18n::tr("These colours for every frame")).clicked() {
                self.remember();
                let colours = self.sprites.frames[self.frame].colours.clone();
                self.sprites.frames.iter_mut().for_each(|f| f.colours = colours.clone());
            }
            if ui.small_button(crate::i18n::tr("Background this colour")).clicked() {
                self.sprites.background = self.colour;
            }
        });
    }

    fn playfield_controls(&mut self, ui: &mut Ui) {
        ui.horizontal(|ui| {
            ui.label(crate::i18n::tr("Halves"));
            let mut halves = self.field.halves;
            egui::ComboBox::from_id_salt("vcs-halves").selected_text(crate::i18n::tr(halves.name())).show_ui(ui, |ui| {
                for h in Halves::ALL {
                    ui.selectable_value(&mut halves, h, crate::i18n::tr(h.name()));
                }
            });
            if halves != self.field.halves {
                self.remember();
                self.field.set_halves(halves);
            }
        });
        let mut lines = self.field.lines;
        ui.add(egui::Slider::new(&mut lines, 1..=16).text(crate::i18n::tr("TV lines a row")));
        let mut rows = self.field.rows;
        let most = self.tv.lines() / lines;
        ui.add(egui::Slider::new(&mut rows, 1..=most).text(crate::i18n::tr("rows")));
        let rows = rows.min(most);
        if lines != self.field.lines || rows != self.field.rows {
            if self.undo.last().is_none_or(|(_, f)| f.rows == self.field.rows && f.lines == self.field.lines && f.pixels == self.field.pixels) {
                self.remember();
            }
            self.field.lines = lines;
            self.field.set_rows(rows);
        }
        ui.label(crate::i18n::tf!("{} lines of the {} a frame shows", self.field.rows * self.field.lines, self.tv.lines()));
        ui.horizontal_wrapped(|ui| {
            if ui.small_button(crate::i18n::tr("Every row this colour")).clicked() {
                self.remember();
                let colour = self.colour;
                self.field.colours.iter_mut().for_each(|c| *c = colour);
            }
            if ui.small_button(crate::i18n::tr("Every background this colour")).clicked() {
                self.remember();
                let colour = self.colour;
                self.field.backgrounds.iter_mut().for_each(|c| *c = colour);
            }
        });
        ui.separator();
        ui.label(crate::i18n::tr("Bringing in a picture"));
        ui.checkbox(&mut self.one_background, crate::i18n::tr("One background for every row"));
        egui::ComboBox::from_id_salt("vcs-fit").selected_text(crate::i18n::tr(self.fit.name())).show_ui(ui, |ui| {
            for fit in crate::quantise::Fit::ALL {
                ui.selectable_value(&mut self.fit, fit, crate::i18n::tr(fit.name()));
            }
        });
    }

    /// The frames, the animation playing, and a cartridge being looked through.
    fn sprite_side(&mut self, ui: &mut Ui) {
        let tv = self.tv;
        let frames = self.sprites.frames.len();
        ui.label(crate::i18n::tf!("Frame {} of {frames}", self.frame + 1));
        ui.horizontal_wrapped(|ui| {
            ui.set_max_width(200.0);
            for n in 0..frames {
                let (rect, response) = ui.allocate_exact_size(vec2(8.0 * 3.0, (self.sprites.height as f32 * 1.5).clamp(12.0, 64.0)), Sense::click());
                self.paint_frame(ui, rect, n);
                if n == self.frame {
                    ui.painter().rect_stroke(rect, 0.0, egui::Stroke::new(2.0, crate::theme::accent()), egui::StrokeKind::Outside);
                }
                if response.clicked() {
                    self.frame = n;
                }
            }
        });
        ui.horizontal_wrapped(|ui| {
            ui.set_max_width(200.0);
            if ui.small_button(crate::i18n::tr("New")).clicked() {
                self.remember();
                self.sprites.frames.insert(self.frame + 1, Sprites::blank(self.sprites.height));
                self.frame += 1;
            }
            if ui.small_button(crate::i18n::tr("Copy of this")).clicked() {
                self.remember();
                let copy = self.sprites.frames[self.frame].clone();
                self.sprites.frames.insert(self.frame + 1, copy);
                self.frame += 1;
            }
            if ui.add_enabled(frames > 1, egui::Button::new(crate::i18n::tr("Delete")).small()).clicked() {
                self.remember();
                self.sprites.frames.remove(self.frame);
                self.frame = self.frame.min(self.sprites.frames.len() - 1);
            }
            if ui.add_enabled(self.frame > 0, egui::Button::new(crate::i18n::tr("\u{2190}")).small()).on_hover_text(crate::i18n::tr("Earlier")).clicked() {
                self.sprites.frames.swap(self.frame, self.frame - 1);
                self.frame -= 1;
            }
            if ui.add_enabled(self.frame + 1 < frames, egui::Button::new(crate::i18n::tr("\u{2192}")).small()).on_hover_text(crate::i18n::tr("Later")).clicked() {
                self.sprites.frames.swap(self.frame, self.frame + 1);
                self.frame += 1;
            }
            if ui.small_button(crate::i18n::tr("Copy")).clicked() {
                self.copied = Some(self.sprites.frames[self.frame].clone());
            }
            if ui.add_enabled(self.copied.is_some(), egui::Button::new(crate::i18n::tr("Paste")).small()).on_hover_text(crate::i18n::tr("Over this frame")).clicked()
                && let Some(mut frame) = self.copied.clone()
            {
                self.remember();
                let last = frame.colours.last().copied().unwrap_or(0x0E);
                frame.rows.resize(self.sprites.height, 0);
                frame.colours.resize(self.sprites.height, last);
                self.sprites.frames[self.frame] = frame;
            }
        });
        ui.separator();
        // The frames played as the TV would, at twice the size.
        let shown = if self.sprites.speed == 0 || frames == 1 {
            self.frame
        } else {
            let hz = if tv == Tv::Ntsc { 60.0 } else { 50.0 };
            let tick = (ui.input(|i| i.time) * hz) as usize / self.sprites.speed;
            ui.ctx().request_repaint_after(std::time::Duration::from_millis(20));
            tick % frames
        };
        let clocks = self.sprites.size.clocks();
        let lines = self.sprites.lines;
        let unit = 2.0;
        let (w, h) = ((8 * clocks + 16) as f32 * tv.aspect() * unit, (self.sprites.height * lines + 16) as f32 * unit);
        let (rect, _) = ui.allocate_exact_size(vec2(w.max(60.0), h), Sense::hover());
        ui.painter().rect_filled(rect, 0.0, rgb32(tv, self.sprites.background));
        let picture = self.sprites.picture(shown, 8 * clocks);
        let (cw, ch) = (tv.aspect() * unit, unit);
        let origin = rect.min + vec2(8.0 * cw, 8.0 * ch);
        for (i, value) in picture.iter().enumerate() {
            if let Some(v) = value {
                let (x, y) = (i % (8 * clocks), i / (8 * clocks));
                ui.painter().rect_filled(Rect::from_min_size(origin + vec2(x as f32 * cw, y as f32 * ch), vec2(cw, ch)), 0.0, rgb32(tv, *v));
            }
        }
        ui.label(egui::RichText::new(crate::i18n::tf!("As the TV shows it ({})", tv.name())).size(11.0).color(crate::theme::text_dim()));
        if self.rip.is_some() {
            ui.separator();
            self.rip_ui(ui);
        }
    }

    /// A frame drawn small.
    fn paint_frame(&self, ui: &Ui, rect: Rect, n: usize) {
        let painter = ui.painter_at(rect);
        painter.rect_filled(rect, 0.0, rgb32(self.tv, self.sprites.background));
        let frame = &self.sprites.frames[n];
        let (cw, ch) = (rect.width() / 8.0, rect.height() / self.sprites.height as f32);
        for (y, row) in frame.rows.iter().enumerate() {
            for x in 0..8 {
                if row & (0x80 >> x) != 0 {
                    painter.rect_filled(Rect::from_min_size(rect.min + vec2(x as f32 * cw, y as f32 * ch), vec2(cw, ch.max(1.0))), 0.0, rgb32(self.tv, frame.colours[y]));
                }
            }
        }
    }

    /// A cartridge's bytes as rows of 8 pixels, from where it's being looked at, the frames that would be taken
    /// marked.
    fn rip_ui(&mut self, ui: &mut Ui) {
        let height = self.sprites.height;
        let Some(rip) = self.rip.as_mut() else { return };
        ui.label(crate::i18n::tf!("{} ({} bytes)", rip.name, rip.bytes.len()));
        let last = rip.bytes.len().saturating_sub(1);
        ui.horizontal(|ui| {
            ui.label(crate::i18n::tr("From"));
            ui.add(egui::DragValue::new(&mut rip.offset).range(0..=last).hexadecimal(4, false, true).prefix("$"));
        });
        ui.horizontal(|ui| {
            for (label, step) in [("\u{2191} 1", -1i64), ("\u{2193} 1", 1), ("\u{2191} frame", -(height as i64)), ("\u{2193} frame", height as i64)] {
                if ui.small_button(crate::i18n::tr(label)).clicked() {
                    rip.offset = (rip.offset as i64 + step).clamp(0, last as i64) as usize;
                }
            }
        });
        ui.add(egui::Slider::new(&mut rip.count, 1..=16).text(crate::i18n::tr("frames")));
        let shown = 96usize;
        let (cw, ch) = (3.0 * self.tv.aspect(), 3.0);
        let (rect, response) = ui.allocate_exact_size(vec2(8.0 * cw + 4.0, shown as f32 * ch), Sense::click_and_drag());
        let painter = ui.painter_at(rect);
        painter.rect_filled(rect, 0.0, Color32::from_gray(20));
        for i in 0..shown {
            let Some(&byte) = rip.bytes.get(rip.offset + i) else { break };
            let taking = i < height * rip.count;
            let ink = if taking { Color32::from_rgb(240, 220, 120) } else { Color32::from_gray(150) };
            for x in 0..8 {
                if byte & (0x80 >> x) != 0 {
                    painter.rect_filled(Rect::from_min_size(rect.min + vec2(2.0 + x as f32 * cw, i as f32 * ch), vec2(cw, ch)), 0.0, ink);
                }
            }
            if taking && i % height == 0 {
                painter.line_segment([pos2(rect.min.x, rect.min.y + i as f32 * ch), pos2(rect.max.x, rect.min.y + i as f32 * ch)], egui::Stroke::new(1.0, crate::theme::accent()));
            }
        }
        // Dragging the strip moves through the bytes.
        if response.dragged() {
            let moved = (-response.drag_delta().y / ch).round() as i64;
            rip.offset = (rip.offset as i64 + moved).clamp(0, last as i64) as usize;
        }
        let scrolled = if response.hovered() { ui.input(|i| i.smooth_scroll_delta.y) } else { 0.0 };
        if scrolled != 0.0 {
            rip.offset = (rip.offset as i64 - (scrolled / ch).round() as i64).clamp(0, last as i64) as usize;
        }
        ui.label(egui::RichText::new(crate::i18n::tf!("${:04X} \u{B7} drag or scroll to move", rip.offset)).size(11.0).color(crate::theme::text_dim()));
        ui.horizontal(|ui| {
            if ui.button(crate::i18n::tr("Take these")).on_hover_text(crate::i18n::tr("The marked bytes as frames of this height, in this frame's colours")).clicked() {
                self.take_frames();
            }
            if ui.button(crate::i18n::tr("Done")).clicked() {
                self.rip = None;
            }
        });
    }

    /// The grid drawn and drawn on, the colours of each line beside it; where the mouse is over the grid.
    fn canvas(&mut self, ui: &mut Ui) -> Option<(usize, usize)> {
        let tv = self.tv;
        let (w, h) = self.size();
        // Each cell the shape the TV shows it: so many colour clocks across and lines down.
        let (clocks, lines) = match self.part {
            Part::Sprites => (self.sprites.size.clocks() as f32, self.sprites.lines as f32),
            Part::Playfield => (4.0, self.field.lines as f32),
        };
        let room = ui.ctx().content_rect().size() - vec2(620.0, 270.0);
        let (room_w, room_h) = (room.x.clamp(300.0, 700.0), room.y.clamp(200.0, 620.0));
        let unit = (room_w / (w as f32 * clocks * tv.aspect())).min(room_h / (h as f32 * lines)).clamp(0.5, 40.0);
        let (cw, ch) = (unit * clocks * tv.aspect(), unit * lines);
        self.cell = vec2(cw, ch);
        let swatches = if self.part == Part::Sprites { 1 } else { 2 };
        let swatch_w = 18.0;
        let (whole, response) = ui.allocate_exact_size(vec2(w as f32 * cw + 6.0 + swatches as f32 * (swatch_w + 2.0), h as f32 * ch), Sense::click_and_drag());
        let rect = Rect::from_min_size(whole.min, vec2(w as f32 * cw, h as f32 * ch));
        let to_cell = |p: egui::Pos2| (((p.x - rect.min.x) / cw).floor() as i32, ((p.y - rect.min.y) / ch).floor() as i32);
        let inside = |(x, y): (i32, i32)| (0..w as i32).contains(&x) && (0..h as i32).contains(&y);
        let (primary, secondary) = ui.input(|i| (i.pointer.primary_down(), i.pointer.secondary_down()));
        let pointer = response.interact_pointer_pos().or_else(|| response.hover_pos());
        let cell = pointer.map(to_cell);
        let down = response.is_pointer_button_down_on() && (primary || secondary);
        // Which column of swatches the mouse is over, if any.
        let swatch_column = pointer.and_then(|p| {
            let x = p.x - rect.max.x - 6.0;
            (x >= 0.0).then(|| (x / (swatch_w + 2.0)) as usize).filter(|&c| c < swatches)
        });

        if self.swatching.is_some() || (down && swatch_column.is_some() && self.drag.is_none() && self.stroke.is_none()) {
            // Colours given to lines (each line between where the mouse was and is), or taken from one.
            let (column, last, mut kept) = *self.swatching.get_or_insert((swatch_column.unwrap_or(0), None, false));
            if let Some((_, y)) = cell.filter(|&(_, y)| (0..h as i32).contains(&y)) {
                let y = y as usize;
                let from = last.unwrap_or(y);
                let before = (self.sprites.clone(), self.field.clone());
                let colours = match (self.part, column) {
                    (Part::Sprites, _) => &mut self.sprites.frames[self.frame].colours,
                    (Part::Playfield, 0) => &mut self.field.colours,
                    (Part::Playfield, _) => &mut self.field.backgrounds,
                };
                if secondary {
                    self.colour = colours[y];
                } else if primary {
                    colours[from.min(y)..=from.max(y)].fill(self.colour);
                    if !kept && (self.sprites.clone(), self.field.clone()) != before {
                        self.undo.push(before);
                        self.redo.clear();
                        kept = true;
                    }
                }
                self.swatching = Some((column, Some(y), kept));
            }
            if !down {
                self.swatching = None;
            }
        } else {
            match self.tool {
                Tool::Pencil => {
                    if let Some(p) = cell.filter(|_| down) {
                        let from = match self.stroke {
                            Some(last) => last,
                            None => {
                                self.remember();
                                p
                            }
                        };
                        for (x, y) in crate::screen_designer::line_points(from, p) {
                            self.put(x, y, primary);
                        }
                        self.stroke = Some(p);
                    } else {
                        self.stroke = None;
                    }
                }
                Tool::Fill => {
                    if (response.clicked() || response.secondary_clicked())
                        && let Some((x, y)) = cell.filter(|c| inside(*c))
                    {
                        self.remember();
                        self.fill(x as usize, y as usize, response.clicked());
                    }
                }
                _ => {
                    if self.drag.is_none() && down
                        && let Some(p) = cell
                    {
                        self.drag = Some(Drag { from: p, to: p, on: primary });
                    }
                    if let (Some(drag), Some(p)) = (self.drag.as_mut(), cell) {
                        drag.to = p;
                    }
                    if self.drag.is_some() && !response.is_pointer_button_down_on() && !primary && !secondary
                        && let Some(drag) = self.drag.take()
                    {
                        self.remember();
                        for (x, y) in shape_points(self.tool, drag.from, drag.to) {
                            self.put(x, y, drag.on);
                        }
                    }
                }
            }
        }

        // What's shown: the grid with a shape being dragged drawn on a copy of it.
        let (sprites, field) = (self.sprites.clone(), self.field.clone());
        if let Some(drag) = self.drag {
            for (x, y) in shape_points(self.tool, drag.from, drag.to) {
                self.put(x, y, drag.on);
            }
        }
        let mut rgba = Vec::with_capacity(w * h * 4);
        for y in 0..h {
            let (ink, paper) = match self.part {
                Part::Sprites => (self.sprites.frames[self.frame].colours[y], self.sprites.background),
                Part::Playfield => (self.field.colours[y], self.field.backgrounds[y]),
            };
            for x in 0..w {
                let [r, g, b] = tv.rgb(if self.get(x, y) { ink } else { paper });
                rgba.extend([r, g, b, 255]);
            }
        }
        self.sprites = sprites;
        self.field = field;
        if self.shown != rgba || self.texture.is_none() {
            let image = egui::ColorImage::from_rgba_unmultiplied([w, h], &rgba);
            match &mut self.texture {
                Some(texture) => texture.set(image, egui::TextureOptions::NEAREST),
                None => self.texture = Some(ui.ctx().load_texture("vcs-graphics", image, egui::TextureOptions::NEAREST)),
            }
            self.shown = rgba;
        }
        let painter = ui.painter_at(whole);
        if let Some(texture) = &self.texture {
            painter.image(texture.id(), rect, Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)), Color32::WHITE);
        }
        // A grid where the cells are big enough, and the halves of the playfield.
        let grid = egui::Stroke::new(0.5, Color32::from_white_alpha(50));
        if cw >= 6.0 {
            for x in 0..=w {
                let px = rect.min.x + x as f32 * cw;
                painter.line_segment([pos2(px, rect.min.y), pos2(px, rect.max.y)], grid);
            }
        }
        if ch >= 6.0 {
            for y in 0..=h {
                let py = rect.min.y + y as f32 * ch;
                painter.line_segment([pos2(rect.min.x, py), pos2(rect.max.x, py)], grid);
            }
        }
        if self.part == Part::Playfield {
            let middle = rect.center().x;
            painter.line_segment([pos2(middle, rect.min.y), pos2(middle, rect.max.y)], egui::Stroke::new(1.5, crate::theme::accent()));
            // PF0, PF1 and PF2's edges, faintly.
            for x in [4, 12, 24, 32] {
                let px = rect.min.x + x as f32 * cw;
                painter.line_segment([pos2(px, rect.min.y), pos2(px, rect.max.y)], egui::Stroke::new(1.0, Color32::from_white_alpha(90)));
            }
        }
        for column in 0..swatches {
            let left = rect.max.x + 6.0 + column as f32 * (swatch_w + 2.0);
            for y in 0..h {
                let value = match (self.part, column) {
                    (Part::Sprites, _) => self.sprites.frames[self.frame].colours[y],
                    (Part::Playfield, 0) => self.field.colours[y],
                    (Part::Playfield, _) => self.field.backgrounds[y],
                };
                let r = Rect::from_min_size(pos2(left, rect.min.y + y as f32 * ch), vec2(swatch_w, ch.max(1.0)));
                painter.rect_filled(r.shrink2(vec2(0.0, if ch >= 4.0 { 0.5 } else { 0.0 })), 0.0, rgb32(tv, value));
            }
        }
        let hover = response.hover_pos().map(to_cell).filter(|c| inside(*c));
        if let Some((x, y)) = hover {
            painter.rect_stroke(Rect::from_min_size(rect.min + vec2(x as f32 * cw, y as f32 * ch), vec2(cw, ch)), 0.0, egui::Stroke::new(1.5, crate::theme::accent()), egui::StrokeKind::Inside);
        }
        if swatch_column.is_some() && response.hovered() {
            let tip = if self.part == Part::Sprites { "Each line's colour" } else if swatch_column == Some(0) { "Each row's colour (COLUPF)" } else { "Each row's background (COLUBK)" };
            response.on_hover_text(crate::i18n::tr(tip));
        }
        hover.map(|(x, y)| (x as usize, y as usize))
    }
}

#[cfg(test)]
mod tests;
