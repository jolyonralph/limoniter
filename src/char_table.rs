//! The Character table: the old ASCII table, now following the document's own encoding.
//! Everything it shows comes from the same definitions the editor opens and saves with.

use crate::charset::{self, CellState, Charset, Kind, Model, raw_byte, raw_char};
use crate::codec::{self, Display, TextFormat};
use crate::console::{self, Form};
use crate::panels::{Action, DocView};
use crate::text_buffer::TextBuffer;
use crate::theme;
use eframe::egui::{self, Align2, Color32, FontId, RichText, Sense, Ui, pos2, vec2};

/// Which keyboard's sequences the table lists.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum KeyOs {
    Windows,
    Mac,
    Linux,
}

const THIS_OS: KeyOs = if cfg!(target_os = "macos") {
    KeyOs::Mac
} else if cfg!(target_os = "windows") {
    KeyOs::Windows
} else {
    KeyOs::Linux
};

/// The keys that type a character. Control codes keep their Ctrl sequence on every system.
/// On Windows a plain Alt+number uses the older OEM character set, which only agrees from 32 to 126,
/// so codes from 128 up need the leading 0 of Alt+0NNN, and those type Windows-1252.
pub fn key_sequence(byte: u8, c: Option<char>, os: KeyOs) -> String {
    let Some(c) = c else {
        return String::new();
    };
    let value = c as u32;
    if value < 32 && value == u32::from(byte) {
        return format!("^{}", char::from(byte + 64));
    }
    if value == 127 {
        return "^?".to_string();
    }
    if value < 32 || (0x80..0xA0).contains(&value) || raw_byte(c).is_some() {
        return String::new();
    }
    match os {
        KeyOs::Windows if (32..127).contains(&value) => format!("Alt+{value}"),
        KeyOs::Windows => charset::known("windows-1252").table(0).and_then(|table| table.byte_of(c)).map_or_else(String::new, |code| format!("Alt+{code:04}")),
        // The Unicode Hex Input keyboard.
        KeyOs::Mac => format!("Opt+{value:04X}"),
        // Unicode entry in GTK and IBus: the hex digits, then Enter or Space.
        KeyOs::Linux => format!("C+S+U {value:X}"),
    }
}

/// Common Unicode blocks to browse, when a document's encoding has no table of bytes.
pub const UNICODE_BLOCKS: [(&str, u32, u32); 26] = [
    ("Basic Latin", 0x0000, 0x007F),
    ("Latin-1 Supplement", 0x0080, 0x00FF),
    ("Latin Extended-A", 0x0100, 0x017F),
    ("Latin Extended-B", 0x0180, 0x024F),
    ("Greek and Coptic", 0x0370, 0x03FF),
    ("Cyrillic", 0x0400, 0x04FF),
    ("Hebrew", 0x0590, 0x05FF),
    ("Arabic", 0x0600, 0x06FF),
    ("General Punctuation", 0x2000, 0x206F),
    ("Currency Symbols", 0x20A0, 0x20CF),
    ("Letterlike Symbols", 0x2100, 0x214F),
    ("Arrows", 0x2190, 0x21FF),
    ("Mathematical Operators", 0x2200, 0x22FF),
    ("Miscellaneous Technical", 0x2300, 0x23FF),
    ("Box Drawing", 0x2500, 0x257F),
    ("Block Elements", 0x2580, 0x259F),
    ("Geometric Shapes", 0x25A0, 0x25FF),
    ("Miscellaneous Symbols", 0x2600, 0x26FF),
    ("Dingbats", 0x2700, 0x27BF),
    ("CJK Symbols and Punctuation", 0x3000, 0x303F),
    ("Hiragana", 0x3040, 0x309F),
    ("Katakana", 0x30A0, 0x30FF),
    ("Halfwidth and Fullwidth Forms", 0xFF00, 0xFFEF),
    ("Symbols for Legacy Computing", 0x1FB00, 0x1FBFF),
    ("Emoticons", 0x1F600, 0x1F64F),
    ("Miscellaneous Symbols and Pictographs", 0x1F300, 0x1F5FF),
];

#[derive(Default)]
pub struct TableState {
    /// Browsing another set. The document's own encoding doesn't change.
    browse: Option<&'static Charset>,
    selected: Option<u8>,
    selected_char: Option<char>,
    /// A display state picked by hand; None follows the document's starting state.
    manual_state: Option<usize>,
    follow_cursor: bool,
    /// What the table last scrolled to for the cursor, so it only moves when the cursor does.
    followed: Option<(u64, u32)>,
    block: usize,
    pub sequence_form: Option<Form>,
    /// The document last shown, so that switching documents starts from that document's own set.
    shown_doc: Option<u64>,
}

