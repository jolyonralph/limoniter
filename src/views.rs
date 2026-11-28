//! Drawing the text and hex editors, and turning keyboard and mouse input into edits.
//!
//! Scrolling counts whole lines rather than pixels, because a pixel position can't be held
//! accurately once a file runs to many millions of lines.

use crate::charset::{self, CellState, Kind, raw_byte};
use crate::codec::{Display, TextFormat};
use crate::console;
use crate::hex_buffer::{ASCII_GAP_CELLS, HexBuffer, ascii_cell, bytes_per_row, hex_cell};
use crate::highlight::{self, LineColours};
use crate::rainbow::{RainbowCache, scan_line};
use crate::wrap::{self, Slot};
use crate::text_buffer::{DEFAULT_TAB_SIZE, MAX_TAB_SIZE, MIN_TAB_SIZE, TextBuffer, display_text, is_special, next_visual, source_view, tab_size};
use crate::theme;
use eframe::egui::{
    self, Align2, Color32, CursorIcon, Event, EventFilter, FontId, Id, Key, Modifiers, Pos2, Rect, Sense, Stroke, Ui, Vec2, pos2, vec2,
};

const CARET_BLINK_MS: u64 = 530;
const BAR: f32 = 14.0;
/// How long colouring may take in one frame, so typing and scrolling stay smooth.
const HIGHLIGHT_BUDGET: std::time::Duration = std::time::Duration::from_millis(12);
const MAP_WIDTH: f32 = 130.0;

/// The focus id for the main editor, or the only one showing.
pub fn editor_id() -> Id {
    Id::unique("limoniter-editor")
}

/// The focus id for one pane of a split window.
pub fn pane_id(number: usize) -> Id {
    match number {
        0 => editor_id(),
        1 => Id::unique("limoniter-editor-second"),
        _ => Id::unique("limoniter-editor-beside"),
    }
}

/// The focus id of a table view, which takes commands as the text would.
pub fn table_id() -> Id {
    Id::unique("limoniter-table")
}

/// The focus id of a notebook, whose cells are in command mode (no cell is being typed in).
pub fn notebook_id() -> Id {
    Id::unique("limoniter-notebook")
}

pub fn is_editor_id(id: Id) -> bool {
    id == pane_id(0) || id == pane_id(1) || id == table_id() || id == notebook_id()
}

/// One stretch of hidden lines, and how many lines show before it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Hidden {
    pub first: usize,
    pub last: usize,
    /// How many lines show before this stretch.
    pub shown_before: usize,
}

/// Which lines show when not every line does: a list of them, as a filter makes; or every line but some stretches,
/// as hiding and folding leave, which takes no more room however many lines the file has.
#[derive(Clone, Copy, Debug)]
pub enum Shown<'a> {
    Lines(&'a [usize]),
    AllBut { hidden: &'a [Hidden], total: usize },
}

impl Shown<'_> {
    /// How many lines show.
    pub fn len(&self) -> usize {
        match self {
            Shown::Lines(lines) => lines.len(),
            Shown::AllBut { hidden, total } => total - hidden.iter().map(|h| h.last + 1 - h.first).sum::<usize>(),
        }
    }

    /// The line that shows at a row.
    pub fn line(&self, row: usize) -> Option<usize> {
        match self {
            Shown::Lines(lines) => lines.get(row).copied(),
            Shown::AllBut { hidden, total } => {
                // The stretches hidden before the row's line add to its number.
                let before = hidden.partition_point(|h| h.shown_before <= row);
                let skipped: usize = hidden[..before].iter().map(|h| h.last + 1 - h.first).sum();
                Some(row + skipped).filter(|line| line < total)
            }
        }
    }

    /// How many showing lines come before a line: its row if it shows.
    pub fn row_of(&self, line: usize) -> usize {
        match self {
            Shown::Lines(lines) => lines.partition_point(|shown| *shown < line),
            Shown::AllBut { hidden, .. } => {
                let hidden_before: usize = hidden.iter().take_while(|h| h.first < line).map(|h| h.last.min(line - 1) + 1 - h.first).sum();
                line - hidden_before
            }
        }
    }

    pub fn contains(&self, line: usize) -> bool {
        match self {
            Shown::Lines(lines) => lines.binary_search(&line).is_ok(),
            Shown::AllBut { hidden, total } => line < *total && !hidden.iter().any(|h| (h.first..=h.last).contains(&line)),
        }
    }

    /// The lines showing from a row on.
    pub fn rows_from(&self, row: usize, count: usize) -> Vec<usize> {
        (row..row + count).map_while(|r| self.line(r)).collect()
    }
}

/// The stretches folds hide, from where they start and end: each fold hides the lines after its first, and a fold
/// inside another hides nothing more.
pub fn hidden_by_folds(folds: &std::collections::BTreeMap<usize, usize>) -> Vec<Hidden> {
    let mut out: Vec<Hidden> = Vec::new();
    let mut hidden_so_far = 0;
    for (&start, &last) in folds {
        let first = start + 1;
        if last < first {
            continue;
        }
        match out.last_mut() {
            Some(previous) if first <= previous.last + 1 => {
                if last > previous.last {
                    hidden_so_far += last - previous.last;
                    previous.last = last;
                }
            }
            _ => {
                out.push(Hidden { first, last, shown_before: first - hidden_so_far });
                hidden_so_far += last + 1 - first;
            }
        }
    }
    out
}

/// Which part of a window a text view is, and what it's limited to showing.
#[derive(Clone, Copy)]
pub struct Pane<'a> {
    pub number: usize,
    /// Only the active pane takes keys and scrolls to follow the cursor.
    pub active: bool,
    /// When filtering or folding, the only lines to show. This only changes the drawing.
    pub filter: Option<Shown<'a>>,
    /// These lines come from folding rather than a filter, so the map and the fold markers stay.
    pub folded: bool,
}

impl Default for Pane<'_> {
    fn default() -> Self {
        Self { number: 0, active: true, filter: None, folded: false }
    }
}

/// What the View ribbon tab can switch on and off.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct ViewOptions {
    pub ruler: bool,
    pub line_numbers: bool,
    pub active_line: bool,
    pub active_col: bool,
    pub whitespace: bool,
    pub line_endings: bool,
    pub document_map: bool,
    pub bookmark_gutter: bool,
    pub rainbow_parentheses: bool,
    /// Controls show as readable tokens rather than one small cell each.
    pub source_view: bool,
    /// Long lines continue on the next row of the screen. Only a way of drawing: the text isn't changed.
    pub word_wrap: bool,
    /// Marks every other copy of the selected text on screen.
    pub highlight_selected: bool,
    /// Shows markers beside the lines that can be folded.
    pub fold_lines: bool,
    /// Marks changed lines beside their numbers.
    pub line_change: bool,
    pub tab_size: usize,
    pub font_size: f32,
    /// Lines down the text at chosen columns, such as 72 or 80.
    pub column_markers: bool,
    pub marker_columns: MarkerColumns,
    /// The path and the function or label the cursor is in, above the text.
    pub breadcrumbs: bool,
    /// The function or label scrolled past, kept at the top of the text.
    pub sticky_scroll: bool,
    /// A call's parameter hints under its line rather than over it.
    pub hints_below: bool,
    /// The pane under the pointer takes the keyboard, without a click.
    pub focus_follows_mouse: bool,
    /// A faint line down each level of indentation, coloured by its depth if `indent_rainbow`.
    pub indent_guides: bool,
    pub indent_rainbow: bool,
    /// In a delimited file (CSV, TSV and the like), each column in a colour of its own.
    pub rainbow_csv: bool,
    /// Each line's problems written faintly at its end, as well as marked beside it.
    pub problems_at_line_end: bool,
    /// Colours written in the text (#rrggbb, rgb() and the like) shown under them, with a picker at the cursor.
    pub colour_swatches: bool,
    /// TODO, FIXME, NOTE and comments beginning with ! or ? in colours of their own.
    pub comment_kinds: bool,
    /// Comments not drawn: the code alone, the file unchanged.
    pub hide_comments: bool,
    /// A fold takes in the line closing its bracket, shown as { … } on the folded line.
    pub fold_closer: bool,
    /// Now and then, a note of the keys for a command chosen from a menu or the command box.
    pub learn_keys: bool,
    /// The ribbon in place of the menus, as Word and UltraEdit have, and whether it's folded to its tabs.
    pub ribbon: bool,
    pub ribbon_folded: bool,
    /// The start page, once every tab is closed, rather than an empty window.
    pub start_page: bool,
    /// The side panels hide themselves, coming out when the pointer reaches the window's edge.
    pub panels_hide: bool,
    /// Time spent working is kept, on this computer only.
    pub track_time: bool,
    /// Trailing spaces taken off as a file's saved (as .editorconfig's trim_trailing_whitespace does), and if
    /// `trim_only_changed`, only on the lines changed since it was opened.
    pub trim_on_save: bool,
    pub trim_only_changed: bool,
    /// How tall each row is, as a multiple of the font's own line height: 1 is as close as the font sets them.
    pub line_spacing: f32,
}

/// The line spacings offered, from as close as the font allows to twice that.
pub const LINE_SPACINGS: std::ops::RangeInclusive<f32> = 1.0..=2.0;

/// The columns that column markers are drawn after, counted from 1 as the ruler and status bar count them: a
/// marker at 80 is drawn between the 80th character and the 81st. Up to eight; a 0 is no marker.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct MarkerColumns([u16; 8]);

impl Default for MarkerColumns {
    fn default() -> Self {
        Self([80, 0, 0, 0, 0, 0, 0, 0])
    }
}

impl MarkerColumns {
    pub fn columns(&self) -> impl Iterator<Item = usize> + '_ {
        self.0.iter().filter(|c| **c > 0).map(|c| usize::from(*c))
    }

    /// Reads "72, 80" or "7 72 80": numbers from 1 to 9999 between commas or spaces, kept in order without
    /// repeats. None if anything else is there.
    pub fn parse(text: &str) -> Option<Self> {
        let mut found: Vec<u16> = Vec::new();
        for word in text.split(|c: char| c == ',' || c == ';' || c.is_whitespace()).filter(|w| !w.is_empty()) {
            let column: u16 = word.parse().ok().filter(|c| (1..=9999).contains(c))?;
            found.push(column);
        }
        found.sort_unstable();
        found.dedup();
        if found.len() > 8 {
            return None;
        }
        let mut out = [0; 8];
        out[..found.len()].copy_from_slice(&found);
        Some(Self(out))
    }
}

impl std::fmt::Display for MarkerColumns {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let words: Vec<String> = self.columns().map(|c| c.to_string()).collect();
        f.write_str(&words.join(", "))
    }
}

impl Default for ViewOptions {
    fn default() -> Self {
        Self { ruler: false, line_numbers: true, active_line: true, active_col: false, whitespace: false, line_endings: false, document_map: true, bookmark_gutter: true, rainbow_parentheses: false, source_view: true, word_wrap: false, highlight_selected: false, fold_lines: true, line_change: true, tab_size: DEFAULT_TAB_SIZE, font_size: 14.0, column_markers: false, marker_columns: MarkerColumns::default(), breadcrumbs: true, sticky_scroll: true, hints_below: false, focus_follows_mouse: false, indent_guides: true, indent_rainbow: false, rainbow_csv: true, problems_at_line_end: true, colour_swatches: true, comment_kinds: true, hide_comments: false, fold_closer: true, learn_keys: true, panels_hide: false, track_time: false, trim_on_save: false, trim_only_changed: true, line_spacing: 1.0, ribbon: false, ribbon_folded: false, start_page: true }
    }
}

impl ViewOptions {
    /// One "name=value" per line, for the settings file.
    pub fn to_settings(self) -> String {
        format!(
            "ruler={}\nline_numbers={}\nactive_line={}\nactive_col={}\nwhitespace={}\nline_endings={}\ndocument_map={}\nbookmark_gutter={}\nrainbow_parentheses={}\nsource_view={}\nword_wrap={}\nhighlight_selected={}\nfold_lines={}\nline_change={}\ntab_size={}\nfont_size={}\ncolumn_markers={}\nmarker_columns={}\nline_spacing={}\nbreadcrumbs={}\nsticky_scroll={}\nhints_below={}\nfocus_follows_mouse={}\nindent_guides={}\nindent_rainbow={}\nrainbow_csv={}\nproblems_at_line_end={}\ncolour_swatches={}\ncomment_kinds={}\nhide_comments={}\nfold_closer={}\nlearn_keys={}\npanels_hide={}\ntrack_time={}\ntrim_on_save={}\ntrim_only_changed={}\nribbon={}\nribbon_folded={}\nstart_page={}\n",
            self.ruler, self.line_numbers, self.active_line, self.active_col, self.whitespace, self.line_endings, self.document_map, self.bookmark_gutter, self.rainbow_parentheses, self.source_view, self.word_wrap, self.highlight_selected, self.fold_lines, self.line_change, self.tab_size, self.font_size, self.column_markers, self.marker_columns, self.line_spacing, self.breadcrumbs, self.sticky_scroll, self.hints_below, self.focus_follows_mouse, self.indent_guides, self.indent_rainbow, self.rainbow_csv, self.problems_at_line_end, self.colour_swatches, self.comment_kinds, self.hide_comments, self.fold_closer, self.learn_keys, self.panels_hide, self.track_time, self.trim_on_save, self.trim_only_changed, self.ribbon, self.ribbon_folded, self.start_page
        )
    }

    /// Reads the settings file; anything missing or unreadable keeps its default.
    pub fn from_settings(text: &str) -> Self {
        let mut options = Self::default();
        for line in text.lines() {
            let Some((name, value)) = line.split_once('=') else {
                continue;
            };
            let flag = value.trim().parse::<bool>().ok();
            match name.trim() {
                "ruler" => options.ruler = flag.unwrap_or(options.ruler),
                "line_numbers" => options.line_numbers = flag.unwrap_or(options.line_numbers),
                "active_line" => options.active_line = flag.unwrap_or(options.active_line),
                "active_col" => options.active_col = flag.unwrap_or(options.active_col),
                "whitespace" => options.whitespace = flag.unwrap_or(options.whitespace),
                "line_endings" => options.line_endings = flag.unwrap_or(options.line_endings),
                "document_map" => options.document_map = flag.unwrap_or(options.document_map),
                "bookmark_gutter" => options.bookmark_gutter = flag.unwrap_or(options.bookmark_gutter),
                "rainbow_parentheses" => options.rainbow_parentheses = flag.unwrap_or(options.rainbow_parentheses),
                "source_view" => options.source_view = flag.unwrap_or(options.source_view),
                "word_wrap" => options.word_wrap = flag.unwrap_or(options.word_wrap),
                "highlight_selected" => options.highlight_selected = flag.unwrap_or(options.highlight_selected),
                "line_change" => options.line_change = flag.unwrap_or(options.line_change),
                "fold_lines" => options.fold_lines = flag.unwrap_or(options.fold_lines),
                "column_markers" => options.column_markers = flag.unwrap_or(options.column_markers),
                "breadcrumbs" => options.breadcrumbs = flag.unwrap_or(options.breadcrumbs),
                "sticky_scroll" => options.sticky_scroll = flag.unwrap_or(options.sticky_scroll),
                "hints_below" => options.hints_below = flag.unwrap_or(options.hints_below),
                "focus_follows_mouse" => options.focus_follows_mouse = flag.unwrap_or(options.focus_follows_mouse),
                "indent_guides" => options.indent_guides = flag.unwrap_or(options.indent_guides),
                "indent_rainbow" => options.indent_rainbow = flag.unwrap_or(options.indent_rainbow),
                "problems_at_line_end" => options.problems_at_line_end = flag.unwrap_or(options.problems_at_line_end),
                "colour_swatches" => options.colour_swatches = flag.unwrap_or(options.colour_swatches),
                "rainbow_csv" => options.rainbow_csv = flag.unwrap_or(options.rainbow_csv),
                "comment_kinds" => options.comment_kinds = flag.unwrap_or(options.comment_kinds),
                "hide_comments" => options.hide_comments = flag.unwrap_or(options.hide_comments),
                "fold_closer" => options.fold_closer = flag.unwrap_or(options.fold_closer),
                "learn_keys" => options.learn_keys = flag.unwrap_or(options.learn_keys),
                "panels_hide" => options.panels_hide = flag.unwrap_or(options.panels_hide),
                "track_time" => options.track_time = flag.unwrap_or(options.track_time),
                "trim_on_save" => options.trim_on_save = flag.unwrap_or(options.trim_on_save),
                "trim_only_changed" => options.trim_only_changed = flag.unwrap_or(options.trim_only_changed),
                "ribbon" => options.ribbon = flag.unwrap_or(options.ribbon),
                "ribbon_folded" => options.ribbon_folded = flag.unwrap_or(options.ribbon_folded),
                "start_page" => options.start_page = flag.unwrap_or(options.start_page),
                "marker_columns" => options.marker_columns = MarkerColumns::parse(value).unwrap_or(options.marker_columns),
                "tab_size" => {
                    if let Ok(size) = value.trim().parse::<usize>()
                        && (MIN_TAB_SIZE..=MAX_TAB_SIZE).contains(&size)
                    {
                        options.tab_size = size;
                    }
                }
                "line_spacing" => {
                    if let Ok(spacing) = value.trim().parse::<f32>()
                        && LINE_SPACINGS.contains(&spacing)
                    {
                        options.line_spacing = spacing;
                    }
                }
                "font_size" => {
                    if let Ok(size) = value.trim().parse::<f32>()
                        && (8.0..=40.0).contains(&size)
                    {
                        options.font_size = size;
                    }
                }
                _ => {}
            }
        }
        options
    }
}

/// Remembers which text was last copied as a column block, so pasting it keeps its shape.
#[derive(Default)]
pub struct ClipState {
    column_text: Option<String>,
    /// What was copied, with its exact bytes, as long as the clipboard still holds the readable copy of it.
    exact: Option<(String, String)>,
}

fn normalized(text: &str) -> String {
    text.replace("\r\n", "\n")
}

impl ClipState {
    pub fn remember(&mut self, text: &str, is_block: bool) {
        self.column_text = is_block.then(|| normalized(text));
    }

    pub fn is_block(&self, text: &str) -> bool {
        self.column_text.as_deref() == Some(normalized(text).as_str())
    }

    /// Other programs get readable text; pasting back here gets the exact bytes that were copied.
    pub fn remember_exact(&mut self, readable: &str, exact: &str) {
        self.exact = (readable != exact).then(|| (normalized(readable), exact.to_string()));
    }

    pub fn exact_for(&self, pasted: &str) -> String {
        match &self.exact {
            Some((readable, exact)) if *readable == normalized(pasted) => exact.clone(),
            _ => pasted.to_string(),
        }
    }
}

use crate::scripting;

/// Scroll position of an editor, in lines and columns, kept between frames.
#[derive(Default)]
pub struct ViewState {
    /// With a filter, this counts the lines showing rather than the lines of the file.
    pub top: usize,
    /// For a Python or R script: its cells' markers are drawn with a line above and links to run them.
    pub cells: Option<crate::code_cells::Style>,
    /// For a `.http` file: the line each request's "Send request" link is drawn on, and the request's number.
    pub http_links: Vec<(usize, usize)>,
    /// For a `package.json`: the line each script is named on, and its name, for the "Debug" link beside it.
    pub script_links: Vec<(usize, String)>,
    /// The version of the text those were read from, so a file isn't read again until it changes.
    pub script_links_version: Option<u64>,
    /// The line kept at the top as the text scrolls past it (sticky scroll), set before drawing.
    pub sticky: Option<usize>,
    /// The folder the file's in, for finishing paths typed in it.
    pub folder: Option<std::path::PathBuf>,
    /// The colour at the cursor being changed with the picker: its line, where it starts, and how it's written.
    colour_picking: Option<(usize, usize, crate::view_extras::Form)>,
    left: usize,
    rows: usize,
    wheel: Vec2,
    last_activity: f64,
    rainbow: RainbowCache,
    /// Where each column's colour starts, for a delimited file.
    csv: crate::csv_colours::Cache,
    pub colours: LineColours,
    /// The display state at the top of the view, for sets where earlier bytes change later ones.
    cell_state: Option<(u64, usize, CellState)>,
    /// Under word wrap, how many rows of the top line are scrolled off the screen.
    top_sub: usize,
    /// How wide a row is under word wrap, as last drawn, for keys pressed by a macro.
    wrap_width: Option<usize>,
    /// Under word wrap, the place along a row that going up and down tries to keep to.
    wrap_goal: Option<usize>,
    /// The words offered to finish the one being typed, while the list is open.
    pub completion: Option<Completion>,
    /// For a language server: where the pointer rests over the text, and where words are wanted to finish one
    /// being typed (a line and column), for the window to ask about.
    pub hover_at: Option<(usize, usize)>,
    pub completion_wanted: Option<(usize, usize)>,
    /// In a comparison: how each line differs from the other side's, drawn behind the text and in the margin.
    pub diff: Option<std::sync::Arc<crate::diff_engine::Marks>>,
    /// The Amiga function (or MUI attribute or method) right-clicked in C or 68000 source, when the autodocs
    /// explain it: the menu offers to jump to its page.
    explain: Option<String>,
    /// The misspelt word under the pointer when the menu was opened, which the menu offers to add to the project's.
    misspelt: Option<String>,
}

/// The list of words offered to finish the one being typed.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Completion {
    pub line: usize,
    /// Where the word being finished starts.
    pub start: usize,
    pub items: Vec<String>,
    pub selected: usize,
}

/// Opens, updates or closes the list of words offered at the cursor. `asked` opens it whatever has been typed;
/// otherwise it opens once two characters of a word are typed.
pub fn offer_completions(buffer: &TextBuffer, state: &mut ViewState, asked: bool) {
    // A path being typed: the files and folders that could finish it.
    if !buffer.has_selection()
        && !buffer.has_multi()
        && let Some(folder) = state.folder.clone()
        && let Some((start, items)) = crate::path_complete::complete(&buffer.line(buffer.caret_line), buffer.caret_col, &folder)
    {
        let keep = state.completion.as_ref().and_then(|c| c.items.get(c.selected)).and_then(|was| items.iter().position(|i| i == was)).unwrap_or(0);
        state.completion = Some(Completion { line: buffer.caret_line, start, items, selected: keep });
        return;
    }
    let Some(intel) = buffer.intel.as_ref().filter(|_| !buffer.has_selection() && !buffer.has_multi()) else {
        state.completion = None;
        return;
    };
    let line = buffer.caret_line;
    let text = buffer.line(line);
    let col = buffer.caret_col;
    let Some((start, prefix)) = crate::code_intel::prefix_at(&text, col) else {
        state.completion = None;
        return;
    };
    let long_enough = asked || prefix.chars().count() >= 2 || state.completion.is_some();
    let items = if long_enough { intel.completions(line, &text, col) } else { Vec::new() };
    state.completion = (!items.is_empty()).then(|| {
        let keep = state.completion.as_ref().and_then(|c| c.items.get(c.selected)).and_then(|was| items.iter().position(|i| i == was)).unwrap_or(0);
        Completion { line, start, items, selected: keep }
    });
}

