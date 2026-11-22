//! The compare tabs, the window's side of comparing: two folders (or Git revisions, images, archives or folders
//! on a server) side by side as a tree, with copying between them; two files side by side, both editable, with
//! their differences marked down to the word; and, for files that aren't text, their bytes or their pictures.
//! Files too large to hold are compared the old way, read-only, as they're drawn.

use super::*;
use crate::compare_sync::{Snapshot, SyncKind, SyncPlan, SyncReport, SyncWay};
use crate::diff_engine::{self as engine, Chunk, ChunkKind, Direction, FolderResult, Job, Mode, Side, Status, TextOptions};
use std::collections::HashSet;
use std::ops::Range;

/// One side of a file comparison, as it's asked for.
pub enum FileSide {
    /// A file on this computer. If it's open, its tab's text is used, unsaved edits and all.
    Path(PathBuf),
    /// A document open here, by its id.
    Open(u64),
    /// A file's bytes from somewhere it can't be saved back to: a Git revision, an image, a server.
    Bytes { name: String, bytes: Vec<u8> },
    /// Lines already read, such as an earlier state of a file. Read-only.
    Lines { name: String, lines: Vec<Arc<str>> },
}

/// What a compare tab shows.
pub(super) enum CompareTab {
    Files(Box<FileCompare>),
    Folders(Box<FolderCompare>),
    Bytes(Box<ByteCompare>),
    Pictures(Box<PictureCompare>),
    /// Files too large to hold, compared line by line as they're drawn, read-only.
    Streamed(Box<compare::Comparison>),
}

impl CompareTab {
    /// Whether a side has edits not yet saved, that only this tab holds.
    pub fn unsaved(&self) -> bool {
        match self {
            CompareTab::Files(files) => files.sources.iter().any(|source| matches!(source, Source2::Own(doc) if doc.is_modified())),
            _ => false,
        }
    }

    /// What the status bar says about it.
    pub fn status_words(&self) -> String {
        match self {
            CompareTab::Files(files) => crate::i18n::tf!("Comparing two files: {}", files.diff.summary().to_lowercase()),
            CompareTab::Folders(folders) => match (&folders.job, &folders.result) {
                (Some(_), _) => crate::i18n::tr("Comparing folders\u{2026}").to_string(),
                (None, Some(result)) => crate::i18n::tf!("Comparing folders: {}", result.counts().describe()),
                (None, None) => crate::i18n::tr("Comparing folders: choose both sides, then Compare").to_string(),
            },
            CompareTab::Bytes(bytes) => bytes.summary(),
            CompareTab::Pictures(pictures) => pictures.summary(),
            CompareTab::Streamed(comparison) => crate::i18n::tf!("Comparing large files, read-only: {}", comparison.summary().to_lowercase()),
        }
    }
}

// ---------- Two files ----------

/// Where a side's text is.
pub(super) enum Source2 {
    /// A document open in its own tab, by id: edits here are edits there.
    Open(u64),
    /// A document only this tab has.
    Own(Box<Document>),
}

/// Two files side by side.
pub(super) struct FileCompare {
    pub sources: [Source2; 2],
    /// Each side's file, or what it is, in full.
    pub names: [String; 2],
    pub diff: DiffView,
}

/// What a file comparison shows of its two sides, and how they differ.
pub(super) struct DiffView {
    states: [ViewState; 2],
    pub options: TextOptions,
    /// Differences within lines are marked by single characters rather than words.
    pub by_chars: bool,
    /// The two sides scroll together.
    pub in_step: bool,
    pub chunks: Vec<Chunk>,
    /// What the differences were worked out from: each side's version, and the options.
    worked_out: Option<(u64, u64, TextOptions, bool)>,
    /// The side with the keyboard.
    pub active: usize,
    tops: [usize; 2],
    /// The difference gone to last, by its place in `chunks`.
    pub current: Option<usize>,
    /// Something to say about the two, such as their line endings differing.
    note: String,
    /// Lines matching this count as the same as each other, from a rule for comparing files.
    pub ignore_lines: Option<regex::Regex>,
}

/// What the file comparison asks the window to do.
enum FileAct {
    Save(usize),
    OpenInTab(usize),
    ExportPatch,
}

/// A side's lines and what each is compared by. Lines matching `ignore` are all alike.
fn keys_of(text: &TextBuffer, options: &TextOptions, ignore: Option<&regex::Regex>) -> (Vec<Arc<str>>, Vec<u64>) {
    let lines = text.all_lines();
    let keys = lines.iter().enumerate().map(|(i, line)| if ignore.is_some_and(|re| re.is_match(line.trim_end_matches('\r'))) { 0 } else { engine::line_key(line, (!options.ignore_line_endings).then(|| text.terminator(i)), options) }).collect();
    (lines, keys)
}

impl DiffView {
    fn new() -> Self {
        Self { states: [ViewState::default(), ViewState::default()], options: TextOptions::default(), by_chars: false, in_step: true, chunks: Vec::new(), worked_out: None, active: 0, tops: [0, 0], current: None, note: String::new(), ignore_lines: None }
    }

    /// Works the differences out again if either side or the options have changed.
    fn refresh(&mut self, docs: [&Document; 2]) {
        let wanted = (docs[0].text.version(), docs[1].text.version(), self.options, self.by_chars);
        if self.worked_out == Some(wanted) {
            return;
        }
        self.worked_out = Some(wanted);
        let (left, left_keys) = keys_of(&docs[0].text, &self.options, self.ignore_lines.as_ref());
        let (right, right_keys) = keys_of(&docs[1].text, &self.options, self.ignore_lines.as_ref());
        self.chunks = engine::diff_keys(&left_keys, &right_keys);
        if self.current.is_some_and(|at| self.chunks.get(at).is_none_or(|chunk| chunk.kind == ChunkKind::Same)) {
            self.current = None;
        }
        let (mut l, mut r) = engine::marks_for(&self.chunks, &left, &right, self.by_chars, &self.options);
        if let Some(chunk) = self.current.map(|at| &self.chunks[at]) {
            l.current = Some(chunk.left.clone());
            r.current = Some(chunk.right.clone());
        }
        self.states[0].diff = Some(Arc::new(l));
        self.states[1].diff = Some(Arc::new(r));
        let (a, b) = (docs[0].text.insert_eol(), docs[1].text.insert_eol());
        self.note = if a != b && !self.options.ignore_line_endings { crate::i18n::tf!("Line endings differ: {} on the left, {} on the right.", a.name(), b.name()) } else { String::new() };
    }

    /// How many differences there are.
    pub fn difference_count(&self) -> usize {
        engine::changes(&self.chunks).count()
    }

    pub fn summary(&self) -> String {
        match self.difference_count() {
            0 => crate::i18n::tr("No differences").to_string(),
            1 => "1 difference".to_string(),
            count => format!("{count} differences"),
        }
    }

    /// Goes to the next or previous difference from where the cursor is on the side in use, wrapping round.
    fn step(&mut self, docs: [&mut Document; 2], forward: bool) {
        let changes: Vec<usize> = engine::changes(&self.chunks).map(|(at, _)| at).collect();
        if changes.is_empty() {
            return;
        }
        let side = self.active;
        let start = |at: usize| if side == 0 { self.chunks[at].left.start } else { self.chunks[at].right.start };
        let caret = docs[side].text.caret_line;
        let next = match self.current.and_then(|current| changes.iter().position(|&at| at == current)) {
            Some(i) if forward => changes[(i + 1) % changes.len()],
            Some(i) => changes[(i + changes.len() - 1) % changes.len()],
            None if forward => changes.iter().copied().find(|&at| start(at) > caret).unwrap_or(changes[0]),
            None => changes.iter().rev().copied().find(|&at| start(at) < caret).unwrap_or(changes[changes.len() - 1]),
        };
        self.go_to(docs, next);
    }

    /// Puts both cursors at the start of a difference, with a little of what comes before it in view.
    fn go_to(&mut self, docs: [&mut Document; 2], at: usize) {
        self.current = Some(at);
        let chunk = self.chunks[at].clone();
        for (n, doc) in docs.into_iter().enumerate() {
            let range = if n == 0 { &chunk.left } else { &chunk.right };
            let line = range.start.min(doc.text.line_count().saturating_sub(1));
            doc.text.move_caret(line, 0, false);
            self.states[n].set_top(range.start.saturating_sub(3));
            self.tops[n] = self.states[n].top();
            if let Some(marks) = &mut self.states[n].diff {
                Arc::make_mut(marks).current = Some(range.clone());
            }
        }
    }

    /// The difference the cursor is in on the side in use, or the one gone to last.
    fn difference_here(&self, docs: [&Document; 2]) -> Option<usize> {
        let caret = docs[self.active].text.caret_line;
        let inside = engine::changes(&self.chunks).find(|(_, chunk)| {
            let range = if self.active == 0 { &chunk.left } else { &chunk.right };
            range.contains(&caret) || (range.is_empty() && range.start == caret)
        });
        inside.map(|(at, _)| at).or(self.current)
    }

    /// Copies a difference's lines over the other side's, as one step that Undo takes back.
    fn copy_change(&mut self, docs: [&mut Document; 2], at: usize, direction: Direction) -> Result<(), String> {
        let chunk = self.chunks.get(at).cloned().ok_or(crate::i18n::tr("There's no difference there."))?;
        let [left, right] = docs;
        let (from, to, from_range, to_range) = if direction.goes_right() { (left, right, chunk.left, chunk.right) } else { (right, left, chunk.right, chunk.left) };
        if to.text.read_only {
            return Err(crate::i18n::tf!("{} is read-only here, so nothing can be copied to it.", to.name()));
        }
        let lines: Vec<Arc<str>> = from_range.map(|line| from.text.line(line)).collect();
        to.text.splice_lines(to_range.start, to_range.end, lines);
        to.text.reveal = true;
        self.current = Some(at);
        Ok(())
    }
}

/// The two documents a file comparison shows, wherever they are.
fn two_docs<'a>(docs: &'a mut [Document], sources: &'a mut [Source2; 2]) -> Option<[&'a mut Document; 2]> {
    let find = |source: &Source2| match source {
        Source2::Open(id) => Some(docs.iter().position(|doc| doc.id == *id)),
        Source2::Own(_) => None,
    };
    let (a, b) = (find(&sources[0]), find(&sources[1]));
    if a == Some(None) || b == Some(None) {
        return None;
    }
    let [left, right] = sources;
    Some(match (a.flatten(), b.flatten(), left, right) {
        (None, None, Source2::Own(x), Source2::Own(y)) => [&mut **x, &mut **y],
        (Some(i), None, _, Source2::Own(y)) => [&mut docs[i], &mut **y],
        (None, Some(j), Source2::Own(x), _) => [&mut **x, &mut docs[j]],
        (Some(i), Some(j), _, _) if i != j => {
            let [x, y] = docs.get_disjoint_mut([i, j]).ok()?;
            [x, y]
        }
        _ => return None,
    })
}

/// Width of the strip between the two sides of a file comparison.
const STRIP: f32 = 44.0;

thread_local! {
    /// Something a comparison drawn this frame has to tell the person, picked up once it's drawn.
    static NOTICE: std::cell::RefCell<Option<String>> = const { std::cell::RefCell::new(None) };
}

fn tell(problem: String) {
    NOTICE.with(|notice| *notice.borrow_mut() = Some(problem));
}

