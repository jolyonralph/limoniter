//! Changing how text sits on its lines: reindenting, wrapping and paragraphs. Nothing here touches a document;
//! each takes lines and gives back lines.

use crate::text_buffer::{next_visual, visual_len};
use std::sync::Arc;

pub const LEAST_WIDTH: usize = 10;
pub const MOST_WIDTH: usize = 10_000;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Align {
    Left,
    Center,
    Right,
    Justify,
}

impl Align {
    pub fn name(self) -> &'static str {
        match self {
            Align::Left => "left",
            Align::Center => "center",
            Align::Right => "right",
            Align::Justify => "justify",
        }
    }

    pub fn from_name(name: &str) -> Option<Self> {
        [Align::Left, Align::Center, Align::Right, Align::Justify].into_iter().find(|align| align.name() == name)
    }
}

/// How Reformat lays paragraphs out. Remembered in the settings file.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ParagraphSetup {
    /// The column lines end by.
    pub width: usize,
    /// Columns before each line.
    pub left: usize,
    /// Further columns before a paragraph's first line.
    pub first: usize,
    pub align: Align,
}

impl Default for ParagraphSetup {
    fn default() -> Self {
        Self { width: 78, left: 0, first: 0, align: Align::Left }
    }
}

fn leading(line: &str) -> &str {
    &line[..line.len() - line.trim_start_matches([' ', '\t']).len()]
}

fn is_blank(line: &str) -> bool {
    line.trim().is_empty()
}

/// The bullet or number a list item starts with, with its space, if it is one.
fn list_marker(line: &str) -> Option<&str> {
    let text = line.trim_start_matches([' ', '\t']);
    if text.starts_with("- ") || text.starts_with("* ") || text.starts_with("+ ") {
        return Some(&text[..2]);
    }
    let digits = text.chars().take_while(char::is_ascii_digit).count();
    let rest = &text[digits..];
    (digits > 0 && (rest.starts_with(". ") || rest.starts_with(") "))).then(|| &text[..digits + 2])
}

// ---------- Wrap to line ends, and back ----------

fn wrap_one(line: &str, width: usize) -> Vec<String> {
    if visual_len(line) <= width || is_blank(line) {
        return vec![line.to_string()];
    }
    let indent = leading(line);
    let start = visual_len(indent);
    let room = width.max(start + 1);
    let mut out = Vec::new();
    let mut rest = &line[indent.len()..];
    loop {
        let (mut col, mut was_space, mut cut, mut over) = (start, true, None, false);
        for (byte, c) in rest.char_indices() {
            let next = next_visual(col, c);
            let space = c == ' ' || c == '\t';
            if space && !was_space {
                cut = Some(byte);
                if col > room {
                    over = true;
                    break;
                }
            }
            if next > room && cut.is_some() {
                over = true;
                break;
            }
            was_space = space;
            col = next;
        }
        let Some(cut) = cut.filter(|_| over) else {
            out.push(format!("{indent}{}", rest.trim_end_matches([' ', '\t'])));
            return out;
        };
        out.push(format!("{indent}{}", &rest[..cut]));
        rest = rest[cut..].trim_start_matches([' ', '\t']);
        if rest.is_empty() {
            return out;
        }
    }
}

/// Breaks long lines at spaces, so they end by a column. Continuing lines keep the indent. A word too long
/// to fit stays whole, and shorter lines aren't touched.
pub fn wrap_to_line_ends<S: AsRef<str>>(lines: &[S], width: usize) -> Vec<String> {
    lines.iter().flat_map(|line| wrap_one(line.as_ref(), width)).collect()
}

/// Joins the lines of each paragraph into one long line. Blank lines and list items still separate them.
pub fn line_ends_to_wrap<S: AsRef<str>>(lines: &[S]) -> Vec<String> {
    let mut out: Vec<String> = Vec::with_capacity(lines.len());
    let mut joining = false;
    for line in lines {
        let line = line.as_ref();
        if is_blank(line) {
            out.push(line.to_string());
            joining = false;
        } else if joining && list_marker(line).is_none() {
            let last = out.last_mut().expect("joining means there's a line");
            let kept = last.trim_end_matches([' ', '\t']).len();
            last.truncate(kept);
            last.push(' ');
            last.push_str(line.trim_matches([' ', '\t']));
        } else {
            out.push(line.trim_end_matches([' ', '\t']).to_string());
            joining = true;
        }
    }
    out
}

