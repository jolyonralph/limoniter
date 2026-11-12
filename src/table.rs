//! Delimited data (CSV, TSV and their kind) seen as a table. The text stays the document: this reads records and
//! fields out of its lines, and works out the new lines an edit to a cell, a row or a column makes, changing only
//! what was edited so every other field keeps its quoting and spacing exactly. Nothing here draws anything.
//!
//! Fields follow RFC 4180 as most programs write it: a field that starts with a quote runs to the closing quote,
//! with "" for a quote inside it, and may hold delimiters and line breaks. A quote anywhere else is just a
//! character, so `5'10"` in an unquoted field doesn't swallow the rest of the file.

use std::cmp::Ordering;
use std::sync::Arc;

/// The delimiters a table can use, with their names.
pub const DELIMITERS: [(&str, char); 4] = [("Comma", ','), ("Tab", '\t'), ("Semicolon", ';'), ("Pipe", '|')];

/// A record is given up on after this many lines, so one stray quote can't make the rest of a file one cell.
const MOST_LINES_IN_A_RECORD: usize = 1000;

/// How the data is written.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Dialect {
    pub delimiter: char,
    /// The first record holds the columns' names.
    pub header: bool,
}

/// One record: the lines it takes up, first and last.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Record {
    pub first: usize,
    pub last: usize,
    /// Its quote is never closed, within the lines allowed for one record.
    pub broken: bool,
}

/// One field: its value, and where its text sits in the record's text (the record's lines joined by "\n").
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Field {
    pub value: String,
    pub start: usize,
    pub end: usize,
    pub quoted: bool,
}

/// Where reading a line leaves things, for the line after it.
#[derive(Clone, Copy, PartialEq, Eq)]
enum State {
    /// At the start of a field.
    Start,
    /// In a field without quotes.
    Plain,
    /// Inside quotes.
    Quoted,
    /// Just after a quote inside quotes: the end of the quotes, or the first of "".
    QuoteInQuoted,
    /// After the closing quote, before the delimiter.
    AfterQuoted,
}

fn step(state: State, c: char, delimiter: char) -> State {
    match (state, c) {
        (State::Start, '"') => State::Quoted,
        (State::Start | State::Plain | State::AfterQuoted, c) if c == delimiter => State::Start,
        (State::Start | State::Plain, _) => State::Plain,
        (State::Quoted, '"') => State::QuoteInQuoted,
        (State::Quoted, _) => State::Quoted,
        (State::QuoteInQuoted, '"') => State::Quoted,
        (State::QuoteInQuoted, c) if c == delimiter => State::Start,
        (State::QuoteInQuoted | State::AfterQuoted, _) => State::AfterQuoted,
    }
}

/// Whether a line ends inside quotes, given how the line before it left things.
fn ends_in_quotes(line: &str, delimiter: char, mut state: State) -> State {
    for c in line.chars() {
        state = step(state, c, delimiter);
    }
    state
}

/// Finds the records in a run of lines. A last line that's empty, as most files end, isn't a record.
pub fn records<S: AsRef<str>>(lines: &[S], delimiter: char) -> Vec<Record> {
    let count = if lines.last().is_some_and(|l| l.as_ref().is_empty()) && lines.len() > 1 { lines.len() - 1 } else { lines.len() };
    let mut out = Vec::new();
    let mut first = 0;
    while first < count {
        let mut state = ends_in_quotes(lines[first].as_ref(), delimiter, State::Start);
        let mut last = first;
        // A line break inside quotes carries the field on to the next line.
        while state == State::Quoted && last + 1 < count && last - first + 1 < MOST_LINES_IN_A_RECORD {
            last += 1;
            state = ends_in_quotes(lines[last].as_ref(), delimiter, step(State::Quoted, '\n', delimiter));
        }
        let broken = state == State::Quoted;
        // A record whose quote never closes is kept to its own first line, so what follows still reads.
        let last = if broken { first } else { last };
        out.push(Record { first, last, broken });
        first = last + 1;
    }
    out
}