fn draw_file_compare(ui: &mut Ui, fc: &mut FileCompare, docs: &mut [Document], clip: &mut ClipState, view: ViewOptions, input_on: bool) -> Vec<FileAct> {
    let mut acts = Vec::new();
    let own = [matches!(fc.sources[0], Source2::Own(_)), matches!(fc.sources[1], Source2::Own(_))];
    let full_names = fc.names.clone();
    let dv = &mut fc.diff;
    let Some(mut pair) = two_docs(docs, &mut fc.sources) else {
        ui.label(crate::i18n::tr("One of the files has been closed."));
        return acts;
    };
    for doc in pair.iter_mut() {
        doc.update_syntax();
    }
    let syntax = [pair[0].resolved_syntax.flatten(), pair[1].resolved_syntax.flatten()];
    let names = [pair[0].name(), pair[1].name()];
    let modified = [pair[0].is_modified(), pair[1].is_modified()];
    let locked = [pair[0].text.read_only, pair[1].text.read_only];
    let savable = [pair[0].path.is_some() && !locked[0], pair[1].path.is_some() && !locked[1]];
    dv.refresh([&*pair[0], &*pair[1]]);
    let (mut step, mut copy) = (None, None);
    egui::Frame::new().inner_margin(egui::Margin { left: 10, right: 10, top: 6, bottom: 2 }).show(ui, |ui| {
        ui.horizontal_wrapped(|ui| {
            ui.label(RichText::new(dv.summary()).strong());
            let any = dv.difference_count() > 0;
            if ui.add_enabled(any, Button::new(crate::i18n::tr("\u{25B2} Previous"))).on_hover_text(crate::i18n::tr("Go to the difference before this one")).clicked() {
                step = Some(false);
            }
            if ui.add_enabled(any, Button::new(crate::i18n::tr("\u{25BC} Next"))).on_hover_text(crate::i18n::tr("Go to the next difference")).clicked() {
                step = Some(true);
            }
            ui.separator();
            if ui.add_enabled(any && !locked[1], Button::new(crate::i18n::tr("Copy to the right \u{25B6}"))).on_hover_text(crate::i18n::tr("Copy the difference the cursor is in, or the one gone to last, over the right side's")).clicked() {
                copy = Some(Direction::LeftToRight);
            }
            if ui.add_enabled(any && !locked[0], Button::new(crate::i18n::tr("\u{25C0} Copy to the left"))).on_hover_text(crate::i18n::tr("Copy the difference the cursor is in, or the one gone to last, over the left side's")).clicked() {
                copy = Some(Direction::RightToLeft);
            }
            ui.separator();
            ui.checkbox(&mut dv.options.ignore_whitespace, crate::i18n::tr("Ignore spaces"));
            ui.checkbox(&mut dv.options.ignore_case, crate::i18n::tr("Ignore case"));
            ui.checkbox(&mut dv.options.ignore_line_endings, crate::i18n::tr("Ignore line endings"));
            let by_chars = &mut dv.by_chars;
            egui::ComboBox::from_id_salt("compare-within").selected_text(if *by_chars { "Within lines: characters" } else { "Within lines: words" }).show_ui(ui, |ui| {
                ui.selectable_value(by_chars, false, crate::i18n::tr("Words"));
                ui.selectable_value(by_chars, true, crate::i18n::tr("Characters"));
            });
            ui.checkbox(&mut dv.in_step, crate::i18n::tr("Scroll together"));
            if ui.button(crate::i18n::tr("Save as a patch\u{2026}")).on_hover_text(crate::i18n::tr("Save the differences as a unified diff, as patch and git apply read")).clicked() {
                acts.push(FileAct::ExportPatch);
            }
        });
        if !dv.note.is_empty() {
            ui.label(RichText::new(&dv.note).color(theme::text_dim()).size(12.5));
        }
        if let Some(re) = &dv.ignore_lines {
            ui.label(RichText::new(crate::i18n::tf!("Lines matching {} count as the same, by a rule for comparing files.", re.as_str())).color(theme::text_dim()).size(12.5));
        }
        ui.add_space(2.0);
    });
    // Each side's name, over its half.
    let (row, _) = ui.allocate_exact_size(egui::vec2(ui.available_width(), 24.0), egui::Sense::hover());
    let half = ((row.width() - STRIP) / 2.0).floor();
    for n in 0..2 {
        let x = if n == 0 { row.min.x } else { row.min.x + half + STRIP };
        let area = egui::Rect::from_min_max(egui::pos2(x + 8.0, row.min.y), egui::pos2(if n == 0 { row.min.x + half } else { row.max.x }, row.max.y));
        ui.scope_builder(egui::UiBuilder::new().max_rect(area).layout(egui::Layout::left_to_right(egui::Align::Center)), |ui| {
            let mut name = RichText::new(crate::i18n::tf!("{}{}", names[n], if modified[n] { " *" } else { "" })).color(theme::text());
            if n == dv.active {
                name = name.strong();
            }
            ui.label(name).on_hover_text(&full_names[n]);
            if locked[n] {
                ui.label(RichText::new(crate::i18n::tr("read-only")).color(theme::text_faint()).size(12.0));
            }
            if ui.add_enabled(savable[n] && modified[n], Button::new(crate::i18n::tr("Save")).small()).clicked() {
                acts.push(FileAct::Save(n));
            }
            if own[n] && ui.small_button(crate::i18n::tr("Open in a tab")).on_hover_text(crate::i18n::tr("Open this side in a tab of its own too")).clicked() {
                acts.push(FileAct::OpenInTab(n));
            }
        });
    }
    if let Some(forward) = step {
        dv.step(pair, forward);
        return acts;
    }
    if let Some(direction) = copy {
        if let Some(at) = dv.difference_here([&*pair[0], &*pair[1]])
            && let Err(problem) = dv.copy_change(pair, at, direction)
        {
            tell(problem);
        }
        return acts;
    }

    let mut view = view;
    view.word_wrap = false;
    view.document_map = false;
    view.ruler = false;
    let whole = ui.available_rect_before_wrap();
    let half = ((whole.width() - STRIP) / 2.0).floor();
    let left_rect = egui::Rect::from_min_max(whole.min, egui::pos2(whole.min.x + half, whole.max.y));
    let strip = egui::Rect::from_min_max(egui::pos2(left_rect.max.x, whole.min.y), egui::pos2(left_rect.max.x + STRIP, whole.max.y));
    let right_rect = egui::Rect::from_min_max(egui::pos2(strip.max.x, whole.min.y), whole.max);
    if input_on && let Some(pointer) = ui.input(|i| i.pointer.primary_pressed().then(|| i.pointer.interact_pos()).flatten()) {
        if left_rect.contains(pointer) {
            dv.active = 0;
        } else if right_rect.contains(pointer) {
            dv.active = 1;
        }
    }
    let before = [dv.states[0].top(), dv.states[1].top()];
    for (n, rect) in [left_rect, right_rect].into_iter().enumerate() {
        let doc = &mut *pair[n];
        let state = &mut dv.states[n];
        state.colours.set_syntax(syntax[n]);
        let active = dv.active == n;
        // The right-click menu's copy and paste are taken by the keys themselves here.
        let _ = ui.scope_builder(egui::UiBuilder::new().max_rect(rect), |ui| views::text_pane(ui, &mut doc.text, state, clip, view, input_on, Pane { number: n, active, filter: None, folded: false }));
    }
    // Keeping the two in step: whichever side moved, the other follows to the line that goes with its top.
    let after = [dv.states[0].top(), dv.states[1].top()];
    if dv.in_step {
        let moved = (0..2).find(|&n| after[n] != dv.tops[n] && after[n] != before[n]).or_else(|| (0..2).find(|&n| after[n] != dv.tops[n]));
        if let Some(n) = moved {
            let other = 1 - n;
            let line = engine::matching_line(&dv.chunks, after[n], n == 0).min(pair[other].text.line_count().saturating_sub(1));
            dv.states[other].set_top(line);
            ui.ctx().request_repaint();
        }
    }
    dv.tops = [dv.states[0].top(), dv.states[1].top()];
    // The strip between: each difference joined across, with buttons to copy it either way.
    let font = egui::FontId::monospace(view.font_size);
    let cell = views::cell_size(ui, &font);
    let painter = ui.painter_at(strip);
    painter.rect_filled(strip, 0.0, theme::gutter_back());
    let tops = dv.tops;
    let y_of = |n: usize, line: usize| strip.min.y + (line as f32 - tops[n] as f32) * cell.y;
    let mut copy_at = None;
    for (at, chunk) in engine::changes(&dv.chunks) {
        let (l0, l1, r0, r1) = (y_of(0, chunk.left.start), y_of(0, chunk.left.end), y_of(1, chunk.right.start), y_of(1, chunk.right.end));
        if l1.max(r1) < strip.min.y || l0.min(r0) > strip.max.y {
            continue;
        }
        let colour = views::diff_colour(chunk.kind);
        let points = vec![egui::pos2(strip.min.x, l0), egui::pos2(strip.max.x, r0), egui::pos2(strip.max.x, r1.max(r0 + 1.0)), egui::pos2(strip.min.x, l1.max(l0 + 1.0))];
        painter.add(egui::Shape::convex_polygon(points, colour, egui::Stroke::new(1.0, colour.lerp_to_gamma(theme::editor_text(), 0.3))));
        let middle_left = ((l0 + l1.max(l0 + cell.y)) / 2.0).clamp(strip.min.y + 8.0, strip.max.y - 8.0);
        let middle_right = ((r0 + r1.max(r0 + cell.y)) / 2.0).clamp(strip.min.y + 8.0, strip.max.y - 8.0);
        let button = |ui: &mut Ui, centre: egui::Pos2, text: &str, tip: &str| ui.put(egui::Rect::from_center_size(centre, egui::vec2(16.0, 16.0)), Button::new(RichText::new(text).size(10.0)).small()).on_hover_text(crate::i18n::tr(tip)).clicked();
        if input_on && !locked[1] && button(ui, egui::pos2(strip.min.x + 9.0, middle_left), "\u{25B6}", "Copy this difference to the right") {
            copy_at = Some((at, Direction::LeftToRight));
        }
        if input_on && !locked[0] && button(ui, egui::pos2(strip.max.x - 9.0, middle_right), "\u{25C0}", "Copy this difference to the left") {
            copy_at = Some((at, Direction::RightToLeft));
        }
    }
    if let Some((at, direction)) = copy_at
        && let Err(problem) = dv.copy_change(pair, at, direction)
    {
        tell(problem);
    }
    acts
}

// ---------- Bytes ----------

/// Two files that aren't text, compared byte by byte at the same offsets.
pub(super) struct ByteCompare {
    pub names: [String; 2],
    pub bytes: [Vec<u8>; 2],
    pub differences: Vec<Range<usize>>,
    pub current: Option<usize>,
    scroll_to: Option<usize>,
}

const BYTES_PER_ROW: usize = 16;

impl ByteCompare {
    pub fn new(names: [String; 2], bytes: [Vec<u8>; 2]) -> Self {
        let differences = engine::byte_differences(&bytes[0], &bytes[1]);
        Self { names, bytes, differences, current: None, scroll_to: None }
    }

    pub fn summary(&self) -> String {
        let differing: usize = self.differences.iter().map(|range| range.len()).sum();
        let sizes = if self.bytes[0].len() == self.bytes[1].len() { crate::i18n::tf!("Both are {} bytes", self.bytes[0].len()) } else { crate::i18n::tf!("{} bytes on the left, {} on the right", self.bytes[0].len(), self.bytes[1].len()) };
        match self.differences.len() {
            0 => crate::i18n::tf!("{sizes}, and they're the same"),
            places => {
                let bytes = if differing == 1 { crate::i18n::tr("1 byte differs").to_string() } else { crate::i18n::tf!("{differing} bytes differ") };
                if places == 1 { crate::i18n::tf!("{sizes}. {bytes}, in 1 place") } else { crate::i18n::tf!("{sizes}. {bytes}, in {places} places") }
            },
        }
    }

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
        self.scroll_to = Some(self.differences[next].start / BYTES_PER_ROW);
    }

    fn differs(&self, offset: usize) -> bool {
        let at = self.differences.partition_point(|range| range.end <= offset);
        self.differences.get(at).is_some_and(|range| range.contains(&offset))
    }
}

