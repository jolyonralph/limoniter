//! What a notebook cell's outputs are made into, apart from drawing them: terminal colours (ANSI) in text,
//! carriage returns as a terminal treats them, which of an output's many forms to show, and a pandas table's HTML
//! read into rows and columns. Nothing here draws or needs a window.

use eframe::egui::Color32;
use serde_json::Value;

/// A stretch of text in one style, from ANSI colour codes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Run {
    pub text: String,
    pub colour: Option<Color32>,
    pub background: Option<Color32>,
    pub bold: bool,
    pub underline: bool,
}

/// The eight normal and eight bright terminal colours, in tones that read on a dark page or a light one.
fn palette(dark: bool) -> [Color32; 16] {
    let c = |r, g, b| Color32::from_rgb(r, g, b);
    if dark {
        [c(0x55, 0x55, 0x55), c(0xE0, 0x5A, 0x5A), c(0x4C, 0xC0, 0x6A), c(0xD9, 0xA8, 0x3A), c(0x5C, 0x8D, 0xE8), c(0xC0, 0x6A, 0xD8), c(0x3A, 0xC0, 0xC0), c(0xC8, 0xC8, 0xC8), c(0x80, 0x80, 0x80), c(0xFF, 0x7A, 0x7A), c(0x6C, 0xE0, 0x8A), c(0xF2, 0xC8, 0x5A), c(0x7C, 0xAD, 0xFF), c(0xE0, 0x8A, 0xF0), c(0x5A, 0xE0, 0xE0), c(0xF0, 0xF0, 0xF0)]
    } else {
        [c(0x30, 0x30, 0x30), c(0xB5, 0x1F, 0x1F), c(0x1F, 0x7A, 0x32), c(0x8A, 0x60, 0x00), c(0x1F, 0x4F, 0xB5), c(0x8A, 0x2A, 0xA8), c(0x0F, 0x7A, 0x80), c(0x70, 0x70, 0x70), c(0x55, 0x55, 0x55), c(0xD0, 0x30, 0x30), c(0x2A, 0x90, 0x44), c(0xA0, 0x70, 0x10), c(0x30, 0x68, 0xD0), c(0xA0, 0x40, 0xC0), c(0x20, 0x90, 0x98), c(0x30, 0x30, 0x30)]
    }
}

/// The colour of one of the 256 colours of xterm.
fn xterm(n: u32, dark: bool) -> Color32 {
    match n {
        0..=15 => palette(dark)[n as usize],
        16..=231 => {
            let n = n - 16;
            let level = |i: u32| if i == 0 { 0 } else { (55 + 40 * i) as u8 };
            Color32::from_rgb(level(n / 36), level((n / 6) % 6), level(n % 6))
        }
        _ => {
            let grey = (8 + 10 * (n.min(255) - 232)) as u8;
            Color32::from_gray(grey)
        }
    }
}

