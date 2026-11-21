//! The Structure panel: the file in hex mode read with a structure template, its fields shown as a tree with their
//! values. Clicking a field selects its bytes in the hex view; moving the hex cursor finds its field in the tree;
//! the hex view colours each field's bytes and names the field under the pointer. The language is in
//! `structure.rs`, and the templates there are to choose from in `structure_library.rs`.

use crate::hex_buffer::HexBuffer;
use crate::panels::{Action, DocView};
use crate::structure::{NodeKind, Source, Tree};
use crate::structure_library::Library;
use crate::theme;
use eframe::egui::{self, Align2, Color32, FontId, Rect, RichText, Sense, Stroke, Ui, pos2, vec2};
use std::collections::HashSet;

impl Source for HexBuffer {
    fn len(&self) -> u64 {
        HexBuffer::len(self) as u64
    }

    fn read(&self, at: u64, count: usize) -> Vec<u8> {
        let len = HexBuffer::len(self);
        let at = usize::try_from(at).unwrap_or(usize::MAX).min(len);
        HexBuffer::read(self, at, count.min(len - at))
    }
}

/// How often your templates folder is looked at for templates added or saved.
const REFRESH_SECONDS: f64 = 1.5;
/// Lists with more entries than this start closed.
const OPEN_UP_TO: usize = 64;

/// Which template the panel reads the file with.
#[derive(Clone, Debug, PartialEq, Eq, Default)]
enum Choice {
    /// The first that fits.
    #[default]
    Automatic,
    /// This one, by its file's name and whether it's one of yours.
    Template(String, bool),
    Off,
}

/// The file as last read: for which document, at which change, with which template.
struct Reading {
    doc: u64,
    version: u64,
    entry: Option<usize>,
    tree: Option<Tree>,
}

#[derive(Default)]
pub struct StructurePanel {
    library: Option<Library>,
    choice: Choice,
    reading: Option<Reading>,
    /// The node chosen in the tree.
    selected: Option<usize>,
    /// The nodes opened in the tree.
    open: HashSet<usize>,
    /// The hex selection last seen, to notice the cursor moving.
    last_selection: Option<(usize, usize)>,
    /// A node to bring into view, after the cursor moved to it.
    reveal: Option<usize>,
    checked_at: f64,
    message: Option<String>,
    /// How far down the tree is scrolled.
    scrolled: f32,
}

/// Where the hex selection is, from its start up to its end.
fn selection(hex: &HexBuffer) -> (usize, usize) {
    (hex.caret.min(hex.anchor), hex.caret.max(hex.anchor))
}

fn ending_of(doc: &DocView<'_>) -> String {
    let name = doc.path.and_then(|p| p.file_name()).map_or_else(|| doc.name.clone(), |n| n.to_string_lossy().into_owned());
    name.rsplit_once('.').map(|(_, ending)| ending.to_lowercase()).unwrap_or_default()
}

impl StructurePanel {
    fn library(&mut self) -> &mut Library {
        self.library.get_or_insert_with(Library::default)
    }

    /// The fields the file in front was read into, for the hex view to colour, while the panel is showing them.
    pub fn marks(&self, doc: u64, version: u64) -> Option<&Tree> {
        self.reading.as_ref().filter(|r| r.doc == doc && r.version == version).and_then(|r| r.tree.as_ref())
    }

    /// Reads the file again if it, the template, or the choice of template has changed since.
    fn update(&mut self, doc: &DocView<'_>, now: f64) {
        if now - self.checked_at > REFRESH_SECONDS {
            self.checked_at = now;
            if self.library().refresh() {
                self.reading = None;
            }
        }
        let version = doc.hex.version();
        if self.reading.as_ref().is_some_and(|r| r.doc == doc.id && r.version == version) {
            return;
        }
        let same_doc = self.reading.as_ref().is_some_and(|r| r.doc == doc.id);
        let ending = ending_of(doc);
        let choice = self.choice.clone();
        let library = self.library();
        let entry = match &choice {
            Choice::Automatic => library.choose(doc.hex, &ending),
            Choice::Template(file, own) => library.find(file, *own),
            Choice::Off => None,
        };
        let tree = entry.and_then(|i| library.entries[i].template.as_ref().ok()).map(|t| t.read(doc.hex));
        if !same_doc || self.reading.as_ref().is_some_and(|r| r.entry != entry) {
            self.open.clear();
            self.selected = None;
            if let Some(tree) = &tree {
                // The file's own fields start open, and the structs among them that aren't long lists.
                for &child in &tree.nodes[0].children {
                    if tree.nodes[child].children.len() <= OPEN_UP_TO {
                        self.open.insert(child);
                    }
                }
                self.open.insert(0);
            }
        }
        self.reading = Some(Reading { doc: doc.id, version, entry, tree });
        self.last_selection = None;
    }