// ---------- Paragraphs ----------

fn justified(words: &[&str], lead: usize, width: usize) -> String {
    let length: usize = words.iter().map(|word| visual_len(word)).sum();
    let gaps = words.len().saturating_sub(1);
    if gaps == 0 || lead + length + gaps > width {
        return format!("{}{}", " ".repeat(lead), words.join(" "));
    }
    let spaces = width - lead - length;
    let (each, extra) = (spaces / gaps, spaces % gaps);
    let mut out = " ".repeat(lead);
    for (place, word) in words.iter().enumerate() {
        out.push_str(word);
        if place < gaps {
            out.extend(std::iter::repeat_n(' ', each + usize::from(place < extra)));
        }
    }
    out
}

fn placed(text: &str, lead: usize, width: usize, align: Align) -> String {
    let length = visual_len(text);
    let room = width.saturating_sub(length);
    let before = match align {
        Align::Left | Align::Justify => lead,
        Align::Center => (lead + room.saturating_sub(lead) / 2).min(room.max(lead)),
        Align::Right => room.max(lead),
    };
    format!("{}{text}", " ".repeat(before))
}

/// Lines up each line within the width. Justify leaves a paragraph's last line alone, as it usually is.
pub fn align_lines<S: AsRef<str>>(lines: &[S], align: Align, width: usize, left: usize) -> Vec<String> {
    let mut out = Vec::with_capacity(lines.len());
    for (place, line) in lines.iter().enumerate() {
        let line = line.as_ref();
        if is_blank(line) {
            out.push(line.to_string());
            continue;
        }
        let text = line.trim_matches([' ', '\t']);
        let last_of_paragraph = lines.get(place + 1).is_none_or(|next| is_blank(next.as_ref()));
        if align == Align::Justify && !last_of_paragraph {
            out.push(justified(&text.split_whitespace().collect::<Vec<_>>(), left, width));
        } else {
            out.push(placed(text, left, width, align));
        }
    }
    out
}

/// Lays one paragraph's words out to fit.
fn fill(words: &[&str], first_lead: usize, lead: usize, width: usize, align: Align, prefix: &str) -> Vec<String> {
    let mut rows: Vec<(usize, Vec<&str>)> = Vec::new();
    let mut row: Vec<&str> = Vec::new();
    let mut col = first_lead;
    let mut row_lead = first_lead;
    for word in words {
        let size = visual_len(word);
        if !row.is_empty() && col + 1 + size > width {
            rows.push((row_lead, std::mem::take(&mut row)));
            row_lead = lead;
            col = lead;
        }
        col += size + usize::from(!row.is_empty());
        row.push(word);
    }
    rows.push((row_lead, row));
    let last = rows.len() - 1;
    rows.iter()
        .enumerate()
        .map(|(place, (row_lead, row))| {
            let line = if align == Align::Justify && place < last { justified(row, *row_lead, width) } else { placed(&row.join(" "), *row_lead, width, align) };
            if place == 0 && !prefix.is_empty() {
                // A list item's marker goes in the spaces left for it.
                let spaces = line.chars().take_while(|c| *c == ' ').count();
                let start = spaces.saturating_sub(prefix.chars().count());
                format!("{}{prefix}{}", " ".repeat(start), &line[spaces..])
            } else {
                line
            }
        })
        .collect()
}

/// Rewraps paragraphs to the setup. Paragraphs are separated by blank lines, and each list item is one of its own,
/// keeping its marker and its indent, with the lines after it hanging under its text.
pub fn reformat_paragraphs<S: AsRef<str>>(lines: &[S], setup: &ParagraphSetup) -> Vec<String> {
    let width = setup.width.clamp(LEAST_WIDTH, MOST_WIDTH);
    let mut out = Vec::with_capacity(lines.len());
    let mut place = 0;
    while place < lines.len() {
        let line = lines[place].as_ref();
        if is_blank(line) {
            out.push(line.to_string());
            place += 1;
            continue;
        }
        let marker = list_marker(line);
        let mut words: Vec<&str> = Vec::new();
        let mut next = place;
        while next < lines.len() {
            let row = lines[next].as_ref();
            if is_blank(row) || (next > place && list_marker(row).is_some()) {
                break;
            }
            let body = if next == place { marker.map_or(row, |mark| row.trim_start_matches([' ', '\t'])[mark.len()..].trim_start()) } else { row };
            words.extend(body.split_whitespace());
            next += 1;
        }
        if words.is_empty() {
            out.push(line.trim_end_matches([' ', '\t']).to_string());
        } else if let Some(mark) = marker {
            let hang = visual_len(leading(line)) + visual_len(mark);
            out.extend(fill(&words, hang, hang, width, setup.align, mark));
        } else {
            out.extend(fill(&words, setup.left + setup.first, setup.left, width, setup.align, ""));
        }
        place = next;
    }
    out
}