/// Puts the chosen word in place of the one being typed.
fn accept_completion(buffer: &mut TextBuffer, state: &mut ViewState) {
    let Some(completion) = state.completion.take() else {
        return;
    };
    let Some(word) = completion.items.get(completion.selected) else {
        return;
    };
    if buffer.caret_line != completion.line || buffer.caret_col < completion.start {
        return;
    }
    let end = buffer.caret_col;
    buffer.move_caret(completion.line, completion.start, false);
    buffer.move_caret(completion.line, end, true);
    buffer.type_text(word);
}

impl ViewState {
    /// A new view looking at the same place.
    pub fn same_place(&self) -> Self {
        Self { top: self.top, left: self.left, top_sub: self.top_sub, ..Self::default() }
    }

    /// The line at the top of the view.
    pub fn top(&self) -> usize {
        self.top
    }

    /// Scrolls so that a line is at the top, as a comparison keeps its two sides in step.
    pub fn set_top(&mut self, line: usize) {
        self.top = line;
        self.top_sub = 0;
    }
}

/// What a view asks the window to do through its right-click menu.
#[derive(Clone, PartialEq, Eq)]
pub enum ContextCommand {
    Cut,
    Copy,
    Paste,
    SelectAll,
    Tag(u8),
    Untag,
    Assist,
    MentionInClaude,
    /// Opens the AutoDoc reader at a function's page.
    Explain(String),
    /// A misspelt word, added to the project's own list.
    AddProjectWord(String),
    /// A script's cell, by its marker line, run in the console; or every cell above it, then it.
    RunCell(usize),
    RunCellsAbove(usize),
    /// A `.http` file's request, by its number, sent from the link above it.
    SendRequest(usize),
    /// A `package.json` script, by its name, run under the debugger from the link above it.
    DebugScript(String),
}

/// A text cell made taller for line spacing, and how far down it the text then sits.
fn spaced_cell(cell: Vec2, spacing: f32) -> (Vec2, f32) {
    let spacing = spacing.clamp(*LINE_SPACINGS.start(), *LINE_SPACINGS.end());
    let height = (cell.y * spacing).round();
    (vec2(cell.x, height), ((height - cell.y) / 2.0).floor())
}

pub(crate) fn cell_size(ui: &Ui, f: &FontId) -> Vec2 {
    ui.ctx().fonts_mut(|fonts| vec2(fonts.glyph_width(f, 'M'), fonts.row_height(f).ceil() + 2.0))
}

/// Takes the keyboard for the editor, so arrows and Tab reach it rather than moving focus.
fn hold_focus(ui: &Ui, id: Id) -> bool {
    ui.ctx().memory_mut(|m| {
        if m.focused().is_none() {
            m.request_focus(id);
        }
        m.set_focus_lock_filter(id, EventFilter { tab: true, horizontal_arrows: true, vertical_arrows: true, escape: false });
        m.has_focus(id)
    })
}

/// Alt+letter is a shortcut, not typing. AltGr characters on other layouts still get through.
fn is_typed(text: &str, modifiers: Modifiers) -> bool {
    !(modifiers.alt && text.chars().all(|c| c.is_ascii_alphanumeric()))
}

fn caret_is_on(ui: &Ui, state: &ViewState, focused: bool) -> bool {
    if !focused {
        return false;
    }
    let now = ui.input(|i| i.time);
    let elapsed_ms = ((now - state.last_activity).max(0.0) * 1000.0) as u64;
    ui.ctx().request_repaint_after(std::time::Duration::from_millis(CARET_BLINK_MS));
    (elapsed_ms / CARET_BLINK_MS).is_multiple_of(2)
}

/// Draws a scrollbar with arrows at its ends and lets it be dragged.
/// The position stays exact however long the file is.
fn scrollbar(ui: &Ui, rect: Rect, salt: &'static str, vertical: bool, position: &mut usize, visible: usize, total: usize) {
    scrollbar_with_thumb(ui, rect, salt, vertical, position, visible, visible, total);
}

/// `blocked` is how much of the end can't be scrolled to; `visible` sets how big the thumb looks.
/// Under word wrap the two differ, since a screenful may be far fewer lines than it has rows.
#[allow(clippy::too_many_arguments)]
fn scrollbar_with_thumb(ui: &Ui, rect: Rect, salt: &'static str, vertical: bool, position: &mut usize, blocked: usize, visible: usize, total: usize) {
    ui.painter().rect_filled(rect, 0.0, theme::window());
    let max = total.saturating_sub(blocked);
    *position = (*position).min(max);

    let size = BAR.min(if vertical { rect.height() } else { rect.width() } / 2.0);
    let (before, track, after) = if vertical {
        (
            Rect::from_min_size(rect.min, vec2(rect.width(), size)),
            Rect::from_min_max(rect.min + vec2(0.0, size), rect.max - vec2(0.0, size)),
            Rect::from_min_max(pos2(rect.min.x, rect.max.y - size), rect.max),
        )
    } else {
        (
            Rect::from_min_size(rect.min, vec2(size, rect.height())),
            Rect::from_min_max(rect.min + vec2(size, 0.0), rect.max - vec2(size, 0.0)),
            Rect::from_min_max(pos2(rect.max.x - size, rect.min.y), rect.max),
        )
    };
    for (arrow, is_before) in [(before, true), (after, false)] {
        let response = ui.interact(arrow, Id::unique(salt).with(is_before), Sense::click_and_drag());
        let held = response.is_pointer_button_down_on() && max > 0;
        if held {
            *position = if is_before { position.saturating_sub(1) } else { (*position + 1).min(max) };
            ui.ctx().request_repaint();
        }
        let color = if max == 0 {
            theme::text_disabled()
        } else if held || response.hovered() {
            theme::text()
        } else {
            theme::text_dim()
        };
        let c = arrow.center();
        let r = size * 0.28;
        let points = match (vertical, is_before) {
            (true, true) => vec![c + vec2(0.0, -r), c + vec2(r, r * 0.7), c + vec2(-r, r * 0.7)],
            (true, false) => vec![c + vec2(0.0, r), c + vec2(-r, -r * 0.7), c + vec2(r, -r * 0.7)],
            (false, true) => vec![c + vec2(-r, 0.0), c + vec2(r * 0.7, r), c + vec2(r * 0.7, -r)],
            (false, false) => vec![c + vec2(r, 0.0), c + vec2(-r * 0.7, -r), c + vec2(-r * 0.7, r)],
        };
        ui.painter().add(egui::Shape::convex_polygon(points, color, Stroke::NONE));
    }
    if max == 0 {
        return;
    }

    let length = if vertical { track.height() } else { track.width() } as f64;
    let thumb = ((visible as f64 / total as f64) * length).clamp(24.0_f64.min(length), length);
    let travel = (length - thumb).max(1.0);
    let response = ui.interact(track, Id::unique(salt), Sense::click_and_drag());
    if let Some(pointer) = response.interact_pointer_pos()
        && response.is_pointer_button_down_on()
    {
        let along = if vertical { pointer.y - track.min.y } else { pointer.x - track.min.x } as f64;
        let share = ((along - thumb / 2.0) / travel).clamp(0.0, 1.0);
        *position = (share * max as f64).round() as usize;
    }
    let start = (*position as f64 / max as f64) * travel;
    let shrink = 2.0;
    let thumb_rect = if vertical {
        Rect::from_min_max(pos2(track.min.x + shrink, track.min.y + start as f32), pos2(track.max.x - shrink, track.min.y + (start + thumb) as f32))
    } else {
        Rect::from_min_max(pos2(track.min.x + start as f32, track.min.y + shrink), pos2(track.min.x + (start + thumb) as f32, track.max.y - shrink))
    };
    let color = if response.hovered() || response.dragged() { theme::text_dim() } else { theme::text_faint() };
    ui.painter().rect_filled(thumb_rect, 2.0, color);
}

/// The document map: a tiny picture of the text beside the scrollbar, with the visible part marked.
/// On a huge file it shows the stretch around the view, so only those lines are read.
fn document_map(ui: &Ui, rect: Rect, salt: &'static str, buffer: &TextBuffer, state: &mut ViewState, rows: usize, shown: Option<Shown<'_>>) {
    const LINE_H: f32 = 2.0;
    const CHAR_W: f32 = 1.0;
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, 0.0, theme::map_back());
    let total = buffer.line_count();
    let map_rows = ((rect.height() / LINE_H).floor() as usize).max(1);
    // With folds, the top of the view is counted among the lines showing; the map shows the whole file.
    let top_line = shown.map_or(state.top, |lines| lines.line(state.top).unwrap_or(0));
    let map_top = if total <= map_rows {
        0
    } else {
        let scrollable = total.saturating_sub(rows).max(1) as f64;
        ((top_line as f64 / scrollable).min(1.0) * (total - map_rows) as f64) as usize
    };

    let response = ui.interact(rect, Id::unique(salt), Sense::click_and_drag());
    // Over a tagged section, the map says what the tag is called.
    if let Some(p) = response.hover_pos()
        && !response.dragged()
    {
        let line = map_top + (((p.y - rect.min.y) / LINE_H).max(0.0) as usize);
        if let Some((_, _, tag)) = buffer.tag_at(line) {
            response.clone().on_hover_text_at_pointer(crate::own_tags::name(tag));
        }
    }
    if let Some(pointer) = response.interact_pointer_pos()
        && response.is_pointer_button_down_on()
    {
        let line = map_top + (((pointer.y - rect.min.y) / LINE_H).max(0.0) as usize);
        let wanted = line.saturating_sub(rows / 2);
        state.top = shown.map_or(wanted, |lines| lines.row_of(wanted));
    }

    if top_line + rows > map_top && top_line < map_top + map_rows {
        let y1 = rect.min.y + (top_line.saturating_sub(map_top)) as f32 * LINE_H;
        let y2 = (y1 + rows as f32 * LINE_H).min(rect.max.y);
        let band = Rect::from_min_max(pos2(rect.min.x, y1), pos2(rect.max.x, y2));
        painter.rect_filled(band, 0.0, theme::map_view());
    }

    // Tagged sections run right across the map in their colours, so they can be found and clicked at a glance.
    let shown_end = map_top + map_rows;
    for (first, last, colour) in buffer.tags() {
        if last < map_top || first >= shown_end {
            continue;
        }
        let y1 = rect.min.y + (first.max(map_top) - map_top) as f32 * LINE_H;
        let y2 = (rect.min.y + ((last + 1).min(shown_end) - map_top) as f32 * LINE_H).max(y1 + 3.0);
        painter.rect_filled(Rect::from_min_max(pos2(rect.min.x, y1), pos2(rect.max.x, y2)), 0.0, crate::own_tags::colour(colour).gamma_multiply(0.6));
    }

    let columns = (rect.width() / CHAR_W).floor() as usize;
    let mut mesh = egui::Mesh::default();
    for i in 0..map_rows.min(total.saturating_sub(map_top)) {
        let line = display_text(&buffer.line(map_top + i));
        let y = rect.min.y + i as f32 * LINE_H;
        let mut run_start = None;
        let mut column = 0;
        for c in line.chars().take(columns) {
            if c == ' ' {
                if let Some(start) = run_start.take() {
                    map_run(&mut mesh, rect.min.x, y, start, column, CHAR_W);
                }
            } else if run_start.is_none() {
                run_start = Some(column);
            }
            column += 1;
        }
        if let Some(start) = run_start {
            map_run(&mut mesh, rect.min.x, y, start, column, CHAR_W);
        }
    }
    painter.add(egui::Shape::mesh(mesh));
}

/// How wide a tagged section's block is, down the left of the editor.
const TAG_BLOCK: f32 = 5.0;

/// Whether this is the last row of its line.
fn slot_is_last(slots: &[Slot], index: usize) -> bool {
    slots.get(index + 1).is_none_or(|next| next.line != slots[index].line)
}

fn map_run(mesh: &mut egui::Mesh, left: f32, y: f32, start: usize, end: usize, char_w: f32) {
    let rect = Rect::from_min_max(pos2(left + start as f32 * char_w, y), pos2(left + end as f32 * char_w, y + 1.0));
    mesh.add_colored_rect(rect, theme::map_text());
}

/// Moves a scroll position by the wheel, keeping the part too small to make a whole step.
fn apply_wheel(ui: &Ui, area: Rect, state: &mut ViewState, cell: Vec2) {
    if !ui.rect_contains_pointer(area) {
        return;
    }
    state.wheel += ui.input(|i| i.smooth_scroll_delta);
    let rows = (state.wheel.y / cell.y).trunc();
    let cols = (state.wheel.x / cell.x).trunc();
    state.wheel.y -= rows * cell.y;
    state.wheel.x -= cols * cell.x;
    state.top = state.top.saturating_add_signed(-(rows as isize));
    state.left = state.left.saturating_add_signed(-(cols as isize));
}

// ---------- Text ----------

struct TextLayout {
    font: FontId,
    cell: Vec2,
    gutter: f32,
    /// Room after the line numbers for the Git marks.
    git_space: f32,
    ruler_h: f32,
    /// How far down its row the text sits, half the room line spacing adds.
    pad: f32,
    options: ViewOptions,
    caret_on: bool,
    left: usize,
    cols: usize,
    /// The brackets open at the top of the view, when rainbow parentheses are on.
    rainbow: Option<Vec<u8>>,
    /// For a delimited file, each drawn line's runs of characters and the colour of their column.
    columns: std::collections::HashMap<usize, Vec<(usize, Color32)>>,
    /// The rows on screen, in order.
    slots: Vec<Slot>,
    mark_width: f32,
    fold_width: f32,
    diff: Option<std::sync::Arc<crate::diff_engine::Marks>>,
    diff_space: f32,
}

/// Where the rows on screen come from, for a view with no wrapping: one row per line.
fn plain_slots(top: usize, total: usize, rows: usize, left: usize, count: usize) -> Vec<Slot> {
    (top..(top + rows + 1).min(total)).map(|line| Slot { line, sub: 0, start: left, end: left + count, last: true }).collect()
}

/// Moves the cursor off a line the filter is hiding, on in the direction it was going.
fn snap_to_filter(buffer: &mut TextBuffer, lines: Shown<'_>, before: usize) {
    let line = buffer.caret_line;
    if lines.contains(line) {
        return;
    }
    let at = lines.row_of(line);
    let last = lines.len().checked_sub(1).and_then(|row| lines.line(row));
    let target = if line >= before { lines.line(at).or(last) } else { at.checked_sub(1).and_then(|row| lines.line(row)).or(lines.line(0)) };
    if let Some(target) = target {
        let extend = buffer.has_selection();
        buffer.move_caret(target, buffer.caret_col, extend);
        buffer.set_desired_col();
    }
}

#[cfg(test)]
pub fn text_view(ui: &mut Ui, buffer: &mut TextBuffer, state: &mut ViewState, clip: &mut ClipState, options: ViewOptions, input_on: bool) -> Option<ContextCommand> {
    text_pane(ui, buffer, state, clip, options, input_on, Pane::default())
}