/// Machines and regions that are variants of one another, shown as their own selector.
fn sibling_sets(set: &Charset) -> Vec<&'static Charset> {
    let prefix = |id: &str| id.split('-').next().unwrap_or(id).to_string();
    if !matches!(set.group.as_str(), "Commodore" | "Teletext" | "MSX" | "Atari") {
        return Vec::new();
    }
    let own = prefix(&set.id);
    charset::all().iter().copied().filter(|other| other.group == set.group && prefix(&other.id) == own).collect()
}

pub fn type_text(entry: &charset::Entry) -> &'static str {
    entry.kind.describe()
}

/// What the name column says: a short control name, or nothing for ordinary characters as in the original.
fn short_name(set: &Charset, byte: u8, entry: &charset::Entry) -> String {
    match entry.kind {
        Kind::Printable if entry.unicode == Some(' ') => "Sp".to_string(),
        Kind::Printable => String::new(),
        Kind::Undefined => "undefined".to_string(),
        _ => {
            let label = set.label(byte);
            let inner = label.trim_start_matches('[').trim_end_matches(']');
            if inner.starts_with("0x") { entry.name.clone() } else { inner.to_string() }
        }
    }
}

/// A character's full description, for the details box and tooltips.
fn full_name(entry: &charset::Entry) -> String {
    if !entry.name.is_empty() {
        return entry.name.clone();
    }
    match (entry.kind, entry.unicode) {
        (Kind::Undefined, _) => "Not defined in this set".to_string(),
        (_, Some(c)) => charset::char_name(c),
        _ => String::new(),
    }
}

fn unicode_text(entry: &charset::Entry) -> String {
    // Controls are given placeholder values in some tables; they aren't really those characters.
    match (entry.kind, entry.unicode) {
        (Kind::Printable, Some(c)) => format!("U+{:04X}", c as u32),
        (Kind::Control, Some(c)) if charset::control_name(c as u32).is_some() => format!("U+{:04X}", c as u32),
        _ => "None".to_string(),
    }
}

/// Draws a table glyph in a cell: inverse where the set draws it so, a visible fallback if no font has it.
fn paint_glyph(ui: &Ui, center_left: egui::Pos2, entry: &charset::Entry, row_h: f32, font: &FontId) {
    let Some(c) = entry.unicode.filter(|_| entry.kind == Kind::Printable) else {
        if entry.kind == Kind::Control || entry.kind == Kind::StateDependent {
            ui.painter().text(center_left, Align2::LEFT_TOP, "\u{25AF}", font.clone(), theme::text_dim());
        }
        return;
    };
    if c == ' ' {
        return;
    }
    let width = 9.0;
    let cell = egui::Rect::from_min_size(center_left, vec2(width, row_h - 1.0));
    if !ui.ctx().fonts_mut(|fonts| fonts.has_glyph(font, c)) {
        ui.painter().rect_stroke(cell, 1.0, egui::Stroke::new(1.0, theme::text_dim()), egui::StrokeKind::Inside);
        return;
    }
    if entry.inverse {
        ui.painter().rect_filled(cell, 0.0, theme::text());
        ui.painter().text(center_left, Align2::LEFT_TOP, c, font.clone(), theme::window());
    } else {
        ui.painter().text(center_left, Align2::LEFT_TOP, c, font.clone(), theme::text());
    }
}

/// What's at the cursor, as bytes, for linking the table to the editor.
pub struct CaretBytes {
    pub bytes: Vec<u8>,
    pub character: Option<char>,
}

pub fn caret_bytes(doc: &DocView<'_>, state: usize) -> CaretBytes {
    if doc.hex_mode {
        let byte = (doc.hex.caret < doc.hex.len()).then(|| doc.hex.byte(doc.hex.caret));
        return CaretBytes { bytes: byte.into_iter().collect(), character: None };
    }
    let Some(c) = doc.text.char_at_caret() else {
        return CaretBytes { bytes: Vec::new(), character: None };
    };
    let bytes = codec::char_bytes(c, doc.format.charset, state).unwrap_or_default();
    CaretBytes { bytes, character: raw_byte(c).is_none().then_some(c) }
}

fn cursor_state(text: &TextBuffer) -> CellState {
    text.state_at_caret()
}

/// The table to show, from a hand-picked state, the cursor, or the document's start.
fn state_in_use(state: &TableState, set: &Charset, doc: &DocView<'_>, same_set: bool) -> usize {
    if state.follow_cursor && same_set && !doc.hex_mode {
        return cursor_state(doc.text).table.min(set.tables.len().saturating_sub(1));
    }
    if let Some(chosen) = state.manual_state {
        return chosen.min(set.tables.len().saturating_sub(1));
    }
    if same_set { doc.format.state.min(set.tables.len().saturating_sub(1)) } else { 0 }
}

pub fn heading_text(format: &TextFormat, browse: Option<&Charset>, table_title: Option<&str>) -> String {
    let set = browse.unwrap_or(format.charset);
    let mut text = set.name.clone();
    if let Some(title) = table_title {
        text = format!("{text}, {title}");
    }
    if browse.is_some() {
        text = format!("Browsing {text}");
    }
    text
}

