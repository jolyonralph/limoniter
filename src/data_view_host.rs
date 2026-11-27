//! The data viewer in a tab: a table of data (a spreadsheet's sheets, JSON lines, a data frame, the rows a query
//! gave back) to look at, sort, filter and copy, but not to change. What it works out is in `data_view`; what it
//! reads from files is in `xlsx` and `jsonl`. Like the notebook, it's a tab of its own: the tab's text underneath
//! is empty and read-only, and saving writes the rows shown as CSV.

use super::*;
use crate::data_table::{ColumnKind, DataTable};
use crate::data_view::{DataState, SheetState, Summary};
use eframe::egui::{FontId, Rect, Sense};

const MIN_WIDTH: f32 = 40.0;
const MAX_AUTO_WIDTH: f32 = 360.0;
/// How wide the strip with a column's summary is.
const STRIP_WIDTH: f32 = 250.0;

/// Whether a file's name says it opens in the data viewer rather than as text.
pub(super) fn is_data_name(name: &str) -> bool {
    let lower = name.to_lowercase();
    lower.ends_with(".xlsx") || crate::jsonl::is_jsonl_name(&lower)
}

use crate::data_view::grouped;

/// A number as short as it can be written, without trailing zeros.
fn number_text(n: f64) -> String {
    let text = format!("{n:.6}");
    let text = text.trim_end_matches('0').trim_end_matches('.');
    if text == "-0" { "0".to_string() } else { text.to_string() }
}

/// What a column holds, in words.
fn kind_words(kind: ColumnKind) -> String {
    match kind {
        ColumnKind::Text | ColumnKind::Unknown => crate::i18n::tr("Text"),
        ColumnKind::Integer => crate::i18n::tr("Whole numbers"),
        ColumnKind::Real => crate::i18n::tr("Numbers"),
        ColumnKind::Bool => crate::i18n::tr("True or false"),
        ColumnKind::Date => crate::i18n::tr("Dates"),
        ColumnKind::DateTime => crate::i18n::tr("Dates and times"),
        ColumnKind::Binary => crate::i18n::tr("Binary data"),
        ColumnKind::Json => crate::i18n::tr("JSON"),
    }
    .to_string()
}

fn right_aligned(kind: ColumnKind) -> bool {
    matches!(kind, ColumnKind::Integer | ColumnKind::Real)
}

/// What the viewer was asked to do while it was drawn, done once it's finished with the document.
enum Act {
    SaveCsv,
    AsText(PathBuf),
}

impl Limoniter {
    /// Opens tables of data in a viewer of their own, named `title`: one sheet each, as tabs inside it.
    pub(super) fn open_data_sheets(&mut self, title: &str, sheets: Vec<(String, DataTable)>, from: Option<PathBuf>) {
        self.new_document();
        let doc = self.doc();
        doc.untitled = title.to_string();
        doc.set_read_only(true);
        let mut state = DataState::new(sheets);
        state.from = from;
        doc.data = Some(Box::new(state));
    }

    /// Opens a spreadsheet or a JSON-lines file in the viewer. True if it was one; if it couldn't be read, the
    /// reason is said.
    pub(super) fn open_data_file(&mut self, path: &Path) -> bool {
        let name = path.file_name().map_or_else(|| path.display().to_string(), |n| n.to_string_lossy().into_owned());
        let read = std::fs::read(path).map_err(|e| e.to_string()).and_then(|bytes| {
            if crate::jsonl::is_jsonl_name(&name) { crate::jsonl::read(&String::from_utf8_lossy(&bytes)).map(|table| vec![(name.clone(), table)]) } else { crate::xlsx::read(bytes) }
        });
        match read {
            Ok(sheets) => {
                self.note_recent_file(path);
                // A blank, untouched only tab gives way to it.
                if self.docs.len() == 1 && self.docs[0].is_blank() {
                    self.docs.clear();
                }
                self.open_data_sheets(&name, sheets, crate::jsonl::is_jsonl_name(&name).then(|| path.to_path_buf()));
                true
            }
            Err(e) => {
                warn(&crate::i18n::tf!("Couldn't open {}.\n\n{e}", path.display()));
                false
            }
        }
    }