pub fn text_pane(ui: &mut Ui, buffer: &mut TextBuffer, state: &mut ViewState, clip: &mut ClipState, options: ViewOptions, input_on: bool, pane: Pane<'_>) -> Option<ContextCommand> {
    let id = pane_id(pane.number);
    let (vbar_salt, hbar_salt, map_salt) = if pane.number == 0 { ("text-vbar", "text-hbar", "document-map") } else { ("text-vbar-2", "text-hbar-2", "document-map-2") };
    let font = FontId::monospace(options.font_size);
    let (cell, pad) = spaced_cell(cell_size(ui, &font), options.line_spacing);
    let ruler_h = if options.ruler { cell.y - 2.0 * pad + 4.0 } else { 0.0 };
    let digits = (buffer.line_count() + buffer.line_offset).to_string().len().max(4);
    let mark_width = if options.bookmark_gutter { cell.x } else { 0.0 };
    let fold_width = if options.fold_lines && (pane.filter.is_none() || pane.folded) && !buffer.is_disk_backed() { cell.x } else { 0.0 };
    // Room for the marks of what's changed since the last commit, when the file is in a Git repository.
    let git_space = if options.line_change && buffer.git_marks.is_some() { 6.0 } else { 0.0 };
    // In a comparison, a mark beside each line that differs says how, so colour isn't the only way to tell.
    let diff_space = if state.diff.is_some() { cell.x * 1.5 } else { 0.0 };
    let gutter = mark_width + fold_width + git_space + diff_space + if options.line_numbers { (digits + 1) as f32 * cell.x } else { cell.x };

    let outer = ui.available_rect_before_wrap();
    ui.allocate_rect(outer, Sense::hover());
    // An empty document has no map, and a scrollbar only appears when there's something to scroll to.
    let has_text = buffer.line_count() > 1 || !buffer.line(0).is_empty();
    // The map and word wrap both think in whole lines of the file, so they step aside while filtering.
    let map_width = if options.document_map && has_text && (pane.filter.is_none() || pane.folded) { MAP_WIDTH.min((outer.width() / 4.0).max(0.0)) } else { 0.0 };
    let wrapping = options.word_wrap && pane.filter.is_none();
    let total_lines = pane.filter.map_or(buffer.line_count(), |shown| shown.len());
    let total_cols = if wrapping { 0 } else { buffer.max_visual().max(buffer.caret_col).max(buffer.anchor_col) + 3 };
    let rows_with = |h_bar: bool| (((outer.height() - if h_bar { BAR } else { 0.0 } - ruler_h) / cell.y).floor() as usize).max(1);
    let cols_with = |v_bar: bool| (((outer.width() - map_width - if v_bar { BAR } else { 0.0 } - gutter) / cell.x).floor() as usize).max(1);
    // Under word wrap the bar is always kept, so that the row width doesn't depend on what fits.
    let mut need_v = (wrapping && has_text) || total_lines > rows_with(false);
    let need_h = total_cols > cols_with(need_v);
    need_v = need_v || (need_h && total_lines > rows_with(true));
    let need_h = need_h || total_cols > cols_with(need_v);
    let v_space = if need_v { BAR } else { 0.0 };
    let h_space = if need_h { BAR } else { 0.0 };
    let view = Rect::from_min_max(outer.min, outer.max - vec2(v_space + map_width, h_space));
    let rows = rows_with(need_h);
    let cols = cols_with(need_v);
    state.rows = rows;
    let wrap_width = wrapping.then_some(cols);
    state.wrap_width = wrap_width;

    let focused = input_on && pane.active && hold_focus(ui, id);
    if focused {
        let (before, version) = (buffer.caret_line, buffer.version());
        handle_text_input(ui, buffer, state, clip);
        // After an edit the list is out of date until the next frame, and the cursor is where it should be.
        if let Some(lines) = pane.filter
            && buffer.version() == version
        {
            snap_to_filter(buffer, lines, before);
        }
    }

    if let Some(width) = wrap_width {
        if buffer.reveal && pane.active {
            reveal_wrapped(buffer, state, rows, width);
            buffer.reveal = false;
            ui.ctx().request_repaint();
        }
        state.left = 0;
        let at = |i: usize| buffer.line(i).to_string();
        apply_wrapped_wheel(ui, Rect::from_min_max(outer.min, outer.max - vec2(v_space, 0.0)), state, cell, &at, total_lines, width);
    } else if buffer.reveal && pane.active {
        let caret_row = pane.filter.map_or(buffer.caret_line, |lines| lines.row_of(buffer.caret_line));
        if caret_row < state.top {
            state.top = caret_row;
        } else if caret_row >= state.top + rows {
            state.top = caret_row + 1 - rows;
        }
        if buffer.caret_col < state.left {
            state.left = buffer.caret_col.saturating_sub(4);
        } else if buffer.caret_col + 2 > state.left + cols {
            state.left = buffer.caret_col + 6 - cols.min(buffer.caret_col + 6);
        }
        buffer.reveal = false;
        ui.ctx().request_repaint();
    }
    if !wrapping {
        state.top_sub = 0;
        apply_wheel(ui, Rect::from_min_max(outer.min, outer.max - vec2(v_space, h_space)), state, cell);
    }

    let vbar = Rect::from_min_max(pos2(outer.max.x - BAR, outer.min.y), pos2(outer.max.x, view.max.y));
    let hbar = Rect::from_min_max(pos2(outer.min.x, view.max.y), pos2(outer.max.x - v_space, outer.max.y));
    if map_width > 0.0 {
        let map = Rect::from_min_max(pos2(view.max.x, outer.min.y), pos2(outer.max.x - v_space, view.max.y));
        document_map(ui, map, map_salt, buffer, state, rows, pane.filter);
    }
    if need_v && wrapping {
        // Dragging picks a line. The very end can be reached, since the rows may outnumber the lines.
        let before = state.top;
        scrollbar_with_thumb(ui, vbar, vbar_salt, true, &mut state.top, 1, rows.min(total_lines), total_lines);
        if state.top != before {
            state.top_sub = 0;
        }
    } else if need_v {
        scrollbar(ui, vbar, vbar_salt, true, &mut state.top, rows, total_lines);
    } else {
        state.top = 0;
        state.top_sub = 0;
    }
    if need_h {
        scrollbar(ui, hbar, hbar_salt, false, &mut state.left, cols, total_cols);
    } else {
        state.left = 0;
    }

    // Under word wrap the last screenful stays full, however far the bar or the wheel went.
    let slots = match wrap_width {
        Some(width) => {
            let at = |i: usize| buffer.line(i).to_string();
            let mut found = wrap::slots(&at, total_lines, state.top, state.top_sub, rows, width);
            if found.len() < rows && (state.top > 0 || state.top_sub > 0) {
                let short = (rows - found.len()) as isize;
                let (line, sub) = wrap::step(&at, total_lines, width, state.top, state.top_sub, -short);
                state.top = line;
                state.top_sub = sub;
                found = wrap::slots(&at, total_lines, line, sub, rows, width);
            }
            found
        }
        None => match pane.filter {
            Some(lines) => lines.rows_from(state.top, rows + 1).into_iter().map(|line| Slot { line, sub: 0, start: state.left, end: state.left + cols + 2, last: true }).collect(),
            None => plain_slots(state.top, total_lines, rows, state.left, cols + 2),
        },
    };
    let (top, left) = (state.top, state.left);
    let sense = if input_on { Sense::click_and_drag() } else { Sense::hover() };
    let response = ui.interact(view, id, sense).on_hover_cursor(CursorIcon::Text);
    // Screen readers are given the line the cursor is on, and where it is; the whole file could be far too big.
    if crate::access::wanted(&response) {
        crate::access::editor(&response, "Text", &buffer.line(buffer.caret_line.min(buffer.line_count().saturating_sub(1))), &buffer.position_text(), buffer.read_only);
    }
    let filtering = pane.filter.is_some();
    let to_position = |p: Pos2| {
        let row = ((p.y - view.min.y - ruler_h) / cell.y).floor() as isize;
        let col = ((p.x - view.min.x - gutter) / cell.x).round() as isize;
        if wrapping {
            // Below the text belongs to the last row; past a row's end belongs to that row.
            let slot = usize::try_from(row).ok().and_then(|row| slots.get(row).or(slots.last())).or(slots.first());
            return slot.map_or((top, 0), |slot| {
                let wanted = slot.start.saturating_add_signed(col);
                (slot.line, if slot.last { wanted } else { wanted.min(slot.end.saturating_sub(1)) })
            });
        }
        if filtering {
            let slot = usize::try_from(row).ok().and_then(|row| slots.get(row).or(slots.last())).or(slots.first());
            return (slot.map_or(0, |slot| slot.line), left.saturating_add_signed(col));
        }
        (top.saturating_add_signed(row), left.saturating_add_signed(col))
    };

    let (pressed, modifiers, pointer) = ui.input(|i| (i.pointer.primary_pressed(), i.modifiers, i.pointer.interact_pos()));
    let mut cell_links = state.cells.map(|style| cell_links(ui, buffer, style, &slots, view, ruler_h, cell, options.font_size)).unwrap_or_default();
    cell_links.extend(http_links(ui, &state.http_links, &slots, view, ruler_h, cell, options.font_size));
    cell_links.extend(script_links(ui, &state.script_links, &slots, view, ruler_h, cell, options.font_size));
    let mut cell_command = None;
    if let Some(p) = pointer {
        if input_on && pressed && response.hovered() && let Some(link) = cell_links.iter().find(|l| l.rect.contains(p)) {
            cell_command = Some(link.command.clone());
        } else if input_on && pressed && response.hovered() && p.x < view.min.x + mark_width && p.y >= view.min.y + ruler_h {
            // Clicking beside a line's number sets or clears its bookmark; in assembler source, its breakpoint.
            let (line, _) = to_position(p);
            match &mut buffer.code {
                Some(code) => {
                    if !code.breakpoints.remove(&line) {
                        code.breakpoints.insert(line);
                    }
                }
                None => buffer.toggle_bookmark(line),
            }
        } else if input_on && pressed && response.hovered() && fold_width > 0.0 && p.x < view.min.x + mark_width + fold_width && p.y >= view.min.y + ruler_h {
            // Clicking a marker folds or opens that line's stretch. Anywhere else in the strip does nothing.
            let (line, _) = to_position(p);
            buffer.toggle_fold(line);
            ui.ctx().request_repaint();
        } else if input_on && pressed && response.hovered() && view.contains(p) {
            response.request_focus();
            let (line, col) = to_position(p);
            state.wrap_goal = None;
            if modifiers.command && !modifiers.alt {
                buffer.add_cursor(line, col);
            } else if modifiers.alt && !wrapping {
                buffer.press_for_block(line, col, modifiers.shift);
            } else {
                if !modifiers.shift {
                    buffer.end_temporary_block();
                }
                buffer.move_caret(line, col, modifiers.shift);
                buffer.set_desired_col();
            }
            state.last_activity = ui.input(|i| i.time);
        } else if response.dragged_by(egui::PointerButton::Primary) {
            let (line, col) = to_position(p);
            buffer.move_caret(line, col, true);
            buffer.set_desired_col();
        }
    }
    if response.double_clicked() && !modifiers.command {
        buffer.select_word_at_caret();
    }
    let mut command = None;
    // A right-click on an Amiga system function's name offers its autodoc.
    if response.secondary_clicked() {
        state.explain = None;
        state.misspelt = None;
        // A misspelt word under the pointer can go into the project's own words.
        if crate::spelling::is_on()
            && crate::code_spelling::project_open()
            && let Some(p) = response.interact_pointer_pos()
            && p.x > view.min.x + gutter
        {
            let (line, col) = to_position(pos2(p.x - cell.x / 2.0, p.y));
            if line < buffer.line_count() {
                let raw = buffer.line(line);
                let coloured = state.colours.line(line).filter(|_| state.colours.is_active());
                let text = buffer.format.readable_text(&raw);
                state.misspelt = spelling_marks(buffer, state.colours.is_code(), coloured, &raw, line).into_iter().find(|&(from, to)| col >= from && col < to).map(|(from, to)| text.chars().skip(from).take(to - from).collect());
            }
        }
        if let Some(p) = response.interact_pointer_pos()
            && let Some(intel) = buffer.intel.as_ref().filter(|i| i.amiga.is_some() || i.cpu == Some(crate::assemblers::Cpu::M68k))
            && p.x > view.min.x + gutter
        {
            let (line, col) = to_position(pos2(p.x - cell.x / 2.0, p.y));
            if line < buffer.line_count()
                && let Some((_, _, word)) = crate::code_intel::word_at(&buffer.line(line), col)
                && crate::autodocs::shelf().find(&word, intel.amiga.as_deref()).is_some()
            {
                state.explain = Some(word);
            }
        }
    }
    crate::fit::context_menu(&response, |ui| command = context_menu(ui, true, state.explain.as_deref(), state.misspelt.as_deref()));

    let caret_on = caret_is_on(ui, state, focused);
    if let Some(engine) = highlight::engine() {
        let done = if filtering {
            slots.iter().all(|slot| state.colours.prepare(engine, buffer, slot.line, slot.line + 1, HIGHLIGHT_BUDGET))
        } else {
            state.colours.prepare(engine, buffer, top, top + rows + 1, HIGHLIGHT_BUDGET)
        };
        if !done {
            ui.ctx().request_repaint();
        }
    }
    let highlighting = state.colours.is_active();
    // With a language the colouring tracks brackets itself, leaving out strings and comments.
    let rainbow = (options.rainbow_parentheses && !highlighting && !filtering).then(|| state.rainbow.stack_at(buffer, top));
    let first_line = slots.first().map_or(top, |slot| slot.line);
    let needs_state = buffer.format.charset.model != charset::Model::Plain;
    let start_state = if !needs_state {
        buffer.format.tracker()
    } else if let Some((version, line, known)) = state.cell_state.filter(|(version, line, _)| *version == buffer.version() && *line == first_line) {
        let _ = (version, line);
        known
    } else {
        let found = buffer.state_at_line(first_line);
        state.cell_state = Some((buffer.version(), first_line, found));
        found
    };
    // Where the cursor is drawn, for the list of words offered to finish the one being typed.
    let caret_spot = slots.iter().enumerate().find(|(_, slot)| slot.line == buffer.caret_line && slot.holds(buffer.caret_col)).map(|(row, slot)| pos2(view.min.x + gutter + (buffer.caret_col as f32 - slot.start as f32) * cell.x, view.min.y + ruler_h + (row + 1) as f32 * cell.y));
    // Words from a language server, come since the last frame, open the list.
    if focused && buffer.intel.as_ref().is_some_and(|i| i.lsp_fresh) {
        if let Some(intel) = buffer.intel.as_mut() {
            intel.lsp_fresh = false;
        }
        offer_completions(buffer, state, true);
    }
    state.hover_at = None;
    // The block a tag has down the margin says its name.
    if let Some(p) = response.hover_pos()
        && p.x < view.min.x + TAG_BLOCK + 3.0
        && !response.dragged()
    {
        let (line, _) = to_position(pos2(p.x + cell.x, p.y));
        if let Some((_, _, tag)) = (line < buffer.line_count()).then(|| buffer.tag_at(line)).flatten() {
            response.clone().on_hover_text_at_pointer(crate::own_tags::name(tag));
        }
    }
    if buffer.intel.as_ref().is_some_and(|i| i.lsp)
        && let Some(p) = response.hover_pos()
        && p.x > view.min.x + gutter
    {
        let (line, col) = to_position(pos2(p.x - cell.x / 2.0, p.y));
        if line < buffer.line_count() {
            state.hover_at = Some((line, col));
        }
    }
    // Hovering over a label says what it is; over an instruction, what it became.
    if let Some(intel) = buffer.intel.as_ref()
        && state.completion.is_none()
        && let Some(p) = response.hover_pos()
        && p.x > view.min.x + gutter
        && !response.dragged()
    {
        let (line, col) = to_position(pos2(p.x - cell.x / 2.0, p.y));
        if line < buffer.line_count()
            && let Some(words) = intel.describe(line, &buffer.line(line), col)
        {
            response.clone().on_hover_ui_at_pointer(|ui| {
                let font = FontId::monospace(options.font_size * 0.9);
                if words.contains('\u{25A0}') {
                    ui.label(swatched(&words, font, ui.visuals().text_color()));
                } else {
                    ui.label(egui::RichText::new(words).font(font));
                }
            });
        }
    }
    let columns = match buffer.delimited {
        Some(delimiter) if options.rainbow_csv => {
            let mut lines: Vec<usize> = slots.iter().map(|slot| slot.line).collect();
            lines.dedup();
            state.csv.runs_for(buffer, delimiter, lines)
        }
        _ => Default::default(),
    };
    // The Words tools' marks: syllable counts down the right, and underlines under repeated words, filler words and adverbs.
    let word_marks = match (slots.iter().map(|s| s.line).min(), slots.iter().map(|s| s.line).max()) {
        (Some(first), Some(last)) => crate::app::words_host::marks_for(buffer, state.colours.is_code(), first, last),
        _ => Default::default(),
    };
    if !word_marks.lines.is_empty()
        && let Some(p) = response.hover_pos()
        && p.x > view.min.x + gutter
        && !response.dragged()
    {
        let (line, col) = to_position(pos2(p.x - cell.x / 2.0, p.y));
        if let Some(mark) = word_marks.lines.get(&line).and_then(|l| l.spans.iter().find(|m| col >= m.from && col < m.to)) {
            let words = crate::app::words_host::reason(mark);
            response.clone().on_hover_ui_at_pointer(|ui| {
                ui.label(words);
            });
        }
    }
    let layout = TextLayout { font: font.clone(), cell, gutter, git_space, ruler_h, pad, options, caret_on, left, cols, rainbow, columns, slots, mark_width, fold_width, diff: state.diff.clone(), diff_space };
    paint_text(ui, buffer, view, &layout, &state.colours, start_state, &word_marks);
    paint_cell_links(ui, view, &cell_links, pointer);
    // Sticky scroll: the function or label scrolled past, kept along the top; a click goes to it.
    if let Some(line) = state.sticky.filter(|line| pane.filter.is_none() && *line < state.top && *line < buffer.line_count()) {
        let strip = Rect::from_min_size(pos2(view.min.x, view.min.y + ruler_h), vec2(view.width(), cell.y));
        let painter = ui.painter_at(view);
        painter.rect_filled(strip, 0.0, theme::editor_back());
        painter.hline(strip.x_range(), strip.max.y, Stroke::new(1.0, theme::gutter_text().gamma_multiply(0.5)));
        if options.line_numbers {
            painter.text(pos2(view.min.x + gutter - cell.x, strip.min.y + pad), Align2::RIGHT_TOP, (line + 1 + buffer.line_offset).to_string(), font.clone(), theme::gutter_text());
        }
        let shown: String = display_text(&buffer.line(line)).chars().skip(state.left).take(cols).collect();
        painter.text(pos2(view.min.x + gutter, strip.min.y + pad), Align2::LEFT_TOP, shown, font.clone(), theme::editor_text());
        if ui.interact(strip, id.with("sticky"), Sense::click()).on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
            buffer.move_caret(line, 0, false);
            buffer.reveal = true;
        }
    }
    if !focused {
        state.completion = None;
    }
    // A colour written where the cursor is: a swatch of it just above, which opens a picker that writes it back.
    // While the picker's in use it keeps the keyboard from the text, so it stays open until the cursor moves on.
    if (focused || state.colour_picking.is_some()) && options.colour_swatches && !buffer.has_selection()
        && let Some(at) = caret_spot
    {
        let (line, caret) = buffer.caret_char_position();
        let text = buffer.line(line).to_string();
        let here = crate::view_extras::colours_in(&text).into_iter().find(|w| caret >= w.from && caret <= w.to);
        if state.colour_picking.is_some_and(|(l, from, _)| l != line || here.is_none_or(|w| w.from != from)) {
            state.colour_picking = None;
        }
        if let Some(written) = here {
            let column_x = |chars: usize| at.x - (caret as f32 - chars as f32) * cell.x;
            let spot = pos2(column_x(written.from), at.y - cell.y - 16.0);
            let mut picking = state.colour_picking.is_some();
            egui::Area::new(id.with("colour-swatch")).order(egui::Order::Foreground).fixed_pos(spot).show(ui.ctx(), |ui| {
                let (rect, response) = ui.allocate_exact_size(vec2(14.0, 14.0), Sense::click());
                ui.painter().rect_filled(rect, 2.0, written.colour);
                ui.painter().rect_stroke(rect, 2.0, Stroke::new(1.0, theme::editor_text()), egui::StrokeKind::Outside);
                if response.on_hover_text(crate::i18n::tr("Change this colour")).clicked() {
                    picking = !picking;
                }
            });
            if picking {
                state.colour_picking = Some((line, written.from, written.form));
                let mut colour = written.colour;
                let mut changed = false;
                egui::Area::new(id.with("colour-picker")).order(egui::Order::Foreground).fixed_pos(pos2(spot.x, spot.y + 18.0)).show(ui.ctx(), |ui| {
                    egui::Frame::popup(ui.style()).show(ui, |ui| {
                        changed = egui::color_picker::color_picker_color32(ui, &mut colour, egui::color_picker::Alpha::OnlyBlend);
                        // The full picker: other forms, the machines' colours, contrast and the screen's.
                        if ui.small_button(crate::i18n::tr("More\u{2026}")).clicked() {
                            ui.ctx().data_mut(|d| d.insert_temp(egui::Id::unique("limoniter-open-colour-picker"), true));
                        }
                    });
                });
                if changed {
                    // Written back as it was written, over the old: one undo takes it back.
                    let new = crate::view_extras::write(colour, written.form);
                    let byte = |chars: usize| text.char_indices().nth(chars).map_or(text.len(), |(b, _)| b);
                    buffer.select_match(line, byte(written.from), byte(written.to));
                    buffer.type_text(&new);
                    buffer.move_caret_to_char(line, written.from + new.chars().count().min(caret.saturating_sub(written.from)));
                }
                if ui.input(|i| i.key_pressed(Key::Escape)) {
                    state.colour_picking = None;
                }
            } else {
                state.colour_picking = None;
            }
        }
    }
    // The call being typed: what it takes, the parameter being typed in bold, just above the line.
    if focused
        && let (Some((line, words, part)), Some(at)) = (buffer.intel.as_ref().and_then(|i| i.lsp_signature.clone()), caret_spot)
        && line == buffer.caret_line
    {
        let small = FontId::monospace((options.font_size * 0.9).round());
        let mut job = egui::text::LayoutJob::default();
        let plain = egui::TextFormat { font_id: small.clone(), color: theme::text_dim(), ..Default::default() };
        let strong = egui::TextFormat { font_id: small, color: theme::text(), underline: Stroke::new(1.0, theme::text()), ..Default::default() };
        let chars: Vec<char> = words.chars().collect();
        let (a, b) = part.map_or((chars.len(), chars.len()), |(a, b)| (a.min(chars.len()), b.min(chars.len())));
        job.append(&chars[..a].iter().collect::<String>(), 0.0, plain.clone());
        job.append(&chars[a..b].iter().collect::<String>(), 0.0, strong);
        job.append(&chars[b..].iter().collect::<String>(), 0.0, plain);
        // Above the line (or below it, as chosen, and near the top).
        let above = !options.hints_below && at.y - cell.y * 2.2 > view.min.y;
        let (at, pivot) = if above { (pos2(at.x, at.y - cell.y - 2.0), egui::Align2::LEFT_BOTTOM) } else { (at, egui::Align2::LEFT_TOP) };
        if state.completion.is_none() || above {
            egui::Area::new(id.with("signature")).order(egui::Order::Foreground).fixed_pos(at).pivot(pivot).interactable(false).show(ui.ctx(), |ui| {
                egui::Frame::popup(ui.style()).show(ui, |ui| {
                    ui.label(job);
                });
            });
        }
    }
    if let (Some(completion), Some(at)) = (state.completion.clone(), caret_spot) {
        let mut chosen = None;
        // Near the bottom, the list opens above the line instead.
        let shown = completion.items.len().min(10) as f32 * (cell.y + 4.0) + 12.0;
        let (at, pivot) = if at.y + shown > view.max.y { (pos2(at.x, at.y - cell.y), egui::Align2::LEFT_BOTTOM) } else { (at, egui::Align2::LEFT_TOP) };
        egui::Area::new(id.with("completion")).order(egui::Order::Foreground).fixed_pos(at).pivot(pivot).show(ui.ctx(), |ui| {
            egui::Frame::popup(ui.style()).show(ui, |ui| {
                ui.set_min_width(cell.x * 16.0);
                egui::ScrollArea::vertical().max_height(cell.y * 10.0).show(ui, |ui| {
                    for (index, item) in completion.items.iter().enumerate() {
                        let shown = ui.selectable_label(index == completion.selected, egui::RichText::new(item).font(font.clone()));
                        if index == completion.selected {
                            shown.scroll_to_me(None);
                        }
                        if shown.clicked() {
                            chosen = Some(index);
                        }
                    }
                });
            });
        });
        if let Some(index) = chosen {
            if let Some(open) = state.completion.as_mut() {
                open.selected = index;
            }
            accept_completion(buffer, state);
        }
    }
    command.or(cell_command)
}

/// A link above a script's cell, and where it's drawn.
struct CellLink {
    rect: Rect,
    /// The row it's on, for the line drawn above the cell.
    row_top: f32,
    galley: std::sync::Arc<egui::Galley>,
    command: ContextCommand,
}

/// The links on each cell marker showing: "Run cell" and "Run above", at the right of its row.
#[allow(clippy::too_many_arguments)]
fn cell_links(ui: &Ui, buffer: &TextBuffer, style: crate::code_cells::Style, slots: &[Slot], view: Rect, ruler_h: f32, cell: Vec2, font_size: f32) -> Vec<CellLink> {
    let mut links = Vec::new();
    let font = FontId::proportional((font_size * 0.8).max(9.0));
    for (row, slot) in slots.iter().enumerate() {
        if slot.sub != 0 || slot.line >= buffer.line_count() || !crate::code_cells::is_marker(&buffer.line(slot.line), style) {
            continue;
        }
        let row_top = view.min.y + ruler_h + row as f32 * cell.y;
        if row_top + cell.y > view.max.y + 1.0 {
            break;
        }
        let mut right = view.max.x - 6.0;
        for (words, command) in [("Run above", ContextCommand::RunCellsAbove(slot.line)), ("\u{25B6} Run cell", ContextCommand::RunCell(slot.line))] {
            let galley = ui.fonts_mut(|f| f.layout_no_wrap(crate::i18n::tr(words).to_string(), font.clone(), theme::accent()));
            let size = galley.size();
            let rect = Rect::from_min_size(pos2(right - size.x, row_top + (cell.y - size.y) / 2.0), size);
            right = rect.min.x - 12.0;
            links.push(CellLink { rect, row_top, galley, command });
        }
    }
    links
}

/// A `.http` file's "Send request" links, each at the right of the line it's given (see `http_host`).
fn http_links(ui: &Ui, lines: &[(usize, usize)], slots: &[Slot], view: Rect, ruler_h: f32, cell: Vec2, font_size: f32) -> Vec<CellLink> {
    let mut links = Vec::new();
    if lines.is_empty() {
        return links;
    }
    let font = FontId::proportional((font_size * 0.8).max(9.0));
    for (row, slot) in slots.iter().enumerate().filter(|(_, s)| s.sub == 0) {
        let Some(&(_, number)) = lines.iter().find(|(line, _)| *line == slot.line) else { continue };
        let row_top = view.min.y + ruler_h + row as f32 * cell.y;
        if row_top + cell.y > view.max.y + 1.0 {
            break;
        }
        let galley = ui.fonts_mut(|f| f.layout_no_wrap(crate::i18n::tr("\u{25B6} Send request").to_string(), font.clone(), theme::accent()));
        let size = galley.size();
        let rect = Rect::from_min_size(pos2(view.max.x - 6.0 - size.x, row_top + (cell.y - size.y) / 2.0), size);
        links.push(CellLink { rect, row_top, galley, command: ContextCommand::SendRequest(number) });
    }
    links
}

/// A `package.json`'s "Debug" links, each at the right of the line a script is named on.
fn script_links(ui: &Ui, lines: &[(usize, String)], slots: &[Slot], view: Rect, ruler_h: f32, cell: Vec2, font_size: f32) -> Vec<CellLink> {
    let mut links = Vec::new();
    if lines.is_empty() {
        return links;
    }
    let font = FontId::proportional((font_size * 0.8).max(9.0));
    for (row, slot) in slots.iter().enumerate().filter(|(_, s)| s.sub == 0) {
        let Some((_, name)) = lines.iter().find(|(line, _)| *line == slot.line) else { continue };
        let row_top = view.min.y + ruler_h + row as f32 * cell.y;
        if row_top + cell.y > view.max.y + 1.0 {
            break;
        }
        let galley = ui.fonts_mut(|f| f.layout_no_wrap(crate::i18n::tr("\u{25B6} Debug").to_string(), font.clone(), theme::accent()));
        let size = galley.size();
        let rect = Rect::from_min_size(pos2(view.max.x - 6.0 - size.x, row_top + (cell.y - size.y) / 2.0), size);
        links.push(CellLink { rect, row_top, galley, command: ContextCommand::DebugScript(name.clone()) });
    }
    links
}

/// Each cell's line, across the text above its marker, and its links, underlined under the pointer.
fn paint_cell_links(ui: &Ui, view: Rect, links: &[CellLink], pointer: Option<Pos2>) {
    let painter = ui.painter_at(view);
    let mut lined = None;
    for link in links {
        if lined != Some(link.row_top) {
            painter.hline(view.x_range(), link.row_top + 0.5, Stroke::new(1.0, theme::gutter_text().gamma_multiply(0.45)));
            lined = Some(link.row_top);
        }
        painter.rect_filled(link.rect.expand(2.0), 2.0, theme::editor_back());
        painter.galley(link.rect.min, link.galley.clone(), theme::accent());
        if pointer.is_some_and(|p| link.rect.contains(p)) {
            painter.hline(link.rect.x_range(), link.rect.max.y, Stroke::new(1.0, theme::accent()));
            ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
        }
    }
}

/// The longest line looked through for colours written in it.
const MAX_SWATCH_LINE: usize = 2000;

/// Scrolls so that the cursor's row is on screen, doing nothing if it already is.
fn reveal_wrapped(buffer: &TextBuffer, state: &mut ViewState, rows: usize, width: usize) {
    let at = |i: usize| buffer.line(i).to_string();
    let total = buffer.line_count();
    let caret_sub = wrap::sub_of(&at(buffer.caret_line), width, buffer.caret_col);
    let target = (buffer.caret_line, caret_sub);
    if target < (state.top, state.top_sub) {
        (state.top, state.top_sub) = target;
        return;
    }
    let shown = if buffer.caret_line <= state.top + rows { wrap::slots(&at, total, state.top, state.top_sub, rows, width) } else { Vec::new() };
    if !shown.iter().any(|slot| (slot.line, slot.sub) == target) {
        (state.top, state.top_sub) = wrap::step(&at, total, width, target.0, target.1, -(rows.saturating_sub(1) as isize));
    }
}