/// Text with ANSI escape codes as runs of styled text. Colour, bold and underline are kept; every other escape
/// sequence (moving the cursor, clearing the line, the title) is dropped.
pub fn ansi_runs(text: &str, dark: bool) -> Vec<Run> {
    let mut runs: Vec<Run> = Vec::new();
    let (mut colour, mut background, mut bold, mut underline) = (None, None, false, false);
    let mut current = String::new();
    let flush = |current: &mut String, runs: &mut Vec<Run>, colour: Option<Color32>, background: Option<Color32>, bold: bool, underline: bool| {
        if !current.is_empty() {
            runs.push(Run { text: std::mem::take(current), colour, background, bold, underline });
        }
    };
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        if c != '\u{1b}' {
            current.push(c);
            continue;
        }
        match chars.peek() {
            Some('[') => {
                chars.next();
                let mut body = String::new();
                let mut last = None;
                for next in chars.by_ref() {
                    if ('\u{40}'..='\u{7e}').contains(&next) {
                        last = Some(next);
                        break;
                    }
                    body.push(next);
                }
                if last != Some('m') {
                    continue;
                }
                flush(&mut current, &mut runs, colour, background, bold, underline);
                let codes: Vec<u32> = if body.is_empty() { vec![0] } else { body.split(';').map(|p| p.parse().unwrap_or(0)).collect() };
                let mut i = 0;
                while i < codes.len() {
                    match codes[i] {
                        0 => (colour, background, bold, underline) = (None, None, false, false),
                        1 => bold = true,
                        4 => underline = true,
                        22 => bold = false,
                        24 => underline = false,
                        n @ 30..=37 => colour = Some(palette(dark)[(n - 30) as usize]),
                        n @ 90..=97 => colour = Some(palette(dark)[(n - 90 + 8) as usize]),
                        n @ 40..=47 => background = Some(palette(dark)[(n - 40) as usize]),
                        n @ 100..=107 => background = Some(palette(dark)[(n - 100 + 8) as usize]),
                        39 => colour = None,
                        49 => background = None,
                        n @ (38 | 48) => {
                            let target = if n == 38 { &mut colour } else { &mut background };
                            match codes.get(i + 1) {
                                Some(5) if i + 2 < codes.len() => {
                                    *target = Some(xterm(codes[i + 2], dark));
                                    i += 2;
                                }
                                Some(2) if i + 4 < codes.len() => {
                                    *target = Some(Color32::from_rgb(codes[i + 2].min(255) as u8, codes[i + 3].min(255) as u8, codes[i + 4].min(255) as u8));
                                    i += 4;
                                }
                                _ => {}
                            }
                        }
                        _ => {}
                    }
                    i += 1;
                }
            }
            // A title or other operating system command, up to the bell or the string terminator.
            Some(']') => {
                chars.next();
                while let Some(next) = chars.next() {
                    if next == '\u{7}' || (next == '\u{1b}' && chars.peek() == Some(&'\\')) {
                        if next != '\u{7}' {
                            chars.next();
                        }
                        break;
                    }
                }
            }
            Some(_) => {
                chars.next();
            }
            None => {}
        }
    }
    flush(&mut current, &mut runs, colour, background, bold, underline);
    runs
}

/// The text with its ANSI escapes taken out.
pub fn strip_ansi(text: &str) -> String {
    ansi_runs(text, true).into_iter().map(|r| r.text).collect()
}

/// Text as a terminal would show it with carriage returns: a `\r` moves back to the start of the line and what
/// follows writes over what was there, which is how progress bars update in place. `\r\n` is just a line end.
pub fn settle_carriage_returns(text: &str) -> String {
    if !text.contains('\r') {
        return text.to_string();
    }
    let mut out = String::with_capacity(text.len());
    for (n, line) in text.split('\n').enumerate() {
        if n > 0 {
            out.push('\n');
        }
        let line = line.strip_suffix('\r').unwrap_or(line);
        let mut shown: Vec<char> = Vec::new();
        // Every piece is written from the start of the line, over what the pieces before it left.
        for part in line.split('\r') {
            for (at, c) in part.chars().enumerate() {
                if at < shown.len() {
                    shown[at] = c;
                } else {
                    shown.push(c);
                }
            }
        }
        out.extend(shown);
    }
    out
}

/// Pieces of text with a mime type, or the lines of it (a notebook may hold either).
pub fn text_of(value: &Value) -> String {
    match value {
        Value::String(text) => text.clone(),
        Value::Array(lines) => lines.iter().filter_map(Value::as_str).collect(),
        _ => String::new(),
    }
}

/// A table read from HTML: rows of cells, each with whether it is a heading cell.
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct Table {
    pub rows: Vec<Vec<(String, bool)>>,
}

/// What to show for one output.
#[derive(Clone, Debug, PartialEq)]
pub enum Shown {
    /// A stream: its name ("stdout", "stderr") and text.
    Stream { name: String, text: String },
    Text(String),
    Png(Vec<u8>),
    Jpeg(Vec<u8>),
    Svg(String),
    Markdown(String),
    Table { table: Table, before_and_after: String },
    /// An error's name, its message and the traceback (with ANSI colours in it).
    Error { name: String, message: String, traceback: String },
    /// Nothing to show for it, and the kinds it did have.
    Nothing(Vec<String>),
}