pub fn panel(ui: &mut Ui, state: &mut TableState, doc: &DocView<'_>, show_sequences: &mut bool, actions: &mut Vec<Action>) {
    if state.shown_doc != Some(doc.id) {
        state.shown_doc = Some(doc.id);
        state.browse = None;
        state.manual_state = None;
        state.selected = None;
        state.selected_char = None;
        state.sequence_form = None;
    }
    let set = state.browse.unwrap_or(doc.format.charset);
    let same_set = state.browse.is_none();
    let table_index = state_in_use(state, set, doc, same_set);
    let title = set.table(table_index).filter(|_| set.tables.len() > 1).map(|table| table.title.as_str());
    let heading = heading_text(doc.format, state.browse, title);
    let color = if same_set { theme::text() } else { theme::ruler_mark() };
    ui.add(egui::Label::new(RichText::new(heading).size(12.0).color(color)).wrap()).on_hover_text(format!("{}\n\nScope: {}\nMapping source: {}\nVersion: {}", set.name, set.scope, set.source, set.version));

    // Browsing another set never changes the document's encoding.
    let mut browse_choice = None;
    egui::ComboBox::from_id_salt("table-browse")
        .width(ui.available_width() - 8.0)
        .truncate()
        .selected_text(RichText::new(if same_set { "Follow this document" } else { "Browsing only" }).size(12.0))
        .height(420.0)
        .show_ui(ui, |ui| {
            if ui.selectable_label(same_set, "Follow this document").clicked() {
                browse_choice = Some(None);
            }
            for group in charset::groups() {
                ui.label(RichText::new(group).color(theme::text_dim()).size(11.5));
                for other in charset::all().iter().copied().filter(|other| other.group == group) {
                    if ui.selectable_label(state.browse.is_some_and(|b| b.id == other.id), &other.name).clicked() {
                        browse_choice = Some(Some(other));
                    }
                }
            }
        });
    if let Some(choice) = browse_choice {
        state.browse = choice;
        state.manual_state = None;
        state.selected = None;
    }
    variant_selectors(ui, state, set, same_set, doc);

    if set.holds_characters() {
        unicode_browser(ui, state, doc, actions);
    } else {
        byte_table(ui, state, doc, set, table_index, same_set, actions);
    }
    ui.add_space(3.0);
    ui.horizontal(|ui| {
        let insert = ui.add_enabled(state.selected.is_some() || state.selected_char.is_some(), egui::Button::new("Insert char")).on_hover_text("Or double-click a row");
        if insert.clicked() {
            push_insert(state, set, table_index, actions);
        }
        if ui.button("Insert byte").on_hover_text("Type a byte's value in hexadecimal").clicked() {
            actions.push(Action::AskInsertByte);
        }
        ui.toggle_value(show_sequences, "Sequences").on_hover_text("Multi-byte control sequences, in their own panel");
    });
}

fn push_insert(state: &TableState, set: &'static Charset, table: usize, actions: &mut Vec<Action>) {
    if set.holds_characters() {
        if let Some(c) = state.selected_char {
            actions.push(Action::InsertCharacter(c));
        }
    } else if let Some(byte) = state.selected {
        actions.push(Action::InsertEntry { set, table, byte });
    }
}

fn variant_selectors(ui: &mut Ui, state: &mut TableState, set: &'static Charset, same_set: bool, doc: &DocView<'_>) {
    let siblings = sibling_sets(set);
    if siblings.len() > 1 {
        let label = if set.group == "Teletext" { "National set" } else if set.group == "MSX" { "Region" } else { "Machine" };
        let mut chosen = None;
        egui::ComboBox::from_id_salt("table-variant")
            .width(ui.available_width() - 8.0)
            .truncate()
            .selected_text(RichText::new(format!("{label}: {}", set.name)).size(12.0))
            .height(360.0)
            .show_ui(ui, |ui| {
                for other in &siblings {
                    if ui.selectable_label(other.id == set.id, &other.name).clicked() {
                        chosen = Some(*other);
                    }
                }
            });
        if let Some(other) = chosen {
            if other.id != doc.format.charset.id || !same_set {
                state.browse = (other.id != doc.format.charset.id).then_some(other);
            }
            state.selected = None;
        }
    }
    if set.tables.len() > 1 {
        let current = state_in_use(state, set, doc, same_set);
        let mut chosen = None;
        ui.add_enabled_ui(!state.follow_cursor || !same_set, |ui| {
            egui::ComboBox::from_id_salt("table-state").width(ui.available_width() - 8.0).truncate().selected_text(RichText::new(&set.tables[current].title).size(12.0)).show_ui(ui, |ui| {
                for (index, table) in set.tables.iter().enumerate() {
                    if ui.selectable_label(index == current, &table.title).clicked() {
                        chosen = Some(index);
                    }
                }
            });
        });
        if chosen.is_some() {
            state.manual_state = chosen;
        }
        if matches!(set.model, Model::Petscii | Model::Teletext | Model::Mode7) && same_set {
            ui.checkbox(&mut state.follow_cursor, RichText::new("Follow cursor state").size(12.0));
        }
    }
}