/// The mouse wheel, counting rows of the screen rather than lines.
fn apply_wrapped_wheel(ui: &Ui, area: Rect, state: &mut ViewState, cell: Vec2, at: &dyn Fn(usize) -> String, total: usize, width: usize) {
    if !ui.rect_contains_pointer(area) {
        return;
    }
    state.wheel += ui.input(|i| i.smooth_scroll_delta);
    let rows = (state.wheel.y / cell.y).trunc();
    state.wheel.y -= rows * cell.y;
    state.wheel.x = 0.0;
    if rows != 0.0 {
        (state.top, state.top_sub) = wrap::step(at, total, width, state.top, state.top_sub, -(rows as isize));
    }
}

/// Under word wrap, the keys that go by rows of the screen. Returns whether it dealt with the key.
fn wrapped_key(b: &mut TextBuffer, goal: &mut Option<usize>, key: Key, m: Modifiers, rows: isize, width: usize) -> bool {
    let (key, m) = mac_keys(key, m);
    let delta = match key {
        Key::ArrowUp => -1,
        Key::ArrowDown => 1,
        Key::PageUp => -rows,
        Key::PageDown => rows,
        Key::Home | Key::End if !m.ctrl => 0,
        _ => return false,
    };
    let at = |i: usize| b.line(i).to_string();
    let total = b.line_count();
    let text = at(b.caret_line);
    let starts = wrap::row_starts(&text, width);
    let sub = wrap::sub_of(&text, width, b.caret_col);
    let row_start = starts[sub];
    if delta == 0 {
        let is_last = sub + 1 == starts.len();
        let line = b.caret_line;
        *goal = None;
        if key == Key::Home {
            if b.caret_col == row_start {
                // Already at the row's start: on to where Home usually goes.
                b.move_home(m.shift);
            } else {
                b.move_caret(line, row_start, m.shift);
                b.set_desired_col();
            }
        } else if is_last {
            b.move_end(m.shift);
        } else {
            let end = wrap::row_end_col(&text, starts[sub + 1]);
            if b.caret_col == end {
                b.move_end(m.shift);
            } else {
                b.move_caret(line, end, m.shift);
                b.set_desired_col();
            }
        }
        return true;
    }
    let along = *goal.get_or_insert(b.caret_col - row_start);
    let (line, new_sub) = wrap::step(&at, total, width, b.caret_line, sub, delta);
    let target = at(line);
    let target_starts = wrap::row_starts(&target, width);
    let start = target_starts[new_sub.min(target_starts.len() - 1)];
    let mut col = start + along;
    if let Some(next) = target_starts.get(new_sub + 1) {
        col = col.min(wrap::row_end_col(&target, *next));
    }
    b.move_caret(line, col, m.shift);
    true
}

/// In GRAPH mode, what a key types or does: a block graphic or UDG, or leaving GRAPH mode. The key's place on
/// the keyboard counts, so Shift and 1 is still 1, as Caps Shift and 1 is on the Spectrum.
enum GraphKey {
    Type(String),
    /// With whether the key also types something, as 9 does and Escape doesn't.
    Leave(bool),
}

fn graph_key(buffer: &TextBuffer, event: &Event) -> Option<GraphKey> {
    let zx = buffer.zx.as_ref().filter(|zx| zx.graph && !buffer.read_only)?;
    let Event::Key { key, physical_key, pressed: true, modifiers, .. } = event else {
        return None;
    };
    if modifiers.ctrl || modifiers.alt || modifiers.command || modifiers.mac_cmd {
        return None;
    }
    let key = physical_key.unwrap_or(*key);
    if matches!(key, Key::Num9 | Key::Escape) {
        return Some(GraphKey::Leave(key == Key::Num9));
    }
    let name = key.name();
    let c = name.chars().next().filter(|_| name.chars().count() == 1)?;
    crate::zx_graphics::graph_text(c, modifiers.shift, zx.notation).map(GraphKey::Type)
}

fn handle_text_input(ui: &Ui, buffer: &mut TextBuffer, state: &mut ViewState, clip: &mut ClipState) {
    let events = ui.input(|i| i.events.clone());
    let current_mods = ui.input(|i| i.modifiers);
    let (version, caret) = (buffer.version(), (buffer.caret_line, buffer.caret_col));
    let mut typed_word = false;
    let mut asked = false;
    // The text a GRAPH mode key would also have typed is left out, as the key has typed its graphic instead.
    let mut swallow = events.iter().filter(|event| matches!(graph_key(buffer, event), Some(GraphKey::Type(_) | GraphKey::Leave(true)))).count();
    for event in events {
        // While words are offered, the arrows choose among them, Enter or Tab puts one in, and Escape closes them.
        if let (Some(completion), Event::Key { key, pressed: true, modifiers, .. }) = (state.completion.as_mut(), &event)
            && !modifiers.ctrl
            && !modifiers.alt
        {
            let count = completion.items.len();
            match key {
                Key::ArrowDown => {
                    completion.selected = (completion.selected + 1) % count;
                    continue;
                }
                Key::ArrowUp => {
                    completion.selected = (completion.selected + count - 1) % count;
                    continue;
                }
                Key::Enter | Key::Tab => {
                    accept_completion(buffer, state);
                    state.last_activity = ui.input(|i| i.time);
                    continue;
                }
                Key::Escape => {
                    state.completion = None;
                    continue;
                }
                _ => {}
            }
        }
        if let Event::Key { key: Key::Space, pressed: true, modifiers, .. } = &event
            && modifiers.command
            && buffer.intel.is_some()
        {
            asked = true;
            continue;
        }
        if let Some(graph) = graph_key(buffer, &event) {
            match graph {
                GraphKey::Type(text) => {
                    state.wrap_goal = None;
                    scripting::note_typed(&text);
                    buffer.type_text(&text);
                }
                GraphKey::Leave(_) => {
                    if let Some(zx) = buffer.zx.as_mut() {
                        zx.graph = false;
                    }
                }
            }
            state.last_activity = ui.input(|i| i.time);
            continue;
        }
        match event {
            Event::Text(t) if swallow > 0 && t.chars().count() == 1 => swallow -= 1,
            Event::Text(t) if is_typed(&t, current_mods) => {
                state.wrap_goal = None;
                typed_word = t.chars().all(|c| c.is_alphanumeric() || matches!(c, '_' | '.' | '@' | '/' | '\\'));
                if !buffer.read_only {
                    scripting::note_typed(&t);
                    buffer.type_text(&t);
                }
            }
            Event::Ime(egui::ImeEvent::Commit(t)) => {
                if !buffer.read_only {
                    scripting::note_typed(&t);
                    buffer.type_text(&t);
                }
            }
            Event::Copy => {
                scripting::note_line("command(\"copy\");".into());
                copy_text(ui.ctx(), buffer, clip);
            }
            Event::Cut => {
                scripting::note_line("command(\"cut\");".into());
                cut_text(ui.ctx(), buffer, clip);
            }
            Event::Paste(t) => {
                if !buffer.read_only {
                    scripting::note_line("command(\"paste\");".into());
                    paste_into(ui.ctx(), buffer, clip, &crate::clipboards::pasted(&t));
                }
            }
            Event::Key { key, pressed: true, modifiers, .. } => {
                // Escape only does something to several cursors at once.
                if key != Key::Escape || buffer.has_multi() {
                    scripting::note_key(key, modifiers);
                }
                press_key(buffer, state, key, modifiers);
            }
            _ => continue,
        }
        state.last_activity = ui.input(|i| i.time);
    }
    // Typing a word, or Ctrl+Space, offers words to finish it; an edit keeps the list up to date, and moving
    // away closes it.
    if asked || typed_word {
        offer_completions(buffer, state, asked);
        if buffer.intel.as_ref().is_some_and(|i| i.lsp) {
            state.completion_wanted = Some((buffer.caret_line, buffer.caret_col));
        }
    } else if state.completion.is_some() {
        let moved = (buffer.caret_line, buffer.caret_col) != caret;
        if buffer.version() != version && buffer.caret_line == caret.0 {
            offer_completions(buffer, state, false);
        } else if moved || buffer.version() != version {
            state.completion = None;
        }
    }
}

/// A key does what it does, whether a person pressed it or a macro did.
pub fn press_key(buffer: &mut TextBuffer, state: &mut ViewState, key: Key, modifiers: Modifiers) {
    let rows = (state.rows as isize - 1).max(1);
    let wrap_width = state.wrap_width;
    let by_rows = !buffer.has_multi() && wrap_width.is_some_and(|width| wrapped_key(buffer, &mut state.wrap_goal, key, modifiers, rows, width));
    if !by_rows {
        state.wrap_goal = None;
        handle_text_key(buffer, key, modifiers, rows, wrap_width.is_some());
    }
}

/// The text of each of several selections, one to a line.
fn copy_multi(ctx: &egui::Context, buffer: &TextBuffer, clip: &mut ClipState) {
    let exact = buffer.multi_text();
    if !exact.is_empty() {
        let readable = buffer.format.readable_text(&exact);
        clip.remember(&readable, false);
        clip.remember_exact(&readable, &exact);
        crate::clipboards::put(ctx, readable);
    }
}

/// Pastes over any selection. With Copy before paste on and something to paste, what's selected goes on the clipboard,
/// which only changes when the frame ends. If it's too large to go there, nothing is lost: the paste doesn't happen.
pub fn paste_into(ctx: &egui::Context, buffer: &mut TextBuffer, clip: &mut ClipState, text: &str) {
    paste_swapping(ctx, buffer, clip, text, crate::text_buffer::edit_prefs().copy_before_paste);
}

/// Pastes, and with `swap` the selection goes on the clipboard first, as Swap with clipboard and Copy before paste do.
pub fn paste_swapping(ctx: &egui::Context, buffer: &mut TextBuffer, clip: &mut ClipState, text: &str, swap: bool) {
    // What's being pasted has to be recognised before the clipboard's record of it is replaced.
    let block = clip.is_block(text);
    let exact = clip.exact_for(text);
    if swap && !text.is_empty() {
        if buffer.has_multi() {
            copy_multi(ctx, buffer, clip);
        } else if !copy_text_selection(ctx, buffer, clip) {
            return;
        }
    }
    buffer.paste_text(&exact, block);
}

/// Copy, which with the whole-line setting takes this line if nothing is selected.
pub fn copy_text(ctx: &egui::Context, buffer: &mut TextBuffer, clip: &mut ClipState) {
    if buffer.has_multi() {
        copy_multi(ctx, buffer, clip);
        return;
    }
    let place = (buffer.caret_line, buffer.caret_col);
    let took_line = buffer.select_line_if_none();
    copy_text_selection(ctx, buffer, clip);
    if took_line {
        buffer.move_caret(place.0, place.1, false);
    }
}

/// Cut, which with the whole-line setting takes this line if nothing is selected.
pub fn cut_text(ctx: &egui::Context, buffer: &mut TextBuffer, clip: &mut ClipState) {
    if buffer.has_multi() {
        copy_multi(ctx, buffer, clip);
        if !buffer.read_only {
            buffer.delete_selected_spots();
        }
        return;
    }
    if buffer.read_only {
        copy_text(ctx, buffer, clip);
        return;
    }
    buffer.select_line_if_none();
    if copy_text_selection(ctx, buffer, clip) {
        buffer.delete_selection();
    }
}

/// Puts the selection on the clipboard. Returns false if it was too large to copy.
pub fn copy_text_selection(ctx: &egui::Context, buffer: &mut TextBuffer, clip: &mut ClipState) -> bool {
    if !buffer.has_selection() {
        return true;
    }
    if buffer.selection_too_large_to_copy() {
        buffer.notice = Some(crate::i18n::tr("That selection is too large for the clipboard. Select fewer lines to copy or cut them.").into());
        return false;
    }
    let exact = buffer.selection_text();
    let readable = buffer.format.readable_text(&exact);
    clip.remember(&readable, buffer.is_block());
    clip.remember_exact(&readable, &exact);
    crate::clipboards::put(ctx, readable);
    true
}

/// A Mac's ways of moving, turned into the ones the rest of this follows: Option and an arrow moves by a word as
/// Ctrl does elsewhere, Cmd and Left or Right goes to the start or end of the line, and Cmd and Up or Down to the
/// start or end of the file. Elsewhere keys are as they are.
pub fn mac_keys(key: Key, m: Modifiers) -> (Key, Modifiers) {
    if !cfg!(target_os = "macos") {
        return (key, m);
    }
    let plain = Modifiers { shift: m.shift, ..Modifiers::NONE };
    match key {
        Key::ArrowLeft | Key::ArrowRight | Key::Backspace | Key::Delete if m.alt && !m.mac_cmd => (key, Modifiers { ctrl: true, command: true, ..plain }),
        Key::ArrowLeft if m.mac_cmd => (Key::Home, plain),
        Key::ArrowRight if m.mac_cmd => (Key::End, plain),
        Key::ArrowUp if m.mac_cmd => (Key::Home, Modifiers { ctrl: true, command: true, ..plain }),
        Key::ArrowDown if m.mac_cmd => (Key::End, Modifiers { ctrl: true, command: true, ..plain }),
        _ => (key, m),
    }
}

/// The keys that work at every one of several cursors. Others end them, and carry on as usual.
fn multi_key(b: &mut TextBuffer, key: Key, m: Modifiers) -> bool {
    use crate::text_buffer::SpotMove;
    let (key, m) = mac_keys(key, m);
    if key == Key::Escape {
        b.end_multi();
        return true;
    }
    if m.ctrl || m.alt {
        return false;
    }
    let how = match key {
        Key::ArrowLeft => SpotMove::Left,
        Key::ArrowRight => SpotMove::Right,
        Key::Home => SpotMove::Home,
        Key::End => SpotMove::End,
        Key::ArrowUp if !m.shift => SpotMove::Up,
        Key::ArrowDown if !m.shift => SpotMove::Down,
        _ => return false,
    };
    b.multi_move(how, m.shift);
    true
}

fn handle_text_key(b: &mut TextBuffer, key: Key, m: Modifiers, rows: isize, wrapping: bool) {
    let (key, m) = mac_keys(key, m);
    if b.has_multi() && multi_key(b, key, m) {
        return;
    }
    let arrow = matches!(key, Key::ArrowLeft | Key::ArrowRight | Key::ArrowUp | Key::ArrowDown);
    // Column selection and word wrap don't go together, as in the original.
    if m.alt && m.shift && arrow && !b.is_block() && !wrapping {
        b.begin_block();
    }
    if b.read_only && matches!(key, Key::Backspace | Key::Delete | Key::Enter | Key::Tab) {
        return;
    }
    match key {
        Key::ArrowLeft => b.move_horizontal(false, m.ctrl, m.shift),
        Key::ArrowRight => b.move_horizontal(true, m.ctrl, m.shift),
        Key::ArrowUp => b.move_vertical(-1, m.shift),
        Key::ArrowDown => b.move_vertical(1, m.shift),
        Key::PageUp => b.move_vertical(-rows, m.shift),
        Key::PageDown => b.move_vertical(rows, m.shift),
        Key::Home if m.ctrl => b.move_document_start(m.shift),
        Key::Home => b.move_home(m.shift),
        Key::End if m.ctrl => b.move_document_end(m.shift),
        Key::End => b.move_end(m.shift),
        Key::Backspace => b.backspace(),
        Key::Delete if !m.shift => b.delete_forward(),
        Key::Enter => b.new_line(),
        Key::Tab => b.tab_key(m.shift),
        Key::Insert if !m.shift && !m.ctrl && !crate::text_buffer::edit_prefs().lock_insert => b.overwrite = !b.overwrite,
        _ => {}
    }
}

fn context_menu(ui: &mut Ui, text: bool, explain: Option<&str>, misspelt: Option<&str>) -> Option<ContextCommand> {
    let mut command = None;
    if let Some(word) = misspelt {
        if ui.button(crate::i18n::tf!("Add \u{201C}{word}\u{201D} to the project's words")).on_hover_text(crate::i18n::tr("Spell checking accepts it in this project from now on. The list is kept with the project, and in a folder's .limoniter/words.txt, so Git has it")).clicked() {
            command = Some(ContextCommand::AddProjectWord(word.to_string()));
        }
        ui.separator();
    }
    if let Some(word) = explain {
        if ui.button(crate::i18n::tf!("Jump to its explanation ({word})")).clicked() {
            command = Some(ContextCommand::Explain(word.to_string()));
        }
        ui.separator();
    }
    if ui.button(crate::i18n::tr("Cut")).clicked() {
        command = Some(ContextCommand::Cut);
    }
    if ui.button(crate::i18n::tr("Copy")).clicked() {
        command = Some(ContextCommand::Copy);
    }
    if ui.button(crate::i18n::tr("Paste")).clicked() {
        command = Some(ContextCommand::Paste);
    }
    ui.separator();
    if ui.button(crate::i18n::tr("Select All")).clicked() {
        command = Some(ContextCommand::SelectAll);
    }
    if text {
        ui.separator();
        crate::fit::menu_button(ui, crate::i18n::tr("Colour tag"), |ui| {
            for tag in crate::own_tags::all() {
                let swatch = egui::RichText::new(crate::i18n::tr("\u{25A0} ")).color(tag.colour);
                let mut job = egui::text::LayoutJob::default();
                swatch.append_to(&mut job, ui.style(), egui::FontSelection::Default, egui::Align::Center);
                egui::RichText::new(tag.name).append_to(&mut job, ui.style(), egui::FontSelection::Default, egui::Align::Center);
                if ui.button(job).clicked() {
                    command = Some(ContextCommand::Tag(tag.id));
                    ui.close();
                }
            }
            ui.separator();
            if ui.button(crate::i18n::tr("Remove the tag")).clicked() {
                command = Some(ContextCommand::Untag);
                ui.close();
            }
        });
        if ui.button(crate::i18n::tr("Ask the assistant\u{2026}")).clicked() {
            command = Some(ContextCommand::Assist);
        }
        // Only when a Claude Code in the terminal is there to be told.
        if crate::claude_ide::connected() && ui.button(crate::i18n::tr("Mention in Claude Code")).on_hover_text(crate::i18n::tr("Puts this file, and the lines selected, into Claude Code's prompt")).clicked() {
            command = Some(ContextCommand::MentionInClaude);
        }
    }
    command
}

/// A cell or run of cells drawn specially, instead of as plain text.
struct Token {
    column: usize,
    width: usize,
    label: String,
    style: TokenStyle,
}

#[derive(Clone, Copy, PartialEq)]
enum TokenStyle {
    Control,
    Undefined,
    State,
    /// A character drawn the wrong way round, as the machine does.
    Inverse,
    /// Part of a control sequence: its own text, in another colour.
    Sequence,
    /// No font here has this character, so its code stands in for it.
    Missing,
}

/// Works out a line's cells when it holds controls or kept bytes. The result has one character per
/// cell, with spaces where tokens go, so that selection and colouring keep working by column.
fn legacy_cells(raw: &str, format: &TextFormat, state: &mut CellState, has_glyph: &mut dyn FnMut(char) -> bool) -> (String, Vec<Token>) {
    let set = format.charset;
    let sequences = if matches!(format.display, Display::AmigaConsole | Display::AnsiArt) { console::parse_line(raw) } else { Vec::new() };
    let mut shown = String::with_capacity(raw.len());
    let mut tokens = Vec::new();
    let mut column = 0;
    state.next_row();
    for (index, c) in raw.chars().enumerate() {
        let next = next_visual(column, c);
        let width = next - column;
        let in_sequence = sequences.iter().any(|found| index >= found.start && index < found.end);
        if let Some(byte) = raw_byte(c) {
            let code = state.lookup(byte);
            let entry = set.entry(state.table_for(set, byte), code);
            match entry.filter(|entry| entry.kind == Kind::Printable).and_then(|entry| entry.unicode.map(|ch| (ch, entry.inverse))) {
                Some((ch, _)) if !has_glyph(ch) => {
                    tokens.push(Token { column, width, label: format!("{:02X}", byte), style: TokenStyle::Missing });
                    shown.extend(std::iter::repeat_n(' ', width));
                }
                Some((ch, true)) => {
                    tokens.push(Token { column, width, label: ch.to_string(), style: TokenStyle::Inverse });
                    shown.extend(std::iter::repeat_n(' ', width));
                }
                Some((ch, false)) => {
                    shown.push(ch);
                    shown.extend(std::iter::repeat_n(' ', width - 1));
                }
                None => {
                    let label = if source_view() { set.label(code).to_string() } else { format!("{byte:02X}") };
                    let label = if label.chars().count() > width && width >= 6 { format!("[0x{byte:02X}]") } else { label };
                    let style = match entry.map(|entry| entry.kind) {
                        Some(Kind::Undefined) | None => TokenStyle::Undefined,
                        Some(Kind::StateDependent) => TokenStyle::State,
                        _ => TokenStyle::Control,
                    };
                    tokens.push(Token { column, width, label, style });
                    shown.extend(std::iter::repeat_n(' ', width));
                }
            }
            state.advance(byte);
        } else if c == '\t' {
            shown.extend(std::iter::repeat_n(' ', width));
        } else if is_special(c) {
            let label = if source_view() { charset::control_label(c) } else { format!("{:02X}", c as u32) };
            tokens.push(Token { column, width, label, style: TokenStyle::Control });
            shown.extend(std::iter::repeat_n(' ', width));
        } else if in_sequence {
            tokens.push(Token { column, width, label: c.to_string(), style: TokenStyle::Sequence });
            shown.push(' ');
        } else {
            shown.push(c);
        }
        column = next;
    }
    (shown, tokens)
}

fn paint_tokens(painter: &egui::Painter, tokens: &[&Token], y: f32, cell: Vec2, font: &FontId, col_x: &dyn Fn(usize) -> f32) {
    let small = FontId::monospace((font.size * 0.72).round());
    for token in tokens.iter().copied() {
        let rect = Rect::from_min_size(pos2(col_x(token.column), y + 1.0), vec2(token.width as f32 * cell.x, cell.y - 2.0));
        if rect.max.x < painter.clip_rect().min.x || rect.min.x > painter.clip_rect().max.x {
            continue;
        }
        match token.style {
            TokenStyle::Sequence => {
                painter.text(pos2(rect.min.x, y + 1.0), Align2::LEFT_TOP, &token.label, font.clone(), theme::sequence());
            }
            TokenStyle::Inverse => {
                painter.rect_filled(rect, 0.0, theme::editor_text());
                painter.text(pos2(rect.min.x, y + 1.0), Align2::LEFT_TOP, &token.label, font.clone(), theme::editor_back());
            }
            TokenStyle::Control | TokenStyle::Undefined | TokenStyle::State | TokenStyle::Missing => {
                let back = match token.style {
                    TokenStyle::Undefined => crate::char_table::kind_color(Kind::Undefined),
                    TokenStyle::State => crate::char_table::kind_color(Kind::StateDependent),
                    _ => crate::char_table::kind_color(Kind::Control),
                };
                painter.rect_filled(rect, 2.0, back);
                let text_font = if token.width <= 1 { FontId::monospace((font.size * 0.5).round()) } else { small.clone() };
                painter.with_clip_rect(rect).text(rect.center(), Align2::CENTER_CENTER, &token.label, text_font, theme::token_text());
            }
        }
    }
}