fn draw_byte_compare(ui: &mut Ui, bc: &mut ByteCompare) {
    egui::Frame::new().inner_margin(egui::Margin::symmetric(10, 6)).show(ui, |ui| {
        ui.horizontal(|ui| {
            ui.label(RichText::new(bc.summary()).strong());
            let any = !bc.differences.is_empty();
            if ui.add_enabled(any, Button::new(crate::i18n::tr("\u{25B2} Previous"))).clicked() {
                bc.step(false);
            }
            if ui.add_enabled(any, Button::new(crate::i18n::tr("\u{25BC} Next"))).clicked() {
                bc.step(true);
            }
            ui.label(RichText::new(crate::i18n::tr("These aren't text, so they're compared byte by byte, offset for offset. Differences are marked \u{2260}.")).color(theme::text_faint()).size(12.5));
        });
        ui.label(RichText::new(crate::i18n::tf!("Left: {}    Right: {}", bc.names[0], bc.names[1])).color(theme::text_dim()).size(12.5));
        let font = egui::FontId::monospace(13.0);
        let cell = views::cell_size(ui, &font);
        let rows = bc.bytes[0].len().max(bc.bytes[1].len()).div_ceil(BYTES_PER_ROW);
        let mut area = egui::ScrollArea::vertical().id_salt("byte-compare").auto_shrink([false, false]);
        if let Some(row) = bc.scroll_to.take() {
            area = area.vertical_scroll_offset(row.saturating_sub(3) as f32 * cell.y);
        }
        ui.spacing_mut().item_spacing.y = 0.0;
        area.show_rows(ui, cell.y, rows, |ui, range| {
            for row in range {
                let (rect, _) = ui.allocate_exact_size(egui::vec2(ui.available_width(), cell.y), egui::Sense::hover());
                let painter = ui.painter_at(rect);
                let start = row * BYTES_PER_ROW;
                let any = (start..start + BYTES_PER_ROW).any(|offset| bc.differs(offset));
                let mut x = rect.min.x;
                let text = |painter: &egui::Painter, x: f32, words: &str, colour: egui::Color32| {
                    painter.text(egui::pos2(x, rect.min.y + 1.0), egui::Align2::LEFT_TOP, words, font.clone(), colour);
                };
                text(&painter, x, if any { "\u{2260}" } else { " " }, theme::editor_text());
                x += cell.x * 2.0;
                text(&painter, x, &format!("{start:08X}"), theme::gutter_text());
                x += cell.x * 10.0;
                for side in 0..2 {
                    let bytes = &bc.bytes[side];
                    for i in 0..BYTES_PER_ROW {
                        let offset = start + i;
                        let at = egui::pos2(x + i as f32 * cell.x * 3.0, rect.min.y);
                        let character_at = x + (BYTES_PER_ROW * 3 + 1) as f32 * cell.x + i as f32 * cell.x;
                        let Some(&byte) = bytes.get(offset) else {
                            continue;
                        };
                        if bc.differs(offset) {
                            let back = if side == 0 { theme::compare_removed() } else { theme::compare_added() };
                            painter.rect_filled(egui::Rect::from_min_size(at, egui::vec2(cell.x * 2.0, cell.y)), 0.0, back);
                            painter.rect_filled(egui::Rect::from_min_size(egui::pos2(character_at, rect.min.y), egui::vec2(cell.x, cell.y)), 0.0, back);
                            painter.hline(at.x..=at.x + cell.x * 2.0, rect.max.y - 1.0, egui::Stroke::new(1.0, theme::editor_text()));
                        }
                        text(&painter, at.x, &format!("{byte:02X}"), theme::editor_text());
                        let shown = if (0x20..0x7F).contains(&byte) { char::from(byte) } else { '\u{B7}' };
                        text(&painter, character_at, &shown.to_string(), theme::editor_text());
                    }
                    x += (BYTES_PER_ROW * 4 + 3) as f32 * cell.x;
                }
            }
        });
    });
}

// ---------- Pictures ----------

/// Two pictures side by side, with where they differ.
pub(super) struct PictureCompare {
    pub names: [String; 2],
    pub pictures: [crate::pictures::Picture; 2],
    /// Where they differ: its width, height and pixels, and how many differ.
    pub difference: (usize, usize, Vec<u8>, usize),
    textures: Option<[egui::TextureHandle; 3]>,
    pub show_difference: bool,
}

impl PictureCompare {
    pub fn new(names: [String; 2], pictures: [crate::pictures::Picture; 2]) -> Self {
        let [a, b] = &pictures;
        let difference = engine::pixel_difference(a.width, a.height, &a.rgba, b.width, b.height, &b.rgba);
        Self { names, pictures, difference, textures: None, show_difference: true }
    }

    pub fn summary(&self) -> String {
        let [a, b] = &self.pictures;
        let sizes = if (a.width, a.height) == (b.width, b.height) { format!("Both {} \u{D7} {}", a.width, a.height) } else { crate::i18n::tf!("{} \u{D7} {} on the left, {} \u{D7} {} on the right", a.width, a.height, b.width, b.height) };
        let total = (self.difference.0 * self.difference.1).max(1);
        match self.difference.3 {
            0 => crate::i18n::tf!("{sizes}, and every pixel is the same"),
            n => format!("{sizes}. {n} pixel{} differ{} ({:.1}%)", if n == 1 { "" } else { "s" }, if n == 1 { "s" } else { "" }, n as f64 * 100.0 / total as f64),
        }
    }
}

fn draw_picture_compare(ui: &mut Ui, pc: &mut PictureCompare) {
    let options = egui::TextureOptions { magnification: egui::TextureFilter::Nearest, minification: egui::TextureFilter::Linear, ..egui::TextureOptions::default() };
    let textures = pc.textures.get_or_insert_with(|| {
        let load = |name: &str, w: usize, h: usize, rgba: &[u8]| ui.ctx().load_texture(name, egui::ColorImage::from_rgba_unmultiplied([w, h], rgba), options);
        let [a, b] = &pc.pictures;
        [load("compare-left", a.width, a.height, &a.rgba), load("compare-right", b.width, b.height, &b.rgba), load("compare-difference", pc.difference.0, pc.difference.1, &pc.difference.2)]
    }).clone();
    egui::Frame::new().inner_margin(egui::Margin::symmetric(10, 6)).show(ui, |ui| {
        ui.horizontal(|ui| {
            ui.label(RichText::new(pc.summary()).strong());
            ui.checkbox(&mut pc.show_difference, crate::i18n::tr("Show where they differ")).on_hover_text(crate::i18n::tr("A third picture: the left one in grey, with every pixel that differs in magenta"));
        });
        let count = if pc.show_difference { 3 } else { 2 };
        let room = egui::vec2((ui.available_width() - 12.0 * (count - 1) as f32) / count as f32, ui.available_height() - 24.0);
        ui.horizontal_top(|ui| {
            for (n, texture) in textures.iter().enumerate().take(count) {
                ui.vertical(|ui| {
                    ui.set_width(room.x);
                    let (label, w, h, aspect) = match n {
                        0 => (pc.names[0].clone(), pc.pictures[0].width, pc.pictures[0].height, pc.pictures[0].aspect),
                        1 => (pc.names[1].clone(), pc.pictures[1].width, pc.pictures[1].height, pc.pictures[1].aspect),
                        _ => (crate::i18n::tr("Where they differ").to_string(), pc.difference.0, pc.difference.1, pc.pictures[0].aspect),
                    };
                    ui.label(RichText::new(label).color(theme::text_dim()).size(12.5));
                    let shown = egui::vec2(w as f32 * aspect, h as f32).max(egui::vec2(1.0, 1.0));
                    let fit = (room.x / shown.x).min(room.y / shown.y);
                    let scale = if fit >= 1.0 { fit.floor() } else { fit };
                    ui.add(egui::Image::new(texture).fit_to_exact_size(shown * scale));
                });
            }
        });
    });
}

// ---------- Folders ----------

/// What kind of place a side of a folder comparison is.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum SideKind {
    #[default]
    Folder,
    Git,
    Image,
    Server,
}

impl SideKind {
    fn words(self) -> &'static str {
        match self {
            SideKind::Folder => "Folder",
            SideKind::Git => "Git revision",
            SideKind::Image => "Disk image or archive",
            SideKind::Server => "Folder on a server",
        }
    }
}

/// A side of a folder comparison as it's being chosen.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SidePicker {
    pub kind: SideKind,
    /// The folder, the image or archive, the folder in a repository, or sftp://user@host/folder.
    pub path: String,
    /// For a Git revision: which one.
    pub revision: String,
    /// For a server chosen from the saved sites: the key file it signs in with.
    pub key_file: Option<PathBuf>,
}

impl SidePicker {
    pub fn of(side: &Side) -> Self {
        match side {
            Side::Folder(path) => Self { kind: SideKind::Folder, path: path.display().to_string(), revision: "HEAD".into(), key_file: None },
            Side::Git { repo, revision, folder } => Self { kind: SideKind::Git, path: if folder.is_empty() { repo.display().to_string() } else { repo.join(folder).display().to_string() }, revision: revision.clone(), key_file: None },
            Side::Image { file, folder } => Self { kind: SideKind::Image, path: if folder.is_empty() { file.display().to_string() } else { format!("{}/{folder}", file.display()) }, revision: "HEAD".into(), key_file: None },
            Side::Sftp { site, folder } => Self { kind: SideKind::Server, path: site.url(folder), revision: "HEAD".into(), key_file: site.key_file.clone() },
        }
    }

    /// The side it describes, or what's wrong with it.
    pub fn side(&self) -> Result<Side, String> {
        let path = self.path.trim();
        if path.is_empty() {
            return Err(crate::i18n::tr("Choose what to compare on both sides.").into());
        }
        match self.kind {
            SideKind::Folder => Ok(Side::Folder(PathBuf::from(path))),
            SideKind::Git => Side::git_of(Path::new(path), self.revision.trim()),
            SideKind::Image => Ok(Side::Image { file: PathBuf::from(path), folder: String::new() }),
            SideKind::Server => {
                let (mut site, folder) = crate::remote::parse_url(path)?;
                site.key_file = self.key_file.clone();
                Ok(Side::Sftp { site, folder })
            }
        }
    }
}

/// A copy waiting to be confirmed, or under way.
struct CopyPreview {
    plan: engine::CopyPlan,
    backups: PathBuf,
}

/// Two folders side by side.
pub(super) struct FolderCompare {
    pub pickers: [SidePicker; 2],
    pub mode: Mode,
    pub filters: engine::Filters,
    pub hide_same: bool,
    pub job: Option<Job<Result<FolderResult, String>>>,
    pub result: Option<FolderResult>,
    pub problem: Option<String>,
    /// Folders opened out, by path.
    pub expanded: HashSet<String>,
    /// Rows chosen, by path, so the choice outlasts comparing again.
    pub selected: HashSet<String>,
    anchor: Option<String>,
    copy: Option<CopyPreview>,
    pub copy_job: Option<Job<engine::CopyReport>>,
    /// What the last copy or save did, to show above the list.
    pub notice: Option<String>,
    patch_job: Option<(PathBuf, Job<Result<String, String>>)>,
    /// The first comparison opens out every folder that differs.
    opened_once: bool,
    pub sync_kind: SyncKind,
    pub sync_way: SyncWay,
    /// A sync being planned, waiting to be agreed to, or under way.
    sync_plan_job: Option<Job<Result<(SyncPlan, PathBuf), String>>>,
    sync: Option<sync_host::SyncPreview>,
    pub sync_job: Option<Job<SyncReport>>,
    /// How both sides were after the last sync, for telling conflicts.
    pub snapshot: Option<Snapshot>,
    snapshot_job: Option<Job<Snapshot>>,
    /// A sync has been done: once it's compared again, how both sides are is remembered.
    snapshot_wanted: bool,
    /// The saved comparison this is, by name, and whether it's kept in the project.
    pub session: Option<(String, bool)>,
    /// What's remembered has changed since the saved comparison was last stored.
    pub session_dirty: bool,
}

/// What the folder comparison asks the window to do.
enum FolderAct {
    Start,
    OpenPair(usize),
    OpenOne(usize, bool),
    Browse(usize),
    ExportReport(ReportKind),
    ExportPatch,
    /// Plan a sync, to show before it's done.
    Sync,
    SaveSession,
    Rules,
    /// Apply a patch to the left side, or the right.
    ApplyPatch(bool),
}

/// Which report to save.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum ReportKind {
    /// An HTML page or plain text, by the name's ending.
    Full,
    Summary,
    Csv,
}

impl FolderCompare {
    pub fn new(left: SidePicker, right: SidePicker) -> Self {
        Self {
            pickers: [left, right],
            mode: Mode::Quick,
            filters: engine::Filters::default(),
            hide_same: false,
            job: None,
            result: None,
            problem: None,
            expanded: HashSet::new(),
            selected: HashSet::new(),
            anchor: None,
            copy: None,
            copy_job: None,
            notice: None,
            patch_job: None,
            opened_once: false,
            sync_kind: SyncKind::Update,
            sync_way: SyncWay::LeftToRight,
            sync_plan_job: None,
            sync: None,
            sync_job: None,
            snapshot: None,
            snapshot_job: None,
            snapshot_wanted: false,
            session: None,
            session_dirty: false,
        }
    }

    /// Whether something's under way that the comparison has to wait for.
    pub fn busy(&self) -> bool {
        self.job.is_some() || self.copy_job.is_some() || self.patch_job.is_some() || self.sync_plan_job.is_some() || self.sync_job.is_some() || self.snapshot_job.is_some()
    }

    /// Starts comparing, on another thread.
    pub fn start(&mut self, known: crate::remote::KnownHosts, waker: Option<Context>) {
        let sides = self.pickers[0].side().and_then(|left| Ok((left, self.pickers[1].side()?)));
        let (left, right) = match sides {
            Ok(sides) => sides,
            Err(problem) => {
                self.problem = Some(problem);
                return;
            }
        };
        self.problem = None;
        let (mode, filters) = (self.mode, self.filters.clone());
        self.job = Some(Job::start(
            move |progress, cancel| engine::compare_folders(&left, &right, mode, &filters, &known, progress, cancel),
            move || {
                if let Some(ctx) = waker {
                    ctx.request_repaint();
                }
            },
        ));
    }

