//! The tile map and level editor's window contents: the tile set and the tile being drawn on the left, with its
//! colours and the characters it's made from; the map on the right, painted, filled, picked from and copied a block
//! at a time, scrolled and zoomed; and below it how the whole goes into the source and into files.

use crate::table_gen::Form;
use crate::theme;
use crate::tile_map::{Area, Block, CHAR, Colouring, Colours, Layer, Mode, Part, Project};
use eframe::egui::{self, Color32, Rect, Sense, Ui, pos2, vec2};

/// What the map's mouse does.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tool {
    Paint,
    Fill,
    Pick,
    Select,
}

impl Tool {
    const ALL: [(Tool, &'static str, &'static str); 4] = [
        (Tool::Paint, "Paint", "The left button paints the tile chosen (or the block copied); the right picks up the tile under it"),
        (Tool::Fill, "Fill", "Fills the cells joined to the one clicked that hold its tile"),
        (Tool::Pick, "Pick", "Click a cell to paint with its tile"),
        (Tool::Select, "Select", "Drag across the map, then copy, cut or clear the block"),
    ];
}

/// What the tile being drawn on takes from the mouse.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TileTool {
    /// Pixels drawn into the characters the tile is made of.
    Draw,
    /// The character chosen below put into a cell of the tile.
    Place,
}

/// What the editor asks its window to do.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Asked {
    Open,
    /// Saved where it came from (or asked where, if nowhere yet).
    Save,
    SaveAs,
    Insert(String),
    SaveBinaries,
    Close,
}

/// The textures the tiles and characters are drawn from, and what they were drawn from, to know when to draw them
/// again: the project without its map, and the tile chosen (whose colours the characters show in when each tile
/// has its own).
struct Pictures {
    key: Project,
    tile: usize,
    tiles: egui::TextureHandle,
    tile_columns: usize,
    chars: egui::TextureHandle,
}

const CHAR_COLUMNS: usize = 32;

pub struct Editor {
    pub project: Project,
    pub layer: usize,
    /// The tile chosen, drawn on and painted with.
    pub tile: usize,
    /// The character chosen, to put into tiles.
    pub char: usize,
    /// What painting puts down: the tile chosen, or a block copied from the map.
    pub brush: Block,
    pub tool: Tool,
    pub tile_tool: TileTool,
    /// The pixel value the left button draws with.
    pub pen: u8,
    pub selection: Option<Area>,
    pub zoom: f32,
    pub grid: bool,
    pub label: String,
    pub form: Form,
    pub parts: Vec<Part>,
    pub big_endian: bool,
    /// The CharPad version a .ctm is saved as.
    pub ctm_version: u8,
    /// Where it was opened from or last saved, to save to again.
    pub path: Option<std::path::PathBuf>,
    /// The sizes being typed, applied by their buttons.
    tile_size: (usize, usize),
    map_size: (usize, usize),
    undo: Vec<Project>,
    redo: Vec<Project>,
    stroke: Option<(usize, usize)>,
    drag: Option<((usize, usize), (usize, usize))>,
    pictures: Option<Pictures>,
    /// Something said after the last thing done (what Tidy took out, say).
    pub note: String,
}

impl Default for Editor {
    fn default() -> Self {
        Editor::new(Project::default())
    }
}

impl Editor {
    pub fn new(project: Project) -> Editor {
        let form = if project.mode.c64() { Form::Acme } else { Form::Z80 };
        let pen = if project.wide(project.colours) { 3 } else { 1 };
        Editor {
            tile_size: project.tile_size,
            map_size: project.map_size,
            project,
            layer: 0,
            tile: 1,
            char: 1,
            brush: Block::one(1),
            tool: Tool::Paint,
            tile_tool: TileTool::Draw,
            pen,
            selection: None,
            zoom: 2.0,
            grid: true,
            label: "level".into(),
            form,
            parts: vec![Part::Chars, Part::Tiles, Part::Colours, Part::Map],
            big_endian: false,
            ctm_version: 9,
            path: None,
            undo: Vec::new(),
            redo: Vec::new(),
            stroke: None,
            drag: None,
            pictures: None,
            note: String::new(),
        }
    }

    /// A project put in, as from a file: the choices kept in range of it.
    pub fn set_project(&mut self, project: Project) {
        self.remember();
        self.project = project;
        self.fit();
        self.tile_size = self.project.tile_size;
        self.map_size = self.project.map_size;
        self.selection = None;
        // ACME's lines are for the C64; anything else starts in the Z80's.
        if self.form == Form::Acme && !self.project.mode.c64() {
            self.form = Form::Z80;
        }
    }

    /// The choices kept within the project, after something's been taken out of it.
    fn fit(&mut self) {
        let p = &self.project;
        self.layer = self.layer.min(p.layers.len().saturating_sub(1));
        self.tile = self.tile.min(p.tiles.len().saturating_sub(1));
        self.char = self.char.min(p.chars.len().saturating_sub(1));
        let tiles = p.tiles.len() as u16;
        if self.brush.cells.iter().any(|c| *c >= tiles) {
            self.brush = Block::one(self.tile as u16);
        }
    }

    pub fn remember(&mut self) {
        self.undo.push(self.project.clone());
        if self.undo.len() > 64 {
            self.undo.remove(0);
        }
        self.redo.clear();
    }

    pub fn undo(&mut self) {
        if let Some(before) = self.undo.pop() {
            self.redo.push(std::mem::replace(&mut self.project, before));
            self.fit();
        }
    }

    pub fn redo(&mut self) {
        if let Some(after) = self.redo.pop() {
            self.undo.push(std::mem::replace(&mut self.project, after));
            self.fit();
        }
    }

    /// The selection copied, to paint with.
    pub fn copy(&mut self) {
        if let Some(area) = self.selection {
            self.brush = self.project.copy(self.layer, area);
            self.tool = Tool::Paint;
        }
    }

    pub fn cut(&mut self) {
        if let Some(area) = self.selection {
            self.copy();
            self.remember();
            self.project.clear(self.layer, area);
        }
    }

    pub fn clear_selection(&mut self) {
        if let Some(area) = self.selection {
            self.remember();
            self.project.clear(self.layer, area);
        }
    }

    /// The tile chosen, to draw on and paint with.
    pub fn choose_tile(&mut self, tile: usize) {
        self.tile = tile.min(self.project.tiles.len().saturating_sub(1));
        self.brush = Block::one(self.tile as u16);
        if let Some(&first) = self.project.tiles.get(self.tile).and_then(|t| t.cells.first()) {
            self.char = usize::from(first);
        }
    }