/// Marks spaces with a dot, tabs with an arrow, and the end of a line with a pilcrow.
#[allow(clippy::too_many_arguments)]
fn paint_invisibles(painter: &egui::Painter, line: &str, has_ending: bool, y: f32, left: usize, count: usize, col_x: &dyn Fn(usize) -> f32, l: &TextLayout) {
    let cell = l.cell;
    let mid = y + cell.y / 2.0;
    let mut column = 0usize;
    for c in line.chars() {
        let next = if c == '\t' { column + tab_size() - column % tab_size() } else { column + 1 };
        if column >= left + count {
            return;
        }
        if l.options.whitespace && next > left {
            if c == ' ' {
                painter.circle_filled(pos2(col_x(column) + cell.x / 2.0, mid), 1.2, theme::whitespace());
            } else if c == '\t' {
                let (a, z) = (col_x(column) + 2.0, col_x(next) - 2.0);
                let stroke = Stroke::new(1.0, theme::whitespace());
                painter.hline(a..=z, mid, stroke);
                painter.line_segment([pos2(z, mid), pos2(z - 3.0, mid - 3.0)], stroke);
                painter.line_segment([pos2(z, mid), pos2(z - 3.0, mid + 3.0)], stroke);
            }
        }
        column = next;
    }
    if l.options.line_endings && has_ending && column >= left && column < left + count {
        let small = FontId::monospace((l.font.size * 0.85).round());
        painter.text(pos2(col_x(column) + 1.0, y + 1.0 + l.pad), Align2::LEFT_TOP, '\u{B6}', small, theme::whitespace());
    }
}

/// Turns runs of characters into runs of screen columns, allowing for tabs.
fn span_columns(raw: &str, spans: &[(usize, Color32)]) -> Vec<(usize, usize, Color32)> {
    let mut result = Vec::with_capacity(spans.len());
    let mut chars = raw.chars();
    let mut column = 0;
    for (length, colour) in spans {
        let start = column;
        for c in chars.by_ref().take(*length) {
            column = next_visual(column, c);
        }
        if column > start {
            result.push((start, column, *colour));
        }
    }
    result
}

/// Screen columns for brackets given by character position, in order.
fn bracket_columns(raw: &str, brackets: &[(usize, char, Option<usize>)]) -> Vec<(usize, char, Option<usize>)> {
    let mut result = Vec::with_capacity(brackets.len());
    let mut remaining = brackets.iter().peekable();
    let mut column = 0;
    for (index, c) in raw.chars().enumerate() {
        // Colours from before an edit can be out of step for a moment, so only real brackets are drawn.
        while remaining.peek().is_some_and(|b| b.0 < index) {
            remaining.next();
        }
        if let Some(bracket) = remaining.peek()
            && bracket.0 == index
            && bracket.1 == c
        {
            result.push((column, bracket.1, bracket.2));
        }
        column = next_visual(column, c);
    }
    result
}

/// Where each copy of some text is on a line, as screen columns.
fn matches_on_line(line: &str, needle: &str) -> Vec<(usize, usize)> {
    if needle.is_empty() {
        return Vec::new();
    }
    let mut found = Vec::new();
    let mut column = 0;
    let mut walked = 0;
    for (at, _) in line.match_indices(needle) {
        for c in line[walked..at].chars() {
            column = next_visual(column, c);
        }
        let start = column;
        for c in needle.chars() {
            column = next_visual(column, c);
        }
        walked = at + needle.len();
        found.push((start, column));
    }
    found
}

/// Places given by character, as screen columns.
fn char_spans_to_columns(line: &str, spans: &[(usize, usize)]) -> Vec<(usize, usize)> {
    let mut columns = Vec::with_capacity(line.len() + 1);
    let mut column = 0;
    for c in line.chars() {
        columns.push(column);
        column = next_visual(column, c);
    }
    columns.push(column);
    spans.iter().filter_map(|(a, z)| Some((*columns.get(*a)?, *columns.get(*z)?))).collect()
}

/// The UDGs on a Spectrum BASIC line that have pictures, by column, to draw in place of how they're written.
/// One the cursor is inside is left as text, to edit.
fn udg_marks(b: &TextBuffer, r: usize, raw: &str) -> Vec<(usize, usize, char, [u8; 8])> {
    let Some(zx) = b.zx.as_ref().filter(|zx| zx.has_pictures()) else {
        return Vec::new();
    };
    let spans = crate::zx_graphics::udg_spans(raw, zx.notation == crate::zx_graphics::Notation::Zmakebas);
    let inside = |a: usize, z: usize| r == b.caret_line && b.caret_col > a && b.caret_col < z;
    let found: Vec<(usize, usize, usize, [u8; 8])> = spans.into_iter().filter(|(a, z, _)| !inside(*a, *z)).filter_map(|(a, z, n)| Some((a, z, n, zx.udgs[n]?))).collect();
    let columns = char_spans_to_columns(raw, &found.iter().map(|(a, z, _, _)| (*a, *z)).collect::<Vec<_>>());
    columns.into_iter().zip(found).map(|((a, z), (_, _, n, rows))| (a, z, char::from(b'A' + n as u8), rows)).collect()
}

/// A UDG's picture over where it's written: its eight rows of pixels in a faint square, and its letter after.
fn paint_udg(painter: &egui::Painter, rect: Rect, rows: &[u8; 8], letter: char, active: bool) {
    painter.rect_filled(rect, 0.0, theme::editor_back());
    if active {
        painter.rect_filled(rect, 0.0, theme::active_line());
    }
    let side = (rect.height() - 2.0).min(rect.width() - 2.0).max(8.0);
    let pixel = side / 8.0;
    let origin = pos2(rect.min.x + 1.0, rect.min.y + (rect.height() - side) / 2.0);
    let square = Rect::from_min_size(origin, vec2(side, side));
    painter.rect_filled(square, 0.0, theme::whitespace().gamma_multiply(0.25));
    for (y, row) in rows.iter().enumerate() {
        for x in 0..8 {
            if row & (0x80 >> x) != 0 {
                let at = pos2(origin.x + x as f32 * pixel, origin.y + y as f32 * pixel);
                painter.rect_filled(Rect::from_min_size(at, vec2(pixel, pixel)), 0.0, theme::editor_text());
            }
        }
    }
    if rect.width() >= side + rect.height() {
        let font = FontId::monospace(rect.height() * 0.6);
        painter.text(pos2(square.max.x + 3.0, rect.center().y), Align2::LEFT_CENTER, letter, font, theme::whitespace());
    }
}

/// The wavy line under a spelling mistake.
fn squiggle(painter: &egui::Painter, from: f32, to: f32, y: f32) {
    let mut points = Vec::new();
    let mut x = from;
    let mut up = true;
    while x <= to {
        points.push(pos2(x, if up { y - 1.2 } else { y + 1.2 }));
        x += 2.0;
        up = !up;
    }
    if points.len() > 1 {
        painter.add(egui::Shape::line(points, Stroke::new(1.0, theme::mistake())));
    }
}

/// The line under a word the Words tools mark: unbroken for a repeat, dashed for filler, dotted for an adverb, so the
/// kinds can be told apart without the colours.
fn word_underline(painter: &egui::Painter, kind: crate::words_prose::Kind, from: f32, to: f32, y: f32) {
    let stroke = Stroke::new(1.5, crate::app::words_host::colour(kind));
    let (on, off): (f32, f32) = match kind {
        crate::words_prose::Kind::Repeat => (to - from, 0.0),
        crate::words_prose::Kind::Filler => (4.0, 2.5),
        crate::words_prose::Kind::Adverb => (1.5, 2.0),
    };
    let mut x = from;
    while x < to {
        painter.hline(x..=(x + on).min(to), y, stroke);
        x += on + off.max(0.01);
    }
}

/// The spelling mistakes in a line, by character. A line of prose is checked whole; in a file of code only the
/// comments and strings are, and the words in names if the person has asked for that. The mistakes are
/// remembered by the text that's checked, so a line is only sent to the checker once.
fn spelling_marks(b: &TextBuffer, is_code: bool, coloured: Option<&highlight::ColouredLine>, raw: &str, line: usize) -> Vec<(usize, usize)> {
    let text = b.format.readable_text(raw);
    if !is_code {
        // The project's own words are accepted in prose too.
        let mut marks = crate::spelling::marks_for(&text).unwrap_or_default();
        marks.retain(|&(from, to)| !crate::code_spelling::is_project_word(&text.chars().skip(from).take(to - from).collect::<String>()));
        return marks;
    }
    // The colouring says what's a comment or a string, so a line it hasn't reached yet waits for it.
    let Some(coloured) = coloured.filter(|_| text.chars().count() == raw.chars().count()) else { return Vec::new() };
    let mode = crate::code_spelling::mode();
    let regions = crate::code_spelling::Regions { comments: &coloured.comments, strings: &coloured.strings, keywords: &coloured.keywords };
    let masked = crate::code_spelling::masked_line(&text, &regions, mode, &mut |name| crate::code_spelling::is_first_use(b, line, name));
    if masked.text.is_empty() {
        return Vec::new();
    }
    crate::spelling::marks_for(&masked.text).map_or_else(Vec::new, |marks| crate::code_spelling::unmask(&masked, &marks))
}

/// The longest line whose spelling is checked as it's drawn.
const MOST_SPELLED: usize = 4000;

/// What Highlight all selected looks for: something on one line, with more to it than spaces.
fn highlight_needle(b: &TextBuffer) -> Option<String> {
    if !b.has_selection() || b.is_block() || b.caret_line != b.anchor_line {
        return None;
    }
    let text = b.selection_for_preview();
    (!text.trim().is_empty()).then_some(text)
}

/// The colour behind a line of a comparison, by how it differs.
pub(crate) fn diff_colour(kind: crate::diff_engine::ChunkKind) -> Color32 {
    match kind {
        crate::diff_engine::ChunkKind::Changed => theme::compare_changed(),
        crate::diff_engine::ChunkKind::LeftOnly => theme::compare_removed(),
        crate::diff_engine::ChunkKind::RightOnly => theme::compare_added(),
        crate::diff_engine::ChunkKind::Same => theme::editor_back(),
    }
}

/// The mark beside a line of a comparison, by how it differs: in words for a screen reader, and as a sign.
pub(crate) fn diff_sign(kind: crate::diff_engine::ChunkKind) -> &'static str {
    match kind {
        crate::diff_engine::ChunkKind::Changed => "~",
        crate::diff_engine::ChunkKind::LeftOnly => "-",
        crate::diff_engine::ChunkKind::RightOnly => "+",
        crate::diff_engine::ChunkKind::Same => " ",
    }
}

/// A comparison's marks in the margin: a sign for each line that differs, a bar beside the difference gone to,
/// and a rule where the other side has lines this one hasn't.
fn paint_diff_gutter(painter: &egui::Painter, marks: &crate::diff_engine::Marks, view: Rect, l: &TextLayout, row_y: &dyn Fn(usize) -> f32) {
    let x = view.min.x + l.mark_width + l.fold_width + 2.0;
    for (index, slot) in l.slots.iter().enumerate() {
        let y = row_y(index);
        if let Some(mark) = marks.line(slot.line)
            && slot.sub == 0
        {
            painter.rect_filled(Rect::from_min_max(pos2(x - 2.0, y), pos2(x + l.diff_space - 3.0, y + l.cell.y)), 0.0, diff_colour(mark.kind));
            painter.text(pos2(x + 1.0, y + 1.0 + l.pad), Align2::LEFT_TOP, diff_sign(mark.kind), l.font.clone(), theme::editor_text());
        }
        if marks.current.as_ref().is_some_and(|current| current.contains(&slot.line)) {
            painter.rect_filled(Rect::from_min_size(pos2(view.min.x, y), vec2(3.0, l.cell.y)), 0.0, theme::caret());
        }
        if slot.sub == 0 && marks.gap_before(slot.line) {
            painter.hline(view.min.x..=view.max.x, y, Stroke::new(2.0, theme::compare_added().lerp_to_gamma(theme::editor_text(), 0.35)));
        }
    }
    // The other side's extra lines at the very end.
    if let Some(last) = l.slots.last()
        && last.line + 1 == marks.lines.len()
        && marks.gap_before(marks.lines.len())
    {
        let y = row_y(l.slots.len());
        painter.hline(view.min.x..=view.max.x, y, Stroke::new(2.0, theme::compare_added().lerp_to_gamma(theme::editor_text(), 0.35)));
    }
}

