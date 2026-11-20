//! Markdown helpers, as Markdown All in One and markdownlint have them: lists carried on as Enter is pressed, a
//! table of contents made and kept up to date between its markers, a table's columns lined up, and the usual
//! problems pointed out.

/// What Enter does at the end of a list item: carry the list on with this before the new line's text, or, on an
/// item with nothing in it, end the list by taking its marker off.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Enter {
    Continue(String),
    EndList(String),
}

/// A list item's parts: its indent, marker (with a task box), and text.
fn list_item(line: &str) -> Option<(String, String, String)> {
    let indent: String = line.chars().take_while(|c| *c == ' ' || *c == '\t').collect();
    let rest = &line[indent.len()..];
    let marker_end = if rest.starts_with(['-', '*', '+']) {
        1
    } else {
        let digits = rest.chars().take_while(char::is_ascii_digit).count();
        if digits == 0 || digits > 9 || !rest[digits..].starts_with(['.', ')']) {
            return None;
        }
        digits + 1
    };
    let after = &rest[marker_end..];
    if !after.starts_with(' ') && !after.is_empty() {
        return None;
    }
    let mut marker = rest[..marker_end].to_string();
    let mut text = after.trim_start().to_string();
    // A task's box: carried on empty.
    for task in ["[ ] ", "[x] ", "[X] "] {
        if let Some(rest) = text.strip_prefix(task) {
            marker.push_str(" [ ]");
            text = rest.to_string();
        } else if text == task.trim_end() {
            marker.push_str(" [ ]");
            text.clear();
        }
    }
    Some((indent, marker, text))
}

/// What Enter at the end of this line does, if it's a list item.
pub fn on_enter(line: &str) -> Option<Enter> {
    let (indent, marker, text) = list_item(line)?;
    if text.trim().is_empty() {
        return Some(Enter::EndList(indent));
    }
    let (first, task) = marker.split_once(' ').map_or((marker.as_str(), ""), |(m, t)| (m, t));
    let next = if let Some(number) = first.strip_suffix(['.', ')']).and_then(|n| n.parse::<u64>().ok()) {
        format!("{}{}", number + 1, &first[first.len() - 1..])
    } else {
        first.to_string()
    };
    let task = if task.is_empty() { String::new() } else { format!(" {task}") };
    Some(Enter::Continue(format!("{indent}{next}{task} ")))
}

/// The marker lines a table of contents sits between.
pub const TOC_START: &str = "<!-- toc -->";
pub const TOC_END: &str = "<!-- /toc -->";

/// The headings (level, text, line), outside fenced code, both # and underlined kinds.
pub fn headings<S: AsRef<str>>(lines: &[S]) -> Vec<(usize, String, usize)> {
    let mut found = Vec::new();
    let mut fence: Option<String> = None;
    for (number, line) in lines.iter().enumerate() {
        let line = line.as_ref();
        let trimmed = line.trim_start();
        if trimmed.starts_with("```") || trimmed.starts_with("~~~") {
            let mark: String = trimmed.chars().take(3).collect();
            fence = if fence.as_deref() == Some(mark.as_str()) { None } else if fence.is_none() { Some(mark) } else { fence };
            continue;
        }
        if fence.is_some() {
            continue;
        }
        let hashes = trimmed.chars().take_while(|c| *c == '#').count();
        if (1..=6).contains(&hashes) && trimmed[hashes..].starts_with(' ') && line.len() - trimmed.len() < 4 {
            found.push((hashes, trimmed[hashes..].trim().trim_end_matches('#').trim().to_string(), number));
        } else if number > 0 && !line.trim().is_empty() && line.trim().chars().all(|c| c == '=') && !lines[number - 1].as_ref().trim().is_empty() {
            found.push((1, lines[number - 1].as_ref().trim().to_string(), number - 1));
        } else if number > 0 && line.trim().len() >= 2 && line.trim().chars().all(|c| c == '-') && !lines[number - 1].as_ref().trim().is_empty() && list_item(lines[number - 1].as_ref()).is_none() {
            found.push((2, lines[number - 1].as_ref().trim().to_string(), number - 1));
        }
    }
    found
}

/// A heading's link, as GitHub makes it: lower case, punctuation gone, spaces as hyphens.
pub fn anchor(heading: &str) -> String {
    heading
        .to_lowercase()
        .chars()
        .filter_map(|c| if c.is_alphanumeric() || c == '-' || c == '_' { Some(c) } else if c == ' ' { Some('-') } else { None })
        .collect()
}

