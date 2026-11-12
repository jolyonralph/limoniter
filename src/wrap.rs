//! Word wrap: how a line's cells fall onto rows of the screen.
//! This is only a way of drawing. It never touches the text, so no line ending is ever added,
//! and it only looks at the lines on screen, so it costs the same however large the file is.

use crate::text_buffer::next_visual;

/// One row of the screen under word wrap: cells start..end of a line.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Slot {
    pub line: usize,
    pub sub: usize,
    pub start: usize,
    pub end: usize,
    /// The line's last row, where a line ending and anything past the text belong.
    pub last: bool,
}

impl Slot {
    /// Whether a cursor at this column is drawn on this row. A column on a break belongs to the next row.
    pub fn holds(&self, col: usize) -> bool {
        col >= self.start && (col < self.end || self.last)
    }
}

/// The column each of a line's rows starts at. Breaks after a space where it can, and wherever
/// it must if a word is wider than the row. A token is never split.
pub fn row_starts(line: &str, width: usize) -> Vec<usize> {
    let width = width.max(1);
    let mut starts = vec![0];
    let mut row_start = 0;
    let mut after_space = 0;
    let mut column = 0;
    for c in line.chars() {
        let next = next_visual(column, c);
        if next > row_start + width && column > row_start {
            row_start = if after_space > row_start { after_space } else { column };
            starts.push(row_start);
        }
        if c == ' ' || c == '\t' {
            after_space = next;
        }
        column = next;
    }
    starts
}

/// The screen's rows, from a given row of a given line, until there are enough or the lines run out.
pub fn slots(line_at: &dyn Fn(usize) -> String, total: usize, top: usize, top_sub: usize, rows: usize, width: usize) -> Vec<Slot> {
    let mut out = Vec::with_capacity(rows + 1);
    let mut line = top;
    let mut skip = top_sub;
    while line < total && out.len() < rows {
        let text = line_at(line);
        let starts = row_starts(&text, width);
        let visual = text.chars().fold(0, next_visual);
        let first = skip.min(starts.len() - 1);
        for sub in first..starts.len() {
            if out.len() >= rows {
                break;
            }
            let last = sub + 1 == starts.len();
            out.push(Slot { line, sub, start: starts[sub], end: if last { visual } else { starts[sub + 1] }, last });
        }
        skip = 0;
        line += 1;
    }
    out
}

/// Moves a row position by a number of rows, stopping at either end of the document.
pub fn step(line_at: &dyn Fn(usize) -> String, total: usize, width: usize, mut line: usize, mut sub: usize, delta: isize) -> (usize, usize) {
    let last_line = total.saturating_sub(1);
    line = line.min(last_line);
    sub = sub.min(row_starts(&line_at(line), width).len() - 1);
    if delta >= 0 {
        for _ in 0..delta {
            let count = row_starts(&line_at(line), width).len();
            if sub + 1 < count {
                sub += 1;
            } else if line < last_line {
                line += 1;
                sub = 0;
            } else {
                break;
            }
        }
    } else {
        for _ in 0..delta.unsigned_abs() {
            if sub > 0 {
                sub -= 1;
            } else if line > 0 {
                line -= 1;
                sub = row_starts(&line_at(line), width).len() - 1;
            } else {
                break;
            }
        }
    }
    (line, sub)
}

/// Which row of a line a column is on.
pub fn sub_of(line: &str, width: usize, col: usize) -> usize {
    row_starts(line, width).partition_point(|start| *start <= col).saturating_sub(1)
}