fn paint_text(ui: &Ui, b: &TextBuffer, view: Rect, l: &TextLayout, colours: &LineColours, start_state: CellState, word_marks: &crate::app::words_host::Marks) {
    let painter = ui.painter_at(view);
    let mut cell_state = start_state;
    let (f, cell, gutter, ruler_h) = (&l.font, l.cell, l.gutter, l.ruler_h);
    painter.rect_filled(view, 0.0, theme::editor_back());

    let left = l.left;
    let col_count = l.cols + 2;
    let row_y = |index: usize| view.min.y + ruler_h + index as f32 * cell.y;
    let col_at = |row_start: usize, c: usize| view.min.x + gutter + (c as f64 - row_start as f64) as f32 * cell.x;
    let mut open_brackets = l.rainbow.clone();
    let needle = if l.options.highlight_selected { highlight_needle(b) } else { None };
    // The indentation of the last line with any, for a blank line's guides.
    let mut last_indent: Option<usize> = None;
    // The guides' step: the file's own indent, as the smallest indent of the lines in view, else a tab's width.
    let indent_step = if l.options.indent_guides { crate::view_extras::indent_step(l.slots.iter().filter(|s| s.sub == 0).map(|s| b.line(s.line)), crate::text_buffer::tab_size()) } else { 1 };

    let is_code = colours.is_code();
    let mut first = 0;
    while first < l.slots.len() {
        let r = l.slots[first].line;
        let group = l.slots[first..].iter().take_while(|slot| slot.line == r).count();
        let raw = b.line(r);
        let keep_state = b.format.charset.model != charset::Model::Plain;
        let (mut shown, tokens) = if keep_state || raw.chars().any(is_special) {
            legacy_cells(&raw, &b.format, &mut cell_state, &mut |ch| ui.ctx().fonts_mut(|fonts| fonts.has_glyph(f, ch)))
        } else {
            (display_text(&raw), Vec::new())
        };
        // A .env file's values are dots, until the line is being edited or the pointer is over it.
        if b.hide_values && r != b.caret_line && b.selection_on_row(r).is_none() && !b.has_multi() {
            let (top, bottom) = (row_y(first), row_y(first + group));
            let hovered = ui.input(|i| i.pointer.hover_pos()).is_some_and(|p| view.contains(p) && p.y >= top && p.y < bottom);
            if !hovered && shown.chars().count() == raw.chars().count() {
                shown = crate::env_file::hidden(&shown);
            }
        }
        let coloured = colours.line(r).filter(|_| colours.is_active());
        let mut brackets = Vec::new();
        if let Some(stack) = &mut open_brackets {
            scan_line(stack, &raw, |column, bracket, depth| brackets.push((column, bracket, depth)));
        } else if l.options.rainbow_parentheses
            && let Some(line) = coloured
        {
            brackets = bracket_columns(&raw, &line.brackets);
        }
        // Coloured brackets are drawn on their own, so they're left out of the line's text.
        if !brackets.is_empty() {
            let mut next = brackets.iter().map(|b| b.0).peekable();
            let without: String = shown
                .chars()
                .enumerate()
                .map(|(column, ch)| {
                    if next.peek() == Some(&column) {
                        next.next();
                        ' '
                    } else {
                        ch
                    }
                })
                .collect();
            shown = without;
        }
        let mut runs = match l.columns.get(&r) {
            Some(spans) => span_columns(&raw, spans),
            None => coloured.map_or_else(Vec::new, |line| span_columns(&raw, &line.spans)),
        };
        let comment_columns = coloured.map_or_else(Vec::new, |line| char_spans_to_columns(&raw, &line.comments));
        // Comments of a kind, and the words in them that stand out, in their own colours.
        if l.options.comment_kinds && !comment_columns.is_empty() {
            let over: Vec<(usize, usize, Color32)> = crate::view_extras::comment_kinds(&shown, &comment_columns).into_iter().map(|(a, z, tag)| (a, z, theme::tag_colour(tag))).collect();
            if !over.is_empty() {
                runs = crate::view_extras::overlay(&runs, &over);
            }
        }
        // Hidden comments aren't drawn at all.
        if l.options.hide_comments && !comment_columns.is_empty() {
            runs = crate::view_extras::overlay(&runs, &comment_columns.iter().map(|(a, z)| (*a, *z, Color32::TRANSPARENT)).collect::<Vec<_>>());
        }
        let colours_written = if l.options.colour_swatches && raw.len() <= MAX_SWATCH_LINE { crate::view_extras::colours_in(&raw) } else { Vec::new() };
        let indent = if l.options.indent_guides { crate::view_extras::indent_levels(&raw, crate::text_buffer::tab_size(), indent_step) } else { None };
        if indent.is_some() {
            last_indent = indent;
        }
        let copies = needle.as_deref().map_or_else(Vec::new, |text| matches_on_line(&raw, text));
        let mistakes = if crate::spelling::is_on() && raw.len() <= MOST_SPELLED { char_spans_to_columns(&raw, &spelling_marks(b, is_code, coloured, &raw, r)) } else { Vec::new() };
        let selected = b.selection_on_row(r);
        let spots = if b.has_multi() { b.multi_on_line(r) } else { Vec::new() };
        let tag = b.tag_at(r).map(|(_, _, colour)| crate::own_tags::colour(colour));
        let udgs = udg_marks(b, r, &raw);

        for (offset, slot) in l.slots[first..first + group].iter().enumerate() {
            let y = row_y(first + offset);
            let start = slot.start;
            let count = if slot.last { col_count } else { slot.end - slot.start };
            let col_x = |c: usize| col_at(start, c);
            let text_pos = |c: usize| pos2(col_x(c), y + 1.0 + l.pad);
            // A tagged section has a wash of its colour behind it, as well as the block in the margin.
            if let Some(colour) = tag {
                painter.rect_filled(Rect::from_min_max(pos2(view.min.x + gutter, y), pos2(view.max.x, y + cell.y)), 0.0, colour.gamma_multiply(0.09));
            }
            if let Some(mark) = l.diff.as_ref().and_then(|marks| marks.line(r)) {
                let back = diff_colour(mark.kind);
                painter.rect_filled(Rect::from_min_max(pos2(view.min.x + gutter, y), pos2(view.max.x, y + cell.y)), 0.0, back);
                let strong = back.lerp_to_gamma(theme::editor_text(), 0.22);
                for (a, z) in char_spans_to_columns(&raw, &mark.spans) {
                    let (a, z) = (a.max(start), z.min(start + count));
                    if a < z {
                        painter.rect_filled(Rect::from_min_max(pos2(col_x(a), y), pos2(col_x(z), y + cell.y)), 0.0, strong);
                    }
                }
            }
            if l.options.active_line && r == b.caret_line && !b.has_selection() {
                painter.rect_filled(Rect::from_min_max(pos2(view.min.x, y), pos2(view.max.x, y + cell.y)), 0.0, theme::active_line());
            }
            if l.options.active_col && b.caret_col >= start && b.caret_col < start + count {
                let x = col_x(b.caret_col);
                painter.rect_filled(Rect::from_min_max(pos2(x, y), pos2(x + cell.x, y + cell.y)), 0.0, theme::active_line());
            }
            // The tag the cursor's in and its partner.
            if let Some((one, other)) = &b.tag_pair {
                for tag in [one, other].into_iter().filter(|t| t.line == r) {
                    for (a, z) in char_spans_to_columns(&raw, &[(tag.start, tag.end)]) {
                        let (a, z) = (a.max(start), z.min(start + count));
                        if a < z {
                            painter.rect_filled(Rect::from_min_max(pos2(col_x(a), y), pos2(col_x(z), y + cell.y)), 2.0, theme::highlight_all());
                        }
                    }
                }
            }
            for (a, z) in &copies {
                let (a, z) = ((*a).max(start), (*z).min(start + count));
                if a < z {
                    painter.rect_filled(Rect::from_min_max(pos2(col_x(a), y), pos2(col_x(z), y + cell.y)), 0.0, theme::highlight_all());
                }
            }
            if let Some((a, z)) = selected {
                let z = if slot.last { z } else { z.min(slot.end) };
                let x1 = col_x(a.max(start)).max(view.min.x + gutter);
                let x2 = col_x(z).min(view.max.x);
                if x2 > x1 {
                    painter.rect_filled(Rect::from_min_max(pos2(x1, y), pos2(x2, y + cell.y)), 0.0, theme::selection());
                }
            }
            for (a, z, _) in &spots {
                let (a, z) = ((*a).max(start), (*z).min(start + count));
                if a < z {
                    painter.rect_filled(Rect::from_min_max(pos2(col_x(a).max(view.min.x + gutter), y), pos2(col_x(z).min(view.max.x), y + cell.y)), 0.0, theme::selection());
                }
            }
            if l.options.whitespace || l.options.line_endings {
                paint_invisibles(&painter, &raw, slot.last && r + 1 < b.line_count(), y, start, count, &col_x, l);
            }
            // Indentation guides: at each level's start, a blank line taking the level of the line before.
            if l.options.indent_guides
                && let Some(levels) = indent.or(last_indent)
            {
                for level in 0..levels {
                    let column = level * indent_step;
                    if column >= start && column < start + count {
                        let colour = if l.options.indent_rainbow { theme::rainbow()[level % theme::rainbow().len()].gamma_multiply(0.45) } else { theme::whitespace().lerp_to_gamma(theme::editor_text(), 0.15) };
                        painter.vline(col_x(column) + 0.5, y..=y + cell.y, Stroke::new(1.0, colour));
                    }
                }
            }
            let on_row: Vec<&Token> = tokens.iter().filter(|token| token.column + token.width > start && token.column < start + count).collect();
            paint_tokens(&painter, &on_row, y + l.pad, vec2(cell.x, cell.y - 2.0 * l.pad), f, &col_x);
            for (column, bracket, depth) in &brackets {
                if *column >= start && *column < start + count {
                    let color = depth.map_or(theme::mistake(), |d| theme::rainbow()[d % theme::rainbow().len()]);
                    painter.text(text_pos(*column), Align2::LEFT_TOP, *bracket, f.clone(), color);
                }
            }
            if shown.is_ascii() {
                let end = (start + count).min(shown.len());
                let mut done = start;
                for (run_start, stop, colour) in &runs {
                    let (from, to) = ((*run_start).max(start), (*stop).min(end));
                    if from < to {
                        let piece = &shown[from..to];
                        if !piece.trim().is_empty() {
                            painter.text(text_pos(from), Align2::LEFT_TOP, piece, f.clone(), *colour);
                        }
                        done = to;
                    }
                }
                // Whatever has no colour yet is drawn plain.
                if done < end {
                    painter.text(text_pos(done), Align2::LEFT_TOP, &shown[done..end], f.clone(), theme::editor_text());
                }
            } else {
                // Characters from other fonts have other widths, so each goes in its own cell.
                let mut run_list = runs.iter().peekable();
                for (c, ch) in shown.chars().enumerate().skip(start).take(count) {
                    while run_list.peek().is_some_and(|run| run.1 <= c) {
                        run_list.next();
                    }
                    if ch != ' ' {
                        let colour = run_list.peek().filter(|run| run.0 <= c).map_or(theme::editor_text(), |run| run.2);
                        painter.text(text_pos(c), Align2::LEFT_TOP, ch, f.clone(), colour);
                    }
                }
            }
            for (a, z, letter, rows) in &udgs {
                let on_row = *a >= start && *a < start + count;
                if on_row && !selected.is_some_and(|(from, to)| from < *z && to > *a) {
                    let rect = Rect::from_min_max(pos2(col_x(*a), y + l.pad), pos2(col_x(*z), y + cell.y - l.pad));
                    paint_udg(&painter, rect, rows, *letter, l.options.active_line && r == b.caret_line && !b.has_selection());
                }
            }
            for (a, z) in &mistakes {
                let (a, z) = ((*a).max(start), (*z).min(start + count));
                if a < z {
                    squiggle(&painter, col_x(a), col_x(z), y + cell.y - 1.5);
                }
            }
            // Words that repeat, filler words and adverbs, each kind underlined in its own way and colour.
            if let Some(found) = word_marks.lines.get(&r) {
                for mark in &found.spans {
                    for (a, z) in char_spans_to_columns(&raw, &[(mark.from, mark.to)]) {
                        let (a, z) = (a.max(start), z.min(start + count));
                        if a < z {
                            word_underline(&painter, mark.kind, col_x(a), col_x(z), y + cell.y - 2.0);
                        }
                    }
                }
                // The syllables of the line, at its right edge, on its last row.
                if slot.last
                    && let Some((syllables, guessed)) = found.syllables
                {
                    let said = if guessed { format!("{syllables}?") } else { syllables.to_string() };
                    // On a chip of the page's colour, so a long line running under it can still be read round it.
                    let right = view.max.x - 16.0;
                    let chip = Rect::from_min_max(pos2(right - cell.x * (said.chars().count() as f32 + 1.0), y), pos2(right + cell.x * 0.5, y + cell.y));
                    painter.rect_filled(chip, 3.0, theme::editor_back());
                    painter.text(pos2(right, y + 1.0 + l.pad), Align2::RIGHT_TOP, said, f.clone(), theme::text_faint());
                }
            }
            // Each colour written in the text, shown as a bar of itself under it.
            for written in &colours_written {
                for (a, z) in char_spans_to_columns(&raw, &[(written.from, written.to)]) {
                    let (a, z) = (a.max(start), z.min(start + count));
                    if a < z {
                        let bar = Rect::from_min_max(pos2(col_x(a), y + cell.y - 3.0), pos2(col_x(z), y + cell.y));
                        painter.rect_filled(bar, 1.0, written.colour);
                        painter.rect_stroke(bar, 1.0, Stroke::new(0.5, theme::whitespace()), egui::StrokeKind::Inside);
                    }
                }
            }
            // Every one of several cursors, on the row it falls on.
            if l.caret_on {
                for (_, _, head) in &spots {
                    if slot.holds(*head) {
                        painter.rect_filled(Rect::from_min_size(pos2(col_x(*head), y), vec2(1.5, cell.y)), 0.0, theme::caret());
                    }
                }
            }
            // A folded { … }: what's hidden, and the closing bracket it took in, after the line.
            if slot.last
                && let Some(last) = b.folds().get(&r)
                && let closing = b.line(*last)
                && crate::text_buffer::closer_of(&closing).is_some()
            {
                let end = col_x(display_text(&raw).trim_end().chars().count());
                painter.text(pos2(end + cell.x * 0.5, y + 1.0 + l.pad), Align2::LEFT_TOP, format!("\u{2026} {}", closing.trim()), f.clone(), theme::text_faint());
            }
            // A line standing for a folded stretch has a rule under it.
            if slot.last && l.fold_width > 0.0 && b.is_folded(r) {
                painter.hline(view.min.x..=view.max.x, y + cell.y - 1.0, Stroke::new(1.0, theme::whitespace()));
            }
        }
        first += group;
    }

    if l.caret_on {
        let (from, to) = if b.is_block() {
            (b.caret_line.min(b.anchor_line), b.caret_line.max(b.anchor_line))
        } else {
            (b.caret_line, b.caret_line)
        };
        for (index, slot) in l.slots.iter().enumerate() {
            if slot.line >= from && slot.line <= to && slot.holds(b.caret_col) {
                let x = col_at(slot.start, b.caret_col);
                let width = if b.overwrite { cell.x } else { 1.5 };
                let color = if b.overwrite { theme::overwrite_caret() } else { theme::caret() };
                painter.rect_filled(Rect::from_min_size(pos2(x, row_y(index)), vec2(width, cell.y)), 0.0, color);
            }
        }
    }

    // A language server's hints for each line (types, parameters' names), faint, after its text.
    if let Some(intel) = b.intel.as_deref().filter(|i| !i.lsp_hints.is_empty()) {
        let small = FontId::monospace((f.size * 0.85).round());
        for (index, slot) in l.slots.iter().enumerate() {
            // On the line's last row.
            if l.slots.get(index + 1).is_some_and(|next| next.line == slot.line) {
                continue;
            }
            let hints: Vec<&str> = intel.lsp_hints.iter().filter(|(line, _)| *line == slot.line).map(|(_, h)| h.as_str()).collect();
            if hints.is_empty() {
                continue;
            }
            let end = col_at(slot.start, display_text(&b.line(slot.line)).trim_end().chars().count());
            painter.text(pos2(end + cell.x * 2.0, row_y(index) + 1.0 + l.pad), Align2::LEFT_TOP, hints.join("  "), small.clone(), theme::text_faint());
        }
    }

    // Each line's problems written after it, faint, in the problem's colour (as Error Lens does).
    if l.options.problems_at_line_end {
        let small = FontId::monospace((f.size * 0.85).round());
        for (index, slot) in l.slots.iter().enumerate() {
            if l.slots.get(index + 1).is_some_and(|next| next.line == slot.line) {
                continue;
            }
            let from_code = b.code.as_ref().and_then(|c| c.problem_at(slot.line)).map(|p| p.to_string());
            let from_intel = b.intel.as_ref().and_then(|i| i.diagnostics.iter().chain(&i.build_problems).find(|(line, _)| *line == slot.line).map(|(_, words)| words.clone()));
            let Some(words) = from_code.or(from_intel) else { continue };
            let hints = b.intel.as_ref().map_or(0, |i| i.lsp_hints.iter().filter(|(line, _)| *line == slot.line).map(|(_, h)| h.chars().count() + 2).sum::<usize>());
            let end = col_at(slot.start, display_text(&b.line(slot.line)).trim_end().chars().count() + hints);
            let words: String = words.lines().next().unwrap_or("").chars().take(160).collect();
            painter.text(pos2(end + cell.x * 3.0, row_y(index) + 1.0 + l.pad), Align2::LEFT_TOP, format!("\u{25CF} {words}"), small.clone(), theme::problem_mark().gamma_multiply(0.85));
        }
    }

    // In 6502, Z80, 6809 and 68000 source, each line's cycles at the right, where the line leaves room.
    if let (Some(intel), Some(top), Some(bottom)) = (b.intel.as_deref(), l.slots.first(), l.slots.last()) {
        let notes = crate::m6502_timing::line_notes(intel, b.version(), top.line, bottom.line);
        let small = FontId::monospace((f.size * 0.85).round());
        for (index, slot) in l.slots.iter().enumerate().filter(|(_, s)| s.sub == 0) {
            if let Some(text) = notes.get(&slot.line) {
                let right = view.max.x - cell.x;
                let end = col_at(slot.start, display_text(&b.line(slot.line)).trim_end().chars().count());
                if end + cell.x * (text.len() as f32 + 1.0) < right {
                    painter.text(pos2(right, row_y(index) + 1.0 + l.pad), Align2::RIGHT_TOP, text, small.clone(), theme::gutter_text());
                }
            }
        }
    }

    // The line numbers and ruler stay put while the text scrolls under them.
    let gutter_rect = Rect::from_min_max(view.min, pos2(view.min.x + gutter, view.max.y));
    painter.rect_filled(gutter_rect, 0.0, theme::gutter_back());
    if let Some(marks) = &l.diff {
        paint_diff_gutter(&painter, marks, view, l, &row_y);
    }
    for (index, slot) in l.slots.iter().enumerate() {
        // A tagged section's block runs down the left edge, through all of its rows.
        if let Some((first, last, colour)) = b.tag_at(slot.line) {
            let top = row_y(index) + if slot.line == first && slot.sub == 0 { 1.0 } else { 0.0 };
            let bottom = row_y(index) + cell.y - if slot.line == last && slot_is_last(&l.slots, index) { 1.0 } else { 0.0 };
            painter.rect_filled(Rect::from_min_max(pos2(view.min.x, top), pos2(view.min.x + TAG_BLOCK, bottom)), 0.0, crate::own_tags::colour(colour));
        }
        // Its change since the last commit, beside that: added, changed, or lines taken out above it.
        if l.options.line_change
            && let Some(Some(mark)) = b.git_marks.as_ref().and_then(|marks| marks.get(slot.line))
        {
            let x = view.min.x + gutter - cell.x * 0.75 - l.git_space + 2.0;
            match mark {
                crate::git::Mark::DeletedAbove if slot.sub == 0 => {
                    let top = row_y(index);
                    painter.add(egui::Shape::convex_polygon(vec![pos2(x, top - 3.0), pos2(x + 5.0, top), pos2(x, top + 3.0)], theme::git_deleted(), Stroke::NONE));
                }
                crate::git::Mark::DeletedAbove => {}
                added_or_changed => {
                    let colour = if *added_or_changed == crate::git::Mark::Added { theme::git_added() } else { theme::git_changed() };
                    painter.rect_filled(Rect::from_min_size(pos2(x, row_y(index)), vec2(3.0, cell.y)), 0.0, colour);
                }
            }
        }
        // A changed line's mark runs down all of its rows.
        if l.options.line_change
            && let Some(saved) = b.change_at(slot.line)
        {
            let mark = Rect::from_min_size(pos2(view.min.x + gutter - 5.0, row_y(index)), vec2(3.0, cell.y));
            painter.rect_filled(mark, 0.0, if saved { theme::change_saved() } else { theme::change_unsaved() });
        }
        // A line's number and bookmark go on its first row only.
        if slot.sub != 0 {
            continue;
        }
        if l.fold_width > 0.0 && (b.is_folded(slot.line) || b.can_fold(slot.line)) {
            let middle = pos2(view.min.x + l.mark_width + l.fold_width / 2.0, row_y(index) + cell.y / 2.0);
            let size = (l.fold_width * 0.34).min(cell.y * 0.3);
            let stroke = Stroke::new(1.0, theme::gutter_text());
            painter.rect_stroke(Rect::from_center_size(middle, vec2(size * 2.0, size * 2.0)), 1.0, stroke, egui::StrokeKind::Inside);
            painter.hline((middle.x - size * 0.55)..=(middle.x + size * 0.55), middle.y, stroke);
            if b.is_folded(slot.line) {
                painter.vline(middle.x, (middle.y - size * 0.55)..=(middle.y + size * 0.55), stroke);
            }
        }
        // A language server's problems, marked as the assembler's are.
        if b.code.as_ref().is_none_or(|c| c.problem_at(slot.line).is_none())
            && let Some(intel) = b.intel.as_ref().filter(|i| i.diagnostics.iter().chain(&i.build_problems).any(|(l, _)| *l == slot.line))
        {
            let _ = intel;
            let row = Rect::from_min_max(pos2(view.min.x + gutter, row_y(index)), pos2(view.max.x, row_y(index) + cell.y));
            let middle = pos2(view.min.x + cell.x.max(8.0) / 2.0, row_y(index) + cell.y / 2.0);
            painter.rect_filled(row, 0.0, theme::problem_line());
            if slot.sub == 0 {
                painter.add(egui::Shape::convex_polygon(vec![pos2(middle.x, middle.y - 4.5), pos2(middle.x + 4.5, middle.y + 3.5), pos2(middle.x - 4.5, middle.y + 3.5)], theme::problem_mark(), Stroke::NONE));
            }
        }
        if let Some(code) = &b.code {
            let row = Rect::from_min_max(pos2(view.min.x + gutter, row_y(index)), pos2(view.max.x, row_y(index) + cell.y));
            let middle = pos2(view.min.x + cell.x.max(8.0) / 2.0, row_y(index) + cell.y / 2.0);
            if code.problem_at(slot.line).is_some() {
                painter.rect_filled(row, 0.0, theme::problem_line());
                if slot.sub == 0 {
                    painter.add(egui::Shape::convex_polygon(vec![pos2(middle.x, middle.y - 4.5), pos2(middle.x + 4.5, middle.y + 3.5), pos2(middle.x - 4.5, middle.y + 3.5)], theme::problem_mark(), Stroke::NONE));
                }
            }
            if code.stopped == Some(slot.line) {
                painter.rect_filled(row, 0.0, theme::stopped_line());
            }
            if slot.sub == 0 && code.breakpoints.contains(&slot.line) {
                painter.circle_filled(middle, (cell.y * 0.3).min(5.5), theme::breakpoint());
            }
            if slot.sub == 0 && code.stopped == Some(slot.line) {
                let r = (cell.y * 0.32).min(6.0);
                painter.add(egui::Shape::convex_polygon(vec![pos2(middle.x - r, middle.y - r), pos2(middle.x + r, middle.y), pos2(middle.x - r, middle.y + r)], theme::stopped_mark(), Stroke::new(1.0, theme::gutter_back())));
            }
        }
        if l.options.bookmark_gutter && b.is_bookmarked(slot.line) {
            let mark = Rect::from_min_size(pos2(view.min.x + 1.0, row_y(index) + 2.0), vec2(cell.x - 2.0, cell.y - 4.0));
            painter.rect_filled(mark, 2.0, crate::panels::BOOKMARK_COLOR);
        }
        if l.options.line_numbers {
            let right = view.min.x + gutter - cell.x * 0.75 - l.git_space;
            painter.text(pos2(right, row_y(index) + 1.0 + l.pad), Align2::RIGHT_TOP, slot.line + 1 + b.line_offset, f.clone(), theme::gutter_text());
        }
    }

    if l.options.column_markers && !l.options.word_wrap {
        let stroke = Stroke::new(1.0, theme::ruler_text().gamma_multiply(0.45));
        let text_top = view.min.y + ruler_h;
        for column in l.options.marker_columns.columns() {
            if column >= left && column <= left + col_count {
                painter.vline(col_at(left, column).round() + 0.5, text_top..=view.max.y, stroke);
            }
        }
    }

    if !l.options.ruler {
        return;
    }
    let strip = Rect::from_min_max(view.min, pos2(view.max.x, view.min.y + ruler_h));
    painter.rect_filled(strip, 0.0, theme::ruler_back());
    let tick = Stroke::new(1.0, theme::ruler_text());
    let bottom = strip.max.y - 1.0;
    let ruler_font = FontId::proportional((f.size * 0.82).round());
    for c in left..left + col_count {
        let x = col_at(left, c);
        if c.is_multiple_of(10) {
            painter.vline(x, (bottom - 6.0)..=bottom, tick);
            painter.text(pos2(x, strip.min.y), Align2::CENTER_TOP, c, ruler_font.clone(), theme::ruler_text());
        } else if c.is_multiple_of(5) {
            painter.vline(x, (bottom - 5.0)..=bottom, tick);
        } else {
            painter.vline(x, (bottom - 3.0)..=bottom, tick);
        }
    }
    // Under word wrap the ruler counts along the row the cursor is on.
    let caret_row_start = if l.options.word_wrap { l.slots.iter().find(|slot| slot.line == b.caret_line && slot.holds(b.caret_col)).map(|slot| slot.start) } else { Some(left) };
    if let Some(row_start) = caret_row_start.filter(|row_start| b.caret_col >= *row_start) {
        let marker = col_at(row_start, b.caret_col);
        painter.rect_filled(Rect::from_min_size(pos2(marker - 1.0, strip.min.y), vec2(2.0, ruler_h)), 0.0, theme::ruler_mark());
    }
    painter.hline(strip.x_range(), bottom, Stroke::new(1.0, theme::strong_line()));
}

// ---------- Hex ----------

/// What the character pane shows for a byte; None for one it shows as a dot.
pub fn hex_pane_char(b: &HexBuffer, format: &TextFormat, value: u8) -> Option<char> {
    if b.ebcdic {
        return crate::ebcdic::printable(value);
    }
    // Otherwise the pane shows the document's own set, not the host's.
    format.charset.entry(format.state, value).filter(|entry| entry.kind == Kind::Printable).and_then(|entry| entry.unicode).or_else(|| (format.charset.holds_characters() && (32..127).contains(&value)).then_some(value as char))
}

/// The hex view. `fields`, when the Structure panel is showing, colours each field's bytes and names the field under
/// the pointer.
#[allow(clippy::too_many_arguments)]
pub fn hex_view(ui: &mut Ui, buffer: &mut HexBuffer, state: &mut ViewState, format: &TextFormat, options: ViewOptions, input_on: bool, fields: Option<&crate::structure::Tree>) -> Option<ContextCommand> {
    let font = FontId::monospace(options.font_size);
    let cell = cell_size(ui, &font);
    let header = cell.y + 4.0;
    crate::hex_buffer::set_offset_width(buffer.len());

    let outer = ui.available_rect_before_wrap();
    ui.allocate_rect(outer, Sense::hover());
    let view = Rect::from_min_max(outer.min, outer.max - vec2(BAR, BAR));
    let rows = (((view.height() - header) / cell.y).floor() as usize).max(1);
    let cols = ((view.width() / cell.x).floor() as usize).max(1);
    state.rows = rows;

    let focused = input_on && hold_focus(ui, editor_id());
    if focused {
        handle_hex_input(ui, buffer, state);
    }
    let caret_row = buffer.caret / bytes_per_row();
    if buffer.reveal {
        if caret_row < state.top {
            state.top = caret_row;
        } else if caret_row >= state.top + rows {
            state.top = caret_row + 1 - rows;
        }
        buffer.reveal = false;
        ui.ctx().request_repaint();
    }
    apply_wheel(ui, view, state, cell);

    let total_rows = buffer.len() / bytes_per_row() + 1;
    let total_cols = ascii_cell(bytes_per_row()) + 1;
    let vbar = Rect::from_min_max(pos2(view.max.x, outer.min.y), pos2(outer.max.x, view.max.y));
    let hbar = Rect::from_min_max(pos2(outer.min.x, view.max.y), pos2(view.max.x, outer.max.y));
    scrollbar(ui, vbar, "hex-vbar", true, &mut state.top, rows, total_rows);
    scrollbar(ui, hbar, "hex-hbar", false, &mut state.left, cols, total_cols);

    let (top, left) = (state.top, state.left);
    let sense = if input_on { Sense::click_and_drag() } else { Sense::hover() };
    let response = ui.interact(view, editor_id(), sense).on_hover_cursor(CursorIcon::Text);
    if crate::access::wanted(&response) {
        let pane = if buffer.ascii_pane { crate::i18n::tr("Hex, typing characters") } else { crate::i18n::tr("Hex, typing hex digits") };
        crate::access::editor(&response, pane, &buffer.position_text(), "", buffer.read_only);
    }
    let (pressed, modifiers, pointer) = ui.input(|i| (i.pointer.primary_pressed(), i.modifiers, i.pointer.interact_pos()));
    if let Some(p) = pointer {
        let hit = hex_hit(p, view, top, left, cell, header, buffer.len());
        if input_on && pressed && response.hovered() && view.contains(p) {
            response.request_focus();
            buffer.ascii_pane = hit.ascii;
            buffer.move_caret(hit.position, modifiers.shift);
            if !modifiers.shift {
                buffer.nibble = hit.nibble;
            }
            state.last_activity = ui.input(|i| i.time);
        } else if response.dragged_by(egui::PointerButton::Primary) {
            buffer.move_caret(hit.position, true);
        }
    }
    if let Some(tree) = fields
        && let Some(p) = response.hover_pos()
        && !response.dragged()
    {
        let hit = hex_hit(p, view, top, left, cell, header, buffer.len());
        if hit.position < buffer.len()
            && let Some(words) = tree.describe(hit.position as u64, &*buffer)
        {
            response.clone().on_hover_ui_at_pointer(|ui| {
                ui.label(egui::RichText::new(words).size(options.font_size * 0.9));
            });
        }
    }
    let mut command = None;
    crate::fit::context_menu(&response, |ui| command = context_menu(ui, false, None, None));
    let caret_on = caret_is_on(ui, state, focused);
    paint_hex(ui, buffer, view, top, left, rows, cell, header, caret_on, &font, format, fields);
    command
}

struct HexHit {
    position: usize,
    ascii: bool,
    nibble: u8,
}

fn hex_hit(p: Pos2, view: Rect, top: usize, left: usize, cell: Vec2, header: f32, len: usize) -> HexHit {
    let row = top.saturating_add_signed(((p.y - view.min.y - header) / cell.y).floor() as isize);
    let x = (p.x - view.min.x) / cell.x + left as f32;
    let ascii = x >= (hex_cell(bytes_per_row()) + ASCII_GAP_CELLS / 2) as f32;
    let (col, mut nibble) = if ascii {
        (((x - ascii_cell(0) as f32).floor().max(0.0) as usize).min(bytes_per_row() - 1), 0)
    } else {
        let col = (0..bytes_per_row()).rev().find(|i| x >= hex_cell(*i) as f32).unwrap_or(0);
        (col, u8::from(x - hex_cell(col) as f32 >= 1.0))
    };
    let position = row.saturating_mul(bytes_per_row()).saturating_add(col).min(len);
    if position == len {
        nibble = 0;
    }
    HexHit { position, ascii, nibble }
}

fn handle_hex_input(ui: &Ui, b: &mut HexBuffer, state: &mut ViewState) {
    let events = ui.input(|i| i.events.clone());
    let current_mods = ui.input(|i| i.modifiers);
    let rows = (state.rows as isize - 1).max(1);
    for event in events {
        match event {
            Event::Text(_) | Event::Paste(_) if b.read_only => {}
            Event::Text(t) if is_typed(&t, current_mods) => {
                for c in t.chars() {
                    if b.ascii_pane {
                        b.type_char(c);
                    } else if let Some(d) = c.to_digit(16) {
                        b.type_hex_digit(d as u8);
                    }
                }
            }
            Event::Copy => copy_hex_selection(ui, b),
            Event::Cut => {
                copy_hex_selection(ui, b);
                if !b.read_only {
                    b.delete_selection();
                }
            }
            Event::Paste(t) => paste_into_hex(ui, b, &crate::clipboards::pasted(&t)),
            Event::Key { key, pressed: true, modifiers, .. } => handle_hex_key(b, key, modifiers, rows),
            _ => continue,
        }
        state.last_activity = ui.input(|i| i.time);
    }
}

/// Pastes as bytes, with Copy before paste doing what it does for text.
pub fn paste_into_hex(ui: &Ui, b: &mut HexBuffer, text: &str) {
    if crate::text_buffer::edit_prefs().copy_before_paste && !text.is_empty() {
        copy_hex_selection(ui, b);
    }
    b.paste_text(text);
}

