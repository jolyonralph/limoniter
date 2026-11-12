//! Finding the thing around the cursor: what's inside some quotes or brackets, a tag, a word, a line, a paragraph.
//! Places are a line and a character along it. Nothing here changes any text.

use std::sync::Arc;

/// How far a search goes each way, so that a file with no closing bracket can't hold up the window.
const MOST_LINES_SEARCHED: usize = 20_000;
/// The longest paragraph it will walk along, each way.
const MOST_PARAGRAPH_LINES: usize = 200_000;

pub type Place = (usize, usize);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Object {
    Quotes,
    Brackets,
    Tag,
    Word,
    Line,
    Paragraph,
}

impl Object {
    #[cfg(test)]
    pub const ALL: [Self; 6] = [Self::Quotes, Self::Brackets, Self::Tag, Self::Word, Self::Line, Self::Paragraph];


    pub fn not_found(self) -> &'static str {
        match self {
            Self::Quotes => "There are no quotes around the cursor, or after it on this line.",
            Self::Brackets => "The cursor isn't inside any brackets.",
            Self::Tag => "The cursor isn't inside a pair of tags.",
            Self::Word | Self::Line | Self::Paragraph => "There's nothing here to select.",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    SelectInside,
    SelectAround,
    ReplaceInside,
    DeleteInside,
    DeleteAround,
}

impl Action {
    #[cfg(test)]
    pub const ALL: [Self; 5] = [Self::SelectInside, Self::SelectAround, Self::ReplaceInside, Self::DeleteInside, Self::DeleteAround];

}

/// What was found: its contents, and its contents with whatever marks them out.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Found {
    pub inside: (Place, Place),
    pub around: (Place, Place),
}

/// Reads a line. Asking past the end of the file is never done.
pub type Lines<'a> = &'a dyn Fn(usize) -> Arc<str>;

/// Finds what's around the part from `start` to `end`, which are the same if nothing is selected.
pub fn find(lines: Lines<'_>, count: usize, start: Place, end: Place, object: Object) -> Option<Found> {
    if count == 0 {
        return None;
    }
    match object {
        Object::Quotes => find_quotes(lines, start, end),
        Object::Brackets => find_brackets(lines, count, start, end),
        Object::Tag => find_tag(lines, count, start, end),
        Object::Word => find_word(lines, start),
        Object::Line => find_line(lines, count, start),
        Object::Paragraph => find_paragraph(lines, count, start),
    }
}

fn chars_of(line: &str) -> Vec<char> {
    line.chars().collect()
}

// ---------- Quotes ----------

fn find_quotes(lines: Lines<'_>, start: Place, end: Place) -> Option<Found> {
    // Quotes go by the line, because one left open would otherwise pair with one far away.
    if start.0 != end.0 {
        return None;
    }
    let line = start.0;
    let text = chars_of(&lines(line));
    let spans = quote_spans(&text);
    let containing = spans.iter().find(|(open, close)| *open <= start.1 && end.1 <= close + 1);
    // On no quotes at all, the next ones along the line, which is usually what's wanted.
    let (open, close) = *containing.or_else(|| spans.iter().find(|(open, _)| *open >= start.1))?;
    Some(Found { inside: ((line, open + 1), (line, close)), around: ((line, open), (line, close + 1)) })
}

/// Each quoted stretch, as where it opens and closes. An apostrophe in a word doesn't open one.
fn quote_spans(text: &[char]) -> Vec<(usize, usize)> {
    let mut spans = Vec::new();
    let mut at = 0;
    while at < text.len() {
        let c = text[at];
        let opens = matches!(c, '"' | '`') || (c == '\'' && !(at > 0 && text[at - 1].is_alphanumeric()));
        if !opens || (at > 0 && text[at - 1] == '\\') {
            at += 1;
            continue;
        }
        let mut close = at + 1;
        while close < text.len() {
            if text[close] == '\\' {
                close += 2;
                continue;
            }
            if text[close] == c {
                break;
            }
            close += 1;
        }
        if close < text.len() {
            spans.push((at, close));
            at = close + 1;
        } else {
            at += 1;
        }
    }
    spans
}

// ---------- Brackets ----------

fn closer_of(open: char) -> Option<char> {
    match open {
        '(' => Some(')'),
        '[' => Some(']'),
        '{' => Some('}'),
        _ => None,
    }
}

fn opener_of(close: char) -> Option<char> {
    match close {
        ')' => Some('('),
        ']' => Some('['),
        '}' => Some('{'),
        _ => None,
    }
}