    /// Picks up a comparison that's finished.
    pub fn poll(&mut self) -> bool {
        let Some(outcome) = self.job.as_ref().and_then(Job::take) else {
            return false;
        };
        self.job = None;
        match outcome {
            Ok(result) => {
                if !self.opened_once {
                    self.opened_once = true;
                    self.expanded = result.rows.iter().filter(|row| row.is_dir() && row.status != Status::Same).map(|row| row.path.clone()).collect();
                }
                let paths: HashSet<&str> = result.rows.iter().map(|row| row.path.as_str()).collect();
                self.selected.retain(|path| paths.contains(path.as_str()));
                self.result = Some(result);
            }
            Err(problem) => self.problem = Some(problem),
        }
        true
    }

    /// The rows chosen, by place in the result.
    pub fn selected_rows(&self) -> Vec<usize> {
        self.result.as_ref().map_or_else(Vec::new, |result| (0..result.rows.len()).filter(|&i| self.selected.contains(&result.rows[i].path)).collect())
    }

    /// Plans a copy of what's chosen, to show before it's done.
    pub fn plan_copy(&mut self, direction: Direction, backups_under: &Path) -> Result<(), String> {
        let result = self.result.as_ref().ok_or(crate::i18n::tr("Compare the folders first."))?;
        let selected = self.selected_rows();
        if selected.is_empty() {
            return Err(crate::i18n::tr("Choose what to copy first.").into());
        }
        let plan = engine::plan_copy(result, &selected, direction)?;
        if plan.is_empty() && plan.skipped.is_empty() {
            return Err(crate::i18n::tr("Everything chosen is already the same on the other side.").into());
        }
        self.copy = Some(CopyPreview { plan, backups: engine::backup_folder(backups_under) });
        Ok(())
    }

    /// Goes ahead with the copy that was planned, on another thread.
    pub fn confirm_copy(&mut self, waker: Option<Context>) {
        let (Some(preview), Some(result)) = (self.copy.take(), self.result.as_ref()) else {
            return;
        };
        let from_left = preview.plan.direction.goes_right();
        let (source, target) = (result.tree(from_left).clone(), result.tree(!from_left).clone());
        self.copy_job = Some(Job::start(
            move |progress, cancel| engine::run_copy(&preview.plan, &*source, &*target, &preview.backups, progress, cancel),
            move || {
                if let Some(ctx) = waker {
                    ctx.request_repaint();
                }
            },
        ));
    }

    fn toggle(&mut self, path: &str) {
        if !self.expanded.remove(path) {
            self.expanded.insert(path.to_string());
        }
    }
}

/// The colour behind a row's status, by how it compares.
fn status_colour(status: Status) -> egui::Color32 {
    match status {
        Status::Same => theme::editor_back(),
        Status::Different | Status::NewerLeft | Status::NewerRight => theme::compare_changed(),
        Status::LeftOnly => theme::compare_removed(),
        Status::RightOnly => theme::compare_added(),
        Status::TypeDiffers | Status::Unreadable => theme::compare_changed().lerp_to_gamma(theme::compare_removed(), 0.5),
    }
}

fn short_time(time: Option<u64>) -> String {
    time.and_then(|secs| chrono::DateTime::from_timestamp(secs as i64, 0)).map_or_else(String::new, |when| when.with_timezone(&chrono::Local).format("%Y-%m-%d %H:%M").to_string())
}