    /// A map cell clicked or dragged over with a tool. `right` is the right button.
    pub fn use_tool(&mut self, x: usize, y: usize, right: bool, starting: bool) {
        if right {
            self.choose_tile(usize::from(self.project.get(self.layer, x, y)));
            return;
        }
        match self.tool {
            Tool::Paint => {
                if starting {
                    self.remember();
                } else if self.stroke == Some((x, y)) {
                    return;
                }
                let brush = self.brush.clone();
                self.project.stamp(self.layer, x, y, &brush);
                self.stroke = Some((x, y));
            }
            Tool::Fill if starting => {
                self.remember();
                let brush = self.brush.clone();
                self.project.fill(self.layer, x, y, &brush);
            }
            Tool::Pick if starting => self.choose_tile(usize::from(self.project.get(self.layer, x, y))),
            _ => {}
        }
    }

    /// The colours being set, by colouring: the project's, the chosen tile's or the chosen character's.
    fn target_colours(&mut self) -> &mut Colours {
        let p = &mut self.project;
        match p.colouring {
            Colouring::Project => &mut p.colours,
            Colouring::Tile => match p.tiles.get_mut(self.tile) {
                Some(t) => &mut t.colours,
                None => &mut p.colours,
            },
            Colouring::Char => match p.chars.get_mut(self.char) {
                Some(c) => &mut c.colours,
                None => &mut p.colours,
            },
        }
    }

    pub fn draw(&mut self, ui: &mut Ui) -> Option<Asked> {
        let mut asked = None;
        self.update_pictures(ui.ctx());
        ui.horizontal(|ui| {
            ui.label(crate::i18n::tr("Drawn in"));
            let mut mode = self.project.mode;
            egui::ComboBox::from_id_salt("tile-mode").selected_text(crate::i18n::tr(mode.name())).width(300.0).show_ui(ui, |ui| {
                for m in Mode::ALL {
                    ui.selectable_value(&mut mode, m, crate::i18n::tr(m.name()));
                }
            });
            if mode != self.project.mode {
                self.remember();
                self.project.change_mode(mode);
                self.pen = self.pen.min(self.pens().len().saturating_sub(1) as u8);
                if self.form == Form::Acme && !mode.c64() {
                    self.form = Form::Z80;
                }
            }
            if self.project.mode.c64() || self.project.mode == Mode::Spectrum {
                ui.label(crate::i18n::tr("Colours"));
                let mut colouring = self.project.colouring;
                egui::ComboBox::from_id_salt("tile-colouring").selected_text(crate::i18n::tr(colouring.name())).show_ui(ui, |ui| {
                    for c in Colouring::ALL {
                        ui.selectable_value(&mut colouring, c, crate::i18n::tr(c.name()));
                    }
                });
                if colouring != self.project.colouring {
                    self.remember();
                    self.project.colouring = colouring;
                }
            }
            ui.separator();
            ui.label(crate::i18n::tr("Tiles"));
            ui.add(egui::DragValue::new(&mut self.tile_size.0).range(1..=16));
            ui.label("\u{D7}");
            ui.add(egui::DragValue::new(&mut self.tile_size.1).range(1..=16));
            if self.tile_size != self.project.tile_size && ui.button(crate::i18n::tr("Apply")).on_hover_text(crate::i18n::tr("Each tile keeps the characters at its top left")).clicked() {
                self.remember();
                self.project.resize_tiles(self.tile_size.0, self.tile_size.1);
            }
            ui.label(crate::i18n::tr("characters; map"));
            ui.add(egui::DragValue::new(&mut self.map_size.0).range(1..=8192));
            ui.label("\u{D7}");
            ui.add(egui::DragValue::new(&mut self.map_size.1).range(1..=8192));
            if self.map_size != self.project.map_size && ui.button(crate::i18n::tr("Apply")).on_hover_text(crate::i18n::tr("What's there stays at the top left")).clicked() {
                self.remember();
                self.project.resize_map(self.map_size.0, self.map_size.1);
                self.selection = None;
            }
            ui.label(crate::i18n::tr("tiles"));
        });
        ui.separator();
        ui.horizontal_top(|ui| {
            ui.vertical(|ui| {
                ui.set_width(330.0);
                let most = (ui.ctx().content_rect().height() - 220.0).max(300.0);
                egui::ScrollArea::vertical().id_salt("tile-side").max_height(most).show(ui, |ui| self.tile_side(ui));
            });
            ui.separator();
            ui.vertical(|ui| asked = self.map_side(ui));
        });
        asked
    }

    fn pens(&self) -> Vec<(u8, Color32)> {
        let p = &self.project;
        p.pens(p.colours_for(self.tile, self.char), self.char)
    }

    // ---------- The tile set, the tile, its colours and characters ----------

