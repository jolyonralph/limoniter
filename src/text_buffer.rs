//! The text being edited. Positions are (line, visual column), where a tab counts to the next
//! tab stop, so that a column block lines up on screen. Nothing here draws anything.

use crate::charset::{self, CellState, Charset, Model, raw_byte};
use crate::codec::{Decoded, Eol, EolCounts, TextFormat};
use crate::history_file;
use crate::line_store::{LineStore, Source};
use crate::undo_tree::{NodeId, Row, UndoTree};
use regex::{Captures, NoExpand, Regex, RegexBuilder};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

/// The tab size when none has been set.
pub const DEFAULT_TAB_SIZE: usize = 8;
pub const MIN_TAB_SIZE: usize = 1;
pub const MAX_TAB_SIZE: usize = 32;

/// How keys and files behave, from Settings. The defaults are how the app has always worked.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EditPrefs {
    pub spaces_for_tab: bool,
    pub auto_indent: bool,
    pub lock_insert: bool,
    pub home_to_start: bool,
    pub whole_line_copy: bool,
    pub nulls_open_in_hex: bool,
    /// Pasting over a selection puts what was selected on the clipboard.
    pub copy_before_paste: bool,
}

impl EditPrefs {
    const STARTING: Self = Self { spaces_for_tab: false, auto_indent: true, lock_insert: false, home_to_start: false, whole_line_copy: false, nulls_open_in_hex: true, copy_before_paste: false };
}

impl Default for EditPrefs {
    fn default() -> Self {
        Self::STARTING
    }
}

thread_local! {
    static EDIT_PREFS: std::cell::Cell<EditPrefs> = const { std::cell::Cell::new(EditPrefs::STARTING) };
    static TAB_SIZE: std::cell::Cell<usize> = const { std::cell::Cell::new(DEFAULT_TAB_SIZE) };
    /// Whether controls show as readable tokens, or as one small cell each.
    static SOURCE_VIEW: std::cell::Cell<bool> = const { std::cell::Cell::new(true) };
    /// How many cells each kept byte takes on screen, for the document being worked on.
    static BYTE_CELLS: std::cell::Cell<Option<&'static [u8; 256]>> = const { std::cell::Cell::new(None) };
}

/// Which set's tokens the columns are counted with; set for the document being worked on.
pub fn set_cell_profile(set: &'static Charset) {
    BYTE_CELLS.with(|cell| cell.set(Some(set.widths())));
}

pub fn source_view() -> bool {
    SOURCE_VIEW.with(std::cell::Cell::get)
}

pub fn set_source_view(on: bool) {
    SOURCE_VIEW.with(|cell| cell.set(on));
}

/// The tab size in use; the window sets it from the Format tab.
pub fn edit_prefs() -> EditPrefs {
    EDIT_PREFS.with(std::cell::Cell::get)
}

pub fn set_edit_prefs(prefs: EditPrefs) {
    EDIT_PREFS.with(|cell| cell.set(prefs));
}

pub fn tab_size() -> usize {
    TAB_SIZE.with(std::cell::Cell::get)
}

pub fn set_tab_size(size: usize) {
    TAB_SIZE.with(|cell| cell.set(size.clamp(MIN_TAB_SIZE, MAX_TAB_SIZE)));
}
const MAX_UNDO_SMALL: usize = 200;
const MAX_UNDO_LARGE: usize = 10;
const LARGE_LINE_COUNT: usize = 100_000;
/// On a file kept on disk, a command touching more lines than this is refused rather than left to run for hours.
const MAX_ROWS_ON_DISK: usize = 2_000_000;
/// The largest selection that can go on the clipboard from a file kept on disk.
const MAX_COPY_ROWS: usize = 1_000_000;

/// A script's run, which comes off in one Undo.
#[derive(Clone, Copy)]
struct UndoGroup {
    /// The script uses undo itself, so each step has to be there for it until it ends.
    keep_steps: bool,
    /// The state from before its first change, once it's made one.
    base: Option<NodeId>,
    /// Where the states it makes begin.
    first_new: NodeId,
    /// A state from before its latest change is held, so the next change needs no copy of its own.
    held: bool,
    changed: bool,
}

#[derive(Clone, Default)]
pub struct SearchOptions {
    pub pattern: String,
    pub replacement: String,
    pub match_case: bool,
    pub whole_word: bool,
    pub regex: bool,
    pub hex_bytes: bool,
    pub backward: bool,
}

/// What a word count works out.
#[derive(Debug, PartialEq, Clone, Default)]
pub struct Counts {
    pub words: u64,
    pub lines: u64,
    pub non_space: u64,
    /// Every character, the line endings included.
    pub characters: u64,
    pub non_empty: u64,
    pub average: u64,
    pub longest_chars: u64,
    /// One-based; the first of equally long lines.
    pub longest_line: u64,
}

/// Counts lines the way the original does, reading them as if they were one stream of characters.
pub fn count_lines<S: AsRef<str>>(lines: &[S], eol_chars: u64) -> Counts {
    let mut c = Counts { lines: lines.len() as u64, longest_line: 1, ..Counts::default() };
    let mut in_space = true;
    let mut total_length = 0u64;
    for (i, line) in lines.iter().enumerate() {
        let line = line.as_ref();
        let mut length = 0u64;
        let mut has_text = false;
        let mut after_text = false;
        for ch in line.chars() {
            // A combining mark straight after a character counts as part of it.
            if after_text && matches!(ch as u32, 0x300..=0x36F | 0x1DC0..=0x1DFF) {
                after_text = false;
                continue;
            }
            length += 1;
            if ch.is_whitespace() {
                in_space = true;
                after_text = false;
            } else {
                if in_space {
                    c.words += 1;
                }
                in_space = false;
                has_text = true;
                after_text = true;
                c.non_space += 1;
            }
        }
        c.characters += length;
        total_length += length;
        if length > c.longest_chars {
            c.longest_chars = length;
            c.longest_line = i as u64 + 1;
        }
        let is_last = i + 1 == lines.len();
        if !is_last {
            c.characters += eol_chars;
            in_space = true;
            c.non_empty += u64::from(has_text);
        } else if lines.len() == 1 || length > 0 {
            // The last line counts unless the file ends with a line ending, whatever is on it.
            c.non_empty += 1;
        }
    }
    c.average = total_length / c.lines.max(1);
    c
}

/// The result of adding up a column.
#[derive(Debug, PartialEq)]
pub struct ColumnSum {
    pub sum: f64,
    pub numbers: usize,
    pub skipped: usize,
    pub decimals: usize,
}

#[derive(Clone)]
struct Snapshot {
    lines: LineStore,
    format: TextFormat,
    caret_line: usize,
    caret_col: usize,
    anchor_line: usize,
    anchor_col: usize,
    version: u64,
}

/// Marks for assembler source, by line.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct CodeMarks {
    pub breakpoints: std::collections::BTreeSet<usize>,
    pub problems: Vec<(usize, String)>,
    /// Where a program stopped at a breakpoint or a step is.
    pub stopped: Option<usize>,
}

impl CodeMarks {
    pub fn problem_at(&self, line: usize) -> Option<&str> {
        self.problems.iter().find(|(l, _)| *l == line).map(|(_, m)| m.as_str())
    }
}

pub struct TextBuffer {
    lines: LineStore,
    /// How this document's bytes and text relate. Undo brings it back along with the text.
    pub format: TextFormat,
    pub caret_line: usize,
    pub caret_col: usize,
    pub anchor_line: usize,
    pub anchor_col: usize,
    desired_col: usize,
    column_mode: bool,
    alt_block: bool,
    /// Several selections at once. While there are any, the cursor sits on the last, with no selection of its own.
    multi: Vec<Spot>,
    pub overwrite: bool,
    /// In a file of fixed-width rows, the rows an edit has touched, to be brought back to the width afterwards.
    rows_touched: Option<(usize, usize)>,
    /// Set by the Read-only switch; the window and the views don't let edits through while it's on.
    pub read_only: bool,
    version: u64,
    next_version: u64,
    saved_version: u64,
    /// Every earlier state kept, as a tree: undoing and then typing starts a new branch.
    history: UndoTree<Snapshot>,
    /// While narrowed, how far down the whole file these lines begin, for numbering them as the file does.
    pub line_offset: usize,
    /// While a script runs, everything it does comes off in one Undo.
    undo_group: Option<UndoGroup>,
    last_kind: Option<&'static str>,
    last_time: Instant,
    max_visual: usize,
    /// Set when the view should scroll the caret into sight.
    pub reveal: bool,
    /// Something the window should tell the person, such as a command being refused.
    pub notice: Option<String>,
    /// Each line's change since the last commit, for the margin, when the file is in a Git repository.
    pub git_marks: Option<std::sync::Arc<Vec<Option<crate::git::Mark>>>>,
    /// For assembler source: its breakpoints, the assembler's problems, and the line a stopped program is at.
    pub code: Option<Box<CodeMarks>>,
    /// For Spectrum BASIC: GRAPH mode, and the pictures its UDGs were given.
    pub zx: Option<Box<crate::zx_graphics::ZxText>>,
    /// What's known about the code, for completion, hover, going to definitions and the outline.
    pub intel: Option<Box<crate::code_intel::CodeIntel>>,
}

/// Where a search has got to; large files are searched a little at a time between frames.
pub struct SearchCursor {
    re: Regex,
    backward: bool,
    line: usize,
    byte: usize,
    step: usize,
}

pub enum SearchStep {
    Running(f32),
    Found,
    NotFound,
}

/// Where a Replace All has got to.
pub struct ReplaceCursor {
    re: Regex,
    options: SearchOptions,
    next: usize,
    pub count: usize,
    /// The changes so far, which all go in together at the end.
    edits: Vec<(usize, Arc<str>, crate::codec::Eol)>,
    held: usize,
    most_held: usize,
    /// It stopped because the changes would hold too much in memory, and nothing has changed.
    pub too_large: bool,
}

/// The most text a Replace All on a file kept on disk will hold in memory until it's saved.
const MOST_REPLACED_IN_MEMORY: usize = 1024 * 1024 * 1024;

/// One line with every match replaced, and how many there were. None if it has none.
pub fn replaced_line(re: &Regex, options: &SearchOptions, line: &str) -> Option<(String, usize)> {
    let mut here = 0usize;
    let replaced = re.replace_all(line, |caps: &Captures| {
        if caps[0].is_empty() {
            return String::new();
        }
        here += 1;
        if options.regex {
            let mut out = String::new();
            caps.expand(&options.replacement, &mut out);
            out
        } else {
            re.replace(&caps[0], NoExpand(&options.replacement)).into_owned()
        }
    });
    (here > 0).then(|| (replaced.into_owned(), here))
}

impl Default for TextBuffer {
    fn default() -> Self {
        Self {
            lines: LineStore::from_vec(vec![Arc::from("")]),
            format: TextFormat::default(),
            caret_line: 0,
            caret_col: 0,
            anchor_line: 0,
            anchor_col: 0,
            desired_col: 0,
            column_mode: false,
            alt_block: false,
            multi: Vec::new(),
            overwrite: false,
            rows_touched: None,
            read_only: false,
            version: 0,
            next_version: 0,
            saved_version: 0,
            history: UndoTree::default(),
            line_offset: 0,
            undo_group: None,
            last_kind: None,
            last_time: Instant::now(),
            max_visual: 0,
            reveal: false,
            git_marks: None,
            code: None,
            zx: None,
            intel: None,
            notice: None,
        }
    }
}

/// One of several selections or cursors, all on single lines, counted in characters.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Spot {
    pub line: usize,
    pub anchor: usize,
    pub head: usize,
}

impl Spot {
    fn start(&self) -> usize {
        self.anchor.min(self.head)
    }

    fn end(&self) -> usize {
        self.anchor.max(self.head)
    }
}

/// The most selections Make selections will make.
pub const MOST_SPOTS: usize = 10_000;
/// How far down a fold is looked for, so that one click can't read a whole huge file.
const MOST_FOLD_LINES: usize = 200_000;

/// How to move every cursor at once.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SpotMove {
    Left,
    Right,
    Up,
    Down,
    Home,
    End,
}

/// A line's indent in columns, or None for a line with nothing on it.
fn indent_of(line: &str) -> Option<usize> {
    let mut v = 0;
    for c in line.chars() {
        match c {
            ' ' | '\t' => v = next_visual(v, c),
            _ => return Some(v),
        }
    }
    None
}

/// What the Case menu can do to a selection.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CaseChange {
    Upper,
    Lower,
    Sentence,
    Title,
}

/// Where the case rules have got to, carried from one row of a selection to the next.
struct CaseState {
    /// The next letter starts a sentence.
    sentence_start: bool,
    /// The last thing read could end a sentence, if a space follows.
    after_ending: bool,
    /// The next letter is partway through a word.
    in_word: bool,
}

impl CaseState {
    /// The start of a selection counts as the start of a sentence and of a word.
    fn new() -> Self {
        Self { sentence_start: true, after_ending: false, in_word: false }
    }

    /// A line ending is a space. An empty line also starts a new sentence, as a new paragraph.
    fn end_row(&mut self, row_was_empty: bool) {
        if self.after_ending || row_was_empty {
            self.sentence_start = true;
        }
        self.after_ending = false;
        self.in_word = false;
    }
}

fn push_upper_first(out: &mut String, c: char) {
    // A few letters become two when made upper case: only the first of them stays a capital.
    let mut upper = c.to_uppercase();
    out.extend(upper.next());
    upper.for_each(|rest| out.extend(rest.to_lowercase()));
}

fn sentence_case(text: &str, state: &mut CaseState) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        if c.is_alphabetic() {
            if state.sentence_start {
                push_upper_first(&mut out, c);
            } else {
                out.extend(c.to_lowercase());
            }
            state.sentence_start = false;
            state.after_ending = false;
            continue;
        }
        out.push(c);
        if matches!(c, '.' | '!' | '?') {
            state.after_ending = true;
        } else if c.is_whitespace() {
            state.sentence_start |= state.after_ending;
            state.after_ending = false;
        } else if !matches!(c, '"' | '\'' | ')' | ']' | '}' | '\u{201D}' | '\u{2019}') {
            // A closing quote or bracket can come between a full stop and its space; anything else can't,
            // which is why 3.5 and example.com don't start a sentence.
            state.after_ending = false;
        }
    }
    out
}

fn title_case(text: &str, state: &mut CaseState) -> String {
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        if c.is_alphabetic() && !state.in_word {
            push_upper_first(&mut out, c);
        } else if c.is_alphabetic() {
            out.extend(c.to_lowercase());
        } else {
            out.push(c);
        }
        // An apostrophe inside a word stays inside it, so that it's Don't and not Don'T.
        let joins = matches!(c, '\'' | '\u{2019}') && state.in_word && chars.peek().is_some_and(|next| next.is_alphabetic());
        state.in_word = c.is_alphanumeric() || joins;
    }
    out
}

fn convert_case(text: &str, change: CaseChange, state: &mut CaseState) -> String {
    match change {
        CaseChange::Upper => text.to_uppercase(),
        CaseChange::Lower => text.to_lowercase(),
        CaseChange::Sentence => sentence_case(text, state),
        CaseChange::Title => title_case(text, state),
    }
}

/// How Justify lines up the text in a column block.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Justify {
    Left,
    Center,
    Right,
}

// ---------- Columns and tabs ----------

pub fn next_visual(v: usize, c: char) -> usize {
    if (0x20..0x7F).contains(&(c as u32)) {
        v + 1
    } else if c == '\t' {
        v + tab_size() - (v % tab_size())
    } else {
        v + special_cells(c)
    }
}

/// The cells a character other than ordinary ASCII takes: one, unless it shows as a token.
fn special_cells(c: char) -> usize {
    if !source_view() {
        return 1;
    }
    if let Some(byte) = raw_byte(c) {
        return BYTE_CELLS.with(std::cell::Cell::get).map_or(6, |widths| usize::from(widths[usize::from(byte)]));
    }
    charset::control_name(c as u32).map_or(1, |name| name.len() + 2)
}

/// Whether a character is drawn as a token or from its byte, rather than as plain text.
pub fn is_special(c: char) -> bool {
    let value = c as u32;
    raw_byte(c).is_some() || (c != '\t' && (value < 0x20 || (0x7F..=0x9F).contains(&value)))
}

pub fn visual_len(s: &str) -> usize {
    s.chars().fold(0, next_visual)
}

fn visual_of(s: &str, index: usize) -> usize {
    s.chars().take(index).fold(0, next_visual)
}

/// The character index at or just before a visual column, clamped to the line.
fn index_at_or_before(s: &str, col: usize) -> usize {
    let mut v = 0;
    let mut count = 0;
    for c in s.chars() {
        let next = next_visual(v, c);
        if next > col {
            return count;
        }
        v = next;
        count += 1;
    }
    count
}

fn byte_of(s: &str, index: usize) -> usize {
    s.char_indices().nth(index).map_or(s.len(), |(b, _)| b)
}

fn char_of_byte(s: &str, byte: usize) -> usize {
    s[..byte].chars().count()
}

/// Replaces characters a..b of a line with new text.
fn splice(s: &str, a: usize, b: usize, with: &str) -> String {
    let (ba, bb) = (byte_of(s, a), byte_of(s, b));
    let mut out = String::with_capacity(s.len() + with.len());
    out.push_str(&s[..ba]);
    out.push_str(with);
    out.push_str(&s[bb..]);
    out
}

pub(crate) fn expand_tabs(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut v = 0;
    for c in s.chars() {
        let next = next_visual(v, c);
        if c == '\t' {
            out.extend(std::iter::repeat_n(' ', next - v));
        } else {
            out.push(c);
        }
        v = next;
    }
    out
}

/// Text for the screen: tabs become spaces, and control characters a visible dot.
pub fn display_text(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut v = 0;
    for c in s.chars() {
        let next = next_visual(v, c);
        if c == '\t' {
            out.extend(std::iter::repeat_n(' ', next - v));
        } else if is_special(c) {
            out.extend(std::iter::repeat_n('\u{B7}', next - v));
        } else {
            out.push(c);
        }
        v = next;
    }
    out
}

/// The visual cells left..right of a line as plain text, padded to the block's width.
pub(crate) fn block_cell(s: &str, left: usize, right: usize) -> String {
    let mut cell = String::new();
    let mut have = 0;
    let mut v = 0;
    for c in s.chars() {
        let next = next_visual(v, c);
        if c == '\t' {
            let spaces = next.min(right).saturating_sub(v.max(left));
            cell.extend(std::iter::repeat_n(' ', spaces));
            have += spaces;
        } else if v >= left && v < right {
            // A token is one byte however wide it's drawn, so it's copied whole and never split.
            cell.push(c);
            have += next - v;
        }
        v = next;
        if v >= right {
            break;
        }
    }
    cell.extend(std::iter::repeat_n(' ', (right - left).saturating_sub(have)));
    cell
}