/// The fields of a record's text.
pub fn fields(text: &str, delimiter: char) -> Vec<Field> {
    let mut out = Vec::new();
    let chars: Vec<(usize, char)> = text.char_indices().collect();
    let mut i = 0;
    loop {
        let start = chars.get(i).map_or(text.len(), |(at, _)| *at);
        let mut value = String::new();
        let quoted = chars.get(i).is_some_and(|(_, c)| *c == '"');
        if quoted {
            i += 1;
            while i < chars.len() {
                let c = chars[i].1;
                if c == '"' {
                    if chars.get(i + 1).is_some_and(|(_, n)| *n == '"') {
                        value.push('"');
                        i += 2;
                        continue;
                    }
                    i += 1;
                    break;
                }
                value.push(c);
                i += 1;
            }
        }
        // Anything after a closing quote, or a field with none, runs to the delimiter as it is.
        while i < chars.len() && chars[i].1 != delimiter {
            value.push(chars[i].1);
            i += 1;
        }
        let end = chars.get(i).map_or(text.len(), |(at, _)| *at);
        out.push(Field { value, start, end, quoted });
        if i >= chars.len() {
            return out;
        }
        i += 1;
    }
}

/// A record's lines, joined as its fields are measured against.
pub fn record_text<S: AsRef<str>>(lines: &[S], record: Record) -> String {
    lines[record.first..=record.last].iter().map(AsRef::as_ref).collect::<Vec<_>>().join("\n")
}

/// A value as it's written in a field: in quotes if it has to be, or if the field was quoted before.
pub fn encode_field(value: &str, delimiter: char, quote_anyway: bool) -> String {
    let needs = quote_anyway || value.contains([delimiter, '"', '\n', '\r']) || value.starts_with([' ', '\t']) || value.ends_with([' ', '\t']);
    if needs { format!("\"{}\"", value.replace('"', "\"\"")) } else { value.to_string() }
}

/// The record's text with one field set, everything else as it was. A field past the end is reached by adding
/// empty ones.
pub fn set_field(text: &str, delimiter: char, column: usize, value: &str) -> String {
    let fields = fields(text, delimiter);
    match fields.get(column) {
        Some(field) => format!("{}{}{}", &text[..field.start], encode_field(value, delimiter, field.quoted), &text[field.end..]),
        None => {
            let mut out = text.to_string();
            for _ in fields.len()..column {
                out.push(delimiter);
            }
            out.push(delimiter);
            out += &encode_field(value, delimiter, false);
            out
        }
    }
}

/// The record's text with an empty field put in before a column (or after the last).
pub fn insert_field(text: &str, delimiter: char, column: usize) -> String {
    let fields = fields(text, delimiter);
    match fields.get(column) {
        Some(field) => format!("{}{delimiter}{}", &text[..field.start], &text[field.start..]),
        None => {
            let mut out = text.to_string();
            for _ in fields.len()..=column {
                out.push(delimiter);
            }
            out
        }
    }
}

/// The record's text without one of its fields, and the delimiter that went with it.
pub fn remove_field(text: &str, delimiter: char, column: usize) -> String {
    let fields = fields(text, delimiter);
    let Some(field) = fields.get(column) else {
        return text.to_string();
    };
    if fields.len() == 1 {
        return String::new();
    }
    // The delimiter after it goes with it, or for the last field, the one before.
    let delimiter_len = delimiter.len_utf8();
    if column + 1 < fields.len() { format!("{}{}", &text[..field.start], &text[fields[column + 1].start..]) } else { text[..field.start - delimiter_len].to_string() }
}

/// An empty record with this many fields.
pub fn empty_record(delimiter: char, columns: usize) -> String {
    std::iter::repeat_n(delimiter, columns.saturating_sub(1)).collect()
}