pub fn copy_hex_selection(ui: &Ui, b: &HexBuffer) {
    if b.has_selection() {
        crate::clipboards::put(ui.ctx(), b.selection_text());
    }
}

fn handle_hex_key(b: &mut HexBuffer, key: Key, m: Modifiers, rows: isize) {
    let (key, m) = mac_keys(key, m);
    let row = bytes_per_row() as isize;
    if b.read_only && matches!(key, Key::Backspace | Key::Delete) {
        return;
    }
    match key {
        Key::ArrowLeft => b.move_left(m.shift),
        Key::ArrowRight => b.move_by(1, m.shift),
        Key::ArrowUp => b.move_by(-row, m.shift),
        Key::ArrowDown => b.move_by(row, m.shift),
        Key::PageUp => b.move_by(-rows * row, m.shift),
        Key::PageDown => b.move_by(rows * row, m.shift),
        Key::Home if m.ctrl => b.move_caret(0, m.shift),
        Key::Home => b.move_caret(b.caret - b.caret % bytes_per_row(), m.shift),
        Key::End if m.ctrl => b.move_caret(b.len(), m.shift),
        Key::End => b.move_caret(b.caret - b.caret % bytes_per_row() + bytes_per_row() - 1, m.shift),
        Key::Tab => {
            b.ascii_pane = !b.ascii_pane;
            b.nibble = 0;
        }
        Key::Backspace => b.backspace(),
        Key::Delete if !m.shift => b.delete_forward(),
        Key::Insert if !m.shift && !m.ctrl && !crate::text_buffer::edit_prefs().lock_insert => b.insert_mode = !b.insert_mode,
        _ => {}
    }
}

#[allow(clippy::too_many_arguments)]
fn paint_hex(ui: &Ui, b: &HexBuffer, view: Rect, top: usize, left: usize, rows: usize, cell: Vec2, header: f32, caret_on: bool, f: &FontId, format: &TextFormat, fields: Option<&crate::structure::Tree>) {
    let painter = ui.painter_at(view);
    painter.rect_filled(view, 0.0, theme::editor_back());

    let row_y = |r: usize| view.min.y + header + (r - top) as f32 * cell.y;
    let cell_x = |c: usize| view.min.x + (c as f32 - left as f32) * cell.x;
    let last_row = (top + rows + 1).min(b.len() / bytes_per_row() + 1);
    let (sel_start, sel_end) = (b.caret.min(b.anchor), b.caret.max(b.anchor));

    let divider = cell_x(hex_cell(bytes_per_row())) + cell.x / 2.0;
    painter.vline(divider, view.y_range(), Stroke::new(1.0, theme::strong_line()));

    for r in top..last_row {
        let y = row_y(r);
        let start = r * bytes_per_row();
        painter.text(pos2(cell_x(0), y + 1.0), Align2::LEFT_TOP, crate::hex_buffer::offset_text(start), f.clone(), theme::gutter_text());
        for i in 0..bytes_per_row() {
            let p = start + i;
            if p >= b.len() {
                break;
            }
            let value = b.byte(p);
            let (hx, ax) = (cell_x(hex_cell(i)), cell_x(ascii_cell(i)));
            let hex_rect = Rect::from_min_size(pos2(hx, y), vec2(cell.x * 2.0, cell.y));
            let ascii_rect = Rect::from_min_size(pos2(ax, y), vec2(cell.x, cell.y));
            if let Some(tree) = fields {
                crate::structure_panel::paint_byte(&painter, tree, p, hex_rect, ascii_rect);
            }
            if p >= sel_start && p < sel_end {
                painter.rect_filled(hex_rect, 0.0, theme::selection());
                painter.rect_filled(ascii_rect, 0.0, theme::selection());
            } else if p == b.caret {
                painter.rect_filled(if b.ascii_pane { hex_rect } else { ascii_rect }, 0.0, theme::other_pane());
            }
            let hex_color = if value == 0 { theme::zero_byte() } else { theme::editor_text() };
            painter.text(pos2(hx, y + 1.0), Align2::LEFT_TOP, format!("{value:02X}"), f.clone(), hex_color);
            let (shown, color) = match hex_pane_char(b, format, value) {
                Some(ch) => (ch, theme::editor_text()),
                None => ('.', theme::zero_byte()),
            };
            painter.text(pos2(ax, y + 1.0), Align2::LEFT_TOP, shown, f.clone(), color);
        }
    }

    let caret_row = b.caret / bytes_per_row();
    if caret_on && caret_row >= top && caret_row < last_row {
        let col = b.caret % bytes_per_row();
        let x = if b.ascii_pane { cell_x(ascii_cell(col)) } else { cell_x(hex_cell(col) + usize::from(b.nibble)) };
        let y = row_y(caret_row);
        if b.insert_mode {
            painter.rect_filled(Rect::from_min_size(pos2(x, y), vec2(1.5, cell.y)), 0.0, theme::caret());
        } else {
            painter.rect_filled(Rect::from_min_size(pos2(x, y + cell.y - 2.0), vec2(cell.x, 2.0)), 0.0, theme::caret());
        }
    }

    let strip = Rect::from_min_max(view.min, pos2(view.max.x, view.min.y + header));
    painter.rect_filled(strip, 0.0, theme::ruler_back());
    painter.text(pos2(cell_x(0), strip.min.y + 2.0), Align2::LEFT_TOP, "Offset", f.clone(), theme::ruler_text());
    for i in 0..bytes_per_row() {
        painter.text(pos2(cell_x(hex_cell(i)), strip.min.y + 2.0), Align2::LEFT_TOP, format!("{i:02X}"), f.clone(), theme::ruler_text());
        painter.text(pos2(cell_x(ascii_cell(i)), strip.min.y + 2.0), Align2::LEFT_TOP, format!("{:X}", i % 16), f.clone(), theme::ruler_text());
    }
    painter.hline(strip.x_range(), strip.max.y - 1.0, Stroke::new(1.0, theme::strong_line()));
}

#[cfg(test)]
mod tests {
    use super::*;
    use egui::{MouseWheelUnit, PointerButton, RawInput, TouchPhase};

    const WINDOW_SIZE: Vec2 = vec2(1200.0, 800.0);

    /// Runs the text view without a window, feeding it exact mouse and keyboard events.
    struct Bench {
        ctx: egui::Context,
        time: f64,
        buffer: TextBuffer,
        state: ViewState,
        clip: ClipState,
        options: ViewOptions,
        input_on: bool,
        filter: Option<Vec<usize>>,
        folded: bool,
        number: usize,
        active: bool,
    }

    impl Bench {
        /// Across the fold markers, which come just after the bookmarks.
        fn state_marker_x(&self) -> f32 {
            let cell = self.ctx.fonts_mut(|fonts| fonts.glyph_width(&FontId::monospace(self.options.font_size), 'M'));
            cell * 1.5
        }

        fn first_row_y(&self) -> f32 {
            let height = self.ctx.fonts_mut(|fonts| fonts.row_height(&FontId::monospace(self.options.font_size))).ceil() + 2.0;
            height + 4.0 + height / 2.0
        }

        fn new(lines: usize) -> Self {
            let mut buffer = TextBuffer::default();
            buffer.set_content((0..lines).map(|i| format!("line number {i}\twith a tab")).collect());
            Self {
                ctx: egui::Context::default(),
                time: 0.0,
                buffer,
                state: ViewState::default(),
                clip: ClipState::default(),
                // The click positions here allow for the ruler, which is off by default now.
                options: ViewOptions { ruler: true, ..ViewOptions::default() },
                input_on: true,
                filter: None,
                folded: false,
                number: 0,
                active: true,
            }
        }

        fn frame(&mut self, events: Vec<Event>) {
            self.time += 1.0 / 60.0;
            let raw = RawInput {
                screen_rect: Some(Rect::from_min_size(Pos2::ZERO, WINDOW_SIZE)),
                time: Some(self.time),
                predicted_dt: 1.0 / 60.0,
                events,
                ..Default::default()
            };
            let (buffer, state, clip, options, input_on) = (&mut self.buffer, &mut self.state, &mut self.clip, self.options, self.input_on);
            let pane = Pane { number: self.number, active: self.active, filter: self.filter.as_deref().map(Shown::Lines), folded: self.folded };
            let mut output = self.ctx.run_ui(raw, |ui| {
                egui::CentralPanel::default().frame(egui::Frame::NONE).show(ui, |ui| {
                    text_pane(ui, buffer, state, clip, options, input_on, pane);
                });
            });
            // There's no screen here, so the drawing that would go to it is discarded.
            output.textures_delta.clear();
        }

        fn settle(&mut self) {
            for _ in 0..90 {
                self.frame(Vec::new());
            }
        }

        fn wheel(&mut self, at: Pos2, lines: f32) {
            self.frame(vec![Event::PointerMoved(at)]);
            self.frame(vec![Event::MouseWheel {
                unit: MouseWheelUnit::Line,
                delta: vec2(0.0, lines),
                phase: TouchPhase::Move,
                source: egui::MouseWheelSource::Unknown,
                modifiers: Modifiers::NONE,
            }]);
            self.settle();
        }

        fn press_and_release(&mut self, at: Pos2, hold_frames: usize) {
            self.frame(vec![Event::PointerMoved(at)]);
            self.frame(vec![Event::PointerButton { pos: at, button: PointerButton::Primary, pressed: true, modifiers: Modifiers::NONE }]);
            for _ in 0..hold_frames {
                self.frame(Vec::new());
            }
            self.frame(vec![Event::PointerButton { pos: at, button: PointerButton::Primary, pressed: false, modifiers: Modifiers::NONE }]);
            self.frame(Vec::new());
        }
    }

    fn folded_bench() -> Bench {
        let mut bench = Bench::new(0);
        bench.buffer.set_content((0..30).flat_map(|i| [format!("block {i}"), format!("    inside {i}"), format!("    more {i}")]).collect());
        bench
    }

    #[test]
    fn the_keyboard_steps_over_folded_lines() {
        let mut bench = folded_bench();
        bench.buffer.toggle_fold(0);
        bench.filter = Some(bench.buffer.unfolded_lines());
        bench.folded = true;
        bench.settle();
        bench.frame(vec![key(Key::ArrowDown)]);
        assert_eq!(bench.buffer.caret_line, 3);
        bench.frame(vec![key(Key::ArrowUp)]);
        assert_eq!(bench.buffer.caret_line, 0);
    }

    #[test]
    fn typing_at_a_fold_line_isnt_moved_by_the_old_list() {
        let mut bench = folded_bench();
        bench.buffer.toggle_fold(3);
        bench.filter = Some(bench.buffer.unfolded_lines());
        bench.folded = true;
        bench.buffer.move_caret(2, 0, false);
        bench.settle();
        bench.frame(vec![key(Key::Enter)]);
        assert_eq!(bench.buffer.caret_line, 3, "the cursor stays on the new line");
    }

    #[test]
    fn clicking_a_fold_marker_folds_and_clicking_again_opens() {
        let mut bench = folded_bench();
        bench.settle();
        let marker_x = bench.state_marker_x();
        // The first line of text, just under the ruler.
        bench.press_and_release(pos2(marker_x, bench.first_row_y()), 1);
        assert!(bench.buffer.is_folded(0));
        assert_eq!(bench.buffer.caret_line, 0, "the click doesn't move the cursor anywhere else");
        bench.press_and_release(pos2(marker_x, bench.first_row_y()), 1);
        assert!(!bench.buffer.is_folded(0));
    }

    #[test]
    fn with_fold_lines_off_that_strip_is_for_text() {
        let mut bench = folded_bench();
        bench.options.fold_lines = false;
        bench.settle();
        let marker_x = bench.state_marker_x();
        bench.press_and_release(pos2(marker_x, bench.first_row_y()), 1);
        assert!(!bench.buffer.has_folds());
    }

    #[test]
    fn several_selections_draw_in_every_view_option() {
        let mut bench = Bench::new(50);
        bench.buffer.move_caret(0, 0, false);
        bench.buffer.move_caret(0, 4, true);
        bench.buffer.make_selections().unwrap();
        bench.settle();
        bench.options.word_wrap = true;
        bench.settle();
        assert_eq!(bench.buffer.multi_count(), 50);
    }

    #[test]
    fn typing_goes_to_every_cursor_and_escape_ends_them() {
        let mut bench = Bench::new(3);
        bench.buffer.add_cursor(2, 0);
        bench.settle();
        assert_eq!(bench.buffer.multi_count(), 2);
        bench.frame(vec![Event::Text("#".into())]);
        assert!(bench.buffer.line(0).starts_with('#'));
        assert!(bench.buffer.line(2).starts_with('#'));
        bench.frame(vec![key(Key::ArrowRight)]);
        assert_eq!(bench.buffer.multi_count(), 2, "moving keeps them");
        bench.frame(vec![key(Key::Escape)]);
        assert!(!bench.buffer.has_multi());
    }

    #[test]
    fn a_filter_shows_only_its_lines_and_clicks_land_on_them() {
        let mut bench = Bench::new(1000);
        bench.filter = Some(vec![10, 20, 30, 40, 900]);
        bench.settle();
        bench.press_and_release(pos2(300.0, 200.0), 1);
        assert!([10, 20, 30, 40, 900].contains(&bench.buffer.caret_line), "landed on line {}", bench.buffer.caret_line);
        bench.press_and_release(pos2(300.0, 700.0), 1);
        assert_eq!(bench.buffer.caret_line, 900, "below the lines belongs to the last one");
    }

    #[test]
    fn arrow_keys_step_over_what_the_filter_hides() {
        let mut bench = Bench::new(100);
        bench.filter = Some(vec![5, 50, 60]);
        bench.buffer.move_caret(5, 0, false);
        bench.settle();
        bench.frame(vec![key(Key::ArrowDown)]);
        assert_eq!(bench.buffer.caret_line, 50);
        bench.frame(vec![key(Key::ArrowDown)]);
        assert_eq!(bench.buffer.caret_line, 60);
        bench.frame(vec![key(Key::ArrowDown)]);
        assert_eq!(bench.buffer.caret_line, 60, "the last one showing is as far as it goes");
        bench.frame(vec![key(Key::ArrowUp)]);
        assert_eq!(bench.buffer.caret_line, 50);
        bench.frame(vec![key(Key::ArrowUp)]);
        assert_eq!(bench.buffer.caret_line, 5);
        bench.frame(vec![key(Key::ArrowUp)]);
        assert_eq!(bench.buffer.caret_line, 5);
    }

    #[test]
    fn a_long_filter_scrolls_by_what_is_showing() {
        let mut bench = Bench::new(50_000);
        bench.filter = Some((0..50_000).step_by(2).collect());
        bench.settle();
        bench.wheel(pos2(300.0, 300.0), -10.0);
        assert!(bench.state.top > 0, "the wheel scrolls");
        assert!(bench.state.top < 25_000);
        bench.buffer.move_caret(40_000, 0, false);
        bench.settle();
        assert_eq!(bench.state.top + bench.state.rows, 20_001, "the cursor's line is kept on screen, counted among those showing");
    }

    #[test]
    fn filtering_steps_word_wrap_and_the_map_aside_without_turning_them_off() {
        let mut bench = wrapped_bench(vec!["x".repeat(5000), "y".repeat(5000), "z".repeat(5000)]);
        bench.options.document_map = true;
        bench.filter = Some(vec![0, 2]);
        bench.settle();
        assert!(bench.options.word_wrap && bench.options.document_map);
        assert_eq!(bench.buffer.line_count(), 3);
    }

    #[test]
    fn only_the_active_pane_takes_keys_and_follows_the_cursor() {
        let mut bench = Bench::new(1000);
        bench.active = false;
        bench.number = 1;
        bench.settle();
        bench.frame(vec![Event::Text("typed".into())]);
        assert_eq!(&*bench.buffer.line(0), "line number 0\twith a tab", "the other pane has the keyboard");
        bench.buffer.move_caret(900, 0, false);
        bench.settle();
        assert_eq!(bench.state.top, 0, "and only that pane scrolls to the cursor");
        assert!(bench.buffer.reveal, "it's left for that pane to do");
        bench.active = true;
        bench.settle();
        assert!(bench.state.top > 0);
        assert!(!bench.buffer.reveal);
        bench.frame(vec![Event::Text("x".into())]);
        assert!(bench.buffer.line(900).starts_with('x'));
    }

    #[test]
    fn an_empty_document_has_no_map_or_scrollbars_to_click() {
        let mut b = Bench::new(1);
        b.buffer.set_content(vec![String::new()]);
        b.settle();
        // Where the map and the scrollbar would be is plain editing space, so a click lands in the text.
        b.press_and_release(pos2(WINDOW_SIZE.x - 3.0, 200.0), 0);
        assert_eq!(b.state.top, 0);
        b.press_and_release(pos2(WINDOW_SIZE.x - BAR - 30.0, WINDOW_SIZE.y - 3.0), 0);
        assert_eq!(b.state.left, 0);
    }

    #[test]
    fn marker_columns_are_read_in_order_without_repeats() {
        assert_eq!(MarkerColumns::parse("80 72, 7;72").unwrap().to_string(), "7, 72, 80");
        assert_eq!(MarkerColumns::parse("").unwrap().columns().count(), 0);
        assert!(MarkerColumns::parse("0").is_none() && MarkerColumns::parse("10000").is_none() && MarkerColumns::parse("72a").is_none());
        assert!(MarkerColumns::parse("1 2 3 4 5 6 7 8 9").is_none());
        assert_eq!(MarkerColumns::default().to_string(), "80");
    }

    #[test]
    fn column_markers_draw_with_and_without_the_ruler() {
        let mut b = Bench::new(50);
        b.options.column_markers = true;
        b.options.marker_columns = MarkerColumns::parse("1, 72, 5000").unwrap();
        b.settle();
        b.options.ruler = true;
        b.settle();
    }

    #[test]
    fn line_spacing_makes_rows_taller_and_clicks_still_land_on_their_line() {
        let mut b = Bench::new(50);
        b.options.ruler = false;
        b.options.line_spacing = 2.0;
        b.settle();
        let base = b.ctx.fonts_mut(|fonts| fonts.row_height(&FontId::monospace(b.options.font_size))).ceil() + 2.0;
        let (cell, pad) = spaced_cell(vec2(8.0, base), 2.0);
        assert_eq!(cell.y, (base * 2.0).round());
        assert!(pad > 0.0 && pad * 2.0 <= cell.y - base);
        b.press_and_release(pos2(300.0, cell.y * 3.5), 0);
        assert_eq!(b.buffer.caret_line, 3);
        assert_eq!(spaced_cell(vec2(8.0, 20.0), 9.0).0.y, 40.0, "out of range is held to the range");
    }

    #[test]
    fn view_settings_are_remembered_and_bad_ones_are_ignored() {
        let options = ViewOptions { whitespace: true, document_map: false, rainbow_parentheses: true, source_view: false, tab_size: 3, font_size: 18.0, column_markers: true, marker_columns: MarkerColumns::parse("7, 72").unwrap(), line_spacing: 1.5, hints_below: true, sticky_scroll: false, focus_follows_mouse: true, indent_guides: false, indent_rainbow: true, problems_at_line_end: false, colour_swatches: false, comment_kinds: false, hide_comments: true, fold_closer: false, learn_keys: false, panels_hide: true, track_time: true, trim_on_save: true, trim_only_changed: false, ribbon: true, ribbon_folded: true, start_page: false, ..ViewOptions::default() };
        assert_eq!(ViewOptions::from_settings(&options.to_settings()), options);
        let odd = ViewOptions::from_settings("ruler=maybe\nfont_size=900\ntab_size=0\nnonsense\nwhitespace=true\nmarker_columns=72,x\nline_spacing=7");
        assert_eq!(odd, ViewOptions { whitespace: true, ..ViewOptions::default() });
    }

    #[test]
    fn the_mouse_wheel_scrolls_both_ways_and_leaves_the_caret_alone() {
        let mut b = Bench::new(5000);
        b.settle();
        assert_eq!(b.state.top, 0);
        b.wheel(pos2(400.0, 400.0), -5.0);
        let down = b.state.top;
        assert!(down > 0, "wheel down should scroll down");
        assert_eq!(b.buffer.caret_line, 0);
        b.wheel(pos2(400.0, 400.0), 2.0);
        assert!(b.state.top < down, "wheel up should scroll back up");
        assert!(b.state.top > 0);
        b.wheel(pos2(400.0, 400.0), 500.0);
        assert_eq!(b.state.top, 0, "it stops at the top");
    }

    #[test]
    fn the_wheel_stops_at_the_end_and_works_over_the_document_map() {
        let mut b = Bench::new(100);
        b.settle();
        b.wheel(pos2(WINDOW_SIZE.x - BAR - 20.0, 400.0), -5000.0);
        assert_eq!(b.state.top, 100 - b.state.rows);
    }

    #[test]
    fn a_click_after_scrolling_lands_on_the_line_under_the_pointer() {
        let mut b = Bench::new(5000);
        b.settle();
        b.wheel(pos2(400.0, 400.0), -5.0);
        let top = b.state.top;
        let cell = b.ctx.fonts_mut(|f| vec2(f.glyph_width(&FontId::monospace(14.0), 'M'), f.row_height(&FontId::monospace(14.0)).ceil() + 2.0));
        let ruler = cell.y + 4.0;
        b.press_and_release(pos2(300.0, ruler + cell.y * 3.5), 0);
        assert_eq!(b.buffer.caret_line, top + 3);
    }

    #[test]
    fn the_scrollbar_drags_and_its_arrows_step() {
        let mut b = Bench::new(100_000);
        b.settle();
        let x = WINDOW_SIZE.x - BAR / 2.0;
        b.press_and_release(pos2(x, 400.0), 1);
        let dragged = b.state.top;
        assert!(dragged > 30_000 && dragged < 70_000, "the middle of the track is about the middle of the file: {dragged}");

        let before = b.state.top;
        // These lines are short, so there's no bar along the bottom and the arrow sits in the corner.
        b.press_and_release(pos2(x, WINDOW_SIZE.y - BAR / 2.0), 3);
        assert!(b.state.top > before && b.state.top < before + 20, "the down arrow steps: {before} to {}", b.state.top);
        let before = b.state.top;
        b.press_and_release(pos2(x, BAR / 2.0), 3);
        assert!(b.state.top < before, "the up arrow steps back");
    }

    #[test]
    fn clicking_the_document_map_jumps_there_without_moving_the_caret() {
        let mut b = Bench::new(300);
        b.settle();
        let x = WINDOW_SIZE.x - BAR - MAP_WIDTH / 2.0;
        b.press_and_release(pos2(x, 400.0), 0);
        assert!(b.state.top > 100, "line 200 is 400 points down the map: {}", b.state.top);
        assert_eq!(b.buffer.caret_line, 0);
    }

    #[test]
    fn turning_off_the_map_gives_the_text_its_space() {
        let mut b = Bench::new(50);
        b.options.document_map = false;
        b.settle();
        let x = WINDOW_SIZE.x - BAR - 30.0;
        b.press_and_release(pos2(x, 200.0), 0);
        assert_ne!(b.buffer.caret_line, 0, "that spot is text now, not the map");
    }