// ---------- Reindenting ----------

/// How a language marks its comments, for finding the brackets that count.
#[derive(Clone, Copy, Debug, Default)]
pub struct Marks<'a> {
    pub line: Option<&'a str>,
    pub block: Option<(&'a str, &'a str)>,
    /// `#` starts a line that's for the preprocessor, which stays where it is.
    pub preprocessor: bool,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Inside {
    Code,
    Block,
    Template,
}

/// Goes through a line's brackets: how many it starts by closing, and how the depth changes.
/// A quoted string ends with its line, unless it's a backtick one, so that an apostrophe in plain text does no harm.
fn scan_line(line: &str, marks: &Marks<'_>, inside: &mut Inside) -> (i64, i64) {
    let (mut leading_close, mut net, mut started) = (0, 0, false);
    let mut rest = line;
    while let Some(c) = rest.chars().next() {
        match *inside {
            Inside::Block => {
                let close = marks.block.map_or("*/", |(_, close)| close);
                let Some(at) = rest.find(close) else {
                    return (leading_close, net);
                };
                rest = &rest[at + close.len()..];
                *inside = Inside::Code;
                continue;
            }
            Inside::Template => {
                let Some(at) = rest.find('`') else {
                    return (leading_close, net);
                };
                let escaped = rest[..at].chars().rev().take_while(|c| *c == '\\').count() % 2 == 1;
                rest = &rest[at + 1..];
                if !escaped {
                    *inside = Inside::Code;
                }
                continue;
            }
            Inside::Code => {}
        }
        if marks.line.is_some_and(|mark| rest.starts_with(mark)) {
            return (leading_close, net);
        }
        if let Some((open, _)) = marks.block
            && rest.starts_with(open)
        {
            rest = &rest[open.len()..];
            *inside = Inside::Block;
            continue;
        }
        if c == '"' || c == '\'' {
            let mut chars = rest.char_indices().skip(1);
            let mut end = None;
            while let Some((at, inner)) = chars.next() {
                if inner == '\\' {
                    chars.next();
                } else if inner == c {
                    end = Some(at + 1);
                    break;
                }
            }
            started = true;
            match end {
                Some(end) => rest = &rest[end..],
                None => return (leading_close, net),
            }
            continue;
        }
        if c == '`' {
            started = true;
            rest = &rest[1..];
            *inside = Inside::Template;
            continue;
        }
        match c {
            '{' | '(' | '[' => {
                net += 1;
                started = true;
            }
            '}' | ')' | ']' => {
                net -= 1;
                if !started {
                    leading_close += 1;
                }
            }
            ' ' | '\t' => {}
            _ => started = true,
        }
        rest = &rest[c.len_utf8()..];
    }
    (leading_close, net)
}

