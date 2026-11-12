//! Compare: two lists of lines side by side, with the differences marked. It only reads; nothing here changes a file.

use crate::line_store::LineStore;
use crate::text_buffer::display_text;
use crate::theme;
use eframe::egui::{self, Align2, Color32, Context, FontId, Rect, Sense, pos2, vec2};
use similar::{Algorithm, DiffOp, capture_diff_slices_deadline};
use std::sync::Arc;
use std::time::{Duration, Instant};

/// How long the search for the closest match may take. After that the result is still correct, but cruder.
const MOST_TIME: Duration = Duration::from_secs(5);
const ROW_TEXT: f32 = 13.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RowKind {
    Same,
    Changed,
    LeftOnly,
    RightOnly,
}

/// One row of the comparison: a line from either side, with its line number counted from 0.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Row {
    pub left: Option<(usize, Arc<str>)>,
    pub right: Option<(usize, Arc<str>)>,
    pub kind: RowKind,
}

/// A stretch of rows that are all alike, or all part of one change. A change pairs its lines up in order, and
/// whatever's left over on the longer side stands alone. Keeping stretches rather than rows means a comparison of
/// two huge files costs memory for its differences, not its lines.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Run {
    /// The first row it makes.
    row: usize,
    left: usize,
    right: usize,
    left_len: usize,
    right_len: usize,
    same: bool,
}

impl Run {
    fn rows(&self) -> usize {
        if self.same { self.left_len } else { self.left_len.max(self.right_len) }
    }
}

/// Works out the stretches, for lines or anything standing in for them (such as a hash of each).
pub fn runs<T: std::hash::Hash + Eq + Ord>(left: &[T], right: &[T]) -> Vec<Run> {
    let deadline = Some(Instant::now() + MOST_TIME);
    let mut out: Vec<Run> = Vec::new();
    let mut row = 0;
    for op in capture_diff_slices_deadline(Algorithm::Myers, left, right, deadline) {
        let (same, left_at, left_len, right_at, right_len) = match op {
            DiffOp::Equal { old_index, new_index, len } => (true, old_index, len, new_index, len),
            DiffOp::Delete { old_index, old_len, new_index } => (false, old_index, old_len, new_index, 0),
            DiffOp::Insert { old_index, new_index, new_len } => (false, old_index, 0, new_index, new_len),
            DiffOp::Replace { old_index, old_len, new_index, new_len } => (false, old_index, old_len, new_index, new_len),
        };
        let run = Run { row, left: left_at, right: right_at, left_len, right_len, same };
        row += run.rows();
        out.push(run);
    }
    out
}

/// A line's stand-in when the files are too large to diff line by line in memory.
pub fn line_hash(line: &str) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::hash::DefaultHasher::new();
    line.hash(&mut hasher);
    hasher.finish()
}

/// A comparison that's on screen. Lines are read from the two sides as they're drawn.
pub struct Comparison {
    pub left_name: String,
    pub right_name: String,
    left: LineStore,
    right: LineStore,
    runs: Vec<Run>,
    total: usize,
    pub differences: Vec<usize>,
    /// Which difference was last gone to.
    pub current: Option<usize>,
    scroll_to: Option<usize>,
    across: usize,
    longest: usize,
}

impl Comparison {
    /// Compares two sides held in memory, line by line.
    pub fn new(left_name: String, right_name: String, left: LineStore, right: LineStore) -> Self {
        let runs = runs(&left.range_vec(0, left.len()), &right.range_vec(0, right.len()));
        Self::from_runs(left_name, right_name, left, right, runs)
    }

    /// A comparison whose stretches have already been worked out.
    pub fn from_runs(left_name: String, right_name: String, left: LineStore, right: LineStore, runs: Vec<Run>) -> Self {
        let total = runs.last().map_or(0, |run| run.row + run.rows());
        let mut differences = Vec::new();
        let mut inside = false;
        for run in &runs {
            if !run.same && !inside && run.rows() > 0 {
                differences.push(run.row);
            }
            inside = !run.same;
        }
        let longest = left.longest_estimate().max(right.longest_estimate());
        Self { left_name, right_name, left, right, runs, total, differences, current: None, scroll_to: None, across: 0, longest }
    }

