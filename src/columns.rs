//! Converting between delimited text and fixed-width columns. Nothing here draws anything or changes a file.

use crate::text_buffer::{block_cell, expand_tabs, next_visual, visual_len};

/// The delimiters CSV convert offers.
pub const DELIMITERS: [(&str, char); 4] = [("Comma", ','), ("Tab", '\t'), ("Semicolon", ';'), ("Pipe", '|')];

/// Splits one line into its fields, undoing the quoting. None if a quote is never closed:
/// that means a line break inside a field, which fixed-width text has no way to hold.
fn fields_of(line: &str, delimiter: char) -> Option<Vec<String>> {
    let mut fields = Vec::new();
    let mut chars = line.chars().peekable();
    loop {
        let mut field = String::new();
        let mut quoted = false;
        while chars.peek() == Some(&' ') {
            chars.next();
            field.push(' ');
        }
        if chars.peek() == Some(&'"') {
            chars.next();
            field.clear();
            quoted = true;
            loop {
                match chars.next()? {
                    '"' if chars.peek() == Some(&'"') => {
                        chars.next();
                        field.push('"');
                    }
                    '"' => break,
                    c => field.push(c),
                }
            }
        }
        let mut ended = true;
        for c in chars.by_ref() {
            if c == delimiter {
                ended = false;
                break;
            }
            field.push(c);
        }
        fields.push(if quoted { field } else { field.trim_matches(' ').to_string() });
        if ended {
            return Some(fields);
        }
    }
}

/// Which offered delimiter a row of text most likely uses. Comma if there's nothing to go on.
pub fn guess_delimiter(rows: &[String]) -> char {
    let Some(row) = rows.iter().find(|row| !row.trim().is_empty()) else {
        return ',';
    };
    let mut quoted = false;
    let mut counts = [0usize; DELIMITERS.len()];
    for c in row.chars() {
        if c == '"' {
            quoted = !quoted;
        } else if !quoted && let Some(place) = DELIMITERS.iter().position(|(_, delimiter)| *delimiter == c) {
            counts[place] += 1;
        }
    }
    let mut best = 0;
    for place in 1..counts.len() {
        if counts[place] > counts[best] {
            best = place;
        }
    }
    DELIMITERS[best].1
}

/// Lays delimited rows out in columns, each as wide as its widest value with one space between, so
/// that every row comes out the same length. Empty rows stay empty. A row whose quote never closes is
/// returned as an error, by its place in the list, and nothing is converted.
pub fn to_fixed(rows: &[String], delimiter: char) -> Result<Vec<String>, usize> {
    let mut parsed = Vec::with_capacity(rows.len());
    for (place, row) in rows.iter().enumerate() {
        if row.is_empty() {
            parsed.push(None);
            continue;
        }
        let fields = fields_of(row, delimiter).ok_or(place)?;
        parsed.push(Some(fields.iter().map(|field| expand_tabs(field)).collect::<Vec<_>>()));
    }
    let mut widths: Vec<usize> = Vec::new();
    for fields in parsed.iter().flatten() {
        if fields.len() > widths.len() {
            widths.resize(fields.len(), 0);
        }
        for (i, field) in fields.iter().enumerate() {
            widths[i] = widths[i].max(visual_len(field));
        }
    }
    Ok(parsed
        .iter()
        .map(|fields| {
            let Some(fields) = fields else {
                return String::new();
            };
            let mut out = String::new();
            for (i, width) in widths.iter().enumerate() {
                if i > 0 {
                    out.push(' ');
                }
                let field = fields.get(i).map_or("", String::as_str);
                out.push_str(field);
                out.extend(std::iter::repeat_n(' ', width - visual_len(field)));
            }
            out
        })
        .collect())
}

/// Where the columns of fixed-width rows start, counting from 0. A column starts wherever text
/// follows a position that's blank on every row.
pub fn detect_starts(rows: &[String]) -> Vec<usize> {
    let mut used: Vec<bool> = Vec::new();
    for row in rows {
        let mut v = 0;
        for c in row.chars() {
            let next = next_visual(v, c);
            if c != ' ' && c != '\t' {
                if used.len() < next {
                    used.resize(next, false);
                }
                used[v..next].fill(true);
            }
            v = next;
        }
    }
    let mut starts = vec![0];
    for col in 1..used.len() {
        if used[col] && !used[col - 1] && used[..col].iter().any(|on| *on) {
            starts.push(col);
        }
    }
    starts
}