/// Which form of an output to show. A picture first, then a table in HTML, then Markdown, then plain text, and
/// other HTML as the text in it.
pub fn choose(output: &Value) -> Shown {
    match output.get("output_type").and_then(Value::as_str) {
        Some("stream") => Shown::Stream { name: output.get("name").and_then(Value::as_str).unwrap_or("stdout").to_string(), text: text_of(output.get("text").unwrap_or(&Value::Null)) },
        Some("error") => {
            let traceback = match output.get("traceback") {
                Some(Value::Array(lines)) => lines.iter().filter_map(Value::as_str).collect::<Vec<_>>().join("\n"),
                Some(other) => text_of(other),
                None => String::new(),
            };
            Shown::Error { name: text_of(output.get("ename").unwrap_or(&Value::Null)), message: text_of(output.get("evalue").unwrap_or(&Value::Null)), traceback }
        }
        Some("display_data" | "execute_result") => {
            let Some(data) = output.get("data").and_then(Value::as_object) else {
                return Shown::Nothing(Vec::new());
            };
            let get = |mime: &str| data.get(mime).map(text_of);
            for (mime, jpeg) in [("image/png", false), ("image/jpeg", true)] {
                if let Some(bytes) = get(mime).and_then(|b| decode_base64(&b)) {
                    return if jpeg { Shown::Jpeg(bytes) } else { Shown::Png(bytes) };
                }
            }
            if let Some(svg) = get("image/svg+xml") {
                return Shown::Svg(svg);
            }
            if let Some(html) = get("text/html")
                && let Some((table, around)) = html_table(&html)
            {
                return Shown::Table { table, before_and_after: around };
            }
            if let Some(markdown) = get("text/markdown") {
                return Shown::Markdown(markdown);
            }
            if let Some(text) = get("text/plain") {
                return Shown::Text(text);
            }
            if let Some(html) = get("text/html") {
                return Shown::Text(html_to_text(&html));
            }
            if let Some(latex) = get("text/latex") {
                return Shown::Text(latex);
            }
            Shown::Nothing(data.keys().cloned().collect())
        }
        _ => Shown::Nothing(Vec::new()),
    }
}

/// The text of an output with no pictures and no colours, for comparing notebooks and for copying.
pub fn plain_text_of(output: &Value) -> String {
    match choose(output) {
        Shown::Stream { text, .. } => strip_ansi(&settle_carriage_returns(&text)),
        Shown::Text(text) | Shown::Markdown(text) => text,
        Shown::Svg(_) => "(a drawing)".to_string(),
        Shown::Png(_) | Shown::Jpeg(_) => "(a picture)".to_string(),
        Shown::Table { table, .. } => table.rows.iter().map(|row| row.iter().map(|(text, _)| text.as_str()).collect::<Vec<_>>().join("\t")).collect::<Vec<_>>().join("\n"),
        Shown::Error { name, message, traceback } => {
            if traceback.is_empty() {
                format!("{name}: {message}")
            } else {
                strip_ansi(&traceback)
            }
        }
        Shown::Nothing(_) => String::new(),
    }
}

/// Adds stream text to the outputs, joining it to the stream before it if that is the same one (stdout or stderr),
/// as Jupyter does.
pub fn add_stream(outputs: &mut Vec<Value>, name: &str, text: &str) {
    if let Some(last) = outputs.last_mut()
        && last.get("output_type").and_then(Value::as_str) == Some("stream")
        && last.get("name").and_then(Value::as_str) == Some(name)
    {
        let mut joined = text_of(last.get("text").unwrap_or(&Value::Null));
        joined.push_str(text);
        last["text"] = Value::String(joined);
        return;
    }
    outputs.push(serde_json::json!({"output_type": "stream", "name": name, "text": text}));
}

// ---- base64, for pictures: the standard alphabet, with line breaks and padding allowed ----

pub fn decode_base64(text: &str) -> Option<Vec<u8>> {
    let mut out = Vec::with_capacity(text.len() / 4 * 3);
    let (mut bits, mut count) = (0u32, 0);
    for c in text.bytes() {
        let value = match c {
            b'A'..=b'Z' => c - b'A',
            b'a'..=b'z' => c - b'a' + 26,
            b'0'..=b'9' => c - b'0' + 52,
            b'+' | b'-' => 62,
            b'/' | b'_' => 63,
            b'=' => break,
            b'\n' | b'\r' | b' ' | b'\t' => continue,
            _ => return None,
        };
        bits = bits << 6 | u32::from(value);
        count += 6;
        if count >= 8 {
            count -= 8;
            out.push((bits >> count) as u8);
            bits &= (1 << count) - 1;
        }
    }
    (!out.is_empty()).then_some(out)
}