const COLUMNS: [(&str, f32); 5] = [("Char", 0.0), ("Dec#", 0.15), ("Hex#", 0.3), ("Name", 0.45), ("Ctrl char", 0.62)];
/// Wide enough to add the Unicode and type columns beside the original's five.
const ROOMY_WIDTH: f32 = 430.0;
const DETAILS_HEIGHT: f32 = 92.0;

fn byte_table(ui: &mut Ui, state: &mut TableState, doc: &DocView<'_>, set: &'static Charset, table_index: usize, same_set: bool, actions: &mut Vec<Action>) {
    let font = FontId::monospace(12.0);
    let row_h = 15.0;
    let width = ui.available_width();
    let roomy = width >= ROOMY_WIDTH;
    let share = if roomy { 0.72 } else { 1.0 };
    let (header, _) = ui.allocate_exact_size(vec2(width, 16.0), Sense::hover());
    for (title, at) in COLUMNS {
        ui.painter().text(pos2(header.min.x + 2.0 + header.width() * at * share, header.min.y), Align2::LEFT_TOP, title, FontId::proportional(11.5), theme::text_dim());
    }
    if roomy {
        for (title, at) in [("Unicode", 0.72), ("Type", 0.84)] {
            ui.painter().text(pos2(header.min.x + 2.0 + header.width() * at, header.min.y), Align2::LEFT_TOP, title, FontId::proportional(11.5), theme::text_dim());
        }
    }

    let caret = caret_bytes(doc, table_index);
    // A one-byte character is that row; a character of several bytes isn't any one row.
    let caret_row = (same_set && caret.bytes.len() == 1).then(|| caret.bytes[0]);
    let table = set.table(table_index);
    let rows = set.row_count();
    let height = (ui.available_height() - DETAILS_HEIGHT - 28.0).max(40.0);
    let mut scroll = egui::ScrollArea::vertical().id_salt("ascii-scroll").auto_shrink([false, false]).max_height(height);
    if let Some(byte) = caret_row {
        let key = (doc.id, u32::from(byte));
        if state.followed != Some(key) {
            state.followed = Some(key);
            scroll = scroll.vertical_scroll_offset((f32::from(byte) * row_h - height / 2.0).max(0.0));
        }
    } else {
        state.followed = None;
    }
    // The rows sit edge to edge, so the scroll area has to count them that way too.
    let spacing = ui.spacing().item_spacing.y;
    ui.spacing_mut().item_spacing.y = 0.0;
    scroll.show_rows(ui, row_h, rows, |ui, range| {
        for index in range {
            let byte = index as u8;
            let Some(entry) = table.map(|table| &table.entries[index]) else {
                continue;
            };
            let (rect, response) = ui.allocate_exact_size(vec2(ui.available_width(), row_h), Sense::click());
            if state.selected == Some(byte) {
                ui.painter().rect_filled(rect, 0.0, theme::selection());
            } else if caret_row == Some(byte) {
                ui.painter().rect_filled(rect, 0.0, theme::other_pane());
            } else if response.hovered() {
                ui.painter().rect_filled(rect, 0.0, theme::hover());
            }
            if response.clicked() {
                state.selected = Some(byte);
                state.selected_char = None;
            }
            if response.double_clicked() {
                actions.push(Action::InsertEntry { set, table: table_index, byte });
            }
            let w = rect.width() * share;
            let y = rect.min.y + 1.0;
            paint_glyph(ui, pos2(rect.min.x + 2.0, y), entry, row_h, &font);
            let name = short_name(set, byte, entry);
            let keys = if entry.kind == Kind::Printable || entry.kind == Kind::Control { key_sequence(byte, entry.unicode, THIS_OS) } else { String::new() };
            let has_keys = !keys.is_empty();
            let name_color = if entry.kind == Kind::Undefined { theme::text_faint() } else { theme::text() };
            for (text, at, color) in [(format!("{byte:02}"), 0.15, theme::text()), (format!("{byte:02X}"), 0.3, theme::text()), (name, 0.45, name_color), (keys, 0.62, theme::text())] {
                if !text.is_empty() {
                    let clip = egui::Rect::from_min_max(pos2(rect.min.x + 2.0 + w * at, rect.min.y), pos2(rect.min.x + w, rect.max.y));
                    // A long name can run on where this row has no keys to show.
                    let clip = if at == 0.45 && !has_keys { egui::Rect::from_min_max(clip.min, pos2(rect.min.x + w * 0.61, rect.max.y)) } else { clip };
                    ui.painter().with_clip_rect(clip).text(pos2(rect.min.x + 2.0 + w * at, y), Align2::LEFT_TOP, text, font.clone(), color);
                }
            }
            if roomy {
                ui.painter().text(pos2(rect.min.x + 2.0 + rect.width() * 0.72, y), Align2::LEFT_TOP, unicode_text(entry), font.clone(), theme::text_dim());
                ui.painter().text(pos2(rect.min.x + 2.0 + rect.width() * 0.84, y), Align2::LEFT_TOP, type_text(entry), font.clone(), theme::text_dim());
            }
            if response.hovered() {
                response.on_hover_text(entry_details(set, byte, entry));
            }
        }
    });
    ui.spacing_mut().item_spacing.y = spacing;
    details_box(ui, state, doc, set, table_index, &caret);
}