/// Column starts as the dialog shows them, counting from 1 as the status bar does.
pub fn starts_text(starts: &[usize]) -> String {
    starts.iter().map(|start| (start + 1).to_string()).collect::<Vec<_>>().join(", ")
}

/// Reads column starts typed in the dialog. The first column always starts at 1, so nothing is left out.
pub fn parse_starts(text: &str) -> Result<Vec<usize>, String> {
    const PROBLEM: &str = "Enter the columns as rising numbers, such as 1, 11, 26.";
    let mut starts = vec![0];
    for part in text.split([',', ' ']).filter(|part| !part.is_empty()) {
        let number: usize = part.parse().map_err(|_| PROBLEM.to_string())?;
        if number == 0 {
            return Err(PROBLEM.to_string());
        }
        let start = number - 1;
        if start == 0 && starts.len() == 1 {
            continue;
        }
        if start <= starts[starts.len() - 1] {
            return Err(PROBLEM.to_string());
        }
        starts.push(start);
    }
    Ok(starts)
}

fn quoted_if_needed(field: &str, delimiter: char) -> String {
    if field.contains(delimiter) || field.contains('"') {
        format!("\"{}\"", field.replace('"', "\"\""))
    } else {
        field.to_string()
    }
}

/// Turns fixed-width rows into delimited ones, trimming the padding from each value. Blank rows come out empty.
pub fn to_csv(rows: &[String], starts: &[usize], delimiter: char) -> Vec<String> {
    let separator = delimiter.to_string();
    rows.iter()
        .map(|row| {
            if row.trim().is_empty() {
                return String::new();
            }
            let end = visual_len(row);
            let mut fields = Vec::with_capacity(starts.len());
            for (i, left) in starts.iter().enumerate() {
                let right = starts.get(i + 1).copied().unwrap_or(end).max(*left);
                let cell = block_cell(row, *left, right);
                fields.push(quoted_if_needed(cell.trim_matches(' '), delimiter));
            }
            fields.join(&separator)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rows(lines: &[&str]) -> Vec<String> {
        lines.iter().map(|line| line.to_string()).collect()
    }

    #[test]
    fn fields_follow_the_quoting_rules() {
        assert_eq!(fields_of("a,b,,c", ',').unwrap(), ["a", "b", "", "c"]);
        assert_eq!(fields_of("\"a,b\",\"say \"\"hi\"\"\"", ',').unwrap(), ["a,b", "say \"hi\""]);
        assert_eq!(fields_of("", ',').unwrap(), [""]);
        assert_eq!(fields_of("a,", ',').unwrap(), ["a", ""]);
        assert_eq!(fields_of(" a , b ", ',').unwrap(), ["a", "b"], "padding around a plain value isn't part of it");
        assert_eq!(fields_of("\" a \",b", ',').unwrap(), [" a ", "b"], "spaces in quotes are");
        assert_eq!(fields_of("  \"a\"  ,b", ',').unwrap(), ["a  ", "b"], "text after a closing quote is kept, not lost");
        assert_eq!(fields_of("5\" disk,b", ',').unwrap(), ["5\" disk", "b"], "a quote inside a plain value is just a character");
        assert_eq!(fields_of("\"never closed,b", ','), None);
    }

    #[test]
    fn the_delimiter_is_guessed_from_the_first_row_with_text() {
        assert_eq!(guess_delimiter(&rows(&["", "a;b;c"])), ';');
        assert_eq!(guess_delimiter(&rows(&["a\tb"])), '\t');
        assert_eq!(guess_delimiter(&rows(&["a|b|c,d"])), '|');
        assert_eq!(guess_delimiter(&rows(&["\"a;b;c\",d"])), ',', "delimiters inside quotes don't count");
        assert_eq!(guess_delimiter(&rows(&["plain"])), ',');
        assert_eq!(guess_delimiter(&[]), ',');
    }

    #[test]
    fn delimited_rows_become_columns_of_equal_length() {
        let fixed = to_fixed(&rows(&["name,qty,city", "Bo,12,\"Paris, FR\"", "", "Alexandra,3"]), ',').unwrap();
        assert_eq!(fixed, ["name      qty city     ", "Bo        12  Paris, FR", "", "Alexandra 3            "]);
        assert!(fixed.iter().filter(|row| !row.is_empty()).all(|row| row.len() == 23));
    }

    #[test]
    fn an_unclosed_quote_converts_nothing_and_says_which_row() {
        assert_eq!(to_fixed(&rows(&["a,b", "c,\"d"]), ','), Err(1));
    }

    #[test]
    fn tabs_inside_a_value_become_spaces_so_the_columns_line_up() {
        let fixed = to_fixed(&rows(&["\"a\tb\",c", "x,y"]), ',').unwrap();
        assert_eq!(visual_len(&fixed[0]), visual_len(&fixed[1]));
        assert!(!fixed[0].contains('\t'));
    }

    #[test]
    fn column_starts_are_found_from_the_gaps_common_to_every_row() {
        let fixed = rows(&["name      qty city", "Bo        12  Paris", "", "Alexandra 3"]);
        assert_eq!(detect_starts(&fixed), [0, 10, 14]);
        assert_eq!(starts_text(&[0, 10, 14]), "1, 11, 15");
    }

    #[test]
    fn a_space_in_a_value_only_splits_it_if_every_row_has_a_gap_there() {
        assert_eq!(detect_starts(&rows(&["New York 1", "Paris    2"])), [0, 9]);
        assert_eq!(detect_starts(&rows(&["New York"])), [0, 4], "with one row there's nothing else to go on, which is why the dialog lets you change it");
    }

    #[test]
    fn indented_columns_still_start_at_one_and_empty_text_is_safe() {
        assert_eq!(detect_starts(&rows(&["   a  b"])), [0, 6]);
        assert_eq!(detect_starts(&rows(&["", "   "])), [0]);
        assert_eq!(detect_starts(&[]), [0]);
    }

    #[test]
    fn typed_column_starts_are_checked() {
        assert_eq!(parse_starts("1, 11, 15").unwrap(), [0, 10, 14]);
        assert_eq!(parse_starts("11 15").unwrap(), [0, 10, 14], "column 1 is always included");
        assert_eq!(parse_starts("").unwrap(), [0]);
        assert!(parse_starts("0, 5").is_err());
        assert!(parse_starts("5, 3").is_err());
        assert!(parse_starts("1, 5, 5").is_err());
        assert!(parse_starts("a").is_err());
        assert!(parse_starts("-3").is_err());
        assert!(parse_starts("99999999999999999999999").is_err());
    }

    #[test]
    fn fixed_rows_become_delimited_and_quoted_where_they_must_be() {
        let csv = to_csv(&rows(&["Bo   12 a,b", "Alex  3 say \"x\"", "", "   ", "Z"]), &[0, 5, 8], ',');
        assert_eq!(csv, ["Bo,12,\"a,b\"", "Alex,3,\"say \"\"x\"\"\"", "", "", "Z,,"]);
    }

    #[test]
    fn tabs_count_as_the_columns_they_fill() {
        crate::text_buffer::set_tab_size(8);
        assert_eq!(to_csv(&rows(&["a\tb"]), &[0, 8], ';'), ["a;b"]);
    }

    #[test]
    fn converting_there_and_back_keeps_every_value() {
        let original = rows(&["name,qty,city", "Bo,12,\"Paris,FR\"", "Alexandra,3,Rome"]);
        let fixed = to_fixed(&original, ',').unwrap();
        let back = to_csv(&fixed, &detect_starts(&fixed), ',');
        assert_eq!(back, original);
    }

    #[test]
    fn other_delimiters_work_both_ways() {
        let original = rows(&["a|b c", "dd|e"]);
        let fixed = to_fixed(&original, '|').unwrap();
        assert_eq!(fixed, ["a  b c", "dd e  "]);
        assert_eq!(to_csv(&fixed, &[0, 3], '|'), original);
    }
}