    pub fn draw(&mut self, ui: &mut Ui, doc: &DocView<'_>, actions: &mut Vec<Action>) {
        if doc.no_file {
            ui.label(RichText::new(crate::i18n::tr("No file open")).color(theme::text_faint()));
            return;
        }
        if !doc.hex_mode {
            ui.label(RichText::new(crate::i18n::tr("The Structure panel takes a file's bytes apart with a template, so it works in hex mode.")).color(theme::text_faint()).size(12.5));
            if ui.button(crate::i18n::tr("Show as hex")).clicked() {
                actions.push(Action::ViewHex);
            }
            return;
        }
        let now = ui.input(|i| i.time);
        self.update(doc, now);
        self.choice_row(ui, doc, actions);
        if let Some(message) = &self.message {
            ui.label(RichText::new(message).color(theme::problem_mark()).size(12.0));
        }
        self.own_mistakes(ui, actions);
        let Some(reading) = &self.reading else {
            return;
        };
        let Some(entry) = reading.entry else {
            let words = if self.choice == Choice::Off { crate::i18n::tr("No template is being used.") } else { crate::i18n::tr("No template fits this file. Choose one above, or write your own.") };
            ui.label(RichText::new(words).color(theme::text_faint()).size(12.5));
            return;
        };
        let library = self.library.as_ref().expect("read above");
        let item = &library.entries[entry];
        let (file, path) = (item.file.clone(), item.path.clone());
        if let Err(p) = &item.template {
            Self::mistake(ui, &file, p.line, &p.message, path.as_deref(), actions);
            return;
        }
        let Some(tree) = self.reading.as_ref().and_then(|r| r.tree.as_ref()) else {
            return;
        };
        if let Some(p) = &tree.error {
            Self::mistake(ui, &file, p.line, &p.message, path.as_deref(), actions);
        }
        self.follow_cursor(doc.hex);
        self.tree_rows(ui, actions);
    }