fn draw_folder_compare(ui: &mut Ui, fc: &mut FolderCompare, input_on: bool, sites: &[remote_host::SavedSite]) -> Vec<FolderAct> {
    let mut acts = Vec::new();
    let busy = fc.busy();
    let field_room = (ui.available_width() - 470.0).max(160.0);
    egui::Frame::new().inner_margin(egui::Margin { left: 10, right: 10, top: 6, bottom: 4 }).show(ui, |ui| {
        egui::Grid::new("folder-compare-sides").num_columns(4).spacing([6.0, 4.0]).show(ui, |ui| {
            for (n, title) in ["Left", "Right"].into_iter().enumerate() {
                let picker = &mut fc.pickers[n];
                ui.label(crate::i18n::tr(title));
                egui::ComboBox::from_id_salt(("compare-kind", n)).width(170.0).selected_text(crate::i18n::tr(picker.kind.words())).show_ui(ui, |ui| {
                    for kind in [SideKind::Folder, SideKind::Git, SideKind::Image, SideKind::Server] {
                        ui.selectable_value(&mut picker.kind, kind, crate::i18n::tr(kind.words()));
                    }
                });
                ui.horizontal(|ui| {
                    let hint = match picker.kind {
                        SideKind::Server => "sftp://user@host/folder",
                        SideKind::Git => "A folder in the repository",
                        SideKind::Image => "The image or archive",
                        SideKind::Folder => "The folder",
                    };
                    let room = (field_room - if picker.kind == SideKind::Git { 120.0 } else { 0.0 }).max(120.0);
                    let edit = ui.add(egui::TextEdit::singleline(&mut picker.path).hint_text(hint).desired_width(room));
                    if edit.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter)) {
                        acts.push(FolderAct::Start);
                    }
                    if picker.kind != SideKind::Server && ui.button(crate::i18n::tr("Browse\u{2026}")).clicked() {
                        acts.push(FolderAct::Browse(n));
                    }
                    if picker.kind == SideKind::Server {
                        egui::ComboBox::from_id_salt(("compare-site", n)).width(110.0).selected_text(crate::i18n::tr("Saved sites")).show_ui(ui, |ui| {
                            if sites.is_empty() {
                                ui.label(crate::i18n::tr("None yet. Sites are kept when you connect with File, Open a remote file."));
                            }
                            for saved in sites {
                                if ui.selectable_label(false, &saved.name).on_hover_text(saved.site.url("~/")).clicked() {
                                    picker.path = saved.site.url("/~/");
                                    picker.key_file = saved.site.key_file.clone();
                                }
                            }
                        });
                    }
                    if picker.kind == SideKind::Git {
                        ui.label(crate::i18n::tr("at"));
                        ui.add(egui::TextEdit::singleline(&mut picker.revision).hint_text(crate::i18n::tr("HEAD")).desired_width(90.0)).on_hover_text(crate::i18n::tr("HEAD for the last commit, or a branch, a tag or a commit"));
                    }
                });
                if n == 0 {
                    if ui.button(crate::i18n::tr("\u{21C5} Swap")).on_hover_text(crate::i18n::tr("Swap the two sides")).clicked() {
                        fc.pickers.swap(0, 1);
                    }
                } else {
                    ui.label(crate::i18n::tr(""));
                }
                ui.end_row();
            }
        });
        ui.horizontal_wrapped(|ui| {
            ui.label(crate::i18n::tr("Compare by"));
            ui.radio_value(&mut fc.mode, Mode::Quick, crate::i18n::tr("size and time")).on_hover_text(crate::i18n::tr("Quick. Where a side keeps no times, as a Git revision doesn't, files of the same size are compared by contents"));
            ui.radio_value(&mut fc.mode, Mode::Thorough, crate::i18n::tr("contents")).on_hover_text(crate::i18n::tr("Every file on both sides is read"));
            ui.separator();
            ui.label(crate::i18n::tr("Only"));
            ui.add(egui::TextEdit::singleline(&mut fc.filters.include).hint_text(crate::i18n::tr("*.rs; *.toml")).desired_width(110.0)).on_hover_text(crate::i18n::tr("Only files matching these, separated by ; (* is any characters, ** any folders)"));
            ui.label(crate::i18n::tr("Leave out"));
            ui.add(egui::TextEdit::singleline(&mut fc.filters.exclude).hint_text(crate::i18n::tr("target; *.log")).desired_width(110.0)).on_hover_text(crate::i18n::tr("Files and folders matching these are left out"));
            ui.checkbox(&mut fc.filters.gitignore, crate::i18n::tr("Leave out what .gitignore does"));
            ui.separator();
            if ui.add_enabled(!busy, Button::new(if fc.result.is_some() { crate::i18n::tr("\u{27F3} Compare again") } else { crate::i18n::tr("Compare") })).clicked() {
                acts.push(FolderAct::Start);
            }
            if let Some(job) = &fc.job
                && ui.button(crate::i18n::tr("Stop")).clicked()
            {
                job.cancel();
            }
        });
        let has_result = fc.result.is_some();
        let chosen = !fc.selected.is_empty();
        ui.horizontal_wrapped(|ui| {
            ui.checkbox(&mut fc.hide_same, crate::i18n::tr("Hide what's the same"));
            if ui.add_enabled(has_result, Button::new(crate::i18n::tr("Open all"))).clicked()
                && let Some(result) = &fc.result
            {
                fc.expanded = result.rows.iter().filter(|row| row.is_dir()).map(|row| row.path.clone()).collect();
            }
            if ui.add_enabled(has_result, Button::new(crate::i18n::tr("Close all"))).clicked() {
                fc.expanded.clear();
            }
            ui.separator();
            if ui.add_enabled(has_result && chosen && !busy, Button::new(crate::i18n::tr("Copy to the right \u{25B6}\u{2026}"))).on_hover_text(crate::i18n::tr("Copy what's chosen from the left over to the right. You'll see what will happen first")).clicked() {
                acts.push(FolderAct::OpenOne(usize::MAX, true));
            }
            if ui.add_enabled(has_result && chosen && !busy, Button::new(crate::i18n::tr("\u{25C0} Copy to the left\u{2026}"))).on_hover_text(crate::i18n::tr("Copy what's chosen from the right over to the left. You'll see what will happen first")).clicked() {
                acts.push(FolderAct::OpenOne(usize::MAX, false));
            }
            ui.separator();
            ui.add_enabled_ui(has_result, |ui| {
                crate::fit::menu_button(ui, crate::i18n::tr("Save a report \u{25BE}"), |ui| {
                    if ui.button(crate::i18n::tr("Every entry\u{2026}")).on_hover_text(crate::i18n::tr("An HTML page or plain text, by the ending you give the name")).clicked() {
                        acts.push(FolderAct::ExportReport(ReportKind::Full));
                        ui.close();
                    }
                    if ui.button(crate::i18n::tr("The counts only\u{2026}")).on_hover_text(crate::i18n::tr("How many are the same and how many differ, for the whole and each folder at the top")).clicked() {
                        acts.push(FolderAct::ExportReport(ReportKind::Summary));
                        ui.close();
                    }
                    if ui.button(crate::i18n::tr("As a CSV file\u{2026}")).on_hover_text(crate::i18n::tr("A row for each entry, for a spreadsheet")).clicked() {
                        acts.push(FolderAct::ExportReport(ReportKind::Csv));
                        ui.close();
                    }
                });
            });
            if ui.add_enabled(has_result && fc.patch_job.is_none(), Button::new(crate::i18n::tr("Save as a patch\u{2026}"))).on_hover_text(crate::i18n::tr("Save every difference in the text files as one unified diff")).clicked() {
                acts.push(FolderAct::ExportPatch);
            }
            ui.add_enabled_ui(!busy, |ui| {
                crate::fit::menu_button(ui, crate::i18n::tr("Apply a patch \u{25BE}"), |ui| {
                    if ui.button(crate::i18n::tr("To the left\u{2026}")).clicked() {
                        acts.push(FolderAct::ApplyPatch(true));
                        ui.close();
                    }
                    if ui.button(crate::i18n::tr("To the right\u{2026}")).clicked() {
                        acts.push(FolderAct::ApplyPatch(false));
                        ui.close();
                    }
                });
            });
        });
        ui.horizontal_wrapped(|ui| {
            ui.label(crate::i18n::tr("Sync"));
            egui::ComboBox::from_id_salt("sync-kind").width(90.0).selected_text(sync_host::kind_words(fc.sync_kind)).show_ui(ui, |ui| {
                ui.selectable_value(&mut fc.sync_kind, SyncKind::Update, crate::i18n::tr("Update")).on_hover_text(crate::i18n::tr("Copy newer files and files that are missing. Nothing is deleted"));
                ui.selectable_value(&mut fc.sync_kind, SyncKind::Mirror, crate::i18n::tr("Mirror")).on_hover_text(crate::i18n::tr("Make one side exactly like the other, which deletes what the other hasn't got"));
            });
            if fc.sync_kind == SyncKind::Mirror && fc.sync_way == SyncWay::Both {
                fc.sync_way = SyncWay::LeftToRight;
            }
            egui::ComboBox::from_id_salt("sync-way").width(170.0).selected_text(sync_host::way_words(fc.sync_kind, fc.sync_way)).show_ui(ui, |ui| {
                for way in [SyncWay::LeftToRight, SyncWay::RightToLeft, SyncWay::Both] {
                    if way == SyncWay::Both && fc.sync_kind == SyncKind::Mirror {
                        continue;
                    }
                    ui.selectable_value(&mut fc.sync_way, way, sync_host::way_words(fc.sync_kind, way));
                }
            });
            if ui.add_enabled(has_result && !busy, Button::new(crate::i18n::tr("Sync\u{2026}"))).on_hover_text(crate::i18n::tr("See everything it would do first, and untick what you don't want")).clicked() {
                acts.push(FolderAct::Sync);
            }
            ui.separator();
            if ui.button(crate::i18n::tr("Rules\u{2026}")).on_hover_text(crate::i18n::tr("How files are compared by name: as text leaving things out, as bytes, as pictures, by size and time only, or left out")).clicked() {
                acts.push(FolderAct::Rules);
            }
            ui.separator();
            let save = match &fc.session {
                Some((name, _)) => format!("Save \u{201C}{name}\u{201D}"),
                None => crate::i18n::tr("Save this comparison\u{2026}").to_string(),
            };
            if ui.button(save).on_hover_text(crate::i18n::tr("Keep both sides, how they're compared, the filters, rules and sync, to open again by name")).clicked() {
                acts.push(FolderAct::SaveSession);
            }
            if fc.snapshot.is_some() {
                ui.label(RichText::new(crate::i18n::tr("Knows the last sync")).color(theme::text_dim()).size(12.0)).on_hover_text(crate::i18n::tr("Files changed on both sides since then are shown as conflicts, and left alone unless you tick them"));
            }
        });
        if let Some(problem) = &fc.problem {
            ui.label(RichText::new(problem).color(theme::mistake()));
        }
        if let Some(notice) = &fc.notice {
            ui.label(RichText::new(notice).color(theme::text_dim()).size(12.5));
        }
        for progress in [
            fc.job.as_ref().map(|job| &job.progress),
            fc.copy_job.as_ref().map(|job| &job.progress),
            fc.patch_job.as_ref().map(|(_, job)| &job.progress),
            fc.sync_plan_job.as_ref().map(|job| &job.progress),
            fc.sync_job.as_ref().map(|job| &job.progress),
            fc.snapshot_job.as_ref().map(|job| &job.progress),
        ]
        .into_iter()
        .flatten()
        {
            let phase = progress.phase();
            let bar = match progress.share() {
                Some(share) => egui::ProgressBar::new(share).text(crate::i18n::tf!("{phase} \u{B7} {} of {}", progress.done.load(std::sync::atomic::Ordering::Relaxed), progress.total.load(std::sync::atomic::Ordering::Relaxed))),
                None => egui::ProgressBar::new(0.0).animate(true).text(crate::i18n::tf!("{phase} \u{B7} {} found", progress.done.load(std::sync::atomic::Ordering::Relaxed))),
            };
            ui.add(bar.desired_width(ui.available_width().min(520.0)));
        }
        if let Some(result) = &fc.result {
            ui.label(RichText::new(result.counts().describe()).strong());
        }
    });
    let Some(result) = &fc.result else {
        return acts;
    };
    // The list: a status column, then each side's name, size and time.
    let visible = engine::visible_rows(&result.rows, &fc.expanded, fc.hide_same);
    let font = egui::FontId::proportional(13.0);
    let row_height = 20.0;
    let width = ui.available_width();
    let status_width = 190.0;
    let half = ((width - status_width - 8.0) / 2.0).floor();
    let (head, _) = ui.allocate_exact_size(egui::vec2(width, 18.0), egui::Sense::hover());
    {
        ui.painter_at(head).rect_filled(head, 0.0, theme::ruler_back());
        let x0 = head.min.x + status_width;
        for (from, to, words) in [(head.min.x, x0, "Status".to_string()), (x0, x0 + half, format!("Left: {}", result.left.describe())), (x0 + half + 8.0, head.max.x, format!("Right: {}", result.right.describe()))] {
            let area = egui::Rect::from_min_max(egui::pos2(from, head.min.y), egui::pos2(to - 4.0, head.max.y));
            ui.painter_at(area).text(egui::pos2(from + 6.0, head.center().y), egui::Align2::LEFT_CENTER, words, egui::FontId::proportional(12.0), theme::text_dim());
        }
    }
    let modifiers = ui.input(|i| i.modifiers);
    let mut clicked: Option<(usize, bool)> = None;
    let mut toggled: Option<String> = None;
    let mut keyboard_move: Option<isize> = None;
    if input_on && ui.memory(|m| m.focused().is_none()) {
        ui.input(|i| {
            if i.key_pressed(Key::ArrowDown) {
                keyboard_move = Some(1);
            } else if i.key_pressed(Key::ArrowUp) {
                keyboard_move = Some(-1);
            }
        });
        if ui.input(|i| i.key_pressed(Key::Enter))
            && let Some(anchor) = &fc.anchor
            && let Some(i) = result.rows.iter().position(|row| &row.path == anchor)
        {
            if result.rows[i].is_dir() {
                toggled = Some(anchor.clone());
            } else {
                acts.push(FolderAct::OpenPair(i));
            }
        }
        let side_key = ui.input(|i| if i.key_pressed(Key::ArrowRight) { Some(true) } else if i.key_pressed(Key::ArrowLeft) { Some(false) } else { None });
        if let (Some(open), Some(anchor)) = (side_key, &fc.anchor)
            && fc.expanded.contains(anchor) != open
            && result.rows.iter().any(|row| &row.path == anchor && row.is_dir())
        {
            toggled = Some(anchor.clone());
        }
    }
    ui.spacing_mut().item_spacing.y = 0.0;
    egui::ScrollArea::vertical().id_salt("folder-compare-rows").auto_shrink([false, false]).show_rows(ui, row_height, visible.len(), |ui, range| {
        for &i in &visible[range] {
            let row = &result.rows[i];
            let (rect, response) = ui.allocate_exact_size(egui::vec2(width, row_height), egui::Sense::click());
            let painter = ui.painter_at(rect);
            let selected = fc.selected.contains(&row.path);
            let back = if selected { theme::selection() } else { theme::editor_back() };
            painter.rect_filled(rect, 0.0, back);
            let status_rect = egui::Rect::from_min_size(rect.min, egui::vec2(status_width, row_height));
            painter.rect_filled(status_rect.shrink2(egui::vec2(2.0, 1.0)), 2.0, status_colour(row.status));
            painter.text(egui::pos2(status_rect.min.x + 6.0, rect.center().y), egui::Align2::LEFT_CENTER, format!("{}  {}", row.status.symbol(), row.status.words()), font.clone(), theme::editor_text());
            let indent = row.depth as f32 * 14.0;
            for left in [true, false] {
                let x0 = rect.min.x + status_width + if left { 0.0 } else { half + 8.0 };
                let area = egui::Rect::from_min_size(egui::pos2(x0, rect.min.y), egui::vec2(half, row_height));
                let Some(meta) = row.side(left) else {
                    painter.rect_filled(area.shrink2(egui::vec2(2.0, 1.0)), 0.0, theme::ruler_back());
                    continue;
                };
                let toggle = if row.is_dir() { if fc.expanded.contains(&row.path) { "\u{25BE}" } else { "\u{25B8}" } } else { " " };
                let name = if meta.is_dir { format!("{toggle} {}/", row.name) } else { format!("{toggle} {}", row.name) };
                painter.text(egui::pos2(x0 + 4.0 + indent, rect.center().y), egui::Align2::LEFT_CENTER, name, font.clone(), theme::editor_text());
                let detail = if meta.is_dir { String::new() } else { format!("{}   {}", short_size(meta.size), short_time(meta.modified)) };
                painter.text(egui::pos2(area.max.x - 6.0, rect.center().y), egui::Align2::RIGHT_CENTER, detail, egui::FontId::proportional(12.0), theme::text_dim());
            }
            let spoken = format!("{}: {}{}", row.status.words(), row.path, row.problem.as_deref().map(|p| format!(" ({p})")).unwrap_or_default());
            // Only when the pointer is on it: a row chosen with the keys shouldn't show its words in a box.
            let response = if response.hovered() { response.on_hover_text(&spoken) } else { response };
            crate::access::chosen(&response, egui::Role::ListItem, selected, &spoken);
            if response.clicked() {
                let on_toggle = row.is_dir() && response.interact_pointer_pos().is_some_and(|p| {
                    let x = p.x - rect.min.x - status_width;
                    let x = if x > half { x - half - 8.0 } else { x };
                    x >= indent && x <= indent + 18.0
                });
                if on_toggle {
                    toggled = Some(row.path.clone());
                } else {
                    clicked = Some((i, false));
                }
            }
            if response.double_clicked() {
                if row.is_dir() {
                    toggled = Some(row.path.clone());
                } else {
                    clicked = Some((i, true));
                }
            }
            crate::fit::context_menu(&response, |ui| {
                if !fc.selected.contains(&row.path) {
                    fc.selected = HashSet::from([row.path.clone()]);
                    fc.anchor = Some(row.path.clone());
                }
                if row.is_file_pair() && ui.button(crate::i18n::tr("Compare these files")).clicked() {
                    acts.push(FolderAct::OpenPair(i));
                    ui.close();
                }
                if row.left.is_some_and(|m| !m.is_dir) && ui.button(crate::i18n::tr("Open the left one")).clicked() {
                    acts.push(FolderAct::OpenOne(i, true));
                    ui.close();
                }
                if row.right.is_some_and(|m| !m.is_dir) && ui.button(crate::i18n::tr("Open the right one")).clicked() {
                    acts.push(FolderAct::OpenOne(i, false));
                    ui.close();
                }
                ui.separator();
                if ui.button(crate::i18n::tr("Copy to the right\u{2026}")).clicked() {
                    acts.push(FolderAct::OpenOne(usize::MAX, true));
                    ui.close();
                }
                if ui.button(crate::i18n::tr("Copy to the left\u{2026}")).clicked() {
                    acts.push(FolderAct::OpenOne(usize::MAX, false));
                    ui.close();
                }
            });
        }
    });
    if let Some(step) = keyboard_move
        && !visible.is_empty()
    {
        let at = fc.anchor.as_ref().and_then(|anchor| visible.iter().position(|&i| &result.rows[i].path == anchor));
        let next = match at {
            Some(at) => at.saturating_add_signed(step).min(visible.len() - 1),
            None => 0,
        };
        let path = result.rows[visible[next]].path.clone();
        fc.selected = HashSet::from([path.clone()]);
        fc.anchor = Some(path);
    }
    if let Some((i, double)) = clicked {
        let path = result.rows[i].path.clone();
        if double {
            acts.push(FolderAct::OpenPair(i));
        } else if modifiers.command {
            if !fc.selected.remove(&path) {
                fc.selected.insert(path.clone());
            }
            fc.anchor = Some(path);
        } else if modifiers.shift
            && let Some(from) = fc.anchor.as_ref().and_then(|anchor| visible.iter().position(|&v| &result.rows[v].path == anchor))
            && let Some(to) = visible.iter().position(|&v| v == i)
        {
            fc.selected = visible[from.min(to)..=from.max(to)].iter().map(|&v| result.rows[v].path.clone()).collect();
        } else {
            fc.selected = HashSet::from([path.clone()]);
            fc.anchor = Some(path);
        }
    }
    if let Some(path) = toggled {
        fc.toggle(&path);
    }
    acts
}

/// The box showing what a copy will do, before it's done. Returns whether to go ahead, or Some(false) to cancel.
fn copy_preview_window(ctx: &Context, preview: &CopyPreview) -> Option<bool> {
    let plan = &preview.plan;
    let mut answer = ctx.input(|i| i.key_pressed(Key::Escape)).then_some(false);
    let title = if plan.direction.goes_right() { "Copy to the right" } else { "Copy to the left" };
    crate::fit::Dialog::new(title).collapsible(false).resizable(true).default_width(560.0).anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0]).show(ctx, |ui| {
        ui.label(crate::i18n::tf!("Into {}:", plan.target));
        ui.add_space(4.0);
        egui::ScrollArea::vertical().max_height(300.0).auto_shrink([false, true]).show(ui, |ui| {
            for folder in &plan.folders {
                ui.label(crate::i18n::tf!("New folder    {folder}/"));
            }
            for item in &plan.files {
                ui.label(crate::i18n::tf!("{}    {}  ({})", if item.replaces { "Replace" } else { "New      " }, item.path, short_size(item.size)));
            }
            for (path, why) in &plan.skipped {
                ui.label(RichText::new(crate::i18n::tf!("Left alone    {path}: {why}")).color(theme::text_dim()));
            }
        });
        ui.add_space(6.0);
        let replaced = plan.replaced();
        if replaced > 0 {
            ui.label(if replaced == 1 { crate::i18n::tf!("The {replaced} file replaced will be kept first, in {}.", preview.backups.display()) } else { crate::i18n::tf!("The {replaced} files replaced will be kept first, in {}.", preview.backups.display()) });
        }
        ui.label(RichText::new(crate::i18n::tr("Nothing is deleted.")).color(theme::text_dim()));
        ui.add_space(6.0);
        ui.horizontal(|ui| {
            let count = plan.files.len();
            if ui.add_enabled(!plan.is_empty(), Button::new(if count == 1 { crate::i18n::tf!("Copy {count} file") } else { crate::i18n::tf!("Copy {count} files") })).clicked() {
                answer = Some(true);
            }
            if ui.button(crate::i18n::tr("Cancel")).clicked() {
                answer = Some(false);
            }
        });
    });
    answer
}