    /// How many rows there are.
    pub fn len(&self) -> usize {
        self.total
    }

    /// One row, read from the two sides.
    pub fn row(&self, row: usize) -> Row {
        let run = &self.runs[self.runs.partition_point(|run| run.row <= row).saturating_sub(1)];
        let i = row - run.row;
        let line = |store: &LineStore, at: usize| Some((at, store.get(at)));
        let left = (i < run.left_len).then(|| line(&self.left, run.left + i)).flatten();
        let right = (i < run.right_len).then(|| line(&self.right, run.right + i)).flatten();
        let kind = match (run.same, &left, &right) {
            (true, ..) => RowKind::Same,
            (_, Some(_), Some(_)) => RowKind::Changed,
            (_, Some(_), None) => RowKind::LeftOnly,
            _ => RowKind::RightOnly,
        };
        Row { left, right, kind }
    }

    /// Goes to the next or previous stretch of differences, wrapping round.
    pub fn step(&mut self, forward: bool) {
        let count = self.differences.len();
        if count == 0 {
            return;
        }
        let next = match self.current {
            Some(at) if forward => (at + 1) % count,
            Some(at) => (at + count - 1) % count,
            None if forward => 0,
            None => count - 1,
        };
        self.current = Some(next);
        self.scroll_to = Some(self.differences[next]);
    }

    pub fn summary(&self) -> String {
        match self.differences.len() {
            0 => "No differences in the text".to_string(),
            1 => "1 difference".to_string(),
            count => format!("{count} differences"),
        }
    }
}

fn side(ui: &egui::Ui, area: Rect, line: Option<&(usize, Arc<str>)>, back: Option<Color32>, across: usize, cell: f32, number_width: f32) {
    let painter = ui.painter_at(area);
    match (line, back) {
        (None, _) => {
            painter.rect_filled(area, 0.0, theme::ruler_back());
        }
        (Some(_), Some(color)) => {
            painter.rect_filled(area, 0.0, color);
        }
        _ => {}
    }
    let Some((number, text)) = line else {
        return;
    };
    let font = FontId::monospace(ROW_TEXT);
    painter.text(pos2(area.min.x + number_width - cell, area.min.y + 1.0), Align2::RIGHT_TOP, number + 1, font.clone(), theme::gutter_text());
    let fits = ((area.width() - number_width) / cell).ceil() as usize + 1;
    let shown: String = display_text(text).chars().skip(across).take(fits).collect();
    painter.text(pos2(area.min.x + number_width, area.min.y + 1.0), Align2::LEFT_TOP, shown, font, theme::editor_text());
}