fn entry_details(set: &Charset, byte: u8, entry: &charset::Entry) -> String {
    format!("{} (decimal {byte}, hex {byte:02X})\nUnicode: {}\nType: {}\nIn {}", full_name(entry), unicode_text(entry), type_text(entry), set.name)
}

fn details_box(ui: &mut Ui, state: &TableState, doc: &DocView<'_>, set: &Charset, table_index: usize, caret: &CaretBytes) {
    egui::Frame::new().fill(theme::ribbon()).inner_margin(egui::Margin::same(3)).show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.set_height(DETAILS_HEIGHT - 8.0);
        let small = |text: String| RichText::new(text).size(11.5);
        if let (Some(byte), Some(entry)) = (state.selected, state.selected.and_then(|byte| set.entry(table_index, byte))) {
            ui.add(egui::Label::new(small(full_name(entry))).truncate());
            ui.label(small(format!("Dec {byte}  Hex {byte:02X}  {}", unicode_text(entry))));
            ui.label(small(type_text(entry).to_string()).color(theme::text_dim()));
        } else if let Some(c) = state.selected_char {
            ui.add(egui::Label::new(small(charset::char_name(c))).truncate());
            let bytes = codec::char_bytes(c, doc.format.charset, 0).unwrap_or_default().iter().map(|b| format!("{b:02X}")).collect::<Vec<_>>().join(" ");
            ui.label(small(format!("U+{:04X}  In this file: {bytes}", c as u32)));
        }
        let at_cursor = if doc.hex_mode { "Byte at cursor" } else { "At cursor" };
        let bytes = caret.bytes.iter().map(|b| format!("{b:02X}")).collect::<Vec<_>>().join(" ");
        let summary = match (caret.bytes.len(), caret.character) {
            (0, _) => format!("{at_cursor}: end of line"),
            (1, _) => format!("{at_cursor}: byte {bytes}"),
            (_, Some(c)) => format!("{at_cursor}: {bytes} = one character, U+{:04X}", c as u32),
            _ => format!("{at_cursor}: {bytes}"),
        };
        ui.add(egui::Label::new(small(summary).color(theme::ruler_text())).truncate());
        if !doc.hex_mode {
            let selection = doc.text.selection_for_preview();
            if !selection.is_empty() {
                let shown: Vec<String> = selection.chars().flat_map(|c| codec::char_bytes(c, doc.format.charset, doc.format.state).unwrap_or_default()).take(24).map(|b| format!("{b:02X}")).collect();
                ui.add(egui::Label::new(small(format!("Selection: {}", shown.join(" "))).color(theme::ruler_text())).truncate());
            }
        }
    });
}