/// A table of contents of the headings below the first, as a nested list of links.
pub fn table_of_contents<S: AsRef<str>>(lines: &[S]) -> Vec<String> {
    let found = headings(lines);
    let top = found.iter().map(|(level, ..)| *level).filter(|l| *l > 1 || found.iter().filter(|(l2, ..)| *l2 == 1).count() > 1).min().unwrap_or(1);
    let mut used: std::collections::HashMap<String, usize> = std::collections::HashMap::new();
    let mut out = Vec::new();
    for (level, text, _) in found.iter().filter(|(level, ..)| *level >= top) {
        let base = anchor(text);
        let count = used.entry(base.clone()).or_insert(0);
        let link = if *count == 0 { base.clone() } else { format!("{base}-{count}") };
        *count += 1;
        out.push(format!("{}- [{}](#{link})", "  ".repeat(level - top), text.replace(['[', ']'], "")));
    }
    out
}

/// The lines with their table of contents put in afresh between its markers; None if there are no markers.
pub fn refresh_toc<S: AsRef<str>>(lines: &[S]) -> Option<(usize, usize, Vec<String>)> {
    let start = lines.iter().position(|l| l.as_ref().trim() == TOC_START)?;
    let end = start + lines[start..].iter().position(|l| l.as_ref().trim() == TOC_END)?;
    let mut rows = vec![TOC_START.to_string()];
    rows.extend(table_of_contents(lines));
    rows.push(TOC_END.to_string());
    Some((start, end, rows))
}

fn cells(line: &str) -> Vec<String> {
    let trimmed = line.trim();
    let inner = trimmed.strip_prefix('|').unwrap_or(trimmed);
    let inner = inner.strip_suffix('|').unwrap_or(inner);
    // Pipes escaped with a backslash stay in their cell.
    let mut out = Vec::new();
    let mut current = String::new();
    let mut escaped = false;
    for c in inner.chars() {
        if c == '|' && !escaped {
            out.push(current.trim().to_string());
            current.clear();
        } else {
            current.push(c);
        }
        escaped = c == '\\' && !escaped;
    }
    out.push(current.trim().to_string());
    out
}

/// A table's rows lined up, each column as wide as its widest cell and aligned as its separator says.
pub fn line_up_table<S: AsRef<str>>(rows: &[S]) -> Option<Vec<String>> {
    let table: Vec<Vec<String>> = rows.iter().map(|r| cells(r.as_ref())).collect();
    let separator = table.get(1)?;
    if !separator.iter().all(|c| !c.is_empty() && c.chars().all(|ch| matches!(ch, '-' | ':'))) {
        return None;
    }
    let columns = table.iter().map(Vec::len).max()?;
    let align: Vec<(bool, bool)> = (0..columns).map(|i| separator.get(i).map_or((false, false), |c| (c.starts_with(':'), c.ends_with(':')))).collect();
    let width = |i: usize| table.iter().enumerate().filter(|(n, _)| *n != 1).map(|(_, row)| row.get(i).map_or(0, |c| c.chars().count())).max().unwrap_or(0).max(3);
    let widths: Vec<usize> = (0..columns).map(width).collect();
    let mut out = Vec::new();
    for (n, row) in table.iter().enumerate() {
        let parts: Vec<String> = (0..columns)
            .map(|i| {
                let w = widths[i];
                if n == 1 {
                    let (left, right) = align[i];
                    let dashes = w - usize::from(left) - usize::from(right);
                    return format!("{}{}{}", if left { ":" } else { "" }, "-".repeat(dashes), if right { ":" } else { "" });
                }
                let cell = row.get(i).map_or("", String::as_str);
                let pad = w - cell.chars().count();
                match align[i] {
                    (true, true) => format!("{}{cell}{}", " ".repeat(pad / 2), " ".repeat(pad - pad / 2)),
                    (false, true) => format!("{}{cell}", " ".repeat(pad)),
                    _ => format!("{cell}{}", " ".repeat(pad)),
                }
            })
            .collect();
        out.push(format!("| {} |", parts.join(" | ")));
    }
    Some(out)
}

/// The table the line is in: its first and last lines.
pub fn table_around<S: AsRef<str>>(lines: &[S], line: usize) -> Option<(usize, usize)> {
    let is_row = |l: &str| l.trim_start().starts_with('|') || (l.contains('|') && !l.trim().is_empty());
    if !is_row(lines.get(line)?.as_ref()) {
        return None;
    }
    let mut top = line;
    while top > 0 && is_row(lines[top - 1].as_ref()) {
        top -= 1;
    }
    let mut bottom = line;
    while bottom + 1 < lines.len() && is_row(lines[bottom + 1].as_ref()) {
        bottom += 1;
    }
    (bottom > top).then_some((top, bottom))
}