    fn tile_side(&mut self, ui: &mut Ui) {
        let (tw, th) = self.project.tile_size;
        let tile_px = vec2((tw * CHAR) as f32, (th * CHAR) as f32);
        // The tile set, small, a click choosing one.
        ui.label(egui::RichText::new(crate::i18n::tf!("Tile set: {} tiles", self.project.tiles.len())).strong());
        let thumb = tile_px * (36.0 / tile_px.x.max(tile_px.y)).clamp(1.0, 4.0);
        let per_row = ((320.0 / (thumb.x + 3.0)) as usize).max(1);
        egui::ScrollArea::vertical().id_salt("tile-set").max_height(150.0).show(ui, |ui| {
            egui::Grid::new("tile-thumbs").spacing([3.0, 3.0]).show(ui, |ui| {
                for t in 0..self.project.tiles.len() {
                    let (rect, response) = ui.allocate_exact_size(thumb, Sense::click());
                    if ui.is_rect_visible(rect) {
                        self.paint_tile(ui, rect, t);
                        if t == self.tile {
                            ui.painter().rect_stroke(rect, 0.0, egui::Stroke::new(2.0, theme::accent()), egui::StrokeKind::Outside);
                        }
                    }
                    let response = response.on_hover_text(crate::i18n::tf!("Tile {t}"));
                    if response.clicked() {
                        self.choose_tile(t);
                    }
                    if t % per_row == per_row - 1 {
                        ui.end_row();
                    }
                }
            });
        });
        ui.horizontal_wrapped(|ui| {
            if ui.small_button(crate::i18n::tr("Add")).on_hover_text(crate::i18n::tr("A blank tile after this one")).clicked() {
                self.remember();
                let blank = self.project.blank_tile();
                self.project.insert_tile(self.tile + 1, blank);
                self.choose_tile(self.tile + 1);
            }
            if ui.small_button(crate::i18n::tr("Copy")).on_hover_text(crate::i18n::tr("A copy of this tile after it, made of the same characters")).clicked() {
                self.remember();
                let copy = self.project.tiles[self.tile].clone();
                self.project.insert_tile(self.tile + 1, copy);
                self.choose_tile(self.tile + 1);
            }
            if ui.add_enabled(self.project.tiles.len() > 1, egui::Button::new(crate::i18n::tr("Remove")).small()).on_hover_text(crate::i18n::tr("Where the map has it, tile 0 instead")).clicked() {
                self.remember();
                self.project.remove_tile(self.tile);
                self.fit();
                self.choose_tile(self.tile);
            }
            if ui.small_button(crate::i18n::tr("Own characters")).on_hover_text(crate::i18n::tr("Gives this tile copies of its characters, so drawing on it changes no other tile")).clicked() {
                self.remember();
                let made = self.project.own_chars(self.tile);
                self.note = crate::i18n::tf!("{made} characters added for tile {}.", self.tile);
            }
        });
        if let Some(t) = self.project.tiles.get_mut(self.tile) {
            ui.horizontal(|ui| {
                ui.label(crate::i18n::tr("Name"));
                ui.add(egui::TextEdit::singleline(&mut t.name).desired_width(120.0).char_limit(32));
                ui.label(crate::i18n::tr("Tag"));
                ui.add(egui::DragValue::new(&mut t.tag).range(0..=255)).on_hover_text(crate::i18n::tr("A number for the game: CharPad's tile tag"));
            });
        }
        ui.separator();
        // The tile, big.
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new(crate::i18n::tf!("Tile {}", self.tile)).strong());
            ui.selectable_value(&mut self.tile_tool, TileTool::Draw, crate::i18n::tr("Draw")).on_hover_text(crate::i18n::tr("The left button draws in the colour chosen, the right rubs out"));
            ui.selectable_value(&mut self.tile_tool, TileTool::Place, crate::i18n::tr("Place characters")).on_hover_text(crate::i18n::tr("The left button puts the character chosen below into a cell; the right picks up a cell's character"));
        });
        let scale = (300.0 / tile_px.x).min(220.0 / tile_px.y).floor().max(1.0);
        let (rect, response) = ui.allocate_exact_size(tile_px * scale, Sense::click_and_drag());
        self.paint_tile(ui, rect, self.tile);
        let grid = egui::Stroke::new(0.5, Color32::from_white_alpha(40));
        let strong = egui::Stroke::new(1.0, Color32::from_white_alpha(110));
        for i in 0..=tw * CHAR {
            let x = rect.min.x + i as f32 * scale;
            ui.painter().line_segment([pos2(x, rect.min.y), pos2(x, rect.max.y)], if i % CHAR == 0 { strong } else { grid });
        }
        for i in 0..=th * CHAR {
            let y = rect.min.y + i as f32 * scale;
            ui.painter().line_segment([pos2(rect.min.x, y), pos2(rect.max.x, y)], if i % CHAR == 0 { strong } else { grid });
        }
        let (primary, secondary) = ui.input(|i| (i.pointer.primary_down(), i.pointer.secondary_down()));
        let at = response.interact_pointer_pos().filter(|_| response.is_pointer_button_down_on() && (primary || secondary)).map(|p| (((p.x - rect.min.x) / scale) as usize, ((p.y - rect.min.y) / scale) as usize)).filter(|(x, y)| *x < tw * CHAR && *y < th * CHAR);
        let hover = response.hover_pos().map(|p| (((p.x - rect.min.x) / scale) as usize, ((p.y - rect.min.y) / scale) as usize)).filter(|(x, y)| *x < tw * CHAR && *y < th * CHAR);
        if let Some((x, y)) = at {
            let cell = (y / CHAR) * tw + x / CHAR;
            let char = self.project.tile_char(self.tile, cell);
            match self.tile_tool {
                TileTool::Draw => {
                    if self.stroke.is_none() {
                        self.remember();
                        self.stroke = Some((usize::MAX, usize::MAX));
                    }
                    let colours = self.project.colours_for(self.tile, char);
                    let wide = self.project.wide(colours);
                    let value = if secondary { 0 } else { self.pen };
                    // In ECM, only the first 64 characters' shapes are shown.
                    let shape = if self.project.mode == Mode::C64Ecm { char & 63 } else { char };
                    self.project.set_pixel(shape, x % CHAR, y % CHAR, value, wide);
                    self.char = char;
                }
                TileTool::Place if response.drag_started() || response.clicked() || response.secondary_clicked() || self.stroke.is_none() => {
                    if secondary {
                        self.char = char;
                    } else if self.project.tile_char(self.tile, cell) != self.char {
                        self.remember();
                        self.project.tiles[self.tile].cells[cell] = self.char as u16;
                    }
                    self.stroke = Some((usize::MAX, usize::MAX));
                }
                TileTool::Place => {}
            }
        } else if !primary && !secondary {
            self.stroke = None;
        }
        let status = match hover {
            Some((x, y)) => {
                let char = self.project.tile_char(self.tile, (y / CHAR) * tw + x / CHAR);
                let wide = self.project.wide(self.project.colours_for(self.tile, char));
                let shape = if self.project.mode == Mode::C64Ecm { char & 63 } else { char };
                let value = self.project.pixel(shape, x % CHAR, y % CHAR, wide);
                crate::i18n::tf!("x {}, y {} \u{B7} character {char} \u{B7} pen {value}", x, y)
            }
            None => String::new(),
        };
        ui.label(egui::RichText::new(status).monospace().size(11.0).color(theme::text_dim()));
        // The pens.
        ui.horizontal_wrapped(|ui| {
            ui.label(crate::i18n::tr("Draw with"));
            let names = self.pen_names();
            for (value, colour) in self.pens() {
                let (rect, response) = ui.allocate_exact_size(vec2(20.0, 20.0), Sense::click());
                ui.painter().rect_filled(rect, 2.0, colour);
                if value == self.pen {
                    ui.painter().rect_stroke(rect, 2.0, egui::Stroke::new(2.0, theme::accent()), egui::StrokeKind::Outside);
                }
                let tip = names.get(usize::from(value)).map_or_else(|| crate::i18n::tf!("Colour {value}"), |n| crate::i18n::tr(n).to_string());
                if response.on_hover_text(tip).clicked() {
                    self.pen = value;
                }
            }
        });
        ui.horizontal_wrapped(|ui| {
            for (label, how, tip) in [
                ("\u{2194}", crate::pixel_editor::Transform::FlipAcross, "Flip the character chosen left to right"),
                ("\u{2195}", crate::pixel_editor::Transform::FlipDown, "Flip it top to bottom"),
                ("\u{21BB}", crate::pixel_editor::Transform::Rotate, "Turn it a quarter clockwise"),
                ("Invert", crate::pixel_editor::Transform::Invert, "Swaps its set and clear pixels"),
            ] {
                if ui.small_button(crate::i18n::tr(label)).on_hover_text(crate::i18n::tr(tip)).clicked() && self.char < self.project.chars.len() {
                    self.remember();
                    let wide = self.project.wide(self.project.colours_for(self.tile, self.char));
                    self.project.transform_char(self.char, how, wide);
                }
            }
            if ui.small_button(crate::i18n::tr("Clear")).on_hover_text(crate::i18n::tr("Clears the character chosen")).clicked() && self.char < self.project.chars.len() {
                self.remember();
                self.project.chars[self.char].pixels = [0; 64];
            }
        });
        self.colours(ui);
        ui.separator();
        self.char_set(ui);
    }

    /// What each pen is, in words, where the machine names them.
    fn pen_names(&self) -> Vec<&'static str> {
        let colours = self.project.colours_for(self.tile, self.char);
        match self.project.mode {
            Mode::C64Multicolour if self.project.wide(colours) => vec!["Background ($D021)", "Multicolour 1 ($D022)", "Multicolour 2 ($D023)", "The character's colour"],
            Mode::C64BitmapMulticolour => vec!["Background ($D021)", "The screen's high nibble", "The screen's low nibble", "Colour RAM"],
            Mode::C64BitmapHires => vec!["The screen's low nibble", "The screen's high nibble"],
            Mode::Spectrum => vec!["Paper", "Ink"],
            m if m.c64() => vec!["Background", "The character's colour"],
            _ => Vec::new(),
        }
    }

    /// The machine's colours: the C64's backgrounds and the colours being set; the Spectrum's ink, paper and
    /// bright; the palette for the modes of so many bits.
    fn colours(&mut self, ui: &mut Ui) {
        let mode = self.project.mode;
        let whose = match self.project.colouring {
            Colouring::Project => crate::i18n::tr("everywhere").to_string(),
            Colouring::Tile => crate::i18n::tf!("tile {}", self.tile),
            Colouring::Char => crate::i18n::tf!("character {}", self.char),
        };
        ui.add_space(4.0);
        if mode.c64() {
            let backgrounds: &[(usize, &str)] = match mode {
                Mode::C64Multicolour => &[(0, "Background $D021"), (1, "Multicolour 1 $D022"), (2, "Multicolour 2 $D023")],
                Mode::C64Ecm => &[(0, "Background $D021"), (1, "Background $D022"), (2, "Background $D023"), (3, "Background $D024")],
                Mode::C64BitmapHires => &[],
                _ => &[(0, "Background $D021")],
            };
            let own: &[(usize, &str)] = match mode {
                Mode::C64BitmapHires => &[(2, "Set pixels (screen high nibble)"), (1, "Clear pixels (screen low nibble)")],
                Mode::C64BitmapMulticolour => &[(2, "01 (screen high nibble)"), (1, "10 (screen low nibble)"), (0, "11 (colour RAM)")],
                Mode::C64Multicolour => &[(0, "Colour (8 and up: multicolour)")],
                _ => &[(0, "Colour")],
            };
            egui::Grid::new("tile-c64-colours").num_columns(2).spacing([8.0, 3.0]).show(ui, |ui| {
                for &(slot, label) in backgrounds {
                    ui.label(crate::i18n::tr(label));
                    let mut chosen = self.project.backgrounds[slot];
                    c64_choice(ui, ("tile-bg", slot), &mut chosen);
                    if chosen != self.project.backgrounds[slot] {
                        self.remember();
                        self.project.backgrounds[slot] = chosen;
                    }
                    ui.end_row();
                }
                for &(slot, label) in own {
                    ui.label(crate::i18n::tf!("{}, {whose}", crate::i18n::tr(label)));
                    let mut chosen = self.target_colours()[slot];
                    c64_choice(ui, ("tile-own", slot), &mut chosen);
                    if chosen != self.target_colours()[slot] {
                        self.remember();
                        self.target_colours()[slot] = chosen;
                    }
                    ui.end_row();
                }
            });
        } else if mode == Mode::Spectrum {
            let attribute = self.target_colours()[0];
            let (mut ink, mut paper, mut bright, mut flash) = (attribute & 7, (attribute >> 3) & 7, attribute & 0x40 != 0, attribute & 0x80 != 0);
            ui.label(crate::i18n::tf!("Attribute, {whose}"));
            ui.horizontal(|ui| {
                ui.label(crate::i18n::tr("Ink"));
                spectrum_choice(ui, "tile-ink", &mut ink, bright);
                ui.label(crate::i18n::tr("Paper"));
                spectrum_choice(ui, "tile-paper", &mut paper, bright);
            });
            ui.horizontal(|ui| {
                ui.checkbox(&mut bright, crate::i18n::tr("Bright"));
                ui.checkbox(&mut flash, crate::i18n::tr("Flash"));
            });
            let chosen = ink | (paper << 3) | if bright { 0x40 } else { 0 } | if flash { 0x80 } else { 0 };
            if chosen != attribute {
                self.remember();
                self.target_colours()[0] = chosen;
            }
        } else if self.project.palette.len() <= 16 {
            ui.label(crate::i18n::tr("Palette"));
            ui.horizontal_wrapped(|ui| {
                let mut palette = self.project.palette.clone();
                for c in &mut palette {
                    ui.color_edit_button_srgb(c);
                }
                if palette != self.project.palette {
                    self.project.palette = palette;
                }
            });
        }
        if let Some(c) = self.project.chars.get_mut(self.char) {
            ui.horizontal(|ui| {
                ui.label(crate::i18n::tf!("Character {}'s material", self.char));
                ui.add(egui::DragValue::new(&mut c.material).range(0..=15)).on_hover_text(crate::i18n::tr("A number for the game, 0 to 15 (wall, ladder and so on): CharPad's material"));
            });
        }
    }

    /// The characters, small; a click chooses one.
    fn char_set(&mut self, ui: &mut Ui) {
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new(crate::i18n::tf!("Characters: {}", self.project.chars.len())).strong());
            if ui.small_button(crate::i18n::tr("Add")).on_hover_text(crate::i18n::tr("A blank character at the end")).clicked() && self.project.chars.len() < 65536 {
                self.remember();
                let colours = self.project.colours;
                self.project.chars.push(crate::tile_map::Char { pixels: [0; 64], colours, material: 0 });
                self.char = self.project.chars.len() - 1;
            }
            if ui.small_button(crate::i18n::tr("Tidy")).on_hover_text(crate::i18n::tr("Takes out characters no tile uses, and makes those just like another one")).clicked() {
                self.remember();
                let gone = self.project.tidy_chars();
                self.fit();
                self.note = crate::i18n::tf!("{gone} characters taken out.");
            }
        });
        let Some(pictures) = &self.pictures else { return };
        let count = self.project.chars.len();
        let size = 16.0;
        let rows = count.div_ceil(CHAR_COLUMNS);
        let id = pictures.chars.id();
        let texture_size = pictures.chars.size_vec2();
        egui::ScrollArea::vertical().id_salt("tile-chars").max_height(160.0).show(ui, |ui| {
            let (rect, response) = ui.allocate_exact_size(vec2(CHAR_COLUMNS as f32 * size * 0.62, rows as f32 * size * 0.62), Sense::click());
            let cell = size * 0.62;
            let mut mesh = egui::Mesh::with_texture(id);
            for c in 0..count {
                let (cx, cy) = ((c % CHAR_COLUMNS) as f32, (c / CHAR_COLUMNS) as f32);
                let r = Rect::from_min_size(rect.min + vec2(cx * cell, cy * cell), vec2(cell, cell));
                if !ui.is_rect_visible(r) {
                    continue;
                }
                let uv = Rect::from_min_size(pos2(cx * CHAR as f32 / texture_size.x, cy * CHAR as f32 / texture_size.y), vec2(CHAR as f32 / texture_size.x, CHAR as f32 / texture_size.y));
                mesh.add_rect_with_uv(r, uv, Color32::WHITE);
            }
            ui.painter().add(egui::Shape::mesh(mesh));
            let chosen = Rect::from_min_size(rect.min + vec2((self.char % CHAR_COLUMNS) as f32 * cell, (self.char / CHAR_COLUMNS) as f32 * cell), vec2(cell, cell));
            ui.painter().rect_stroke(chosen, 0.0, egui::Stroke::new(1.5, theme::accent()), egui::StrokeKind::Outside);
            let pointed = response.hover_pos().map(|p| ((p.y - rect.min.y) / cell) as usize * CHAR_COLUMNS + ((p.x - rect.min.x) / cell) as usize).filter(|c| *c < count);
            if let Some(c) = pointed {
                let response = response.on_hover_text(crate::i18n::tf!("Character {c}"));
                if response.clicked() {
                    self.char = c;
                    self.tile_tool = TileTool::Place;
                }
            }
        });
    }

    /// A tile drawn into a rectangle, from the tile set's picture.
    fn paint_tile(&self, ui: &Ui, rect: Rect, tile: usize) {
        if let Some(uv) = self.tile_uv(tile)
            && let Some(pictures) = &self.pictures
        {
            ui.painter().image(pictures.tiles.id(), rect, uv, Color32::WHITE);
        } else {
            ui.painter().rect_filled(rect, 0.0, Color32::from_rgb(255, 0, 255));
        }
    }

    fn tile_uv(&self, tile: usize) -> Option<Rect> {
        let pictures = self.pictures.as_ref()?;
        let (tw, th) = (self.project.tile_size.0 * CHAR, self.project.tile_size.1 * CHAR);
        let size = pictures.tiles.size_vec2();
        let (col, row) = (tile % pictures.tile_columns, tile / pictures.tile_columns);
        let max = pos2(((col + 1) * tw) as f32, ((row + 1) * th) as f32);
        (max.y <= size.y + 0.5 && tile < self.project.tiles.len()).then(|| Rect::from_min_max(pos2((col * tw) as f32 / size.x, (row * th) as f32 / size.y), pos2(max.x / size.x, max.y / size.y)))
    }

    /// The tile set and characters drawn as pictures again, if they've changed.
    fn update_pictures(&mut self, ctx: &egui::Context) {
        let p = &self.project;
        if let Some(pictures) = &self.pictures {
            let k = &pictures.key;
            let tile_matters = p.colouring == Colouring::Tile && pictures.tile != self.tile;
            if !tile_matters && k.mode == p.mode && k.colouring == p.colouring && k.backgrounds == p.backgrounds && k.colours == p.colours && k.palette == p.palette && k.chars == p.chars && k.tile_size == p.tile_size && k.tiles == p.tiles {
                return;
            }
        }
        // The map isn't kept: it doesn't change the pictures, and can be big.
        let key = Project { mode: p.mode, colouring: p.colouring, backgrounds: p.backgrounds, colours: p.colours, palette: p.palette.clone(), chars: p.chars.clone(), tile_size: p.tile_size, tiles: p.tiles.clone(), map_size: p.map_size, layers: Vec::new(), ctm: p.ctm };
        let (tw, th) = (p.tile_size.0 * CHAR, p.tile_size.1 * CHAR);
        let columns = (2048 / tw).max(1);
        // The texture is at most 8192 high: tiles past that are shown magenta.
        let rows = p.tiles.len().div_ceil(columns).clamp(1, 8192 / th);
        let mut pixels = vec![Color32::TRANSPARENT; columns * tw * rows * th];
        for t in 0..p.tiles.len().min(columns * rows) {
            let (ox, oy) = ((t % columns) * tw, (t / columns) * th);
            for (i, c) in p.tile_rgba(t).into_iter().enumerate() {
                pixels[(oy + i / tw) * columns * tw + ox + i % tw] = c;
            }
        }
        let tiles = egui::ColorImage::new([columns * tw, rows * th], pixels);
        let char_rows = p.chars.len().div_ceil(CHAR_COLUMNS).clamp(1, 1024);
        let mut char_pixels = vec![Color32::TRANSPARENT; CHAR_COLUMNS * CHAR * char_rows * CHAR];
        for c in 0..p.chars.len().min(CHAR_COLUMNS * char_rows) {
            let colours = if p.colouring == Colouring::Char { p.chars[c].colours } else { p.colours_for(self.tile, c) };
            let (ox, oy) = ((c % CHAR_COLUMNS) * CHAR, (c / CHAR_COLUMNS) * CHAR);
            for (i, colour) in p.char_rgba(c, colours).into_iter().enumerate() {
                char_pixels[(oy + i / CHAR) * CHAR_COLUMNS * CHAR + ox + i % CHAR] = colour;
            }
        }
        let chars = egui::ColorImage::new([CHAR_COLUMNS * CHAR, char_rows * CHAR], char_pixels);
        let options = egui::TextureOptions::NEAREST;
        match &mut self.pictures {
            Some(pictures) => {
                pictures.tiles.set(tiles, options);
                pictures.chars.set(chars, options);
                pictures.tile_columns = columns;
                pictures.key = key;
                pictures.tile = self.tile;
            }
            None => {
                self.pictures = Some(Pictures { key, tile: self.tile, tiles: ctx.load_texture("tile-map-tiles", tiles, options), tile_columns: columns, chars: ctx.load_texture("tile-map-chars", chars, options) });
            }
        }
    }

    // ---------- The map ----------

    fn map_side(&mut self, ui: &mut Ui) -> Option<Asked> {
        let mut asked = None;
        ui.horizontal_wrapped(|ui| {
            for (tool, name, tip) in Tool::ALL {
                if ui.selectable_label(self.tool == tool, crate::i18n::tr(name)).on_hover_text(crate::i18n::tr(tip)).clicked() {
                    self.tool = tool;
                }
            }
            ui.separator();
            let selected = self.selection.is_some();
            if ui.add_enabled(selected, egui::Button::new(crate::i18n::tr("Copy"))).on_hover_text(crate::i18n::tr("The block selected becomes what's painted")).clicked() {
                self.copy();
            }
            if ui.add_enabled(selected, egui::Button::new(crate::i18n::tr("Cut"))).on_hover_text(crate::i18n::tr("Copied, and tile 0 left in its place")).clicked() {
                self.cut();
            }
            if ui.add_enabled(selected, egui::Button::new(crate::i18n::tr("Clear"))).on_hover_text(crate::i18n::tr("Tile 0 in the block selected")).clicked() {
                self.clear_selection();
            }
            if ui.add_enabled(!self.undo.is_empty(), egui::Button::new(crate::i18n::tr("Undo"))).clicked() {
                self.undo();
            }
            if ui.add_enabled(!self.redo.is_empty(), egui::Button::new(crate::i18n::tr("Redo"))).clicked() {
                self.redo();
            }
        });
        self.layers(ui);
        let (tw, th) = (self.project.tile_size.0 * CHAR, self.project.tile_size.1 * CHAR);
        let (mw, mh) = self.project.map_size;
        let cell = vec2(tw as f32 * self.zoom, th as f32 * self.zoom);
        let room = ui.ctx().content_rect().size() - vec2(470.0, 410.0);
        let (width, height) = (room.x.max(320.0), room.y.max(160.0));
        let mut hovered = None;
        egui::ScrollArea::both().id_salt("tile-map").max_width(width).max_height(height).auto_shrink([false, false]).show_viewport(ui, |ui, viewport| {
            let (rect, response) = ui.allocate_exact_size(vec2(mw as f32 * cell.x, mh as f32 * cell.y), Sense::click_and_drag());
            hovered = self.map_canvas(ui, rect, &response, viewport, cell);
        });
        let brush = if self.brush.cells.len() == 1 { crate::i18n::tf!("tile {}", self.brush.cells[0]) } else { crate::i18n::tf!("a block of {}\u{D7}{}", self.brush.width, self.brush.height) };
        let status = match hovered {
            Some((x, y)) => crate::i18n::tf!("x {x}, y {y} \u{B7} tile {} \u{B7} painting with {brush}", self.project.get(self.layer, x, y)),
            None => crate::i18n::tf!("{mw}\u{D7}{mh} tiles \u{B7} painting with {brush}"),
        };
        ui.label(egui::RichText::new(status).monospace().size(11.0).color(theme::text_dim()));
        if !self.note.is_empty() {
            ui.label(egui::RichText::new(&self.note).size(11.5).color(theme::text_dim()));
        }
        ui.separator();
        self.output(ui, &mut asked);
        asked
    }

    fn layers(&mut self, ui: &mut Ui) {
        ui.horizontal_wrapped(|ui| {
            ui.label(crate::i18n::tr("Layer"));
            let names: Vec<String> = self.project.layers.iter().enumerate().map(|(i, l)| if l.name.is_empty() { crate::i18n::tf!("Layer {}", i + 1) } else { l.name.clone() }).collect();
            let mut layer = self.layer;
            egui::ComboBox::from_id_salt("tile-layer").selected_text(names.get(layer).cloned().unwrap_or_default()).show_ui(ui, |ui| {
                for (i, name) in names.iter().enumerate() {
                    ui.selectable_value(&mut layer, i, name);
                }
            });
            if layer != self.layer {
                self.layer = layer;
                self.selection = None;
            }
            if let Some(Layer { name, shown, .. }) = self.project.layers.get_mut(self.layer) {
                ui.add(egui::TextEdit::singleline(name).desired_width(90.0));
                ui.checkbox(shown, crate::i18n::tr("Shown"));
            }
            if ui.small_button(crate::i18n::tr("Add a layer")).on_hover_text(crate::i18n::tr("Another map over this one: its tile 0 lets those below show through")).clicked() {
                self.remember();
                self.project.add_layer();
                self.layer = self.project.layers.len() - 1;
            }
            if self.project.layers.len() > 1 && ui.small_button(crate::i18n::tr("Remove it")).clicked() {
                self.remember();
                self.project.layers.remove(self.layer);
                self.fit();
            }
            ui.separator();
            ui.label(crate::i18n::tr("Zoom"));
            egui::ComboBox::from_id_salt("tile-zoom").width(50.0).selected_text(crate::i18n::tf!("{}\u{D7}", self.zoom)).show_ui(ui, |ui| {
                for z in [1.0, 2.0, 3.0, 4.0] {
                    ui.selectable_value(&mut self.zoom, z, crate::i18n::tf!("{z}\u{D7}"));
                }
            });
            ui.checkbox(&mut self.grid, crate::i18n::tr("Grid"));
        });
    }

    /// The map drawn, the cells in view, and the mouse on it; where the mouse is.
    fn map_canvas(&mut self, ui: &mut Ui, rect: Rect, response: &egui::Response, viewport: Rect, cell: egui::Vec2) -> Option<(usize, usize)> {
        let (mw, mh) = self.project.map_size;
        let to_cell = |p: egui::Pos2| (((p.x - rect.min.x) / cell.x).floor() as i64, ((p.y - rect.min.y) / cell.y).floor() as i64);
        let inside = |(x, y): (i64, i64)| (0..mw as i64).contains(&x) && (0..mh as i64).contains(&y);
        let (primary, secondary) = ui.input(|i| (i.pointer.primary_down(), i.pointer.secondary_down()));
        let pointer = response.interact_pointer_pos().map(to_cell);
        if self.tool == Tool::Select && !secondary {
            if let Some(p) = pointer.filter(|_| response.is_pointer_button_down_on() && primary) {
                let clamp = |(x, y): (i64, i64)| (x.clamp(0, mw as i64 - 1) as usize, y.clamp(0, mh as i64 - 1) as usize);
                let p = clamp(p);
                let from = self.drag.map_or(p, |d| d.0);
                self.drag = Some((from, p));
                let ((ax, ay), (bx, by)) = (from, p);
                self.selection = Some((ax.min(bx), ay.min(by), ax.abs_diff(bx) + 1, ay.abs_diff(by) + 1));
            } else {
                self.drag = None;
            }
        } else if let Some(p) = pointer.filter(|p| inside(*p) && response.is_pointer_button_down_on() && (primary || secondary)) {
            let starting = self.stroke.is_none();
            self.use_tool(p.0 as usize, p.1 as usize, secondary, starting);
            if self.stroke.is_none() {
                self.stroke = Some((p.0 as usize, p.1 as usize));
            }
        } else if !primary && !secondary {
            self.stroke = None;
        }

        // Only the cells in view are drawn, each from the tile set's picture.
        let view = viewport.translate(rect.min.to_vec2());
        let (x0, y0) = (((view.min.x - rect.min.x) / cell.x).floor().max(0.0) as usize, ((view.min.y - rect.min.y) / cell.y).floor().max(0.0) as usize);
        let (x1, y1) = ((((view.max.x - rect.min.x) / cell.x).ceil().max(0.0) as usize).min(mw), (((view.max.y - rect.min.y) / cell.y).ceil().max(0.0) as usize).min(mh));
        let painter = ui.painter_at(rect);
        painter.rect_filled(rect.intersect(view), 0.0, Color32::from_gray(30));
        if let Some(pictures) = &self.pictures {
            let mut mesh = egui::Mesh::with_texture(pictures.tiles.id());
            for (n, layer) in self.project.layers.iter().enumerate() {
                if !layer.shown {
                    continue;
                }
                for y in y0..y1 {
                    for x in x0..x1 {
                        let tile = layer.cells[y * mw + x];
                        // Tile 0 on a layer above the first lets those below show.
                        if n > 0 && tile == 0 {
                            continue;
                        }
                        let r = Rect::from_min_size(rect.min + vec2(x as f32 * cell.x, y as f32 * cell.y), cell);
                        match self.tile_uv(usize::from(tile)) {
                            Some(uv) => mesh.add_rect_with_uv(r, uv, Color32::WHITE),
                            None => {
                                painter.rect_filled(r, 0.0, Color32::from_rgb(255, 0, 255));
                            }
                        }
                    }
                }
            }
            painter.add(egui::Shape::mesh(mesh));
        }
        if self.grid && cell.x >= 6.0 {
            let stroke = egui::Stroke::new(0.5, Color32::from_white_alpha(45));
            for x in x0..=x1 {
                let px = rect.min.x + x as f32 * cell.x;
                painter.line_segment([pos2(px, rect.min.y + y0 as f32 * cell.y), pos2(px, rect.min.y + y1 as f32 * cell.y)], stroke);
            }
            for y in y0..=y1 {
                let py = rect.min.y + y as f32 * cell.y;
                painter.line_segment([pos2(rect.min.x + x0 as f32 * cell.x, py), pos2(rect.min.x + x1 as f32 * cell.x, py)], stroke);
            }
        }
        let area_rect = |(x, y, w, h): Area| Rect::from_min_size(rect.min + vec2(x as f32 * cell.x, y as f32 * cell.y), vec2(w as f32 * cell.x, h as f32 * cell.y));
        let marking = egui::Stroke::new(2.0, theme::accent());
        if let Some(area) = self.selection {
            painter.rect_stroke(area_rect(area), 0.0, marking, egui::StrokeKind::Inside);
        }
        let hover = response.hover_pos().map(to_cell).filter(|p| inside(*p)).map(|(x, y)| (x as usize, y as usize));
        if let Some((x, y)) = hover
            && self.tool == Tool::Paint
        {
            painter.rect_stroke(area_rect((x, y, self.brush.width, self.brush.height)), 0.0, egui::Stroke::new(1.0, Color32::WHITE), egui::StrokeKind::Inside);
        }
        hover
    }

    // ---------- Out ----------

    fn output(&mut self, ui: &mut Ui, asked: &mut Option<Asked>) {
        ui.horizontal_wrapped(|ui| {
            ui.label(crate::i18n::tr("Out"));
            for part in Part::ALL {
                let mut on = self.parts.contains(&part);
                if ui.checkbox(&mut on, crate::i18n::tr(part.name())).on_hover_text(crate::i18n::tr(part.tip())).changed() {
                    if on {
                        self.parts.push(part);
                    } else {
                        self.parts.retain(|p| *p != part);
                    }
                }
            }
        });
        ui.horizontal_wrapped(|ui| {
            egui::ComboBox::from_id_salt("tile-form").selected_text(crate::i18n::tr(self.form.name())).width(230.0).show_ui(ui, |ui| {
                for form in Form::ALL {
                    ui.selectable_value(&mut self.form, form, crate::i18n::tr(form.name()));
                }
            });
            ui.label(crate::i18n::tr("Label"));
            ui.add(egui::TextEdit::singleline(&mut self.label).desired_width(90.0));
            if ui.button(crate::i18n::tr("Put in the source")).on_hover_text(crate::i18n::tr("In the form chosen, at the cursor")).clicked() {
                *asked = Some(Asked::Insert(self.source()));
            }
            if ui.button(crate::i18n::tr("Copy")).on_hover_text(crate::i18n::tr("In the form chosen, to the clipboard")).clicked() {
                ui.ctx().copy_text(crate::i18n::comments(&self.source()));
                self.note = crate::i18n::tr("Copied to the clipboard.").into();
            }
            if ui.button(crate::i18n::tr("Binary files\u{2026}")).on_hover_text(crate::i18n::tr("Each part as a file of its own, into a folder")).clicked() {
                *asked = Some(Asked::SaveBinaries);
            }
            ui.checkbox(&mut self.big_endian, crate::i18n::tr("Big-endian")).on_hover_text(crate::i18n::tr("Words high byte first, as the 68000 has them; otherwise low byte first (Z80, 6502, x86)"));
        });
        ui.horizontal_wrapped(|ui| {
            if ui.button(crate::i18n::tr("Open\u{2026}")).on_hover_text(crate::i18n::tr("A tile map (.ltm) or a CharPad project (.ctm, versions 5 to 9)")).clicked() {
                *asked = Some(Asked::Open);
            }
            if ui.button(crate::i18n::tr("Save")).clicked() {
                *asked = Some(Asked::Save);
            }
            if ui.button(crate::i18n::tr("Save as\u{2026}")).on_hover_text(crate::i18n::tr("As a tile map (.ltm), or a CharPad project (.ctm) for the C64's modes")).clicked() {
                *asked = Some(Asked::SaveAs);
            }
            ui.label(crate::i18n::tr("CharPad"));
            egui::ComboBox::from_id_salt("tile-ctm").selected_text(crate::i18n::tf!("version {}", self.ctm_version)).show_ui(ui, |ui| {
                for (version, name) in crate::tile_ctm::VERSIONS {
                    ui.selectable_value(&mut self.ctm_version, version, crate::i18n::tr(name));
                }
            });
            if ui.button(crate::i18n::tr("Close")).clicked() {
                *asked = Some(Asked::Close);
            }
        });
    }

    /// The parts chosen, in the form chosen.
    pub fn source(&self) -> String {
        self.project.source(&self.label, &self.parts, self.form, 1000)
    }
}