/// Indents lines top..=bottom by how deep their brackets are, working that out from the top of the file. Only the
/// space before each line changes. Blank lines, and lines inside a comment or a backtick string, stay as they are.
pub fn reindent(lines: &[Arc<str>], top: usize, bottom: usize, unit: &str, marks: &Marks<'_>) -> Vec<String> {
    let mut out = Vec::with_capacity(bottom.saturating_sub(top) + 1);
    let mut depth: i64 = 0;
    let mut inside = Inside::Code;
    for (number, line) in lines.iter().enumerate().take(bottom + 1) {
        let was_code = inside == Inside::Code;
        let preprocessor = marks.preprocessor && was_code && line.trim_start().starts_with('#');
        let (leading_close, net) = if preprocessor { (0, 0) } else { scan_line(line, marks, &mut inside) };
        if number >= top {
            if !was_code || preprocessor || is_blank(line) {
                out.push(line.to_string());
            } else {
                let level = (depth - leading_close).max(0) as usize;
                out.push(format!("{}{}", unit.repeat(level), line.trim_start_matches([' ', '\t'])));
            }
        }
        depth = (depth + net).max(0);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn arcs(lines: &[&str]) -> Vec<Arc<str>> {
        lines.iter().map(|line| Arc::from(*line)).collect()
    }

    const C: Marks<'static> = Marks { line: Some("//"), block: Some(("/*", "*/")), preprocessor: true };

    #[test]
    fn long_lines_break_at_spaces_and_short_ones_stay() {
        assert_eq!(wrap_to_line_ends(&["one two three four", "short"], 9), ["one two", "three", "four", "short"]);
    }

    #[test]
    fn wrapping_keeps_the_indent_and_a_too_long_word_whole() {
        assert_eq!(wrap_to_line_ends(&["    aaa bbb ccc"], 10), ["    aaa", "    bbb", "    ccc"]);
        assert_eq!(wrap_to_line_ends(&["abcdefghijklmnop q"], 5), ["abcdefghijklmnop", "q"]);
        assert_eq!(wrap_to_line_ends(&["abcdefghij"], 5), ["abcdefghij"]);
        assert_eq!(wrap_to_line_ends(&[""], 5), [""]);
    }

    #[test]
    fn wrapping_loses_no_words_and_every_line_fits_if_it_can() {
        let text = "the quick brown fox jumps over the lazy dog ".repeat(20);
        for width in [10, 11, 17, 40, 79] {
            let rows = wrap_to_line_ends(&[text.as_str()], width);
            assert!(rows.iter().all(|row| visual_len(row) <= width), "width {width}");
            assert_eq!(rows.join(" ").split_whitespace().collect::<Vec<_>>(), text.split_whitespace().collect::<Vec<_>>());
        }
    }

    #[test]
    fn line_ends_to_wrap_joins_paragraphs_only() {
        let joined = line_ends_to_wrap(&["  one", "two  ", "", "three", "- item", "more", "1. next", "  "]);
        assert_eq!(joined, ["  one two", "", "three", "- item more", "1. next", "  "]);
    }

    #[test]
    fn wrapping_then_joining_gets_the_words_back() {
        let line = "the quick brown fox jumps over the lazy dog";
        assert_eq!(line_ends_to_wrap(&wrap_to_line_ends(&[line], 12)), [line]);
    }

    #[test]
    fn reformat_rewraps_each_paragraph_to_the_setup() {
        let setup = ParagraphSetup { width: 12, ..ParagraphSetup::default() };
        assert_eq!(reformat_paragraphs(&["one two", "three four five", "", "six"], &setup), ["one two", "three four", "five", "", "six"]);
    }

    #[test]
    fn reformat_uses_the_margins_and_a_first_line_indent() {
        let setup = ParagraphSetup { width: 14, left: 2, first: 3, ..ParagraphSetup::default() };
        assert_eq!(reformat_paragraphs(&["one two three four"], &setup), ["     one two", "  three four"]);
    }

    #[test]
    fn reformat_keeps_list_markers_and_hangs_lines_under_the_text() {
        let setup = ParagraphSetup { width: 12, ..ParagraphSetup::default() };
        assert_eq!(reformat_paragraphs(&["- one two three", "  four", "- five"], &setup), ["- one two", "  three four", "- five"]);
        assert_eq!(reformat_paragraphs(&["  1. aaa bbb ccc"], &setup), ["  1. aaa bbb", "     ccc"]);
    }

    #[test]
    fn reformat_copes_with_a_word_wider_than_the_page_and_small_widths() {
        let setup = ParagraphSetup { width: 1, ..ParagraphSetup::default() };
        assert_eq!(reformat_paragraphs(&["abcdefghijklmnopqrst u"], &setup), ["abcdefghijklmnopqrst", "u"], "the width has a floor");
        assert_eq!(reformat_paragraphs(&["-   "], &ParagraphSetup::default()), ["-"]);
    }

    #[test]
    fn alignment_places_each_line_within_the_width() {
        assert_eq!(align_lines(&["  ab  "], Align::Left, 10, 0), ["ab"]);
        assert_eq!(align_lines(&["ab"], Align::Left, 10, 3), ["   ab"]);
        assert_eq!(align_lines(&["ab"], Align::Center, 10, 0), ["    ab"]);
        assert_eq!(align_lines(&["ab"], Align::Right, 10, 0), ["        ab"]);
        assert_eq!(align_lines(&["a very long line indeed"], Align::Right, 10, 0), ["a very long line indeed"], "too long just stays at the margin");
        assert_eq!(align_lines(&["", "  "], Align::Center, 10, 0), ["", "  "]);
    }

    #[test]
    fn justify_fills_lines_and_leaves_each_paragraphs_last_line() {
        let rows = align_lines(&["a b c", "d e", "", "f g"], Align::Justify, 9, 0);
        assert_eq!(rows, ["a   b   c", "d e", "", "f g"]);
        assert_eq!(visual_len(&rows[0]), 9);
    }

    #[test]
    fn reformat_can_justify_center_and_right() {
        let mut setup = ParagraphSetup { width: 10, align: Align::Justify, ..ParagraphSetup::default() };
        assert_eq!(reformat_paragraphs(&["aa bb cc dd"], &setup), ["aa  bb  cc", "dd"]);
        setup.align = Align::Right;
        assert_eq!(reformat_paragraphs(&["aa bb cc dd"], &setup), ["  aa bb cc", "        dd"]);
        setup.align = Align::Center;
        assert_eq!(reformat_paragraphs(&["aa bb cc dd"], &setup), [" aa bb cc", "    dd"]);
    }

    #[test]
    fn reindent_goes_by_bracket_depth() {
        let lines = arcs(&["fn a() {", "x(", "1,", ");", "if y {", "z;", "}", "}", "next"]);
        assert_eq!(reindent(&lines, 0, 8, "  ", &C), ["fn a() {", "  x(", "    1,", "  );", "  if y {", "    z;", "  }", "}", "next"]);
    }

    #[test]
    fn reindent_works_out_the_depth_from_above_the_selection() {
        let lines = arcs(&["fn a() {", "if b {", "c;", "}", "}"]);
        assert_eq!(reindent(&lines, 2, 3, "\t", &C), ["\t\tc;", "\t}"]);
    }

    #[test]
    fn brackets_in_strings_and_comments_dont_count() {
        let lines = arcs(&["a = \"{{{\";", "b = '(';", "// {{{", "/* {{{", "   {{{ */ c;", "d;"]);
        let rows = reindent(&lines, 0, 5, "  ", &C);
        assert_eq!(rows[5], "d;");
        assert_eq!(rows[4], "   {{{ */ c;", "a line inside a comment stays as it is");
    }

    #[test]
    fn apostrophes_in_plain_text_do_no_harm() {
        let lines = arcs(&["don't {", "x", "}"]);
        assert_eq!(reindent(&lines, 0, 2, "  ", &Marks::default()), ["don't {", "x", "}"].map(str::to_string), "the open quote hid the bracket to the end of its line, and no further");
    }

    #[test]
    fn backtick_strings_keep_their_lines_and_preprocessor_lines_stay() {
        let lines = arcs(&["a = `", "   {", "`;", "b;"]);
        assert_eq!(reindent(&lines, 0, 3, "  ", &C)[1], "   {");
        assert_eq!(reindent(&lines, 0, 3, "  ", &C)[3], "b;");
        let c = arcs(&["void f() {", "  #define X {", "y;", "}"]);
        assert_eq!(reindent(&c, 0, 3, "  ", &C), ["void f() {", "  #define X {", "  y;", "}"]);
    }

    #[test]
    fn too_many_closers_stop_at_the_margin() {
        let lines = arcs(&["}", "}", "x"]);
        assert_eq!(reindent(&lines, 0, 2, "  ", &C), ["}", "}", "x"]);
    }

    #[test]
    fn reindent_only_ever_changes_the_space_in_front() {
        let lines = arcs(&["  a  {  ", "\t\tb  ", "", "   ", "}"]);
        let rows = reindent(&lines, 0, 4, " ", &C);
        for (row, line) in rows.iter().zip(&lines) {
            assert_eq!(row.trim_start(), line.trim_start());
        }
    }
}
