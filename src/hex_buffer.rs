//! The bytes being edited in hex mode. Nothing here draws anything.

use crate::byte_store::{ByteStore, Compare, FileBytes, Piece, ReplaceAll};
use crate::text_buffer::SearchStep;
use crate::ebcdic;
use std::sync::Arc;
use crate::text_buffer::SearchOptions;

thread_local! {
    /// How many bytes hex mode shows on a row, from Settings.
    static BYTES_PER_ROW: std::cell::Cell<usize> = const { std::cell::Cell::new(16) };
}

pub fn bytes_per_row() -> usize {
    BYTES_PER_ROW.with(std::cell::Cell::get)
}

pub fn set_bytes_per_row(count: usize) {
    BYTES_PER_ROW.with(|cell| cell.set(count.clamp(1, 256)));
}

const MAX_UNDO: usize = 2000;
/// The most Insert/delete will add in one go. Each run is also held for undo, so it costs twice this.
const MOST_INSERTED: usize = 64 * 1024 * 1024;
/// The most Copy selected will lay out in one go: each row becomes about five times its size as text.
pub const MOST_VIEW_COPIED: usize = 16 * 1024 * 1024;

thread_local! {
    /// How many hex digits the offsets have: eight, or more for a file past 4 GB.
    static OFFSET_DIGITS: std::cell::Cell<usize> = const { std::cell::Cell::new(8) };
}

/// Sets the offset column wide enough for a file of this many bytes.
pub fn set_offset_width(len: usize) {
    let digits = format!("{:X}", len.max(1)).len().max(8);
    OFFSET_DIGITS.with(|cell| cell.set(digits));
}

pub fn offset_digits() -> usize {
    OFFSET_DIGITS.with(std::cell::Cell::get)
}

/// An offset as the hex view shows it.
pub fn offset_text(offset: usize) -> String {
    format!("{offset:0width$X}", width = offset_digits())
}

fn offset_cells() -> usize {
    offset_digits() + 2
}
pub const ASCII_GAP_CELLS: usize = 2;

/// Where a byte's hex digits start on a row, in character cells.
pub fn hex_cell(i: usize) -> usize {
    offset_cells() + i * 3 + usize::from(i >= bytes_per_row() / 2)
}

/// Where a byte's character goes on a row, in character cells.
pub fn ascii_cell(i: usize) -> usize {
    hex_cell(bytes_per_row()) + ASCII_GAP_CELLS + i
}

struct Edit {
    position: usize,
    /// The pieces that were there before, and the ones put in their place, with how many bytes each stands for.
    old: Vec<Piece>,
    old_len: usize,
    new: Vec<Piece>,
    new_len: usize,
    version_before: u64,
    version_after: u64,
    caret_before: usize,
    nibble_before: u8,
    caret_after: usize,
    nibble_after: u8,
}

/// A search under way in a file left on disk: the stretches still to look through, last first.
pub struct HexSearch {
    pattern: Vec<u8>,
    compare: Compare,
    backward: bool,
    parts: Vec<(usize, usize)>,
    total: usize,
    covered: usize,
}

#[derive(Default)]
pub struct HexBuffer {
    /// Set by the Read-only switch; the window and the views don't let edits through while it's on.
    pub read_only: bool,
    data: ByteStore,
    pub caret: usize,
    pub anchor: usize,
    /// Which half of the byte typing goes to: 0 is the first digit, 1 the second.
    pub nibble: u8,
    pub ascii_pane: bool,
    pub insert_mode: bool,
    /// The character pane reads the bytes as EBCDIC. This only changes how they're read: they stay as they are.
    pub ebcdic: bool,
    /// Something the person should be told, once.
    pub notice: Option<String>,
    version: u64,
    next_version: u64,
    saved_version: u64,
    undo: Vec<Edit>,
    redo: Vec<Edit>,
    merge_nibble: bool,
    /// Set when the view should scroll the caret into sight.
    pub reveal: bool,
}

/// Reads "4A 6F" or "4a6f"; returns None if it isn't a whole number of hex bytes.
pub fn parse_hex(text: &str) -> Option<Vec<u8>> {
    let digits: Vec<u8> = text.chars().filter(|c| !c.is_whitespace()).map(|c| c.to_digit(16).map(|d| d as u8)).collect::<Option<_>>()?;
    if digits.is_empty() || !digits.len().is_multiple_of(2) {
        return None;
    }
    Some(digits.as_chunks::<2>().0.iter().map(|[high, low]| (high << 4) | low).collect())
}

/// Reads a count or offset: decimal, or hex written with 0x.
pub fn parse_number(text: &str) -> Option<usize> {
    let t = text.trim();
    match t.strip_prefix("0x").or_else(|| t.strip_prefix("0X")) {
        Some(hex) => usize::from_str_radix(hex, 16).ok(),
        None => t.parse().ok(),
    }
}

/// Checks what was typed into Insert/delete, and gives the number of bytes and the value to fill with.
pub fn parse_run(count: &str, delete: bool, fill: &str) -> Result<(usize, u8), String> {
    let count = parse_number(count).filter(|n| *n > 0).ok_or("The number of bytes has to be a whole number above 0. Use 0x for hex.")?;
    if delete {
        return Ok((count, 0));
    }
    if count > MOST_INSERTED {
        return Err("That's too many to insert. It can be up to 64 MB at a time.".to_string());
    }
    match parse_hex(fill).as_deref() {
        Some([value]) => Ok((count, *value)),
        _ => Err("The fill value has to be one byte in hex, such as 00 or 20.".to_string()),
    }
}