/// Where the files a copy replaces are kept.
fn compare_backups_root() -> PathBuf {
    if cfg!(test) {
        // Each test thread has its own, so one test tidying up never takes another's.
        return std::env::temp_dir().join(format!("limoniter-test-compare-backups-{}-{:?}", std::process::id(), std::thread::current().id()).replace(['(', ')'], ""));
    }
    settings_folder().map_or_else(|| std::env::temp_dir().join("Limoniter compare backups"), |folder| folder.join("Compare backups"))
}

// ---------- The window's side ----------

/// A short name for a path: its last part.
fn short_name(path: &Path) -> String {
    path.file_name().map_or_else(|| path.display().to_string(), |name| name.to_string_lossy().into_owned())
}

impl Limoniter {
    /// Opens a tab comparing two files. A file already open is compared as it is in its tab, unsaved edits and
    /// all, and edits made in the comparison are made there.
    pub fn open_file_compare(&mut self, left: impl AsRef<Path>, right: impl AsRef<Path>) {
        let absolute = |path: &Path| std::path::absolute(path).unwrap_or_else(|_| path.to_path_buf());
        self.open_file_compare_sides(FileSide::Path(absolute(left.as_ref())), FileSide::Path(absolute(right.as_ref())));
    }

    /// Opens a tab comparing two folders, or anything else a folder comparison can take: a disk image or archive
    /// is read as the files it holds, and sftp://user@host/folder is a folder on a server.
    pub fn open_folder_compare(&mut self, left: impl AsRef<Path>, right: impl AsRef<Path>) {
        self.open_folder_compare_sides(Some(Side::from_path(left.as_ref())), Some(Side::from_path(right.as_ref())));
    }

    /// Opens a tab comparing two sides of any kind. Either can be left to be chosen in the tab.
    pub fn open_folder_compare_sides(&mut self, left: Option<Side>, right: Option<Side>) {
        let blank = || SidePicker { revision: "HEAD".into(), ..SidePicker::default() };
        let both = left.is_some() && right.is_some();
        let pick = |side: Option<Side>| side.as_ref().map_or_else(blank, SidePicker::of);
        let mut folders = FolderCompare::new(pick(left), pick(right));
        folders.filters.rules = self.compare_rules.clone();
        let title = |picker: &SidePicker| if picker.path.is_empty() { "?".to_string() } else { short_name(Path::new(picker.path.trim_end_matches(['/', '\\']))) };
        let name = format!("{} \u{2194} {}", title(&folders.pickers[0]), title(&folders.pickers[1]));
        if both {
            folders.start(self.known_hosts.clone(), self.waker.clone());
        }
        self.add_compare_tab(name, CompareTab::Folders(Box::new(folders)));
    }

    /// Opens a tab comparing two files of any kind: text side by side and editable, or bytes, or pictures.
    pub fn open_file_compare_sides(&mut self, left: FileSide, right: FileSide) {
        match self.build_file_compare(left, right) {
            Ok(Some((name, tab))) => self.add_compare_tab(name, tab),
            Ok(None) => {}
            Err(problem) => self.message("Compare", problem),
        }
    }

    fn add_compare_tab(&mut self, name: String, tab: CompareTab) {
        let mut doc = Document::new(name);
        doc.set_read_only(true);
        doc.compare = Some(Box::new(tab));
        if self.docs.len() == 1 && self.docs[0].is_blank() {
            self.docs.clear();
        }
        self.docs.push(doc);
        self.active = self.docs.len() - 1;
        self.pending_focus = true;
    }

    /// The bytes of a side, for telling a picture, without reading a file that's too large.
    fn side_bytes(&self, side: &FileSide) -> Option<Vec<u8>> {
        match side {
            FileSide::Path(path) => std::fs::metadata(path).ok().filter(|meta| meta.len() <= DISK_THRESHOLD).and_then(|_| std::fs::read(path).ok()),
            FileSide::Open(id) => {
                let doc = self.docs.iter().find(|doc| doc.id == *id)?;
                if doc.picture.is_none() || doc.is_modified() {
                    return None;
                }
                doc.path.as_deref().and_then(|path| std::fs::read(path).ok())
            }
            FileSide::Bytes { bytes, .. } => Some(bytes.clone()),
            FileSide::Lines { .. } => None,
        }
    }

    fn side_name(&self, side: &FileSide) -> String {
        match side {
            FileSide::Path(path) => short_name(path),
            FileSide::Open(id) => self.docs.iter().find(|doc| doc.id == *id).map_or_else(|| "?".to_string(), Document::name),
            FileSide::Bytes { name, .. } | FileSide::Lines { name, .. } => name.clone(),
        }
    }

    /// Works out which kind of comparison two files need, and makes it. None when it's been started in the
    /// background, for files too large to hold.
    fn build_file_compare(&mut self, left: FileSide, right: FileSide) -> Result<Option<(String, CompareTab)>, String> {
        // A file that's open is compared as it is in its tab.
        let resolve = |app: &Self, side: FileSide| match side {
            FileSide::Path(path) => match app.doc_for_path(&path) {
                Some(index) if app.docs[index].compare.is_none() && app.docs[index].browser.is_none() => FileSide::Open(app.docs[index].id),
                _ => FileSide::Path(path),
            },
            other => other,
        };
        let (left, right) = (resolve(self, left), resolve(self, right));
        let same = match (&left, &right) {
            (FileSide::Open(a), FileSide::Open(b)) => a == b,
            (FileSide::Path(a), FileSide::Path(b)) => same_path(a, b),
            _ => false,
        };
        if same {
            return Err(crate::i18n::tr("That's the same file on both sides.").into());
        }
        for side in [&left, &right] {
            match side {
                FileSide::Open(id) => {
                    let doc = self.docs.iter().find(|doc| doc.id == *id).ok_or(crate::i18n::tr("That file isn't open any more."))?;
                    if doc.is_busy() {
                        return Err(crate::i18n::tf!("{} is still being read or written. Please try again when it's finished.", doc.name()));
                    }
                    if doc.compare.is_some() || doc.browser.is_some() {
                        return Err(crate::i18n::tf!("{} isn't a file that can be compared.", doc.name()));
                    }
                }
                FileSide::Path(path) if !path.is_file() => return Err(crate::i18n::tf!("{} isn't a file.", path.display())),
                _ => {}
            }
        }
        let names = [self.side_name(&left), self.side_name(&right)];
        let title = format!("{} \u{2194} {}", names[0], names[1]);
        // Pictures the window can show are compared as pictures.
        if let (Some(a), Some(b)) = (self.side_bytes(&left), self.side_bytes(&right)) {
            let picture = |name: &str, bytes: &[u8]| crate::pictures::detect(name, bytes).and_then(|format| crate::pictures::decode_named(format, name, bytes).ok());
            if let (Some(pa), Some(pb)) = (picture(&names[0], &a), picture(&names[1], &b)) {
                return Ok(Some((title, CompareTab::Pictures(Box::new(PictureCompare::new(names, [pa, pb]))))));
            }
        }
        // Files too large to hold are compared line by line in the background, and only read.
        let large = |app: &Self, side: &FileSide| match side {
            FileSide::Path(path) => std::fs::metadata(path).is_ok_and(|meta| meta.len() > DISK_THRESHOLD),
            FileSide::Open(id) => app.docs.iter().find(|doc| doc.id == *id).is_some_and(|doc| if doc.hex_mode { doc.hex.is_on_disk() } else { doc.text.is_disk_backed() }),
            _ => false,
        };
        if large(self, &left) || large(self, &right) {
            self.start_streamed_compare(&names, &left, &right)?;
            return Ok(None);
        }
        let make = |side: FileSide, name: &str| -> Result<(Source2, String), String> {
            Ok(match side {
                FileSide::Open(id) => (Source2::Open(id), self.docs.iter().find(|doc| doc.id == id).and_then(|doc| doc.path.as_ref()).map_or_else(|| name.to_string(), |path| path.display().to_string())),
                FileSide::Path(path) => {
                    let mut doc = Document::open(&path, self.encoding_prefs.get(&path))?;
                    if std::fs::metadata(&path).is_ok_and(|meta| meta.permissions().readonly()) {
                        doc.set_read_only(true);
                    }
                    (Source2::Own(Box::new(doc)), path.display().to_string())
                }
                FileSide::Bytes { name, bytes } => (Source2::Own(Box::new(Document::from_image_file(name.clone(), bytes, None))), name),
                FileSide::Lines { name, lines } => {
                    let mut doc = Document::untitled(name.clone(), &PRESETS[0]);
                    doc.text.splice_lines(0, 1, lines);
                    doc.text.mark_saved();
                    doc.set_read_only(true);
                    (Source2::Own(Box::new(doc)), name)
                }
            })
        };
        let (a, a_name) = make(left, &names[0])?;
        let (b, b_name) = make(right, &names[1])?;
        // Anything that isn't text is compared byte by byte.
        let hex = |app: &Self, source: &Source2| match source {
            Source2::Open(id) => app.docs.iter().find(|doc| doc.id == *id).is_some_and(|doc| doc.hex_mode),
            Source2::Own(doc) => doc.hex_mode,
        };
        if hex(self, &a) || hex(self, &b) {
            let bytes = |app: &Self, source: &Source2| -> Vec<u8> {
                let doc = match source {
                    Source2::Open(id) => app.docs.iter().find(|doc| doc.id == *id),
                    Source2::Own(doc) => Some(&**doc),
                };
                doc.map_or_else(Vec::new, |doc| if doc.hex_mode { doc.hex.bytes().to_vec() } else { doc.text.encode(&doc.text.format, Lossy::Replace).unwrap_or_default() })
            };
            let pair = [bytes(self, &a), bytes(self, &b)];
            return Ok(Some((title, CompareTab::Bytes(Box::new(ByteCompare::new(names, pair))))));
        }
        let mut files = FileCompare { sources: [a, b], names: [a_name, b_name], diff: DiffView::new() };
        if let Some(pair) = two_docs(&mut self.docs, &mut files.sources) {
            files.diff.refresh([&*pair[0], &*pair[1]]);
            if files.diff.difference_count() > 0 {
                files.diff.step(pair, true);
            }
        }
        Ok(Some((title, CompareTab::Files(Box::new(files)))))
    }

    /// Compares files too large to hold, in the background; the result opens in a tab of its own, read-only.
    fn start_streamed_compare(&mut self, names: &[String; 2], left: &FileSide, right: &FileSide) -> Result<(), String> {
        if self.big_job.is_some() {
            return Err(crate::i18n::tr("Please wait for the file being worked through to finish.").into());
        }
        let side = |app: &Self, side: &FileSide| -> Result<crate::big_jobs::Side, String> {
            Ok(match side {
                FileSide::Path(path) => {
                    if std::fs::metadata(path).map_err(|e| e.to_string())?.len() > DISK_THRESHOLD {
                        crate::big_jobs::Side::Scanning(Source::open_as(path, None).map_err(|e| e.to_string())?)
                    } else {
                        let text = tools::text_of_file(path)?;
                        crate::big_jobs::Side::Lines(LineStore::from_vec(text.split('\n').map(Arc::from).collect()))
                    }
                }
                FileSide::Open(id) => {
                    let doc = app.docs.iter().find(|doc| doc.id == *id).ok_or(crate::i18n::tr("That file isn't open any more."))?;
                    if doc.hex_mode {
                        return Err(crate::i18n::tf!("{} is open in hex mode, and too large to compare byte by byte here.", doc.name()));
                    }
                    crate::big_jobs::Side::Lines(doc.text.store().clone())
                }
                FileSide::Bytes { bytes, .. } => crate::big_jobs::Side::Lines(LineStore::from_vec(String::from_utf8_lossy(bytes).split('\n').map(Arc::from).collect())),
                FileSide::Lines { lines, .. } => crate::big_jobs::Side::Lines(LineStore::from_vec(lines.clone())),
            })
        };
        let (a, b) = (side(self, left)?, side(self, right)?);
        let doc_id = self.docs[self.active].id;
        self.big_job = Some(crate::big_jobs::compare(names[0].clone(), names[1].clone(), a, b, doc_id));
        Ok(())
    }