fn found_pair(open: Place, close: Place) -> Found {
    Found { inside: ((open.0, open.1 + 1), close), around: (open, (close.0, close.1 + 1)) }
}

/// Where the bracket that closes this one is.
fn closing_after(lines: Lines<'_>, count: usize, open: Place, opener: char) -> Option<Place> {
    let closer = closer_of(opener)?;
    let mut depth = 0usize;
    let last = count.min(open.0.saturating_add(MOST_LINES_SEARCHED));
    for line in open.0..last {
        let text = chars_of(&lines(line));
        let from = if line == open.0 { open.1 + 1 } else { 0 };
        for (index, c) in text.iter().enumerate().skip(from) {
            if *c == opener {
                depth += 1;
            } else if *c == closer {
                if depth == 0 {
                    return Some((line, index));
                }
                depth -= 1;
            }
        }
    }
    None
}

/// Where the bracket that opens this one is.
fn opening_before(lines: Lines<'_>, close: Place, closer: char) -> Option<Place> {
    let opener = opener_of(closer)?;
    let mut depth = 0usize;
    let first = close.0.saturating_sub(MOST_LINES_SEARCHED);
    for line in (first..=close.0).rev() {
        let text = chars_of(&lines(line));
        let to = if line == close.0 { close.1.min(text.len()) } else { text.len() };
        for index in (0..to).rev() {
            if text[index] == closer {
                depth += 1;
            } else if text[index] == opener {
                if depth == 0 {
                    return Some((line, index));
                }
                depth -= 1;
            }
        }
    }
    None
}

fn find_brackets(lines: Lines<'_>, count: usize, start: Place, end: Place) -> Option<Found> {
    // With nothing selected, a cursor right beside a bracket means that bracket.
    if start == end {
        let text = chars_of(&lines(start.0));
        if let Some(opener) = text.get(start.1).copied().filter(|c| closer_of(*c).is_some())
            && let Some(close) = closing_after(lines, count, start, opener)
        {
            return Some(found_pair(start, close));
        }
        if start.1 > 0
            && let Some(closer) = text.get(start.1 - 1).copied().filter(|c| opener_of(*c).is_some())
            && let Some(open) = opening_before(lines, (start.0, start.1 - 1), closer)
        {
            return Some(found_pair(open, (start.0, start.1 - 1)));
        }
    }
    // Back from the start of the selection to the first bracket that's still open, as long as it closes after the end.
    let mut waiting: Vec<char> = Vec::new();
    let first = start.0.saturating_sub(MOST_LINES_SEARCHED);
    for line in (first..=start.0).rev() {
        let text = chars_of(&lines(line));
        let to = if line == start.0 { start.1.min(text.len()) } else { text.len() };
        for index in (0..to).rev() {
            let c = text[index];
            if opener_of(c).is_some() {
                waiting.push(c);
            } else if closer_of(c).is_some() {
                if waiting.last().and_then(|closer| opener_of(*closer)) == Some(c) {
                    waiting.pop();
                } else if let Some(close) = closing_after(lines, count, (line, index), c)
                    && close >= end
                {
                    return Some(found_pair((line, index), close));
                }
            }
        }
    }
    None
}

// ---------- Tags ----------

/// Tags that never have a closing one.
const VOID_TAGS: [&str; 14] = ["area", "base", "br", "col", "embed", "hr", "img", "input", "link", "meta", "param", "source", "track", "wbr"];

struct TagMark {
    name: String,
    start: Place,
    end: Place,
    closing: bool,
}

/// The tags in some lines. A tag has to fit on one line to be seen, which leaves out very little.
fn tags_in(lines: Lines<'_>, first: usize, last: usize) -> Vec<TagMark> {
    let mut tags = Vec::new();
    let mut in_comment = false;
    for line in first..last {
        let text = chars_of(&lines(line));
        let mut at = 0;
        while at < text.len() {
            if in_comment {
                if text[at..].starts_with(&['-', '-', '>']) {
                    in_comment = false;
                    at += 2;
                }
                at += 1;
                continue;
            }
            if text[at] != '<' {
                at += 1;
                continue;
            }
            if text[at..].starts_with(&['<', '!', '-', '-']) {
                in_comment = true;
                at += 4;
                continue;
            }
            let closing = text.get(at + 1) == Some(&'/');
            let name_from = at + 1 + usize::from(closing);
            let name: String = text[name_from.min(text.len())..].iter().take_while(|c| c.is_alphanumeric() || matches!(c, '-' | '_' | ':' | '.')).collect();
            let Some(close) = text[at..].iter().position(|c| *c == '>').map(|place| at + place) else {
                at += 1;
                continue;
            };
            let self_closed = text[close - 1] == '/';
            let lower = name.to_lowercase();
            if !name.is_empty() && !self_closed && !VOID_TAGS.contains(&lower.as_str()) {
                tags.push(TagMark { name: lower, start: (line, at), end: (line, close + 1), closing });
            }
            at = close + 1;
        }
    }
    tags
}