/// A Unicode encoding has no table of bytes, so this lists characters, in the original's columns.
fn unicode_browser(ui: &mut Ui, state: &mut TableState, doc: &DocView<'_>, actions: &mut Vec<Action>) {
    let caret = caret_bytes(doc, 0);
    // The cursor's character brings up its own block, once each time the cursor moves to a new one.
    let mut scroll_to = None;
    if let Some(c) = caret.character {
        let key = (doc.id, c as u32);
        if state.followed != Some(key) {
            state.followed = Some(key);
            if let Some(found) = block_of(c) {
                state.block = found;
                scroll_to = Some(c as u32 - UNICODE_BLOCKS[found].1);
            }
        }
    } else {
        state.followed = None;
    }
    let mut chosen = None;
    egui::ComboBox::from_id_salt("table-block").width(ui.available_width() - 8.0).truncate().selected_text(RichText::new(UNICODE_BLOCKS[state.block].0).size(12.0)).height(360.0).show_ui(ui, |ui| {
        for (index, (name, _, _)) in UNICODE_BLOCKS.iter().enumerate() {
            if ui.selectable_label(index == state.block, *name).clicked() {
                chosen = Some(index);
            }
        }
    }).response.on_hover_text("A Unicode encoding has no table of bytes, so this browses characters. Dec# and Hex# are code points. The details below list each one's bytes.");
    if let Some(index) = chosen {
        state.block = index;
    }
    let (_, first, last) = UNICODE_BLOCKS[state.block];
    let font = FontId::monospace(12.0);
    let row_h = 15.0;
    let width = ui.available_width();
    let (header, _) = ui.allocate_exact_size(vec2(width, 16.0), Sense::hover());
    for (title, at) in COLUMNS {
        ui.painter().text(pos2(header.min.x + 2.0 + header.width() * at, header.min.y), Align2::LEFT_TOP, title, FontId::proportional(11.5), theme::text_dim());
    }
    let height = (ui.available_height() - DETAILS_HEIGHT - 28.0).max(40.0);
    let spacing = ui.spacing().item_spacing.y;
    ui.spacing_mut().item_spacing.y = 0.0;
    let mut scroll = egui::ScrollArea::vertical().id_salt("unicode-scroll").auto_shrink([false, false]).max_height(height);
    if let Some(row) = scroll_to {
        scroll = scroll.vertical_scroll_offset((row as f32 * row_h - height / 2.0).max(0.0));
    }
    scroll.show_rows(ui, row_h, (last - first + 1) as usize, |ui, range| {
        for index in range {
            let Some(c) = char::from_u32(first + index as u32) else {
                continue;
            };
            let (rect, response) = ui.allocate_exact_size(vec2(ui.available_width(), row_h), Sense::click());
            let is_caret = caret.character == Some(c);
            if state.selected_char == Some(c) {
                ui.painter().rect_filled(rect, 0.0, theme::selection());
            } else if is_caret {
                ui.painter().rect_filled(rect, 0.0, theme::other_pane());
            } else if response.hovered() {
                ui.painter().rect_filled(rect, 0.0, theme::hover());
            }
            if response.clicked() {
                state.selected_char = Some(c);
                state.selected = None;
            }
            if response.double_clicked() {
                actions.push(Action::InsertCharacter(c));
            }
            let y = rect.min.y + 1.0;
            let value = c as u32;
            let name = charset::control_name(value);
            let entry = charset::Entry { unicode: Some(c), kind: if name.is_some() { Kind::Control } else { Kind::Printable }, inverse: false, name: String::new() };
            paint_glyph(ui, pos2(rect.min.x + 2.0, y), &entry, row_h, &font);
            let short = name.map_or_else(|| if c == ' ' { "Sp".to_string() } else { String::new() }, str::to_string);
            let keys = key_sequence(u8::try_from(value).unwrap_or(0), Some(c), THIS_OS);
            let w = rect.width();
            for (text, at) in [(format!("{value:02}"), 0.15), (format!("{value:02X}"), 0.3), (short, 0.45), (keys, 0.62)] {
                if !text.is_empty() {
                    ui.painter().text(pos2(rect.min.x + 2.0 + w * at, y), Align2::LEFT_TOP, text, font.clone(), theme::text());
                }
            }
            if response.hovered() {
                let bytes = codec::char_bytes(c, doc.format.charset, 0).unwrap_or_default().iter().map(|b| format!("{b:02X}")).collect::<Vec<_>>().join(" ");
                response.on_hover_text(format!("{}\nU+{value:04X}\nIn this file: {bytes}", charset::char_name(c)));
            }
        }
    });
    ui.spacing_mut().item_spacing.y = spacing;
    details_box(ui, state, doc, doc.format.charset, 0, &caret);
}

fn block_of(c: char) -> Option<usize> {
    let value = c as u32;
    UNICODE_BLOCKS.iter().position(|(_, first, last)| value >= *first && value <= *last)
}

/// The multi-byte commands a profile defines, kept apart from the table of single bytes.
pub fn sequences_panel(ui: &mut Ui, state: &mut TableState, doc: &DocView<'_>, actions: &mut Vec<Action>) {
    let amiga = doc.format.display == Display::AmigaConsole || doc.format.charset.id == "amiga-text";
    if !amiga {
        ui.label(RichText::new("No multi-byte sequences are defined for this profile.").size(12.0));
        ui.label(RichText::new("Amiga console sequences appear here when the display profile is Amiga console.").size(11.5).color(theme::text_dim()));
        return;
    }
    let form = state.sequence_form.unwrap_or_else(|| used_form(doc));
    ui.label(RichText::new("Introduced by:").size(12.0));
    let mut chosen = form;
    ui.radio_value(&mut chosen, Form::SingleCsi, RichText::new("CSI, byte 9B").size(12.0));
    ui.radio_value(&mut chosen, Form::EscBracket, RichText::new("ESC [, bytes 1B 5B").size(12.0));
    state.sequence_form = Some(chosen);
    egui::ScrollArea::vertical().id_salt("sequence-scroll").auto_shrink([false, false]).show(ui, |ui| {
        let mut group = "";
        for template in &console::AMIGA_TEMPLATES {
            if template.group != group {
                group = template.group;
                ui.label(RichText::new(group).color(theme::text_dim()).size(11.5));
            }
            let bytes = template.bytes(chosen);
            let shown = bytes.iter().map(|b| format!("{b:02X}")).collect::<Vec<_>>().join(" ");
            let button = egui::Button::new(RichText::new(format!("{}  {shown}", template.name)).size(11.5)).wrap_mode(egui::TextWrapMode::Truncate).min_size(vec2(ui.available_width(), 0.0));
            if ui.add(button).on_hover_text(format!("Inserts the bytes {shown}")).clicked() {
                actions.push(Action::InsertBytes(bytes));
            }
        }
    });
}