/// The start of the last character on a row that isn't the line's last: where End puts the cursor,
/// so that it stays on the row instead of appearing at the start of the next.
pub fn row_end_col(line: &str, end: usize) -> usize {
    let mut column = 0;
    let mut before = 0;
    for c in line.chars() {
        let next = next_visual(column, c);
        if next >= end {
            return column;
        }
        before = next;
        column = next;
    }
    before
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lines(text: &[&str]) -> Vec<String> {
        text.iter().map(|line| (*line).to_string()).collect()
    }

    #[test]
    fn lines_break_after_spaces_and_only_split_words_too_wide_for_a_row() {
        assert_eq!(row_starts("short", 10), [0]);
        assert_eq!(row_starts("", 10), [0]);
        assert_eq!(row_starts("one two three four", 8), [0, 8, 14]);
        assert_eq!(row_starts("abcdefghijklmnopqrst", 8), [0, 8, 16]);
        assert_eq!(row_starts("ab cdefghijklmn", 8), [0, 3, 11]);
        assert_eq!(row_starts("exactly8", 8), [0], "a line that just fits stays on one row");
    }

    #[test]
    fn every_cell_is_on_exactly_one_row_and_nothing_is_lost() {
        let text = "some words\twith a tab and averyveryveryverylongwordindeed plus more";
        for width in 1..40 {
            let starts = row_starts(text, width);
            assert_eq!(starts[0], 0);
            assert!(starts.windows(2).all(|pair| pair[0] < pair[1]), "width {width}: {starts:?}");
        }
    }

    #[test]
    fn a_wide_token_goes_whole_onto_a_row() {
        crate::text_buffer::set_source_view(true);
        crate::text_buffer::set_cell_profile(crate::charset::known("amiga-text"));
        let line = format!("abc{}d", crate::charset::raw_char(0x9B));
        assert_eq!(row_starts(&line, 5), [0, 3, 8], "[CSI] takes 5 cells and stays together");
        crate::text_buffer::set_cell_profile(crate::charset::utf8());
    }

    #[test]
    fn the_screen_is_filled_from_a_row_partway_through_a_line() {
        let text = lines(&["one two three four", "x", "y"]);
        let at = |i: usize| text[i].clone();
        let all = slots(&at, 3, 0, 0, 10, 8);
        assert_eq!(all.len(), 5);
        assert_eq!(all[0], Slot { line: 0, sub: 0, start: 0, end: 8, last: false });
        assert_eq!(all[2], Slot { line: 0, sub: 2, start: 14, end: 18, last: true });
        assert_eq!(all[3].line, 1);
        let later = slots(&at, 3, 0, 1, 2, 8);
        assert_eq!((later[0].sub, later[1].sub), (1, 2));
        assert_eq!(later.len(), 2);
        let past = slots(&at, 3, 1, 99, 5, 8);
        assert_eq!(past[0].line, 1, "a row number that's gone out of date is corrected");
    }

    #[test]
    fn stepping_goes_through_rows_and_stops_at_the_ends() {
        let text = lines(&["one two three four", "x"]);
        let at = |i: usize| text[i].clone();
        assert_eq!(step(&at, 2, 8, 0, 0, 2), (0, 2));
        assert_eq!(step(&at, 2, 8, 0, 2, 1), (1, 0));
        assert_eq!(step(&at, 2, 8, 1, 0, 50), (1, 0));
        assert_eq!(step(&at, 2, 8, 1, 0, -1), (0, 2));
        assert_eq!(step(&at, 2, 8, 0, 1, -50), (0, 0));
    }

    #[test]
    fn a_column_on_a_break_belongs_to_the_next_row() {
        assert_eq!(sub_of("one two three four", 8, 7), 0);
        assert_eq!(sub_of("one two three four", 8, 8), 1);
        assert_eq!(sub_of("one two three four", 8, 18), 2);
        let slot = Slot { line: 0, sub: 0, start: 0, end: 8, last: false };
        assert!(slot.holds(7) && !slot.holds(8));
        let last = Slot { line: 0, sub: 2, start: 14, end: 18, last: true };
        assert!(last.holds(18) && last.holds(40));
    }

    #[test]
    fn end_stays_on_the_row() {
        assert_eq!(row_end_col("one two three four", 8), 7);
        assert_eq!(row_end_col("abcdefgh", 8), 7);
    }
}