/// The problems markdownlint points out most: headings that skip a level or have no space after their #s, more
/// than one top heading, headings and fenced code without a blank line around them, blank lines doubled, trailing
/// spaces other than a line break's two, and hard tabs. Each with its rule's number.
pub fn problems<S: AsRef<str>>(lines: &[S]) -> Vec<(usize, String)> {
    let mut found = Vec::new();
    let mut last_level = 0;
    let mut top_headings = 0;
    let mut fence = false;
    let blank = |n: usize| lines.get(n).is_none_or(|l| l.as_ref().trim().is_empty());
    for (number, line) in lines.iter().enumerate() {
        let line = line.as_ref();
        let trimmed = line.trim_start();
        if trimmed.starts_with("```") || trimmed.starts_with("~~~") {
            if !fence && number > 0 && !blank(number - 1) {
                found.push((number, crate::i18n::tr("MD031: put a blank line before fenced code").to_string()));
            }
            if fence && !blank(number + 1) {
                found.push((number, crate::i18n::tr("MD031: put a blank line after fenced code").to_string()));
            }
            fence = !fence;
            continue;
        }
        if fence {
            continue;
        }
        let hashes = trimmed.chars().take_while(|c| *c == '#').count();
        if (1..=6).contains(&hashes) && line.len() - trimmed.len() < 4 {
            if !trimmed[hashes..].starts_with(' ') && !trimmed[hashes..].is_empty() {
                found.push((number, crate::i18n::tr("MD018: put a space after the # of a heading").to_string()));
            } else {
                if last_level > 0 && hashes > last_level + 1 {
                    found.push((number, crate::i18n::tf!("MD001: a heading of level {hashes} straight after one of level {last_level}; headings go down one level at a time")));
                }
                last_level = hashes;
                if hashes == 1 {
                    top_headings += 1;
                    if top_headings > 1 {
                        found.push((number, crate::i18n::tr("MD025: more than one top-level heading").to_string()));
                    }
                }
                if (number > 0 && !blank(number - 1)) || !blank(number + 1) {
                    found.push((number, crate::i18n::tr("MD022: put a blank line above and below a heading").to_string()));
                }
            }
        }
        if number > 0 && line.trim().is_empty() && blank(number - 1) && number + 1 < lines.len() {
            found.push((number, crate::i18n::tr("MD012: more than one blank line together").to_string()));
        }
        let trailing = line.len() - line.trim_end_matches(' ').len();
        if trailing > 0 && trailing != 2 && !line.trim().is_empty() {
            found.push((number, crate::i18n::tr("MD009: spaces at the end of the line (two make a line break; others do nothing)").to_string()));
        }
        if line.contains('\t') {
            found.push((number, crate::i18n::tr("MD010: a tab; use spaces").to_string()));
        }
    }
    found
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lists_carry_on_and_end_on_an_empty_item() {
        assert_eq!(on_enter("- milk"), Some(Enter::Continue("- ".into())));
        assert_eq!(on_enter("  9. ninth"), Some(Enter::Continue("  10. ".into())));
        assert_eq!(on_enter("* [x] done"), Some(Enter::Continue("* [ ] ".into())));
        assert_eq!(on_enter("  - "), Some(Enter::EndList("  ".into())));
        assert_eq!(on_enter("plain text"), None);
        assert_eq!(on_enter("-not a list"), None);
    }

    #[test]
    fn a_table_of_contents_links_each_heading() {
        let doc = ["# Title", "", "<!-- toc -->", "<!-- /toc -->", "", "## Getting started", "```", "# not a heading", "```", "### Step 1: install!", "## Getting started", "Usage", "-----"];
        assert_eq!(table_of_contents(&doc), ["- [Getting started](#getting-started)", "  - [Step 1: install!](#step-1-install)", "- [Getting started](#getting-started-1)", "- [Usage](#usage)"]);
        let (start, end, rows) = refresh_toc(&doc).unwrap();
        assert_eq!((start, end, rows.len()), (2, 3, 6));
    }

    #[test]
    fn tables_are_lined_up_as_their_separator_says() {
        let table = ["|Name|Size|", "|:-|-:|", "|ship.bin|1024|", "|a|9|"];
        assert_eq!(line_up_table(&table).unwrap(), ["| Name     | Size |", "| :------- | ---: |", "| ship.bin | 1024 |", "| a        |    9 |"]);
        assert_eq!(table_around(&["text", "|a|b|", "|-|-|", "|1|2|", ""], 2), Some((1, 3)));
    }

    #[test]
    fn the_usual_problems_are_pointed_out() {
        let doc = ["# One", "#Two", "", "### Three", "text ", "", "", "# Again", "```", "code", "```", "after"];
        let rules: Vec<(usize, String)> = problems(&doc).into_iter().map(|(n, words)| (n, words[..5].to_string())).collect();
        assert!(rules.contains(&(1, "MD018".into())));
        assert!(rules.contains(&(3, "MD001".into())) || rules.contains(&(3, "MD022".into())));
        assert!(rules.contains(&(4, "MD009".into())));
        assert!(rules.contains(&(6, "MD012".into())));
        assert!(rules.contains(&(7, "MD025".into())));
        assert!(rules.contains(&(10, "MD031".into())));
        assert!(problems(&["# Fine", "", "Text with a break  ", "more"]).is_empty());
    }
}