impl HexBuffer {
    /// The bytes for some text, as the character pane reads them. None if EBCDIC has no byte for part of it.
    fn text_bytes(&self, text: &str) -> Option<Vec<u8>> {
        if self.ebcdic { text.chars().map(ebcdic::byte_of).collect() } else { Some(text.as_bytes().to_vec()) }
    }

    fn pattern_bytes(&self, text: &str, hex: bool) -> Result<Vec<u8>, String> {
        if hex {
            parse_hex(text).ok_or_else(|| "That isn't valid hex: use pairs of digits, like 4A 6F.".to_string())
        } else {
            self.text_bytes(text).ok_or_else(|| "That text has characters EBCDIC doesn't have.".to_string())
        }
    }

    fn replacement_bytes(&self, o: &SearchOptions) -> Result<Vec<u8>, String> {
        if o.hex_bytes && o.replacement.trim().is_empty() {
            return Ok(Vec::new());
        }
        self.pattern_bytes(&o.replacement, o.hex_bytes)
    }

    /// Changes whenever the bytes do, so that panels know when to look again.
    pub fn version(&self) -> u64 {
        self.version
    }

    pub fn len(&self) -> usize {
        self.data.len()
    }

    pub fn byte(&self, i: usize) -> u8 {
        self.data.byte(i)
    }

    /// Every byte, in memory. For a file left on disk use `read` or `write_to` instead.
    pub fn bytes(&self) -> Vec<u8> {
        self.data.to_vec()
    }

    /// Bytes start..start+count, or as many as there are.
    pub fn read(&self, start: usize, count: usize) -> Vec<u8> {
        self.data.read(start, count)
    }

    /// Whether the bytes are read from the file as they're needed, rather than all held in memory.
    pub fn is_on_disk(&self) -> bool {
        self.data.is_on_disk()
    }

    /// The file being read from, when it's left on disk.
    pub fn source(&self) -> Option<&Arc<FileBytes>> {
        self.data.source()
    }

    /// A copy of the bytes as they stand, cheap for a file left on disk, for saving in the background.
    pub fn snapshot(&self) -> ByteStore {
        self.data.clone()
    }

    /// After saving a file left on disk: reads from the saved file from now on, where the cursor was.
    /// Undo starts again, since what it would undo to was in the old file.
    pub fn rebase(&mut self, source: Arc<FileBytes>) {
        let (caret, anchor, scrolled) = (self.caret, self.anchor, self.reveal);
        self.load_file(source);
        self.caret = caret.min(self.len());
        self.anchor = anchor.min(self.len());
        self.reveal = scrolled;
    }

    pub fn has_selection(&self) -> bool {
        self.caret != self.anchor
    }

    pub fn is_modified(&self) -> bool {
        self.version != self.saved_version
    }

    pub fn can_undo(&self) -> bool {
        !self.undo.is_empty()
    }