// ---- HTML, as far as a pandas table needs ----

/// The text of HTML: tags and the contents of `style` and `script` left out, entities turned into characters,
/// paragraphs and line breaks as lines.
pub fn html_to_text(html: &str) -> String {
    let mut out = String::new();
    let mut rest = html;
    while let Some(open) = rest.find('<') {
        out.push_str(&decode_entities(&rest[..open]));
        let Some(close) = rest[open..].find('>') else {
            rest = "";
            break;
        };
        let tag = rest[open + 1..open + close].trim().to_ascii_lowercase();
        rest = &rest[open + close + 1..];
        let name: String = tag.trim_start_matches('/').chars().take_while(|c| c.is_ascii_alphanumeric()).collect();
        if (name == "style" || name == "script") && !tag.starts_with('/') {
            let end = format!("</{name}");
            let lower = rest.to_ascii_lowercase();
            rest = lower.find(&end).and_then(|at| rest[at..].find('>').map(|g| &rest[at + g + 1..])).unwrap_or("");
        } else if matches!(name.as_str(), "br" | "p" | "div" | "tr" | "li" | "h1" | "h2" | "h3" | "h4" | "h5" | "h6") && !out.ends_with('\n') && !out.is_empty() {
            out.push('\n');
        }
    }
    out.push_str(&decode_entities(rest));
    out.trim().to_string()
}

fn decode_entities(text: &str) -> String {
    if !text.contains('&') {
        return text.to_string();
    }
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(at) = rest.find('&') {
        out.push_str(&rest[..at]);
        rest = &rest[at..];
        let decoded = rest.find(';').filter(|end| *end <= 10).and_then(|end| {
            let name = &rest[1..end];
            let c = match name {
                "amp" => '&',
                "lt" => '<',
                "gt" => '>',
                "quot" => '"',
                "apos" => '\'',
                "nbsp" => '\u{a0}',
                _ => {
                    let number = name.strip_prefix('#')?;
                    let code = match number.strip_prefix(['x', 'X']) {
                        Some(hex) => u32::from_str_radix(hex, 16).ok()?,
                        None => number.parse().ok()?,
                    };
                    char::from_u32(code)?
                }
            };
            Some((c, end))
        });
        match decoded {
            Some((c, end)) => {
                out.push(c);
                rest = &rest[end + 1..];
            }
            None => {
                out.push('&');
                rest = &rest[1..];
            }
        }
    }
    out.push_str(rest);
    out
}

/// The first `<table>` in HTML as rows, and the text around it. `None` if there isn't a table with a row in it.
pub fn html_table(html: &str) -> Option<(Table, String)> {
    let lower = html.to_ascii_lowercase();
    let start = lower.find("<table")?;
    let end = lower[start..].find("</table").map_or(html.len(), |e| start + e);
    let inner = &html[start..end];
    let inner_lower = &lower[start..end];
    let mut table = Table::default();
    let mut at = 0;
    while let Some(row_at) = inner_lower[at..].find("<tr") {
        let row_start = at + row_at;
        let row_end = inner_lower[row_start + 3..].find("<tr").map_or(inner.len(), |e| row_start + 3 + e);
        let row = &inner[row_start..row_end];
        let row_lower = &inner_lower[row_start..row_end];
        let mut cells = Vec::new();
        let mut c = 0;
        while let Some(found) = next_cell(&row_lower[c..]) {
            let open = c + found.0;
            let Some(body_start) = row_lower[open..].find('>').map(|g| open + g + 1) else { break };
            let next = next_cell(&row_lower[body_start..]).map_or(row.len(), |n| body_start + n.0);
            let body_end = row_lower[body_start..next].find("</t").map_or(next, |e| body_start + e);
            cells.push((html_to_text(&row[body_start..body_end]).replace('\n', " "), found.1));
            c = next;
        }
        if !cells.is_empty() {
            table.rows.push(cells);
        }
        at = row_end;
    }
    if table.rows.is_empty() {
        return None;
    }
    let tail_start = lower[end..].find('>').map_or(html.len(), |g| end + g + 1);
    let around = format!("{}\n{}", html_to_text(&html[..start]), html_to_text(&html[tail_start.min(html.len())..]));
    Some((table, around.trim().to_string()))
}