    /// View, Open in the data viewer: the delimited text in front, as a read-only table to sort, filter and sum up.
    pub(super) fn open_in_data_viewer(&mut self) {
        let doc = &self.docs[self.active];
        if doc.placeholder || doc.hex_mode || doc.data.is_some() {
            return;
        }
        let lines: Vec<String> = doc.text.all_lines().iter().map(|l| l.to_string()).collect();
        let ending = doc.path_or_name().extension().map(|e| e.to_string_lossy().to_lowercase());
        let dialect = doc.table.as_deref().map(|t| t.dialect).or_else(|| crate::table::sniff(&lines, ending.as_deref())).unwrap_or(crate::table::Dialect { delimiter: if ending.as_deref() == Some("tsv") { '\t' } else { ',' }, header: true });
        let title = doc.name();
        let table = table_from_lines(&lines, dialect);
        self.open_data_sheets(&title, vec![(title.clone(), table)], None);
    }

    /// Writes the rows shown as CSV, to a file chosen. True if it was written.
    pub(super) fn save_data_csv(&mut self, index: usize) -> bool {
        let Some(sheet) = self.docs[index].data.as_deref().and_then(|d| d.sheets.get(d.active)) else { return false };
        let stem = Path::new(&self.docs[index].name()).file_stem().map_or_else(|| "data".to_string(), |s| s.to_string_lossy().into_owned());
        let name = if self.docs[index].data.as_deref().is_some_and(|d| d.sheets.len() > 1) { format!("{stem} {}", sheet.name) } else { stem };
        let Some(path) = crate::sys_dialogs::file_dialog().set_title(crate::i18n::tr("Save as CSV")).add_filter("CSV", &["csv"]).set_file_name(format!("{name}.csv")).save_file() else { return false };
        let text = crate::data_view::to_csv(&sheet.table, &sheet.rows);
        match std::fs::write(&path, text) {
            Ok(()) => {
                crate::files_changing::note_own_write(&path);
                self.print_status = crate::i18n::tf!("Saved {} as CSV.", path.display());
                true
            }
            Err(e) => {
                self.message("Save as CSV", crate::i18n::tf!("{} couldn't be written: {e}", path.display()));
                false
            }
        }
    }

    /// Copy and Select All from the menus, while the data viewer is in front. True if it took the command.
    pub(super) fn data_command(&mut self, command: Command, ui: &Ui) -> bool {
        let Some(sheet) = self.docs.get_mut(self.active).and_then(|d| d.data.as_deref_mut()).and_then(DataState::sheet) else { return false };
        match command {
            Command::Copy => ui.ctx().copy_text(sheet.copy_text()),
            Command::SelectAll => {
                if !sheet.rows.is_empty() {
                    sheet.anchor = Some((0, 0));
                    sheet.cursor = (sheet.rows.len() - 1, sheet.table.columns.len().max(1) - 1);
                }
            }
            Command::Cut | Command::Paste => {}
            _ => return false,
        }
        true
    }

    /// A step before a screenshot: "sheet-N", "sort-N" (a click on that column's heading), "filter-WORDS" and
    /// "cell-ROW-COLUMN" (the cell chosen).
    pub(super) fn data_step(&mut self, step: &str) {
        let Some(state) = self.docs.get_mut(self.active).and_then(|d| d.data.as_deref_mut()) else { return };
        let number = |text: &str| text.parse::<usize>().unwrap_or(0);
        if let Some(n) = step.strip_prefix("sheet-") {
            state.active = number(n).min(state.sheets.len().saturating_sub(1));
        } else if let Some(sheet) = state.sheet() {
            if let Some(n) = step.strip_prefix("sort-") {
                sheet.click_heading(number(n));
            } else if let Some(words) = step.strip_prefix("filter-") {
                sheet.filter = words.to_string();
            } else if let Some(place) = step.strip_prefix("cell-") {
                let (row, column) = place.split_once('-').unwrap_or((place, "0"));
                sheet.cursor = (number(row), number(column));
            }
        }
    }