fn find_tag(lines: Lines<'_>, count: usize, start: Place, end: Place) -> Option<Found> {
    let first = start.0.saturating_sub(MOST_LINES_SEARCHED);
    let last = count.min(end.0.saturating_add(MOST_LINES_SEARCHED));
    let tags = tags_in(lines, first, last);
    let mut open: Vec<&TagMark> = Vec::new();
    let mut best: Option<Found> = None;
    for tag in &tags {
        if !tag.closing {
            open.push(tag);
            continue;
        }
        // A closing tag ends the nearest one of its name. Any left open inside it are ignored, as browsers do.
        let Some(place) = open.iter().rposition(|candidate| candidate.name == tag.name) else {
            continue;
        };
        let opener = open[place];
        open.truncate(place);
        let found = Found { inside: (opener.end, tag.start), around: (opener.start, tag.end) };
        // It has to hold the selection, and be more than the selection, so that a second go reaches further out.
        let holds = found.around.0 <= start && end <= found.around.1 && found.around != (start, end);
        let smaller = best.is_none_or(|known| found.around.0 >= known.around.0 && found.around.1 <= known.around.1);
        if holds && smaller {
            best = Some(found);
        }
    }
    best
}

// ---------- Words, lines and paragraphs ----------

fn is_word(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

fn find_word(lines: Lines<'_>, at: Place) -> Option<Found> {
    let line = at.0;
    let text = chars_of(&lines(line));
    if text.is_empty() {
        return None;
    }
    let here = at.1.min(text.len());
    // Right after a word counts as on it.
    let on = if here < text.len() && !text[here].is_whitespace() && (is_word(text[here]) || here == 0 || !is_word(text[here - 1])) { here } else if here > 0 { here - 1 } else { here };
    let kind = |c: char| if c.is_whitespace() { 0 } else if is_word(c) { 1 } else { 2 };
    let wanted = kind(text[on]);
    let mut from = on;
    while from > 0 && kind(text[from - 1]) == wanted {
        from -= 1;
    }
    let mut to = on + 1;
    while to < text.len() && kind(text[to]) == wanted {
        to += 1;
    }
    // Around a word takes the spaces after it, or if there are none, those before it.
    let mut after = to;
    while after < text.len() && text[after].is_whitespace() {
        after += 1;
    }
    let mut before = from;
    if after == to {
        while before > 0 && text[before - 1].is_whitespace() {
            before -= 1;
        }
    }
    Some(Found { inside: ((line, from), (line, to)), around: ((line, before), (line, after)) })
}

fn find_line(lines: Lines<'_>, count: usize, at: Place) -> Option<Found> {
    let line = at.0;
    let text = chars_of(&lines(line));
    let from = text.iter().position(|c| !c.is_whitespace()).unwrap_or(text.len());
    let to = text.iter().rposition(|c| !c.is_whitespace()).map_or(from, |last| last + 1);
    // Around takes the line ending, or for the last line, the ending before it.
    let around = if line + 1 < count {
        ((line, 0), (line + 1, 0))
    } else if line > 0 {
        ((line - 1, lines(line - 1).chars().count()), (line, text.len()))
    } else {
        ((line, 0), (line, text.len()))
    };
    Some(Found { inside: ((line, from), (line, to)), around })
}

fn find_paragraph(lines: Lines<'_>, count: usize, at: Place) -> Option<Found> {
    let blank = |line: usize| lines(line).chars().all(char::is_whitespace);
    let line = at.0;
    let kind = blank(line);
    let mut top = line;
    while top > 0 && line - top < MOST_PARAGRAPH_LINES && blank(top - 1) == kind {
        top -= 1;
    }
    let mut bottom = line;
    while bottom + 1 < count && bottom - line < MOST_PARAGRAPH_LINES && blank(bottom + 1) == kind {
        bottom += 1;
    }
    let inside = ((top, 0), (bottom, lines(bottom).chars().count()));
    // Around takes the blank lines after it, up to the next text. At the end of the file, those before it instead.
    let mut next = bottom + 1;
    while next < count && next - bottom < MOST_PARAGRAPH_LINES && blank(next) != kind {
        next += 1;
    }
    let around = if bottom + 1 >= count {
        let mut before = top;
        while before > 0 && top - before < MOST_PARAGRAPH_LINES && blank(before - 1) != kind {
            before -= 1;
        }
        if before == top { inside } else { ((before - 1, lines(before - 1).chars().count()), inside.1) }
    } else if next >= count {
        (inside.0, ((count - 1), lines(count - 1).chars().count()))
    } else {
        (inside.0, (next, 0))
    };
    Some(Found { inside, around })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn finder(text: &str, start: Place, end: Place, object: Object) -> Option<Found> {
        let rows: Vec<Arc<str>> = text.split('\n').map(Arc::from).collect();
        let read = |line: usize| rows[line].clone();
        find(&read, rows.len(), start, end, object)
    }

    fn at(text: &str, place: Place, object: Object) -> Option<Found> {
        finder(text, place, place, object)
    }

    /// The text of a stretch.
    fn piece(text: &str, range: (Place, Place)) -> String {
        let rows: Vec<&str> = text.split('\n').collect();
        let mut out = String::new();
        for (line, row) in rows.iter().enumerate().take(range.1.0 + 1).skip(range.0.0) {
            let chars: Vec<char> = row.chars().collect();
            let from = if line == range.0.0 { range.0.1 } else { 0 };
            let to = if line == range.1.0 { range.1.1 } else { chars.len() };
            out.extend(&chars[from..to]);
            if line != range.1.0 {
                out.push('\n');
            }
        }
        out
    }

    fn both(text: &str, place: Place, object: Object) -> (String, String) {
        let found = at(text, place, object).unwrap();
        (piece(text, found.inside), piece(text, found.around))
    }

    #[test]
    fn quotes_of_each_kind_are_found() {
        let text = r#"say "hello there" and 'bye' and `run`"#;
        assert_eq!(both(text, (0, 7), Object::Quotes), ("hello there".into(), "\"hello there\"".into()));
        assert_eq!(both(text, (0, 24), Object::Quotes), ("bye".into(), "'bye'".into()));
        assert_eq!(both(text, (0, 34), Object::Quotes), ("run".into(), "`run`".into()));
    }

    #[test]
    fn quotes_are_found_from_before_them_and_right_on_them() {
        let text = r#"say "hello""#;
        assert_eq!(both(text, (0, 0), Object::Quotes).0, "hello", "the next ones along");
        assert_eq!(both(text, (0, 4), Object::Quotes).0, "hello", "on the opening one");
        assert_eq!(both(text, (0, 11), Object::Quotes).0, "hello", "just after the closing one");
        assert!(at("no quotes here", (0, 3), Object::Quotes).is_none());
        assert!(at(r#"past "them" now"#, (0, 13), Object::Quotes).is_none());
    }

    #[test]
    fn apostrophes_and_escapes_dont_confuse_quotes() {
        let text = r#"don't touch 'this one' ok"#;
        assert_eq!(both(text, (0, 15), Object::Quotes).0, "this one");
        let escaped = r#"a "b \" c" d"#;
        assert_eq!(both(escaped, (0, 4), Object::Quotes).0, r#"b \" c"#);
        assert_eq!(both(r#""it's fine""#, (0, 3), Object::Quotes).0, "it's fine", "an apostrophe inside another kind");
    }

    #[test]
    fn a_quote_never_closes_on_another_line() {
        assert!(at("an \"open\nand \"more", (0, 5), Object::Quotes).is_none());
        assert!(finder("\"a\"\n\"b\"", (0, 1), (1, 1), Object::Quotes).is_none(), "a selection across lines has none around it");
    }

    #[test]
    fn brackets_find_the_innermost_and_go_across_lines() {
        let text = "f(a, [b, c], {\n  d\n})";
        assert_eq!(both(text, (0, 6), Object::Brackets), ("b, c".into(), "[b, c]".into()));
        assert_eq!(both(text, (1, 2), Object::Brackets).0, "\n  d\n");
        assert_eq!(both(text, (0, 3), Object::Brackets).0, "a, [b, c], {\n  d\n}");
    }

    #[test]
    fn brackets_right_beside_the_cursor_are_the_ones_meant() {
        let text = "(a (b) c)";
        assert_eq!(both(text, (0, 3), Object::Brackets).0, "b", "just before the opener");
        assert_eq!(both(text, (0, 6), Object::Brackets).0, "b", "just after the closer");
        assert_eq!(both(text, (0, 0), Object::Brackets).0, "a (b) c");
    }

    #[test]
    fn brackets_reach_further_out_from_a_selection() {
        let text = "(a (b) c)";
        // From the inside of the inner pair, that's the pair. From all of it, the outer one.
        let inner = finder(text, (0, 4), (0, 5), Object::Brackets).unwrap();
        assert_eq!(piece(text, inner.around), "(b)");
        let outer = finder(text, (0, 3), (0, 6), Object::Brackets).unwrap();
        assert_eq!(piece(text, outer.around), "(a (b) c)");
        assert!(finder(text, (0, 0), (0, 9), Object::Brackets).is_none(), "nothing's outside that");
    }

    #[test]
    fn unmatched_brackets_are_said_to_be_missing() {
        assert!(at("no brackets", (0, 3), Object::Brackets).is_none());
        assert!(at("(never closed", (0, 3), Object::Brackets).is_none());
        assert!(at("closed) only", (0, 3), Object::Brackets).is_none());
        assert_eq!(both("(a] b)", (0, 2), Object::Brackets).0, "a] b", "a stray one of another kind");
    }

    #[test]
    fn tags_pair_up_by_name_and_nest() {
        let text = "<div class=\"a\">\n  <b>bold</b> <br> <img src=x/>\n</div>";
        assert_eq!(both(text, (1, 7), Object::Tag), ("bold".into(), "<b>bold</b>".into()));
        let div = at(text, (1, 15), Object::Tag).unwrap();
        assert_eq!(piece(text, div.around).lines().next().unwrap(), "<div class=\"a\">");
        // All of the inner pair selected reaches the outer one.
        let outer = finder(text, (1, 2), (1, 13), Object::Tag).unwrap();
        assert!(piece(text, outer.around).starts_with("<div"));
        assert!(at("no tags < at all", (0, 2), Object::Tag).is_none());
        assert!(at("<!-- <b> --> x </b>", (0, 15), Object::Tag).is_none(), "a tag in a comment doesn't count");
    }

    #[test]
    fn words_lines_and_their_spaces() {
        assert_eq!(both("one two  three", (0, 5), Object::Word), ("two".into(), "two  ".into()));
        assert_eq!(both("one two", (0, 7), Object::Word), ("two".into(), " two".into()), "right after, at the end of the line");
        assert_eq!(both("one, two", (0, 3), Object::Word).0, "one", "between a word and punctuation, the word");
        assert_eq!(both("one, two", (0, 4), Object::Word).0, ",", "right after punctuation");
        assert!(at("", (0, 0), Object::Word).is_none());
        assert_eq!(both("  both ends  \nnext", (0, 5), Object::Line), ("both ends".into(), "  both ends  \n".into()));
        assert_eq!(both("first\n  last", (1, 3), Object::Line), ("last".into(), "\n  last".into()), "the last line takes the ending before it");
        assert_eq!(both("only", (0, 1), Object::Line), ("only".into(), "only".into()));
    }

    #[test]
    fn paragraphs_end_at_blank_lines() {
        let text = "one\ntwo\n\n   \nthree\nfour\n\nfive";
        assert_eq!(both(text, (1, 0), Object::Paragraph), ("one\ntwo".into(), "one\ntwo\n\n   \n".into()));
        assert_eq!(both(text, (5, 2), Object::Paragraph), ("three\nfour".into(), "three\nfour\n\n".into()));
        assert_eq!(both(text, (7, 0), Object::Paragraph), ("five".into(), "\n\nfive".into()), "at the end it takes the blank before");
        assert_eq!(both(text, (2, 0), Object::Paragraph).0, "\n   ", "a blank stretch is one too");
        assert_eq!(both("alone", (0, 2), Object::Paragraph), ("alone".into(), "alone".into()));
    }

    #[test]
    fn a_place_past_the_end_of_a_line_is_no_trouble() {
        for object in Object::ALL {
            let _ = at("short (x) \"y\"", (0, 500), object);
            let _ = at("", (0, 0), object);
            let _ = finder("a\nb", (0, 500), (1, 500), object);
        }
    }

    #[test]
    fn a_very_long_search_gives_up_rather_than_going_on() {
        let rows: Vec<Arc<str>> = std::iter::once(Arc::from("(")).chain((0..MOST_LINES_SEARCHED + 100).map(|_| Arc::from("x"))).chain(std::iter::once(Arc::from(")"))).collect();
        let read = |line: usize| rows[line].clone();
        let place = (rows.len() - 2, 0);
        assert!(find(&read, rows.len(), place, place, Object::Brackets).is_none());
    }
}