/// Whichever form the document already uses is offered first.
fn used_form(doc: &DocView<'_>) -> Form {
    if doc.hex_mode || doc.text.is_disk_backed() {
        return Form::SingleCsi;
    }
    for line in doc.text.all_lines().iter().take(2000) {
        if let Some(found) = console::parse_line(line).first() {
            return found.form;
        }
    }
    Form::SingleCsi
}

/// What inserting a table entry puts in the text, for the document's own set.
pub fn text_for_byte(set: &Charset, byte: u8) -> String {
    // A line-ending byte is still inserted as that byte, which is what it asked for.
    let c = set.decoded[usize::from(byte)];
    if set.holds_characters() { raw_char(byte).to_string() } else { c.to_string() }
}

/// The colour a token is drawn in for its kind.
pub fn kind_color(kind: Kind) -> Color32 {
    match kind {
        Kind::Undefined => theme::token_undefined(),
        Kind::StateDependent => theme::token_state(),
        _ => theme::token_back(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Draws the panel with no window, for a document in the given format.
    fn draw(ctx: &egui::Context, state: &mut TableState, id: u64, text: &TextBuffer, hex: &crate::hex_buffer::HexBuffer, sequences: bool) -> Vec<Action> {
        let view = DocView { id, text, hex, format: &text.format, path: None, hex_mode: false, name: "x".into(), language: String::new(), no_file: false, syntax: None, image: None };
        let raw = egui::RawInput { screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, vec2(420.0, 800.0))), ..Default::default() };
        let mut actions = Vec::new();
        let mut show = sequences;
        let mut output = ctx.run_ui(raw, |ui| {
            egui::CentralPanel::default().show(ui, |ui| {
                if sequences {
                    sequences_panel(ui, state, &view, &mut actions);
                } else {
                    panel(ui, state, &view, &mut show, &mut actions);
                }
            });
        });
        output.textures_delta.clear();
        actions
    }

    fn buffer_of(id: &str) -> TextBuffer {
        let mut text = TextBuffer::default();
        text.format = TextFormat { charset: charset::known(id), ..TextFormat::default() };
        crate::text_buffer::set_cell_profile(text.format.charset);
        text.set_content(vec!["abc".to_string()]);
        text
    }

    #[test]
    fn every_set_draws_in_the_table_and_browsing_ends_when_the_document_changes() {
        let ctx = egui::Context::default();
        let hex = crate::hex_buffer::HexBuffer::default();
        let mut state = TableState::default();
        for (number, set) in charset::all().iter().enumerate() {
            let text = buffer_of(&set.id);
            for _ in 0..2 {
                assert!(draw(&ctx, &mut state, number as u64, &text, &hex, false).is_empty(), "looking at a table inserts nothing");
            }
        }
        let text = buffer_of("cp437");
        state.browse = Some(charset::known("atascii"));
        state.shown_doc = Some(500);
        draw(&ctx, &mut state, 500, &text, &hex, false);
        assert!(state.browse.is_some(), "still browsing in the same document");
        assert_eq!(text.format.charset.id, "cp437", "browsing leaves the document's encoding alone");
        draw(&ctx, &mut state, 501, &text, &hex, false);
        assert!(state.browse.is_none(), "another document starts from its own set");
    }

    #[test]
    fn the_sequence_panel_draws_for_amiga_and_says_so_otherwise() {
        let ctx = egui::Context::default();
        let hex = crate::hex_buffer::HexBuffer::default();
        let mut state = TableState::default();
        let mut text = buffer_of("amiga-text");
        text.format.display = Display::AmigaConsole;
        draw(&ctx, &mut state, 1, &text, &hex, true);
        assert_eq!(state.sequence_form, Some(Form::SingleCsi));
        let plain = buffer_of("utf-8");
        draw(&ctx, &mut state, 2, &plain, &hex, true);
    }

    #[test]
    fn the_browser_finds_the_block_of_the_character_at_the_cursor() {
        assert_eq!(block_of('A').map(|i| UNICODE_BLOCKS[i].0), Some("Basic Latin"));
        assert_eq!(block_of('\u{20AC}').map(|i| UNICODE_BLOCKS[i].0), Some("Currency Symbols"));
        assert_eq!(block_of('\u{1FB70}').map(|i| UNICODE_BLOCKS[i].0), Some("Symbols for Legacy Computing"));
        assert_eq!(block_of('\u{E000}'), None);
    }

    #[test]
    fn keys_follow_the_character_not_the_row() {
        for os in [KeyOs::Windows, KeyOs::Mac, KeyOs::Linux] {
            assert_eq!(key_sequence(0, Some('\0'), os), "^@", "control codes keep Ctrl on {os:?}");
            assert_eq!(key_sequence(1, Some('\u{1}'), os), "^A");
            assert_eq!(key_sequence(9, Some('\t'), os), "^I");
            assert_eq!(key_sequence(31, Some('\u{1F}'), os), "^_");
            assert_eq!(key_sequence(127, Some('\u{7F}'), os), "^?");
        }
        assert_eq!(key_sequence(32, Some(' '), KeyOs::Windows), "Alt+32");
        assert_eq!(key_sequence(65, Some('A'), KeyOs::Windows), "Alt+65");
        assert_eq!(key_sequence(126, Some('~'), KeyOs::Windows), "Alt+126");
        assert_eq!(key_sequence(128, Some('\u{20AC}'), KeyOs::Windows), "Alt+0128");
        assert_eq!(key_sequence(252, Some('\u{FC}'), KeyOs::Windows), "Alt+0252");
        assert_eq!(key_sequence(252, Some('\u{FC}'), KeyOs::Mac), "Opt+00FC");
        assert_eq!(key_sequence(128, Some('\u{20AC}'), KeyOs::Mac), "Opt+20AC");
        assert_eq!(key_sequence(252, Some('\u{FC}'), KeyOs::Linux), "C+S+U FC");
        assert_eq!(key_sequence(128, Some('\u{20AC}'), KeyOs::Linux), "C+S+U 20AC");
        // CP437 byte 0x82 is é: the keys type that character, whichever row it's on.
        assert_eq!(key_sequence(0x82, Some('\u{E9}'), KeyOs::Windows), "Alt+0233");
        assert_eq!(key_sequence(0xB0, Some('\u{2591}'), KeyOs::Windows), "", "no Alt code types a shade block");
        assert_eq!(key_sequence(0x81, None, KeyOs::Windows), "");
    }

    #[test]
    fn the_table_shows_what_each_set_says() {
        let csi = charset::known("amiga-text");
        assert_eq!(short_name(csi, 0x9B, csi.entry(0, 0x9B).unwrap()), "CSI");
        let eol = charset::known("atascii");
        assert_eq!(short_name(eol, 0x9B, eol.entry(0, 0x9B).unwrap()), "END OF LINE");
        let dos = charset::known("cp437");
        assert_eq!(unicode_text(dos.entry(0, 0xE9).unwrap()), "U+0398");
        let win = charset::known("windows-1252");
        assert_eq!(unicode_text(win.entry(0, 0xE9).unwrap()), "U+00E9");
        assert_eq!(short_name(win, 0x81, win.entry(0, 0x81).unwrap()), "undefined");
        assert_eq!(type_text(win.entry(0, 0x81).unwrap()), "Undefined");
        let zx = charset::known("zx-spectrum");
        assert_eq!(zx.entry(0, 0xE7).unwrap().kind, Kind::StateDependent);
        assert_eq!(short_name(zx, 0xE7, zx.entry(0, 0xE7).unwrap()), "BORDER");
        assert_eq!(full_name(dos.entry(0, 0x41).unwrap()), "LATIN CAPITAL LETTER A");
    }

    #[test]
    fn the_heading_says_which_set_and_state_and_browsing_is_labelled() {
        let format = TextFormat { charset: charset::known("petscii-c64"), ..TextFormat::default() };
        assert_eq!(heading_text(&format, None, Some("Lowercase / uppercase")), "PETSCII, C64 / C128, Lowercase / uppercase");
        assert_eq!(heading_text(&format, Some(charset::known("cp437")), None), "Browsing CP437 (DOS US)");
    }

    #[test]
    fn machines_and_regions_are_offered_as_variants() {
        let c64 = charset::known("petscii-c64");
        let names: Vec<&str> = sibling_sets(c64).iter().map(|set| set.id.as_str()).collect();
        assert!(names.contains(&"petscii-pet") && names.contains(&"petscii-vic20"));
        assert!(!names.contains(&"screen-c64"), "screen codes are a separate profile");
        assert!(sibling_sets(charset::known("teletext-en")).len() > 10);
        assert!(sibling_sets(charset::known("cp437")).is_empty());
    }

    #[test]
    fn inserting_an_entry_saves_the_exact_byte_chosen() {
        for (id, byte) in [("petscii-c64", 0x61u8), ("petscii-c64", 0xC1), ("cp437", 0xE9), ("atascii", 0x9B), ("windows-1252", 0x81), ("zx-spectrum", 0xE7), ("amiga-text", 0x9B)] {
            let set = charset::known(id);
            let text = text_for_byte(set, byte);
            let format = TextFormat { charset: set, ..TextFormat::default() };
            let eol = |_: usize| codec::Eol::Lf;
            assert_eq!(codec::encode(&[text], &eol, &format).unwrap(), [byte], "{id} {byte:02X}");
        }
    }
}
