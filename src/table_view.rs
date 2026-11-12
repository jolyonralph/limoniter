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
    shown: Arc<Vec<usize>>,
    shown_for: Option<ShownKey>,
    /// The cell the cursor is in: a place in the rows shown, and a column.
    pub cursor: (usize, usize),
    /// The other corner of a block of selected cells, if more than the cursor's cell is selected.
    pub anchor: Option<(usize, usize)>,
    /// Cells are being selected by dragging across them.
    dragging: bool,
    /// The first column stays in view when the table is scrolled sideways.
    pub frozen: bool,
    /// The figures for the selection, and what they were worked out from.
    figures: Option<(FiguresKey, String)>,
    /// For a file too large to hold, each of these is worked out in the background; what's shown carries on from
    /// the last until it's done.
    index_work: Option<((u64, char), Background<Index>)>,
    shown_work: Option<(ShownKey, Background<Vec<usize>>)>,
    figures_work: Option<(FiguresKey, Background<String>)>,
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

/// The text's version, the delimiter, whether there are headings, the order and the filter.
type ShownKey = (u64, char, bool, Option<(usize, bool)>, String);
/// The text's version, how many rows show, and the selected block, or the cursor's column for one cell.
type FiguresKey = (u64, usize, Option<((usize, usize), (usize, usize))>, usize);

/// Something worked out on another thread, which is told to stop if it's no longer wanted.
struct Background<T> {
    result: Arc<Mutex<Option<T>>>,
    done: Arc<std::sync::atomic::AtomicUsize>,
    total: usize,
    cancel: Arc<std::sync::atomic::AtomicBool>,
}

impl<T: Send + 'static> Background<T> {
    /// Starts the work, which tells how far it's got out of the total and gives up, with None, when told to.
    fn start(total: usize, work: impl FnOnce(&mut dyn FnMut(usize) -> bool) -> Option<T> + Send + 'static) -> Self {
        use std::sync::atomic::Ordering;
        let (result, done, cancel) = (Arc::new(Mutex::new(None)), Arc::new(std::sync::atomic::AtomicUsize::new(0)), Arc::new(std::sync::atomic::AtomicBool::new(false)));
        let (to, so_far, stop) = (result.clone(), done.clone(), cancel.clone());
        std::thread::spawn(move || {
            let mut going = |at: usize| {
                so_far.store(at, Ordering::Relaxed);
                !stop.load(Ordering::Relaxed)
            };
            if let Ok(Some(found)) = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| work(&mut going))) {
                *to.lock().unwrap_or_else(std::sync::PoisonError::into_inner) = Some(found);
            }
        });
        Self { result, done, total, cancel }
    }

    fn take(&self) -> Option<T> {
        self.result.lock().unwrap_or_else(std::sync::PoisonError::into_inner).take()
    }

    fn share(&self) -> f32 {
        self.done.load(std::sync::atomic::Ordering::Relaxed) as f32 / self.total.max(1) as f32
    }
}

impl<T> Drop for Background<T> {
    fn drop(&mut self) {
        self.cancel.store(true, std::sync::atomic::Ordering::Relaxed);
    }
}

impl TableState {
    /// Whether anything is being worked out in the background.
    pub(super) fn working(&self) -> bool {
        self.index_work.is_some() || self.shown_work.is_some() || self.figures_work.is_some()
    }

    /// Whether the records are up to date with the text, so cells can be changed.
    pub(super) fn settled(&self) -> bool {
        self.index_work.is_none()
    }

    /// The selected block, top-left and bottom-right, in the rows shown.
    pub(super) fn selection(&self) -> ((usize, usize), (usize, usize)) {
        let (a, b) = (self.anchor.unwrap_or(self.cursor), self.cursor);
        ((a.0.min(b.0), a.1.min(b.1)), (a.0.max(b.0), a.1.max(b.1)))
    }

    pub(super) fn new(dialect: Dialect) -> Self {
        TableState { dialect, index: None, widths: Vec::new(), order: None, filter: String::new(), shown: Arc::default(), shown_for: None, cursor: (0, 0), anchor: None, dragging: false, frozen: false, figures: None, index_work: None, shown_work: None, figures_work: None, editing: None, seen_caret: (usize::MAX, 0), scroll_to_cursor: true, take_keyboard: true, scrolled: (0.0, 0.0) }
    }
}

