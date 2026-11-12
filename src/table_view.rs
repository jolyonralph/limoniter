//! The window's side of table view: delimited data shown as rows and columns, with the text still the document
//! underneath. Editing a cell changes just that field's text, so undo, saving and every encoding work as they
//! always do. Sorting and filtering here only change what's shown; Sort the file by this column changes the file.

use super::*;
use crate::table::{self, Dialect, Index, Record};
use eframe::egui::{FontId, Id, Rect, Sense};

const MIN_WIDTH: f32 = 36.0;
const MAX_AUTO_WIDTH: f32 = 360.0;

/// A document's table view.
pub(super) struct TableState {
    pub dialect: Dialect,
    index: Option<Index>,
    /// Each column's width, set from what's in it until it's dragged.
    widths: Vec<f32>,
    /// Shown sorted by a column, smallest first or largest first. The file isn't changed.
    pub order: Option<(usize, bool)>,
    /// Only rows with this in them are shown.
    pub filter: String,
    /// The records shown, in the order shown, and what that was worked out from.
    shown: Vec<usize>,
    shown_for: Option<(u64, char, bool, Option<(usize, bool)>, String)>,
    /// The cell the cursor is in: a place in the rows shown, and a column.
    pub cursor: (usize, usize),
    /// A cell being typed into, and whether its box still needs the keyboard.
    pub editing: Option<(String, bool)>,
    /// The text's cursor as it was last seen, to follow a Find or Go to that moved it.
    seen_caret: (usize, u64),
    scroll_to_cursor: bool,
    /// Take the keyboard on the next frame.
    take_keyboard: bool,
    /// How far down the rows are scrolled, and how much of them shows, as last drawn.
    scrolled: (f32, f32),
}

impl TableState {
    pub(super) fn new(dialect: Dialect) -> Self {
        TableState { dialect, index: None, widths: Vec::new(), order: None, filter: String::new(), shown: Vec::new(), shown_for: None, cursor: (0, 0), editing: None, seen_caret: (usize::MAX, 0), scroll_to_cursor: true, take_keyboard: true, scrolled: (0.0, 0.0) }
    }
}

/// What the table's controls ask for.
enum Action {
    Commit { record: usize, column: usize, value: String },
    InsertRow,
    DeleteRow,
    InsertColumn,
    DeleteColumn,
    SortFile,
    TextView,
}

/// The lines of a document, cheaply shared.
fn lines_of_doc(text: &text_buffer::TextBuffer) -> Vec<Arc<str>> {
    text.all_lines()
}

fn record_fields(lines: &[Arc<str>], record: Record, delimiter: char) -> Vec<String> {
    table::fields(&table::record_text(lines, record), delimiter).into_iter().map(|f| f.value).collect()
}

/// A value as it's shown in one line of a cell.
fn shown_value(value: &str) -> String {
    value.replace("\r\n", "\u{23CE}").replace(['\n', '\r'], "\u{23CE}").replace('\t', " ")
}

impl Limoniter {
    /// Turns table view on or off for the file in front, working out how its data is written the first time.
    pub(super) fn toggle_table(&mut self) {
        let doc = &mut self.docs[self.active];
        if doc.table.is_some() {
            doc.table = None;
            self.pending_focus = true;
            return;
        }
        if doc.hex_mode || doc.browser.is_some() {
            self.message("Table view", "Table view shows text. Switch the file to text first.".into());
            return;
        }
        if doc.text.is_disk_backed() {
            self.message("Table view", "This file is too large for table view, which works on files under 64 MB for now.".into());
            return;
        }
        let ending = doc.path.as_deref().and_then(Path::extension).and_then(|e| e.to_str()).map(str::to_string);
        let sample: Vec<Arc<str>> = (0..doc.text.line_count().min(200)).map(|i| doc.text.line(i)).collect();
        let dialect = doc.table_offer.or_else(|| table::sniff(&sample, ending.as_deref())).unwrap_or(Dialect { delimiter: ',', header: true });
        doc.table_offer = None;
        doc.table = Some(Box::new(TableState::new(dialect)));
    }