/// Draws the window. Returns true when it's been closed.
pub fn window(ctx: &Context, state: &mut Comparison) -> bool {
    let mut close = ctx.input(|i| i.key_pressed(egui::Key::Escape));
    let screen = ctx.content_rect().size();
    let size = vec2((screen.x * 0.92).max(300.0), (screen.y * 0.86).max(200.0));
    egui::Window::new("Compare").collapsible(false).resizable(false).fixed_size(size).anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0]).show(ctx, |ui| {
        ui.horizontal(|ui| {
            ui.label(state.summary());
            let any = !state.differences.is_empty();
            if ui.add_enabled(any, egui::Button::new("Previous difference")).clicked() {
                state.step(false);
            }
            if ui.add_enabled(any, egui::Button::new("Next difference")).clicked() {
                state.step(true);
            }
            ui.label(egui::RichText::new("Line endings aren't compared.").color(theme::text_faint()));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                close |= ui.button("Close").clicked();
                if state.longest > 40 {
                    ui.add(egui::Slider::new(&mut state.across, 0..=state.longest).show_value(false).text("Across"));
                }
            });
        });
        ui.add_space(2.0);
        let width = ui.available_width();
        let half = ((width - 6.0) / 2.0).floor();
        let (names, _) = ui.allocate_exact_size(vec2(width, 18.0), Sense::hover());
        for (name, x, end) in [(&state.left_name, names.min.x, names.min.x + half), (&state.right_name, names.min.x + half + 6.0, names.max.x)] {
            let area = Rect::from_min_max(pos2(x, names.min.y), pos2(end, names.max.y));
            ui.painter_at(area).text(area.left_center(), Align2::LEFT_CENTER, name, FontId::proportional(12.5), theme::text_dim());
        }
        let font = FontId::monospace(ROW_TEXT);
        let (cell, row_height) = ui.ctx().fonts_mut(|fonts| (fonts.glyph_width(&font, 'M'), fonts.row_height(&font).ceil() + 2.0));
        let digits = state.len().to_string().len().max(3);
        let number_width = (digits + 1) as f32 * cell;
        let mut area = egui::ScrollArea::vertical().id_salt("compare-rows").auto_shrink([false, false]);
        if let Some(row) = state.scroll_to.take() {
            // A little of what comes before, for context. Rows are all the same height, so this lands exactly.
            area = area.vertical_scroll_offset(row.saturating_sub(3) as f32 * row_height);
        }
        let across = state.across;
        // No gaps, so that a row's place is exactly its number times the height.
        ui.spacing_mut().item_spacing.y = 0.0;
        area.show_rows(ui, row_height, state.len(), |ui, range| {
            for row in range.map(|row| state.row(row)) {
                let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), row_height), Sense::hover());
                let split = rect.min.x + ((rect.width() - 6.0) / 2.0).floor();
                let left = Rect::from_min_max(rect.min, pos2(split, rect.max.y));
                let right = Rect::from_min_max(pos2(split + 6.0, rect.min.y), rect.max);
                let (left_back, right_back) = match row.kind {
                    RowKind::Same => (None, None),
                    RowKind::Changed => (Some(theme::compare_changed()), Some(theme::compare_changed())),
                    RowKind::LeftOnly => (Some(theme::compare_removed()), None),
                    RowKind::RightOnly => (None, Some(theme::compare_added())),
                };
                side(ui, left, row.left.as_ref(), left_back, across, cell, number_width);
                side(ui, right, row.right.as_ref(), right_back, across, cell, number_width);
            }
        });
    });
    close
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lines(items: &[&str]) -> Vec<Arc<str>> {
        items.iter().map(|item| Arc::from(*item)).collect()
    }

    fn compared(left: &[Arc<str>], right: &[Arc<str>]) -> Comparison {
        Comparison::new("left".into(), "right".into(), LineStore::from_vec(left.to_vec()), LineStore::from_vec(right.to_vec()))
    }

    fn rows(left: &[Arc<str>], right: &[Arc<str>]) -> Vec<Row> {
        let state = compared(left, right);
        (0..state.len()).map(|row| state.row(row)).collect()
    }

    fn kinds(left: &[&str], right: &[&str]) -> Vec<RowKind> {
        rows(&lines(left), &lines(right)).iter().map(|row| row.kind).collect()
    }

    #[test]
    fn the_same_text_has_no_differences() {
        let found = rows(&lines(&["a", "b"]), &lines(&["a", "b"]));
        assert!(found.iter().all(|row| row.kind == RowKind::Same));
        assert!(compared(&lines(&["a", "b"]), &lines(&["a", "b"])).differences.is_empty());
        assert_eq!(found[1].left.as_ref().unwrap().0, 1);
    }

    #[test]
    fn added_removed_and_changed_lines_are_told_apart() {
        assert_eq!(kinds(&["a", "c"], &["a", "b", "c"]), [RowKind::Same, RowKind::RightOnly, RowKind::Same]);
        assert_eq!(kinds(&["a", "b", "c"], &["a", "c"]), [RowKind::Same, RowKind::LeftOnly, RowKind::Same]);
        assert_eq!(kinds(&["a", "b", "c"], &["a", "x", "c"]), [RowKind::Same, RowKind::Changed, RowKind::Same]);
    }

    #[test]
    fn a_changed_stretch_of_different_lengths_keeps_every_line() {
        let found = rows(&lines(&["1", "2", "3"]), &lines(&["x"]));
        let left: Vec<usize> = found.iter().filter_map(|row| row.left.as_ref().map(|line| line.0)).collect();
        let right: Vec<usize> = found.iter().filter_map(|row| row.right.as_ref().map(|line| line.0)).collect();
        assert_eq!(left, [0, 1, 2]);
        assert_eq!(right, [0]);
        assert_eq!(found[0].kind, RowKind::Changed);
        assert_eq!(found[2].kind, RowKind::LeftOnly);
    }

    #[test]
    fn comparing_hashes_matches_comparing_lines() {
        let (a, b) = (lines(&["a", "b", "c", "d", "e", "f", "b"]), lines(&["x", "b", "d", "y", "z", "f", "g", "b"]));
        let hashed = |lines: &[Arc<str>]| lines.iter().map(|line| line_hash(line)).collect::<Vec<_>>();
        assert_eq!(runs(&hashed(&a), &hashed(&b)), runs(&a, &b));
        assert_ne!(line_hash("a"), line_hash("b"));
    }

    #[test]
    fn empty_sides_work() {
        assert!(rows(&[], &[]).is_empty());
        assert_eq!(kinds(&[], &["a"]), [RowKind::RightOnly]);
        assert_eq!(kinds(&["a"], &[]), [RowKind::LeftOnly]);
    }

    #[test]
    fn nothing_from_either_side_is_ever_lost_or_out_of_order() {
        let (a, b) = (lines(&["a", "b", "c", "d", "e", "f"]), lines(&["x", "b", "d", "y", "z", "f", "g"]));
        let found = rows(&a, &b);
        let left: Vec<Arc<str>> = found.iter().filter_map(|row| row.left.as_ref().map(|line| line.1.clone())).collect();
        let right: Vec<Arc<str>> = found.iter().filter_map(|row| row.right.as_ref().map(|line| line.1.clone())).collect();
        assert_eq!(left, a);
        assert_eq!(right, b);
    }

    #[test]
    fn stepping_goes_through_each_stretch_and_wraps() {
        let (a, b) = (lines(&["a", "x", "y", "b", "c", "z", "d"]), lines(&["a", "b", "c", "d"]));
        let mut state = compared(&a, &b);
        assert_eq!(state.differences, [1, 5]);
        assert_eq!(state.summary(), "2 differences");
        state.step(true);
        assert_eq!(state.scroll_to, Some(1));
        state.step(true);
        assert_eq!(state.scroll_to, Some(5));
        state.step(true);
        assert_eq!(state.scroll_to, Some(1), "it wraps round");
        state.step(false);
        assert_eq!(state.scroll_to, Some(5));
    }

    #[test]
    fn stepping_with_no_differences_does_nothing() {
        let mut state = compared(&lines(&["a"]), &lines(&["a"]));
        state.step(true);
        assert_eq!(state.scroll_to, None);
        assert_eq!(state.summary(), "No differences in the text");
    }

    #[test]
    fn the_window_draws_and_closes_with_escape() {
        let ctx = Context::default();
        let many: Vec<String> = (0..500).map(|i| format!("line {i}\twith a tab")).collect();
        let a: Vec<Arc<str>> = many.iter().map(|line| Arc::from(line.as_str())).collect();
        let mut b = a.clone();
        b[100] = Arc::from("changed");
        b.remove(300);
        let mut state = compared(&a, &b);
        let frame = |events: Vec<egui::Event>, state: &mut Comparison| {
            let raw = egui::RawInput { screen_rect: Some(Rect::from_min_size(pos2(0.0, 0.0), vec2(1200.0, 800.0))), events, ..Default::default() };
            let mut closed = false;
            let mut output = ctx.run_ui(raw, |ui| closed = window(ui.ctx(), state));
            output.textures_delta.clear();
            closed
        };
        assert!(!frame(Vec::new(), &mut state));
        state.step(true);
        assert!(!frame(Vec::new(), &mut state));
        let escape = egui::Event::Key { key: egui::Key::Escape, physical_key: None, pressed: true, repeat: false, modifiers: egui::Modifiers::NONE };
        assert!(frame(vec![escape], &mut state));
    }
}