    /// The template in use, and the others to choose from, with what can be done with templates.
    fn choice_row(&mut self, ui: &mut Ui, doc: &DocView<'_>, actions: &mut Vec<Action>) {
        let entry = self.reading.as_ref().and_then(|r| r.entry);
        let library = self.library.as_ref().expect("made in update");
        let current = match (&self.choice, entry) {
            (Choice::Off, _) => crate::i18n::tr("None").to_string(),
            (Choice::Automatic, Some(i)) => crate::i18n::tf!("Automatic: {}", library.entries[i].title()),
            (Choice::Automatic, None) => crate::i18n::tr("Automatic: none fits").to_string(),
            (Choice::Template(..), Some(i)) => library.entries[i].title(),
            (Choice::Template(file, _), None) => file.clone(),
        };
        let mut chosen = None;
        let mut act = None;
        ui.horizontal(|ui| {
            ui.label(crate::i18n::tr("Template"));
            egui::ComboBox::from_id_salt("structure-template").selected_text(current).width((ui.available_width() - 34.0).max(80.0)).show_ui(ui, |ui| {
                if ui.selectable_label(self.choice == Choice::Automatic, crate::i18n::tr("Automatic, by the file's ending and contents")).clicked() {
                    chosen = Some(Choice::Automatic);
                }
                if ui.selectable_label(self.choice == Choice::Off, crate::i18n::tr("None")).clicked() {
                    chosen = Some(Choice::Off);
                }
                ui.separator();
                for item in &library.entries {
                    let choice = Choice::Template(item.file.clone(), item.is_own());
                    let mut words = item.title();
                    if item.is_own() {
                        words = crate::i18n::tf!("{words} (yours)");
                    }
                    let fits = item.template.as_ref().is_ok_and(|t| t.fits(doc.hex, &ending_of(doc)));
                    let text = if fits { RichText::new(words).strong() } else { RichText::new(words) };
                    let response = ui.selectable_label(self.choice == choice, text);
                    let response = if item.template.is_err() { response.on_hover_text(crate::i18n::tr("This template has a mistake in it")) } else { response };
                    if response.clicked() {
                        chosen = Some(choice);
                    }
                }
            });
            crate::fit::menu_button(ui, "\u{2026}", |ui| {
                if ui.button(crate::i18n::tr("Change this template\u{2026}")).on_hover_text(crate::i18n::tr("Opens it to edit; a built-in one is copied to your templates folder first")).clicked() {
                    act = Some(1);
                    ui.close();
                }
                if ui.button(crate::i18n::tr("New template of your own")).clicked() {
                    act = Some(2);
                    ui.close();
                }
                if ui.button(crate::i18n::tr("Open your templates folder")).clicked() {
                    act = Some(3);
                    ui.close();
                }
                ui.separator();
                if ui.button(crate::i18n::tr("How templates are written")).clicked() {
                    act = Some(4);
                    ui.close();
                }
            });
        });
        if let Some(choice) = chosen {
            self.choice = choice;
            self.reading = None;
        }
        self.message = None;
        match act {
            Some(1) => match entry.map(|i| self.library().copy_to_own(i)) {
                Some(Ok(path)) => {
                    // From now on the copy is the one used, so changes to it show at once.
                    if let Some(file) = path.file_name().map(|n| n.to_string_lossy().into_owned()) {
                        self.choice = Choice::Template(file, true);
                    }
                    self.reading = None;
                    actions.push(Action::OpenPath(path));
                }
                Some(Err(e)) => self.message = Some(e),
                None => self.message = Some(crate::i18n::tr("Choose a template first.").into()),
            },
            Some(2) => match self.library().new_own() {
                Ok(path) => {
                    if let Some(file) = path.file_name().map(|n| n.to_string_lossy().into_owned()) {
                        self.choice = Choice::Template(file, true);
                    }
                    self.reading = None;
                    actions.push(Action::OpenPath(path));
                }
                Err(e) => self.message = Some(e),
            },
            Some(3) => {
                let folder = self.library().folder().map(std::path::Path::to_path_buf);
                let opened = folder.ok_or_else(|| crate::i18n::tr("There's no data folder to keep templates in.").to_string()).and_then(|folder| {
                    std::fs::create_dir_all(&folder).map_err(|e| e.to_string())?;
                    crate::platform::open_folder(&folder)
                });
                if let Err(e) = opened {
                    self.message = Some(e);
                }
            }
            Some(4) => actions.push(Action::StructureHelp),
            _ => {}
        }
    }

    /// Your own templates that have mistakes in them, each with its line, which opens it there.
    fn own_mistakes(&self, ui: &mut Ui, actions: &mut Vec<Action>) {
        let Some(library) = &self.library else {
            return;
        };
        let current = self.reading.as_ref().and_then(|r| r.entry);
        for (i, item) in library.entries.iter().enumerate() {
            if let (Err(p), true) = (&item.template, item.is_own() && Some(i) != current) {
                Self::mistake(ui, &item.file, p.line, &p.message, item.path.as_deref(), actions);
            }
        }
    }

    fn mistake(ui: &mut Ui, file: &str, line: usize, message: &str, path: Option<&std::path::Path>, actions: &mut Vec<Action>) {
        let words = crate::i18n::tf!("{file}, line {line}: {message}");
        let response = ui.add(egui::Label::new(RichText::new(words).color(theme::problem_mark()).size(12.0)).sense(Sense::click()));
        if let Some(path) = path
            && response.on_hover_text(crate::i18n::tr("Click to go to the line")).clicked()
        {
            actions.push(Action::OpenPath(path.to_path_buf()));
            actions.push(Action::GoToLine(line.saturating_sub(1)));
        }
    }