    /// Draws the viewer of the tab in front.
    pub(super) fn data_view(&mut self, ui: &mut Ui, input_on: bool) {
        let font = FontId::monospace(self.view.font_size);
        let ctx = ui.ctx().clone();
        let index = self.active;
        let mut act = None;
        let Some(state) = self.docs[index].data.as_deref_mut() else { return };
        state.active = state.active.min(state.sheets.len().saturating_sub(1));
        for sheet in &mut state.sheets {
            sheet.update(Some(&ctx));
        }
        let from = state.from.clone();
        let (active, sheet_count) = (state.active, state.sheets.len());
        let mut chosen_sheet = None;
        let strip = state.strip;
        let mut strip_on = strip;
        let Some(sheet) = state.sheets.get_mut(active) else { return };
        let busy = sheet.busy();

        // ---------- What the person was told about it, above the table ----------
        if let Some(note) = sheet.table.note.clone() {
            egui::Frame::new().inner_margin(egui::Margin { left: 10, right: 10, top: 5, bottom: 5 }).fill(theme::active_line()).show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.label(RichText::new(note).color(theme::text_dim()));
            });
        }
        // ---------- The bar ----------
        egui::Frame::new().inner_margin(egui::Margin { left: 8, right: 8, top: 6, bottom: 6 }).show(ui, |ui| {
            ui.horizontal_wrapped(|ui| {
                if sheet_count > 1 {
                    for (i, name) in state_names(&self.docs[index]).into_iter().enumerate() {
                        if ui.selectable_label(i == active, name).clicked() {
                            chosen_sheet = Some(i);
                        }
                    }
                    ui.separator();
                }
                let Some(sheet) = self.docs[index].data.as_deref_mut().and_then(DataState::sheet) else { return };
                ui.add(egui::TextEdit::singleline(&mut sheet.filter).hint_text(crate::i18n::tr("Show rows containing\u{2026}")).desired_width(200.0));
                let (total, shown, columns) = (sheet.table.rows.len(), sheet.rows.len(), sheet.table.columns.len());
                let counts = if sheet.working.is_some() {
                    crate::i18n::tr("Working\u{2026}").to_string()
                } else if shown == total {
                    crate::i18n::tf!("{} rows, {} columns", grouped(total), grouped(columns))
                } else {
                    crate::i18n::tf!("{} of {} rows", grouped(shown), grouped(total))
                };
                ui.label(RichText::new(counts).color(theme::text_dim()));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.button(crate::i18n::tr("Save as CSV\u{2026}")).on_hover_text(crate::i18n::tr("Saves the rows shown, in the order shown")).clicked() {
                        act = Some(Act::SaveCsv);
                    }
                    if let Some(path) = &from
                        && ui.button(crate::i18n::tr("Show as text")).clicked()
                    {
                        act = Some(Act::AsText(path.clone()));
                    }
                    ui.checkbox(&mut strip_on, crate::i18n::tr("Column summary")).on_hover_text(crate::i18n::tr("A strip beside the table with what the chosen column holds"));
                });
            });
        });
        ui.separator();

        let Some(state) = self.docs[index].data.as_deref_mut() else { return };
        state.strip = strip_on;
        if let Some(i) = chosen_sheet {
            state.active = i;
        }
        let Some(sheet) = state.sheets.get_mut(state.active) else { return };

        // ---------- The table, and the strip beside it ----------
        let full = ui.available_rect_before_wrap();
        let (table_rect, strip_rect) = if strip_on && full.width() > STRIP_WIDTH * 2.0 { (Rect::from_min_max(full.min, egui::pos2(full.max.x - STRIP_WIDTH, full.max.y)), Some(Rect::from_min_max(egui::pos2(full.max.x - STRIP_WIDTH, full.min.y), full.max))) } else { (full, None) };
        ui.scope_builder(egui::UiBuilder::new().max_rect(table_rect), |ui| draw_table(ui, sheet, &font, input_on));
        if let Some(rect) = strip_rect {
            ui.scope_builder(egui::UiBuilder::new().max_rect(rect), |ui| draw_strip(ui, sheet));
        }
        if busy || sheet.busy() {
            ui.ctx().request_repaint_after(std::time::Duration::from_millis(100));
        }

        match act {
            Some(Act::SaveCsv) => {
                self.save_data_csv(index);
            }
            Some(Act::AsText(path)) => self.open_data_as_text(&path),
            None => {}
        }
    }

    /// Opens a file the viewer showed as the text it is, in a tab of its own.
    pub(super) fn open_data_as_text(&mut self, path: &Path) {
        match Document::open(path, self.encoding_prefs.get(path)) {
            Ok(doc) => {
                self.docs.push(doc);
                self.active = self.docs.len() - 1;
                self.pending_focus = true;
            }
            Err(e) => warn(&crate::i18n::tf!("Couldn't open {}.\n\n{e}", path.display())),
        }
    }
}