/// The next `<td` or `<th` in lower-case HTML: where, and whether it's a heading.
fn next_cell(lower: &str) -> Option<(usize, bool)> {
    let mut from = 0;
    while let Some(at) = lower[from..].find("<t") {
        let at = from + at;
        let tag = lower.as_bytes().get(at + 2).copied();
        let after = lower.as_bytes().get(at + 3).copied();
        if matches!(tag, Some(b'd' | b'h')) && matches!(after, Some(b'>' | b' ' | b'\t' | b'\n' | b'\r')) {
            return Some((at, tag == Some(b'h')));
        }
        from = at + 2;
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn colour_codes_become_styled_runs() {
        let runs = ansi_runs("plain \u{1b}[0;31mred\u{1b}[0m \u{1b}[1;4mbold\u{1b}[22;24m end", true);
        let words: Vec<&str> = runs.iter().map(|r| r.text.as_str()).collect();
        assert_eq!(words, vec!["plain ", "red", " ", "bold", " end"]);
        assert_eq!(runs[0].colour, None);
        assert!(runs[1].colour.is_some());
        assert_eq!(runs[2].colour, None);
        assert!(runs[3].bold && runs[3].underline);
        assert!(!runs[4].bold);
    }

    #[test]
    fn the_256_colour_and_true_colour_codes_are_read() {
        let runs = ansi_runs("\u{1b}[38;5;241ma\u{1b}[38;2;10;20;30mb\u{1b}[48;5;196mc", true);
        assert_eq!(runs[0].colour, Some(Color32::from_gray(8 + 10 * (241 - 232))));
        assert_eq!(runs[1].colour, Some(Color32::from_rgb(10, 20, 30)));
        assert_eq!(runs[2].background, Some(Color32::from_rgb(255, 0, 0)));
        assert_ne!(ansi_runs("\u{1b}[31mx", true)[0].colour, ansi_runs("\u{1b}[31mx", false)[0].colour, "dark and light pages get different reds");
    }

    #[test]
    fn other_escapes_are_dropped_and_text_survives() {
        assert_eq!(strip_ansi("a\u{1b}[2Kb\u{1b}[1Ac\u{1b}]0;title\u{7}d\u{1b}[31"), "abcd");
        assert_eq!(strip_ansi("no escapes \u{e9}"), "no escapes \u{e9}");
        assert_eq!(strip_ansi(""), "");
    }

    #[test]
    fn carriage_returns_write_over_the_line() {
        assert_eq!(settle_carriage_returns("10%\r50%\r100%\ndone"), "100%\ndone");
        assert_eq!(settle_carriage_returns("abc\rX"), "Xbc");
        assert_eq!(settle_carriage_returns("one\r\ntwo\r\n"), "one\ntwo\n");
        assert_eq!(settle_carriage_returns("plain\n"), "plain\n");
    }

    #[test]
    fn base64_is_decoded_with_line_breaks_and_padding() {
        assert_eq!(decode_base64("aGVsbG8=").unwrap(), b"hello");
        assert_eq!(decode_base64("aGVs\nbG8=\n").unwrap(), b"hello");
        assert_eq!(decode_base64("aGVsbG8").unwrap(), b"hello");
        assert_eq!(decode_base64("////").unwrap(), vec![255, 255, 255]);
        assert!(decode_base64("a$b").is_none());
        assert!(decode_base64("").is_none());
    }

    #[test]
    fn the_best_form_of_an_output_is_chosen() {
        let png = "iVBORw0KGgo=";
        let both = json!({"output_type": "display_data", "data": {"text/plain": "<Figure>", "image/png": png}, "metadata": {}});
        assert!(matches!(choose(&both), Shown::Png(bytes) if bytes.starts_with(&[0x89, b'P', b'N', b'G'])));
        let svg = json!({"output_type": "display_data", "data": {"text/plain": "x", "image/svg+xml": ["<svg>", "</svg>"]}, "metadata": {}});
        assert_eq!(choose(&svg), Shown::Svg("<svg></svg>".into()));
        let md = json!({"output_type": "execute_result", "data": {"text/plain": "x", "text/markdown": "**x**"}, "execution_count": 1, "metadata": {}});
        assert_eq!(choose(&md), Shown::Markdown("**x**".into()));
        let plain = json!({"output_type": "execute_result", "data": {"text/plain": "2"}, "execution_count": 1, "metadata": {}});
        assert_eq!(choose(&plain), Shown::Text("2".into()));
        let html_only = json!({"output_type": "display_data", "data": {"text/html": "<b>bold</b> &amp; <i>more</i>"}, "metadata": {}});
        assert_eq!(choose(&html_only), Shown::Text("bold & more".into()));
        let html_and_plain = json!({"output_type": "display_data", "data": {"text/html": "<div>hi</div>", "text/plain": "hi (plain)"}, "metadata": {}});
        assert_eq!(choose(&html_and_plain), Shown::Text("hi (plain)".into()), "html that isn't a table is shown as its plain form");
        let strange = json!({"output_type": "display_data", "data": {"application/vnd.widget": {"a": 1}}, "metadata": {}});
        assert_eq!(choose(&strange), Shown::Nothing(vec!["application/vnd.widget".into()]));
        let stream = json!({"output_type": "stream", "name": "stderr", "text": ["a\n", "b"]});
        assert_eq!(choose(&stream), Shown::Stream { name: "stderr".into(), text: "a\nb".into() });
        let error = json!({"output_type": "error", "ename": "ValueError", "evalue": "bad", "traceback": ["\u{1b}[31mValueError\u{1b}[0m", "line"]});
        assert_eq!(choose(&error), Shown::Error { name: "ValueError".into(), message: "bad".into(), traceback: "\u{1b}[31mValueError\u{1b}[0m\nline".into() });
        assert_eq!(plain_text_of(&error), "ValueError\nline");
    }

    #[test]
    fn a_broken_picture_falls_back_to_text() {
        let bad = json!({"output_type": "display_data", "data": {"text/plain": "<Figure>", "image/png": "!!!not base64!!!"}, "metadata": {}});
        assert_eq!(choose(&bad), Shown::Text("<Figure>".into()));
    }

    #[test]
    fn a_pandas_table_is_read_into_rows() {
        let html = include_str!("notebook_testdata/pandas.ipynb");
        let notebook = crate::notebook::Notebook::parse(html).unwrap();
        let output = &notebook.cells[0].outputs[0];
        let Shown::Table { table, .. } = choose(output) else { panic!("not a table: {:?}", choose(output)) };
        assert_eq!(table.rows.len(), 3);
        assert_eq!(table.rows[0], vec![(String::new(), true), ("a".into(), true), ("b".into(), true)]);
        assert_eq!(table.rows[1], vec![("0".into(), true), ("1".into(), false), ("x & y".into(), false)]);
        assert_eq!(table.rows[2][2], ("z".into(), false));
        assert!(plain_text_of(output).contains("x & y"));
    }

    #[test]
    fn a_table_keeps_the_text_around_it() {
        let (table, around) = html_table("<p>Before</p><table><tr><td>1</td><td>2</td></tr></table><p>2 rows &times; 2</p>").unwrap();
        assert_eq!(table.rows, vec![vec![("1".to_string(), false), ("2".to_string(), false)]]);
        assert!(around.starts_with("Before") && around.contains("2 rows"));
        assert!(html_table("<p>no table</p>").is_none());
        assert!(html_table("<table></table>").is_none());
        // Styles inside the table's wrapper are not text.
        assert_eq!(html_to_text("<style>a { b: c }</style>x<script>alert(1)</script>y"), "xy");
    }

    #[test]
    fn entities_are_turned_into_characters() {
        assert_eq!(decode_entities("a &amp; b &lt;c&gt; &#65;&#x42; &unknown; &"), "a & b <c> AB &unknown; &");
    }

    #[test]
    fn streams_in_pieces_are_joined_when_they_are_the_same_kind() {
        let mut outputs = Vec::new();
        add_stream(&mut outputs, "stdout", "a");
        add_stream(&mut outputs, "stdout", "b\n");
        add_stream(&mut outputs, "stderr", "e");
        add_stream(&mut outputs, "stdout", "c");
        let texts: Vec<String> = outputs.iter().map(|o| format!("{}:{}", o["name"].as_str().unwrap(), o["text"].as_str().unwrap())).collect();
        assert_eq!(texts, vec!["stdout:ab\n", "stderr:e", "stdout:c"]);
    }
}