/// The record's text with several fields set, everything else as it was.
pub fn set_fields(text: &str, delimiter: char, changes: &[(usize, String)]) -> String {
    changes.iter().fold(text.to_string(), |text, (column, value)| set_field(&text, delimiter, *column, value))
}

/// A block of cells as text for the clipboard: a tab between cells and a line break between rows, as spreadsheets
/// copy and paste them. A cell with a tab, line break or quote in it is put in quotes.
pub fn block_text(cells: &[Vec<String>]) -> String {
    cells.iter().map(|row| row.iter().map(|value| encode_field(value, '\t', false)).collect::<Vec<_>>().join("\t")).collect::<Vec<_>>().join("\n")
}

/// Cells from the clipboard: rows of tab-separated cells, as a spreadsheet copies them, quoted cells and all.
/// Text with no tabs is one cell to a line. The line break that ends the last row isn't another row.
pub fn parse_block(text: &str) -> Vec<Vec<String>> {
    let text = text.replace("\r\n", "\n");
    let text = text.strip_suffix('\n').unwrap_or(&text);
    let lines: Vec<&str> = text.split('\n').collect();
    if !text.contains('\t') {
        // Without tabs, quotes are just characters, and each line is a cell.
        return lines.iter().map(|line| vec![(*line).to_string()]).collect();
    }
    records(&lines, '\t').iter().map(|record| fields(&record_text(&lines, *record), '\t').into_iter().map(|f| f.value).collect()).collect()
}

/// What a set of values adds up to: how many there are, and of the numbers among them, the total, smallest and
/// largest.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Figures {
    pub values: usize,
    pub numbers: usize,
    pub sum: f64,
    pub smallest: f64,
    pub largest: f64,
}

impl Figures {
    pub fn of<'a>(values: impl IntoIterator<Item = &'a str>) -> Figures {
        let mut figures = Figures { smallest: f64::INFINITY, largest: f64::NEG_INFINITY, ..Figures::default() };
        for value in values {
            if value.trim().is_empty() {
                continue;
            }
            figures.values += 1;
            if let Some(n) = number(value) {
                figures.numbers += 1;
                figures.sum += n;
                figures.smallest = figures.smallest.min(n);
                figures.largest = figures.largest.max(n);
            }
        }
        figures
    }

    /// In words, for the bar under the table.
    pub fn describe(&self) -> String {
        if self.numbers == 0 {
            return format!("{} filled", self.values);
        }
        let show = |n: f64| {
            let text = format!("{:.6}", n);
            let text = text.trim_end_matches('0').trim_end_matches('.');
            if text == "-0" { "0".to_string() } else { text.to_string() }
        };
        format!("{} filled, {} numbers: total {}, average {}, smallest {}, largest {}", self.values, self.numbers, show(self.sum), show(self.sum / self.numbers as f64), show(self.smallest), show(self.largest))
    }
}

/// Splits a record's text back into lines.
pub fn lines_of(text: &str) -> Vec<String> {
    text.split('\n').map(str::to_string).collect()
}

/// Compares two values as people expect: as numbers if both are numbers, otherwise as words, ignoring case.
pub fn compare_values(a: &str, b: &str) -> Ordering {
    match (number(a), number(b)) {
        (Some(x), Some(y)) => x.partial_cmp(&y).unwrap_or(Ordering::Equal),
        // Numbers before words, and empty fields last.
        (Some(_), None) => Ordering::Less,
        (None, Some(_)) => Ordering::Greater,
        (None, None) => match (a.trim().is_empty(), b.trim().is_empty()) {
            (true, false) => Ordering::Greater,
            (false, true) => Ordering::Less,
            _ => a.to_lowercase().cmp(&b.to_lowercase()).then_with(|| a.cmp(b)),
        },
    }
}

/// A value read as a number, allowing thousands separators and a currency sign or percent.
pub fn number(value: &str) -> Option<f64> {
    let trimmed = value.trim().trim_start_matches(['$', '£', '€']).trim_end_matches('%');
    if trimmed.is_empty() || !trimmed.chars().any(|c| c.is_ascii_digit()) {
        return None;
    }
    trimmed.replace(',', "").parse().ok()
}