fn is_word_char(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

fn split_rows(text: &str) -> Vec<String> {
    text.replace("\r\n", "\n").replace('\r', "\n").split('\n').map(str::to_string).collect()
}

enum LineSearch {
    Hit(usize, usize),
    Nothing,
    Finished,
}

/// Looks through one line going forward; on the last pass round, stops where the search began.
fn forward_in_line(re: &Regex, s: &str, k: usize, count: usize, byte: usize) -> LineSearch {
    let from = if k == 0 { byte } else { 0 };
    for m in re.find_iter(s) {
        if m.start() < from || m.is_empty() {
            continue;
        }
        if k == count && m.start() >= byte {
            return LineSearch::Finished;
        }
        return LineSearch::Hit(m.start(), m.end());
    }
    LineSearch::Nothing
}

fn backward_in_line(re: &Regex, s: &str, k: usize, count: usize, byte: usize) -> LineSearch {
    let mut best = LineSearch::Nothing;
    for m in re.find_iter(s) {
        if m.is_empty() {
            continue;
        }
        if k == 0 && m.end() > byte {
            break;
        }
        if k == count && m.end() <= byte {
            continue;
        }
        best = LineSearch::Hit(m.start(), m.end());
    }
    best
}

impl TextBuffer {
    // ---------- State ----------

    pub fn line_count(&self) -> usize {
        self.lines.len()
    }

    pub fn line(&self, i: usize) -> Arc<str> {
        self.lines.get(i)
    }

    /// Every line. Only for a document held in memory.
    pub fn all_lines(&self) -> Vec<Arc<str>> {
        self.lines.range_vec(0, self.lines.len())
    }

    /// Changes whenever the text does, so that panels know when to look again.
    pub fn version(&self) -> u64 {
        self.version
    }

    /// Each colour-tagged stretch, as its first line, last line and colour, in order.
    pub fn tags(&self) -> Vec<(usize, usize, u8)> {
        self.lines.tags().iter().map(|(first, (last, colour))| (*first, *last, *colour)).collect()
    }

    pub fn tag_at(&self, line: usize) -> Option<(usize, usize, u8)> {
        self.lines.tag_at(line)
    }

    /// Tags these lines with a colour. Like bookmarks, tags aren't an undo step of their own.
    pub fn tag_lines(&mut self, first: usize, last: usize, colour: u8) {
        self.lines.set_tag(first, last, colour);
    }

    pub fn untag_lines(&mut self, first: usize, last: usize) -> bool {
        self.lines.remove_tags(first, last)
    }

    pub fn set_tags(&mut self, tags: std::collections::BTreeMap<usize, (usize, u8)>) {
        self.lines.set_tags(tags);
    }

    /// Goes to the start of the next tagged stretch after the cursor's, or the one before, going round. Returns
    /// false if there are none.
    pub fn go_to_tag(&mut self, next: bool) -> bool {
        let tags = self.tags();
        if tags.is_empty() {
            return false;
        }
        let here = self.caret_line;
        let inside = self.tag_at(here).map(|(first, _, _)| first);
        let target = if next {
            tags.iter().find(|(first, _, _)| *first > here).or(tags.first())
        } else {
            let from = inside.unwrap_or(here);
            tags.iter().rev().find(|(first, _, _)| *first < from).or(tags.last())
        };
        let Some((first, _, _)) = target.copied() else {
            return false;
        };
        self.move_caret(first, 0, false);
        self.reveal = true;
        true
    }

    pub fn bookmarks(&self) -> Vec<usize> {
        self.lines.bookmarks()
    }

    pub fn is_bookmarked(&self, line: usize) -> bool {
        self.lines.is_bookmarked(line)
    }

    pub fn toggle_bookmark(&mut self, line: usize) {
        if line < self.lines.len() {
            self.lines.toggle_bookmark(line);
        }
    }

    /// Goes to the next or previous bookmark, wrapping round. Returns false if there are none.
    pub fn go_to_bookmark(&mut self, forward: bool) -> bool {
        let marks = self.lines.bookmarks();
        let target = if forward {
            marks.iter().find(|line| **line > self.caret_line).or(marks.first())
        } else {
            marks.iter().rev().find(|line| **line < self.caret_line).or(marks.last())
        };
        let Some(&line) = target else {
            return false;
        };
        self.move_caret(line, 0, false);
        self.desired_col = 0;
        true
    }

    /// The selection, or the whole document if nothing is selected and it's held in memory.
    pub fn selection_or_all(&self) -> Option<String> {
        if self.has_selection() {
            return (!self.selection_too_large_to_copy()).then(|| self.selection_text());
        }
        if self.lines.is_disk_backed() {
            return None;
        }
        Some(self.all_lines().iter().map(|l| &**l).collect::<Vec<_>>().join("\r\n"))
    }

    /// Replaces the selection, or the whole document if nothing is selected, in one undoable step.
    pub fn replace_selection_or_all(&mut self, text: &str) {
        if !self.has_selection() {
            self.select_all();
        }
        self.push_undo(None);
        self.delete_stream_selection();
        self.insert_stream(text);
        self.edited();
    }

    /// The selected text if it's short enough to preview.
    pub fn selection_for_preview(&self) -> String {
        const MAX_ROWS: usize = 8;
        if !self.has_selection() || self.caret_line.abs_diff(self.anchor_line) > MAX_ROWS {
            return String::new();
        }
        self.selection_text().chars().take(64).collect()
    }

    pub fn char_at_caret(&self) -> Option<char> {
        let line = self.lines.get(self.caret_line);
        line.chars().nth(index_at_or_before(&line, self.caret_col))
    }

    /// The state at the start of a line, for sets where earlier bytes change what later ones show.
    pub fn state_at_line(&self, line: usize) -> CellState {
        let mut state = self.format.tracker();
        if self.format.charset.model == Model::Petscii {
            for i in 0..line.min(self.lines.len()) {
                for c in self.lines.get(i).chars() {
                    if let Some(byte) = raw_byte(c) {
                        state.advance(byte);
                    }
                }
            }
        }
        state
    }

    /// The state in force at the caret.
    pub fn state_at_caret(&self) -> CellState {
        let mut state = self.state_at_line(self.caret_line);
        let line = self.lines.get(self.caret_line);
        for c in line.chars().take(index_at_or_before(&line, self.caret_col)) {
            if let Some(byte) = raw_byte(c) {
                state.advance(byte);
            }
        }
        state
    }

    /// The table typing goes through at the caret.
    pub fn input_table(&self) -> usize {
        match self.format.charset.model {
            Model::Petscii => self.state_at_caret().table.min(1),
            _ => self.format.state.min(self.format.charset.tables.len().saturating_sub(1)),
        }
    }

    pub fn eol_counts(&self) -> EolCounts {
        self.lines.eol_counts()
    }

    pub fn insert_eol(&self) -> Eol {
        self.lines.insert_eol()
    }

    pub fn set_insert_eol(&mut self, eol: Eol) {
        self.lines.set_insert_eol(eol);
    }

    /// The ending a line has; the last line has none.
    pub fn terminator(&self, line: usize) -> Eol {
        self.lines.terminator(line)
    }

    /// The whole document's bytes, or what stands in the way of them.
    pub fn encode(&self, format: &TextFormat, lossy: crate::codec::Lossy) -> Result<Vec<u8>, crate::codec::Problems> {
        self.lines.encode(format, lossy)
    }

    /// Rows of a fixed-width format that aren't the right width, which would shift everything after them.
    pub fn uneven_rows(&self) -> Vec<usize> {
        let crate::codec::NewlinePolicy::FixedRows { width, .. } = self.format.newline else {
            return Vec::new();
        };
        let last = self.lines.len().saturating_sub(1);
        (0..self.lines.len()).filter(|i| *i != last && self.lines.get(*i).chars().count() != width).take(200).collect()
    }

    fn is_rows(&self) -> bool {
        self.format.newline.is_rows() && !self.format.charset.holds_characters()
    }

    /// Changes a setting that affects saving, as one undoable step.
    pub fn change_format(&mut self, format: TextFormat) {
        self.push_undo(None);
        self.format = format;
        set_cell_profile(self.format.charset);
        self.edited();
    }

    /// Replaces the text with the same text in another encoding, as one undoable step.
    /// Line endings aren't touched; encoding alone never changes them.
    pub fn apply_conversion(&mut self, converted: Vec<String>, format: TextFormat) {
        let (caret, anchor) = (self.caret_char_position(), self.anchor_char_position());
        self.push_undo(None);
        for (i, line) in converted.into_iter().enumerate() {
            if i < self.lines.len() && *self.lines.get(i) != *line {
                self.lines.set(i, Arc::from(line));
            }
        }
        self.format = format;
        set_cell_profile(self.format.charset);
        self.after_tab_size_change(caret, anchor);
        self.edited();
    }

    /// Makes every line end the same way, as one undoable step. Not for a file kept on disk.
    pub fn convert_line_endings(&mut self, eol: Eol) -> bool {
        if self.lines.is_disk_backed() {
            return false;
        }
        self.push_undo(None);
        self.lines.convert_all_eols(eol);
        self.lines.note_changed(0);
        self.edited();
        true
    }

    /// Adds a line ending at the very end if there isn't one, for the option that asks for it.
    pub fn ensure_final_newline(&mut self) {
        if self.lines.get(self.lines.len() - 1).is_empty() || self.is_rows() {
            return;
        }
        self.push_undo(None);
        self.lines.push(Arc::from(""));
        self.edited();
    }

    /// Puts text in exactly as given, with nothing mapped, as one undoable action.
    pub fn insert_exact(&mut self, text: &str) {
        if text.is_empty() || self.block_too_tall() {
            return;
        }
        self.push_undo(None);
        if self.is_block() {
            self.type_in_block(text);
        } else {
            self.delete_stream_selection();
            if self.is_rows() {
                self.remove_cells_at_caret(text.chars().count());
            }
            self.insert_stream(text);
        }
        self.edited();
    }

    /// Makes room in a fixed-width row, so a new character replaces one rather than shifting the rest.
    fn remove_cells_at_caret(&mut self, count: usize) {
        let s = self.lines.get(self.caret_line).clone();
        let i = index_at_or_before(&s, self.caret_col);
        let end = (i + count).min(s.chars().count());
        if end > i {
            self.set_line(self.caret_line, splice(&s, i, end, ""));
        }
    }

    /// The first line changed since the colouring last looked.
    pub fn take_changed_from(&mut self) -> Option<usize> {
        self.lines.take_changed_from()
    }

    /// For when every line's text may differ, as when a file turns out not to be UTF-8.
    pub fn mark_all_changed(&mut self) {
        self.lines.note_changed(0);
    }

    pub fn store(&self) -> &LineStore {
        &self.lines
    }

    pub fn is_disk_backed(&self) -> bool {
        self.lines.is_disk_backed()
    }

    /// Shows a file that stays on disk, starting with what the scan has found so far.
    pub fn set_source(&mut self, source: Arc<Source>) {
        self.lines = LineStore::from_source(source);
        self.lines.refresh_from_source();
        self.caret_line = 0;
        self.caret_col = 0;
        self.anchor_line = 0;
        self.anchor_col = 0;
        self.history.clear();
        self.next_version += 1;
        self.version = self.next_version;
        self.saved_version = self.version;
        self.recalc_max();
    }

    /// Keeps up with the background scan of the file.
    pub fn refresh_from_source(&mut self) {
        self.lines.refresh_from_source();
        self.recalc_max();
    }

    /// Refuses a command that would touch too many lines of a file kept on disk.
    fn too_many_rows(&mut self, rows: usize) -> bool {
        if self.lines.is_disk_backed() && rows > MAX_ROWS_ON_DISK {
            self.notice = Some(format!(
                "That would change {rows} lines, which is too many in a file this large. Select fewer lines (at most {MAX_ROWS_ON_DISK})."
            ));
            return true;
        }
        false
    }

    /// Whether the selection is too big to put on the clipboard.
    pub fn selection_too_large_to_copy(&self) -> bool {
        self.lines.is_disk_backed() && self.caret_line.abs_diff(self.anchor_line) > MAX_COPY_ROWS
    }

    pub fn max_visual(&self) -> usize {
        self.max_visual
    }

    pub fn has_selection(&self) -> bool {
        self.caret_line != self.anchor_line || self.caret_col != self.anchor_col
    }

    pub fn is_block(&self) -> bool {
        self.column_mode || self.alt_block
    }

    pub fn column_mode(&self) -> bool {
        self.column_mode
    }

    pub fn is_modified(&self) -> bool {
        self.version != self.saved_version
    }

    pub fn can_undo(&self) -> bool {
        self.history.can_undo()
    }

    pub fn can_redo(&self) -> bool {
        self.history.can_redo()
    }

    pub fn set_column_mode(&mut self, on: bool) {
        if self.column_mode == on {
            return;
        }
        self.column_mode = on;
        self.alt_block = false;
        if !on {
            self.snap_caret();
            self.snap_anchor();
        }
        self.reveal = true;
    }

    pub fn set_content(&mut self, new_lines: Vec<String>) {
        let eol = self.format.preset_eol;
        self.adopt(LineStore::from_decoded(
            if new_lines.is_empty() { vec![Arc::from("")] } else { new_lines.into_iter().map(Arc::from).collect() },
            eol,
            std::collections::BTreeMap::new(),
        ));
    }

    /// Shows a file's lines, each with the ending it had.
    pub fn set_decoded(&mut self, decoded: Decoded) {
        let fallback = self.format.preset_eol;
        let insert = decoded.counts.predominant(fallback);
        let mut store = LineStore::from_decoded(decoded.lines.into_iter().map(Arc::from).collect(), decoded.base, decoded.exceptions);
        store.set_insert_eol(if decoded.base == Eol::None { Eol::None } else { insert });
        set_cell_profile(self.format.charset);
        self.adopt(store);
    }

    /// Shows a document brought back from a recovery copy, with the cursor where it was.
    pub fn adopt_recovered(&mut self, store: LineStore, caret: (usize, usize)) {
        set_cell_profile(self.format.charset);
        self.adopt(store);
        self.move_caret_to_char(caret.0, caret.1);
    }

    /// After saving over the file this view reads from: its lines are now read from the new file, so a
    /// recovery copy can refer to it. Undo keeps working, since it holds on to the old file's handle.
    pub fn rebase_on_source(&mut self, source: Arc<Source>) {
        let bookmarks = self.lines.bookmarks();
        let changes = self.lines.changed_lines().clone();
        self.lines = LineStore::from_source(source);
        self.lines.set_changes(changes);
        for line in bookmarks {
            self.lines.toggle_bookmark(line);
        }
        self.lines.refresh_from_source();
        self.lines.note_changed(0);
        self.recalc_max();
    }

    /// Carries on from a whole new version of the file, written out by a change too large to make in memory: the
    /// lines are read from it from now on, and one Undo goes back to how they were before.
    pub fn replace_with_source(&mut self, source: Arc<Source>, format: TextFormat) {
        self.push_undo(None);
        let bookmarks = self.lines.bookmarks();
        self.lines = LineStore::from_source(source);
        for line in bookmarks {
            self.lines.toggle_bookmark(line);
        }
        self.lines.refresh_from_source();
        self.format = format;
        self.lines.note_changed(0);
        let last = self.lines.len().saturating_sub(1);
        self.caret_line = self.caret_line.min(last);
        self.anchor_line = self.anchor_line.min(last);
        self.edited();
        self.recalc_max();
    }

    fn adopt(&mut self, store: LineStore) {
        self.multi.clear();
        self.lines = store;
        self.caret_line = 0;
        self.caret_col = 0;
        self.anchor_line = 0;
        self.anchor_col = 0;
        self.desired_col = 0;
        self.alt_block = false;
        self.history.clear();
        self.last_kind = None;
        self.next_version += 1;
        self.version = self.next_version;
        self.saved_version = self.version;
        self.recalc_max();
        self.reveal = true;
    }

    pub fn mark_saved(&mut self) {
        self.saved_version = self.version;
        self.lines.changes_saved();
    }

    /// Whether a line has changed since the file was opened, and if so whether that's been saved.
    pub fn change_at(&self, line: usize) -> Option<bool> {
        self.lines.change_at(line)
    }

    pub fn changed_line_count(&self) -> usize {
        self.lines.changed_lines().len()
    }

    /// Goes to the next or previous stretch of changed lines, wrapping round. Returns false if there are none.
    pub fn go_to_change(&mut self, forward: bool) -> bool {
        let changes = self.lines.changed_lines();
        let is_changed = |line: usize| changes.contains_key(&line);
        let run_start = |mut line: usize| {
            while line > 0 && is_changed(line - 1) {
                line -= 1;
            }
            line
        };
        let target = if forward {
            let mut past = self.caret_line;
            while is_changed(past) {
                past += 1;
            }
            changes.range(past..).next().or(changes.iter().next()).map(|(line, _)| *line)
        } else {
            let before = run_start(self.caret_line).saturating_sub(1);
            let here = if is_changed(self.caret_line) && run_start(self.caret_line) == 0 { None } else { changes.range(..=before).next_back().map(|(line, _)| *line) };
            here.filter(|line| *line < self.caret_line).or(changes.iter().next_back().map(|(line, _)| *line)).map(run_start)
        };
        let Some(line) = target else {
            return false;
        };
        self.move_caret(line, 0, false);
        self.desired_col = 0;
        true
    }

    pub fn position_text(&self) -> String {
        let mut s = format!("Line {}, Column {}", self.caret_line + 1 + self.line_offset, self.caret_col + 1);
        if self.has_multi() {
            s += &format!("   {} selections", self.multi.len());
        } else if self.has_selection() {
            if self.is_block() {
                s += &format!(
                    "   Block {} x {}",
                    self.caret_line.abs_diff(self.anchor_line) + 1,
                    self.caret_col.abs_diff(self.anchor_col)
                );
            } else {
                let rows = self.caret_line.abs_diff(self.anchor_line);
                if rows > 10_000 {
                    s += &format!("   Selected {} lines", rows + 1);
                } else {
                    let chars = self.selection_text().replace("\r\n", "\n").chars().count();
                    s += &format!("   Selected {chars} chars");
                }
            }
        }
        s
    }

    fn set_line(&mut self, i: usize, s: String) {
        if self.is_rows() {
            self.rows_touched = Some(self.rows_touched.map_or((i, i), |(a, b)| (a.min(i), b.max(i))));
        }
        self.lines.set(i, Arc::from(s));
    }

    /// The width of a fixed-row file's rows, and what fills an empty cell: a space in the file's own set.
    fn row_width(&self) -> Option<(usize, char)> {
        let crate::codec::NewlinePolicy::FixedRows { width, .. } = self.format.newline else {
            return None;
        };
        let pad = self.format.prepare_input(" ", 0).chars().next().unwrap_or(' ');
        self.is_rows().then_some((width, pad))
    }

    /// Brings a row an edit touched back to the width: a short one is filled with spaces, and a long one loses
    /// spaces from its end. Anything else that would make it too long is left for the check before saving.
    /// The last row is only filled if it was already whole, since a file can end with a short row of its own.
    fn fit_row(&mut self, r: usize) {
        let Some((width, pad)) = self.row_width() else {
            return;
        };
        if r >= self.lines.len() {
            return;
        }
        let s = self.lines.get(r).clone();
        let count = s.chars().count();
        if count < width && r + 1 < self.lines.len() {
            let filled: String = s.chars().chain(std::iter::repeat_n(pad, width - count)).collect();
            self.lines.set(r, Arc::from(filled));
        } else if count > width && s.chars().skip(width).all(|c| c == pad || c == ' ') {
            let trimmed: String = s.chars().take(width).collect();
            self.lines.set(r, Arc::from(trimmed));
        }
    }

    /// Types into fixed-width rows: it writes over cells, stops at the row's end, and each further line of what's
    /// typed or pasted writes over the next row down, so no row grows and none is added.
    fn type_in_rows(&mut self, text: &str) {
        let Some((width, _)) = self.row_width() else {
            return;
        };
        // Widths are counted in cells, the file's bytes, not screen columns: a control can be drawn wider than one.
        let parts = split_rows(text);
        let mut caret = (self.caret_line, self.caret_col);
        for (k, part) in parts.iter().enumerate() {
            let r = self.caret_line + k;
            if r >= self.lines.len() {
                break;
            }
            let s = self.lines.get(r).clone();
            let i = if k == 0 { index_at_or_before(&s, self.caret_col) } else { 0 };
            let taken: String = part.chars().take(width.saturating_sub(i)).collect();
            let end = (i + taken.chars().count()).min(s.chars().count());
            self.set_line(r, splice(&s, i, end, &taken));
            caret = (r, visual_of(&self.lines.get(r), i + taken.chars().count()));
        }
        (self.caret_line, self.caret_col) = caret;
        self.collapse_to_caret();
    }

    /// Takes out the cell before (or at) the cursor in a fixed-width row, closing up the rest and filling the end.
    fn delete_in_row(&mut self, forward: bool) {
        let Some((_, pad)) = self.row_width() else {
            return;
        };
        if self.has_selection() {
            self.push_undo(None);
            self.delete_stream_selection();
            self.edited();
            return;
        }
        if !forward && self.caret_col == 0 {
            // Rows are never joined: Backspace at the start of one just moves up.
            if self.caret_line > 0 {
                let above = self.caret_line - 1;
                let end = visual_len(&self.lines.get(above));
                self.move_caret(above, end, false);
            }
            return;
        }
        let s = self.lines.get(self.caret_line).clone();
        let here = index_at_or_before(&s, self.caret_col);
        let i = if forward { here } else { here.saturating_sub(1) };
        if i >= s.chars().count() || (!forward && here == 0) {
            return;
        }
        self.push_undo(Some(if forward { "del" } else { "back" }));
        let mut cells: Vec<char> = s.chars().collect();
        cells.remove(i);
        cells.push(pad);
        self.set_line(self.caret_line, cells.into_iter().collect());
        self.caret_col = visual_of(&self.lines.get(self.caret_line), i);
        self.collapse_to_caret();
        self.edited();
    }

    fn snap_caret(&mut self) {
        self.caret_line = self.caret_line.min(self.lines.len() - 1);
        let s = &self.lines.get(self.caret_line);
        self.caret_col = visual_of(s, index_at_or_before(s, self.caret_col));
    }

    fn snap_anchor(&mut self) {
        self.anchor_line = self.anchor_line.min(self.lines.len() - 1);
        let s = &self.lines.get(self.anchor_line);
        self.anchor_col = visual_of(s, index_at_or_before(s, self.anchor_col));
    }

    fn collapse_to_caret(&mut self) {
        self.anchor_line = self.caret_line;
        self.anchor_col = self.caret_col;
    }

    /// The character index exactly at a visual column of a line. A tab that straddles the
    /// column becomes spaces, and with pad set a short line is extended with spaces.
    fn index_for_edit(&mut self, line: usize, col: usize, pad: bool) -> usize {
        let s = self.lines.get(line).clone();
        let mut v = 0;
        let mut count = 0;
        for c in s.chars() {
            if v == col {
                return count;
            }
            let next = next_visual(v, c);
            if next > col && c != '\t' {
                return count;
            }
            if next > col {
                let spaces = " ".repeat(next - v);
                let rewritten = splice(&s, count, count + 1, &spaces);
                self.set_line(line, rewritten);
                return count + (col - v);
            }
            v = next;
            count += 1;
        }
        if v < col && pad {
            let mut padded = s.to_string();
            padded.extend(std::iter::repeat_n(' ', col - v));
            self.set_line(line, padded);
            return count + (col - v);
        }
        count
    }

    fn block_bounds(&self) -> (usize, usize, usize, usize) {
        (
            self.caret_line.min(self.anchor_line),
            self.caret_line.max(self.anchor_line),
            self.caret_col.min(self.anchor_col),
            self.caret_col.max(self.anchor_col),
        )
    }

    fn stream_bounds(&self) -> (usize, usize, usize, usize) {
        let caret = (self.caret_line, self.caret_col);
        let anchor = (self.anchor_line, self.anchor_col);
        let (a, b) = if caret < anchor { (caret, anchor) } else { (anchor, caret) };
        (a.0, a.1, b.0, b.1)
    }

    /// The selected visual columns of a row, for drawing.
    pub fn selection_on_row(&self, row: usize) -> Option<(usize, usize)> {
        if !self.has_selection() {
            return None;
        }
        if self.is_block() {
            let (top, bottom, left, right) = self.block_bounds();
            return (row >= top && row <= bottom && right > left).then_some((left, right));
        }
        let (sl, sc, el, ec) = self.stream_bounds();
        if row < sl || row > el {
            return None;
        }
        let left = if row == sl { sc } else { 0 };
        let right = if row == el { ec } else { visual_len(&self.lines.get(row)) + 1 };
        (right > left).then_some((left, right))
    }

    // ---------- Undo ----------

    fn push_undo(&mut self, kind: Option<&'static str>) {
        self.multi.clear();
        let now = Instant::now();
        let coalesce = kind.is_some() && kind == self.last_kind && now.duration_since(self.last_time).as_secs() < 2;
        self.last_kind = kind;
        self.last_time = now;
        if coalesce {
            self.history.set_dirty(true);
            return;
        }
        // One copy from before a script's first change is all that's needed, and saves copying for each of a million.
        if self.undo_group.is_some_and(|group| !group.keep_steps && group.held) {
            self.history.set_dirty(true);
            return;
        }
        let snapshot = self.snapshot();
        if self.history.is_clean_here() {
            // The text is still a state that's kept, so that serves as the state before this edit.
            self.history.refresh_here(snapshot);
            self.history.set_dirty(true);
        } else {
            let label = self.history_label();
            self.history.push(snapshot, label);
        }
        if let Some(group) = &mut self.undo_group {
            group.held = true;
            group.changed = true;
            group.base = group.base.or(self.history.at());
        }
        self.prune_history();
    }

    fn prune_history(&mut self) {
        let limit = if self.lines.memory_lines() > LARGE_LINE_COUNT { MAX_UNDO_LARGE } else { MAX_UNDO_SMALL };
        let group = self.undo_group.filter(|group| group.keep_steps).and_then(|group| Some((group.base?, group.first_new)));
        self.history.prune(limit, group);
    }

    /// What a state is listed as: where the cursor was, and what was on that line.
    fn history_label(&self) -> String {
        let line = self.format.readable_text(&self.lines.get(self.caret_line));
        let words: String = line.trim().chars().take(40).collect();
        format!("Line {}: {words}", self.caret_line + 1 + self.line_offset)
    }

    /// From here until the group ends, everything comes off in one Undo. Starting one that's going does nothing.
    pub fn begin_undo_group(&mut self, keep_steps: bool) {
        if self.undo_group.is_none() {
            self.undo_group = Some(UndoGroup { keep_steps, base: None, first_new: self.history.next_id(), held: false, changed: false });
            self.last_kind = None;
        }
    }

    /// Says whether anything happened during the group.
    pub fn end_undo_group(&mut self) -> bool {
        let Some(group) = self.undo_group.take() else {
            return false;
        };
        self.last_kind = None;
        if group.keep_steps
            && let Some(base) = group.base
        {
            self.history.squash_to(base, group.first_new);
            let same = self.history.node(base).is_some_and(|node| node.state.version == self.version);
            self.history.set_dirty(!same);
        }
        if group.changed {
            self.recalc_max();
        }
        group.changed
    }

    /// Keeps the text as it is now, before going to another state.
    fn leave_this_state(&mut self) {
        let snapshot = self.snapshot();
        let label = self.history_label();
        self.history.leave(snapshot, label);
        self.prune_history();
    }

    /// Every state kept, newest first, for the picture of the history.
    pub fn history_rows(&self) -> Vec<Row> {
        self.history.rows("Now")
    }

    /// Goes straight to any kept state. Where the text is now stays in the history, so nothing is lost.
    pub fn history_jump(&mut self, id: NodeId) {
        if self.history.at() == Some(id) && !self.history.is_dirty() {
            return;
        }
        self.leave_this_state();
        if let Some(state) = self.history.jump(id).cloned() {
            self.restore(state);
        }
    }

    /// The kept states that led to the text as it is, oldest first, each with when it was kept. The last is where the
    /// text is now unless it has moved on from it, which the second answer says.
    pub fn history_line(&self) -> (Vec<(NodeId, u64)>, bool) {
        let mut line = Vec::new();
        let mut at = self.history.at();
        while let Some(id) = at {
            let Some(node) = self.history.node(id) else {
                break;
            };
            line.push((id, node.when));
            at = node.parent;
        }
        line.reverse();
        (line, self.history.is_dirty() || self.history.at().is_none())
    }

    /// Puts these lines in place of some others, as one undoable step. Either can be none. Returns false if nothing
    /// changed.
    pub fn splice_lines(&mut self, start: usize, end: usize, rows: Vec<Arc<str>>) -> bool {
        let end = end.min(self.lines.len());
        let start = start.min(end);
        if self.lines.range_vec(start, end) == rows {
            return false;
        }
        self.push_undo(None);
        // A file always has at least one line, even if it's empty.
        let rows = if rows.is_empty() && start == 0 && end == self.lines.len() { vec![Arc::from("")] } else { rows };
        let count = rows.len();
        self.lines.replace_lines(start, end, rows);
        self.alt_block = false;
        // What went in is selected, so it can be seen.
        let last = self.lines.len().saturating_sub(1);
        (self.anchor_line, self.anchor_col) = (start.min(last), 0);
        if count == 0 {
            (self.caret_line, self.caret_col) = (self.anchor_line, 0);
        } else {
            self.caret_line = (start + count - 1).min(last);
            self.caret_col = visual_len(&self.lines.get(self.caret_line));
        }
        self.edited();
        true
    }

    /// The lines of a kept state.
    pub fn history_lines(&self, id: NodeId) -> Option<Vec<Arc<str>>> {
        let store = &self.history.node(id)?.state.lines;
        Some(store.range_vec(0, store.len()))
    }

    pub fn forget_history(&mut self) {
        self.history.clear();
        self.last_kind = None;
    }

    /// The history as a file can hold it, if it can. The text as it is becomes a state too, as it's the one the file
    /// now holds. A change of encoding is as far back as it goes, as the file only records lines.
    pub fn history_image(&mut self, file_hash: [u8; 32]) -> Option<history_file::Image> {
        if self.history.is_empty() || self.lines.is_disk_backed() || self.lines.len() > history_file::MOST_LINES {
            return None;
        }
        self.leave_this_state();
        let here = self.history.at()?;
        let fits = |state: &Snapshot| {
            let format = &state.format;
            !state.lines.is_disk_backed() && state.lines.len() <= history_file::MOST_LINES && format.charset.id == self.format.charset.id && format.newline.key() == self.format.newline.key() && format.state == self.format.state && format.has_bom == self.format.has_bom
        };
        let mut top = here;
        while let Some(parent) = self.history.node(top)?.parent {
            if !self.history.node(parent).is_some_and(|node| fits(&node.state)) {
                break;
            }
            top = parent;
        }
        let mut places: HashMap<NodeId, usize> = HashMap::new();
        let mut nodes = Vec::new();
        for node in self.history.nodes() {
            let parent = node.parent.and_then(|parent| places.get(&parent).copied());
            if !(node.id == top || parent.is_some()) || !fits(&node.state) {
                continue;
            }
            places.insert(node.id, nodes.len());
            let (base, insert, eols) = node.state.lines.eol_parts();
            nodes.push(history_file::NodeImage {
                parent,
                when: node.when,
                label: node.label.clone(),
                caret: (node.state.caret_line, node.state.caret_col),
                anchor: (node.state.anchor_line, node.state.anchor_col),
                lines: node.state.lines.range_vec(0, node.state.lines.len()),
                base,
                insert,
                eols,
            });
        }
        Some(history_file::Image { file_hash, current: *places.get(&here)?, nodes })
    }

    /// Takes up a history kept from before, as long as it ends with the text as it is now.
    pub fn load_history(&mut self, image: history_file::Image) -> bool {
        let Some(current) = image.nodes.get(image.current) else {
            return false;
        };
        if current.lines != self.lines.range_vec(0, self.lines.len()) {
            return false;
        }
        let mut nodes = Vec::with_capacity(image.nodes.len());
        for (index, node) in image.nodes.into_iter().enumerate() {
            let mut lines = LineStore::from_decoded(node.lines, node.base, node.eols);
            lines.set_insert_eol(node.insert);
            let version = if index == image.current {
                self.version
            } else {
                self.next_version += 1;
                self.next_version
            };
            let state = Snapshot { lines, format: self.format.clone(), caret_line: node.caret.0, caret_col: node.caret.1, anchor_line: node.anchor.0, anchor_col: node.anchor.1, version };
            nodes.push((node.parent, node.when, node.label, state));
        }
        self.history = UndoTree::rebuilt(nodes, image.current);
        let here = self.snapshot();
        self.history.refresh_here(here);
        true
    }

    // ---------- Narrowing ----------

    /// Some of these lines as a document of their own, to be put back later with `replace_region`.
    pub fn region_copy(&self, top: usize, bottom: usize) -> Self {
        let mut part = Self { format: self.format.clone(), ..Self::default() };
        set_cell_profile(part.format.charset);
        part.next_version = self.next_version;
        part.adopt(self.lines.extract(top, bottom));
        part.line_offset = self.line_offset + top;
        part.read_only = self.read_only;
        part.overwrite = self.overwrite;
        part.move_caret(self.caret_line.clamp(top, bottom) - top, self.caret_col, false);
        part
    }

    /// Puts a part back in place of these lines, as one step of this document's undo. If it's the same, that isn't a change.
    pub fn replace_region(&mut self, top: usize, bottom: usize, part: &Self) {
        if !self.lines.region_is_same(top, bottom, &part.lines) {
            self.push_undo(None);
            self.lines.splice_region(top, bottom, &part.lines);
            self.settle_cursor();
            self.edited();
        } else {
            self.lines.splice_region(top, bottom, &part.lines);
        }
    }

    /// All the lines as they'd be with a part put back, leaving this document as it is.
    pub fn store_with_region(&self, top: usize, bottom: usize, part: &Self) -> LineStore {
        let mut store = self.lines.clone();
        store.splice_region(top, bottom, &part.lines);
        store
    }

    /// Where the selection begins and ends, each as a line and a character along it.
    pub fn selection_char_bounds(&self) -> ((usize, usize), (usize, usize)) {
        let (start_line, start_col, end_line, end_col) = self.stream_bounds();
        let place = |line: usize, col: usize| (line, index_at_or_before(&self.lines.get(line), col));
        (place(start_line, start_col), place(end_line, end_col))
    }

    /// Goes to a line by the number shown beside it, which while narrowed is its number in the whole file.
    pub fn go_to_numbered(&mut self, place: &str) -> bool {
        match place.trim().parse::<usize>() {
            Ok(number) if number >= 1 && self.line_offset > 0 => self.go_to(&number.saturating_sub(self.line_offset).max(1).to_string()),
            _ => self.go_to(place),
        }
    }

    fn snapshot(&self) -> Snapshot {
        Snapshot {
            lines: self.lines.clone(),
            format: self.format.clone(),
            caret_line: self.caret_line,
            caret_col: self.caret_col,
            anchor_line: self.anchor_line,
            anchor_col: self.anchor_col,
            version: self.version,
        }
    }

    fn restore(&mut self, s: Snapshot) {
        self.multi.clear();
        self.lines = s.lines;
        self.format = s.format;
        set_cell_profile(self.format.charset);
        self.lines.note_changed(0);
        self.caret_line = s.caret_line;
        self.caret_col = s.caret_col;
        self.anchor_line = s.anchor_line;
        self.anchor_col = s.anchor_col;
        self.keep_in_bounds();
        self.desired_col = self.caret_col;
        self.version = s.version;
        self.last_kind = None;
        self.recalc_max();
        self.reveal = true;
    }

    pub fn undo(&mut self) {
        if !self.history.can_undo() {
            return;
        }
        self.leave_this_state();
        if let Some(state) = self.history.undo().cloned() {
            self.restore(state);
        }
        if let Some(group) = &mut self.undo_group {
            group.held = false;
        }
    }

    pub fn redo(&mut self) {
        if !self.history.can_redo() {
            return;
        }
        self.leave_this_state();
        if let Some(state) = self.history.redo().cloned() {
            self.restore(state);
        }
    }

    /// However a command left them, the cursor and the selection's other end stay on real lines.
    fn keep_in_bounds(&mut self) {
        let last = self.lines.len().saturating_sub(1);
        self.caret_line = self.caret_line.min(last);
        self.anchor_line = self.anchor_line.min(last);
    }

    fn edited(&mut self) {
        if let Some((first, last)) = self.rows_touched.take() {
            for r in first..=last {
                self.fit_row(r);
            }
        }
        self.keep_in_bounds();
        self.next_version += 1;
        self.version = self.next_version;
        self.desired_col = self.caret_col;
        // Looking through every line after every one of a script's changes would make long scripts crawl. It's done once at the end.
        if self.undo_group.is_none() {
            self.recalc_max();
        }
        self.reveal = true;
    }

    fn recalc_max(&mut self) {
        if self.lines.is_disk_backed() {
            // Close enough for the scrollbar, and it doesn't read the file.
            self.max_visual = self.lines.longest_estimate();
            return;
        }
        let mut m = 0;
        for line in self.lines.range_vec(0, self.lines.len()) {
            // A character is at most one tab's worth of cells and at least one byte, so this skips most scans.
            if line.len() * tab_size() > m {
                m = m.max(visual_len(&line));
            }
        }
        self.max_visual = m;
    }

    // ---------- Editing ----------

    /// Types text as the keyboard would: in every row of a column block, or at the caret.
    pub fn type_text(&mut self, text: &str) {
        if text.is_empty() || self.block_too_tall() {
            return;
        }
        let prepared = self.format.prepare_input(text, self.input_table());
        let text = prepared.as_str();
        if self.has_multi() && !text.contains(['\r', '\n']) {
            let kind = if text.chars().count() == 1 { Some("type") } else { None };
            self.multi_edit(kind, |spot, _, _| (spot.start(), spot.end(), text.to_string()));
            return;
        }
        self.push_undo(if text.chars().count() == 1 { Some("type") } else { None });
        if self.is_block() {
            self.type_in_block(text);
        } else {
            self.delete_stream_selection();
            if self.is_rows() {
                self.type_in_rows(text);
                self.edited();
                return;
            } else if self.overwrite {
                let s = self.lines.get(self.caret_line).clone();
                let i = index_at_or_before(&s, self.caret_col);
                if i < s.chars().count() {
                    self.set_line(self.caret_line, splice(&s, i, i + 1, ""));
                }
            }
            self.insert_stream(text);
        }
        self.edited();
    }

    fn type_in_block(&mut self, text: &str) {
        let (top, bottom, left, right) = self.block_bounds();
        let width = visual_len(text);
        for r in top..=bottom {
            if right > left {
                self.remove_block_cells(r, left, right);
            }
            let i = self.index_for_edit(r, left, true);
            let s = self.lines.get(r).clone();
            let end = if self.overwrite && right == left { (i + text.chars().count()).min(s.chars().count()) } else { i };
            self.set_line(r, splice(&s, i, end, text));
        }
        self.caret_col = left + width;
        self.anchor_col = self.caret_col;
    }

    fn remove_block_cells(&mut self, line: usize, left: usize, right: usize) {
        if visual_len(&self.lines.get(line)) <= left {
            return;
        }
        let a = self.index_for_edit(line, left, false);
        let b = self.index_for_edit(line, right, false);
        if b > a {
            let s = self.lines.get(line).clone();
            self.set_line(line, splice(&s, a, b, ""));
        }
    }

    fn delete_stream_selection(&mut self) {
        if !self.has_selection() {
            return;
        }
        let (sl, sc, el, ec) = self.stream_bounds();
        if let Some((width, pad)) = self.row_width() {
            // In fixed-width rows a selection is blanked, cell for cell, so nothing moves.
            for r in sl..=el {
                let s = self.lines.get(r).clone();
                let from = if r == sl { index_at_or_before(&s, sc) } else { 0 };
                let to = if r == el { index_at_or_before(&s, ec) } else { s.chars().count().max(width) };
                let cells: String = s.chars().enumerate().map(|(i, c)| if i >= from && i < to { pad } else { c }).collect();
                self.set_line(r, cells);
            }
            self.caret_line = sl;
            self.caret_col = sc;
            self.collapse_to_caret();
            return;
        }
        let first = self.lines.get(sl).clone();
        let last = self.lines.get(el).clone();
        let a = index_at_or_before(&first, sc);
        let b = index_at_or_before(&last, ec);
        if a == 0 && b == 0 && el > sl {
            // Whole lines: remove them outright, so their bookmarks go with them and the next line stays as it is.
            self.lines.remove_range(sl, el);
            self.caret_line = sl;
            self.caret_col = 0;
            self.collapse_to_caret();
            return;
        }
        let joined = format!("{}{}", &first[..byte_of(&first, a)], &last[byte_of(&last, b)..]);
        self.set_line(sl, joined);
        if el > sl {
            self.lines.take_terminator_from(sl, el);
            self.lines.remove_range(sl + 1, el + 1);
        }
        self.caret_line = sl;
        self.caret_col = visual_of(&self.lines.get(sl), a);
        self.collapse_to_caret();
    }

    fn insert_stream(&mut self, text: &str) {
        let parts = split_rows(text);
        let line = self.lines.get(self.caret_line).clone();
        let i = index_at_or_before(&line, self.caret_col);
        let split = byte_of(&line, i);
        let (head, tail) = line.split_at(split);
        if parts.len() == 1 {
            self.set_line(self.caret_line, format!("{head}{}{tail}", parts[0]));
            self.caret_col = visual_of(&self.lines.get(self.caret_line), i + parts[0].chars().count());
        } else {
            let last = &parts[parts.len() - 1];
            let last_chars = last.chars().count();
            let mut rows: Vec<Arc<str>> = Vec::with_capacity(parts.len() - 1);
            for p in &parts[1..parts.len() - 1] {
                rows.push(Arc::from(p.as_str()));
            }
            rows.push(Arc::from(format!("{last}{tail}")));
            self.set_line(self.caret_line, format!("{head}{}", parts[0]));
            let at = self.caret_line + 1;
            self.lines.insert_many(at, rows);
            self.lines.split_terminator(self.caret_line, parts.len() - 1);
            self.caret_line += parts.len() - 1;
            self.caret_col = visual_of(&self.lines.get(self.caret_line), last_chars);
        }
        self.collapse_to_caret();
    }

    /// A column command touches every row of the block, so a very tall one is refused.
    fn block_too_tall(&mut self) -> bool {
        self.is_block() && self.too_many_rows(self.caret_line.abs_diff(self.anchor_line) + 1)
    }

    pub fn backspace(&mut self) {
        if self.block_too_tall() {
            return;
        }
        if self.is_rows() && !self.has_multi() && !self.is_block() {
            return self.delete_in_row(false);
        }
        if self.has_multi() {
            self.multi_edit(Some("back"), |spot, _, _| if spot.anchor == spot.head { (spot.head.saturating_sub(1), spot.head, String::new()) } else { (spot.start(), spot.end(), String::new()) });
            return;
        }
        if self.is_block() {
            let (top, bottom, left, right) = self.block_bounds();
            if right > left {
                self.delete_block_selection();
                return;
            }
            if left == 0 {
                if top == bottom && top > 0 {
                    self.join_with_previous();
                }
                return;
            }
            self.push_undo(Some("back"));
            for r in top..=bottom {
                if visual_len(&self.lines.get(r)) >= left {
                    let a = self.index_for_edit(r, left - 1, false);
                    let b = self.index_for_edit(r, left, false);
                    if b > a {
                        let s = self.lines.get(r).clone();
                        self.set_line(r, splice(&s, a, b, ""));
                    }
                }
            }
            self.caret_col = left - 1;
            self.anchor_col = left - 1;
            self.edited();
            return;
        }
        if self.has_selection() {
            self.push_undo(None);
            self.delete_stream_selection();
            self.edited();
            return;
        }
        let s = self.lines.get(self.caret_line).clone();
        let i = index_at_or_before(&s, self.caret_col);
        if i == 0 {
            if self.caret_line > 0 {
                self.join_with_previous();
            }
            return;
        }
        self.push_undo(Some("back"));
        self.set_line(self.caret_line, splice(&s, i - 1, i, ""));
        self.caret_col = visual_of(&self.lines.get(self.caret_line), i - 1);
        self.collapse_to_caret();
        self.edited();
    }

    fn join_with_previous(&mut self) {
        self.push_undo(None);
        let col = visual_len(&self.lines.get(self.caret_line - 1));
        let joined = format!("{}{}", self.lines.get(self.caret_line - 1), self.lines.get(self.caret_line));
        self.set_line(self.caret_line - 1, joined);
        self.lines.take_terminator_from(self.caret_line - 1, self.caret_line);
        self.lines.remove(self.caret_line);
        self.caret_line -= 1;
        self.caret_col = col;
        self.collapse_to_caret();
        self.edited();
    }

    pub fn delete_forward(&mut self) {
        if self.block_too_tall() {
            return;
        }
        if self.is_rows() && !self.has_multi() && !self.is_block() {
            return self.delete_in_row(true);
        }
        if self.has_multi() {
            self.multi_edit(Some("del"), |spot, _, chars| if spot.anchor == spot.head { (spot.head, (spot.head + 1).min(chars.len()), String::new()) } else { (spot.start(), spot.end(), String::new()) });
            return;
        }
        if self.is_block() {
            let (top, bottom, left, right) = self.block_bounds();
            if right > left {
                self.delete_block_selection();
                return;
            }
            self.push_undo(Some("del"));
            for r in top..=bottom {
                if visual_len(&self.lines.get(r)) > left {
                    let a = self.index_for_edit(r, left, false);
                    let s = self.lines.get(r).clone();
                    if a < s.chars().count() {
                        self.set_line(r, splice(&s, a, a + 1, ""));
                    }
                }
            }
            self.edited();
            return;
        }
        if self.has_selection() {
            self.push_undo(None);
            self.delete_stream_selection();
            self.edited();
            return;
        }
        let s = self.lines.get(self.caret_line).clone();
        let i = index_at_or_before(&s, self.caret_col);
        if i < s.chars().count() {
            self.push_undo(Some("del"));
            self.set_line(self.caret_line, splice(&s, i, i + 1, ""));
            self.edited();
        } else if self.caret_line + 1 < self.lines.len() {
            self.push_undo(None);
            let joined = format!("{s}{}", self.lines.get(self.caret_line + 1));
            self.set_line(self.caret_line, joined);
            self.lines.take_terminator_from(self.caret_line, self.caret_line + 1);
            self.lines.remove(self.caret_line + 1);
            self.edited();
        }
    }

    fn delete_block_selection(&mut self) {
        let (top, bottom, left, right) = self.block_bounds();
        self.push_undo(None);
        for r in top..=bottom {
            self.remove_block_cells(r, left, right);
        }
        self.caret_col = left;
        self.anchor_col = left;
        self.edited();
    }

    pub fn new_line(&mut self) {
        if self.is_rows() {
            // Fixed-width rows have nothing between them, so Enter only moves down a row.
            let next = (self.caret_line + 1).min(self.lines.len() - 1);
            self.move_caret(next, 0, false);
            self.set_desired_col();
            return;
        }
        self.push_undo(None);
        if self.is_block() {
            self.collapse_to_caret();
            self.alt_block = false;
        } else {
            self.delete_stream_selection();
        }
        let i = self.index_for_edit(self.caret_line, self.caret_col, false);
        let s = self.lines.get(self.caret_line).clone();
        let indent_chars = if edit_prefs().auto_indent { s.chars().take_while(|c| *c == ' ' || *c == '\t').count().min(i) } else { 0 };
        let indent = s[..byte_of(&s, indent_chars)].to_string();
        let split = byte_of(&s, i);
        // Splitting the last line of a tagged section keeps both halves in it. Enter at its very end starts a line
        // after it instead, outside.
        let tag = self.lines.tag_at(self.caret_line).filter(|(_, last, _)| *last == self.caret_line && split < s.len());
        self.set_line(self.caret_line, s[..split].to_string());
        self.lines.insert(self.caret_line + 1, Arc::from(format!("{indent}{}", &s[split..])));
        self.lines.split_terminator(self.caret_line, 1);
        if let Some((first, last, colour)) = tag {
            self.lines.set_tag(first, last + 1, colour);
        }
        self.caret_line += 1;
        self.caret_col = visual_of(&self.lines.get(self.caret_line), indent_chars);
        self.collapse_to_caret();
        self.edited();
    }

    pub fn tab_key(&mut self, shift: bool) {
        if self.has_multi() {
            self.multi_tab(shift);
            return;
        }
        let multi = self.has_selection() && self.caret_line != self.anchor_line && !self.is_block();
        if multi || shift {
            let top = self.caret_line.min(self.anchor_line);
            let bottom = self.caret_line.max(self.anchor_line);
            if self.too_many_rows(bottom - top + 1) {
                return;
            }
            self.push_undo(None);
            for r in top..=bottom {
                let s = self.lines.get(r).clone();
                let updated = if !shift && edit_prefs().spaces_for_tab {
                    format!("{}{s}", " ".repeat(tab_size()))
                } else if !shift {
                    format!("\t{s}")
                } else if let Some(rest) = s.strip_prefix('\t') {
                    rest.to_string()
                } else {
                    let n = s.chars().take(tab_size()).take_while(|c| *c == ' ').count();
                    s[n..].to_string()
                };
                self.set_line(r, updated);
            }
            self.snap_caret();
            self.snap_anchor();
            self.edited();
            return;
        }
        if self.is_block() || edit_prefs().spaces_for_tab {
            let col = self.caret_col.min(self.anchor_col);
            self.type_text(&" ".repeat(tab_size() - (col % tab_size())));
        } else {
            self.type_text("\t");
        }
    }

    // ---------- Clipboard ----------

    /// With the whole-line setting on and nothing selected, selects this line and its ending, and says so.
    pub fn select_line_if_none(&mut self) -> bool {
        if !edit_prefs().whole_line_copy || self.has_selection() || self.is_block() {
            return false;
        }
        let line = self.caret_line;
        let (end_line, end_col) = if line + 1 < self.lines.len() { (line + 1, 0) } else { (line, visual_len(&self.lines.get(line))) };
        if (end_line, end_col) == (line, 0) {
            return false;
        }
        (self.anchor_line, self.anchor_col) = (line, 0);
        (self.caret_line, self.caret_col) = (end_line, end_col);
        true
    }

    pub fn selection_text(&self) -> String {
        if !self.has_selection() {
            return String::new();
        }
        if self.is_block() {
            let (top, bottom, left, right) = self.block_bounds();
            let rows: Vec<String> = (top..=bottom).map(|r| block_cell(&self.lines.get(r), left, right)).collect();
            return rows.join("\r\n");
        }
        let (sl, sc, el, ec) = self.stream_bounds();
        let first = &self.lines.get(sl);
        let last = &self.lines.get(el);
        let a = byte_of(first, index_at_or_before(first, sc));
        let b = byte_of(last, index_at_or_before(last, ec));
        if sl == el {
            return first[a..b].to_string();
        }
        let mut out = first[a..].to_string();
        for r in sl + 1..el {
            out.push_str("\r\n");
            out.push_str(&self.lines.get(r));
        }
        out.push_str("\r\n");
        out.push_str(&last[..b]);
        out
    }

    /// Removes the selection, for Cut.
    pub fn delete_selection(&mut self) {
        if !self.has_selection() || self.block_too_tall() {
            return;
        }
        if self.is_block() {
            self.delete_block_selection();
        } else {
            self.push_undo(None);
            self.delete_stream_selection();
            self.edited();
        }
    }

    /// Pastes text; a column block keeps its shape and goes in as columns.
    pub fn paste_text(&mut self, text: &str, as_block: bool) {
        if text.is_empty() || self.block_too_tall() {
            return;
        }
        let prepared = self.format.prepare_input(text, self.input_table());
        let text = prepared.as_str();
        if self.has_multi() {
            self.multi_paste(text);
            return;
        }
        self.push_undo(None);
        if self.is_block() || as_block {
            self.paste_as_block(&split_rows(text));
        } else {
            self.delete_stream_selection();
            self.insert_stream(text);
        }
        self.edited();
    }

    fn paste_as_block(&mut self, rows: &[String]) {
        let (top, bottom, left, right) = self.block_bounds();
        // Outside column mode a pasted block replaces a selection as a whole, then goes in at its start.
        if !self.is_block() && self.has_selection() {
            let (sl, sc, _, _) = self.stream_bounds();
            self.delete_stream_selection();
            self.caret_line = sl;
            self.caret_col = sc;
            self.collapse_to_caret();
            self.insert_block_rows(rows, sl, sl, sc);
            return;
        }
        if right > left {
            for r in top..=bottom {
                self.remove_block_cells(r, left, right);
            }
        }
        self.insert_block_rows(rows, top, bottom, left);
    }

    fn insert_block_rows(&mut self, rows: &[String], top: usize, bottom: usize, left: usize) {
        let width = rows.iter().map(|r| visual_len(r)).max().unwrap_or(0);
        if rows.len() == 1 && bottom > top {
            // One line into several selected rows repeats it on each row.
            for r in top..=bottom {
                self.insert_cell(r, left, &rows[0]);
            }
            self.caret_col = left + visual_len(&rows[0]);
            self.anchor_col = self.caret_col;
            return;
        }
        while top + rows.len() > self.lines.len() {
            self.lines.push(Arc::from(""));
        }
        for (i, row) in rows.iter().enumerate() {
            let r = top + i;
            let has_tail = visual_len(&self.lines.get(r)) > left;
            let mut cell = row.clone();
            if has_tail {
                cell.extend(std::iter::repeat_n(' ', width - visual_len(row)));
            }
            self.insert_cell(r, left, &cell);
        }
        self.caret_line = top;
        self.anchor_line = top;
        self.caret_col = left + width;
        self.anchor_col = self.caret_col;
    }

    fn insert_cell(&mut self, line: usize, col: usize, text: &str) {
        let i = self.index_for_edit(line, col, true);
        let s = self.lines.get(line).clone();
        self.set_line(line, splice(&s, i, i, text));
    }

    // ---------- Column commands ----------

    /// Rows a column command applies to: the selected rows, or the caret's row to the end.
    fn column_rows(&self) -> (usize, usize, usize, usize) {
        let (top, mut bottom, left, right) = self.block_bounds();
        if top == bottom {
            bottom = self.lines.len() - 1;
        }
        (top, bottom, left, right)
    }

    /// Inserts text at the block's left edge of each row, replacing what the block covers.
    /// With skip_empty_last, an empty line at the very end of the file is left alone, unless it's the only row.
    pub fn insert_column_fill(&mut self, text: &str, skip_empty_last: bool) {
        let (top, mut bottom, left, right) = self.column_rows();
        if skip_empty_last && bottom > top && bottom == self.lines.len() - 1 && self.lines.get(bottom).is_empty() {
            bottom -= 1;
        }
        if self.too_many_rows(bottom - top + 1) {
            return;
        }
        self.push_undo(None);
        for r in top..=bottom {
            if right > left && self.is_block() {
                self.remove_block_cells(r, left, right);
            }
            self.insert_cell(r, left, text);
        }
        self.caret_col = left + visual_len(text);
        self.anchor_col = self.caret_col;
        self.edited();
    }

    /// Selects the columns that Cut columns or Delete columns will take: a number of them from the
    /// cursor down, or with 0 just what's highlighted. Says why not if there's nothing to take.
    pub fn select_columns(&mut self, count: usize) -> Result<(), String> {
        let (top, bottom, left, right) = if count == 0 {
            let bounds = self.block_bounds();
            if !self.has_selection() || bounds.3 == bounds.2 {
                return Err("Highlight the columns first, or enter how many to take.".to_string());
            }
            bounds
        } else {
            let (top, bottom, left, _) = self.column_rows();
            (top, bottom, left, left.saturating_add(count))
        };
        if self.too_many_rows(bottom - top + 1) {
            return Err(self.notice.take().unwrap_or_default());
        }
        // No wider than the text there, so that a huge number doesn't copy a huge block of spaces.
        let widest = self.lines.range_vec(top, bottom + 1).iter().map(|line| visual_len(line)).max().unwrap_or(0);
        let right = right.min(widest);
        if right <= left {
            return Err("There's no text in those columns.".to_string());
        }
        self.alt_block = !self.column_mode;
        (self.anchor_line, self.anchor_col) = (top, left);
        (self.caret_line, self.caret_col) = (bottom, right);
        self.last_kind = None;
        Ok(())
    }

    /// Removes the columns just selected, leaving the cursor where they started on the first row.
    pub fn remove_selected_columns(&mut self) {
        let (top, _, left, _) = self.block_bounds();
        self.delete_selection();
        self.alt_block = false;
        (self.caret_line, self.caret_col) = (top, left);
        self.collapse_to_caret();
        self.desired_col = left;
    }

    /// Lines up the text in each row of the highlighted columns. The block stays the same size, and
    /// anything outside it stays where it is. Rows the block doesn't reach, or that are blank in it, are left alone.
    pub fn justify_columns(&mut self, how: Justify) {
        let (top, bottom, left, right) = self.block_bounds();
        if !self.has_selection() || right == left {
            self.notice = Some("Highlight the columns to justify first.".to_string());
            return;
        }
        if self.too_many_rows(bottom - top + 1) {
            return;
        }
        let width = right - left;
        let mut changes = Vec::new();
        for row in top..=bottom {
            let line = self.lines.get(row).clone();
            let length = visual_len(&line);
            if length <= left {
                continue;
            }
            let cell = block_cell(&line, left, right);
            let text = cell.trim_matches(' ');
            if text.is_empty() {
                continue;
            }
            let spare = width.saturating_sub(visual_len(text));
            let before = match how {
                Justify::Left => 0,
                Justify::Center => spare / 2,
                Justify::Right => spare,
            };
            let after = spare - before;
            if format!("{}{text}{}", " ".repeat(before), " ".repeat(after)) == cell {
                continue;
            }
            // With nothing to the right of the block, no spaces are left trailing on the line.
            let tail = if length > right { " ".repeat(after) } else { String::new() };
            changes.push((row, format!("{}{text}{tail}", " ".repeat(before))));
        }
        if changes.is_empty() {
            return;
        }
        self.push_undo(None);
        for (row, cell) in changes {
            self.remove_block_cells(row, left, right);
            self.insert_cell(row, left, &cell);
        }
        self.edited();
    }

    /// The rows CSV convert works on: the selected lines, or the whole file without an empty last line.
    /// None, with a notice, if that's too many for a file kept on disk.
    pub fn rows_for_convert(&mut self) -> Option<(usize, usize)> {
        let (top, bottom) = self.convert_bounds();
        (!self.too_many_rows(bottom - top + 1)).then_some((top, bottom))
    }

    /// The selected lines, or every line of the file but an empty last one, however many there are.
    pub fn convert_bounds(&self) -> (usize, usize) {
        let last = self.lines.len() - 1;
        if !self.has_selection() {
            (0, if last > 0 && self.lines.get(last).is_empty() { last - 1 } else { last })
        } else if self.is_block() {
            let (top, bottom, _, _) = self.block_bounds();
            (top, bottom)
        } else {
            let (first, _, end, end_col) = self.stream_bounds();
            (first, if end_col == 0 && end > first { end - 1 } else { end })
        }
    }

    /// The cursor's paragraph, or the selected lines if there are any. None on a blank line.
    pub fn rows_for_paragraph(&mut self) -> Option<(usize, usize)> {
        let (mut top, mut bottom) = if self.has_selection() {
            self.selected_rows()
        } else {
            if self.lines.get(self.caret_line).trim().is_empty() {
                return None;
            }
            (self.caret_line, self.caret_line)
        };
        if !self.has_selection() {
            let limit = MAX_ROWS_ON_DISK;
            while top > 0 && bottom - top < limit && !self.lines.get(top - 1).trim().is_empty() {
                top -= 1;
            }
            while bottom + 1 < self.lines.len() && bottom - top < limit && !self.lines.get(bottom + 1).trim().is_empty() {
                bottom += 1;
            }
        }
        (!self.too_many_rows(bottom - top + 1)).then_some((top, bottom))
    }

    /// Replaces whole lines as one undoable step, and selects the result. Lines that come out the same stay
    /// untouched, with their marks and bookmarks. Returns false if nothing changed.
    pub fn replace_lines(&mut self, top: usize, bottom: usize, rows: Vec<String>) -> bool {
        if rows.is_empty() {
            return false;
        }
        let old = self.lines.range_vec(top, bottom + 1);
        let same_start = old.iter().zip(&rows).take_while(|(a, b)| &***a == b.as_str()).count();
        if same_start == old.len() && same_start == rows.len() {
            return false;
        }
        let room = old.len().min(rows.len()) - same_start;
        let same_end = old.iter().rev().zip(rows.iter().rev()).take(room).take_while(|(a, b)| &***a == b.as_str()).count();
        let count = rows.len();
        let changed: Vec<Arc<str>> = rows[same_start..count - same_end].iter().map(|row| Arc::from(row.as_str())).collect();
        self.push_undo(None);
        self.lines.replace_lines(top + same_start, bottom + 1 - same_end, changed);
        self.alt_block = false;
        let last = top + count - 1;
        (self.anchor_line, self.anchor_col) = (top, 0);
        (self.caret_line, self.caret_col) = (last, visual_len(&self.lines.get(last)));
        self.edited();
        true
    }

    /// Selects from one place to another, each given as a line and a character along it.
    pub fn select_chars(&mut self, from: (usize, usize), to: (usize, usize)) {
        let places = [from, to].map(|(line, index)| (line, visual_of(&self.lines.get(line), index)));
        self.multi.clear();
        self.set_selection(places[0], places[1], false);
        self.last_kind = None;
        self.reveal = true;
    }

    /// After a change to other lines, the cursor is still on a real place.
    fn settle_cursor(&mut self) {
        self.keep_in_bounds();
        self.snap_caret();
        self.snap_anchor();
        self.alt_block = false;
        self.multi.clear();
    }

    /// Changes one line, which can become several, keeping its line ending and leaving the cursor where it is.
    pub fn set_line_keeping_cursor(&mut self, line: usize, mut rows: Vec<String>) {
        if rows.len() == 1 && *self.lines.get(line) == *rows[0] {
            return;
        }
        self.push_undo(None);
        if rows.len() == 1 {
            self.set_line(line, rows.remove(0));
        } else {
            self.lines.replace_lines(line, line + 1, rows.into_iter().map(Arc::from).collect());
        }
        self.settle_cursor();
        self.edited();
    }

    /// Puts new lines in front of a line, or after the last, and leaves the cursor on the same text.
    pub fn insert_lines_keeping_cursor(&mut self, line: usize, rows: Vec<String>) {
        let count = rows.len();
        self.push_undo(None);
        if line >= self.lines.len() {
            // The last line had no ending of its own to give, so the new ones go after it with the file's usual one.
            self.lines.replace_range(line, line, rows.into_iter().map(Arc::from).collect());
        } else {
            self.lines.replace_range(line, line, rows.into_iter().map(Arc::from).collect());
            if self.caret_line >= line {
                self.caret_line += count;
            }
            if self.anchor_line >= line {
                self.anchor_line += count;
            }
        }
        self.settle_cursor();
        self.edited();
    }

    /// Removes one line. The only line left in a file is emptied instead, as a file always has one.
    pub fn delete_line_keeping_cursor(&mut self, line: usize) {
        if self.lines.len() == 1 {
            if !self.lines.get(0).is_empty() {
                self.push_undo(None);
                self.set_line(0, String::new());
                self.settle_cursor();
                self.edited();
            }
            return;
        }
        self.push_undo(None);
        self.lines.replace_range(line, line + 1, Vec::new());
        if self.caret_line > line {
            self.caret_line -= 1;
        }
        if self.anchor_line > line {
            self.anchor_line -= 1;
        }
        self.settle_cursor();
        self.edited();
    }

    /// The text of some rows, as they stand.
    pub fn rows_text(&self, top: usize, bottom: usize) -> Vec<String> {
        self.lines.range_vec(top, bottom + 1).iter().map(|line| line.to_string()).collect()
    }

    /// Puts new text on rows in place, as one undoable step. Line endings and bookmarks stay as they are.
    pub fn replace_rows(&mut self, top: usize, rows: Vec<String>) {
        if rows.iter().enumerate().all(|(i, row)| *row == *self.lines.get(top + i)) {
            return;
        }
        self.push_undo(None);
        for (i, row) in rows.into_iter().enumerate() {
            self.set_line(top + i, row);
        }
        self.alt_block = false;
        (self.caret_line, self.caret_col) = (top, 0);
        self.collapse_to_caret();
        self.edited();
    }

    /// Inserts a right-aligned counting number at the block's left edge of each row.
    /// Returns false, changing nothing, if the numbers would overflow.
    pub fn insert_column_numbers(&mut self, start: i64, step: i64) -> bool {
        let (top, bottom, left, right) = self.column_rows();
        if self.too_many_rows(bottom - top + 1) {
            return true;
        }
        let mut numbers = Vec::with_capacity(bottom - top + 1);
        for r in top..=bottom {
            let Some(n) = i64::try_from(r - top).ok().and_then(|i| i.checked_mul(step)).and_then(|d| d.checked_add(start)) else {
                return false;
            };
            numbers.push(n.to_string());
        }
        let width = numbers.iter().map(String::len).max().unwrap_or(0);
        self.push_undo(None);
        for (i, number) in numbers.iter().enumerate() {
            let r = top + i;
            if right > left && self.is_block() {
                self.remove_block_cells(r, left, right);
            }
            self.insert_cell(r, left, &format!("{number:>width$}"));
        }
        self.caret_col = left + width;
        self.anchor_col = self.caret_col;
        self.edited();
        true
    }

    /// Adds up the numbers in the selected block, and counts the cells it could not read.
    pub fn sum_column(&self) -> ColumnSum {
        let (top, bottom, left, right) = self.block_bounds();
        let mut result = ColumnSum { sum: 0.0, numbers: 0, skipped: 0, decimals: 0 };
        for r in top..=bottom {
            let cell = block_cell(&self.lines.get(r), left, right);
            let cell = cell.trim();
            if cell.is_empty() {
                continue;
            }
            let plain = cell.replace(',', "");
            match plain.parse::<f64>() {
                Ok(value) if value.is_finite() => {
                    result.sum += value;
                    result.numbers += 1;
                    if let Some((_, frac)) = plain.split_once('.') {
                        result.decimals = result.decimals.max(frac.chars().take_while(char::is_ascii_digit).count());
                    }
                }
                _ => result.skipped += 1,
            }
        }
        result
    }

    pub fn change_case(&mut self, change: CaseChange) {
        let rows = self.caret_line.abs_diff(self.anchor_line) + 1;
        if !self.has_selection() || self.too_many_rows(rows) {
            return;
        }
        self.push_undo(None);
        if self.is_block() {
            let (top, bottom, left, right) = self.block_bounds();
            for r in top..=bottom {
                if visual_len(&self.lines.get(r)) <= left {
                    continue;
                }
                let a = self.index_for_edit(r, left, false);
                let b = self.index_for_edit(r, right, false);
                let s = self.lines.get(r).clone();
                let (ba, bb) = (byte_of(&s, a), byte_of(&s, b));
                // Each row of a block is a thing of its own, such as a cell in a column.
                let updated = splice(&s, a, b, &convert_case(&s[ba..bb], change, &mut CaseState::new()));
                self.set_line(r, updated);
            }
        } else {
            let (sl, sc, el, ec) = self.stream_bounds();
            let mut state = CaseState::new();
            for r in sl..=el {
                let s = self.lines.get(r).clone();
                let a = if r == sl { index_at_or_before(&s, sc) } else { 0 };
                let b = if r == el { index_at_or_before(&s, ec) } else { s.chars().count() };
                let (ba, bb) = (byte_of(&s, a), byte_of(&s, b));
                let updated = splice(&s, a, b, &convert_case(&s[ba..bb], change, &mut state));
                self.set_line(r, updated);
                state.end_row(s.trim().is_empty());
            }
            self.snap_caret();
            self.snap_anchor();
        }
        self.edited();
    }

    /// Puts text either side of the selection. With nothing selected, the cursor ends up between the two.
    pub fn wrap_selection(&mut self, before: &str, after: &str) {
        if self.has_multi() {
            let (before, after) = (before.to_string(), after.to_string());
            self.multi_edit(None, |spot, _, chars| {
                let inside: String = chars[spot.start().min(chars.len())..spot.end().min(chars.len())].iter().collect();
                (spot.start(), spot.end(), format!("{before}{inside}{after}"))
            });
            return;
        }
        if self.is_block() {
            self.type_text(&format!("{before}{after}"));
            return;
        }
        if self.has_selection() {
            let inside = self.selection_text();
            self.paste_text(&format!("{before}{inside}{after}"), false);
            return;
        }
        self.paste_text(&format!("{before}{after}"), false);
        if !after.contains('\n') {
            let (line, at) = self.caret_char_position();
            self.move_caret_to_char(line, at.saturating_sub(after.chars().count()));
        }
    }

    // ---------- Several selections ----------

    pub fn has_multi(&self) -> bool {
        !self.multi.is_empty()
    }

    #[cfg(test)]
    pub fn multi_count(&self) -> usize {
        self.multi.len()
    }

    /// Back to one cursor, on the last of them.
    pub fn end_multi(&mut self) {
        self.multi.clear();
    }

    /// The selections and cursors on a line, as screen columns: from, to, and where the cursor is.
    pub fn multi_on_line(&self, line: usize) -> Vec<(usize, usize, usize)> {
        let at = self.multi.partition_point(|spot| spot.line < line);
        let on_line: Vec<&Spot> = self.multi[at..].iter().take_while(|spot| spot.line == line).collect();
        if on_line.is_empty() {
            return Vec::new();
        }
        let s = self.lines.get(line);
        on_line.into_iter().map(|spot| (visual_of(&s, spot.start()), visual_of(&s, spot.end()), visual_of(&s, spot.head))).collect()
    }

    fn sync_main_to_multi(&mut self) {
        self.multi.sort_by_key(|spot| (spot.line, spot.start()));
        self.multi.dedup_by_key(|spot| (spot.line, spot.start()));
        if let Some(last) = self.multi.last().copied() {
            let col = visual_of(&self.lines.get(last.line), last.head);
            (self.caret_line, self.caret_col) = (last.line, col);
            (self.anchor_line, self.anchor_col) = (last.line, col);
            self.desired_col = col;
        }
        self.reveal = true;
    }

    /// Selects every copy of the selected text, line by line. Says why not if it can't.
    pub fn make_selections(&mut self) -> Result<usize, String> {
        if self.column_mode || self.is_block() || !self.has_selection() || self.caret_line != self.anchor_line {
            return Err("Select some text on one line first. Every copy of it will be selected too.".to_string());
        }
        if self.lines.is_disk_backed() {
            return Err("This file is too large for that. It is being kept on disk so that it opens quickly.".to_string());
        }
        let needle = self.selection_text();
        let mut spots = Vec::new();
        for (number, line) in self.all_lines().iter().enumerate() {
            for (byte, _) in line.match_indices(needle.as_str()) {
                let start = char_of_byte(line, byte);
                spots.push(Spot { line: number, anchor: start, head: start + needle.chars().count() });
                if spots.len() > MOST_SPOTS {
                    return Err(format!("There are too many copies. It can select up to {MOST_SPOTS}."));
                }
            }
        }
        if spots.len() < 2 {
            return Err("There are no other copies of that text.".to_string());
        }
        // Editing what can't be seen would be a nasty surprise.
        for spot in &spots {
            self.reveal_line(spot.line);
        }
        let count = spots.len();
        self.multi = spots;
        self.sync_main_to_multi();
        Ok(count)
    }

    /// Adds a cursor, as Ctrl+click does, or takes one away if there's one there already.
    pub fn add_cursor(&mut self, line: usize, col: usize) {
        if self.column_mode || self.alt_block {
            self.move_caret(line, col, false);
            return;
        }
        let line = line.min(self.lines.len() - 1);
        let text = self.lines.get(line).clone();
        let at = index_at_or_before(&text, col);
        if self.multi.is_empty() {
            // What's here now becomes the first of them, if it fits on one line.
            let first = self.lines.get(self.caret_line).clone();
            let head = index_at_or_before(&first, self.caret_col);
            let anchor = if self.caret_line == self.anchor_line { index_at_or_before(&first, self.anchor_col) } else { head };
            self.multi.push(Spot { line: self.caret_line, anchor, head });
        }
        match self.multi.iter().position(|spot| spot.line == line && spot.head == at) {
            Some(place) => {
                self.multi.remove(place);
            }
            None => self.multi.push(Spot { line, anchor: at, head: at }),
        }
        self.reveal_line(line);
        self.sync_main_to_multi();
        if self.multi.len() < 2 {
            self.multi.clear();
        }
        self.last_kind = None;
    }

    /// Makes an edit at every selection, as one undoable action. The edit says which characters of the line
    /// to replace, and with what. Edits are done in order along each line, so that none can get in another's way.
    fn multi_edit(&mut self, kind: Option<&'static str>, edit: impl Fn(&Spot, usize, &[char]) -> (usize, usize, String)) {
        let spots = std::mem::take(&mut self.multi);
        self.push_undo(kind);
        let mut moved = Vec::with_capacity(spots.len());
        let mut place = 0;
        while place < spots.len() {
            let line = spots[place].line;
            let chars: Vec<char> = self.lines.get(line).chars().collect();
            let mut out = String::with_capacity(chars.len());
            let mut count = 0;
            let mut done = 0;
            while place < spots.len() && spots[place].line == line {
                let (from, to, with) = edit(&spots[place], place, &chars);
                let from = from.clamp(done, chars.len());
                let to = to.clamp(from, chars.len());
                out.extend(&chars[done..from]);
                out.push_str(&with);
                count += from - done + with.chars().count();
                moved.push(Spot { line, anchor: count, head: count });
                done = to;
                place += 1;
            }
            out.extend(&chars[done..]);
            self.set_line(line, out);
        }
        self.multi = moved;
        self.sync_main_to_multi();
        self.edited();
    }

    /// One row goes in at every selection. Several rows go in one each, if there are as many as selections.
    fn multi_paste(&mut self, text: &str) {
        let mut rows = split_rows(text);
        if rows.len() > 1 && rows.last().is_some_and(String::is_empty) {
            rows.pop();
        }
        if rows.len() != 1 && rows.len() != self.multi.len() {
            self.notice = Some(format!("That text has {} lines and there are {} selections, so it hasn't been pasted. It needs one line, or one for each selection.", rows.len(), self.multi.len()));
            return;
        }
        self.multi_edit(None, |spot, place, _| (spot.start(), spot.end(), if rows.len() == 1 { rows[0].clone() } else { rows[place].clone() }));
    }

    /// Takes away what's selected, leaving the cursors.
    pub fn delete_selected_spots(&mut self) {
        self.multi_edit(None, |spot, _, _| (spot.start(), spot.end(), String::new()));
    }

    /// The selected text of each selection, one to a line.
    pub fn multi_text(&self) -> String {
        let mut rows = Vec::with_capacity(self.multi.len());
        for spot in self.multi.iter().filter(|spot| spot.anchor != spot.head) {
            let chars: String = self.lines.get(spot.line).chars().skip(spot.start()).take(spot.end() - spot.start()).collect();
            rows.push(chars);
        }
        rows.join("\r\n")
    }

    /// Tab goes in at each cursor. If any text is selected, or with Shift, their lines are indented or outdented instead.
    fn multi_tab(&mut self, shift: bool) {
        if !shift && self.multi.iter().all(|spot| spot.anchor == spot.head) {
            let tab = if edit_prefs().spaces_for_tab { " ".repeat(tab_size()) } else { "\t".to_string() };
            self.multi_edit(None, |spot, _, _| (spot.start(), spot.end(), tab.clone()));
            return;
        }
        let mut spots = std::mem::take(&mut self.multi);
        self.push_undo(None);
        let mut done = None;
        for place in 0..spots.len() {
            let line = spots[place].line;
            if done == Some(line) {
                continue;
            }
            done = Some(line);
            let s = self.lines.get(line).clone();
            let (updated, change): (String, isize) = if !shift {
                let lead = if edit_prefs().spaces_for_tab { " ".repeat(tab_size()) } else { "\t".to_string() };
                (format!("{lead}{s}"), lead.chars().count() as isize)
            } else if let Some(rest) = s.strip_prefix('\t') {
                (rest.to_string(), -1)
            } else {
                let spaces = s.chars().take(tab_size()).take_while(|c| *c == ' ').count();
                (s[spaces..].to_string(), -(spaces as isize))
            };
            self.set_line(line, updated);
            for spot in spots[place..].iter_mut().take_while(|spot| spot.line == line) {
                spot.anchor = spot.anchor.saturating_add_signed(change);
                spot.head = spot.head.saturating_add_signed(change);
            }
        }
        self.multi = spots;
        self.sync_main_to_multi();
        self.edited();
    }

    /// Moves every cursor. With `extend`, along the line only, each selection grows or shrinks.
    pub fn multi_move(&mut self, how: SpotMove, extend: bool) {
        let last = self.lines.len() - 1;
        let mut spots = std::mem::take(&mut self.multi);
        for spot in &mut spots {
            let length = |line: usize| self.lines.get(line).chars().count();
            match how {
                SpotMove::Left if !extend && spot.anchor != spot.head => spot.head = spot.start(),
                SpotMove::Right if !extend && spot.anchor != spot.head => spot.head = spot.end(),
                SpotMove::Left => spot.head = spot.head.saturating_sub(1),
                SpotMove::Right => spot.head = (spot.head + 1).min(length(spot.line)),
                SpotMove::Home => spot.head = 0,
                SpotMove::End => spot.head = length(spot.line),
                SpotMove::Up | SpotMove::Down => {
                    spot.line = if how == SpotMove::Up { spot.line.saturating_sub(1) } else { (spot.line + 1).min(last) };
                    spot.head = spot.head.min(length(spot.line));
                }
            }
            if !extend {
                spot.anchor = spot.head;
            }
        }
        self.multi = spots;
        self.last_kind = None;
        self.sync_main_to_multi();
        if self.multi.len() < 2 {
            self.multi.clear();
        }
    }

    // ---------- Folding ----------

    pub fn has_folds(&self) -> bool {
        !self.lines.folds().is_empty()
    }

    pub fn folds(&self) -> &std::collections::BTreeMap<usize, usize> {
        self.lines.folds()
    }

    pub fn is_folded(&self, line: usize) -> bool {
        self.lines.folds().contains_key(&line)
    }

    /// The last line of the stretch indented under this one, which is what folding it would hide. Blank lines
    /// inside it go with it, and any at the end of it stay out. None if nothing is indented under it.
    pub fn fold_end(&self, line: usize) -> Option<usize> {
        let indent = indent_of(&self.lines.get(line))?;
        let mut last = None;
        let limit = (line + 1 + MOST_FOLD_LINES).min(self.lines.len());
        for next in line + 1..limit {
            match indent_of(&self.lines.get(next)) {
                None => {}
                Some(deeper) if deeper > indent => last = Some(next),
                Some(_) => break,
            }
        }
        last
    }

    /// Whether anything's indented under a line. This only looks as far as the next line with something on it.
    pub fn can_fold(&self, line: usize) -> bool {
        let Some(indent) = indent_of(&self.lines.get(line)) else {
            return false;
        };
        (line + 1..self.lines.len()).take(1000).find_map(|next| indent_of(&self.lines.get(next))).is_some_and(|next| next > indent)
    }

    /// Keeps the cursor out of what's just been hidden.
    fn pull_caret_out_of_folds(&mut self) {
        let caret = self.caret_line;
        let holder = self.lines.folds().iter().find(|(start, last)| **start < caret && caret <= **last).map(|(start, _)| *start);
        if let Some(start) = holder {
            self.move_caret(start, 0, false);
            self.desired_col = 0;
        }
    }

    /// Folds this line's stretch, or opens it if it's folded. Returns false if there's nothing to fold.
    pub fn toggle_fold(&mut self, line: usize) -> bool {
        if self.lines.remove_fold(line) {
            return true;
        }
        let Some(last) = self.fold_end(line) else {
            return false;
        };
        self.lines.set_fold(line, last);
        self.pull_caret_out_of_folds();
        true
    }

    /// Folds every outermost stretch.
    pub fn collapse_all(&mut self) {
        if self.lines.is_disk_backed() {
            self.notice = Some("This file is too large to fold all at once. It is being kept on disk so that it opens quickly.".to_string());
            return;
        }
        let mut line = 0;
        while line < self.lines.len() {
            match self.fold_end(line) {
                Some(last) => {
                    self.lines.set_fold(line, last);
                    line = last + 1;
                }
                None => line += 1,
            }
        }
        self.pull_caret_out_of_folds();
    }

    pub fn expand_all(&mut self) {
        self.lines.clear_folds();
    }

    /// Whether lines can be hidden. Keeping a list of the lines showing would cost too much in a file on disk.
    /// Hides the selected lines under the first of them.
    pub fn hide_selected_lines(&mut self) -> bool {
        let (top, bottom) = self.selected_rows();
        if bottom <= top {
            return false;
        }
        self.lines.set_fold(top, bottom);
        self.pull_caret_out_of_folds();
        true
    }

    /// Opens every fold that the selected lines touch.
    pub fn show_selected_lines(&mut self) {
        let (top, bottom) = self.selected_rows();
        let touching: Vec<usize> = self.lines.folds().iter().filter(|(start, last)| **start <= bottom && top <= **last).map(|(start, _)| *start).collect();
        for start in touching {
            self.lines.remove_fold(start);
        }
    }

    /// Opens whatever is hiding a line. Returns whether anything was.
    pub fn reveal_line(&mut self, line: usize) -> bool {
        let holding: Vec<usize> = self.lines.folds().iter().filter(|(start, last)| **start < line && line <= **last).map(|(start, _)| *start).collect();
        for start in &holding {
            self.lines.remove_fold(*start);
        }
        !holding.is_empty()
    }

    /// The lines showing, in order, given what's folded. The window works from the folds directly; tests check
    /// against this simple list.
    #[cfg(test)]
    pub fn unfolded_lines(&self) -> Vec<usize> {
        let mut shown = Vec::with_capacity(self.lines.len());
        let mut next = 0;
        for (start, last) in self.lines.folds() {
            if *start >= next {
                shown.extend(next..=*start);
            }
            next = next.max(last + 1);
        }
        shown.extend(next..self.lines.len());
        shown
    }

    // ---------- Whole-line commands ----------

    /// The rows a line command works on. A selection ending at the very start of a row leaves that row out.
    fn selected_rows(&self) -> (usize, usize) {
        let top = self.caret_line.min(self.anchor_line);
        let mut bottom = self.caret_line.max(self.anchor_line);
        if bottom > top && !self.is_block() {
            let end_col = if self.caret_line > self.anchor_line { self.caret_col } else { self.anchor_col };
            if end_col == 0 {
                bottom -= 1;
            }
        }
        (top, bottom)
    }

    /// Moves the selected lines up or down one row, keeping them selected.
    pub fn move_lines(&mut self, up: bool) {
        let (top, bottom) = self.selected_rows();
        if self.too_many_rows(bottom - top + 1) {
            return;
        }
        if (up && top == 0) || (!up && bottom + 1 >= self.lines.len()) {
            return;
        }
        self.push_undo(None);
        if up {
            let mut rows = self.lines.range_vec(top - 1, bottom + 1);
            rows.rotate_left(1);
            self.lines.replace_range(top - 1, bottom + 1, rows);
            self.caret_line -= 1;
            self.anchor_line -= 1;
        } else {
            let mut rows = self.lines.range_vec(top, bottom + 2);
            rows.rotate_right(1);
            self.lines.replace_range(top, bottom + 2, rows);
            self.caret_line += 1;
            self.anchor_line += 1;
        }
        let caret = self.caret_col;
        self.edited();
        self.caret_col = caret;
    }

    /// Repeats the selected lines straight after themselves, and moves down onto the copy.
    pub fn duplicate_lines(&mut self) {
        let (top, bottom) = self.selected_rows();
        if self.too_many_rows(bottom - top + 1) {
            return;
        }
        let copy = self.lines.range_vec(top, bottom + 1);
        let count = copy.len();
        self.push_undo(None);
        self.lines.insert_many(bottom + 1, copy);
        self.caret_line += count;
        self.anchor_line += count;
        self.edited();
    }

    /// Joins the selected lines into one (or this line with the next), with a single space between.
    pub fn join_lines(&mut self) {
        let (top, mut bottom) = self.selected_rows();
        if bottom == top {
            bottom = top + 1;
        }
        if bottom >= self.lines.len() || self.too_many_rows(bottom - top + 1) {
            return;
        }
        self.push_undo(None);
        let mut joined = self.lines.get(top).trim_end().to_string();
        for r in top + 1..=bottom {
            let line = self.lines.get(r);
            let next = line.trim();
            if !next.is_empty() {
                if !joined.is_empty() {
                    joined.push(' ');
                }
                joined.push_str(next);
            }
        }
        self.set_line(top, joined);
        self.lines.take_terminator_from(top, bottom);
        self.lines.remove_range(top + 1, bottom + 1);
        self.caret_line = top;
        self.alt_block = false;
        self.caret_col = visual_len(&self.lines.get(top));
        self.collapse_to_caret();
        self.edited();
    }

    /// Comments out the selected lines, or the cursor's line, lining the marks up with the least-indented line.
    pub fn comment_lines(&mut self, marks: &crate::tools::CommentMarks) {
        let (top, bottom) = self.selected_rows();
        if self.too_many_rows(bottom - top + 1) {
            return;
        }
        let rows = self.lines.range_vec(top, bottom + 1);
        let indent_of = |line: &str| line.chars().take_while(|c| *c == ' ' || *c == '\t').count();
        let Some(indent) = rows.iter().filter(|line| !line.trim().is_empty()).map(|line| indent_of(line)).min() else {
            return;
        };
        let state = self.input_table();
        let mark = |text: &str| self.format.prepare_input(text, state);
        let updated: Vec<Option<String>> = rows
            .iter()
            .map(|line| {
                if line.trim().is_empty() {
                    return None;
                }
                let split = byte_of(line, indent);
                let (head, body) = line.split_at(split);
                Some(match marks.line {
                    Some(prefix) => format!("{head}{} {body}", mark(prefix)),
                    None => {
                        let (open, close) = marks.block.unwrap_or(("", ""));
                        format!("{head}{} {body} {}", mark(open), mark(close))
                    }
                })
            })
            .collect();
        self.apply_row_changes(top, updated);
    }

    /// Takes comment marks off the selected lines, or the cursor's line, wherever they have them.
    pub fn uncomment_lines(&mut self, marks: &crate::tools::CommentMarks) {
        let (top, bottom) = self.selected_rows();
        if self.too_many_rows(bottom - top + 1) {
            return;
        }
        let state = self.input_table();
        let line_mark = marks.line.map(|text| self.format.prepare_input(text, state));
        let block_marks = marks.block.map(|(open, close)| (self.format.prepare_input(open, state), self.format.prepare_input(close, state)));
        let updated: Vec<Option<String>> = self
            .lines
            .range_vec(top, bottom + 1)
            .iter()
            .map(|line| {
                let body = line.trim_start_matches([' ', '\t']);
                let head = &line[..line.len() - body.len()];
                if let Some(rest) = line_mark.as_deref().and_then(|mark| body.strip_prefix(mark)) {
                    return Some(format!("{head}{}", rest.strip_prefix(' ').unwrap_or(rest)));
                }
                let (open, close) = block_marks.as_ref()?;
                let inner = body.trim_end_matches([' ', '\t']).strip_prefix(open.as_str())?.strip_suffix(close.as_str())?;
                let inner = inner.strip_prefix(' ').unwrap_or(inner);
                Some(format!("{head}{}", inner.strip_suffix(' ').unwrap_or(inner)))
            })
            .collect();
        self.apply_row_changes(top, updated);
    }

    /// Replaces the lines that have a replacement, as one undoable step, or does nothing if none do.
    fn apply_row_changes(&mut self, top: usize, updated: Vec<Option<String>>) {
        if updated.iter().all(Option::is_none) {
            return;
        }
        self.push_undo(None);
        for (offset, line) in updated.into_iter().enumerate() {
            if let Some(line) = line {
                self.set_line(top + offset, line);
            }
        }
        self.snap_after_rewrite();
        self.edited();
    }

    /// Rows for a command that means "the selection, or else the whole file".
    fn selection_or_file(&self) -> (usize, usize) {
        if self.has_selection() { self.selected_rows() } else { (0, self.lines.len() - 1) }
    }

    /// Sorts the selected lines, or the whole file if nothing is selected.
    pub fn sort_lines(&mut self, descending: bool) {
        let (top, bottom) = self.selection_or_file();
        if self.too_many_rows(bottom - top + 1) {
            return;
        }
        self.push_undo(None);
        let mut rows = self.lines.range_vec(top, bottom + 1);
        rows.sort();
        if descending {
            rows.reverse();
        }
        self.lines.replace_range(top, bottom + 1, rows);
        self.snap_after_rewrite();
        self.edited();
    }

    fn snap_after_rewrite(&mut self) {
        if !self.is_block() {
            self.snap_caret();
            self.snap_anchor();
        }
    }

    fn rewrite_rows(&mut self, convert: impl Fn(&str) -> String) {
        let (top, bottom) = self.selection_or_file();
        if self.too_many_rows(bottom - top + 1) {
            return;
        }
        let old = self.lines.range_vec(top, bottom + 1);
        let updated: Vec<String> = old.iter().map(|l| convert(l)).collect();
        if updated.iter().zip(&old).all(|(new, old)| new.as_str() == &**old) {
            return;
        }
        self.push_undo(None);
        for (i, line) in updated.into_iter().enumerate() {
            self.set_line(top + i, line);
        }
        self.snap_after_rewrite();
        self.edited();
    }

    pub fn trim_leading(&mut self) {
        self.rewrite_rows(|s| s.trim_start_matches([' ', '\t']).to_string());
    }

    pub fn trim_trailing(&mut self) {
        self.rewrite_rows(|s| s.trim_end_matches([' ', '\t']).to_string());
    }

    pub fn tabs_to_spaces(&mut self) {
        self.rewrite_rows(expand_tabs);
    }

    /// Turns the spaces that indent each line into tabs, leaving the rest of the line alone.
    pub fn spaces_to_tabs(&mut self) {
        self.rewrite_rows(|s| {
            let body = s.trim_start_matches([' ', '\t']);
            let width = visual_len(&s[..s.len() - body.len()]);
            format!("{}{}{body}", "\t".repeat(width / tab_size()), " ".repeat(width % tab_size()))
        });
    }

    /// The counts for the selection, or for the whole file.
    pub fn counts(&self, eol_chars: u64) -> Counts {
        if self.has_selection() {
            let text = self.selection_text();
            let rows: Vec<&str> = text.split("\r\n").collect();
            return count_lines(&rows, 2);
        }
        count_lines(&self.lines.range_vec(0, self.lines.len()), eol_chars)
    }

    /// The caret as (line, character index), for working out its place in the file's bytes.
    /// Keeps the caret and selection on the same characters when the tab size changes.
    /// Call with the positions from before the change, once the new size is set.
    pub fn after_tab_size_change(&mut self, caret: (usize, usize), anchor: (usize, usize)) {
        self.caret_col = visual_of(&self.lines.get(caret.0), caret.1);
        self.anchor_col = visual_of(&self.lines.get(anchor.0), anchor.1);
        self.desired_col = self.caret_col;
        self.recalc_max();
        self.reveal = true;
    }

    pub fn anchor_char_position(&self) -> (usize, usize) {
        let line = self.lines.get(self.anchor_line);
        (self.anchor_line, index_at_or_before(&line, self.anchor_col))
    }

    pub fn caret_char_position(&self) -> (usize, usize) {
        let line = self.lines.get(self.caret_line);
        (self.caret_line, index_at_or_before(&line, self.caret_col))
    }

    pub fn move_caret_to_char(&mut self, line: usize, char_index: usize) {
        let line = line.min(self.lines.len() - 1);
        let col = visual_of(&self.lines.get(line), char_index);
        self.move_caret(line, col, false);
        self.desired_col = col;
    }

    /// Finds the brace matching the one at or just before the caret, across lines.
    fn matching_brace(&self) -> Option<((usize, usize), (usize, usize))> {
        const PAIRS: [(char, char); 3] = [('(', ')'), ('[', ']'), ('{', '}')];
        let line: Vec<char> = self.lines.get(self.caret_line).chars().collect();
        let here = index_at_or_before(&self.lines.get(self.caret_line), self.caret_col);
        let is_brace = |c: char| PAIRS.iter().any(|(o, s)| c == *o || c == *s);
        let at = if line.get(here).copied().is_some_and(is_brace) {
            here
        } else if here > 0 && is_brace(line[here - 1]) {
            here - 1
        } else {
            return None;
        };
        let brace = line[at];
        let forward = PAIRS.iter().any(|(o, _)| *o == brace);
        let other = PAIRS.iter().find_map(|(o, s)| if *o == brace { Some(*s) } else if *s == brace { Some(*o) } else { None })?;
        let mut depth = 0usize;
        let mut r = self.caret_line;
        let mut chars = line.clone();
        let mut i = at;
        loop {
            if chars[i] == brace {
                depth += 1;
            } else if chars[i] == other {
                depth -= 1;
                if depth == 0 {
                    return Some(((self.caret_line, at), (r, i)));
                }
            }
            if forward {
                i += 1;
                while i >= chars.len() {
                    r += 1;
                    // Walking a huge file line by line would hang the window, so the search has a limit.
                    if r >= self.lines.len() || r - self.caret_line > MAX_COPY_ROWS {
                        return None;
                    }
                    chars = self.lines.get(r).chars().collect();
                    i = 0;
                    if !chars.is_empty() {
                        break;
                    }
                }
            } else if i > 0 {
                i -= 1;
            } else {
                loop {
                    r = r.checked_sub(1)?;
                    if self.caret_line - r > MAX_COPY_ROWS {
                        return None;
                    }
                    chars = self.lines.get(r).chars().collect();
                    if !chars.is_empty() {
                        break;
                    }
                }
                i = chars.len() - 1;
            }
        }
    }

    /// Jumps to the matching brace, or selects up to and including it. Returns whether there was one.
    pub fn go_to_brace(&mut self, select: bool) -> bool {
        let Some((from, to)) = self.matching_brace() else {
            return false;
        };
        let col_of = |b: &Self, (line, index): (usize, usize)| visual_of(&b.lines.get(line), index);
        if select {
            let forward = to > from;
            let start_col = col_of(self, if forward { from } else { to });
            let end_col = col_of(self, if forward { to } else { from }) + 1;
            let (start_line, end_line) = if forward { (from.0, to.0) } else { (to.0, from.0) };
            self.alt_block = false;
            self.anchor_line = start_line;
            self.anchor_col = start_col;
            self.move_caret(end_line, end_col, true);
        } else {
            let col = col_of(self, to);
            self.move_caret(to.0, col, false);
        }
        self.desired_col = self.caret_col;
        true
    }

    // ---------- Search ----------

    pub fn build_regex(o: &SearchOptions) -> Result<Regex, String> {
        let mut pattern = if o.regex { o.pattern.clone() } else { regex::escape(&o.pattern) };
        if o.whole_word {
            pattern = format!(r"\b(?:{pattern})\b");
        }
        RegexBuilder::new(&pattern)
            .case_insensitive(!o.match_case)
            .build()
            .map_err(|_| "That search pattern isn't valid.".to_string())
    }

    /// Selects the next match, wrapping around the file. Returns whether one was found.
    pub fn find(&mut self, o: &SearchOptions) -> Result<bool, String> {
        let Some(mut cursor) = self.begin_find(o)? else {
            return Ok(false);
        };
        loop {
            match self.find_step(&mut cursor, Duration::from_secs(3600)) {
                SearchStep::Found => return Ok(true),
                SearchStep::NotFound => return Ok(false),
                SearchStep::Running(_) => {}
            }
        }
    }

    /// Sets up a search from the selection; None means there is nothing to look for.
    pub fn begin_find(&self, o: &SearchOptions) -> Result<Option<SearchCursor>, String> {
        if o.pattern.is_empty() {
            return Ok(None);
        }
        let re = Self::build_regex(o)?;
        let (sl, sc, el, ec) = self.stream_bounds();
        let (line, col) = if o.backward { (sl, sc) } else { (el, ec) };
        let text = self.lines.get(line);
        let byte = byte_of(&text, index_at_or_before(&text, col));
        Ok(Some(SearchCursor { re, backward: o.backward, line, byte, step: 0 }))
    }

    /// Searches for up to the given time, selecting the match if it finds one.
    pub fn find_step(&mut self, c: &mut SearchCursor, budget: Duration) -> SearchStep {
        let began = Instant::now();
        let count = self.lines.len();
        while c.step <= count {
            let k = c.step;
            let r = if c.backward { (c.line + count - (k % count)) % count } else { (c.line + k) % count };
            let s = self.lines.get(r);
            let found = if c.backward { backward_in_line(&c.re, &s, k, count, c.byte) } else { forward_in_line(&c.re, &s, k, count, c.byte) };
            match found {
                LineSearch::Hit(start, end) => {
                    self.select_match(r, start, end);
                    return SearchStep::Found;
                }
                LineSearch::Finished => return SearchStep::NotFound,
                LineSearch::Nothing => {}
            }
            c.step += 1;
            if c.step.is_multiple_of(64) && began.elapsed() >= budget {
                return SearchStep::Running(c.step as f32 / (count + 1) as f32);
            }
        }
        SearchStep::NotFound
    }

    /// Selects part of a line given as byte offsets, as search results report them.
    pub fn select_match(&mut self, line: usize, start: usize, end: usize) {
        self.multi.clear();
        self.alt_block = false;
        let s = &self.lines.get(line);
        let anchor = visual_of(s, char_of_byte(s, start));
        let caret = visual_of(s, char_of_byte(s, end));
        self.anchor_line = line;
        self.caret_line = line;
        self.anchor_col = anchor;
        self.caret_col = caret;
        self.desired_col = caret;
        self.last_kind = None;
        self.reveal = true;
    }

    /// Replaces the selected match if there is one, then selects the next.
    pub fn replace_current(&mut self, o: &SearchOptions) -> Result<bool, String> {
        if o.pattern.is_empty() {
            return Ok(false);
        }
        let re = Self::build_regex(o)?;
        if self.has_selection() && self.caret_line == self.anchor_line {
            let s = self.lines.get(self.caret_line).clone();
            let a = index_at_or_before(&s, self.caret_col.min(self.anchor_col));
            let b = index_at_or_before(&s, self.caret_col.max(self.anchor_col));
            let (ba, bb) = (byte_of(&s, a), byte_of(&s, b));
            if let Some(caps) = re.captures_at(&s, ba) {
                let whole = caps.get(0).unwrap();
                if whole.start() == ba && whole.end() == bb {
                    let mut with = String::new();
                    if o.regex {
                        caps.expand(&o.replacement, &mut with);
                    } else {
                        with.push_str(&o.replacement);
                    }
                    self.push_undo(None);
                    self.set_line(self.caret_line, splice(&s, a, b, &with));
                    let end = a + with.chars().count();
                    self.caret_col = visual_of(&self.lines.get(self.caret_line), end);
                    self.anchor_col = self.caret_col;
                    self.edited();
                }
            }
        }
        self.find(o)
    }

    /// Replaces every match in one undoable step, and returns how many there were.
    pub fn replace_all(&mut self, o: &SearchOptions) -> Result<usize, String> {
        let Some(mut cursor) = self.begin_replace_all(o)? else {
            return Ok(0);
        };
        while self.replace_step(&mut cursor, Duration::from_secs(3600)).is_some() {}
        Ok(cursor.count)
    }

    pub fn begin_replace_all(&mut self, o: &SearchOptions) -> Result<Option<ReplaceCursor>, String> {
        if o.pattern.is_empty() {
            return Ok(None);
        }
        let re = Self::build_regex(o)?;
        let most_held = if self.lines.is_disk_backed() { MOST_REPLACED_IN_MEMORY } else { usize::MAX };
        Ok(Some(ReplaceCursor { re, options: o.clone(), next: 0, count: 0, edits: Vec::new(), held: 0, most_held, too_large: false }))
    }

    /// Works through lines for up to the given time. Returns the share done, or None when finished.
    pub fn replace_step(&mut self, c: &mut ReplaceCursor, budget: Duration) -> Option<f32> {
        let began = Instant::now();
        while c.next < self.lines.len() {
            let line = self.lines.get(c.next);
            if let Some((replaced, here)) = replaced_line(&c.re, &c.options, &line) {
                c.held += replaced.len();
                if c.held > c.most_held {
                    c.edits = Vec::new();
                    c.count = 0;
                    c.too_large = true;
                    return None;
                }
                c.count += here;
                c.edits.push((c.next, Arc::from(replaced), self.lines.terminator(c.next)));
            }
            c.next += 1;
            if c.next.is_multiple_of(64) && began.elapsed() >= budget {
                return Some(c.next as f32 / self.lines.len() as f32);
            }
        }
        if c.count > 0 {
            self.push_undo(None);
            self.lines.set_many(std::mem::take(&mut c.edits));
            self.snap_caret();
            self.collapse_to_caret();
            self.edited();
        }
        None
    }

    /// Jumps to a one-based line number; returns false if it isn't one.
    pub fn go_to(&mut self, place: &str) -> bool {
        match place.trim().parse::<usize>() {
            Ok(n) if n >= 1 => {
                self.move_caret(n.min(self.lines.len()) - 1, 0, false);
                self.desired_col = 0;
                true
            }
            _ => false,
        }
    }

    // ---------- Caret and selection ----------

    /// Selects a range; block makes it a column block even outside column mode.
    pub fn set_selection(&mut self, from: (usize, usize), to: (usize, usize), block: bool) {
        self.alt_block = block && !self.column_mode;
        (self.anchor_line, self.anchor_col) = from;
        (self.caret_line, self.caret_col) = to;
        self.anchor_line = self.anchor_line.min(self.lines.len() - 1);
        self.caret_line = self.caret_line.min(self.lines.len() - 1);
        if !self.is_block() {
            self.snap_anchor();
            self.snap_caret();
        }
        self.desired_col = self.caret_col;
        self.last_kind = None;
        self.reveal = true;
    }

    pub fn select_all(&mut self) {
        self.multi.clear();
        self.alt_block = false;
        self.anchor_line = 0;
        self.anchor_col = 0;
        self.caret_line = self.lines.len() - 1;
        // In column mode "everything" is the rectangle that reaches the widest line.
        self.caret_col = if self.column_mode { self.max_visual } else { visual_len(&self.lines.get(self.caret_line)) };
        self.desired_col = self.caret_col;
        self.last_kind = None;
        self.reveal = true;
    }

    pub fn move_caret(&mut self, line: usize, col: usize, extend: bool) {
        self.multi.clear();
        self.caret_line = line.min(self.lines.len() - 1);
        self.caret_col = col;
        if !extend && !self.column_mode {
            self.alt_block = false;
        }
        if !self.is_block() {
            self.snap_caret();
        }
        if !extend {
            self.collapse_to_caret();
        }
        self.last_kind = None;
        self.reveal = true;
    }

    /// Handles an Alt+press: a column block starts here, or grows to here with extend.
    pub fn press_for_block(&mut self, line: usize, col: usize, extend: bool) {
        self.multi.clear();
        self.begin_block();
        self.caret_line = line.min(self.lines.len() - 1);
        self.caret_col = col;
        if !extend {
            self.collapse_to_caret();
        }
        self.desired_col = col;
        self.last_kind = None;
        self.reveal = true;
    }

    /// Starts a column block from Shift+Alt+arrow, outside column mode too.
    pub fn begin_block(&mut self) {
        if !self.column_mode {
            self.alt_block = true;
        }
    }

    pub fn end_temporary_block(&mut self) {
        self.alt_block = false;
    }

    pub fn set_desired_col(&mut self) {
        self.desired_col = self.caret_col;
    }

    pub fn move_horizontal(&mut self, forward: bool, word: bool, extend: bool) {
        if word {
            self.move_word(forward, extend);
        } else if self.is_block() {
            let col = if forward { self.caret_col + 1 } else { self.caret_col.saturating_sub(1) };
            self.move_caret(self.caret_line, col, extend);
        } else if !extend && self.has_selection() {
            let (sl, sc, el, ec) = self.stream_bounds();
            if forward {
                self.move_caret(el, ec, false);
            } else {
                self.move_caret(sl, sc, false);
            }
        } else {
            let s = self.lines.get(self.caret_line).clone();
            let i = index_at_or_before(&s, self.caret_col);
            if forward {
                if i < s.chars().count() {
                    self.move_caret(self.caret_line, visual_of(&s, i + 1), extend);
                } else if self.caret_line + 1 < self.lines.len() {
                    self.move_caret(self.caret_line + 1, 0, extend);
                }
            } else if i > 0 {
                self.move_caret(self.caret_line, visual_of(&s, i - 1), extend);
            } else if self.caret_line > 0 {
                let col = visual_len(&self.lines.get(self.caret_line - 1));
                self.move_caret(self.caret_line - 1, col, extend);
            }
        }
        self.desired_col = self.caret_col;
    }

    fn move_word(&mut self, forward: bool, extend: bool) {
        let s = self.lines.get(self.caret_line).clone();
        let chars: Vec<char> = s.chars().collect();
        let mut i = index_at_or_before(&s, self.caret_col);
        if forward {
            if i >= chars.len() {
                if self.caret_line + 1 < self.lines.len() {
                    self.move_caret(self.caret_line + 1, 0, extend);
                }
                return;
            }
            if is_word_char(chars[i]) {
                while i < chars.len() && is_word_char(chars[i]) {
                    i += 1;
                }
            } else if !chars[i].is_whitespace() {
                i += 1;
            }
            while i < chars.len() && chars[i].is_whitespace() {
                i += 1;
            }
        } else {
            if i == 0 {
                if self.caret_line > 0 {
                    let col = visual_len(&self.lines.get(self.caret_line - 1));
                    self.move_caret(self.caret_line - 1, col, extend);
                }
                return;
            }
            i -= 1;
            while i > 0 && chars[i].is_whitespace() {
                i -= 1;
            }
            if is_word_char(chars[i]) {
                while i > 0 && is_word_char(chars[i - 1]) {
                    i -= 1;
                }
            }
        }
        self.move_caret(self.caret_line, visual_of(&s, i), extend);
    }

    pub fn move_vertical(&mut self, delta: isize, extend: bool) {
        let keep = self.desired_col;
        let line = self.caret_line.saturating_add_signed(delta);
        self.move_caret(line, keep, extend);
        self.desired_col = keep;
    }

    pub fn move_home(&mut self, extend: bool) {
        let s = &self.lines.get(self.caret_line);
        let first = s.chars().take_while(|c| *c == ' ' || *c == '\t').count();
        let first_col = visual_of(s, first);
        let col = if self.caret_col == first_col || edit_prefs().home_to_start { 0 } else { first_col };
        self.move_caret(self.caret_line, col, extend);
        self.desired_col = self.caret_col;
    }

    pub fn move_end(&mut self, extend: bool) {
        let col = visual_len(&self.lines.get(self.caret_line));
        self.move_caret(self.caret_line, col, extend);
        self.desired_col = self.caret_col;
    }

    pub fn move_document_start(&mut self, extend: bool) {
        self.move_caret(0, 0, extend);
        self.desired_col = self.caret_col;
    }

    pub fn move_document_end(&mut self, extend: bool) {
        let last = self.lines.len() - 1;
        let col = visual_len(&self.lines.get(last));
        self.move_caret(last, col, extend);
        self.desired_col = self.caret_col;
    }

    pub fn select_word_at_caret(&mut self) {
        self.multi.clear();
        let s = self.lines.get(self.caret_line).clone();
        let chars: Vec<char> = s.chars().collect();
        let i = index_at_or_before(&s, self.caret_col);
        if i < chars.len() && is_word_char(chars[i]) {
            let (mut a, mut b) = (i, i);
            while a > 0 && is_word_char(chars[a - 1]) {
                a -= 1;
            }
            while b < chars.len() && is_word_char(chars[b]) {
                b += 1;
            }
            self.anchor_line = self.caret_line;
            self.anchor_col = visual_of(&s, a);
            self.caret_col = visual_of(&s, b);
            self.desired_col = self.caret_col;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn buffer(lines: &[&str]) -> TextBuffer {
        let mut b = TextBuffer::default();
        b.set_content(lines.iter().map(|s| s.to_string()).collect());
        b
    }

    fn text(b: &TextBuffer) -> Vec<String> {
        b.all_lines().iter().map(|l| l.to_string()).collect()
    }

    #[test]
    fn typing_in_a_zero_width_block_goes_on_every_row() {
        let mut b = buffer(&["aaa", "bbb", "ccc"]);
        b.set_column_mode(true);
        b.set_selection((0, 1), (2, 1), false);
        b.type_text("X");
        b.type_text("Y");
        assert_eq!(text(&b), ["aXYaa", "bXYbb", "cXYcc"]);
        assert_eq!(b.caret_col, 3);
        assert_eq!(b.anchor_col, 3);
    }

    #[test]
    fn typing_past_a_short_line_pads_it_with_spaces() {
        let mut b = buffer(&["long line", "ab", "long line"]);
        b.set_column_mode(true);
        b.set_selection((0, 5), (2, 5), false);
        b.type_text("|");
        assert_eq!(text(&b), ["long |line", "ab   |", "long |line"]);
    }

    #[test]
    fn a_selected_block_is_replaced_by_typing() {
        let mut b = buffer(&["abcdef", "ghijkl"]);
        b.set_selection((0, 1), (1, 3), true);
        b.type_text("Z");
        assert_eq!(text(&b), ["aZdef", "gZjkl"]);
    }

    #[test]
    fn deleting_a_block_leaves_short_lines_alone() {
        let mut b = buffer(&["abcdef", "g", "hijklm"]);
        b.set_selection((0, 2), (2, 4), true);
        b.delete_forward();
        assert_eq!(text(&b), ["abef", "g", "hilm"]);
        assert_eq!(b.caret_col, 2);
    }

    #[test]
    fn backspace_and_delete_work_on_every_row() {
        let mut b = buffer(&["abc", "def", "g"]);
        b.set_column_mode(true);
        b.set_selection((0, 2), (2, 2), false);
        b.backspace();
        assert_eq!(text(&b), ["ac", "df", "g"]);
        assert_eq!(b.caret_col, 1);
        b.delete_forward();
        assert_eq!(text(&b), ["a", "d", "g"]);
    }

    #[test]
    fn copying_a_block_pads_short_rows_and_expands_tabs() {
        let mut b = buffer(&["abcdefghi", "g", "\tx"]);
        b.set_selection((0, 5), (2, 9), true);
        assert_eq!(b.selection_text(), "fghi\r\n    \r\n   x");
    }

    #[test]
    fn a_block_pastes_with_its_shape() {
        let mut b = buffer(&["11", "22", "33"]);
        b.move_caret(0, 1, false);
        b.paste_text("ab\r\ncd", true);
        assert_eq!(text(&b), ["1ab1", "2cd2", "33"]);
    }

    #[test]
    fn a_block_paste_adds_lines_at_the_end_of_the_file() {
        let mut b = buffer(&["x"]);
        b.move_caret(0, 1, false);
        b.paste_text("a\nb\nc", true);
        assert_eq!(text(&b), ["xa", " b", " c"]);
    }

    #[test]
    fn one_line_pastes_onto_every_selected_row() {
        let mut b = buffer(&["aa", "bb", "cc"]);
        b.set_column_mode(true);
        b.set_selection((0, 1), (2, 1), false);
        b.paste_text("-", false);
        assert_eq!(text(&b), ["a-a", "b-b", "c-c"]);
    }

    #[test]
    fn a_block_paste_replaces_a_plain_selection() {
        let mut b = buffer(&["hello world", "second"]);
        b.set_selection((0, 0), (0, 5), false);
        b.paste_text("AB\nCD", true);
        assert_eq!(text(&b), ["AB world", "CDsecond"]);
    }

    #[test]
    fn a_tab_inside_a_block_becomes_spaces_to_keep_alignment() {
        let mut b = buffer(&["a\tb", "abcdefgh"]);
        b.set_column_mode(true);
        b.set_selection((0, 2), (1, 2), false);
        b.type_text("#");
        assert_eq!(text(&b), ["a #      b", "ab#cdefgh"]);
    }

    #[test]
    fn numbers_count_and_line_up() {
        let mut b = buffer(&["a"; 11]);
        b.set_selection((0, 1), (10, 1), true);
        assert!(b.insert_column_numbers(1, 1));
        assert_eq!(&*b.line(0), "a 1");
        assert_eq!(&*b.line(8), "a 9");
        assert_eq!(&*b.line(10), "a11");
    }

    #[test]
    fn numbers_without_a_block_run_to_the_end_of_the_file() {
        let mut b = buffer(&["a", "b", "c"]);
        b.move_caret(1, 0, false);
        assert!(b.insert_column_numbers(5, -2));
        assert_eq!(text(&b), ["a", "5b", "3c"]);
    }

    #[test]
    fn numbers_that_would_overflow_change_nothing() {
        let mut b = buffer(&["a", "b"]);
        b.set_selection((0, 0), (1, 0), true);
        assert!(!b.insert_column_numbers(i64::MAX, 1));
        assert_eq!(text(&b), ["a", "b"]);
        assert!(!b.can_undo());
    }

    #[test]
    fn the_tab_key_can_insert_spaces_up_to_the_next_stop() {
        set_tab_size(4);
        set_edit_prefs(EditPrefs { spaces_for_tab: true, ..EditPrefs::default() });
        let mut b = buffer(&["ab", "cd"]);
        b.move_caret(0, 1, false);
        b.tab_key(false);
        assert_eq!(text(&b), ["a   b", "cd"]);
        assert_eq!(b.caret_col, 4);
        b.select_all();
        b.tab_key(false);
        assert_eq!(text(&b), ["    a   b", "    cd"]);
        b.tab_key(true);
        assert_eq!(text(&b), ["a   b", "cd"], "and outdenting takes them back");
        set_edit_prefs(EditPrefs::default());
        let mut plain = buffer(&["ab"]);
        plain.tab_key(false);
        assert_eq!(text(&plain), ["\tab"], "off, it's a tab as before");
        set_tab_size(DEFAULT_TAB_SIZE);
    }

    #[test]
    fn new_lines_keep_the_indent_unless_told_not_to() {
        let mut b = buffer(&["    ab"]);
        b.move_caret(0, 6, false);
        b.new_line();
        assert_eq!(text(&b), ["    ab", "    "]);
        set_edit_prefs(EditPrefs { auto_indent: false, ..EditPrefs::default() });
        let mut off = buffer(&["    ab"]);
        off.move_caret(0, 6, false);
        off.new_line();
        assert_eq!(text(&off), ["    ab", ""]);
        assert_eq!(off.caret_col, 0);
        set_edit_prefs(EditPrefs::default());
    }

    #[test]
    fn home_can_always_go_to_the_start_of_the_line() {
        let mut b = buffer(&["    ab"]);
        b.move_caret(0, 6, false);
        b.move_home(false);
        assert_eq!(b.caret_col, 4, "to the text first, as before");
        set_edit_prefs(EditPrefs { home_to_start: true, ..EditPrefs::default() });
        b.move_caret(0, 6, false);
        b.move_home(false);
        assert_eq!(b.caret_col, 0);
        set_edit_prefs(EditPrefs::default());
    }

    #[test]
    fn the_whole_line_is_selected_only_when_asked_and_nothing_else_is() {
        let mut b = buffer(&["one", "two", ""]);
        b.move_caret(1, 1, false);
        assert!(!b.select_line_if_none(), "off by default");
        set_edit_prefs(EditPrefs { whole_line_copy: true, ..EditPrefs::default() });
        assert!(b.select_line_if_none());
        assert_eq!(b.selection_text(), "two\r\n");
        assert!(!b.select_line_if_none(), "something's selected now");
        b.delete_selection();
        assert_eq!(text(&b), ["one", ""]);
        let mut last = buffer(&["one", "two"]);
        last.move_caret(1, 1, false);
        assert!(last.select_line_if_none());
        assert_eq!(last.selection_text(), "two", "the last line has no ending to take");
        let mut empty = buffer(&[""]);
        assert!(!empty.select_line_if_none(), "nothing to take");
        let mut column = buffer(&["one", "two"]);
        column.set_column_mode(true);
        assert!(!column.select_line_if_none(), "column mode has its own idea of a selection");
        set_edit_prefs(EditPrefs::default());
    }

    #[test]
    fn changed_lines_are_marked_and_follow_their_lines() {
        let mut b = buffer(&["a", "b", "c", "d"]);
        assert_eq!(b.changed_line_count(), 0, "opening a file changes nothing");
        b.move_caret(1, 1, false);
        b.type_text("x");
        assert_eq!(b.change_at(1), Some(false));
        assert_eq!(b.change_at(0), None);
        b.move_caret(0, 1, false);
        b.new_line();
        assert_eq!(text(&b), ["a", "", "bx", "c", "d"]);
        assert_eq!(b.change_at(2), Some(false), "the mark moved down with its line");
        assert_eq!(b.change_at(1), Some(false), "and the new line is marked");
        assert_eq!(b.change_at(3), None);
    }

    #[test]
    fn saving_turns_marks_into_saved_marks_and_later_edits_are_unsaved_again() {
        let mut b = buffer(&["a", "b"]);
        b.type_text("x");
        b.mark_saved();
        assert_eq!(b.change_at(0), Some(true));
        b.move_caret(1, 0, false);
        b.type_text("y");
        assert_eq!(b.change_at(0), Some(true));
        assert_eq!(b.change_at(1), Some(false));
        b.move_caret(0, 0, false);
        b.type_text("z");
        assert_eq!(b.change_at(0), Some(false));
    }

    #[test]
    fn undo_takes_the_marks_back_with_the_text() {
        let mut b = buffer(&["a", "b"]);
        b.type_text("x");
        assert_eq!(b.changed_line_count(), 1);
        b.undo();
        assert_eq!(b.changed_line_count(), 0);
        b.redo();
        assert_eq!(b.change_at(0), Some(false));
    }

    #[test]
    fn deleting_lines_marks_where_they_were() {
        let mut b = buffer(&["a", "b", "c"]);
        b.set_selection((1, 0), (2, 0), false);
        b.delete_selection();
        assert_eq!(text(&b), ["a", "c"]);
        assert_eq!(b.change_at(1), Some(false), "something went from here");
        assert_eq!(b.change_at(0), None);
        let mut end = buffer(&["a", "b"]);
        end.set_selection((0, 0), (1, 1), false);
        end.delete_selection();
        assert_eq!(text(&end), [""]);
        assert_eq!(end.changed_line_count(), 1, "no mark is left beyond the end");
    }

    #[test]
    fn last_and_next_change_step_through_stretches_and_wrap() {
        let mut b = buffer(&["0", "1", "2", "3", "4", "5", "6"]);
        for line in [1, 2, 5] {
            b.move_caret(line, 0, false);
            b.type_text("x");
        }
        b.move_caret(0, 0, false);
        assert!(b.go_to_change(true));
        assert_eq!(b.caret_line, 1);
        assert!(b.go_to_change(true));
        assert_eq!(b.caret_line, 5, "line 2 is in the same stretch as line 1");
        assert!(b.go_to_change(true));
        assert_eq!(b.caret_line, 1, "it wraps round");
        assert!(b.go_to_change(false));
        assert_eq!(b.caret_line, 5, "and back round the other way");
        assert!(b.go_to_change(false));
        assert_eq!(b.caret_line, 1);
        b.move_caret(4, 0, false);
        assert!(b.go_to_change(false));
        assert_eq!(b.caret_line, 1, "to the start of the stretch before");
        assert!(!buffer(&["a"]).go_to_change(true), "nothing has changed");
        assert!(!buffer(&["a"]).go_to_change(false));
    }

    fn cased(text_in: &str, change: CaseChange) -> String {
        convert_case(text_in, change, &mut CaseState::new())
    }

    #[test]
    fn sentence_case_capitalizes_only_the_start_of_each_sentence() {
        assert_eq!(cased("hELLO wORLD. tHIS IS IT! rEALLY? yes", CaseChange::Sentence), "Hello world. This is it! Really? Yes");
    }

    #[test]
    fn sentence_case_isnt_fooled_by_numbers_names_or_quotes() {
        assert_eq!(cased("it costs 3.50 AT example.com TODAY", CaseChange::Sentence), "It costs 3.50 at example.com today");
        assert_eq!(cased("  (first) word", CaseChange::Sentence), "  (First) word", "the first letter, wherever it is");
        assert_eq!(cased("he left.\" SHE STAYED.", CaseChange::Sentence), "He left.\" She stayed.");
        assert_eq!(cased("WAIT... WHAT", CaseChange::Sentence), "Wait... What");
        assert_eq!(cased("123 ABC", CaseChange::Sentence), "123 Abc");
    }

    #[test]
    fn title_case_capitalizes_each_word_and_lowers_the_rest() {
        assert_eq!(cased("the qUICK brown FOX", CaseChange::Title), "The Quick Brown Fox");
        assert_eq!(cased("well-known, tried_and_true", CaseChange::Title), "Well-Known, Tried_And_True");
    }

    #[test]
    fn title_case_keeps_apostrophes_and_numbers_inside_their_words() {
        assert_eq!(cased("DON'T stop 'til IT'S the 3RD", CaseChange::Title), "Don't Stop 'Til It's The 3rd");
        assert_eq!(cased("don\u{2019}t", CaseChange::Title), "Don\u{2019}t");
    }

    #[test]
    fn letters_outside_english_follow_the_same_rules() {
        assert_eq!(cased("\u{E9}T\u{C9} \u{C0} PARIS", CaseChange::Title), "\u{C9}t\u{E9} \u{C0} Paris");
        assert_eq!(cased("\u{DF}a", CaseChange::Title), "Ssa", "a letter that becomes two only capitalizes the first");
    }

    #[test]
    fn a_sentence_runs_across_lines_until_it_ends_or_a_blank_line_comes() {
        let mut b = buffer(&["ONE SENTENCE", "GOES ON. A NEW ONE", "", "AFTER A GAP"]);
        b.select_all();
        b.change_case(CaseChange::Sentence);
        assert_eq!(text(&b), ["One sentence", "goes on. A new one", "", "After a gap"]);
    }

    #[test]
    fn a_sentence_ending_at_the_end_of_a_line_starts_the_next_there() {
        let mut b = buffer(&["FIRST.", "SECOND"]);
        b.select_all();
        b.change_case(CaseChange::Sentence);
        assert_eq!(text(&b), ["First.", "Second"]);
    }

    #[test]
    fn only_the_selection_changes_and_its_start_counts_as_a_start() {
        let mut b = buffer(&["keep THIS part HERE"]);
        b.set_selection((0, 5), (0, 14), false);
        b.change_case(CaseChange::Title);
        assert_eq!(text(&b), ["keep This Part HERE"]);
        b.undo();
        b.change_case(CaseChange::Sentence);
        assert_eq!(text(&b), ["keep This part HERE"]);
        b.undo();
        assert_eq!(text(&b), ["keep THIS part HERE"]);
    }

    #[test]
    fn each_row_of_a_block_is_cased_on_its_own() {
        let mut b = buffer(&["x ALPHA ONE y", "x BETA TWO  y"]);
        b.set_selection((0, 2), (1, 11), true);
        b.change_case(CaseChange::Sentence);
        assert_eq!(text(&b), ["x Alpha one y", "x Beta two  y"]);
        b.change_case(CaseChange::Title);
        assert_eq!(text(&b), ["x Alpha One y", "x Beta Two  y"]);
    }

    #[test]
    fn upper_and_lower_still_work_and_nothing_selected_does_nothing() {
        let mut b = buffer(&["Mixed"]);
        b.change_case(CaseChange::Upper);
        assert_eq!(text(&b), ["Mixed"]);
        assert!(!b.can_undo());
        b.select_all();
        b.change_case(CaseChange::Upper);
        assert_eq!(text(&b), ["MIXED"]);
        b.change_case(CaseChange::Lower);
        assert_eq!(text(&b), ["mixed"]);
    }

    #[test]
    fn exact_bytes_in_a_line_come_through_case_changes() {
        let raw = charset::raw_char(0xE9);
        let mut b = buffer(&[&format!("ab{raw}cd")]);
        b.select_all();
        b.change_case(CaseChange::Title);
        assert_eq!(text(&b), [format!("Ab{raw}Cd")], "it isn't a letter, so a word ends there, and the byte itself stays");
    }

    #[test]
    fn fill_replaces_the_block() {
        let mut b = buffer(&["abcd", "efgh"]);
        b.set_selection((0, 1), (1, 3), true);
        b.insert_column_fill("--", false);
        assert_eq!(text(&b), ["a--d", "e--h"]);
    }

    #[test]
    fn fill_runs_from_the_cursor_to_the_end_of_the_file() {
        let mut b = buffer(&["one", "two", "three"]);
        b.set_column_mode(true);
        b.move_caret(1, 1, false);
        b.insert_column_fill("+", false);
        assert_eq!(text(&b), ["one", "t+wo", "t+hree"]);
    }

    #[test]
    fn fill_can_leave_an_empty_last_line_alone() {
        let mut b = buffer(&["ab", "cd", ""]);
        b.set_column_mode(true);
        b.move_caret(0, 1, false);
        b.insert_column_fill("+", true);
        assert_eq!(text(&b), ["a+b", "c+d", ""]);
        b.undo();
        b.insert_column_fill("+", false);
        assert_eq!(text(&b), ["a+b", "c+d", " +"], "and pads it when asked not to");
    }

    #[test]
    fn leaving_the_last_line_alone_only_applies_to_an_empty_one_at_the_very_end() {
        let mut b = buffer(&["ab", "", "cd"]);
        b.set_column_mode(true);
        b.move_caret(0, 0, false);
        b.insert_column_fill("+", true);
        assert_eq!(text(&b), ["+ab", "+", "+cd"]);
        let mut only = buffer(&[""]);
        only.set_column_mode(true);
        only.insert_column_fill("+", true);
        assert_eq!(text(&only), ["+"], "the cursor is on it, so that's what's meant");
    }

    #[test]
    fn a_number_of_columns_is_taken_from_the_cursor_down() {
        let mut b = buffer(&["keep", "abcdef", "ghijkl", "m"]);
        b.set_column_mode(true);
        b.move_caret(1, 1, false);
        b.select_columns(2).unwrap();
        assert_eq!(b.selection_text(), "bc\r\nhi\r\n  ");
        b.remove_selected_columns();
        assert_eq!(text(&b), ["keep", "adef", "gjkl", "m"]);
        assert_eq!((b.caret_line, b.caret_col, b.anchor_line, b.anchor_col), (1, 1, 1, 1), "the cursor is on one row again, not all of them");
        b.undo();
        assert_eq!(text(&b), ["keep", "abcdef", "ghijkl", "m"]);
    }

    #[test]
    fn zero_columns_takes_only_whats_highlighted() {
        let mut b = buffer(&["abcd", "efgh", "ijkl", "mnop"]);
        b.set_column_mode(true);
        b.set_selection((1, 1), (2, 3), false);
        b.select_columns(0).unwrap();
        b.remove_selected_columns();
        assert_eq!(text(&b), ["abcd", "eh", "il", "mnop"]);
    }

    #[test]
    fn taking_zero_columns_with_nothing_highlighted_says_so_and_changes_nothing() {
        let mut b = buffer(&["abcd", "efgh"]);
        b.set_column_mode(true);
        assert!(b.select_columns(0).is_err());
        b.set_selection((0, 2), (1, 2), false);
        assert!(b.select_columns(0).is_err(), "a cursor over two rows has no width");
        assert_eq!(text(&b), ["abcd", "efgh"]);
    }

    #[test]
    fn taking_columns_past_the_text_stops_at_the_text() {
        let mut b = buffer(&["abcd", "ef"]);
        b.set_column_mode(true);
        b.move_caret(0, 1, false);
        b.select_columns(usize::MAX).unwrap();
        assert_eq!(b.selection_text(), "bcd\r\nf  ", "no huge block of spaces");
        b.remove_selected_columns();
        assert_eq!(text(&b), ["a", "e"]);
        b.move_caret(0, 3, false);
        assert!(b.select_columns(2).is_err(), "nothing out there to take");
        assert_eq!(text(&b), ["a", "e"]);
    }

    #[test]
    fn taking_columns_splits_a_tab_rather_than_shifting_the_text() {
        set_tab_size(4);
        let mut b = buffer(&["\tabc"]);
        b.set_column_mode(true);
        b.move_caret(0, 2, false);
        b.select_columns(3).unwrap();
        b.remove_selected_columns();
        assert_eq!(text(&b), ["  bc"]);
    }

    #[test]
    fn taking_columns_works_on_a_block_made_outside_column_mode_and_ends_it() {
        let mut b = buffer(&["abcd", "efgh"]);
        b.set_selection((0, 1), (1, 2), true);
        b.select_columns(0).unwrap();
        b.remove_selected_columns();
        assert_eq!(text(&b), ["acd", "egh"]);
        assert!(!b.is_block());
    }

    #[test]
    fn justify_left_closes_up_to_the_left_edge_of_the_block() {
        let mut b = buffer(&["x  ab  y", "x cde  y", "x      y"]);
        b.set_selection((0, 1), (2, 7), true);
        b.justify_columns(Justify::Left);
        assert_eq!(text(&b), ["xab    y", "xcde   y", "x      y"]);
    }

    #[test]
    fn justify_right_and_center_keep_the_block_and_whats_beyond_it() {
        let mut b = buffer(&["|ab    |", "|abcd  |", "|      |"]);
        b.set_selection((0, 1), (2, 7), true);
        b.justify_columns(Justify::Right);
        assert_eq!(text(&b), ["|    ab|", "|  abcd|", "|      |"]);
        assert_eq!(b.selection_on_row(0), Some((1, 7)), "the highlight stays, to try another");
        b.justify_columns(Justify::Center);
        assert_eq!(text(&b), ["|  ab  |", "| abcd |", "|      |"]);
        b.justify_columns(Justify::Left);
        assert_eq!(text(&b), ["|ab    |", "|abcd  |", "|      |"]);
        b.undo();
        assert_eq!(text(&b), ["|  ab  |", "| abcd |", "|      |"]);
    }

    #[test]
    fn justify_adds_no_trailing_spaces_to_short_lines_and_leaves_shorter_ones_alone() {
        let mut b = buffer(&["a  bb", "a", "a  c  "]);
        b.set_selection((0, 2), (2, 6), true);
        b.justify_columns(Justify::Left);
        assert_eq!(text(&b), ["a bb", "a", "a c"]);
        b.justify_columns(Justify::Right);
        assert_eq!(text(&b), ["a   bb", "a", "a    c"]);
    }

    #[test]
    fn justify_with_nothing_to_do_or_nothing_highlighted_marks_no_edit() {
        let mut b = buffer(&["ab  ", "cd  "]);
        b.set_selection((0, 0), (1, 4), true);
        b.justify_columns(Justify::Left);
        assert!(!b.can_undo());
        assert!(!b.is_modified());
        let mut none = buffer(&["ab"]);
        none.set_column_mode(true);
        none.justify_columns(Justify::Right);
        assert!(none.notice.take().is_some());
        assert_eq!(text(&none), ["ab"]);
    }

    #[test]
    fn converting_takes_the_selected_lines_or_the_file_without_an_empty_last_line() {
        let mut b = buffer(&["a", "b", "c", ""]);
        assert_eq!(b.rows_for_convert(), Some((0, 2)));
        b.set_selection((1, 0), (2, 0), false);
        assert_eq!(b.rows_for_convert(), Some((1, 1)), "a selection ending at the start of a line doesn't include it");
        b.set_selection((1, 0), (2, 1), false);
        assert_eq!(b.rows_for_convert(), Some((1, 2)));
        b.set_selection((0, 0), (2, 1), true);
        assert_eq!(b.rows_for_convert(), Some((0, 2)));
        assert_eq!(buffer(&[""]).rows_for_convert(), Some((0, 0)));
    }

    #[test]
    fn replacing_rows_is_one_undo_and_keeps_line_endings() {
        let mut b = decoded_buffer(b"a\r\nb\nc", format_of("utf-8"));
        b.replace_rows(0, vec!["one".into(), "two".into()]);
        assert_eq!(bytes_of(&b), b"one\r\ntwo\nc");
        b.undo();
        assert_eq!(bytes_of(&b), b"a\r\nb\nc");
        assert!(!b.is_modified());
        b.replace_rows(0, vec!["a".into(), "b".into()]);
        assert!(!b.is_modified(), "no change, no edit");
    }

    #[test]
    fn column_sum_adds_numbers_and_counts_the_rest() {
        let b = {
            let mut b = buffer(&["x 1,000.50", "x    2.25", "x     abc", "x"]);
            b.set_selection((0, 2), (3, 10), true);
            b
        };
        let s = b.sum_column();
        assert_eq!(s, ColumnSum { sum: 1002.75, numbers: 2, skipped: 1, decimals: 2 });
    }

    #[test]
    fn undo_and_redo_restore_a_column_edit_exactly() {
        let mut b = buffer(&["aa", "bb"]);
        b.set_column_mode(true);
        b.set_selection((0, 1), (1, 1), false);
        b.type_text("X");
        assert!(b.is_modified());
        b.undo();
        assert_eq!(text(&b), ["aa", "bb"]);
        assert!(!b.is_modified());
        b.redo();
        assert_eq!(text(&b), ["aXa", "bXb"]);
        assert!(b.is_modified());
    }

    #[test]
    fn tokens_take_their_label_width_and_are_never_split() {
        set_source_view(true);
        set_cell_profile(charset::known("amiga-text"));
        let csi = charset::raw_char(0x9B);
        assert_eq!(visual_len(&format!("{csi}x")), 6, "[CSI] and x");
        assert_eq!(visual_len("\u{1B}"), 5, "[ESC] in a Unicode file too");
        let mut b = buffer(&[&format!("a{csi}b"), "next"]);
        b.format.charset = charset::known("amiga-text");
        b.move_caret(0, 3, false);
        b.set_desired_col();
        assert_eq!(b.caret_col, 1, "a click inside a token lands before it");
        b.move_horizontal(true, false, false);
        assert_eq!(b.caret_col, 6);
        b.move_horizontal(true, false, false);
        assert_eq!(b.caret_col, 7);
        b.backspace();
        assert_eq!(&*b.line(0), format!("a{csi}"));
        b.backspace();
        assert_eq!(&*b.line(0), "a", "a token goes in one keypress");
        b.undo();
        b.undo();
        assert_eq!(&*b.line(0), format!("a{csi}b"));
        set_cell_profile(charset::utf8());
    }

    #[test]
    fn a_block_copy_keeps_a_token_whole() {
        set_cell_profile(charset::utf8());
        let raw = charset::raw_char(0xFF);
        assert_eq!(block_cell(&format!("a{raw}b"), 1, 7), format!("{raw}"));
        assert_eq!(block_cell("a\tb", 0, 3), "a  ");
    }

    fn decoded_buffer(data: &[u8], format: TextFormat) -> TextBuffer {
        let mut b = TextBuffer { format, ..TextBuffer::default() };
        let decoded = crate::codec::decode(data, &mut b.format);
        b.set_decoded(decoded);
        b
    }

    fn bytes_of(b: &TextBuffer) -> Vec<u8> {
        b.encode(&b.format, crate::codec::Lossy::Refuse).unwrap()
    }

    fn format_of(id: &str) -> TextFormat {
        TextFormat { charset: charset::known(id), ..TextFormat::default() }
    }

    #[test]
    fn enter_uses_the_insertion_style_and_keeps_the_old_ending_at_the_far_end() {
        let mut b = decoded_buffer(b"one\rtwo\r\nthree\r\n", format_of("utf-8"));
        assert_eq!(b.insert_eol(), Eol::CrLf, "the commonest in the file");
        b.move_caret(0, 1, false);
        b.new_line();
        assert_eq!(bytes_of(&b), b"o\r\nne\rtwo\r\nthree\r\n");
        b.undo();
        b.set_insert_eol(Eol::Lf);
        b.move_caret(1, 1, false);
        b.new_line();
        assert_eq!(bytes_of(&b), b"one\rt\nwo\r\nthree\r\n");
    }

    #[test]
    fn deleting_a_line_break_keeps_the_later_ending() {
        let mut b = decoded_buffer(b"a\rb\nc\r\nd", format_of("utf-8"));
        b.move_caret(0, 1, false);
        b.delete_forward();
        assert_eq!(bytes_of(&b), b"ab\nc\r\nd");
        b.undo();
        b.move_caret(1, 0, false);
        b.backspace();
        assert_eq!(bytes_of(&b), b"ab\nc\r\nd");
        b.undo();
        b.move_caret(0, 1, false);
        b.move_caret(2, 0, true);
        b.delete_selection();
        assert_eq!(bytes_of(&b), b"ac\r\nd");
    }

    #[test]
    fn undo_restores_both_text_and_conversion_settings() {
        let mut b = decoded_buffer(b"caf\x82\r\nx\n", format_of("cp437"));
        let original = bytes_of(&b);
        let target = format_of("utf-8");
        let converted = crate::codec::convert_lines(&b.all_lines(), &b.format, &target).unwrap();
        b.apply_conversion(converted, target);
        assert_eq!(b.format.charset.id, "utf-8");
        assert_eq!(bytes_of(&b), "caf\u{e9}\r\nx\n".as_bytes(), "encoding alone leaves the line endings alone");
        assert!(b.is_modified());
        b.undo();
        assert_eq!(b.format.charset.id, "cp437");
        assert_eq!(bytes_of(&b), original);
        assert!(!b.is_modified());
        b.redo();
        assert_eq!(b.format.charset.id, "utf-8");

        let mut b = decoded_buffer(b"a\r\nb\nc\r", format_of("utf-8"));
        let original = bytes_of(&b);
        assert!(b.convert_line_endings(Eol::Lf));
        assert_eq!(bytes_of(&b), b"a\nb\nc\n");
        b.undo();
        assert_eq!(bytes_of(&b), original);

        let mut with_mark = b.format.clone();
        with_mark.has_bom = true;
        b.change_format(with_mark);
        assert_eq!(&bytes_of(&b)[..3], b"\xEF\xBB\xBF");
        b.undo();
        assert_eq!(bytes_of(&b), original);
    }

    #[test]
    fn typing_in_petscii_follows_the_character_set_in_force() {
        let format = TextFormat { newline: crate::codec::NewlinePolicy::CrOnly, ..format_of("petscii-c64") };
        let mut b = decoded_buffer(&[0x41, 0x0E, 0x41, 0x0D, 0x41], format);
        b.move_caret(0, 1, false);
        b.set_desired_col();
        b.type_text("A");
        assert_eq!(bytes_of(&b), [0x41, 0x41, 0x0E, 0x41, 0x0D, 0x41], "uppercase state: A is 0x41");
        b.move_document_end(false);
        b.type_text("A");
        assert_eq!(bytes_of(&b), [0x41, 0x41, 0x0E, 0x41, 0x0D, 0x41, 0xC1], "after the switch: A is 0xC1");
        assert_eq!(b.state_at_caret().table, 1);
        b.insert_exact(&charset::raw_char(0x61).to_string());
        assert_eq!(*bytes_of(&b).last().unwrap(), 0x61, "the exact byte chosen, duplicate or not");
    }

    #[test]
    fn inserted_bytes_go_in_exactly_and_undo_as_one() {
        let mut b = decoded_buffer(b"ab", format_of("cp437"));
        b.move_caret(0, 1, false);
        b.insert_exact(&charset::raw_char(0x1B).to_string());
        assert_eq!(bytes_of(&b), b"a\x1Bb");
        b.undo();
        assert_eq!(bytes_of(&b), b"ab");
    }

    #[test]
    fn fixed_rows_overwrite_and_enter_adds_nothing() {
        let format = crate::codec::preset_by_id("teletext").unwrap().format();
        let mut data = vec![0x20u8; 80];
        data[0] = 0x01;
        let mut b = decoded_buffer(&data, format);
        b.move_caret(0, 0, false);
        b.move_horizontal(true, false, false);
        b.type_text("HI");
        assert_eq!(b.line(0).chars().count(), 40, "typing replaces cells");
        b.new_line();
        assert_eq!(b.line_count(), 2);
        assert_eq!(b.caret_line, 1);
        let saved = bytes_of(&b);
        assert_eq!(saved.len(), 80);
        assert_eq!(&saved[..3], &[0x01, b'H', b'I']);
        assert!(!saved.contains(&b'\r') && !saved.contains(&b'\n'));
        assert!(b.uneven_rows().is_empty());
    }

    fn teletext_page() -> TextBuffer {
        let format = crate::codec::preset_by_id("teletext").unwrap().format();
        let mut data = vec![0x20u8; 120];
        data[..5].copy_from_slice(b"ABCDE");
        decoded_buffer(&data, format)
    }

    fn widths(b: &TextBuffer) -> Vec<usize> {
        (0..b.line_count()).map(|r| b.line(r).chars().count()).collect()
    }

    #[test]
    fn teletext_rows_stay_forty_cells_however_they_are_edited() {
        let mut b = teletext_page();
        // Typing at the end of a row writes nothing past it.
        b.move_caret(0, 38, false);
        b.type_text("XYZ");
        assert_eq!(widths(&b), [40, 40, 40]);
        assert_eq!(bytes_of(&b)[38..40], *b"XY");
        assert_eq!(b.caret_col, 40);
        b.type_text("Q");
        assert_eq!(widths(&b), [40, 40, 40], "at the end of a row, typing does nothing");
        // Backspace and Delete close up the row and fill its end.
        b.move_caret(0, 2, false);
        b.backspace();
        assert_eq!(&bytes_of(&b)[..5], b"ACDE ");
        b.delete_forward();
        assert_eq!(&bytes_of(&b)[..4], b"ADE ");
        assert_eq!(widths(&b), [40, 40, 40]);
        // Backspace at the start of a row moves up, and doesn't join the rows.
        b.move_caret(1, 0, false);
        b.backspace();
        assert_eq!((b.caret_line, b.line_count()), (0, 3));
        // A selection over two rows is blanked, not taken out.
        b.move_caret(0, 1, false);
        b.move_caret(1, 5, true);
        b.delete_forward();
        assert_eq!(widths(&b), [40, 40, 40]);
        assert_eq!(bytes_of(&b)[0], b'A');
        assert!(bytes_of(&b)[1..45].iter().all(|byte| *byte == 0x20));
        // A paste of several lines writes over the rows below, clipped at the edge, and adds none.
        b.move_caret(0, 35, false);
        b.type_text("HELLO WORLD\r\nNEXT\r\nTHIRD\r\nFOURTH");
        assert_eq!(widths(&b), [40, 40, 40]);
        assert_eq!(b.line_count(), 3);
        let bytes = bytes_of(&b);
        assert_eq!(&bytes[35..40], b"HELLO");
        assert_eq!(&bytes[40..44], b"NEXT");
        assert_eq!(&bytes[80..85], b"THIRD");
        assert_eq!(bytes.len(), 120, "the file stays three rows");
        assert!(b.uneven_rows().is_empty());
        // It all comes back with Undo, a step at a time.
        for _ in 0..8 {
            b.undo();
        }
        assert_eq!(&bytes_of(&b)[..5], b"ABCDE");
    }

    #[test]
    fn a_wide_control_label_doesnt_change_how_many_cells_a_row_has() {
        // A control byte is drawn as a label several columns wide; the row is still counted in cells.
        let format = crate::codec::preset_by_id("teletext").unwrap().format();
        let mut data = vec![0x20u8; 80];
        data[0] = 0x01;
        let mut b = decoded_buffer(&data, format);
        b.move_caret(0, 0, false);
        for _ in 0..45 {
            b.move_horizontal(true, false, false);
        }
        b.type_text("WXYZ");
        assert_eq!(widths(&b), [40, 40]);
        assert_eq!(bytes_of(&b)[0], 0x01, "the control is untouched");
        assert_eq!(bytes_of(&b).len(), 80);
    }

    #[test]
    fn other_edits_to_rows_are_brought_back_to_the_width() {
        let mut b = teletext_page();
        // Replacing text with shorter text fills the row's end, so the rows after it stay where they are.
        b.move_caret(0, 1, false);
        b.move_caret(0, 5, true);
        b.replace_selection_or_all("Z");
        assert_eq!(widths(&b), [40, 40, 40]);
        assert_eq!(&bytes_of(&b)[..3], b"AZ ");
        // EBCDIC card images are rows of 80, and keep to it too.
        let format = crate::codec::preset_by_id("ibm-records").unwrap().format();
        let mut cards = decoded_buffer(&[0x40u8; 160], format);
        cards.move_caret(0, 79, false);
        cards.type_text("AB");
        assert_eq!(widths(&cards), [80, 80]);
        assert_eq!(bytes_of(&cards)[79], 0xC1, "A in EBCDIC");
    }

    #[test]
    fn an_edit_that_wont_fit_is_found_before_saving() {
        let mut b = decoded_buffer(b"abc", format_of("iso-8859-1"));
        b.type_text("\u{4E2D}");
        let problems = b.encode(&b.format, crate::codec::Lossy::Refuse).unwrap_err();
        assert_eq!(problems.listed[0].ch, '\u{4E2D}');
    }

    #[test]
    fn comments_go_on_and_come_off_in_one_step_lined_up_with_the_least_indented_line() {
        let marks = crate::tools::CommentMarks { line: Some("//"), block: Some(("/*", "*/")) };
        let mut b = buffer(&["    one", "", "  two", "three after"]);
        b.move_caret(0, 0, false);
        b.move_caret(2, 3, true);
        b.comment_lines(&marks);
        assert_eq!(text(&b), ["  //   one", "", "  // two", "three after"], "a blank line stays blank");
        b.uncomment_lines(&marks);
        assert_eq!(text(&b), ["    one", "", "  two", "three after"]);
        b.undo();
        b.undo();
        assert_eq!(text(&b), ["    one", "", "  two", "three after"]);
        assert!(!b.is_modified());
    }

    #[test]
    fn with_no_selection_only_the_cursors_line_is_commented() {
        let marks = crate::tools::CommentMarks { line: Some("#"), block: None };
        let mut b = buffer(&["a", "b"]);
        b.move_caret(1, 0, false);
        b.comment_lines(&marks);
        assert_eq!(text(&b), ["a", "# b"]);
        b.uncomment_lines(&marks);
        assert_eq!(text(&b), ["a", "b"]);
    }

    #[test]
    fn languages_with_only_block_comments_wrap_each_line() {
        let marks = crate::tools::CommentMarks { line: None, block: Some(("<!--", "-->")) };
        let mut b = buffer(&["  <p>x</p>"]);
        b.comment_lines(&marks);
        assert_eq!(text(&b), ["  <!-- <p>x</p> -->"]);
        b.uncomment_lines(&marks);
        assert_eq!(text(&b), ["  <p>x</p>"]);
    }

    #[test]
    fn nothing_to_uncomment_makes_no_undo_step() {
        let marks = crate::tools::CommentMarks { line: Some("#"), block: None };
        let mut b = buffer(&["plain"]);
        b.uncomment_lines(&marks);
        assert!(!b.can_undo());
        assert!(!b.is_modified());
        let mut blank = buffer(&["", "  "]);
        blank.select_all();
        blank.comment_lines(&marks);
        assert!(!blank.can_undo());
    }

    #[test]
    fn typed_letters_undo_together() {
        let mut b = buffer(&[""]);
        for c in ["a", "b", "c"] {
            b.type_text(c);
        }
        b.undo();
        assert_eq!(text(&b), [""]);
    }

    #[test]
    fn stream_selection_spanning_lines_deletes_cleanly() {
        let mut b = buffer(&["hello", "middle", "world"]);
        b.set_selection((0, 2), (2, 3), false);
        assert_eq!(b.selection_text(), "llo\r\nmiddle\r\nwor");
        b.delete_selection();
        assert_eq!(text(&b), ["held"]);
        assert_eq!((b.caret_line, b.caret_col), (0, 2));
    }

    #[test]
    fn non_ascii_text_is_edited_by_character_not_by_byte() {
        let mut b = buffer(&["caf\u{e9} \u{1F600}x", "abcdefgh"]);
        b.set_column_mode(true);
        b.set_selection((0, 4), (1, 4), false);
        b.type_text("|");
        assert_eq!(text(&b), ["caf\u{e9}| \u{1F600}x", "abcd|efgh"]);
        b.set_selection((0, 6), (0, 7), false);
        assert_eq!(b.selection_text(), "\u{1F600}");
        b.backspace();
        assert_eq!(&*b.line(0), "caf\u{e9}| x");
    }

    #[test]
    fn enter_keeps_indentation_and_splits_the_line() {
        let mut b = buffer(&["    abcd"]);
        b.move_caret(0, 6, false);
        b.new_line();
        assert_eq!(text(&b), ["    ab", "    cd"]);
        assert_eq!((b.caret_line, b.caret_col), (1, 4));
    }

    #[test]
    fn overwrite_replaces_rather_than_inserts() {
        let mut b = buffer(&["abc"]);
        b.overwrite = true;
        b.type_text("X");
        assert_eq!(text(&b), ["Xbc"]);
        let mut block = buffer(&["abc", "def"]);
        block.overwrite = true;
        block.set_column_mode(true);
        block.set_selection((0, 1), (1, 1), false);
        block.type_text("X");
        assert_eq!(text(&block), ["aXc", "dXf"]);
    }

    #[test]
    fn find_moves_forward_wraps_and_selects() {
        let mut b = buffer(&["one two", "two one"]);
        let o = SearchOptions { pattern: "TWO".into(), ..Default::default() };
        assert_eq!(b.find(&o), Ok(true));
        assert_eq!((b.caret_line, b.anchor_col, b.caret_col), (0, 4, 7));
        assert_eq!(b.find(&o), Ok(true));
        assert_eq!((b.caret_line, b.anchor_col, b.caret_col), (1, 0, 3));
        assert_eq!(b.find(&o), Ok(true));
        assert_eq!((b.caret_line, b.anchor_col), (0, 4));
    }

    #[test]
    fn find_backward_and_options() {
        let mut b = buffer(&["cat concat Cat"]);
        b.move_document_end(false);
        let back = SearchOptions { pattern: "cat".into(), backward: true, match_case: true, ..Default::default() };
        assert_eq!(b.find(&back), Ok(true));
        assert_eq!(b.anchor_col, 7);
        assert_eq!(b.find(&back), Ok(true));
        assert_eq!(b.anchor_col, 0);
        let whole = SearchOptions { pattern: "cat".into(), whole_word: true, ..Default::default() };
        b.move_caret(0, 1, false);
        assert_eq!(b.find(&whole), Ok(true));
        assert_eq!(b.anchor_col, 11);
        let none = SearchOptions { pattern: "dog".into(), ..Default::default() };
        assert_eq!(b.find(&none), Ok(false));
        let bad = SearchOptions { pattern: "(".into(), regex: true, ..Default::default() };
        assert!(b.find(&bad).is_err());
    }

    #[test]
    fn find_positions_are_right_after_non_ascii_text() {
        let mut b = buffer(&["\u{e9}\u{e9} x"]);
        let o = SearchOptions { pattern: "x".into(), ..Default::default() };
        assert_eq!(b.find(&o), Ok(true));
        assert_eq!((b.anchor_col, b.caret_col), (3, 4));
    }

    #[test]
    fn replace_keeps_dollar_signs_literal_unless_regex() {
        let mut b = buffer(&["a1 a2"]);
        let literal = SearchOptions { pattern: "a".into(), replacement: "$0".into(), ..Default::default() };
        assert_eq!(b.replace_all(&literal), Ok(2));
        assert_eq!(&*b.line(0), "$01 $02");
        b.undo();
        assert_eq!(&*b.line(0), "a1 a2");
        let re = SearchOptions { pattern: r"a(\d)".into(), replacement: "<$1>".into(), regex: true, ..Default::default() };
        assert_eq!(b.replace_all(&re), Ok(2));
        assert_eq!(&*b.line(0), "<1> <2>");
    }

    #[test]
    fn replace_one_at_a_time() {
        let mut b = buffer(&["x x"]);
        let o = SearchOptions { pattern: "x".into(), replacement: "yy".into(), ..Default::default() };
        assert_eq!(b.find(&o), Ok(true));
        assert_eq!(b.replace_current(&o), Ok(true));
        assert_eq!(&*b.line(0), "yy x");
        assert_eq!(b.anchor_col, 3);
        assert_eq!(b.replace_current(&o), Ok(false));
        assert_eq!(&*b.line(0), "yy yy");
    }

    #[test]
    fn leaving_column_mode_puts_the_caret_back_on_real_text() {
        let mut b = buffer(&["ab"]);
        b.set_column_mode(true);
        b.move_caret(0, 10, false);
        assert_eq!(b.caret_col, 10);
        b.set_column_mode(false);
        assert_eq!(b.caret_col, 2);
    }

    #[test]
    fn select_all_in_column_mode_covers_the_widest_line() {
        let mut b = buffer(&["longest line", "x"]);
        b.set_column_mode(true);
        b.select_all();
        assert_eq!(b.selection_text(), "longest line\r\nx           ");
    }

    #[test]
    fn alt_press_and_drag_makes_a_block_outside_column_mode() {
        let mut b = buffer(&["abcdef", "gh", "ijklmn"]);
        b.press_for_block(0, 4, false);
        assert!(b.is_block());
        b.move_caret(2, 2, true);
        assert!(b.is_block());
        assert_eq!(b.selection_text(), "cd\r\n  \r\nkl");
        b.type_text("Z");
        assert_eq!(text(&b), ["abZef", "ghZ", "ijZmn"]);
        // A plain click afterwards goes back to ordinary selecting.
        b.end_temporary_block();
        b.move_caret(0, 99, false);
        assert!(!b.is_block());
        assert_eq!(b.caret_col, 5);
    }

    #[test]
    fn alt_press_past_the_end_of_a_line_keeps_its_column() {
        let mut b = buffer(&["ab", "cd"]);
        b.press_for_block(0, 6, false);
        b.move_caret(1, 6, true);
        b.type_text("|");
        assert_eq!(text(&b), ["ab    |", "cd    |"]);
    }

    #[test]
    fn moving_lines_keeps_them_selected_and_stops_at_the_ends() {
        let mut b = buffer(&["a", "b", "c"]);
        b.move_caret(1, 0, false);
        b.move_lines(true);
        assert_eq!(text(&b), ["b", "a", "c"]);
        assert_eq!(b.caret_line, 0);
        b.move_lines(true);
        assert_eq!(text(&b), ["b", "a", "c"]);
        b.set_selection((0, 0), (1, 1), false);
        b.move_lines(false);
        assert_eq!(text(&b), ["c", "b", "a"]);
        assert_eq!((b.anchor_line, b.caret_line), (1, 2));
        b.move_lines(false);
        assert_eq!(text(&b), ["c", "b", "a"]);
        b.undo();
        assert_eq!(text(&b), ["b", "a", "c"]);
    }

    #[test]
    fn a_selection_ending_at_column_zero_leaves_that_line_out() {
        let mut b = buffer(&["a", "b", "c"]);
        b.set_selection((0, 0), (1, 0), false);
        b.duplicate_lines();
        assert_eq!(text(&b), ["a", "a", "b", "c"]);
    }

    #[test]
    fn duplicating_moves_onto_the_copy() {
        let mut b = buffer(&["a", "b"]);
        b.move_caret(0, 1, false);
        b.duplicate_lines();
        assert_eq!(text(&b), ["a", "a", "b"]);
        assert_eq!((b.caret_line, b.caret_col), (1, 1));
    }

    #[test]
    fn joining_lines_uses_one_space() {
        let mut b = buffer(&["one  ", "   two", "", "three", "four"]);
        b.set_selection((0, 0), (3, 2), false);
        b.join_lines();
        assert_eq!(text(&b), ["one two three", "four"]);
        b.move_caret(0, 0, false);
        b.join_lines();
        assert_eq!(text(&b), ["one two three four"]);
        b.join_lines();
        assert_eq!(text(&b), ["one two three four"]);
    }

    #[test]
    fn sorting_covers_the_selection_or_the_file() {
        let mut b = buffer(&["c", "a", "b"]);
        b.sort_lines(false);
        assert_eq!(text(&b), ["a", "b", "c"]);
        b.sort_lines(true);
        assert_eq!(text(&b), ["c", "b", "a"]);
        b.set_selection((1, 0), (2, 1), false);
        b.sort_lines(false);
        assert_eq!(text(&b), ["c", "a", "b"]);
    }

    #[test]
    fn trimming_and_tab_conversion() {
        let mut b = buffer(&["  a  ", "\tb\t"]);
        b.trim_trailing();
        assert_eq!(text(&b), ["  a", "\tb"]);
        b.spaces_to_tabs();
        assert_eq!(text(&b), ["  a", "\tb"]);
        b.tabs_to_spaces();
        assert_eq!(text(&b), ["  a", "        b"]);
        b.spaces_to_tabs();
        assert_eq!(text(&b), ["  a", "\tb"]);
        b.trim_leading();
        assert_eq!(text(&b), ["a", "b"]);
        assert!(b.can_undo());
    }

    #[test]
    fn a_command_that_changes_nothing_adds_no_undo_step() {
        let mut b = buffer(&["clean"]);
        b.trim_trailing();
        assert!(!b.can_undo());
        assert!(!b.is_modified());
    }

    #[test]
    fn counts_follow_the_original_rules() {
        let c = count_lines(&["one two", "", "   ", "three"], 2);
        assert_eq!(c.words, 3);
        assert_eq!(c.lines, 4);
        assert_eq!(c.non_space, 11);
        assert_eq!(c.characters, 7 + 3 + 5 + 3 * 2, "line endings are counted as characters");
        assert_eq!(c.non_empty, 2, "a line of only spaces doesn't count");
        assert_eq!(c.average, 15 / 4, "line endings aren't in the average, and it rounds down");
        assert_eq!((c.longest_chars, c.longest_line), (7, 1));
    }

    #[test]
    fn counting_at_the_edges_of_a_file() {
        let empty = count_lines(&[""], 2);
        assert_eq!((empty.lines, empty.non_empty, empty.longest_line, empty.average), (1, 1, 1, 0));
        let ends_with_newline = count_lines(&["a", ""], 2);
        assert_eq!((ends_with_newline.lines, ends_with_newline.non_empty), (2, 1));
        let trailing_spaces = count_lines(&["a", "  "], 2);
        assert_eq!(trailing_spaces.non_empty, 2, "the last line counts unless the file ends in a line ending");
        let equal = count_lines(&["abc", "xyz"], 1);
        assert_eq!(equal.longest_line, 1, "the first of equally long lines");
        assert_eq!(equal.characters, 7);
    }

    #[test]
    fn a_word_runs_across_nothing_but_breaks_at_line_ends() {
        assert_eq!(count_lines(&["end", "start"], 2).words, 2);
        assert_eq!(count_lines(&["a-b c_d"], 2).words, 2);
    }

    #[test]
    fn combining_marks_count_with_their_letter() {
        let c = count_lines(&["e\u{301}x", "\u{301}"], 2);
        assert_eq!(c.non_space, 3, "the mark after e joins it; one on its own is counted");
        assert_eq!(c.longest_chars, 2);
    }

    #[test]
    fn selection_counts_use_the_selection() {
        let mut b = buffer(&["one two", "three"]);
        b.set_selection((0, 4), (1, 2), false);
        let c = b.counts(2);
        assert_eq!((c.words, c.lines, c.non_space), (2, 2, 5));
    }

    #[test]
    fn braces_match_across_lines_and_through_nesting() {
        let mut b = buffer(&["f(a, {", "", "  [x]", "})"]);
        b.move_caret(0, 1, false);
        assert!(b.go_to_brace(false));
        assert_eq!((b.caret_line, b.caret_col), (3, 1));
        assert!(b.go_to_brace(false));
        assert_eq!((b.caret_line, b.caret_col), (0, 1));
        b.move_caret(2, 2, false);
        assert!(b.go_to_brace(true));
        assert_eq!(b.selection_text(), "[x]");
        b.move_caret(0, 0, false);
        assert!(!b.go_to_brace(false));
        let mut open = buffer(&["(never closed", ""]);
        assert!(!open.go_to_brace(false));
    }

    fn disk_buffer(tag: &str, content: &str) -> (TextBuffer, std::path::PathBuf) {
        let dir = std::env::temp_dir().join(format!("limoniter-diskbuf-{tag}-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("f.txt");
        std::fs::write(&path, content).unwrap();
        let source = Source::open(&path).unwrap();
        while !source.is_finished() {
            std::thread::sleep(Duration::from_millis(1));
        }
        let mut b = TextBuffer::default();
        b.set_source(source);
        (b, dir)
    }

    #[test]
    fn column_editing_undo_and_search_work_on_a_file_on_disk() {
        let content: Vec<String> = (0..1000).map(|i| format!("row{i:04} value")).collect();
        let (mut b, dir) = disk_buffer("column", &content.join("\n"));
        assert!(b.is_disk_backed());
        assert_eq!(b.line_count(), 1000);

        b.set_column_mode(true);
        b.set_selection((500, 7), (502, 7), false);
        b.type_text("|");
        assert_eq!(&*b.line(500), "row0500| value");
        assert_eq!(&*b.line(502), "row0502| value");
        assert_eq!(&*b.line(503), "row0503 value");
        assert!(b.is_modified());
        b.undo();
        assert_eq!(&*b.line(500), "row0500 value");
        assert!(!b.is_modified());
        b.redo();
        assert_eq!(&*b.line(501), "row0501| value");
        b.set_column_mode(false);

        let o = SearchOptions { pattern: "row0900".into(), ..Default::default() };
        b.move_caret(0, 0, false);
        assert_eq!(b.find(&o), Ok(true));
        assert_eq!(b.caret_line, 900);
        let back = SearchOptions { pattern: "row0100".into(), backward: true, ..Default::default() };
        assert_eq!(b.find(&back), Ok(true));
        assert_eq!(b.caret_line, 100);

        let all = SearchOptions { pattern: "value".into(), replacement: "v".into(), ..Default::default() };
        assert_eq!(b.replace_all(&all), Ok(1000));
        assert_eq!(&*b.line(999), "row0999 v");
        b.undo();
        assert_eq!(&*b.line(999), "row0999 value");
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn deleting_a_huge_selection_needs_no_reading() {
        let content: Vec<String> = (0..12000).map(|i| format!("{i}")).collect();
        let (mut b, dir) = disk_buffer("delete", &content.join("\n"));
        b.set_selection((10, 0), (11990, 0), false);
        assert!(b.position_text().contains("Selected 11981 lines"), "{}", b.position_text());
        b.delete_selection();
        assert_eq!(b.line_count(), 20);
        assert_eq!(&*b.line(9), "9");
        assert_eq!(&*b.line(10), "11990");
        b.undo();
        assert_eq!(b.line_count(), 12000);
        assert_eq!(&*b.line(11), "11");
        std::fs::remove_dir_all(&dir).unwrap();
    }

    /// A full trial on a real huge file, run by hand: set LIMONITER_BIG_FILE to a copy that may be
    /// changed, then run the ignored tests in release mode. It prints how long each step took.
    #[test]
    #[ignore]
    fn trial_on_a_real_huge_file() {
        let path = std::path::PathBuf::from(std::env::var("LIMONITER_BIG_FILE").expect("set LIMONITER_BIG_FILE"));
        let began = Instant::now();
        let source = Source::open(&path).unwrap();
        let mut b = TextBuffer::default();
        b.set_source(source.clone());
        println!("file opened: {:?}", began.elapsed());
        while source.known_lines() == 0 {
            std::thread::sleep(Duration::from_micros(200));
        }
        b.refresh_from_source();
        assert!(!b.line(0).is_empty());
        println!("first screen of text readable: {:?}", began.elapsed());
        while !source.is_finished() {
            std::thread::sleep(Duration::from_millis(5));
        }
        b.refresh_from_source();
        println!("all {} lines found: {:?}", b.line_count(), began.elapsed());
        let format = source.format();

        let last = b.line_count() - 1;
        let step = Instant::now();
        b.move_caret(last, 0, false);
        b.type_text("x");
        b.move_caret(0, 0, false);
        b.type_text("y");
        b.move_caret(22_099_999, 0, false);
        b.type_text("z");
        println!("jump and edit at the end, start and middle: {:?}", step.elapsed());
        assert_eq!(b.store().memory_lines(), 3, "only the edited lines are in memory");

        let step = Instant::now();
        let needle = SearchOptions { pattern: "NEEDLE-IN".into(), match_case: true, ..Default::default() };
        b.move_caret(0, 0, false);
        assert_eq!(b.find(&needle), Ok(true));
        println!("search to line {}: {:?}", b.caret_line + 1, step.elapsed());

        let step = Instant::now();
        b.store().write_file(&path, &format, &std::sync::atomic::AtomicU64::new(0)).unwrap();
        println!("save: {:?}", step.elapsed());
    }

    #[test]
    fn replacing_many_lines_at_once_gives_what_one_at_a_time_would() {
        let text = "a1\r\nb\nc1\rd\ne1\r\nf1\n";
        let (mut b, dir) = disk_buffer("many", &text.repeat(300));
        let o = SearchOptions { pattern: "1".into(), replacement: "22".into(), ..Default::default() };
        let count = b.line_count();
        assert_eq!(b.replace_all(&o), Ok(1200));
        assert_eq!(b.line_count(), count, "no lines come or go");
        b.bookmarks();
        let path = dir.join("out.txt");
        let format = b.format.clone();
        b.store().write_file(&path, &format, &std::sync::atomic::AtomicU64::new(0)).unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), text.replace('1', "22").repeat(300), "each line keeps the ending it had");
        assert_eq!(b.change_at(0), Some(false));
        assert_eq!(b.change_at(1), None);
        b.undo();
        assert_eq!(&*b.line(0), "a1");
        assert!(b.store().memory_lines() == 0, "and it's all back on disk");
        b.redo();
        assert_eq!(&*b.line(0), "a22");
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn replacing_many_lines_keeps_folds_away_from_them_and_opens_those_they_touch() {
        let mut b = buffer(&["top", "    x", "next", "    y", "z"]);
        b.toggle_fold(0);
        b.toggle_fold(2);
        let o = SearchOptions { pattern: "y".into(), replacement: "w".into(), ..Default::default() };
        assert_eq!(b.replace_all(&o), Ok(1));
        assert!(b.is_folded(0));
        assert!(!b.is_folded(2), "its hidden line changed");
    }

    #[test]
    fn a_replace_too_big_to_hold_changes_nothing_and_says_so() {
        let (mut b, dir) = disk_buffer("too-big", &"needle line\n".repeat(100));
        let o = SearchOptions { pattern: "needle".into(), replacement: "pin".into(), ..Default::default() };
        let mut cursor = b.begin_replace_all(&o).unwrap().unwrap();
        cursor.most_held = 100;
        while b.replace_step(&mut cursor, Duration::from_secs(60)).is_some() {}
        assert!(cursor.too_large);
        assert_eq!(cursor.count, 0);
        assert!(!b.is_modified());
        assert!(!b.can_undo());
        assert_eq!(&*b.line(0), "needle line");
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_search_can_pause_and_carry_on() {
        let (mut b, dir) = disk_buffer("pause", &format!("{}needle", "line\n".repeat(2000)));
        let o = SearchOptions { pattern: "needle".into(), ..Default::default() };
        let mut cursor = b.begin_find(&o).unwrap().unwrap();
        let mut slices = 0;
        loop {
            match b.find_step(&mut cursor, Duration::ZERO) {
                SearchStep::Running(_) => slices += 1,
                SearchStep::Found => break,
                SearchStep::NotFound => panic!("should be found"),
            }
        }
        assert!(slices > 5, "it should have paused, not run in one go: {slices}");
        assert_eq!(b.caret_line, 2000);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn bookmarks_follow_their_lines_and_can_be_stepped_through() {
        let mut b = buffer(&["a", "b", "c", "d"]);
        b.toggle_bookmark(1);
        b.toggle_bookmark(3);
        assert_eq!(b.bookmarks(), [1, 3]);
        b.move_caret(0, 0, false);
        b.new_line();
        assert_eq!(b.bookmarks(), [2, 4], "a line added above moves them down");
        b.set_selection((2, 0), (3, 0), false);
        b.delete_selection();
        assert_eq!(b.bookmarks(), [3], "a deleted line takes its bookmark with it");
        b.move_caret(0, 0, false);
        assert!(b.go_to_bookmark(true));
        assert_eq!(b.caret_line, 3);
        assert!(b.go_to_bookmark(true), "it wraps round");
        assert_eq!(b.caret_line, 3);
        b.toggle_bookmark(3);
        assert!(!b.go_to_bookmark(false));
        b.move_caret(0, 0, false);
        b.type_text("x");
        assert_eq!(b.char_at_caret(), None, "the caret is past the end of a one-letter line");
        b.move_caret(0, 0, false);
        assert_eq!(b.char_at_caret(), Some('x'));
    }

    #[test]
    fn replacing_the_whole_document_or_a_selection_is_one_undo_step() {
        let mut b = buffer(&["one", "two"]);
        assert_eq!(b.selection_or_all().as_deref(), Some("one\r\ntwo"));
        b.replace_selection_or_all("new\nlines\nhere");
        assert_eq!(text(&b), ["new", "lines", "here"]);
        b.undo();
        assert_eq!(text(&b), ["one", "two"]);
        b.set_selection((0, 1), (0, 3), false);
        assert_eq!(b.selection_or_all().as_deref(), Some("ne"));
        b.replace_selection_or_all("X");
        assert_eq!(text(&b), ["oX", "two"]);
    }

    #[test]
    fn changing_the_tab_size_keeps_the_caret_on_its_character() {
        let mut b = buffer(&["\tabc", "x\ty"]);
        b.move_caret_to_char(0, 2);
        assert_eq!(b.caret_col, 9);
        let (caret, anchor) = (b.caret_char_position(), b.anchor_char_position());
        set_tab_size(4);
        b.after_tab_size_change(caret, anchor);
        assert_eq!(b.caret_col, 5);
        assert_eq!(b.caret_char_position(), (0, 2));
        assert_eq!(visual_len(b.line(1).as_ref()), 5, "x, then a tab to column 4, then y");
        set_tab_size(0);
        assert_eq!(tab_size(), MIN_TAB_SIZE, "a tab is always at least one column");
        set_tab_size(1000);
        assert_eq!(tab_size(), MAX_TAB_SIZE);
        set_tab_size(DEFAULT_TAB_SIZE);
    }

    #[test]
    fn go_to_rejects_nonsense() {
        let mut b = buffer(&["a", "b"]);
        assert!(b.go_to("2"));
        assert_eq!(b.caret_line, 1);
        assert!(b.go_to("99"));
        assert_eq!(b.caret_line, 1);
        assert!(!b.go_to("0"));
        assert!(!b.go_to("abc"));
    }
}