/// One of the C64's sixteen colours, chosen.
fn c64_choice(ui: &mut Ui, id: (&str, usize), chosen: &mut u8) {
    let names = crate::c64_gfx::NAMES;
    let current = *chosen;
    egui::ComboBox::from_id_salt(id).selected_text(egui::RichText::new(crate::i18n::tf!("\u{25A0} {current}: {}", crate::i18n::tr(names[usize::from(current & 15)]))).color(crate::tile_map::c64_colour(current))).show_ui(ui, |ui| {
        for c in 0..16u8 {
            ui.selectable_value(chosen, c, egui::RichText::new(crate::i18n::tf!("\u{25A0} {c}: {}", crate::i18n::tr(names[usize::from(c)]))).color(crate::tile_map::c64_colour(c)));
        }
    });
}

/// One of the Spectrum's eight colours, chosen.
fn spectrum_choice(ui: &mut Ui, id: &str, chosen: &mut u8, bright: bool) {
    const NAMES: [&str; 8] = ["black", "blue", "red", "magenta", "green", "cyan", "yellow", "white"];
    let current = *chosen & 7;
    egui::ComboBox::from_id_salt(id).width(90.0).selected_text(egui::RichText::new(crate::i18n::tf!("\u{25A0} {}", crate::i18n::tr(NAMES[usize::from(current)]))).color(crate::screen_designer::colour(current, bright))).show_ui(ui, |ui| {
        for c in 0..8u8 {
            ui.selectable_value(chosen, c, egui::RichText::new(crate::i18n::tf!("\u{25A0} {}", crate::i18n::tr(NAMES[usize::from(c)]))).color(crate::screen_designer::colour(c, true)));
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_map_tools_paint_fill_pick_and_undo() {
        let mut e = Editor::new(Project::new(Mode::C64Hires, (2, 2), (8, 6)));
        e.choose_tile(3);
        e.use_tool(1, 1, false, true);
        e.use_tool(2, 1, false, false);
        e.use_tool(2, 1, false, false);
        assert_eq!((e.project.get(0, 1, 1), e.project.get(0, 2, 1)), (3, 3));
        assert_eq!(e.undo.len(), 1, "a stroke is one change to undo");
        // The right button picks up the tile under it.
        e.choose_tile(0);
        e.use_tool(2, 1, true, true);
        assert_eq!((e.tile, e.brush.cells[0]), (3, 3));
        e.tool = Tool::Fill;
        e.choose_tile(5);
        e.use_tool(0, 0, false, true);
        assert_eq!(e.project.layers[0].cells.iter().filter(|c| **c == 5).count(), 46);
        e.undo();
        assert_eq!(e.project.layers[0].cells.iter().filter(|c| **c == 5).count(), 0);
        e.redo();
        assert_eq!(e.project.get(0, 7, 5), 5);
        e.tool = Tool::Pick;
        e.use_tool(1, 1, false, true);
        assert_eq!(e.tile, 3);
        // Selected, copied, and painted elsewhere as a block.
        e.selection = Some((1, 1, 2, 1));
        e.copy();
        assert_eq!((e.tool, e.brush.width, e.brush.cells.clone()), (Tool::Paint, 2, vec![3, 3]));
        e.use_tool(5, 4, false, true);
        assert_eq!((e.project.get(0, 5, 4), e.project.get(0, 6, 4)), (3, 3));
        e.cut();
        assert_eq!((e.project.get(0, 1, 1), e.project.get(0, 2, 1)), (0, 0));
        e.undo();
        assert_eq!(e.project.get(0, 1, 1), 3);
        // Taking out tiles keeps what's chosen in range.
        e.project.tiles.truncate(2);
        e.fit();
        assert_eq!(e.brush, Block::one(1));
    }

    #[test]
    fn the_window_draws_in_every_mode_and_colouring() {
        let ctx = egui::Context::default();
        for mode in Mode::ALL {
            for colouring in Colouring::ALL {
                let mut project = Project::new(mode, (2, 2), (12, 8));
                project.colouring = colouring;
                project.chars[1].pixels = [1; 64];
                project.tiles[1].cells = vec![1, 0, 0, 1];
                project.set(0, 3, 3, 1);
                let mut editor = Editor::new(project);
                editor.project.add_layer();
                for _ in 0..2 {
                    let _ = ctx.run_ui(Default::default(), |ui| {
                        crate::fit::Dialog::new("t").show(ui.ctx(), |ui| editor.draw(ui));
                    });
                }
                assert!(editor.pictures.is_some(), "{mode:?}");
            }
        }
    }
}