/// What the table's controls ask for.
enum Action {
    Commit { record: usize, column: usize, value: String },
    /// Cells filled from a corner, row by row in the order shown; rows past the end are added.
    Fill { at: (usize, usize), block: Vec<Vec<String>> },
    /// The selected cells emptied.
    Clear,
    InsertRow,
    DeleteRow,
    InsertColumn,
    DeleteColumn,
    SortFile,
    TextView,
}

/// Copy, Cut or Paste, as the keyboard sends them.
enum ClipboardEvent {
    Copy,
    Cut,
    Paste(String),
}

/// The lines of a document, shared: those of a file kept on disk are read as they're needed.
fn lines_of_doc(text: &text_buffer::TextBuffer) -> LineStore {
    text.store().clone()
}

fn record_fields<L: table::Lines + ?Sized>(lines: &L, record: Record, delimiter: char) -> Vec<String> {
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
        if doc.hex_mode || doc.browser.is_some() || doc.table.is_some() || doc.text.line_count() < 2 {
            return;
        }
        let ending = doc.path.as_deref().and_then(Path::extension).and_then(|e| e.to_str()).map(str::to_string);
        let sample: Vec<Arc<str>> = (0..doc.text.line_count().min(200)).map(|i| doc.text.line(i)).collect();
        doc.table_offer = table::sniff(&sample, ending.as_deref());
        // CSV and TSV files can open straight into a table, if that's been asked for.
        if self.prefs.open_tables && doc.table_offer.is_some() && ending.as_deref().is_some_and(is_table_ending) {
            self.toggle_table();
        }
    }

    /// The bar offering table view for a file that looks like delimited data.
    pub(super) fn table_offer_bar(&mut self, ui: &mut Ui) {
        let doc = &self.docs[self.active];
        let Some(dialect) = doc.table_offer.filter(|_| doc.table.is_none() && !doc.hex_mode) else {
            return;
        };
        let name = table::DELIMITERS.iter().find(|(_, d)| *d == dialect.delimiter).map_or("delimited", |(n, _)| n).to_lowercase();
        let (mut show, mut dismiss, mut always) = (false, false, false);
        let ending = doc.path.as_deref().and_then(Path::extension).and_then(|e| e.to_str()).is_some_and(is_table_ending);
        Panel::top("table-offer").frame(egui::Frame::new().fill(theme::selected()).inner_margin(egui::Margin::symmetric(8, 4))).show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label(RichText::new(format!("This looks like {name}-separated data.")).strong());
                show = ui.button("Show it as a table").clicked();
                dismiss = ui.button("No thanks").clicked();
                if ending {
                    always = ui.button("Always open .csv and .tsv files as tables").on_hover_text("Also in Settings, under Files").clicked();
                }
            });
        });
        if always {
            self.prefs.open_tables = true;
            self.save_settings_if_changed();
        }
        if show || always {
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
        // A file too large to hold has its records found, rows sorted and figures worked out in the background.
        let large = lines.is_disk_backed();
        let mut working: Option<(&str, f32)> = None;
        let scanning = doc.scanning.as_ref().map(|source| source.progress());
        if let Some(share) = scanning {
            // A file just rewritten is still having its lines found; the records are read once it has.
            working = Some(("Reading the file", share));
            state.index_work = None;
        } else if state.index.as_ref().is_none_or(|i| i.version != version || i.delimiter != delimiter) {
            if !large {
                state.index = Some(Index::build(&lines, delimiter, version));
                state.index_work = None;
            } else {
                if state.index_work.as_ref().is_none_or(|(key, _)| *key != (version, delimiter)) {
                    let store = lines.clone();
                    state.index_work = Some(((version, delimiter), Background::start(lines.len() * 2, move |going| Index::build_watched(&store, delimiter, version, going))));
                }
                if let Some(index) = state.index_work.as_ref().and_then(|(_, work)| work.take()) {
                    // Rows still shown from before can't point past the records there now.
                    let count = index.records.len();
                    if state.shown.iter().any(|&r| r >= count) {
                        state.shown = Arc::new(state.shown.iter().copied().filter(|&r| r < count).collect());
                    }
                    state.index = Some(index);
                    state.index_work = None;
                }
            }
        }
        if let Some((_, work)) = &state.index_work {
            working = Some(("Reading the records", work.share()));
        }
        let Some(index) = state.index.clone() else {
            let share = working.map_or(0.0, |(_, share)| share);
            ui.centered_and_justified(|ui| ui.label(format!("Reading the records\u{2026} {:.0}%", share * 100.0)));
            ui.ctx().request_repaint_after(std::time::Duration::from_millis(100));
            return;
        };
        let read_only = read_only || state.index_work.is_some() || scanning.is_some();
        let records = index.records.clone();
        let header = state.dialect.header && !records.is_empty();
        let first_data = usize::from(header);
        let columns = index.columns.max(1);

        // The rows shown: filtered, then sorted, as asked. Worked out from the records as they are now.
        let key = (index.version, index.delimiter, header, state.order, state.filter.clone());
        if state.shown_for.as_ref() != Some(&key) {
            let (needle, order) = (state.filter.to_lowercase(), state.order);
            if !large {
                state.shown = Arc::new(shown_rows(&lines, &records, first_data, &needle, order, delimiter, &mut |_| true).unwrap_or_default());
                state.shown_for = Some(key);
                state.shown_work = None;
            } else {
                if state.shown_work.as_ref().is_none_or(|(at, _)| *at != key) {
                    let (store, records) = (lines.clone(), records.clone());
                    let total = records.len();
                    state.shown_work = Some((key.clone(), Background::start(total, move |going| shown_rows(&store, &records, first_data, &needle, order, delimiter, going))));
                }
                if let Some(shown) = state.shown_work.as_ref().and_then(|(_, work)| work.take()) {
                    state.shown = Arc::new(shown);
                    state.shown_for = Some(key);
                    state.shown_work = None;
                }
            }
        }
        if let Some((_, work)) = &state.shown_work {
            working = working.or(Some((if state.order.is_some() { "Sorting" } else { "Finding the rows" }, work.share())));
        }
        let shown = state.shown.clone();
        state.cursor.0 = state.cursor.0.min(shown.len().saturating_sub(1));
        state.cursor.1 = state.cursor.1.min(columns - 1);

        // A Find or Go to that moved the text's cursor moves the table's to the row it's in.
        let caret = (doc.text.caret_line, version);
        if caret.0 != state.seen_caret.0 {
            let record = records.partition_point(|r| r.last < caret.0);
            if records.get(record).is_some_and(|r| r.first <= caret.0)
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

        // Figures for the selected block, or for the whole of the cursor's column when one cell is selected.
        let block = state.anchor.is_some_and(|a| a != state.cursor);
        let figures_key = (index.version, shown.len(), block.then(|| state.selection()), state.cursor.1);
        if state.figures.as_ref().is_none_or(|(key, _)| *key != figures_key) {
            let ((top, left), (bottom, right)) = state.selection();
            let (rows, cols) = if block { (top..=bottom.min(shown.len().saturating_sub(1)), left..=right) } else { (0..=shown.len().saturating_sub(1), state.cursor.1..=state.cursor.1) };
            let label = if block {
                format!("{} by {} cells", bottom - top + 1, right - left + 1)
            } else {
                let name = if header { names.get(state.cursor.1).cloned().unwrap_or_default() } else { column_letter(state.cursor.1) };
                format!("Column {}", if name.trim().is_empty() { column_letter(state.cursor.1) } else { name })
            };
            if !large {
                let figures = figures_of(&lines, &records, &shown, rows, cols, delimiter, &mut |_| true).unwrap_or_default();
                state.figures = Some((figures_key, format!("{label}: {figures}")));
                state.figures_work = None;
            } else {
                if state.figures_work.as_ref().is_none_or(|(at, _)| *at != figures_key) {
                    let (store, records, shown) = (lines.clone(), records.clone(), shown.clone());
                    let total = rows.end().saturating_sub(*rows.start()) + 1;
                    state.figures_work = Some((figures_key, Background::start(total, move |going| figures_of(&store, &records, &shown, rows, cols, delimiter, going).map(|figures| format!("{label}: {figures}")))));
                }
                if let Some(figures) = state.figures_work.as_ref().and_then(|(_, work)| work.take()) {
                    state.figures = Some((figures_key, figures));
                    state.figures_work = None;
                }
            }
        }
        if let Some((_, work)) = &state.figures_work {
            working = working.or(Some(("Working out the figures", work.share())));
        }
        let figures = match working {
            Some((what, share)) => {
                ui.ctx().request_repaint_after(std::time::Duration::from_millis(100));
                format!("{what}\u{2026} {:.0}%", share * 100.0)
            }
            None => state.figures.as_ref().map(|(_, text)| text.clone()).unwrap_or_default(),
        };

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
                ui.checkbox(&mut state.frozen, "Keep the first column in view").on_hover_text("When the table is scrolled sideways");
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
                ui.label(RichText::new(&figures).color(theme::text_dim())).on_hover_text("Double-click or type to change a cell; Enter keeps it, Esc doesn't. Shift and the arrow keys, or dragging, select a block to copy, paste over or clear.");
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
        let mut clipboard = None;
        if focused && input_on && !editing && !shown.is_empty() {
            let page = ((ui.available_height() / row_height) as usize).saturating_sub(2).max(1);
            let (row, column) = state.cursor;
            let mut start_with = None;
            ui.input(|i| {
                // Copy, Cut and Paste come as events of their own, whichever keys a system uses for them.
                for event in &i.events {
                    match event {
                        egui::Event::Copy => clipboard = Some(ClipboardEvent::Copy),
                        egui::Event::Cut => clipboard = Some(ClipboardEvent::Cut),
                        egui::Event::Paste(text) => clipboard = Some(ClipboardEvent::Paste(text.clone())),
                        _ => {}
                    }
                }
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
                    // Shift and a move selects a block; Tab, or a move alone, leaves just the cursor's cell.
                    if i.modifiers.shift && !i.key_pressed(Key::Tab) {
                        state.anchor.get_or_insert(state.cursor);
                    } else {
                        state.anchor = None;
                    }
                    state.cursor = (r.min(shown.len() - 1), c.min(columns - 1));
                    state.scroll_to_cursor = true;
                }
                if i.key_pressed(Key::Escape) {
                    state.anchor = None;
                }
                if !read_only {
                    if i.key_pressed(Key::Enter) || i.key_pressed(Key::F2) {
                        start_with = Some(None);
                    } else if i.key_pressed(Key::Delete) || i.key_pressed(Key::Backspace) {
                        action = Some(Action::Clear);
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
                state.anchor = None;
                let current = record_fields(&lines, records[shown[state.cursor.0]], delimiter).get(state.cursor.1).cloned().unwrap_or_default();
                state.editing = Some((start.unwrap_or(current), true));
            }
        }

        // ---------- The table ----------
        let widths = state.widths.clone();
        let total_width = number_width + widths.iter().take(columns).sum::<f32>();
        let mut clicked: Option<(usize, usize, bool, bool)> = None;
        let mut drag_to: Option<(usize, usize)> = None;
        let mut drag_from: Option<(usize, usize)> = None;
        let mut heading_clicked = None;
        let mut dragged: Option<(usize, f32)> = None;
        let mut fit: Option<usize> = None;
        let mut commit_edit: Option<(bool, (isize, isize))> = None;
        let cursor = state.cursor;
        let scroll_to = std::mem::take(&mut state.scroll_to_cursor);
        let order = state.order;
        let scrolled = state.scrolled;
        let frozen = state.frozen;
        let dragging = state.dragging;
        let selection = state.selection();
        let block = state.anchor.is_some_and(|a| a != cursor);
        let mut now_scrolled = scrolled;
        let pointer_down = ui.input(|i| i.pointer.primary_down());
        let pointer = ui.input(|i| i.pointer.interact_pos());
        egui::ScrollArea::horizontal().id_salt("table-across").auto_shrink([false, false]).show(ui, |ui| {
            ui.set_min_width(total_width);
            // How far the table is scrolled sideways: the row numbers, and the first column if asked, stay in view.
            let left_edge = ui.clip_rect().left();
            // Where each column starts on screen, and which column a point is in.
            let place = |row_left: f32| {
                let across = (left_edge - row_left).max(0.0);
                let numbers_at = row_left + across;
                let mut starts = Vec::with_capacity(columns);
                let mut x = row_left + number_width;
                for width in widths.iter().take(columns) {
                    starts.push(x);
                    x += width;
                }
                let pinned = frozen && across > 0.0;
                if pinned {
                    starts[0] = numbers_at + number_width;
                }
                (numbers_at, starts, pinned)
            };
            let column_at = |row_left: f32, x: f32| -> Option<usize> {
                let (_, starts, pinned) = place(row_left);
                if pinned && x >= starts[0] && x < starts[0] + widths[0] {
                    return Some(0);
                }
                (0..columns).rev().find(|&c| x >= starts[c] && (c > 0 || !pinned)).filter(|&c| x < starts[c] + widths[c])
            };

            // The headings stay put while the rows scroll under them.
            let (head_rect, _) = ui.allocate_exact_size(egui::vec2(total_width, row_height), Sense::hover());
            let painter = ui.painter_at(head_rect.intersect(ui.clip_rect()));
            painter.rect_filled(head_rect, 0.0, theme::gutter_back());
            let (numbers_at, starts, pinned) = place(head_rect.min.x);
            let draw_heading = |column: usize, x: f32| {
                let rect = Rect::from_min_size(egui::pos2(x, head_rect.min.y), egui::vec2(widths[column], row_height));
                painter.rect_filled(rect, 0.0, theme::gutter_back());
                let mut name = if header { names.get(column).cloned().unwrap_or_default() } else { column_letter(column) };
                if let Some((c, up)) = order
                    && c == column
                {
                    name = format!("{name} {}", if up { "\u{25B2}" } else { "\u{25BC}" });
                }
                let colour = if (selection.0.1..=selection.1.1).contains(&column) { theme::caret() } else { theme::editor_text() };
                painter.with_clip_rect(rect.shrink2(egui::vec2(6.0, 0.0)).intersect(ui.clip_rect())).text(rect.left_center() + egui::vec2(6.0, 0.0), egui::Align2::LEFT_CENTER, shown_value(&name), font.clone(), colour);
                painter.vline(rect.right(), rect.y_range(), egui::Stroke::new(1.0, theme::border()));
                rect
            };
            for column in (0..columns).filter(|&c| c > 0 || !pinned).chain(pinned.then_some(0)) {
                let rect = draw_heading(column, starts[column]);
                let response = ui.interact(rect.shrink2(egui::vec2(4.0, 0.0)), focus_id.with(("heading", column)), Sense::click());
                if response.clicked() {
                    heading_clicked = Some(column);
                }
                response.on_hover_text("Click to show the rows in order of this column; again for the other way; a third time as in the file.");
                // The edge of a heading can be dragged to make the column wider or narrower, or double-clicked to fit.
                let edge = Rect::from_center_size(egui::pos2(rect.right(), rect.center().y), egui::vec2(8.0, row_height));
                let drag = ui.interact(edge, focus_id.with(("edge", column)), Sense::click_and_drag()).on_hover_cursor(egui::CursorIcon::ResizeHorizontal).on_hover_text("Drag to change the width; double-click to fit what's in it");
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
                    let (rect, response) = ui.allocate_exact_size(egui::vec2(total_width, row_height), Sense::click_and_drag());
                    let painter = ui.painter_at(rect.intersect(ui.clip_rect()));
                    let stripe = if row % 2 == 1 { theme::active_line() } else { theme::editor_back() };
                    painter.rect_filled(rect, 0.0, stripe);
                    let (numbers_at, starts, pinned) = place(rect.min.x);
                    let draw_cell = |column: usize| {
                        let cell = Rect::from_min_size(egui::pos2(starts[column], rect.min.y), egui::vec2(widths[column], row_height));
                        if pinned && column == 0 {
                            painter.rect_filled(cell, 0.0, stripe);
                        }
                        let here = (row, column) == cursor;
                        let chosen = (selection.0.0..=selection.1.0).contains(&row) && (selection.0.1..=selection.1.1).contains(&column);
                        if chosen && (block || here) {
                            painter.rect_filled(cell, 0.0, theme::selection());
                        }
                        if here {
                            painter.rect_stroke(cell.shrink(1.0), 0.0, egui::Stroke::new(1.5, theme::caret()), egui::StrokeKind::Inside);
                        }
                        if let Some(value) = fields.get(column) {
                            let align_right = table::number(value).is_some();
                            let inner = cell.shrink2(egui::vec2(6.0, 0.0));
                            let (anchor, align) = if align_right { (inner.right_center(), egui::Align2::RIGHT_CENTER) } else { (inner.left_center(), egui::Align2::LEFT_CENTER) };
                            painter.with_clip_rect(inner.intersect(ui.clip_rect())).text(anchor, align, shown_value(value), font.clone(), theme::editor_text());
                        }
                        painter.vline(cell.right(), cell.y_range(), egui::Stroke::new(1.0, theme::border()));
                    };
                    for column in (0..columns).filter(|&c| c > 0 || !pinned) {
                        draw_cell(column);
                    }
                    if pinned {
                        draw_cell(0);
                    }
                    // Rows that don't have the usual number of fields are marked by their number.
                    let uneven = record.broken || (header && fields.len() != names.len());
                    let number_rect = Rect::from_min_size(egui::pos2(numbers_at, rect.min.y), egui::vec2(number_width, row_height));
                    painter.rect_filled(number_rect, 0.0, theme::gutter_back());
                    let in_rows = (selection.0.0..=selection.1.0).contains(&row);
                    let number_colour = if uneven { theme::mistake() } else if in_rows { theme::caret() } else { theme::gutter_text() };
                    painter.text(number_rect.right_center() - egui::vec2(8.0, 0.0), egui::Align2::RIGHT_CENTER, format!("{}", shown[row] + 1 - first_data), font.clone(), number_colour);
                    if uneven {
                        let why = if record.broken { "A quote here is never closed.".to_string() } else { format!("This row has {} fields; the headings have {}.", fields.len(), names.len()) };
                        ui.interact(number_rect, focus_id.with(("number", row)), Sense::hover()).on_hover_text(why);
                    }
                    // A value too long for its column, or on more than one line, shows in full under the pointer.
                    if let Some(pos) = response.hover_pos()
                        && !dragging
                        && let Some(column) = column_at(rect.min.x, pos.x)
                        && let Some(value) = fields.get(column)
                        && (value.contains('\n') || value.chars().count() as f32 * char_width + 12.0 > widths[column])
                    {
                        response.clone().on_hover_text(value.clone());
                    }
                    if (response.clicked() || response.double_clicked())
                        && let Some(pos) = response.interact_pointer_pos()
                        && let Some(column) = column_at(rect.min.x, pos.x)
                    {
                        clicked = Some((row, column, response.double_clicked(), ui.input(|i| i.modifiers.shift)));
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
                    // The cell being typed into has a box of its own over it.
                    if cursor.0 == row && editing {
                        let cell = Rect::from_min_size(egui::pos2(starts[cursor.1], rect.min.y), egui::vec2(widths[cursor.1].max(160.0), row_height));
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
        if let Some(column) = fit {
            // As wide as the widest value shown, within reason, the heading included.
            let longest = shown.iter().take(5000).map(|&r| record_fields(&lines, records[r], delimiter).get(column).map_or(0, |v| shown_value(v).chars().count())).chain(names.get(column).map(|n| n.chars().count() + 2)).max().unwrap_or(4);
            state.widths[column] = (longest as f32 * char_width + 18.0).clamp(MIN_WIDTH, 900.0);
        }
        if let Some(from) = drag_from
            && state.editing.is_none()
        {
            state.dragging = true;
            state.anchor = Some(from);
            state.cursor = from;
            take_keyboard(ui, focus_id);
        }
        if let Some(to) = drag_to
            && state.dragging
        {
            state.cursor = to;
        }
        if !pointer_down {
            state.dragging = false;
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
        if let Some((row, column, double, shift)) = clicked {
            if shift {
                state.anchor.get_or_insert(cursor);
            } else {
                state.anchor = None;
            }
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
        match clipboard {
            Some(ClipboardEvent::Copy) => {
                self.table_command(Command::Copy, ui);
            }
            Some(ClipboardEvent::Cut) => {
                self.table_command(Command::Cut, ui);
            }
            Some(ClipboardEvent::Paste(text)) => self.table_paste(&text),
            None => {}
        }
    }

    /// Copy, Cut, Paste and Select All work on cells while the table is shown: a block of cells goes to the
    /// clipboard as tab-separated text, as spreadsheets copy it, and a block pasted fills cells from the cursor.
    /// True if it took the command.
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
        if state.shown.is_empty() && command != Command::Paste {
            return true;
        }
        let shown = state.shown.clone();
        let read_only = doc.text.read_only || !state.settled();
        let block = |state: &TableState| {
            let ((top, left), (bottom, right)) = state.selection();
            (top..=bottom.min(shown.len().saturating_sub(1)))
                .map(|row| {
                    let fields = record_fields(&lines, index.records[shown[row]], delimiter);
                    (left..=right).map(|c| fields.get(c).cloned().unwrap_or_default()).collect::<Vec<_>>()
                })
                .collect::<Vec<_>>()
        };
        match command {
            Command::SelectAll => {
                state.anchor = Some((0, 0));
                state.cursor = (shown.len() - 1, index.columns.max(1) - 1);
            }
            Command::Copy => ui.ctx().copy_text(table::block_text(&block(state))),
            Command::Cut if !read_only => {
                ui.ctx().copy_text(table::block_text(&block(state)));
                self.table_action(Action::Clear, &lines, &index.records, &shown, false);
            }
            Command::Paste if !read_only => {
                if let Ok(text) = arboard::Clipboard::new().and_then(|mut c| c.get_text()) {
                    self.table_paste(&text);
                }
            }
            _ => {}
        }
        true
    }

    /// A step before a picture: "sort" shows the rows in order of a column, "sort-file" puts the file in that
    /// order, and "insert-column" puts an empty column before it.
    pub(super) fn table_step(&mut self, name: &str, value: &str) {
        let column: usize = value.parse().unwrap_or(0);
        let doc = &mut self.docs[self.active];
        let Some(state) = doc.table.as_deref_mut() else {
            return;
        };
        let action = match name {
            "sort" => {
                state.order = Some((column, true));
                return;
            }
            "sort-file" => Action::SortFile,
            "insert-column" => Action::InsertColumn,
            _ => return,
        };
        state.cursor.1 = column;
        let lines = lines_of_doc(&doc.text);
        let (Some(index), shown, header) = (state.index.clone(), state.shown.clone(), state.dialect.header) else {
            return;
        };
        self.table_action(action, &lines, &index.records, &shown, header);
    }

    /// Pastes cells from the cursor's corner of the selection.
    fn table_paste(&mut self, text: &str) {
        let doc = &mut self.docs[self.active];
        if doc.text.read_only {
            return;
        }
        let Some(state) = doc.table.as_deref_mut() else {
            return;
        };
        if !state.settled() {
            return;
        }
        let lines = lines_of_doc(&doc.text);
        let Some(index) = state.index.clone() else {
            return;
        };
        let shown = state.shown.clone();
        let mut cells = table::parse_block(text);
        let ((top, left), (bottom, right)) = state.selection();
        // One value pasted over a block fills every cell of it, as a spreadsheet does.
        if cells.len() == 1 && cells[0].len() == 1 && state.anchor.is_some_and(|a| a != state.cursor) {
            cells = vec![vec![cells[0][0].clone(); right - left + 1]; bottom.min(shown.len().saturating_sub(1)) - top + 1];
        }
        let header = state.dialect.header;
        self.table_action(Action::Fill { at: (top, left), block: cells }, &lines, &index.records, &shown, header);
    }

    fn table_action(&mut self, action: Action, lines: &LineStore, records: &Arc<Vec<Record>>, shown: &[usize], header: bool) {
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
            Action::Fill { at, block } => {
                // Each row's changes, by the record they're in; rows past the end are added after the last record.
                let mut changes: Vec<(Record, Vec<(usize, String)>)> = Vec::new();
                let mut added: Vec<String> = Vec::new();
                for (i, cells) in block.iter().enumerate() {
                    let row_changes: Vec<(usize, String)> = cells.iter().enumerate().map(|(j, value)| (at.1 + j, value.clone())).collect();
                    match shown.get(at.0 + i) {
                        Some(&r) => changes.push((records[r], row_changes)),
                        None => added.push(table::set_fields(&table::empty_record(delimiter, columns), delimiter, &row_changes)),
                    }
                }
                let height = block.len();
                let width = block.iter().map(Vec::len).max().unwrap_or(1);
                apply_records(&mut doc.text, lines, records, changes, added, delimiter);
                if let Some(state) = doc.table.as_deref_mut() {
                    state.anchor = Some(at);
                    state.cursor = (at.0 + height.saturating_sub(1), at.1 + width.saturating_sub(1));
                    state.scroll_to_cursor = true;
                }
            }
            Action::Clear => {
                let ((top, left), (bottom, right)) = state.selection();
                let mut changes = Vec::new();
                for row in top..=bottom.min(shown.len().saturating_sub(1)) {
                    let Some(&r) = shown.get(row) else {
                        break;
                    };
                    // Only fields there are, with something in them: emptying never adds fields.
                    let fields = record_fields(lines, records[r], delimiter);
                    let emptied: Vec<(usize, String)> = (left..=right).filter(|&c| fields.get(c).is_some_and(|v| !v.is_empty())).map(|c| (c, String::new())).collect();
                    if !emptied.is_empty() {
                        changes.push((records[r], emptied));
                    }
                }
                apply_records(&mut doc.text, lines, records, changes, Vec::new(), delimiter);
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
            Action::InsertColumn | Action::DeleteColumn if lines.is_disk_backed() => {
                // Too large to hold: every record is rewritten into a new file, in the background.
                let inserting = matches!(action, Action::InsertColumn);
                let change: crate::big_jobs::RecordChange = Box::new(move |text| if inserting { table::insert_field(text, delimiter, column) } else { table::remove_field(text, delimiter, column) });
                state.widths.clear();
                state.order = None;
                let title = if inserting { "Insert column" } else { "Delete column" };
                self.start_record_rewrite(title, records.clone(), delimiter, crate::big_jobs::Arrange::AsTheyAre, change);
            }
            Action::SortFile if lines.is_disk_backed() => {
                state.order = None;
                self.start_record_rewrite("Sort the file", records.clone(), delimiter, crate::big_jobs::Arrange::SortedBy { column, headings: header }, Box::new(str::to_string));
            }
            Action::InsertColumn | Action::DeleteColumn => {
                let Some(last) = records.last() else {
                    return;
                };
                let mut rows = Vec::new();
                for r in records.iter() {
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

/// Changes several records, and adds rows after the last, as one step that one Undo takes back. Records are changed
/// from the bottom up, so a record that gains or loses lines doesn't move the ones still to change.
fn apply_records(text: &mut text_buffer::TextBuffer, lines: &LineStore, records: &[Record], mut changes: Vec<(Record, Vec<(usize, String)>)>, added: Vec<String>, delimiter: char) {
    if changes.is_empty() && added.is_empty() {
        return;
    }
    text.begin_undo_group(true);
    if !added.is_empty() {
        let new_lines: Vec<String> = added.iter().flat_map(|record| table::lines_of(record)).collect();
        match records.last() {
            Some(last) => {
                let mut rows = table::lines_of(&table::record_text(lines, *last));
                rows.extend(new_lines);
                text.replace_lines(last.first, last.last, rows);
            }
            None => {
                text.replace_lines(0, 0, new_lines);
            }
        }
    }
    changes.sort_by_key(|(record, _)| std::cmp::Reverse(record.first));
    for (record, fields) in changes {
        let old = table::record_text(lines, record);
        let new = table::set_fields(&old, delimiter, &fields);
        if new != old {
            text.replace_lines(record.first, record.last, table::lines_of(&new));
        }
    }
    text.end_undo_group();
}

/// The records to show, from the first after any headings: those containing the filter (already in small
/// letters), in the order asked for. None if told to stop.
fn shown_rows(lines: &LineStore, records: &[Record], first_data: usize, needle: &str, order: Option<(usize, bool)>, delimiter: char, going: &mut dyn FnMut(usize) -> bool) -> Option<Vec<usize>> {
    let mut shown = Vec::new();
    // Progress counts out of the number of records: all of it filtering, or half filtering and half sorting.
    let half = |at: usize| if order.is_some() { at / 2 } else { at };
    for (r, record) in records.iter().enumerate().skip(first_data) {
        if r % 4096 == 0 && !going(half(r)) {
            return None;
        }
        if needle.is_empty() || table::record_text(lines, *record).to_lowercase().contains(needle) {
            shown.push(r);
        }
    }
    if let Some((column, up)) = order {
        let mut keyed: Vec<(usize, table::SortKey)> = Vec::with_capacity(shown.len());
        for (i, &r) in shown.iter().enumerate() {
            if i % 4096 == 0 && !going(records.len() / 2 + i * records.len() / 2 / shown.len().max(1)) {
                return None;
            }
            let text = table::record_text(lines, records[r]);
            keyed.push((r, table::SortKey::new(table::fields(&text, delimiter).into_iter().nth(column).map(|f| f.value).unwrap_or_default())));
        }
        keyed.sort_by(|a, b| {
            let order = a.1.cmp(&b.1);
            if up { order } else { order.reverse() }
        });
        shown = keyed.into_iter().map(|(r, _)| r).collect();
    }
    Some(shown)
}

/// The figures for a block of the rows shown. None if told to stop.
fn figures_of(lines: &LineStore, records: &[Record], shown: &[usize], rows: std::ops::RangeInclusive<usize>, columns: std::ops::RangeInclusive<usize>, delimiter: char, going: &mut dyn FnMut(usize) -> bool) -> Option<String> {
    let mut figures = table::Figures::new();
    if !shown.is_empty() {
        let start = *rows.start();
        for row in rows {
            if (row - start).is_multiple_of(4096) && !going(row - start) {
                return None;
            }
            let fields = record_fields(lines, records[shown[row]], delimiter);
            for column in columns.clone() {
                figures.add(fields.get(column).map_or("", String::as_str));
            }
        }
    }
    Some(figures.describe())
}

/// Gives the table the keyboard, with Tab and the arrow keys moving around it rather than to the next control.
fn take_keyboard(ui: &Ui, id: Id) {
    ui.memory_mut(|m| {
        m.request_focus(id);
        m.set_focus_lock_filter(id, egui::EventFilter { tab: true, horizontal_arrows: true, vertical_arrows: true, escape: false });
    });
}

/// Whether a file ending says the file is delimited data, to open as a table at once if that's asked for.
fn is_table_ending(ending: &str) -> bool {
    matches!(ending.to_ascii_lowercase().as_str(), "csv" | "tsv" | "tab" | "psv")
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