/// Works out whether lines are delimited data, and how. `ending` is the file's ending, if it has one, which
/// settles it for .csv, .tsv and the like even when the rows are uneven.
pub fn sniff<S: AsRef<str>>(lines: &[S], ending: Option<&str>) -> Option<Dialect> {
    let sample = &lines[..lines.len().min(200)];
    let ending = ending.map(str::to_ascii_lowercase);
    let named = match ending.as_deref() {
        Some("csv") => Some(','),
        Some("tsv" | "tab") => Some('\t'),
        Some("psv") => Some('|'),
        _ => None,
    };
    let mut best: Option<(char, f32, usize)> = None;
    for (_, delimiter) in DELIMITERS {
        let found = records(sample, delimiter);
        let counts: Vec<usize> = found.iter().map(|r| fields(&record_text(sample, *r), delimiter).len()).collect();
        if counts.len() < 2 && named != Some(delimiter) {
            continue;
        }
        // The usual number of fields, and how many records have it.
        let mut tally: Vec<(usize, usize)> = Vec::new();
        for &count in &counts {
            match tally.iter_mut().find(|(c, _)| *c == count) {
                Some(entry) => entry.1 += 1,
                None => tally.push((count, 1)),
            }
        }
        let Some(&(usual, times)) = tally.iter().max_by_key(|(count, times)| (*times, *count)) else {
            continue;
        };
        if usual < 2 {
            continue;
        }
        let steady = times as f32 / counts.len() as f32;
        let better = best.is_none_or(|(_, s, u)| steady > s + 0.001 || ((steady - s).abs() <= 0.001 && usual > u));
        if better {
            best = Some((delimiter, steady, usual));
        }
    }
    let delimiter = match (best, named) {
        // The ending says which; uneven rows don't change that.
        (Some((d, ..)), Some(n)) if d == n => n,
        (_, Some(n)) => n,
        (Some((d, steady, _)), None) if steady >= 0.9 => d,
        _ => return None,
    };
    Some(Dialect { delimiter, header: looks_like_header(sample, delimiter) })
}

/// Whether the first record names the columns: none of its fields is a number or empty, and some column below
/// it is mostly numbers, or its fields are all different and none of them appears again below.
fn looks_like_header<S: AsRef<str>>(lines: &[S], delimiter: char) -> bool {
    let found = records(lines, delimiter);
    let Some((first, rest)) = found.split_first() else {
        return false;
    };
    let names: Vec<String> = fields(&record_text(lines, *first), delimiter).into_iter().map(|f| f.value).collect();
    if names.iter().any(|n| n.trim().is_empty() || number(n).is_some()) {
        return false;
    }
    let below: Vec<Vec<String>> = rest.iter().take(50).map(|r| fields(&record_text(lines, *r), delimiter).into_iter().map(|f| f.value).collect()).collect();
    if below.is_empty() {
        return false;
    }
    let numeric_column = (0..names.len()).any(|c| {
        let values: Vec<&String> = below.iter().filter_map(|row| row.get(c)).filter(|v| !v.trim().is_empty()).collect();
        !values.is_empty() && values.iter().filter(|v| number(v).is_some()).count() * 10 >= values.len() * 8
    });
    let unique = names.iter().enumerate().all(|(i, n)| !names[..i].contains(n));
    let repeated_below = (0..names.len()).any(|c| below.iter().any(|row| row.get(c) == Some(&names[c])));
    numeric_column || (unique && !repeated_below && names.iter().all(|n| n.chars().any(char::is_alphabetic)))
}

/// The records of a document, worked out once for each version of its text.
#[derive(Clone, Debug, Default)]
pub struct Index {
    pub version: u64,
    pub delimiter: char,
    pub records: Arc<Vec<Record>>,
    /// The most fields any record has: the table's width.
    pub columns: usize,
}