    /// When the hex cursor moves, the field it's now in is chosen in the tree, and its structs opened to show it.
    fn follow_cursor(&mut self, hex: &HexBuffer) {
        let now = selection(hex);
        if self.last_selection == Some(now) {
            return;
        }
        self.last_selection = Some(now);
        let Some(tree) = self.reading.as_ref().and_then(|r| r.tree.as_ref()) else {
            return;
        };
        // A selection that's exactly the chosen node's bytes was made by choosing it, so it stays chosen.
        if let Some(node) = self.selected.and_then(|i| tree.nodes.get(i))
            && (node.start as usize, node.end as usize) == now
        {
            return;
        }
        let Some(found) = tree.field_at(hex.caret as u64) else {
            return;
        };
        self.selected = Some(found);
        let mut at = tree.nodes[found].parent;
        while let Some(i) = at {
            self.open.insert(i);
            at = tree.nodes[i].parent;
        }
        self.reveal = Some(found);
    }

    /// The nodes showing, in order: each open node's children after it.
    fn visible(tree: &Tree, open: &HashSet<usize>) -> Vec<usize> {
        let mut out = Vec::new();
        let mut stack: Vec<usize> = tree.nodes[0].children.iter().rev().copied().collect();
        while let Some(i) = stack.pop() {
            out.push(i);
            if open.contains(&i) {
                stack.extend(tree.nodes[i].children.iter().rev());
            }
        }
        out
    }

    fn tree_rows(&mut self, ui: &mut Ui, actions: &mut Vec<Action>) {
        let Some(tree) = self.reading.as_ref().and_then(|r| r.tree.as_ref()) else {
            return;
        };
        let rows = Self::visible(tree, &self.open);
        let font = FontId::proportional(12.5);
        let row_height = ui.ctx().fonts_mut(|f| f.row_height(&font)) + 5.0;
        let mut scroll = egui::ScrollArea::both().id_salt("structure-scroll").auto_shrink([false, false]);
        if let Some(target) = self.reveal.take()
            && let Some(row) = rows.iter().position(|r| *r == target)
        {
            // Into the middle of the panel, unless it's in view already.
            let height = ui.available_height();
            let offset = self.scrolled;
            let y = row as f32 * row_height;
            if y < offset || y + row_height > offset + height {
                scroll = scroll.vertical_scroll_offset((y - height / 2.0).max(0.0));
            }
        }
        let mut toggled = None;
        let mut clicked = None;
        let name_width = (ui.available_width() * 0.45).clamp(80.0, 220.0);
        let output = scroll.show_rows(ui, row_height, rows.len(), |ui, range| {
            for &i in &rows[range] {
                let node = &tree.nodes[i];
                let width = ui.available_width().max(name_width + 120.0);
                let (rect, response) = ui.allocate_exact_size(vec2(width, row_height), Sense::click());
                let painter = ui.painter_at(rect);
                if self.selected == Some(i) {
                    painter.rect_filled(rect, 2.0, theme::selected());
                } else if response.hovered() {
                    painter.rect_filled(rect, 2.0, theme::hover());
                }
                let indent = rect.min.x + 4.0 + f32::from(node.depth.saturating_sub(1)) * 12.0;
                let middle = rect.center().y;
                let has_children = !node.children.is_empty();
                if has_children {
                    let arrow = if self.open.contains(&i) { "\u{25BE}" } else { "\u{25B8}" };
                    painter.text(pos2(indent + 4.0, middle), Align2::CENTER_CENTER, arrow, font.clone(), theme::text_dim());
                }
                let mut x = indent + 12.0;
                if node.kind == NodeKind::Field {
                    let swatch = Rect::from_center_size(pos2(x + 4.0, middle), vec2(8.0, 8.0));
                    painter.rect_filled(swatch, 2.0, field_colour(node.colour, 0.75));
                    x += 12.0;
                }
                let (name, name_colour) = match node.kind {
                    NodeKind::Warning => ("\u{26A0}".to_string(), theme::rainbow_mistake()),
                    NodeKind::Calc => (node.name.clone(), theme::text_dim()),
                    _ => (node.name.clone(), theme::text()),
                };
                let name_end = (rect.min.x + name_width).max(x + 40.0);
                let name_rect = Rect::from_min_max(pos2(x, rect.min.y), pos2(name_end - 6.0, rect.max.y));
                ui.painter_at(name_rect).text(pos2(x, middle), Align2::LEFT_CENTER, name, font.clone(), name_colour);
                let value_colour = match node.kind {
                    NodeKind::Warning => theme::rainbow_mistake(),
                    NodeKind::Struct | NodeKind::Array => theme::text_faint(),
                    _ => theme::text_dim(),
                };
                let value_x = if node.kind == NodeKind::Warning { x + 16.0 } else { name_end };
                painter.text(pos2(value_x, middle), Align2::LEFT_CENTER, &node.shown, font.clone(), value_colour);
                let arrow_hit = has_children && response.interact_pointer_pos().is_some_and(|p| p.x < indent + 12.0);
                if response.double_clicked() || (response.clicked() && arrow_hit) {
                    toggled = Some(i);
                } else if response.clicked() {
                    clicked = Some(i);
                }
                response.on_hover_ui(|ui| {
                    let size = node.end - node.start;
                    let mut words = crate::i18n::tf!("{}\n{}  at {} (0x{:X}), {size} bytes", tree.path(i), node.type_name, node.start, node.start);
                    if matches!(node.kind, NodeKind::Calc | NodeKind::Warning) {
                        words = tree.path(i);
                    }
                    if !node.note.is_empty() {
                        words.push('\n');
                        words.push_str(&node.note);
                    }
                    ui.label(words);
                });
            }
        });
        self.scrolled = output.state.clamped_offset().y;
        if let Some(i) = toggled
            && !self.open.remove(&i)
        {
            self.open.insert(i);
        }
        if let Some(action) = clicked.and_then(|i| self.choose_node(i)) {
            actions.push(action);
        }
    }