fn state_names(doc: &Document) -> Vec<String> {
    doc.data.as_deref().map(|d| d.sheets.iter().map(|s| s.name.clone()).collect()).unwrap_or_default()
}

/// A table from delimited text: the first row names the columns if the dialect says so. Rows short of the
/// widest are filled out with empty cells.
fn table_from_lines(lines: &[String], dialect: crate::table::Dialect) -> DataTable {
    use crate::data_table::{Cell, DataColumn};
    let records = crate::table::records(lines, dialect.delimiter);
    let mut rows: Vec<Vec<String>> = records.iter().map(|r| crate::table::fields(&crate::table::record_text(lines, *r), dialect.delimiter).into_iter().map(|f| f.value).collect()).collect();
    let width = rows.iter().map(Vec::len).max().unwrap_or(0);
    let names: Vec<String> = if dialect.header && !rows.is_empty() { rows.remove(0) } else { Vec::new() };
    let columns = (0..width).map(|i| DataColumn { name: names.get(i).filter(|n| !n.trim().is_empty()).cloned().unwrap_or_else(|| crate::xlsx::column_letters(i)), kind: ColumnKind::Unknown }).collect();
    let rows = rows.into_iter().map(|row| (0..width).map(|i| row.get(i).map_or(Cell::Null, |v| if v.is_empty() { Cell::Null } else { Cell::Text(v.clone()) })).collect()).collect();
    DataTable { columns, rows, note: None }
}