impl Index {
    pub fn build<S: AsRef<str>>(lines: &[S], delimiter: char, version: u64) -> Self {
        let records = records(lines, delimiter);
        let columns = records.iter().map(|r| fields(&record_text(lines, *r), delimiter).len()).max().unwrap_or(0);
        Index { version, delimiter, records: Arc::new(records), columns }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn values(text: &str, delimiter: char) -> Vec<String> {
        fields(text, delimiter).into_iter().map(|f| f.value).collect()
    }

    #[test]
    fn fields_are_read_as_rfc_4180_writes_them() {
        assert_eq!(values("a,b,c", ','), ["a", "b", "c"]);
        assert_eq!(values("a,,c,", ','), ["a", "", "c", ""]);
        assert_eq!(values("\"x, y\",\"say \"\"hi\"\"\",z", ','), ["x, y", "say \"hi\"", "z"]);
        assert_eq!(values("5'10\",tall", ','), ["5'10\"", "tall"], "a quote inside an unquoted field is just a character");
        assert_eq!(values("\"two\nlines\",x", ','), ["two\nlines", "x"]);
        assert_eq!(values("a\tb c\td", '\t'), ["a", "b c", "d"]);
        assert_eq!(values("", ','), [""]);
        let f = fields("one,\"two\",three", ',');
        assert_eq!((f[1].start, f[1].end, f[1].quoted), (4, 9, true));
    }

    #[test]
    fn a_record_can_run_over_several_lines() {
        let lines = ["name,notes", "Ann,\"first line", "second line\"", "Bob,plain", ""];
        let found = records(&lines, ',');
        assert_eq!(found.iter().map(|r| (r.first, r.last)).collect::<Vec<_>>(), [(0, 0), (1, 2), (3, 3)], "the empty last line isn't a record");
        assert_eq!(values(&record_text(&lines, found[1]), ','), ["Ann", "first line\nsecond line"]);
        // A quote that's never closed keeps to its own line, and the rest still reads.
        let lines = ["a,b", "\"never closed,x", "c,d"];
        let found = records(&lines, ',');
        assert_eq!(found.len(), 3);
        assert!(found[1].broken && !found[2].broken);
    }

    #[test]
    fn editing_a_field_changes_only_that_field() {
        let text = " a , \"b\" ,c";
        assert_eq!(set_field(text, ',', 2, "new"), " a , \"b\" ,new");
        assert_eq!(set_field("a,\"b\",c", ',', 1, "plain"), "a,\"plain\",c", "a quoted field stays quoted");
        assert_eq!(set_field("a,b,c", ',', 1, "x, y"), "a,\"x, y\",c");
        assert_eq!(set_field("a,b,c", ',', 1, "say \"hi\""), "a,\"say \"\"hi\"\"\",c");
        assert_eq!(set_field("a,b", ',', 3, "d"), "a,b,,d", "past the end, empty fields fill the gap");
        assert_eq!(set_field("a,b", ',', 0, "two\nlines"), "\"two\nlines\",b");
        assert_eq!(lines_of(&set_field("a,b", ',', 0, "two\nlines")), ["\"two", "lines\",b"]);
    }

    #[test]
    fn columns_go_in_and_come_out() {
        assert_eq!(insert_field("a,b,c", ',', 1), "a,,b,c");
        assert_eq!(insert_field("a,b,c", ',', 3), "a,b,c,");
        assert_eq!(insert_field("a", ',', 2), "a,,");
        assert_eq!(remove_field("a,b,c", ',', 1), "a,c");
        assert_eq!(remove_field("a,b,c", ',', 2), "a,b");
        assert_eq!(remove_field("a,\"b,x\",c", ',', 0), "\"b,x\",c");
        assert_eq!(remove_field("only", ',', 0), "");
        assert_eq!(remove_field("a,b", ',', 5), "a,b");
        assert_eq!(empty_record('\t', 3), "\t\t");
    }

    #[test]
    fn delimited_data_is_recognised_and_plain_text_isnt() {
        let csv = ["id,name,price", "1,Apple,0.50", "2,\"Pear, green\",0.75", "3,Plum,1.20"];
        assert_eq!(sniff(&csv, None), Some(Dialect { delimiter: ',', header: true }));
        let tsv = ["x\ty\tz", "1\t2\t3", "4\t5\t6"];
        assert_eq!(sniff(&tsv, None).map(|d| d.delimiter), Some('\t'));
        let euro = ["Name;Wert;Datum", "A;1,5;2026-01-01", "B;2,5;2026-01-02"];
        assert_eq!(sniff(&euro, None).map(|d| d.delimiter), Some(';'), "commas in numbers don't fool it");
        let prose = ["This is a sentence, with a comma.", "Another line here.", "And one more, or two, here."];
        assert_eq!(sniff(&prose, None), None);
        assert_eq!(sniff(&prose, Some("CSV")).map(|d| d.delimiter), Some(','), "a .csv file is taken as one anyway");
        let no_header = ["1,2,3", "4,5,6"];
        assert_eq!(sniff(&no_header, None).map(|d| d.header), Some(false));
        let names = ["Ann,Smith,London", "Bob,Jones,Paris", "Cy,Lee,Rome"];
        assert_eq!(sniff(&names, None).map(|d| d.header), Some(true), "a guess either way; the table lets it be changed");
    }

    #[test]
    fn values_sort_as_people_expect() {
        let mut v = vec!["10", "9", "apple", "", "Banana", "1,000", "$5", "50%"];
        v.sort_by(|a, b| compare_values(a, b));
        assert_eq!(v, ["$5", "9", "10", "50%", "1,000", "apple", "Banana", ""]);
    }

    #[test]
    fn several_fields_change_at_once() {
        assert_eq!(set_fields("a,\"b\",c", ',', &[(0, "x".into()), (2, "y, z".into())]), "x,\"b\",\"y, z\"");
        assert_eq!(set_fields("a", ',', &[(2, "c".into())]), "a,,c");
    }

    #[test]
    fn blocks_go_to_and_from_the_clipboard_as_spreadsheets_have_them() {
        let cells = vec![vec!["a".to_string(), "b c".to_string()], vec!["two\nlines".to_string(), "tab\there".to_string()], vec!["say \"hi\"".to_string(), String::new()]];
        let text = block_text(&cells);
        assert_eq!(text, "a\tb c\n\"two\nlines\"\t\"tab\there\"\n\"say \"\"hi\"\"\"\t");
        assert_eq!(parse_block(&text), cells);
        // As Excel and LibreOffice copy: CRLF between rows, and one after the last.
        assert_eq!(parse_block("1\t2\r\n3\t4\r\n"), [vec!["1", "2"], vec!["3", "4"]]);
        // Plain lines are a column, quotes and all.
        assert_eq!(parse_block("x\n\"y\"\n"), [vec!["x"], vec!["\"y\""]]);
        assert_eq!(parse_block("one"), [vec!["one"]]);
    }

    #[test]
    fn figures_add_up_the_numbers_among_the_values() {
        let figures = Figures::of(["5.0", "", "4.27", "n/a", "$1,000", "-2"]);
        assert_eq!((figures.values, figures.numbers), (5, 4));
        assert!((figures.sum - 1007.27).abs() < 1e-9);
        assert_eq!((figures.smallest, figures.largest), (-2.0, 1000.0));
        assert_eq!(figures.describe(), "5 filled, 4 numbers: total 1007.27, average 251.8175, smallest -2, largest 1000");
        assert_eq!(Figures::of(["a", "b"]).describe(), "2 filled");
    }

    #[test]
    fn an_index_counts_the_widest_record() {
        let index = Index::build(&["a,b", "c,d,e", ""], ',', 7);
        assert_eq!((index.records.len(), index.columns, index.version), (2, 3, 7));
    }
}