    /// Shows a comparison of large files that's been worked out, in a tab of its own.
    pub(super) fn show_streamed_compare(&mut self, mut comparison: Box<compare::Comparison>) {
        comparison.step(true);
        let name = format!("{} \u{2194} {}", comparison.left_name, comparison.right_name);
        self.add_compare_tab(name, CompareTab::Streamed(comparison));
    }

    /// Draws the compare tab in front.
    pub(super) fn compare_view(&mut self, ui: &mut Ui, input_on: bool) {
        let index = self.active;
        let Some(mut tab) = self.docs[index].compare.take() else {
            return;
        };
        let doc_id = self.docs[index].id;
        let mut file_acts = Vec::new();
        let mut folder_acts = Vec::new();
        let mut store_session = false;
        match &mut *tab {
            CompareTab::Files(files) => {
                self.reopen_closed_sides(files);
                file_acts = draw_file_compare(ui, files, &mut self.docs, &mut self.clip, self.view, input_on);
            }
            CompareTab::Folders(folders) => {
                folders.poll();
                folders.poll_sync(self.known_hosts.clone(), self.waker.clone());
                if let Some(report) = folders.copy_job.as_ref().and_then(Job::take) {
                    folders.copy_job = None;
                    folders.notice = Some(report.describe());
                    folders.start(self.known_hosts.clone(), self.waker.clone());
                }
                if let Some((path, done)) = folders.patch_job.as_ref().and_then(|(path, job)| job.take().map(|done| (path.clone(), done))) {
                    folders.patch_job = None;
                    folders.notice = Some(match done.and_then(|patch| codec::write_file(&path, patch.as_bytes()).map_err(|e| e.to_string())) {
                        Ok(()) => crate::i18n::tf!("The patch is saved in {}.", path.display()),
                        Err(problem) => crate::i18n::tf!("The patch couldn't be saved: {problem}"),
                    });
                }
                if folders.busy() {
                    ui.ctx().request_repaint_after(Duration::from_millis(100));
                }
                folder_acts = draw_folder_compare(ui, folders, input_on, &self.remote_sites);
                if let Some(preview) = &folders.copy {
                    match copy_preview_window(ui.ctx(), preview) {
                        Some(true) => folders.confirm_copy(self.waker.clone()),
                        Some(false) => folders.copy = None,
                        None => {}
                    }
                }
                if let Some(preview) = &mut folders.sync {
                    match sync_host::sync_preview_window(ui.ctx(), preview) {
                        Some(true) => folders.confirm_sync(self.waker.clone()),
                        Some(false) => folders.sync = None,
                        None => {}
                    }
                }
                store_session = std::mem::take(&mut folders.session_dirty);
            }
            CompareTab::Bytes(bytes) => draw_byte_compare(ui, bytes),
            CompareTab::Pictures(pictures) => draw_picture_compare(ui, pictures),
            CompareTab::Streamed(comparison) => compare::view(ui, comparison),
        }
        // The tab goes back where it was, wherever that is now.
        if let Some(doc) = self.docs.iter_mut().find(|doc| doc.id == doc_id) {
            doc.compare = Some(tab);
        }
        if let Some(problem) = NOTICE.with(|notice| notice.borrow_mut().take()) {
            self.message("Compare", problem);
        }
        for act in file_acts {
            self.file_compare_act(doc_id, act);
        }
        if store_session {
            self.store_compare_session(doc_id);
        }
        for act in folder_acts {
            self.folder_compare_act(doc_id, act);
        }
    }

    /// A side whose own tab was closed is read again from its file, or kept as it was if it has none.
    fn reopen_closed_sides(&mut self, files: &mut FileCompare) {
        for (source, name) in files.sources.iter_mut().zip(&files.names) {
            if let Source2::Open(id) = *source
                && !self.docs.iter().any(|doc| doc.id == id)
            {
                let path = PathBuf::from(name);
                let doc = if path.is_file() { Document::open(&path, self.encoding_prefs.get(&path)).ok() } else { None };
                let doc = doc.unwrap_or_else(|| {
                    let mut doc = Document::untitled(name.clone(), &PRESETS[0]);
                    doc.set_read_only(true);
                    doc
                });
                *source = Source2::Own(Box::new(doc));
                files.diff.worked_out = None;
            }
        }
    }

    /// Runs something with the compare tab taken out of its document, for as long as it takes.
    fn with_compare_tab<T>(&mut self, doc_id: u64, work: impl FnOnce(&mut Self, &mut CompareTab) -> T) -> Option<T> {
        let index = self.docs.iter().position(|doc| doc.id == doc_id)?;
        let mut tab = self.docs[index].compare.take()?;
        let out = work(self, &mut tab);
        if let Some(doc) = self.docs.iter_mut().find(|doc| doc.id == doc_id) {
            doc.compare = Some(tab);
        }
        Some(out)
    }

    fn file_compare_act(&mut self, doc_id: u64, act: FileAct) {
        match act {
            FileAct::Save(side) => {
                self.save_compare_side(doc_id, side);
            }
            FileAct::OpenInTab(side) => {
                let moved = self.with_compare_tab(doc_id, |_, tab| {
                    let CompareTab::Files(files) = tab else { return None };
                    let Source2::Own(_) = &files.sources[side] else { return None };
                    let Source2::Own(doc) = std::mem::replace(&mut files.sources[side], Source2::Open(0)) else { return None };
                    files.sources[side] = Source2::Open(doc.id);
                    Some(doc)
                });
                if let Some(Some(doc)) = moved {
                    self.docs.push(*doc);
                }
            }
            FileAct::ExportPatch => {
                let patch = self.with_compare_tab(doc_id, |app, tab| {
                    let CompareTab::Files(files) = tab else { return None };
                    let names = files.names.clone();
                    let pair = two_docs(&mut app.docs, &mut files.sources)?;
                    Some(file_patch(&names, [&*pair[0], &*pair[1]], &files.diff.options))
                });
                let Some(Some(patch)) = patch else { return };
                if patch.is_empty() {
                    return self.message("Save as a patch", crate::i18n::tr("The two are the same, so there's nothing to put in a patch.").into());
                }
                if let Some(path) = crate::sys_dialogs::file_dialog().set_title(crate::i18n::tr("Save as a patch")).set_file_name("changes.patch").save_file()
                    && let Err(e) = codec::write_file(&path, patch.as_bytes())
                {
                    self.message("Save as a patch", crate::i18n::tf!("Couldn't save {}.\n\n{e}", path.display()));
                }
            }
        }
    }

    /// Saves one side of a file comparison: through its own tab if it has one, or straight to its file.
    pub(super) fn save_compare_side(&mut self, doc_id: u64, side: usize) -> bool {
        let open = self.with_compare_tab(doc_id, |_, tab| match tab {
            CompareTab::Files(files) => match &files.sources[side] {
                Source2::Open(id) => Some(*id),
                Source2::Own(_) => None,
            },
            _ => None,
        });
        if let Some(Some(id)) = open {
            return self.docs.iter().position(|doc| doc.id == id).is_some_and(|index| self.save(index));
        }
        let backup = self.prefs.backup_on_save;
        let said = self.with_compare_tab(doc_id, |_, tab| {
            let CompareTab::Files(files) = tab else { return Ok(false) };
            let Source2::Own(doc) = &mut files.sources[side] else { return Ok(false) };
            save_own(doc, backup)
        });
        match said {
            Some(Ok(saved)) => saved,
            Some(Err(problem)) => {
                self.message("Save", problem);
                false
            }
            None => false,
        }
    }

    /// Saves every side of the compare tab in front that has changes.
    pub(super) fn save_compare_tab(&mut self, index: usize) -> bool {
        let doc_id = self.docs[index].id;
        let wanted = self.with_compare_tab(doc_id, |app, tab| match tab {
            CompareTab::Files(files) => (0..2)
                .filter(|&n| match &files.sources[n] {
                    Source2::Own(doc) => doc.is_modified(),
                    Source2::Open(id) => app.docs.iter().any(|doc| doc.id == *id && doc.is_modified()),
                })
                .collect(),
            _ => Vec::new(),
        });
        let mut all = true;
        for side in wanted.unwrap_or_default() {
            all &= self.save_compare_side(doc_id, side);
        }
        all
    }

    fn folder_compare_act(&mut self, doc_id: u64, act: FolderAct) {
        match act {
            FolderAct::Start => {
                let (known, waker) = (self.known_hosts.clone(), self.waker.clone());
                self.with_compare_tab(doc_id, |_, tab| {
                    if let CompareTab::Folders(folders) = tab {
                        folders.start(known, waker);
                    }
                });
            }
            FolderAct::Browse(n) => {
                let kind = self.with_compare_tab(doc_id, |_, tab| if let CompareTab::Folders(folders) = tab { Some(folders.pickers[n].kind) } else { None }).flatten();
                let picked = match kind {
                    Some(SideKind::Image) => crate::sys_dialogs::file_dialog().set_title(crate::i18n::tr("A disk image or archive to compare")).pick_file(),
                    Some(SideKind::Folder | SideKind::Git) => crate::sys_dialogs::file_dialog().set_title(crate::i18n::tr("A folder to compare")).pick_folder(),
                    _ => None,
                };
                if let Some(path) = picked {
                    self.with_compare_tab(doc_id, |_, tab| {
                        if let CompareTab::Folders(folders) = tab {
                            folders.pickers[n].path = path.display().to_string();
                        }
                    });
                }
            }
            FolderAct::OpenOne(usize::MAX, to_right) => {
                let root = compare_backups_root();
                let planned = self.with_compare_tab(doc_id, |_, tab| if let CompareTab::Folders(folders) = tab { folders.plan_copy(if to_right { Direction::LeftToRight } else { Direction::RightToLeft }, &root) } else { Ok(()) });
                if let Some(Err(problem)) = planned {
                    self.message("Copy", problem);
                }
            }
            FolderAct::OpenOne(row, left) => {
                let found = self.with_compare_tab(doc_id, |_, tab| {
                    let CompareTab::Folders(folders) = tab else { return None };
                    let result = folders.result.as_ref()?;
                    let path = &result.rows.get(row)?.path;
                    Some(file_side_of(result, path, left))
                });
                match found.flatten() {
                    Some(Ok(FileSide::Path(path))) => {
                        self.open_path(&path);
                    }
                    Some(Ok(FileSide::Bytes { name, bytes })) => {
                        let doc = Document::from_image_file(name, bytes, None);
                        self.docs.push(doc);
                        self.active = self.docs.len() - 1;
                    }
                    Some(Err(problem)) => self.message("Open", problem),
                    _ => {}
                }
            }
            FolderAct::OpenPair(row) => {
                let found = self.with_compare_tab(doc_id, |_, tab| {
                    let CompareTab::Folders(folders) = tab else { return None };
                    let result = folders.result.as_ref()?;
                    let found = result.rows.get(row)?;
                    if !found.is_file_pair() {
                        // A file on one side only opens by itself.
                        return Some(Err(found.left.is_some()));
                    }
                    let rule = folders.filters.rules.rule_for(&found.path, &found.name, false).cloned();
                    Some(Ok((file_side_of(result, &found.path, true), file_side_of(result, &found.path, false), rule)))
                });
                match found.flatten() {
                    Some(Ok((Ok(left), Ok(right), rule))) => self.open_ruled_pair(left, right, rule),
                    Some(Ok((Err(problem), _, _) | (_, Err(problem), _))) => self.message("Compare", problem),
                    Some(Err(left)) => self.folder_compare_act(doc_id, FolderAct::OpenOne(row, left)),
                    None => {}
                }
            }
            FolderAct::ExportReport(kind) => {
                let dialog = crate::sys_dialogs::file_dialog().set_title(crate::i18n::tr("Save a report"));
                let dialog = match kind {
                    ReportKind::Csv => dialog.add_filter(crate::i18n::tr("CSV file"), &["csv"]).set_file_name("comparison.csv"),
                    ReportKind::Summary => dialog.add_filter(crate::i18n::tr("Plain text"), &["txt"]).add_filter(crate::i18n::tr("HTML page"), &["html"]).set_file_name("comparison summary.txt"),
                    ReportKind::Full => dialog.add_filter(crate::i18n::tr("HTML page"), &["html"]).add_filter(crate::i18n::tr("Plain text"), &["txt"]).set_file_name("comparison.html"),
                };
                let Some(path) = dialog.save_file() else {
                    return;
                };
                if let Some(Err(problem)) = self.save_folder_report_as(doc_id, &path, kind) {
                    self.message("Save a report", problem);
                }
            }
            FolderAct::Sync => {
                let root = compare_backups_root();
                self.with_compare_tab(doc_id, |_, tab| {
                    if let CompareTab::Folders(folders) = tab {
                        folders.plan_sync(root, None);
                    }
                });
            }
            FolderAct::SaveSession => self.save_compare_session(doc_id),
            FolderAct::Rules => self.open_rules_box(Some(doc_id)),
            FolderAct::ApplyPatch(left) => {
                let folder = self.with_compare_tab(doc_id, |_, tab| match tab {
                    CompareTab::Folders(folders) => match folders.pickers[if left { 0 } else { 1 }].side() {
                        Ok(Side::Folder(path)) => Ok(path),
                        Ok(other) => Err(crate::i18n::tf!("A patch can only be applied to a folder on this computer, and {} isn't one.", other.describe())),
                        Err(problem) => Err(problem),
                    },
                    _ => Err(String::new()),
                });
                match folder {
                    Some(Ok(folder)) => self.apply_patch_to(Some(folder), Some(doc_id)),
                    Some(Err(problem)) if !problem.is_empty() => self.message("Apply a patch", problem),
                    _ => {}
                }
            }
            FolderAct::ExportPatch => {
                let Some(path) = crate::sys_dialogs::file_dialog().set_title(crate::i18n::tr("Save as a patch")).set_file_name("changes.patch").save_file() else {
                    return;
                };
                self.start_folder_patch(doc_id, path);
            }
        }
    }