fn draw_table(ui: &mut Ui, sheet: &mut SheetState, font: &FontId, input_on: bool) {
    let row_height = ui.fonts_mut(|f| f.row_height(font)) + 8.0;
    let char_width = ui.fonts_mut(|f| f.glyph_width(font, '0'));
    let table = sheet.table.clone();
    let rows = sheet.rows.clone();
    let columns = table.columns.len();
    if columns == 0 {
        ui.label(RichText::new(crate::i18n::tr("There is nothing in this table.")).color(theme::text_dim()));
        return;
    }
    if sheet.widths.len() < columns {
        for column in sheet.widths.len()..columns {
            let longest = table.rows.iter().take(200).map(|row| row.get(column).map_or(0, |c| super::table_view::shown_value(&c.text()).chars().count())).chain(std::iter::once(table.columns[column].name.chars().count() + 3)).max().unwrap_or(4);
            sheet.widths.push((longest as f32 * char_width + 18.0).clamp(MIN_WIDTH + 20.0, MAX_AUTO_WIDTH));
        }
    }
    let number_width = (grouped(table.rows.len()).len() as f32 * char_width + 20.0).max(44.0);
    let widths = sheet.widths.clone();
    let total_width = number_width + widths.iter().sum::<f32>();

    // ---------- Keys ----------
    let focus_id = views::table_id();
    ui.interact(ui.max_rect(), focus_id, Sense::focusable_noninteractive());
    let first_time = ui.memory(|m| m.focused().is_none());
    if input_on && first_time {
        ui.memory_mut(|m| m.request_focus(focus_id));
    }
    let focused = ui.memory(|m| m.has_focus(focus_id));
    if focused {
        ui.memory_mut(|m| {
            m.set_focus_lock_filter(focus_id, egui::EventFilter { tab: true, horizontal_arrows: true, vertical_arrows: true, escape: false });
        });
    }
    if focused && input_on && !rows.is_empty() {
        let page = ((ui.available_height() / row_height) as usize).saturating_sub(2).max(1);
        let (row, column) = sheet.cursor;
        let mut copy = false;
        let mut moved = None;
        let mut shift = false;
        ui.input(|i| {
            copy = i.events.iter().any(|e| matches!(e, egui::Event::Copy));
            shift = i.modifiers.shift;
            let ctrl = i.modifiers.command;
            moved = if i.key_pressed(Key::ArrowDown) {
                Some((row + 1, column))
            } else if i.key_pressed(Key::ArrowUp) {
                Some((row.saturating_sub(1), column))
            } else if i.key_pressed(Key::ArrowRight) {
                Some((row, column + 1))
            } else if i.key_pressed(Key::ArrowLeft) {
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
            if i.key_pressed(Key::Escape) {
                sheet.anchor = None;
            }
        });
        if let Some((r, c)) = moved {
            if shift {
                sheet.anchor.get_or_insert(sheet.cursor);
            } else {
                sheet.anchor = None;
            }
            sheet.cursor = (r.min(rows.len() - 1), c.min(columns - 1));
            sheet.scroll_to_cursor = true;
        }
        if copy {
            ui.ctx().copy_text(sheet.copy_text());
        }
    }

    // ---------- The table ----------
    let cursor = sheet.cursor;
    let selection = sheet.selection();
    let block = sheet.anchor.is_some_and(|a| a != cursor);
    let order = sheet.order;
    let scroll_to = std::mem::take(&mut sheet.scroll_to_cursor);
    let mut clicked: Option<(usize, usize, bool)> = None;
    let mut drag_from: Option<(usize, usize)> = None;
    let mut drag_to: Option<(usize, usize)> = None;
    let mut heading_clicked = None;
    let mut dragged: Option<(usize, f32)> = None;
    let mut fit: Option<usize> = None;
    let dragging = ui.data(|d| d.get_temp::<bool>(focus_id.with("dragging"))).unwrap_or(false);
    let scrolled: (f32, f32) = ui.data(|d| d.get_temp(focus_id.with("scrolled"))).unwrap_or((0.0, 0.0));
    let mut now_scrolled = scrolled;
    let pointer_down = ui.input(|i| i.pointer.primary_down());
    let pointer = ui.input(|i| i.pointer.interact_pos());
    egui::ScrollArea::horizontal().id_salt("data-across").auto_shrink([false, false]).show(ui, |ui| {
        ui.set_min_width(total_width);
        let left_edge = ui.clip_rect().left();
        // Where each column starts on screen; the row numbers stay in view when the table is scrolled sideways.
        let place = |row_left: f32| {
            let numbers_at = row_left + (left_edge - row_left).max(0.0);
            let mut starts = Vec::with_capacity(columns);
            let mut x = row_left + number_width;
            for width in &widths {
                starts.push(x);
                x += width;
            }
            (numbers_at, starts)
        };
        let column_at = |row_left: f32, x: f32| -> Option<usize> {
            let (_, starts) = place(row_left);
            (0..columns).rev().find(|&c| x >= starts[c]).filter(|&c| x < starts[c] + widths[c])
        };

        let (head_rect, _) = ui.allocate_exact_size(egui::vec2(total_width, row_height), Sense::hover());
        let painter = ui.painter_at(head_rect.intersect(ui.clip_rect()));
        painter.rect_filled(head_rect, 0.0, theme::gutter_back());
        let (numbers_at, starts) = place(head_rect.min.x);
        for column in 0..columns {
            let rect = Rect::from_min_size(egui::pos2(starts[column], head_rect.min.y), egui::vec2(widths[column], row_height));
            let mut name = table.columns[column].name.clone();
            if let Some((c, backwards)) = order
                && c == column
            {
                name = format!("{name} {}", if backwards { "\u{25BC}" } else { "\u{25B2}" });
            }
            let colour = if (selection.0.1..=selection.1.1).contains(&column) { theme::caret() } else { theme::editor_text() };
            painter.with_clip_rect(rect.shrink2(egui::vec2(6.0, 0.0)).intersect(ui.clip_rect())).text(rect.left_center() + egui::vec2(6.0, 0.0), egui::Align2::LEFT_CENTER, super::table_view::shown_value(&name), font.clone(), colour);
            painter.vline(rect.right(), rect.y_range(), egui::Stroke::new(1.0, theme::border()));
            let response = ui.interact(rect.shrink2(egui::vec2(4.0, 0.0)), focus_id.with(("heading", column)), Sense::click());
            if response.clicked() {
                heading_clicked = Some(column);
            }
            let words = crate::i18n::tf!("{}. Click to show the rows in order of this column; again for the other way; a third time as in the table.", kind_words(table.columns[column].kind));
            response.on_hover_text(words);
            let edge = Rect::from_center_size(egui::pos2(rect.right(), rect.center().y), egui::vec2(8.0, row_height));
            let drag = ui.interact(edge, focus_id.with(("edge", column)), Sense::click_and_drag()).on_hover_cursor(egui::CursorIcon::ResizeHorizontal).on_hover_text(crate::i18n::tr("Drag to change the width; double-click to fit what's in it"));
            if drag.dragged() {
                dragged = Some((column, (widths[column] + drag.drag_delta().x).max(MIN_WIDTH)));
            }
            if drag.double_clicked() {
                fit = Some(column);
            }
        }
        let corner = Rect::from_min_size(egui::pos2(numbers_at, head_rect.min.y), egui::vec2(number_width, row_height));
        painter.rect_filled(corner, 0.0, theme::gutter_back());
        painter.hline(head_rect.x_range(), head_rect.bottom(), egui::Stroke::new(1.0, theme::border()));

        let mut area = egui::ScrollArea::vertical().id_salt("data-down").auto_shrink([false, false]);
        if scroll_to {
            let (offset, height) = scrolled;
            let top = cursor.0 as f32 * row_height;
            if top < offset {
                area = area.vertical_scroll_offset(top);
            } else if height > 0.0 && top + row_height > offset + height {
                area = area.vertical_scroll_offset((top + row_height - height).max(0.0));
            }
        }
        let output = area.show_rows(ui, row_height, rows.len(), |ui, range| {
            for row in range {
                let cells = &table.rows[rows[row] as usize];
                let (rect, response) = ui.allocate_exact_size(egui::vec2(total_width, row_height), Sense::click_and_drag());
                let painter = ui.painter_at(rect.intersect(ui.clip_rect()));
                painter.rect_filled(rect, 0.0, if row % 2 == 1 { theme::active_line() } else { theme::editor_back() });
                let (numbers_at, starts) = place(rect.min.x);
                // Only the columns on screen are drawn.
                for column in (0..columns).filter(|&c| starts[c] + widths[c] >= ui.clip_rect().left() && starts[c] <= ui.clip_rect().right()) {
                    let cell = Rect::from_min_size(egui::pos2(starts[column], rect.min.y), egui::vec2(widths[column], row_height));
                    let here = (row, column) == cursor;
                    let chosen = (selection.0.0..=selection.1.0).contains(&row) && (selection.0.1..=selection.1.1).contains(&column);
                    if chosen && (block || here) {
                        painter.rect_filled(cell, 0.0, theme::selection());
                    }
                    if here {
                        painter.rect_stroke(cell.shrink(1.0), 0.0, egui::Stroke::new(1.5, theme::caret()), egui::StrokeKind::Inside);
                    }
                    if let Some(value) = cells.get(column)
                        && *value != crate::data_table::Cell::Null
                    {
                        let inner = cell.shrink2(egui::vec2(6.0, 0.0));
                        let (anchor, align) = if right_aligned(table.columns[column].kind) { (inner.right_center(), egui::Align2::RIGHT_CENTER) } else { (inner.left_center(), egui::Align2::LEFT_CENTER) };
                        let colour = if matches!(value, crate::data_table::Cell::Bytes(_)) { theme::text_dim() } else { theme::editor_text() };
                        painter.with_clip_rect(inner.intersect(ui.clip_rect())).text(anchor, align, super::table_view::shown_value(&value.text()), font.clone(), colour);
                    }
                    painter.vline(cell.right(), cell.y_range(), egui::Stroke::new(1.0, theme::border()));
                }
                let number_rect = Rect::from_min_size(egui::pos2(numbers_at, rect.min.y), egui::vec2(number_width, row_height));
                painter.rect_filled(number_rect, 0.0, theme::gutter_back());
                let number_colour = if (selection.0.0..=selection.1.0).contains(&row) { theme::caret() } else { theme::gutter_text() };
                painter.text(number_rect.right_center() - egui::vec2(8.0, 0.0), egui::Align2::RIGHT_CENTER, rows[row] as usize + 1, font.clone(), number_colour);
                // A value too long for its column, or on more than one line, shows in full under the pointer.
                if let Some(pos) = response.hover_pos()
                    && !dragging
                    && let Some(column) = column_at(rect.min.x, pos.x)
                    && let Some(value) = cells.get(column)
                {
                    let text = value.text();
                    if text.contains('\n') || text.chars().count() as f32 * char_width + 12.0 > widths[column] {
                        response.clone().on_hover_text(text);
                    }
                }
                if (response.clicked() || response.double_clicked())
                    && let Some(pos) = response.interact_pointer_pos()
                    && let Some(column) = column_at(rect.min.x, pos.x)
                {
                    clicked = Some((row, column, ui.input(|i| i.modifiers.shift)));
                }
                if response.drag_started()
                    && let Some(pos) = response.interact_pointer_pos()
                    && let Some(column) = column_at(rect.min.x, pos.x)
                {
                    drag_from = Some((row, column));
                }
                // Dragging across cells selects them, wherever the drag began.
                if dragging
                    && pointer_down
                    && let Some(pos) = pointer
                    && pos.y >= rect.min.y
                    && pos.y < rect.max.y
                    && let Some(column) = column_at(rect.min.x, pos.x.clamp(rect.min.x + number_width, rect.max.x - 1.0))
                {
                    drag_to = Some((row, column));
                }
            }
        });
        now_scrolled = (output.state.clamped_offset().y, output.inner_rect.height());
    });
    ui.data_mut(|d| d.insert_temp(focus_id.with("scrolled"), now_scrolled));
    if rows.is_empty() {
        let words = if table.rows.is_empty() { crate::i18n::tr("There are no rows in this table.") } else { crate::i18n::tr("No rows contain that.") };
        ui.label(RichText::new(words).color(theme::text_dim()));
    }
    if let Some((column, width)) = dragged {
        sheet.widths[column] = width;
    }
    if let Some(column) = fit {
        let longest = rows.iter().take(5000).map(|&r| table.rows[r as usize].get(column).map_or(0, |c| super::table_view::shown_value(&c.text()).chars().count())).chain(std::iter::once(table.columns[column].name.chars().count() + 3)).max().unwrap_or(4);
        sheet.widths[column] = (longest as f32 * char_width + 18.0).clamp(MIN_WIDTH, 900.0);
    }
    let take_keyboard = |ui: &Ui| ui.memory_mut(|m| m.request_focus(focus_id));
    if let Some(from) = drag_from {
        ui.data_mut(|d| d.insert_temp(focus_id.with("dragging"), true));
        sheet.anchor = Some(from);
        sheet.cursor = from;
        take_keyboard(ui);
    }
    if let Some(to) = drag_to
        && dragging
    {
        sheet.cursor = to;
    }
    if !pointer_down && dragging {
        ui.data_mut(|d| d.insert_temp(focus_id.with("dragging"), false));
    }
    if let Some(column) = heading_clicked {
        sheet.click_heading(column);
    }
    if let Some((row, column, shift)) = clicked {
        if shift {
            sheet.anchor.get_or_insert(cursor);
        } else {
            sheet.anchor = None;
        }
        sheet.cursor = (row, column);
        take_keyboard(ui);
    }
}

/// The strip beside the table: what the column the cursor is in holds.
fn draw_strip(ui: &mut Ui, sheet: &mut SheetState) {
    let column = sheet.cursor.1.min(sheet.table.columns.len().saturating_sub(1));
    let Some(heading) = sheet.table.columns.get(column).map(|c| (c.name.clone(), c.kind)) else { return };
    let ctx = ui.ctx().clone();
    let summary = sheet.summary(column, Some(&ctx)).cloned();
    egui::Frame::new().fill(theme::gutter_back()).inner_margin(egui::Margin::same(10)).show(ui, |ui| {
        ui.set_min_size(ui.available_size());
        egui::ScrollArea::vertical().id_salt("data-strip").auto_shrink([false, false]).show(ui, |ui| {
            ui.label(RichText::new(heading.0).strong());
            ui.label(RichText::new(kind_words(heading.1)).color(theme::text_dim()));
            ui.add_space(6.0);
            let Some(summary) = summary else {
                ui.label(RichText::new(crate::i18n::tr("Working it out\u{2026}")).color(theme::text_dim()));
                return;
            };
            summary_rows(ui, &summary);
        });
    });
}

fn summary_rows(ui: &mut Ui, summary: &Summary) {
    let line = |ui: &mut Ui, what: String, value: String| {
        ui.horizontal_wrapped(|ui| {
            ui.label(RichText::new(what).color(theme::text_dim()));
            ui.label(value);
        });
    };
    line(ui, crate::i18n::tr("Rows").to_string(), grouped(summary.count));
    line(ui, crate::i18n::tr("Empty").to_string(), grouped(summary.empty));
    line(ui, crate::i18n::tr("Different values").to_string(), grouped(summary.distinct));
    if let Some(smallest) = &summary.smallest {
        line(ui, crate::i18n::tr("Smallest").to_string(), super::table_view::shown_value(smallest));
    }
    if let Some(largest) = &summary.largest {
        line(ui, crate::i18n::tr("Largest").to_string(), super::table_view::shown_value(largest));
    }
    if let Some(mean) = summary.mean {
        line(ui, crate::i18n::tr("Mean").to_string(), number_text(mean));
    }
    if !summary.common.is_empty() {
        ui.add_space(8.0);
        ui.label(RichText::new(crate::i18n::tr("Commonest values")).color(theme::text_dim()));
        for (value, count) in &summary.common {
            let shown = if value.is_empty() { "\u{201C}\u{201D}".to_string() } else { super::table_view::shown_value(value) };
            ui.horizontal_wrapped(|ui| {
                ui.label(RichText::new(format!("{}\u{00D7}", grouped(*count))).color(theme::text_dim()));
                ui.label(shown);
            });
        }
    }
}