    /// A node clicked in the tree: chosen, and its bytes (if it has any) selected in the hex view.
    pub fn choose_node(&mut self, i: usize) -> Option<Action> {
        let node = self.tree()?.nodes.get(i)?;
        let range = (node.end > node.start).then_some((node.start as usize, node.end as usize));
        self.selected = Some(i);
        range.map(|(start, end)| Action::SelectBytes { start, end })
    }

    /// The fields the file was last read into, whichever file it was.
    pub fn tree(&self) -> Option<&Tree> {
        self.reading.as_ref().and_then(|r| r.tree.as_ref())
    }

    #[cfg(test)]
    pub fn selected(&self) -> Option<usize> {
        self.selected
    }
}

/// A field's colour, from the tag colours, laid over the background as strongly as asked.
pub fn field_colour(colour: u8, strength: f32) -> Color32 {
    theme::mix(theme::editor_back(), theme::tag_colour(colour + 3), strength)
}

/// Draws a field's colour behind a byte in the hex view, and a line where a field starts.
pub fn paint_byte(painter: &egui::Painter, tree: &Tree, offset: usize, hex: Rect, ascii: Rect) {
    let Some(i) = tree.field_at(offset as u64) else {
        return;
    };
    let node = &tree.nodes[i];
    let back = field_colour(node.colour, 0.28);
    // The gap after a hex pair is filled too, unless the field ends there, so a field reads as one block.
    let wide = if (offset as u64) + 1 < node.end && !(offset + 1).is_multiple_of(crate::hex_buffer::bytes_per_row()) { Rect::from_min_max(hex.min, pos2(hex.max.x + hex.width() / 2.0, hex.max.y)) } else { hex };
    painter.rect_filled(wide, 0.0, back);
    painter.rect_filled(ascii, 0.0, back);
    if offset as u64 == node.start {
        painter.vline(hex.min.x, hex.y_range(), Stroke::new(1.0, field_colour(node.colour, 0.9)));
    }
}

/// The help page on writing templates.
pub fn help() -> String {
    crate::structure_library::LANGUAGE.replace("\r\n", "\n")
}
