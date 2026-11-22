//! Each column of a delimited file in a colour of its own in the text view, as the Rainbow CSV extension does.
//! Only the lines being drawn are coloured. A quoted field that runs over several lines needs to know where the
//! line before it left off, so how the lines at every 256th line start is remembered, and a view starts from the
//! nearest one.

use crate::table::{self, Carry};
use crate::text_buffer::TextBuffer;
use eframe::egui::Color32;
use std::collections::HashMap;

/// How far apart the remembered places are, in lines.
const STEP: usize = 256;
/// A line longer than this (in bytes) is left plain: it's no use as a table, and colouring it would be slow.
const MOST_BYTES: usize = 20_000;
/// How many colours there are before they cycle.
pub const COLOURS: usize = 8;

/// The colours, from the theme's rainbow ones with two blends so that eight columns in a row look different.
pub fn palette() -> [Color32; COLOURS] {
    let r = crate::theme::rainbow();
    [r[0], r[1], r[2], r[3], r[4], r[5], r[1].lerp_to_gamma(r[4], 0.5), r[2].lerp_to_gamma(r[5], 0.5)]
}

/// How a document's lines start, worked out as far as it has been asked.
#[derive(Default)]
pub struct Cache {
    version: u64,
    delimiter: Option<char>,
    /// How line `n * STEP` starts.
    marks: Vec<Carry>,
}

impl Cache {
    /// How `line` starts, reading on from the furthest place known.
    fn carry_at(&mut self, buffer: &TextBuffer, delimiter: char, line: usize) -> (usize, Carry) {
        if self.version != buffer.version() || self.delimiter != Some(delimiter) {
            *self = Cache { version: buffer.version(), delimiter: Some(delimiter), marks: vec![Carry::default()] };
        }
        let wanted = (line / STEP).min(buffer.line_count().saturating_sub(1) / STEP);
        while self.marks.len() <= wanted {
            let start = (self.marks.len() - 1) * STEP;
            let mut carry = self.marks[self.marks.len() - 1];
            for i in start..start + STEP {
                carry = table::column_runs(&buffer.line(i), delimiter, carry).1;
            }
            self.marks.push(carry);
        }
        (wanted * STEP, self.marks[wanted])
    }

    /// The colour runs (characters, colour) of each of the lines, which are in order. Lines without any are left out.
    pub fn runs_for(&mut self, buffer: &TextBuffer, delimiter: char, lines: impl IntoIterator<Item = usize>) -> HashMap<usize, Vec<(usize, Color32)>> {
        let colours = palette();
        let mut out = HashMap::new();
        let mut at: Option<(usize, Carry)> = None;
        for line in lines {
            if line >= buffer.line_count() {
                continue;
            }
            // Carry on from the last line done when it's behind this one, or start again from a remembered place.
            let (mut here, mut carry) = match at {
                Some((done, carry)) if done <= line => (done, carry),
                _ => self.carry_at(buffer, delimiter, line),
            };
            while here < line {
                carry = table::column_runs(&buffer.line(here), delimiter, carry).1;
                here += 1;
            }
            let text = buffer.line(line);
            let (runs, next) = table::column_runs(&text, delimiter, carry);
            at = Some((line + 1, next));
            if text.len() <= MOST_BYTES {
                out.insert(line, runs.into_iter().map(|(length, column)| (length, colours[column % COLOURS])).collect());
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn buffer(text: &str) -> TextBuffer {
        let mut b = TextBuffer::default();
        b.set_content(text.lines().map(str::to_string).collect());
        b
    }

    #[test]
    fn the_eight_colours_differ() {
        let colours = palette();
        for (i, a) in colours.iter().enumerate() {
            assert!(!colours[..i].contains(a), "colour {i} repeats an earlier one");
        }
    }

    #[test]
    fn columns_cycle_through_the_colours() {
        let b = buffer("a,b,c,d,e,f,g,h,i,j\n");
        let runs = &Cache::default().runs_for(&b, ',', [0])[&0];
        let colours = palette();
        assert_eq!(runs.len(), 10);
        assert_eq!(runs[0], (2, colours[0]), "the delimiter belongs to the column before it");
        assert_eq!(runs[8].1, colours[0], "the ninth column starts the colours again");
        assert_eq!(runs[9], (1, colours[1]));
    }

    #[test]
    fn a_quoted_field_with_the_delimiter_is_one_column() {
        let b = buffer("x,\"a,b\",y\n");
        let colours = palette();
        let runs = &Cache::default().runs_for(&b, ',', [0])[&0];
        assert_eq!(runs, &vec![(2, colours[0]), (6, colours[1]), (1, colours[2])]);
    }

    #[test]
    fn a_quoted_field_over_several_lines_keeps_its_colour_and_column() {
        let b = buffer("a,\"one\ntwo, still\nthree\",c\nd,e\n");
        let colours = palette();
        let runs = Cache::default().runs_for(&b, ',', [1, 2, 3]);
        assert_eq!(runs[&1], vec![(10, colours[1])], "the second line is all inside the second column");
        assert_eq!(runs[&2], vec![(7, colours[1]), (1, colours[2])]);
        assert_eq!(runs[&3], vec![(2, colours[0]), (1, colours[1])], "the next record starts again");
    }

    #[test]
    fn a_line_far_down_starts_from_the_right_place() {
        let mut lines: Vec<String> = (0..1000).map(|i| format!("{i},x")).collect();
        // A quote opens on line 600 and shuts on 700.
        lines[600] = "1,\"open".into();
        lines[700] = "close\",2".into();
        let b = buffer(&lines.join("\n"));
        let colours = palette();
        let mut cache = Cache::default();
        let runs = cache.runs_for(&b, ',', [650, 700, 701]);
        assert_eq!(runs[&650], vec![(lines[650].chars().count(), colours[1])]);
        assert_eq!(runs[&700], vec![(7, colours[1]), (1, colours[2])]);
        assert_eq!(runs[&701][0].1, colours[0]);
        // Going back up works too.
        assert_eq!(cache.runs_for(&b, ',', [10])[&10][0].1, colours[0]);
    }

    #[test]
    fn an_edit_makes_the_places_be_read_again() {
        let mut b = buffer("a,b\nc,d");
        let mut cache = Cache::default();
        let colours = palette();
        assert_eq!(cache.runs_for(&b, ',', [1])[&1][0].1, colours[0]);
        b.set_content(vec!["\"a".into(), "b\",c".into(), "d,e".into()]);
        assert_eq!(cache.runs_for(&b, ',', [1])[&1], vec![(3, colours[0]), (1, colours[1])]);
    }

    #[test]
    fn a_stray_quote_doesnt_colour_the_rest_of_the_file() {
        let mut text = String::from("a,\"never shut\n");
        for _ in 0..3000 {
            text.push_str("p,q\n");
        }
        let b = buffer(&text);
        let runs = Cache::default().runs_for(&b, ',', [2500]);
        assert_eq!(runs[&2500].len(), 2, "two columns again");
    }

    #[test]
    fn a_huge_line_is_left_plain() {
        let b = buffer(&"a,".repeat(20_000));
        assert!(Cache::default().runs_for(&b, ',', [0]).is_empty());
    }
}