    /// Looks once at a newly opened file, to offer table view if it holds delimited data.
    pub(super) fn check_for_table(&mut self) {
        let doc = &mut self.docs[self.active];
        if doc.table_checked || doc.placeholder || doc.scanning.is_some() {
            return;
        }
        doc.table_checked = true;
        if doc.hex_mode || doc.browser.is_some() || doc.table.is_some() || doc.text.is_disk_backed() || doc.text.line_count() < 2 {
            return;
        }
        let ending = doc.path.as_deref().and_then(Path::extension).and_then(|e| e.to_str()).map(str::to_string);
        let sample: Vec<Arc<str>> = (0..doc.text.line_count().min(200)).map(|i| doc.text.line(i)).collect();
        doc.table_offer = table::sniff(&sample, ending.as_deref());
    }

    /// The bar offering table view for a file that looks like delimited data.
    pub(super) fn table_offer_bar(&mut self, ui: &mut Ui) {
        let doc = &self.docs[self.active];
        let Some(dialect) = doc.table_offer.filter(|_| doc.table.is_none() && !doc.hex_mode) else {
            return;
        };
        let name = table::DELIMITERS.iter().find(|(_, d)| *d == dialect.delimiter).map_or("delimited", |(n, _)| n).to_lowercase();
        let (mut show, mut dismiss) = (false, false);
        Panel::top("table-offer").frame(egui::Frame::new().fill(theme::selected()).inner_margin(egui::Margin::symmetric(8, 4))).show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label(RichText::new(format!("This looks like {name}-separated data.")).strong());
                show = ui.button("Show it as a table").clicked();
                dismiss = ui.button("No thanks").clicked();
            });
        });
        if show {
            self.toggle_table();
        } else if dismiss {
            self.docs[self.active].table_offer = None;
        }
    }

    /// Draws the table in place of the text.
    pub(super) fn table_view(&mut self, ui: &mut Ui, input_on: bool) {
        let font_size = self.view.font_size;
        let doc = &mut self.docs[self.active];
        let read_only = doc.text.read_only;
        let version = doc.text.version();
        let Some(state) = doc.table.as_deref_mut() else {
            return;
        };
        let lines = lines_of_doc(&doc.text);
        let delimiter = state.dialect.delimiter;
        if state.index.as_ref().is_none_or(|i| i.version != version || i.delimiter != delimiter) {
            state.index = Some(Index::build(&lines, delimiter, version));
        }
        let index = state.index.clone().expect("just built");
        let records = index.records.clone();
        let header = state.dialect.header && !records.is_empty();
        let first_data = usize::from(header);
        let columns = index.columns.max(1);

        // The rows shown: filtered, then sorted, as asked.
        let key = (version, delimiter, header, state.order, state.filter.clone());
        if state.shown_for.as_ref() != Some(&key) {
            let needle = state.filter.to_lowercase();
            let mut shown: Vec<usize> = (first_data..records.len()).filter(|&r| needle.is_empty() || table::record_text(&lines, records[r]).to_lowercase().contains(&needle)).collect();
            if let Some((column, up)) = state.order {
                let mut keyed: Vec<(usize, String)> = shown.iter().map(|&r| (r, record_fields(&lines, records[r], delimiter).get(column).cloned().unwrap_or_default())).collect();
                keyed.sort_by(|a, b| {
                    let order = table::compare_values(&a.1, &b.1);
                    if up { order } else { order.reverse() }
                });
                shown = keyed.into_iter().map(|(r, _)| r).collect();
            }
            state.shown = shown;
            state.shown_for = Some(key);
        }
        let shown = state.shown.clone();
        state.cursor.0 = state.cursor.0.min(shown.len().saturating_sub(1));
        state.cursor.1 = state.cursor.1.min(columns - 1);

        // A Find or Go to that moved the text's cursor moves the table's to the row it's in.
        let caret = (doc.text.caret_line, version);
        if caret.0 != state.seen_caret.0 {
            if let Some(record) = records.iter().position(|r| r.first <= caret.0 && caret.0 <= r.last)
                && let Some(row) = shown.iter().position(|&r| r == record)
            {
                state.cursor.0 = row;
                state.scroll_to_cursor = true;
            }
            state.seen_caret = caret;
        }

        let names: Vec<String> = if header { record_fields(&lines, records[0], delimiter) } else { Vec::new() };
        let font = FontId::monospace(font_size);
        let row_height = ui.fonts_mut(|f| f.row_height(&font)) + 8.0;

        // Widths from what's in the first rows, until a column is dragged.
        if state.widths.len() < columns {
            let sample: Vec<Vec<String>> = records.iter().take(200).map(|r| record_fields(&lines, *r, delimiter)).collect();
            let char_width = ui.fonts_mut(|f| f.glyph_width(&font, '0'));
            for column in state.widths.len()..columns {
                let longest = sample.iter().filter_map(|row| row.get(column)).map(|v| shown_value(v).chars().count()).max().unwrap_or(4);
                state.widths.push((longest as f32 * char_width + 18.0).clamp(MIN_WIDTH + 20.0, MAX_AUTO_WIDTH));
            }
        }
        let char_width = ui.fonts_mut(|f| f.glyph_width(&font, '0'));
        let number_width = (format!("{}", records.len()).len() as f32 * ui.fonts_mut(|f| f.glyph_width(&font, '0')) + 20.0).max(44.0);

        let mut action = None;
        // ---------- The bar above the table ----------
        egui::Frame::new().inner_margin(egui::Margin { left: 8, right: 8, top: 6, bottom: 6 }).show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label("Separated by");
                let mut chosen = delimiter;
                egui::ComboBox::from_id_salt("table-delimiter").width(110.0).selected_text(table::DELIMITERS.iter().find(|(_, d)| *d == delimiter).map_or("?", |(n, _)| n)).show_ui(ui, |ui| {
                    for (name, d) in table::DELIMITERS {
                        ui.selectable_value(&mut chosen, d, name);
                    }
                });
                if chosen != delimiter {
                    state.dialect.delimiter = chosen;
                    state.widths.clear();
                    state.order = None;
                }
                ui.checkbox(&mut state.dialect.header, "First row is headings");
                ui.add(egui::TextEdit::singleline(&mut state.filter).hint_text("Show rows containing\u{2026}").desired_width(200.0));
                let total = records.len() - first_data;
                ui.label(RichText::new(if shown.len() == total { format!("{total} rows, {columns} columns") } else { format!("{} of {total} rows", shown.len()) }).color(theme::text_dim()));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.button("Show as text").clicked() {
                        action = Some(Action::TextView);
                    }
                });
            });
            ui.add_space(2.0);
            ui.horizontal(|ui| {
                ui.add_enabled_ui(!read_only, |ui| {
                    if ui.small_button("Insert row").on_hover_text("An empty row below this one").clicked() {
                        action = Some(Action::InsertRow);
                    }
                    if ui.add_enabled(!shown.is_empty(), egui::Button::new("Delete row").small()).clicked() {
                        action = Some(Action::DeleteRow);
                    }
                    if ui.small_button("Insert column").on_hover_text("An empty column before this one").clicked() {
                        action = Some(Action::InsertColumn);
                    }
                    if ui.add_enabled(columns > 1, egui::Button::new("Delete column").small()).clicked() {
                        action = Some(Action::DeleteColumn);
                    }
                    if ui.small_button("Sort the file by this column").on_hover_text("Puts the rows of the file itself in order. Clicking a heading only changes how they're shown.").clicked() {
                        action = Some(Action::SortFile);
                    }
                });
                ui.label(RichText::new("Double-click or type to change a cell. Enter keeps it, Esc doesn't.").color(theme::text_faint()));
            });
        });

        // ---------- Keys ----------
        let focus_id = views::table_id();
        // Seen every frame, as egui lets go of the keyboard for anything it doesn't see.
        ui.interact(ui.max_rect(), focus_id, Sense::focusable_noninteractive());
        // The table takes the keyboard when it's first shown, and whenever nothing else has it.
        if input_on && state.editing.is_none() && (std::mem::take(&mut state.take_keyboard) || ui.memory(|m| m.focused().is_none())) {
            take_keyboard(ui, focus_id);
        }
        let focused = ui.memory(|m| m.has_focus(focus_id));
        if focused {
            take_keyboard(ui, focus_id);
        }
        let editing = state.editing.is_some();
        if focused && input_on && !editing && !shown.is_empty() {
            let page = ((ui.available_height() / row_height) as usize).saturating_sub(2).max(1);
            let (row, column) = state.cursor;
            let mut start_with = None;
            ui.input(|i| {
                let ctrl = i.modifiers.command;
                let moved = if i.key_pressed(Key::ArrowDown) {
                    Some((row + 1, column))
                } else if i.key_pressed(Key::ArrowUp) {
                    Some((row.saturating_sub(1), column))
                } else if i.key_pressed(Key::ArrowRight) || (i.key_pressed(Key::Tab) && !i.modifiers.shift) {
                    Some((row, column + 1))
                } else if i.key_pressed(Key::ArrowLeft) || (i.key_pressed(Key::Tab) && i.modifiers.shift) {
                    Some((row, column.saturating_sub(1)))
                } else if i.key_pressed(Key::PageDown) {
                    Some((row + page, column))
                } else if i.key_pressed(Key::PageUp) {
                    Some((row.saturating_sub(page), column))
                } else if i.key_pressed(Key::Home) {
                    Some(if ctrl { (0, 0) } else { (row, 0) })
                } else if i.key_pressed(Key::End) {
                    Some(if ctrl { (usize::MAX, columns - 1) } else { (row, columns - 1) })
                } else {
                    None
                };
                if let Some((r, c)) = moved {
                    state.cursor = (r.min(shown.len() - 1), c.min(columns - 1));
                    state.scroll_to_cursor = true;
                }
                if !read_only {
                    if i.key_pressed(Key::Enter) || i.key_pressed(Key::F2) {
                        start_with = Some(None);
                    } else if i.key_pressed(Key::Delete) {
                        action = Some(Action::Commit { record: shown[row], column, value: String::new() });
                    } else if !ctrl && !i.modifiers.alt {
                        // Typing replaces what's in the cell, as in a spreadsheet.
                        for event in &i.events {
                            if let egui::Event::Text(text) = event {
                                start_with = Some(Some(text.clone()));
                            }
                        }
                    }
                }
            });
            if let Some(start) = start_with {
                let current = record_fields(&lines, records[shown[state.cursor.0]], delimiter).get(state.cursor.1).cloned().unwrap_or_default();
                state.editing = Some((start.unwrap_or(current), true));
            }
        }

        // ---------- The table ----------
        let widths = state.widths.clone();
        let total_width = number_width + widths.iter().take(columns).sum::<f32>();
        let mut clicked: Option<(usize, usize, bool)> = None;
        let mut heading_clicked = None;
        let mut dragged: Option<(usize, f32)> = None;
        let mut commit_edit: Option<(bool, (isize, isize))> = None;
        let cursor = state.cursor;
        let scroll_to = std::mem::take(&mut state.scroll_to_cursor);
        let order = state.order;
        let scrolled = state.scrolled;
        let mut now_scrolled = scrolled;
        egui::ScrollArea::horizontal().id_salt("table-across").auto_shrink([false, false]).show(ui, |ui| {
            ui.set_min_width(total_width);
            // The headings stay put while the rows scroll under them.
            let (head_rect, _) = ui.allocate_exact_size(egui::vec2(total_width, row_height), Sense::hover());
            let painter = ui.painter_at(head_rect);
            painter.rect_filled(head_rect, 0.0, theme::gutter_back());
            let mut x = head_rect.min.x + number_width;
            for (column, &width) in widths.iter().enumerate().take(columns) {
                let rect = Rect::from_min_size(egui::pos2(x, head_rect.min.y), egui::vec2(width, row_height));
                let mut name = if header { names.get(column).cloned().unwrap_or_default() } else { column_letter(column) };
                if let Some((c, up)) = order
                    && c == column
                {
                    name = format!("{name} {}", if up { "\u{25B2}" } else { "\u{25BC}" });
                }
                painter.with_clip_rect(rect.shrink2(egui::vec2(6.0, 0.0))).text(rect.left_center() + egui::vec2(6.0, 0.0), egui::Align2::LEFT_CENTER, shown_value(&name), font.clone(), theme::strong_text());
                painter.vline(rect.right(), rect.y_range(), egui::Stroke::new(1.0, theme::border()));
                let response = ui.interact(rect.shrink2(egui::vec2(4.0, 0.0)), focus_id.with(("heading", column)), Sense::click());
                if response.clicked() {
                    heading_clicked = Some(column);
                }
                response.on_hover_text("Click to show the rows in order of this column; again for the other way; a third time as in the file.");
                // The edge of a heading can be dragged to make the column wider or narrower.
                let edge = Rect::from_center_size(egui::pos2(rect.right(), rect.center().y), egui::vec2(8.0, row_height));
                let drag = ui.interact(edge, focus_id.with(("edge", column)), Sense::drag()).on_hover_cursor(egui::CursorIcon::ResizeHorizontal);
                if drag.dragged() {
                    dragged = Some((column, (width + drag.drag_delta().x).max(MIN_WIDTH)));
                }
                x += width;
            }
            painter.hline(head_rect.x_range(), head_rect.bottom(), egui::Stroke::new(1.0, theme::border()));

            let mut area = egui::ScrollArea::vertical().id_salt("table-down").auto_shrink([false, false]);
            if scroll_to {
                // Keep the cursor's row in view.
                let (offset, height) = scrolled;
                let top = cursor.0 as f32 * row_height;
                if top < offset {
                    area = area.vertical_scroll_offset(top);
                } else if height > 0.0 && top + row_height > offset + height {
                    area = area.vertical_scroll_offset((top + row_height - height).max(0.0));
                }
            }
            let output = area.show_rows(ui, row_height, shown.len(), |ui, range| {
                for row in range {
                    let record = records[shown[row]];
                    let fields = record_fields(&lines, record, delimiter);
                    let (rect, response) = ui.allocate_exact_size(egui::vec2(total_width, row_height), Sense::click());
                    let painter = ui.painter_at(rect);
                    if row % 2 == 1 {
                        painter.rect_filled(rect, 0.0, theme::active_line());
                    }
                    // Rows that don't have the usual number of fields are marked by their number.
                    let uneven = record.broken || (header && fields.len() != names.len());
                    let number_rect = Rect::from_min_size(rect.min, egui::vec2(number_width, row_height));
                    painter.rect_filled(number_rect, 0.0, theme::gutter_back());
                    let number_colour = if uneven { theme::rainbow_mistake() } else { theme::gutter_text() };
                    painter.text(number_rect.right_center() - egui::vec2(8.0, 0.0), egui::Align2::RIGHT_CENTER, format!("{}", shown[row] + 1 - first_data), font.clone(), number_colour);
                    if uneven {
                        let why = if record.broken { "A quote here is never closed.".to_string() } else { format!("This row has {} fields; the headings have {}.", fields.len(), names.len()) };
                        ui.interact(number_rect, focus_id.with(("number", row)), Sense::hover()).on_hover_text(why);
                    }
                    let mut x = rect.min.x + number_width;
                    for (column, width) in widths.iter().enumerate().take(columns) {
                        let cell = Rect::from_min_size(egui::pos2(x, rect.min.y), egui::vec2(*width, row_height));
                        let here = (row, column) == cursor;
                        if here {
                            painter.rect_filled(cell, 0.0, theme::selection());
                            painter.rect_stroke(cell.shrink(1.0), 0.0, egui::Stroke::new(1.5, theme::accent()), egui::StrokeKind::Inside);
                        }
                        if let Some(value) = fields.get(column) {
                            let colour = theme::editor_text();
                            let align_right = table::number(value).is_some();
                            let inner = cell.shrink2(egui::vec2(6.0, 0.0));
                            let (anchor, align) = if align_right { (inner.right_center(), egui::Align2::RIGHT_CENTER) } else { (inner.left_center(), egui::Align2::LEFT_CENTER) };
                            painter.with_clip_rect(inner).text(anchor, align, shown_value(value), font.clone(), colour);
                        }
                        painter.vline(cell.right(), cell.y_range(), egui::Stroke::new(1.0, theme::border()));
                        x += width;
                    }
                    // A value too long for its column, or on more than one line, shows in full under the pointer.
                    if let Some(pos) = response.hover_pos() {
                        let mut edge = rect.min.x + number_width;
                        let column = widths.iter().take(columns).position(|w| {
                            edge += w;
                            pos.x < edge
                        });
                        if let Some(value) = column.and_then(|c| fields.get(c).map(|v| (c, v)))
                            && (value.1.contains('\n') || value.1.chars().count() as f32 * char_width + 12.0 > widths[value.0])
                        {
                            response.clone().on_hover_text(value.1.clone());
                        }
                    }
                    if (response.clicked() || response.double_clicked())
                        && let Some(pos) = response.interact_pointer_pos()
                    {
                        let mut edge = rect.min.x + number_width;
                        let column = widths.iter().take(columns).position(|w| {
                            edge += w;
                            pos.x < edge
                        });
                        if let Some(column) = column {
                            clicked = Some((row, column, response.double_clicked()));
                        }
                    }
                    // The cell being typed into has a box of its own over it.
                    if cursor.0 == row && editing {
                        let mut left = rect.min.x + number_width;
                        for w in widths.iter().take(cursor.1) {
                            left += w;
                        }
                        let cell = Rect::from_min_size(egui::pos2(left, rect.min.y), egui::vec2(widths[cursor.1].max(160.0), row_height));
                        if let Some((text, focus)) = state.editing.as_mut() {
                            let edit = ui.put(cell, egui::TextEdit::singleline(text).font(font.clone()).margin(egui::vec2(5.0, 3.0)));
                            if std::mem::take(focus) {
                                edit.request_focus();
                            }
                            let keys = ui.input(|i| (i.key_pressed(Key::Enter), i.key_pressed(Key::Tab), i.modifiers.shift, i.key_pressed(Key::Escape)));
                            if keys.3 {
                                commit_edit = Some((false, (0, 0)));
                            } else if edit.lost_focus() && keys.0 {
                                commit_edit = Some((true, (if keys.2 { -1 } else { 1 }, 0)));
                            } else if edit.lost_focus() && keys.1 {
                                commit_edit = Some((true, (0, if keys.2 { -1 } else { 1 })));
                            } else if edit.lost_focus() {
                                commit_edit = Some((true, (0, 0)));
                            }
                        }
                    }
                }
            });
            now_scrolled = (output.state.clamped_offset().y, output.inner_rect.height());
        });
        state.scrolled = now_scrolled;
        if shown.is_empty() {
            ui.label(RichText::new(if records.len() > first_data { "No rows contain that." } else { "There are no rows yet. Insert row adds one." }).color(theme::text_dim()));
        }

        if let Some((column, width)) = dragged {
            state.widths[column] = width;
        }
        if let Some(column) = heading_clicked {
            state.order = match state.order {
                Some((c, true)) if c == column => Some((column, false)),
                Some((c, false)) if c == column => None,
                _ => Some((column, true)),
            };
        }
        if let Some((save, (down, across))) = commit_edit {
            if let Some((value, _)) = state.editing.take()
                && save
                && let Some(&record) = shown.get(cursor.0)
            {
                action = Some(Action::Commit { record, column: cursor.1, value });
            }
            state.cursor = (((cursor.0 as isize + down).max(0) as usize).min(shown.len().saturating_sub(1)), ((cursor.1 as isize + across).max(0) as usize).min(columns - 1));
            state.scroll_to_cursor = true;
            take_keyboard(ui, focus_id);
        }
        if let Some((row, column, double)) = clicked {
            if state.editing.is_some() && (row, column) != cursor {
                // Clicking elsewhere keeps what was typed, as leaving a cell does.
                if let Some((value, _)) = state.editing.take()
                    && let Some(&record) = shown.get(cursor.0)
                {
                    action = Some(Action::Commit { record, column: cursor.1, value });
                }
            }
            state.cursor = (row, column);
            take_keyboard(ui, focus_id);
            if double && !read_only {
                let current = record_fields(&lines, records[shown[row]], delimiter).get(column).cloned().unwrap_or_default();
                state.editing = Some((current, true));
            }
        }
        // Keep the text's cursor on the row, so switching back to text lands there.
        let record_line = shown.get(state.cursor.0).map(|&r| records[r].first);
        if let Some(line) = record_line
            && doc.text.caret_line != line
            && action.is_none()
        {
            doc.text.move_caret(line, 0, false);
            state.seen_caret = (line, version);
        }
        if let Some(action) = action {
            self.table_action(action, &lines, &records, &shown, header);
        }
    }

    /// Copy, Cut and Paste work on the cell the cursor is in while the table is shown. True if it took the command.
    pub(super) fn table_command(&mut self, command: Command, ui: &Ui) -> bool {
        let doc = &mut self.docs[self.active];
        let Some(state) = doc.table.as_deref_mut() else {
            return false;
        };
        if !matches!(command, Command::Copy | Command::Cut | Command::Paste | Command::SelectAll) {
            return false;
        }
        let lines = lines_of_doc(&doc.text);
        let delimiter = state.dialect.delimiter;
        let Some(index) = state.index.clone() else {
            return true;
        };
        let Some(&record) = state.shown.get(state.cursor.0) else {
            return true;
        };
        let column = state.cursor.1;
        let value = record_fields(&lines, index.records[record], delimiter).get(column).cloned().unwrap_or_default();
        let read_only = doc.text.read_only;
        match command {
            Command::Copy => ui.ctx().copy_text(value),
            Command::Cut if !read_only => {
                ui.ctx().copy_text(value);
                self.table_action(Action::Commit { record, column, value: String::new() }, &lines, &index.records, &[], false);
            }
            Command::Paste if !read_only => {
                if let Ok(text) = arboard::Clipboard::new().and_then(|mut c| c.get_text()) {
                    let text = text.strip_suffix("\r\n").or_else(|| text.strip_suffix('\n')).unwrap_or(&text).to_string();
                    self.table_action(Action::Commit { record, column, value: text }, &lines, &index.records, &[], false);
                }
            }
            _ => {}
        }
        true
    }

    fn table_action(&mut self, action: Action, lines: &[Arc<str>], records: &[Record], shown: &[usize], header: bool) {
        let doc = &mut self.docs[self.active];
        let Some(state) = doc.table.as_deref_mut() else {
            return;
        };
        let delimiter = state.dialect.delimiter;
        let columns = state.index.as_ref().map_or(1, |i| i.columns.max(1));
        let (row, column) = state.cursor;
        match action {
            Action::TextView => {
                doc.table = None;
                self.pending_focus = true;
            }
            Action::Commit { record, column, value } => {
                let r = records[record];
                let old = table::record_text(lines, r);
                let new = table::set_field(&old, delimiter, column, &value);
                if new != old {
                    doc.text.replace_lines(r.first, r.last, table::lines_of(&new));
                }
            }
            Action::InsertRow => {
                let blank = table::empty_record(delimiter, columns);
                match shown.get(row).map(|&r| records[r]).or_else(|| records.last().copied()) {
                    Some(r) => {
                        let mut rows = table::lines_of(&table::record_text(lines, r));
                        rows.push(blank);
                        doc.text.replace_lines(r.first, r.last, rows);
                        if let Some(state) = doc.table.as_deref_mut() {
                            state.order = None;
                            state.filter.clear();
                            let at = records.iter().position(|x| *x == r).unwrap_or(0) + 1;
                            state.cursor = (at.saturating_sub(usize::from(header)), 0);
                            state.scroll_to_cursor = true;
                        }
                    }
                    None => {
                        doc.text.replace_lines(0, 0, vec![blank]);
                    }
                }
            }
            Action::DeleteRow => {
                if let Some(&r) = shown.get(row) {
                    let r = records[r];
                    doc.text.splice_lines(r.first, r.last + 1, Vec::new());
                }
            }
            Action::InsertColumn | Action::DeleteColumn => {
                let Some(last) = records.last() else {
                    return;
                };
                let mut rows = Vec::new();
                for r in records {
                    let text = table::record_text(lines, *r);
                    let changed = if matches!(action, Action::InsertColumn) { table::insert_field(&text, delimiter, column) } else { table::remove_field(&text, delimiter, column) };
                    rows.extend(table::lines_of(&changed));
                }
                doc.text.replace_lines(0, last.last, rows);
                if let Some(state) = doc.table.as_deref_mut() {
                    state.widths.clear();
                    state.order = None;
                }
            }
            Action::SortFile => {
                let Some(last) = records.last() else {
                    return;
                };
                let skip = usize::from(header);
                let mut keyed: Vec<(String, String)> = records.iter().skip(skip).map(|r| {
                    let text = table::record_text(lines, *r);
                    let key = table::fields(&text, delimiter).into_iter().nth(column).map(|f| f.value).unwrap_or_default();
                    (key, text)
                }).collect();
                keyed.sort_by(|a, b| table::compare_values(&a.0, &b.0));
                let mut rows = Vec::new();
                if header {
                    rows.extend(table::lines_of(&table::record_text(lines, records[0])));
                }
                for (_, text) in keyed {
                    rows.extend(table::lines_of(&text));
                }
                doc.text.replace_lines(0, last.last, rows);
                if let Some(state) = doc.table.as_deref_mut() {
                    state.order = None;
                }
            }
        }
    }
}

/// Gives the table the keyboard, with Tab and the arrow keys moving around it rather than to the next control.
fn take_keyboard(ui: &Ui, id: Id) {
    ui.memory_mut(|m| {
        m.request_focus(id);
        m.set_focus_lock_filter(id, egui::EventFilter { tab: true, horizontal_arrows: true, vertical_arrows: true, escape: false });
    });
}

/// A column's letter, as a spreadsheet names it, for data with no headings.
fn column_letter(mut column: usize) -> String {
    let mut out = Vec::new();
    loop {
        out.push((b'A' + (column % 26) as u8) as char);
        if column < 26 {
            break;
        }
        column = column / 26 - 1;
    }
    out.iter().rev().collect()
}

#[cfg(test)]
#[path = "table_view_tests.rs"]
mod tests;