    /// Saves a folder comparison's report: an HTML page if the name ends .html or .htm, plain text otherwise.
    #[cfg(test)]
    pub(super) fn save_folder_report(&mut self, doc_id: u64, path: &Path) -> Option<Result<(), String>> {
        self.save_folder_report_as(doc_id, path, ReportKind::Full)
    }

    /// Saves a report of a kind: every entry or the counts only (each an HTML page if the name ends .html or
    /// .htm, plain text otherwise), or a CSV file.
    pub(super) fn save_folder_report_as(&mut self, doc_id: u64, path: &Path, kind: ReportKind) -> Option<Result<(), String>> {
        self.with_compare_tab(doc_id, |_, tab| {
            let CompareTab::Folders(folders) = tab else { return Err(crate::i18n::tr("This isn't a folder comparison.").to_string()) };
            let result = folders.result.as_ref().ok_or(crate::i18n::tr("Compare the folders first."))?;
            let html = path.extension().and_then(|e| e.to_str()).is_some_and(|e| e.eq_ignore_ascii_case("html") || e.eq_ignore_ascii_case("htm"));
            let report = match kind {
                ReportKind::Csv => engine::report_csv(result, folders.hide_same),
                ReportKind::Summary => engine::report_summary(result, html),
                ReportKind::Full if html => engine::report_html(result, folders.hide_same),
                ReportKind::Full => engine::report_text(result, folders.hide_same),
            };
            codec::write_file(path, report.as_bytes()).map_err(|e| e.to_string())?;
            folders.notice = Some(crate::i18n::tf!("The report is saved in {}.", path.display()));
            Ok(())
        })
    }

    /// Writes a patch of every difference in a folder comparison, in the background.
    pub(super) fn start_folder_patch(&mut self, doc_id: u64, path: PathBuf) {
        let waker = self.waker.clone();
        self.with_compare_tab(doc_id, |_, tab| {
            let CompareTab::Folders(folders) = tab else { return };
            let Some(result) = folders.result.as_ref() else { return };
            // The patch is made from the trees already open, and the rows as they are now.
            let snapshot = FolderResult { left: result.left.clone(), right: result.right.clone(), mode: result.mode, filters: result.filters.clone(), rows: result.rows.clone(), left_tree: result.left_tree.clone(), right_tree: result.right_tree.clone() };
            let job = Job::start(
                move |progress, cancel| {
                    progress.set_phase("Making the patch");
                    engine::folder_patch(&snapshot, cancel)
                },
                move || {
                    if let Some(ctx) = waker {
                        ctx.request_repaint();
                    }
                },
            );
            folders.patch_job = Some((path, job));
        });
    }

    /// Asks which two files to compare, and compares them.
    pub(super) fn compare_two_files(&mut self) {
        let Some(left) = crate::sys_dialogs::file_dialog().set_title(crate::i18n::tr("Compare files: the first, on the left")).pick_file() else {
            return;
        };
        let mut second = crate::sys_dialogs::file_dialog().set_title(crate::i18n::tr("Compare files: the second, on the right"));
        if let Some(folder) = left.parent() {
            second = second.set_directory(folder);
        }
        if let Some(right) = second.pick_file() {
            self.open_file_compare(left, right);
        }
    }

    /// A comparison asked for from the Project panel.
    pub(super) fn project_compare(&mut self, action: crate::project::PanelAction) {
        match action {
            crate::project::PanelAction::CompareWith(path) => {
                let mut picker = crate::sys_dialogs::file_dialog().set_title(crate::i18n::tr("Compare with"));
                if let Some(folder) = path.parent() {
                    picker = picker.set_directory(folder);
                }
                if let Some(other) = picker.pick_file() {
                    self.open_file_compare(path, other);
                }
            }
            crate::project::PanelAction::CompareFolder(path) => self.open_folder_compare_sides(Some(Side::Folder(path)), None),
            crate::project::PanelAction::CompareWithHead(path) => self.compare_with_head(&path),
            crate::project::PanelAction::OpenComparison(name) => self.open_compare_session(&name, true),
            crate::project::PanelAction::ManageComparisons(name) => self.open_sessions_box(Some(name)),
            crate::project::PanelAction::Settings => self.open_project_settings(),
            _ => {}
        }
    }

    /// Compares a folder or a file with how it was at the last commit: the commit on the left, read-only.
    pub(super) fn compare_with_head(&mut self, path: &Path) {
        // A notebook is compared by its cells, not as JSON lines full of outputs.
        if !path.is_dir() && super::notebook_host::is_notebook_name(path) {
            return self.compare_notebook(path, "HEAD", false);
        }
        let side = match Side::git_of(path, "HEAD") {
            Ok(side) => side,
            Err(problem) => return self.message("Compare with the last commit", problem),
        };
        if path.is_dir() {
            return self.open_folder_compare_sides(Some(side), Some(Side::Folder(path.to_path_buf())));
        }
        let Side::Git { repo, folder, .. } = &side else { return };
        match crate::git::file_at(repo, "HEAD", folder) {
            Some(bytes) => {
                let name = crate::i18n::tf!("{} at the last commit", short_name(path));
                self.open_file_compare_sides(FileSide::Bytes { name, bytes }, FileSide::Path(path.to_path_buf()));
            }
            None => self.message("Compare with the last commit", crate::i18n::tf!("{} wasn't in the last commit.", short_name(path))),
        }
    }

    /// Opens a folder comparison, starting from the project's folder or the folder of the file in front.
    pub(super) fn compare_folders_command(&mut self) {
        let start = self.project.as_ref().map(|project| project.folder()).or_else(|| self.docs[self.active].path.as_deref().and_then(Path::parent).map(Path::to_path_buf));
        self.open_folder_compare_sides(start.map(Side::Folder), None);
    }

    /// A command given while a compare tab is in front. Editing commands go to the side in use; true if it's
    /// been dealt with.
    pub(super) fn compare_command(&mut self, command: Command, ui: &Ui) -> bool {
        let index = self.active;
        let doc_id = self.docs[index].id;
        let Some(tab) = self.docs[index].compare.as_ref() else {
            return false;
        };
        let CompareTab::Files(files) = &**tab else {
            return matches!(command, Command::Save | Command::SaveAs) || command.edits();
        };
        let side = files.diff.active;
        match command {
            Command::Save => {
                self.save_compare_tab(index);
                return true;
            }
            Command::SaveAs => {
                self.message("Save as", crate::i18n::tr("Each side of a comparison saves to its own file. Open a side in a tab of its own to save it somewhere else.").into());
                return true;
            }
            Command::NextChange | Command::LastChange => {
                let forward = command == Command::NextChange;
                self.with_compare_tab(doc_id, |app, tab| {
                    if let CompareTab::Files(files) = tab
                        && let Some(pair) = two_docs(&mut app.docs, &mut files.sources)
                    {
                        files.diff.step(pair, forward);
                    }
                });
                return true;
            }
            _ => {}
        }
        if !(command.edits() || matches!(command, Command::Copy | Command::SelectAll | Command::Find | Command::FindNext | Command::FindPrevious | Command::Replace | Command::GoTo)) {
            return false;
        }
        // The side's document is put in front for the command, then the comparison is again.
        let open = match &files.sources[side] {
            Source2::Open(id) => Some(*id),
            Source2::Own(_) => None,
        };
        match open {
            Some(id) => {
                let Some(at) = self.docs.iter().position(|doc| doc.id == id) else { return true };
                self.active = at;
                self.run(command, ui);
                if let Some(back) = self.docs.iter().position(|doc| doc.id == doc_id) {
                    self.active = back;
                }
            }
            None => {
                let taken = self.with_compare_tab(doc_id, |_, tab| match tab {
                    CompareTab::Files(files) => match std::mem::replace(&mut files.sources[side], Source2::Open(0)) {
                        Source2::Own(doc) => Some(doc),
                        other => {
                            files.sources[side] = other;
                            None
                        }
                    },
                    _ => None,
                });
                let Some(Some(doc)) = taken else { return true };
                self.docs.push(*doc);
                self.active = self.docs.len() - 1;
                self.run(command, ui);
                let doc = self.docs.pop().map(Box::new);
                if let Some(back) = self.docs.iter().position(|d| d.id == doc_id) {
                    self.active = back;
                }
                self.with_compare_tab(doc_id, |_, tab| {
                    if let (CompareTab::Files(files), Some(doc)) = (tab, doc) {
                        files.sources[side] = Source2::Own(doc);
                    }
                });
            }
        }
        true
    }
}

/// A file from one side of a folder comparison: where it is if it's on this computer, or its bytes.
fn file_side_of(result: &FolderResult, path: &str, left: bool) -> Result<FileSide, String> {
    let tree = result.tree(left);
    if let Some(local) = tree.local_path(path) {
        return Ok(FileSide::Path(local));
    }
    let side = if left { &result.left } else { &result.right };
    let name = path.rsplit('/').next().unwrap_or(path);
    let at = match side {
        Side::Git { revision, .. } => format!("{name} at {}", engine::revision_words(revision)),
        _ => format!("{name} in {}", short_name(Path::new(&side.describe()))),
    };
    Ok(FileSide::Bytes { name: at, bytes: tree.read(path)? })
}

/// Saves a document only a comparison has, to its file, keeping a backup first the first time if asked.
fn save_own(doc: &mut Document, backup: bool) -> Result<bool, String> {
    let Some(path) = doc.path.clone() else {
        return Ok(false);
    };
    if doc.text.read_only {
        return Err(crate::i18n::tf!("{} is read-only here.", doc.name()));
    }
    if doc.text.format.ensure_final_newline {
        doc.text.ensure_final_newline();
    }
    let format = doc.text.format.clone();
    let bytes = doc.text.encode(&format, Lossy::Refuse).map_err(|_| crate::i18n::tf!("Some characters in {} can't be saved in {}. Open this side in a tab of its own to choose what to do with them.", doc.name(), format.encoding_name()))?;
    if backup && doc.backed_up.as_deref().is_none_or(|done| !same_path(done, &path)) && path.is_file() {
        let mut kept = path.as_os_str().to_owned();
        kept.push(".bak");
        std::fs::copy(&path, &kept).map_err(|e| crate::i18n::tf!("Couldn't keep a backup of {} before saving, so it hasn't been saved.\n\n{e}", path.display()))?;
        doc.backed_up = Some(path.clone());
    }
    codec::write_file(&path, &bytes).map_err(|e| crate::i18n::tf!("Couldn't save {}.\n\n{e}", path.display()))?;
    note_saved_to(doc, &path);
    doc.disk_stamp = file_stamp(&path);
    doc.text.mark_saved();
    Ok(true)
}

/// A unified diff of the two sides of a file comparison as they are now.
fn file_patch(names: &[String; 2], docs: [&Document; 2], options: &TextOptions) -> String {
    let lines = |doc: &Document| -> (Vec<String>, bool) {
        let mut lines: Vec<String> = doc.text.all_lines().iter().map(|line| line.to_string()).collect();
        // The text's last line is the one after the last line ending, if it has one.
        let ends = lines.len() > 1 && lines.last().is_some_and(String::is_empty);
        if ends {
            lines.pop();
        }
        (lines, ends)
    };
    let ((left, left_final), (right, right_final)) = (lines(docs[0]), lines(docs[1]));
    let chunks = engine::diff_lines(&left, &right, options);
    let name = |n: usize| names[n].replace('\\', "/");
    engine::unified_diff(&name(0), &name(1), &left, &right, left_final, right_final, &chunks)
}

#[path = "sync_host.rs"]
pub(super) mod sync_host;

#[cfg(test)]
#[path = "compare_host_tests.rs"]
mod compare_host_tests;