    pub fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }

    pub fn load(&mut self, bytes: Vec<u8>) {
        self.load_store(ByteStore::from_vec(bytes));
    }

    /// Opens a file of any size where it lies, reading it as it's looked at.
    pub fn load_file(&mut self, source: Arc<FileBytes>) {
        self.load_store(ByteStore::from_file(source));
    }

    /// The pieces and the bytes they add, for a recovery copy of a file left on disk.
    pub fn image(&self) -> (Vec<Piece>, Vec<u8>) {
        self.data.image()
    }

    /// Brings back edits to a file left on disk, as unsaved changes. False if they don't fit the file.
    pub fn load_image(&mut self, source: Arc<FileBytes>, pieces: Vec<Piece>, added: Vec<u8>) -> bool {
        match ByteStore::from_image(source, pieces, added) {
            Some(store) => {
                self.load_store(store);
                true
            }
            None => false,
        }
    }

    pub(crate) fn load_store(&mut self, store: ByteStore) {
        self.data = store;
        self.caret = 0;
        self.anchor = 0;
        self.nibble = 0;
        self.undo.clear();
        self.redo.clear();
        self.merge_nibble = false;
        self.next_version += 1;
        self.version = self.next_version;
        self.saved_version = self.version;
        self.reveal = true;
    }

    pub fn mark_saved(&mut self) {
        self.saved_version = self.version;
    }

    pub fn position_text(&self) -> String {
        let mut s = format!("Offset 0x{0:08X} ({0})", self.caret);
        if self.caret < self.data.len() {
            s += &format!("   Value 0x{0:02X} ({0})", self.data.byte(self.caret));
        }
        if self.has_selection() {
            s += &format!("   Selected {} bytes", self.caret.abs_diff(self.anchor));
        }
        s += &format!("   Size {} bytes", self.data.len());
        s
    }

    // ---------- Edits and undo ----------

    fn apply(&mut self, position: usize, remove: usize, insert: Vec<u8>, caret: usize, nibble: u8, merge: bool) {
        let pieces = self.data.add(&insert);
        self.apply_pieces(position, remove, pieces, caret, nibble, merge);
    }

    fn apply_pieces(&mut self, position: usize, remove: usize, insert: Vec<Piece>, caret: usize, nibble: u8, merge: bool) {
        let new_len = insert.iter().map(|p| p.len).sum();
        let old = self.data.replace(position, remove, &insert);
        let old_len = old.iter().map(|p| p.len).sum();
        self.next_version += 1;
        let version = self.next_version;
        let merged = merge && self.merge_nibble && self.undo.last().is_some_and(|e| e.position == position && e.new_len == new_len);
        if merged {
            let last = self.undo.last_mut().unwrap();
            last.new = insert;
            last.caret_after = caret;
            last.nibble_after = nibble;
            last.version_after = version;
        } else {
            self.undo.push(Edit {
                position,
                old,
                old_len,
                new: insert,
                new_len,
                version_before: self.version,
                version_after: version,
                caret_before: self.caret,
                nibble_before: self.nibble,
                caret_after: caret,
                nibble_after: nibble,
            });
            if self.undo.len() > MAX_UNDO {
                self.undo.remove(0);
            }
        }
        self.version = version;
        self.redo.clear();
        self.caret = caret;
        self.anchor = caret;
        self.nibble = nibble;
        self.merge_nibble = nibble == 1;
        self.reveal = true;
    }

    pub fn undo(&mut self) {
        if let Some(e) = self.undo.pop() {
            self.data.replace(e.position, e.new_len, &e.old);
            self.version = e.version_before;
            self.caret = e.caret_before;
            self.anchor = e.caret_before;
            self.nibble = e.nibble_before;
            self.merge_nibble = false;
            self.reveal = true;
            self.redo.push(e);
        }
    }

    pub fn redo(&mut self) {
        if let Some(e) = self.redo.pop() {
            self.data.replace(e.position, e.old_len, &e.new);
            self.version = e.version_after;
            self.caret = e.caret_after;
            self.anchor = e.caret_after;
            self.nibble = e.nibble_after;
            self.merge_nibble = false;
            self.reveal = true;
            self.undo.push(e);
        }
    }

    /// In overwrite mode a selection is only a starting point; in insert mode typing replaces it.
    fn prepare_typing(&mut self) {
        if self.has_selection() {
            if self.insert_mode {
                self.delete_selection();
            } else {
                self.caret = self.caret.min(self.anchor);
                self.anchor = self.caret;
                self.nibble = 0;
                self.merge_nibble = false;
            }
        }
    }

    /// Types one hex digit (0-15) at the caret, first digit of the byte first.
    pub fn type_hex_digit(&mut self, digit: u8) {
        self.prepare_typing();
        let at = self.caret;
        if at >= self.data.len() || (self.insert_mode && self.nibble == 0) {
            self.apply(at, 0, vec![digit << 4], at, 1, false);
            return;
        }
        let old = self.data.byte(at);
        if self.nibble == 0 {
            self.apply(at, 1, vec![(digit << 4) | (old & 0x0F)], at, 1, false);
        } else {
            let next = (at + 1).min(self.data.len());
            self.apply(at, 1, vec![(old & 0xF0) | digit], next, 0, true);
        }
    }

    /// Types one byte in the character pane.
    pub fn type_byte(&mut self, value: u8) {
        self.prepare_typing();
        self.nibble = 0;
        self.merge_nibble = false;
        let remove = usize::from(!self.insert_mode && self.caret < self.data.len());
        self.apply(self.caret, remove, vec![value], self.caret + 1, 0, false);
    }

    /// Types one character in the character pane, as the pane is reading the bytes.
    pub fn type_char(&mut self, c: char) {
        let byte = if self.ebcdic { ebcdic::byte_of(c) } else { c.is_ascii().then_some(c as u8) };
        match byte {
            Some(byte) => self.type_byte(byte),
            None => self.paste_text(c.encode_utf8(&mut [0; 4])),
        }
    }

    /// Where Insert/delete works from: the start of the selection, or the caret.
    fn run_start(&self) -> usize {
        self.caret.min(self.anchor)
    }

    /// Adds bytes at the cursor, moving the rest along, however typing is set. They end up selected, to show what's new.
    pub fn insert_run(&mut self, count: usize, fill: u8) {
        let start = self.run_start();
        self.merge_nibble = false;
        self.apply(start, 0, vec![fill; count], start, 0, false);
        self.anchor = start + count;
    }

    /// Removes bytes from the cursor on, or as many as there are; gives how many went.
    pub fn delete_run(&mut self, count: usize) -> usize {
        let start = self.run_start();
        let count = count.min(self.data.len() - start);
        if count > 0 {
            self.merge_nibble = false;
            self.apply(start, count, Vec::new(), start, 0, false);
        }
        count
    }

    /// Puts exact bytes in at the caret, as one undoable action, over the selection if there is one.
    pub fn insert_bytes(&mut self, bytes: &[u8]) {
        if bytes.is_empty() {
            return;
        }
        self.prepare_typing();
        self.nibble = 0;
        self.merge_nibble = false;
        let remove = if self.insert_mode { 0 } else { bytes.len().min(self.data.len() - self.caret) };
        self.apply(self.caret, remove, bytes.to_vec(), self.caret + bytes.len(), 0, false);
    }

    pub fn delete_selection(&mut self) {
        if self.has_selection() {
            let start = self.caret.min(self.anchor);
            let count = self.caret.abs_diff(self.anchor);
            self.merge_nibble = false;
            self.apply(start, count, Vec::new(), start, 0, false);
        }
    }

    pub fn delete_forward(&mut self) {
        self.merge_nibble = false;
        if self.has_selection() {
            self.delete_selection();
        } else if self.caret < self.data.len() {
            self.apply(self.caret, 1, Vec::new(), self.caret, 0, false);
        }
    }

    pub fn backspace(&mut self) {
        self.merge_nibble = false;
        if self.has_selection() {
            self.delete_selection();
        } else if self.caret > 0 {
            self.apply(self.caret - 1, 1, Vec::new(), self.caret - 1, 0, false);
        }
    }

    // ---------- Clipboard ----------

    pub fn selection_text(&self) -> String {
        let start = self.caret.min(self.anchor);
        let end = self.caret.max(self.anchor);
        let bytes = self.data.read(start, end - start);
        if self.ascii_pane && self.ebcdic {
            bytes.iter().map(|b| if *b == 0 { ' ' } else { ebcdic::char_of(*b) }).collect()
        } else if self.ascii_pane {
            bytes.iter().map(|b| if *b == 0 { ' ' } else { *b as char }).collect()
        } else {
            bytes.iter().map(|b| format!("{b:02X}")).collect::<Vec<_>>().join(" ")
        }
    }

    /// The selected rows as they look on screen: offset, hex digits and characters, with gaps for bytes not selected.
    pub fn view_text(&self, shown: impl Fn(u8) -> Option<char>) -> String {
        self.view_rows(self.caret.min(self.anchor), self.caret.max(self.anchor), shown).join("\r\n")
    }

    /// Bytes start..end as rows laid out as on screen.
    pub fn view_rows(&self, start: usize, end: usize, shown: impl Fn(u8) -> Option<char>) -> Vec<String> {
        if start >= end {
            return Vec::new();
        }
        set_offset_width(self.len());
        let mut rows = Vec::new();
        let first_row = start / bytes_per_row() * bytes_per_row();
        let data = self.data.read(first_row, end - first_row);
        for row in start / bytes_per_row()..=(end - 1) / bytes_per_row() {
            let first = row * bytes_per_row();
            let mut cells = vec![' '; ascii_cell(bytes_per_row())];
            for (place, digit) in offset_text(first).chars().enumerate() {
                cells[place] = digit;
            }
            cells[hex_cell(bytes_per_row())] = '|';
            for i in 0..bytes_per_row() {
                let position = first + i;
                if position < start || position >= end {
                    continue;
                }
                let value = data[position - first_row];
                for (place, digit) in format!("{value:02X}").chars().enumerate() {
                    cells[hex_cell(i) + place] = digit;
                }
                cells[ascii_cell(i)] = shown(value).unwrap_or('.');
            }
            rows.push(cells.into_iter().collect::<String>().trim_end().to_string());
        }
        rows
    }

    /// In the hex pane, text of hex digits pastes as those bytes; otherwise as its characters.
    /// Text the pane can't hold changes nothing, rather than going in with parts missing.
    pub fn paste_text(&mut self, text: &str) {
        let parsed = if self.ascii_pane { None } else { parse_hex(text) };
        let Some(bytes) = parsed.or_else(|| self.text_bytes(text)) else {
            self.notice = Some("Nothing was added, because that text has characters EBCDIC doesn't have.".to_string());
            return;
        };
        if bytes.is_empty() {
            return;
        }
        let start = self.caret.min(self.anchor);
        let remove = self.caret.abs_diff(self.anchor);
        let caret = start + bytes.len();
        self.merge_nibble = false;
        self.apply(start, remove, bytes, caret, 0, false);
    }

    // ---------- Search ----------

    /// How two stretches of bytes are compared: exactly, or ignoring case as the character pane reads them.
    fn compare(&self, exact: bool) -> Compare {
        match (exact, self.ebcdic) {
            (true, _) => Compare::Exact,
            (false, true) => Compare::EbcdicCase,
            (false, false) => Compare::AsciiCase,
        }
    }

    fn matches_at(&self, pattern: &[u8], position: usize, exact: bool) -> bool {
        let window = self.data.read(position, pattern.len());
        window.len() == pattern.len() && self.compare(exact).same(&window, pattern)
    }

    /// Starts a search that goes a stretch at a time, for a file left on disk. None if there's nothing to look for.
    pub fn begin_find(&self, o: &SearchOptions) -> Result<Option<HexSearch>, String> {
        if o.pattern.is_empty() {
            return Ok(None);
        }
        let pattern = self.pattern_bytes(&o.pattern, o.hex_bytes)?;
        if pattern.is_empty() || pattern.len() > self.data.len() {
            return Ok(Some(HexSearch { pattern, compare: Compare::Exact, backward: o.backward, parts: Vec::new(), total: 1, covered: 0 }));
        }
        let total = self.data.len() - pattern.len() + 1;
        let sel_start = self.caret.min(self.anchor);
        let sel_end = self.caret.max(self.anchor);
        // Searching starts just past the selection, and wraps round to the other end of the file.
        let first = if o.backward {
            sel_start.checked_sub(1).map_or(total - 1, |s| s.min(total - 1))
        } else if sel_end >= total {
            0
        } else {
            sel_end
        };
        let last = total - 1;
        let mut parts = if o.backward { vec![(0, first), (first + 1, last)] } else { vec![(first, last), (0, first.wrapping_sub(1))] };
        parts.retain(|(lo, hi)| lo <= hi && *hi != usize::MAX);
        parts.reverse();
        Ok(Some(HexSearch { pattern, compare: self.compare(o.match_case || o.hex_bytes), backward: o.backward, parts, total, covered: 0 }))
    }

    /// Searches up to `budget` more bytes; selects what it finds.
    pub fn find_step(&mut self, search: &mut HexSearch, budget: usize) -> SearchStep {
        let Some((lo, hi)) = search.parts.pop() else {
            return SearchStep::NotFound;
        };
        let size = hi - lo + 1;
        let (from, to) = if size <= budget {
            (lo, hi)
        } else if search.backward {
            (hi + 1 - budget, hi)
        } else {
            (lo, lo + budget - 1)
        };
        if let Some(p) = self.data.find_range(&search.pattern, from, to, search.backward, search.compare) {
            self.anchor = p;
            self.caret = p + search.pattern.len();
            self.nibble = 0;
            self.merge_nibble = false;
            self.reveal = true;
            return SearchStep::Found;
        }
        search.covered += to - from + 1;
        if size > budget {
            search.parts.push(if search.backward { (lo, from - 1) } else { (to + 1, hi) });
        }
        if search.parts.is_empty() { SearchStep::NotFound } else { SearchStep::Running(search.covered as f32 / search.total as f32) }
    }

    /// Starts a Replace All that goes a stretch at a time, for a file left on disk.
    pub fn begin_replace_all(&mut self, o: &SearchOptions) -> Result<Option<ReplaceAll>, String> {
        if o.pattern.is_empty() {
            return Ok(None);
        }
        let pattern = self.pattern_bytes(&o.pattern, o.hex_bytes)?;
        let with = self.replacement_bytes(o)?;
        let compare = self.compare(o.match_case || o.hex_bytes);
        Ok(Some(self.data.begin_replace(&pattern, &with, compare)))
    }

    /// Goes on with a Replace All over up to `budget` more bytes: how far it has got, or None once it's done,
    /// when the changes go in as one step to undo.
    pub fn replace_step(&mut self, job: &mut ReplaceAll, budget: usize) -> Option<f32> {
        if !self.data.replace_step(job, budget) {
            return Some(job.at as f32 / self.data.len().max(1) as f32);
        }
        if job.count > 0 {
            let mut pieces = std::mem::take(&mut job.pieces);
            let new_len: usize = pieces.iter().map(|p| p.len).sum();
            // Many small replacements in a file held in memory would leave a long list of pieces to walk on every
            // keystroke; the result is gathered into one instead.
            if pieces.len() > 4096 && !self.data.is_on_disk() {
                let bytes = self.data.bytes_of(&pieces);
                pieces = self.data.add(&bytes);
            }
            let caret = self.caret.min(new_len);
            self.merge_nibble = false;
            self.apply_pieces(0, self.data.len(), pieces, caret, 0, false);
        }
        None
    }

    pub fn find(&mut self, o: &SearchOptions) -> Result<bool, String> {
        let Some(mut search) = self.begin_find(o)? else {
            return Ok(false);
        };
        loop {
            match self.find_step(&mut search, usize::MAX) {
                SearchStep::Found => return Ok(true),
                SearchStep::NotFound => return Ok(false),
                SearchStep::Running(_) => {}
            }
        }
    }

    pub fn replace_current(&mut self, o: &SearchOptions) -> Result<bool, String> {
        if o.pattern.is_empty() {
            return Ok(false);
        }
        self.replace_selected(o)?;
        self.find(o)
    }

    /// Replaces the selection if it's a match; the next match is left to be found.
    pub fn replace_selected(&mut self, o: &SearchOptions) -> Result<(), String> {
        if o.pattern.is_empty() {
            return Ok(());
        }
        let pattern = self.pattern_bytes(&o.pattern, o.hex_bytes)?;
        let with = self.replacement_bytes(o)?;
        let start = self.caret.min(self.anchor);
        if self.caret.abs_diff(self.anchor) == pattern.len() && self.matches_at(&pattern, start, o.match_case || o.hex_bytes) {
            let caret = start + with.len();
            self.merge_nibble = false;
            self.apply(start, pattern.len(), with, caret, 0, false);
        }
        Ok(())
    }

    pub fn replace_all(&mut self, o: &SearchOptions) -> Result<usize, String> {
        let Some(mut job) = self.begin_replace_all(o)? else {
            return Ok(0);
        };
        while self.replace_step(&mut job, usize::MAX / 2).is_some() {}
        Ok(job.count)
    }

    /// Jumps to a decimal offset, or a hex one written with 0x; returns false if it's neither.
    pub fn go_to(&mut self, place: &str) -> bool {
        match parse_number(place) {
            Some(v) => {
                self.move_caret(v.min(self.data.len()), false);
                true
            }
            None => false,
        }
    }

    // ---------- Caret ----------

    pub fn select_all(&mut self) {
        self.anchor = 0;
        self.caret = self.data.len();
        self.nibble = 0;
        self.merge_nibble = false;
        self.reveal = true;
    }

    pub fn move_caret(&mut self, position: usize, extend: bool) {
        self.caret = position.min(self.data.len());
        if !extend {
            self.anchor = self.caret;
        }
        self.nibble = 0;
        self.merge_nibble = false;
        self.reveal = true;
    }

    pub fn move_by(&mut self, delta: isize, extend: bool) {
        self.move_caret(self.caret.saturating_add_signed(delta), extend);
    }

    pub fn move_left(&mut self, extend: bool) {
        if self.nibble == 1 && !extend {
            self.nibble = 0;
            self.merge_nibble = false;
            self.reveal = true;
        } else {
            self.move_by(-1, extend);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn buffer(bytes: &[u8]) -> HexBuffer {
        let mut b = HexBuffer::default();
        b.load(bytes.to_vec());
        b
    }

    #[test]
    fn overwriting_changes_one_digit_at_a_time() {
        let mut b = buffer(&[0x12, 0x34]);
        b.type_hex_digit(0xA);
        assert_eq!(b.bytes(), [0xA2, 0x34]);
        assert_eq!((b.caret, b.nibble), (0, 1));
        b.type_hex_digit(0xB);
        assert_eq!(b.bytes(), [0xAB, 0x34]);
        assert_eq!((b.caret, b.nibble), (1, 0));
        assert!(b.is_modified());
    }

    #[test]
    fn typing_past_the_end_adds_a_byte() {
        let mut b = buffer(&[]);
        b.type_hex_digit(0x4);
        b.type_hex_digit(0x1);
        b.type_hex_digit(0x4);
        b.type_hex_digit(0x2);
        assert_eq!(b.bytes(), b"AB");
    }

    #[test]
    fn insert_mode_adds_bytes_and_keeps_the_rest() {
        let mut b = buffer(&[0xFF]);
        b.insert_mode = true;
        b.type_hex_digit(0x1);
        b.type_hex_digit(0x2);
        assert_eq!(b.bytes(), [0x12, 0xFF]);
        assert_eq!(b.caret, 1);
    }

    #[test]
    fn both_digits_of_a_byte_undo_as_one_step() {
        let mut b = buffer(&[0x12, 0x34]);
        b.type_hex_digit(0xA);
        b.type_hex_digit(0xB);
        b.type_hex_digit(0xC);
        b.type_hex_digit(0xD);
        assert_eq!(b.bytes(), [0xAB, 0xCD]);
        b.undo();
        assert_eq!(b.bytes(), [0xAB, 0x34]);
        b.undo();
        assert_eq!(b.bytes(), [0x12, 0x34]);
        assert!(!b.is_modified());
        assert_eq!(b.caret, 0);
        b.redo();
        b.redo();
        assert_eq!(b.bytes(), [0xAB, 0xCD]);
    }

    #[test]
    fn insert_then_undo_removes_the_byte() {
        let mut b = buffer(&[0xFF]);
        b.insert_mode = true;
        b.type_hex_digit(0x1);
        b.type_hex_digit(0x2);
        b.undo();
        assert_eq!(b.bytes(), [0xFF]);
    }

    #[test]
    fn the_character_pane_types_bytes() {
        let mut b = buffer(&[0, 0, 0]);
        b.ascii_pane = true;
        b.type_byte(b'A');
        b.insert_mode = true;
        b.type_byte(b'B');
        assert_eq!(b.bytes(), [b'A', b'B', 0, 0]);
    }

    #[test]
    fn delete_and_backspace() {
        let mut b = buffer(&[1, 2, 3, 4]);
        b.move_caret(1, false);
        b.delete_forward();
        assert_eq!(b.bytes(), [1, 3, 4]);
        b.backspace();
        assert_eq!(b.bytes(), [3, 4]);
        assert_eq!(b.caret, 0);
        b.backspace();
        assert_eq!(b.bytes(), [3, 4]);
    }

    #[test]
    fn selection_copies_as_hex_or_characters() {
        let mut b = buffer(b"Hi!");
        b.move_caret(0, false);
        b.move_caret(2, true);
        assert_eq!(b.selection_text(), "48 69");
        b.ascii_pane = true;
        assert_eq!(b.selection_text(), "Hi");
    }

    #[test]
    fn paste_understands_hex_in_the_hex_pane() {
        let mut b = buffer(&[0xAA, 0xBB, 0xCC]);
        b.move_caret(1, false);
        b.move_caret(2, true);
        b.paste_text("01 02 03");
        assert_eq!(b.bytes(), [0xAA, 1, 2, 3, 0xCC]);
        b.undo();
        assert_eq!(b.bytes(), [0xAA, 0xBB, 0xCC]);
        b.ascii_pane = true;
        b.move_caret(0, false);
        b.paste_text("AB");
        assert_eq!(b.bytes(), [b'A', b'B', 0xAA, 0xBB, 0xCC]);
    }

    #[test]
    fn parse_hex_rules() {
        assert_eq!(parse_hex("4A 6f"), Some(vec![0x4A, 0x6F]));
        assert_eq!(parse_hex("4a6f"), Some(vec![0x4A, 0x6F]));
        assert_eq!(parse_hex("4"), None);
        assert_eq!(parse_hex("zz"), None);
        assert_eq!(parse_hex(""), None);
    }

    #[test]
    fn find_forward_backward_and_wrap() {
        let mut b = buffer(b"abc abc");
        let o = SearchOptions { pattern: "ABC".into(), ..Default::default() };
        assert_eq!(b.find(&o), Ok(true));
        assert_eq!((b.anchor, b.caret), (0, 3));
        assert_eq!(b.find(&o), Ok(true));
        assert_eq!((b.anchor, b.caret), (4, 7));
        assert_eq!(b.find(&o), Ok(true));
        assert_eq!(b.anchor, 0);
        let back = SearchOptions { pattern: "abc".into(), backward: true, ..Default::default() };
        assert_eq!(b.find(&back), Ok(true));
        assert_eq!(b.anchor, 4);
        assert_eq!(b.find(&back), Ok(true));
        assert_eq!(b.anchor, 0);
    }

    #[test]
    fn find_hex_bytes_and_bad_hex() {
        let mut b = buffer(&[0, 0xDE, 0xAD, 0]);
        let o = SearchOptions { pattern: "de ad".into(), hex_bytes: true, ..Default::default() };
        assert_eq!(b.find(&o), Ok(true));
        assert_eq!(b.anchor, 1);
        let bad = SearchOptions { pattern: "xyz".into(), hex_bytes: true, ..Default::default() };
        assert!(b.find(&bad).is_err());
    }

    #[test]
    fn find_with_a_pattern_longer_than_the_file() {
        let mut b = buffer(b"ab");
        let o = SearchOptions { pattern: "abc".into(), ..Default::default() };
        assert_eq!(b.find(&o), Ok(false));
    }

    #[test]
    fn replace_all_can_change_the_length_and_undoes_in_one_step() {
        let mut b = buffer(&[1, 2, 1, 2, 3]);
        let o = SearchOptions { pattern: "01 02".into(), replacement: "09".into(), hex_bytes: true, ..Default::default() };
        assert_eq!(b.replace_all(&o), Ok(2));
        assert_eq!(b.bytes(), [9, 9, 3]);
        b.undo();
        assert_eq!(b.bytes(), [1, 2, 1, 2, 3]);
    }

    #[test]
    fn replace_one_at_a_time() {
        let mut b = buffer(b"aXaX");
        let o = SearchOptions { pattern: "X".into(), replacement: "YY".into(), match_case: true, ..Default::default() };
        assert_eq!(b.find(&o), Ok(true));
        assert_eq!(b.replace_current(&o), Ok(true));
        assert_eq!(b.bytes(), b"aYYaX");
    }

    #[test]
    fn insert_run_adds_bytes_even_when_typing_overwrites_and_undoes_in_one_step() {
        let mut b = buffer(&[1, 2, 3]);
        assert!(!b.insert_mode);
        b.move_caret(1, false);
        b.insert_run(3, 0xAA);
        assert_eq!(b.bytes(), [1, 0xAA, 0xAA, 0xAA, 2, 3]);
        assert_eq!((b.caret, b.anchor), (1, 4), "what's new is selected");
        assert!(b.is_modified());
        b.undo();
        assert_eq!(b.bytes(), [1, 2, 3]);
        assert!(!b.is_modified());
        b.redo();
        assert_eq!(b.bytes(), [1, 0xAA, 0xAA, 0xAA, 2, 3]);
    }

    #[test]
    fn insert_run_goes_in_front_of_a_selection_without_replacing_it() {
        let mut b = buffer(&[1, 2, 3]);
        b.move_caret(2, false);
        b.move_caret(1, true);
        b.insert_run(1, 9);
        assert_eq!(b.bytes(), [1, 9, 2, 3]);
    }

    #[test]
    fn insert_run_at_the_end_and_in_an_empty_file() {
        let mut b = buffer(&[]);
        b.insert_run(2, 0x20);
        assert_eq!(b.bytes(), [0x20, 0x20]);
        b.move_caret(2, false);
        b.insert_run(1, 0);
        assert_eq!(b.bytes(), [0x20, 0x20, 0]);
    }

    #[test]
    fn delete_run_stops_at_the_end_of_the_file_and_undoes() {
        let mut b = buffer(&[1, 2, 3, 4]);
        b.move_caret(1, false);
        assert_eq!(b.delete_run(2), 2);
        assert_eq!(b.bytes(), [1, 4]);
        b.undo();
        assert_eq!(b.bytes(), [1, 2, 3, 4]);
        b.move_caret(3, false);
        assert_eq!(b.delete_run(1000), 1);
        assert_eq!(b.bytes(), [1, 2, 3]);
        b.move_caret(3, false);
        assert_eq!(b.delete_run(1), 0);
        assert_eq!(b.bytes(), [1, 2, 3]);
    }

    #[test]
    fn delete_run_starts_at_the_selection_whichever_way_it_was_made() {
        let mut b = buffer(&[1, 2, 3, 4]);
        b.move_caret(3, false);
        b.move_caret(1, true);
        assert_eq!(b.delete_run(2), 2);
        assert_eq!(b.bytes(), [1, 4]);
    }

    #[test]
    fn what_was_typed_into_insert_delete_is_checked() {
        assert_eq!(parse_run("16", false, "20"), Ok((16, 0x20)));
        assert_eq!(parse_run("0x10", false, "ff"), Ok((16, 0xFF)));
        assert_eq!(parse_run(" 5 ", true, "not looked at"), Ok((5, 0)));
        assert!(parse_run("0", false, "00").is_err());
        assert!(parse_run("many", true, "").is_err());
        assert!(parse_run("-1", true, "").is_err());
        assert!(parse_run("1", false, "").is_err());
        assert!(parse_run("1", false, "0").is_err());
        assert!(parse_run("1", false, "00 00").is_err());
        assert!(parse_run(&(MOST_INSERTED + 1).to_string(), false, "00").is_err());
        assert_eq!(parse_run(&(usize::MAX).to_string(), true, ""), Ok((usize::MAX, 0)), "a delete just stops at the end");
    }

    #[test]
    fn copy_selected_lays_out_rows_as_the_screen_does() {
        let mut b = buffer(b"Hello");
        b.move_caret(1, false);
        b.move_caret(4, true);
        let text = b.view_text(|value| Some(value as char));
        assert_eq!(text, format!("{:<59}|  ell", "00000000     65 6C 6C"));
    }

    #[test]
    fn copy_selected_covers_every_row_and_shows_dots_for_what_cant_be_shown() {
        let mut bytes = vec![b'a'; 16];
        bytes.extend_from_slice(&[0, b'b']);
        let mut b = buffer(&bytes);
        b.select_all();
        let text = b.view_text(|value| (value != 0).then_some(value as char));
        let rows: Vec<&str> = text.split("\r\n").collect();
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0], format!("{:<59}| {}", format!("00000000  {}  {}", ["61"; 8].join(" "), ["61"; 8].join(" ")), "a".repeat(16)));
        assert_eq!(rows[1], format!("{:<59}| .b", "00000010  00 62"));
    }

    #[test]
    fn copy_selected_ending_on_a_row_boundary_adds_no_empty_row() {
        let mut b = buffer(&[0x41; 32]);
        b.move_caret(0, false);
        b.move_caret(16, true);
        assert_eq!(b.view_text(|_| None).split("\r\n").count(), 1);
        b.move_caret(16, false);
        assert_eq!(b.view_text(|_| None), "");
    }

    fn ebcdic_buffer(bytes: &[u8]) -> HexBuffer {
        let mut b = buffer(bytes);
        b.ebcdic = true;
        b.ascii_pane = true;
        b
    }

    #[test]
    fn typing_in_the_ebcdic_pane_writes_ebcdic() {
        let mut b = ebcdic_buffer(&[0, 0]);
        b.type_char('A');
        b.type_char('1');
        assert_eq!(b.bytes(), [0xC1, 0xF1]);
    }

    #[test]
    fn typing_in_the_plain_pane_is_unchanged() {
        let mut b = buffer(&[0, 0]);
        b.ascii_pane = true;
        b.type_char('A');
        assert_eq!(b.bytes(), [0x41, 0]);
        b.type_char('\u{e9}');
        assert_eq!(b.bytes(), [0x41, 0xC3, 0xA9, 0], "as UTF-8, and put in rather than over, as before");
    }

    #[test]
    fn pasting_and_copying_in_the_ebcdic_pane_convert_both_ways() {
        let mut b = ebcdic_buffer(&[]);
        b.paste_text("Hi!");
        assert_eq!(b.bytes(), [0xC8, 0x89, 0x5A]);
        b.select_all();
        assert_eq!(b.selection_text(), "Hi!");
    }

    #[test]
    fn text_ebcdic_cant_hold_changes_nothing_and_says_so() {
        let mut b = ebcdic_buffer(&[0xC1]);
        b.paste_text("A\u{20AC}");
        assert_eq!(b.bytes(), [0xC1]);
        assert!(!b.is_modified());
        assert!(b.notice.take().is_some());
        b.type_char('\u{20AC}');
        assert_eq!(b.bytes(), [0xC1]);
        assert!(b.notice.take().is_some());
    }

    #[test]
    fn hex_digits_pasted_in_the_hex_pane_are_still_those_bytes() {
        let mut b = ebcdic_buffer(&[]);
        b.ascii_pane = false;
        b.paste_text("C1 C2");
        assert_eq!(b.bytes(), [0xC1, 0xC2]);
    }

    #[test]
    fn find_and_replace_read_text_as_ebcdic_and_ignore_case_properly() {
        // "Hello" in EBCDIC. Its bytes are not "Hello" in ASCII, so a plain search would miss it.
        let hello = [0xC8, 0x85, 0x93, 0x93, 0x96];
        let mut b = ebcdic_buffer(&hello);
        let exact = SearchOptions { pattern: "Hello".into(), match_case: true, ..Default::default() };
        assert_eq!(b.find(&exact), Ok(true));
        assert_eq!((b.anchor, b.caret), (0, 5));
        b.move_caret(0, false);
        let any_case = SearchOptions { pattern: "hELLO".into(), ..Default::default() };
        assert_eq!(b.find(&any_case), Ok(true));
        let wrong_case = SearchOptions { pattern: "hELLO".into(), match_case: true, ..Default::default() };
        assert_eq!(b.find(&wrong_case), Ok(false));
        let swap = SearchOptions { pattern: "ell".into(), replacement: "ip".into(), ..Default::default() };
        assert_eq!(b.replace_all(&swap), Ok(1));
        assert_eq!(b.bytes(), [0xC8, 0x89, 0x97, 0x96]);
        let outside = SearchOptions { pattern: "\u{20AC}".into(), ..Default::default() };
        assert!(b.find(&outside).is_err());
    }

    #[test]
    fn plain_case_folding_is_not_confused_by_ebcdic_bytes() {
        // In ASCII these two differ by the case bit; in EBCDIC they are different letters.
        let mut b = ebcdic_buffer(&[0x41]);
        assert!(!b.matches_at(&[0x61], 0, false));
        b.ebcdic = false;
        assert!(b.matches_at(&[0x61], 0, false));
    }

    #[test]
    fn offsets_past_4_gb_widen_the_column() {
        set_offset_width(0x1_0000_0000);
        assert_eq!(offset_text(0x10), "000000010");
        assert_eq!(hex_cell(0), 11);
        set_offset_width(100);
        assert_eq!(offset_text(0x10), "00000010");
        assert_eq!(hex_cell(0), 10);
    }

    #[test]
    fn go_to_takes_decimal_and_hex_and_stops_at_the_end() {
        let mut b = buffer(&[0; 32]);
        assert!(b.go_to("16"));
        assert_eq!(b.caret, 16);
        assert!(b.go_to("0x1F"));
        assert_eq!(b.caret, 31);
        assert!(b.go_to("9999"));
        assert_eq!(b.caret, 32);
        assert!(!b.go_to("nope"));
    }
}