    #[test]
    fn every_view_option_draws_tabs_spaces_and_foreign_text() {
        let mut b = Bench::new(5);
        b.buffer.set_content(vec!["\ta b\t\u{e9}\u{4e2d}\u{1F600}".to_string(), String::new(), "x".repeat(5000), "end".to_string()]);
        for (whitespace, endings, map, ruler, numbers, column) in [
            (true, true, true, true, true, true),
            (true, false, false, false, false, false),
            (false, true, true, false, true, true),
        ] {
            b.options.whitespace = whitespace;
            b.options.line_endings = endings;
            b.options.document_map = map;
            b.options.ruler = ruler;
            b.options.line_numbers = numbers;
            b.options.active_col = column;
            b.frame(Vec::new());
            b.buffer.move_caret(2, 4000, false);
            b.settle();
            assert!(b.state.left > 0, "the view follows the caret across a long line");
            b.buffer.move_caret(0, 0, false);
            b.settle();
        }
    }

    #[test]
    fn rainbow_parentheses_draw_without_disturbing_the_text() {
        let mut b = Bench::new(5);
        let lines = vec!["f(a[1], {\u{e9}})".to_string(), "\t) unmatched".to_string(), "(".repeat(50), "x".repeat(3000) + ")"];
        b.buffer.set_content(lines.clone());
        b.options.rainbow_parentheses = true;
        b.settle();
        b.buffer.move_caret(3, 2900, false);
        b.settle();
        let now: Vec<String> = b.buffer.all_lines().iter().map(|l| l.to_string()).collect();
        assert_eq!(now, lines, "colouring never changes the document");
    }

    #[test]
    fn colour_runs_become_screen_columns_allowing_for_tabs() {
        let red = Color32::RED;
        let blue = Color32::BLUE;
        assert_eq!(span_columns("\tab", &[(1, red), (2, blue)]), [(0, 8, red), (8, 10, blue)]);
        assert_eq!(span_columns("ab", &[(5, red), (1, blue)]), [(0, 2, red)], "colours from before an edit are clamped");
        assert_eq!(bracket_columns("\t(a)", &[(1, '(', Some(0)), (3, ')', Some(0))]), [(8, '(', Some(0)), (10, ')', Some(0))]);
        assert!(bracket_columns("xy", &[(1, '(', Some(0))]).is_empty(), "a bracket that's no longer there isn't drawn");
    }

    fn cells_for(raw: &str, format: &TextFormat) -> (String, Vec<Token>) {
        crate::text_buffer::set_source_view(true);
        crate::text_buffer::set_cell_profile(format.charset);
        let mut state = format.tracker();
        legacy_cells(raw, format, &mut state, &mut |_| true)
    }

    #[test]
    fn typing_a_word_offers_ways_to_finish_it_and_enter_puts_one_in() {
        use crate::assemblers::Cpu;
        let source = ["  org 32768", "counter: db 0", "  ld a,(counter)", ""];
        let mut bench = Bench::new(1);
        bench.buffer.set_content(source.iter().map(|l| l.to_string()).collect());
        let assembly = crate::assemblers::assemble_own(Cpu::Z80, std::path::Path::new("t.asm"), &source.join("\n")).assembly;
        bench.buffer.intel = Some(Box::new(crate::code_intel::CodeIntel { assembly: Some(std::sync::Arc::new(assembly)), ..crate::code_intel::CodeIntel::for_cpu(Cpu::Z80) }));
        bench.buffer.move_caret(3, 0, false);
        bench.frame(vec![Event::Text("  ld hl,".into())]);
        assert!(bench.state.completion.is_none(), "nothing to finish yet");
        bench.frame(vec![Event::Text("c".into())]);
        assert!(bench.state.completion.is_none(), "one letter isn't enough to offer anything by itself");
        bench.frame(vec![Event::Text("o".into())]);
        let offered = bench.state.completion.clone().expect("two letters are");
        assert_eq!(offered.items.first().map(String::as_str), Some("counter"));
        bench.frame(vec![key(Key::Enter)]);
        assert_eq!(&*bench.buffer.line(3), "  ld hl,counter", "Enter puts it in, and makes no new line");
        assert!(bench.state.completion.is_none());
        // Ctrl+Space asks, whatever has been typed; Escape closes the list.
        bench.frame(vec![Event::Text(" ; ".into())]);
        bench.buffer.move_caret(2, 3, false);
        bench.frame(vec![Event::Key { key: Key::Space, physical_key: None, pressed: true, repeat: false, modifiers: Modifiers::CTRL | Modifiers::COMMAND }]);
        assert!(bench.state.completion.as_ref().is_some_and(|c| c.items.contains(&"ld".to_string())), "{:?}", bench.state.completion);
        bench.frame(vec![key(Key::Escape)]);
        assert!(bench.state.completion.is_none());
        assert_eq!(&*bench.buffer.line(2), "  ld a,(counter)");
    }

    #[test]
    fn graph_mode_types_the_spectrums_graphics_and_udgs_are_drawn() {
        use crate::zx_graphics::{Notation, ZxText};
        let mut bench = Bench::new(1);
        bench.buffer.set_content(vec!["10 PRINT ".into()]);
        bench.buffer.move_caret(0, 9, false);
        bench.buffer.zx = Some(Box::new(ZxText { graph: true, ..ZxText::default() }));
        let key = |key: Key, physical: Key, shift: bool| Event::Key { key, physical_key: Some(physical), pressed: true, repeat: false, modifiers: if shift { Modifiers::SHIFT } else { Modifiers::NONE } };
        // Each key comes with the text it would type, which GRAPH mode leaves out.
        bench.frame(vec![key(Key::Num3, Key::Num3, false), Event::Text("3".into())]);
        bench.frame(vec![key(Key::Exclamationmark, Key::Num1, true), Event::Text("!".into())]);
        bench.frame(vec![Event::Text("a".into()), key(Key::A, Key::A, false)]);
        assert_eq!(&*bench.buffer.line(0), "10 PRINT \u{2580}\u{2599}{UDG-A}", "GRAPH 3, GRAPH Caps Shift 1 and GRAPH A");
        bench.frame(vec![key(Key::Num9, Key::Num9, false), Event::Text("9".into())]);
        assert!(!bench.buffer.zx.as_ref().unwrap().graph, "9 leaves GRAPH mode, as on the Spectrum");
        bench.frame(vec![Event::Text("a".into())]);
        assert_eq!(&*bench.buffer.line(0), "10 PRINT \u{2580}\u{2599}{UDG-A}a");
        // Ctrl keys are still shortcuts.
        bench.buffer.zx.as_mut().unwrap().graph = true;
        bench.frame(vec![Event::Key { key: Key::A, physical_key: None, pressed: true, repeat: false, modifiers: Modifiers::CTRL }]);
        assert_eq!(&*bench.buffer.line(0), "10 PRINT \u{2580}\u{2599}{UDG-A}a");
        bench.buffer.zx.as_mut().unwrap().notation = Notation::Bas2tap;
        bench.buffer.move_caret(0, bench.buffer.line(0).chars().count(), false);
        bench.frame(vec![key(Key::U, Key::U, false), key(Key::Num8, Key::Num8, true)]);
        assert!(bench.buffer.line(0).ends_with("a{U}{+8}"));

        // With a picture, the UDG is drawn, unless the cursor is in it.
        bench.buffer.zx.as_mut().unwrap().udgs[0] = Some([0x18, 0x3C, 0x7E, 0xFF, 0xFF, 0x7E, 0x3C, 0x18]);
        assert_eq!(udg_marks(&bench.buffer, 0, &bench.buffer.line(0)).len(), 1, "{{U}} has no picture yet");
        bench.buffer.move_caret(0, 13, false);
        assert!(udg_marks(&bench.buffer, 0, &bench.buffer.line(0)).is_empty(), "the cursor is inside {{UDG-A}}");
        bench.settle();
    }

    fn format_of(id: &str) -> TextFormat {
        TextFormat { charset: charset::known(id), ..TextFormat::default() }
    }

    #[test]
    fn controls_show_as_tokens_that_are_never_part_of_the_text() {
        let (shown, tokens) = cells_for("a\u{1B}b", &format_of("utf-8"));
        assert_eq!(shown, "a     b");
        assert_eq!((tokens[0].column, tokens[0].width, tokens[0].label.as_str()), (1, 5, "[ESC]"));
        let raw = charset::raw_char(0x81);
        let (_, tokens) = cells_for(&raw.to_string(), &format_of("windows-1252"));
        assert_eq!(tokens[0].label, "[0x81]");
        assert!(tokens[0].style == TokenStyle::Undefined);
    }

    #[test]
    fn the_same_byte_reads_as_each_profile_says() {
        let byte = charset::raw_char(0x9B).to_string();
        let (_, amiga) = cells_for(&byte, &format_of("amiga-text"));
        assert_eq!(amiga[0].label, "[CSI]");
        let (_, atari) = cells_for(&byte, &format_of("atascii"));
        assert_eq!(atari[0].label, "[END OF LINE]");
    }

    #[test]
    fn switching_petscii_changes_what_later_bytes_show() {
        let line: String = [0x41u8, 0x0E, 0x41].iter().map(|b| charset::raw_char(*b)).collect();
        let (shown, tokens) = cells_for(&line, &format_of("petscii-c64"));
        assert!(shown.starts_with('A'));
        assert!(shown.ends_with('a'));
        assert_eq!(tokens[0].label, "[LOWERCASE/LATIN-9]");
    }

    #[test]
    fn inverse_and_missing_glyphs_have_their_own_look() {
        let line = charset::raw_char(0xC1).to_string();
        let (_, tokens) = cells_for(&line, &format_of("atascii"));
        assert!(tokens[0].style == TokenStyle::Inverse);
        assert_eq!(tokens[0].label, "A");
        let format = format_of("cp437");
        crate::text_buffer::set_cell_profile(format.charset);
        let mut state = format.tracker();
        let (_, tokens) = legacy_cells(&charset::raw_char(0x1B).to_string(), &format, &mut state, &mut |_| false);
        assert_eq!(tokens[0].label, "[ESC]", "a control needs no glyph");
    }

    #[test]
    fn amiga_sequences_are_marked_but_their_bytes_stay() {
        let mut format = format_of("amiga-text");
        format.display = Display::AmigaConsole;
        let line = format!("{}31mx", charset::raw_char(0x9B));
        let (shown, tokens) = cells_for(&line, &format);
        assert_eq!(tokens[0].label, "[CSI]");
        assert_eq!(tokens.iter().filter(|t| t.style == TokenStyle::Sequence).count(), 3);
        assert!(shown.ends_with('x'));
        let plain = format_of("amiga-text");
        let (_, tokens) = cells_for(&line, &plain);
        assert_eq!(tokens.iter().filter(|t| t.style == TokenStyle::Sequence).count(), 0, "plain text doesn't read them");
    }

    #[test]
    fn copying_gives_readable_text_and_pasting_back_gives_the_exact_bytes() {
        let mut clip = ClipState::default();
        let exact = charset::raw_char(0x61).to_string();
        clip.remember_exact("\u{2660}", &exact);
        assert_eq!(clip.exact_for("\u{2660}"), exact);
        assert_eq!(clip.exact_for("something else"), "something else");
    }

    fn wrapped_bench(lines: Vec<String>) -> Bench {
        let mut b = Bench::new(1);
        b.buffer.set_content(lines);
        b.options.word_wrap = true;
        b.settle();
        b
    }

    fn key(key: Key) -> Event {
        Event::Key { key, physical_key: None, pressed: true, repeat: false, modifiers: Modifiers::NONE }
    }

    #[test]
    fn word_wrap_only_changes_the_drawing_never_the_text() {
        let long = "word ".repeat(400);
        let lines = vec![long.clone(), "short".to_string(), long.clone()];
        let mut b = wrapped_bench(lines.clone());
        for _ in 0..5 {
            b.frame(vec![key(Key::ArrowDown)]);
        }
        b.settle();
        let now: Vec<String> = b.buffer.all_lines().iter().map(|line| line.to_string()).collect();
        assert_eq!(now, lines);
        assert!(!b.buffer.is_modified());
        assert_eq!(b.buffer.line_count(), 3, "no lines were added");
    }

    #[test]
    fn under_word_wrap_up_and_down_go_by_rows_of_the_screen() {
        let mut b = wrapped_bench(vec!["word ".repeat(400), "after".to_string()]);
        assert_eq!(b.buffer.caret_line, 0);
        let width = b.state.rows; // Only to show the view has settled.
        assert!(width > 0);
        b.frame(vec![key(Key::ArrowDown)]);
        b.settle();
        assert_eq!(b.buffer.caret_line, 0, "still in the long line");
        assert!(b.buffer.caret_col > 0, "on its next row");
        let first_row = b.buffer.caret_col;
        b.frame(vec![key(Key::ArrowDown)]);
        b.settle();
        assert_eq!(b.buffer.caret_col, first_row * 2);
        b.frame(vec![key(Key::ArrowUp)]);
        b.frame(vec![key(Key::ArrowUp)]);
        b.settle();
        assert_eq!(b.buffer.caret_col, 0);
    }

    #[test]
    fn under_word_wrap_home_and_end_keep_to_the_row() {
        let mut b = wrapped_bench(vec!["word ".repeat(400)]);
        b.frame(vec![key(Key::End)]);
        b.settle();
        let end_of_row = b.buffer.caret_col;
        assert!(end_of_row > 0 && end_of_row < 300, "stays on the first row: {end_of_row}");
        b.frame(vec![key(Key::End)]);
        b.settle();
        assert_eq!(b.buffer.caret_col, 2000, "pressed again, it goes to the end of the line");
        b.frame(vec![key(Key::Home)]);
        b.settle();
        assert!(b.buffer.caret_col > 1900, "the start of the last row");
    }

    #[test]
    fn under_word_wrap_the_cursor_is_kept_on_screen_and_the_wheel_counts_rows() {
        let lines: Vec<String> = (0..50).map(|_| "word ".repeat(200)).collect();
        let mut b = wrapped_bench(lines);
        b.buffer.move_caret(30, 900, false);
        b.settle();
        assert!(b.state.top <= 30);
        assert!(b.state.top + b.state.rows >= 30, "top {} rows {}", b.state.top, b.state.rows);
        let at = (b.state.top, b.state.top_sub);
        b.wheel(pos2(400.0, 400.0), -3.0);
        assert!((b.state.top, b.state.top_sub) > at, "the wheel moves down through the rows");
        b.wheel(pos2(400.0, 400.0), -50000.0);
        let end = (b.state.top, b.state.top_sub);
        assert!(end.0 > 40, "stops with the last line on screen: {}", end.0);
        b.wheel(pos2(400.0, 400.0), -5000.0);
        assert_eq!((b.state.top, b.state.top_sub), end, "and stays there");
        b.wheel(pos2(400.0, 400.0), 50000.0);
        assert_eq!((b.state.top, b.state.top_sub), (0, 0));
    }

    #[test]
    fn under_word_wrap_a_click_lands_on_the_row_under_it() {
        let mut b = wrapped_bench(vec!["word ".repeat(400)]);
        let font = FontId::monospace(14.0);
        let cell = b.ctx.fonts_mut(|f| vec2(f.glyph_width(&font, 'M'), f.row_height(&font).ceil() + 2.0));
        let ruler = cell.y + 4.0;
        b.press_and_release(pos2(300.0, ruler + cell.y * 2.5), 0);
        assert_eq!(b.buffer.caret_line, 0);
        assert!(b.buffer.caret_col > 100, "two rows down the same line: {}", b.buffer.caret_col);
    }

    #[test]
    fn alt_dragging_makes_no_block_under_word_wrap() {
        let mut b = wrapped_bench(vec!["word ".repeat(400)]);
        let at = pos2(300.0, 100.0);
        b.frame(vec![Event::PointerMoved(at)]);
        let alt = Modifiers { alt: true, ..Modifiers::NONE };
        b.frame(vec![Event::PointerButton { pos: at, button: egui::PointerButton::Primary, pressed: true, modifiers: alt }]);
        b.frame(vec![Event::PointerButton { pos: at, button: egui::PointerButton::Primary, pressed: false, modifiers: alt }]);
        assert!(!b.buffer.is_block());
    }

    #[test]
    fn every_view_option_draws_under_word_wrap() {
        let mut b = wrapped_bench(vec!["\ta b\t\u{e9}\u{4e2d}\u{1F600} (brackets) ".repeat(30), String::new(), "x".repeat(5000), "\u{1B}[0m end".to_string()]);
        for (whitespace, endings, map, ruler, numbers, column, rainbow, highlight) in [(true, true, true, true, true, true, true, true), (true, false, false, false, false, false, false, false), (false, true, true, false, true, true, true, false)] {
            b.options.whitespace = whitespace;
            b.options.line_endings = endings;
            b.options.document_map = map;
            b.options.ruler = ruler;
            b.options.line_numbers = numbers;
            b.options.active_col = column;
            b.options.rainbow_parentheses = rainbow;
            b.options.highlight_selected = highlight;
            b.buffer.move_caret(2, 4000, false);
            b.settle();
            b.buffer.move_caret(0, 0, false);
            b.buffer.move_caret(0, 12, true);
            b.settle();
        }
    }

    #[test]
    fn copies_of_the_selection_are_found_by_screen_column() {
        assert_eq!(matches_on_line("cat dog cat", "cat"), [(0, 3), (8, 11)]);
        assert_eq!(matches_on_line("\tcat", "cat"), [(8, 11)]);
        assert!(matches_on_line("nothing", "cat").is_empty());
        assert!(matches_on_line("anything", "").is_empty());
        let mut b = Bench::new(2);
        b.buffer.set_content(vec!["cat dog".to_string(), "cat".to_string()]);
        assert_eq!(highlight_needle(&b.buffer), None);
        b.buffer.move_caret(0, 0, false);
        b.buffer.move_caret(0, 3, true);
        assert_eq!(highlight_needle(&b.buffer).as_deref(), Some("cat"));
        b.buffer.move_caret(1, 1, true);
        assert_eq!(highlight_needle(&b.buffer), None, "not across lines");
    }

    #[test]
    fn a_read_only_document_can_be_looked_at_but_not_changed() {
        let mut b = Bench::new(3);
        b.settle();
        b.buffer.read_only = true;
        let before: Vec<String> = b.buffer.all_lines().iter().map(|line| line.to_string()).collect();
        b.frame(vec![Event::Text("x".into()), key(Key::Enter), key(Key::Backspace), key(Key::Delete), key(Key::Tab), Event::Paste("pasted".into()), Event::Cut]);
        b.frame(Vec::new());
        let after: Vec<String> = b.buffer.all_lines().iter().map(|line| line.to_string()).collect();
        assert_eq!(after, before);
        assert!(!b.buffer.is_modified());
        b.frame(vec![key(Key::ArrowDown)]);
        b.frame(Vec::new());
        assert_eq!(b.buffer.caret_line, 1, "moving around still works");
    }

    #[test]
    fn typing_goes_nowhere_while_a_box_is_open() {
        let mut b = Bench::new(3);
        b.settle();
        b.input_on = false;
        b.frame(vec![Event::Text("x".into()), Event::Key {
            key: Key::Enter,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: Modifiers::NONE,
        }]);
        assert!(!b.buffer.is_modified());
        b.input_on = true;
        b.frame(vec![Event::Text("y".into())]);
        b.frame(Vec::new());
        assert!(b.buffer.line(0).starts_with('y'));
    }
}

#[cfg(test)]
mod mac_key_tests {
    use super::*;

    #[test]
    #[cfg(target_os = "macos")]
    fn a_macs_keys_move_as_a_mac_does() {
        let option = Modifiers { alt: true, ..Modifiers::NONE };
        let cmd = Modifiers { mac_cmd: true, command: true, ..Modifiers::NONE };
        assert!(mac_keys(Key::ArrowLeft, option).1.ctrl, "Option and an arrow moves by a word");
        assert_eq!(mac_keys(Key::ArrowLeft, cmd).0, Key::Home, "Cmd and Left goes to the start of the line");
        assert_eq!(mac_keys(Key::ArrowDown, cmd), (Key::End, Modifiers { ctrl: true, command: true, ..Modifiers::NONE }), "Cmd and Down goes to the end of the file");
        let shifted = Modifiers { shift: true, ..cmd };
        assert!(mac_keys(Key::ArrowRight, shifted).1.shift, "Shift still selects");
    }

    #[test]
    #[cfg(not(target_os = "macos"))]
    fn other_systems_keys_are_left_alone() {
        let ctrl = Modifiers { ctrl: true, command: true, ..Modifiers::NONE };
        assert_eq!(mac_keys(Key::ArrowLeft, ctrl), (Key::ArrowLeft, ctrl));
        assert_eq!(mac_keys(Key::ArrowLeft, Modifiers::ALT), (Key::ArrowLeft, Modifiers::ALT));
    }
}

#[cfg(test)]
mod shown_tests {
    use super::*;
    use std::collections::BTreeMap;

    #[test]
    fn hidden_stretches_give_the_same_rows_as_a_list_of_lines() {
        // Folds at 2..=4, 3..=6 (inside and past it), and 10..=11, in a file of 15 lines.
        let folds: BTreeMap<usize, usize> = [(2, 4), (3, 6), (10, 11)].into_iter().collect();
        let hidden = hidden_by_folds(&folds);
        assert_eq!(hidden, [Hidden { first: 3, last: 6, shown_before: 3 }, Hidden { first: 11, last: 11, shown_before: 7 }]);
        let shown = Shown::AllBut { hidden: &hidden, total: 15 };
        let lines: Vec<usize> = (0..15).filter(|l| !(3..=6).contains(l) && *l != 11).collect();
        let list = Shown::Lines(&lines);
        assert_eq!(shown.len(), list.len());
        for row in 0..=lines.len() {
            assert_eq!(shown.line(row), list.line(row), "row {row}");
        }
        for line in 0..=15 {
            assert_eq!(shown.row_of(line), list.row_of(line), "line {line}");
            assert_eq!(shown.contains(line), list.contains(line), "line {line}");
        }
        assert_eq!(shown.rows_from(2, 4), [2, 7, 8, 9]);
    }
}

/// Hover text with colour swatches: each \u{25A0} followed by #RRGGBB is drawn in that colour.
fn swatched(words: &str, font: FontId, colour: egui::Color32) -> egui::text::LayoutJob {
    let mut job = egui::text::LayoutJob::default();
    let plain = egui::TextFormat { font_id: font.clone(), color: colour, ..Default::default() };
    let mut rest = words;
    while let Some(at) = rest.find('\u{25A0}') {
        job.append(&rest[..at], 0.0, plain.clone());
        let after = &rest[at + '\u{25A0}'.len_utf8()..];
        let rgb = after.strip_prefix('#').and_then(|h| h.get(..6)).and_then(|h| u32::from_str_radix(h, 16).ok());
        let swatch = match rgb {
            Some(v) => egui::Color32::from_rgb((v >> 16) as u8, (v >> 8) as u8, v as u8),
            None => colour,
        };
        job.append("\u{2588}\u{2588}", 0.0, egui::TextFormat { font_id: font.clone(), color: swatch, ..Default::default() });
        rest = after;
    }
    job.append(rest, 0.0, plain);
    job
}
