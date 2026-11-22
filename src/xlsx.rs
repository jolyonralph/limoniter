//! Reading an Excel workbook (`.xlsx`) into tables of data, one to a sheet. A workbook is a ZIP of XML files:
//! the sheets' names and order, the shared strings, the number formats (which say which numbers are dates) and
//! the sheets' cells. Formulas show the values Excel last worked out for them, which the file keeps beside them.

use crate::data_table::{Cell, ColumnKind, DataColumn, DataTable};
use crate::data_view::civil_from_days;
use crate::media::zip::Zip;
use quick_xml::Reader;
use quick_xml::events::Event;
use std::collections::HashMap;
use std::sync::Arc;

/// The most rows or columns read from a sheet, the most Excel itself has.
const MOST_ROWS: usize = 1_048_576;
const MOST_COLUMNS: usize = 16_384;

/// What XML said, with the names without their prefixes and the entities already turned into characters.
enum Part {
    Start(String, Vec<(String, String)>),
    End(String),
    Text(String),
}

fn local(name: &[u8]) -> String {
    let name = String::from_utf8_lossy(name);
    name.rsplit(':').next().unwrap_or("").to_string()
}

/// Goes through some XML, giving each part to `each`.
fn parts(xml: &[u8], mut each: impl FnMut(Part)) -> Result<(), String> {
    let mut reader = Reader::from_reader(xml);
    let mut text = String::new();
    let flush = |text: &mut String, each: &mut dyn FnMut(Part)| {
        if !text.is_empty() {
            each(Part::Text(std::mem::take(text)));
        }
    };
    let attributes = |start: &quick_xml::events::BytesStart| -> Vec<(String, String)> {
        start.attributes().flatten().map(|a| (local(a.key.as_ref()), a.normalized_value(quick_xml::XmlVersion::Implicit1_0).map(|v| v.into_owned()).unwrap_or_default())).collect()
    };
    loop {
        match reader.read_event().map_err(|e| crate::i18n::tf!("The spreadsheet's XML couldn't be read: {e}"))? {
            Event::Start(start) => {
                flush(&mut text, &mut each);
                each(Part::Start(local(start.name().as_ref()), attributes(&start)));
            }
            Event::Empty(start) => {
                flush(&mut text, &mut each);
                let name = local(start.name().as_ref());
                each(Part::Start(name.clone(), attributes(&start)));
                each(Part::End(name));
            }
            Event::End(end) => {
                flush(&mut text, &mut each);
                each(Part::End(local(end.name().as_ref())));
            }
            Event::Text(t) => text.push_str(&t.decode().map_err(|e| e.to_string())?),
            Event::CData(t) => text.push_str(&t.decode().map_err(|e| e.to_string())?),
            Event::GeneralRef(r) => {
                if let Ok(Some(c)) = r.resolve_char_ref() {
                    text.push(c);
                } else {
                    match r.decode().map_err(|e| e.to_string())?.as_ref() {
                        "amp" => text.push('&'),
                        "lt" => text.push('<'),
                        "gt" => text.push('>'),
                        "quot" => text.push('"'),
                        "apos" => text.push('\''),
                        _ => {}
                    }
                }
            }
            Event::Eof => break,
            _ => {}
        }
    }
    flush(&mut text, &mut each);
    Ok(())
}

fn attribute<'a>(attributes: &'a [(String, String)], name: &str) -> Option<&'a str> {
    attributes.iter().find(|(key, _)| key == name).map(|(_, value)| value.as_str())
}

/// The shared strings: each `<si>` is plain text or runs of it (and phonetic guides, which are left out).
fn shared_strings(xml: &[u8]) -> Result<Vec<String>, String> {
    let (mut strings, mut current, mut in_text, mut in_phonetic) = (Vec::new(), String::new(), false, 0);
    parts(xml, |part| match part {
        Part::Start(name, _) => match name.as_str() {
            "si" => current.clear(),
            "t" => in_text = true,
            "rPh" => in_phonetic += 1,
            _ => {}
        },
        Part::End(name) => match name.as_str() {
            "si" => strings.push(std::mem::take(&mut current)),
            "t" => in_text = false,
            "rPh" => in_phonetic -= 1,
            _ => {}
        },
        Part::Text(text) => {
            if in_text && in_phonetic == 0 {
                current.push_str(&text);
            }
        }
    })?;
    Ok(strings)
}

/// How a cell's number is shown, as far as it matters here.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Shown {
    Plain,
    Date,
    DateTime,
    Time,
}

/// Which of a number format's pieces say it's a date or a time. Quoted text, escaped letters, colours and
/// conditions in brackets are skipped, and `m` is a minute only when beside hours or seconds.
fn shown_by_format(code: &str) -> Shown {
    let (mut date, mut time, mut month) = (false, false, false);
    let mut chars = code.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '"' => {
                for q in chars.by_ref() {
                    if q == '"' {
                        break;
                    }
                }
            }
            '\\' | '_' | '*' => {
                chars.next();
            }
            '[' => {
                // [h], [mm] and [ss] are elapsed times; anything else in brackets is a colour or condition.
                let inside: String = chars.by_ref().take_while(|&q| q != ']').collect();
                if inside.chars().all(|q| matches!(q.to_ascii_lowercase(), 'h' | 'm' | 's')) && !inside.is_empty() {
                    time = true;
                }
            }
            'y' | 'Y' | 'd' | 'D' => date = true,
            'h' | 'H' | 's' | 'S' => time = true,
            'm' | 'M' => month = true,
            _ => {}
        }
    }
    if month && !time {
        date = true;
    }
    match (date, time) {
        (true, true) => Shown::DateTime,
        (true, false) => Shown::Date,
        (false, true) => Shown::Time,
        _ => Shown::Plain,
    }
}

/// The built-in format ids that are dates or times.
fn shown_by_id(id: u32) -> Shown {
    match id {
        14..=17 | 27..=31 | 34..=36 | 50..=58 => Shown::Date,
        18..=21 | 45..=47 => Shown::Time,
        22 => Shown::DateTime,
        _ => Shown::Plain,
    }
}

/// For each style (cell format) number, whether its numbers are dates.
fn styles(xml: &[u8]) -> Result<Vec<Shown>, String> {
    let (mut custom, mut out, mut in_cell_formats) = (HashMap::new(), Vec::new(), false);
    parts(xml, |part| {
        if let Part::Start(name, attributes) = part {
            match name.as_str() {
                "numFmt" => {
                    if let (Some(id), Some(code)) = (attribute(&attributes, "numFmtId").and_then(|v| v.parse::<u32>().ok()), attribute(&attributes, "formatCode")) {
                        custom.insert(id, shown_by_format(code));
                    }
                }
                "cellXfs" => in_cell_formats = true,
                "cellStyleXfs" => in_cell_formats = false,
                "xf" if in_cell_formats => {
                    let id = attribute(&attributes, "numFmtId").and_then(|v| v.parse::<u32>().ok()).unwrap_or(0);
                    out.push(custom.get(&id).copied().unwrap_or_else(|| shown_by_id(id)));
                }
                _ => {}
            }
        }
    })?;
    Ok(out)
}

/// A date from a count of days since the workbook's own start. The 1900 system counts a 29 February 1900 that
/// never happened (a mistake kept from Lotus 1-2-3), so day 60 has no real date and later days are one off.
fn date_text(days: i64, system_1904: bool) -> String {
    if !system_1904 && days == 60 {
        return "1900-02-29".into();
    }
    // Counted from 1970-01-01: 1900's day 61 is 1900-03-01 (so day 0 is 1899-12-30), and a 1904 workbook's day 0 is 1904-01-01.
    let unix_days = if system_1904 { days - 24107 } else if (1..60).contains(&days) { days - 25568 } else { days - 25569 };
    let (y, m, d) = civil_from_days(unix_days);
    format!("{y:04}-{m:02}-{d:02}")
}

/// What a number cell shows: the number as it is, or the date or time its format says it is.
fn shown_number(text: &str, shown: Shown, system_1904: bool) -> Cell {
    let Ok(value) = text.trim().parse::<f64>() else {
        return Cell::Text(text.to_string());
    };
    if shown == Shown::Plain || !value.is_finite() || value < 0.0 && shown == Shown::Time {
        return plain_number(text, value);
    }
    let whole = value.floor();
    let seconds = ((value - whole) * 86_400.0).round() as i64;
    // Rounding to the second can carry into the next day.
    let (days, seconds) = if seconds >= 86_400 { (whole as i64 + 1, 0) } else { (whole as i64, seconds) };
    let clock = format!("{:02}:{:02}:{:02}", seconds / 3600, seconds / 60 % 60, seconds % 60);
    Cell::Text(match shown {
        Shown::Date => date_text(days, system_1904),
        Shown::DateTime => format!("{} {clock}", date_text(days, system_1904)),
        _ => clock,
    })
}

fn plain_number(text: &str, value: f64) -> Cell {
    let text = text.trim();
    if !text.contains(['.', 'e', 'E']) && let Ok(n) = text.parse::<i64>() {
        return Cell::Int(n);
    }
    // 3.0 written as a real is still the whole number 3.
    if value.fract() == 0.0 && value.abs() < 1e15 {
        return Cell::Int(value as i64);
    }
    Cell::Real(value)
}

/// "C" is column 2, "AA" is 26, counting from 0.
fn column_of(reference: &str) -> Option<usize> {
    let letters: String = reference.chars().take_while(char::is_ascii_alphabetic).collect();
    if letters.is_empty() {
        return None;
    }
    Some(letters.to_ascii_uppercase().bytes().fold(0usize, |n, b| n * 26 + usize::from(b - b'A' + 1)) - 1)
}

/// The letters that name column `n` in a spreadsheet, from 0: A, B, ... Z, AA.
pub fn column_letters(mut n: usize) -> String {
    let mut out = Vec::new();
    loop {
        out.push(b'A' + (n % 26) as u8);
        if n < 26 {
            break;
        }
        n = n / 26 - 1;
    }
    out.reverse();
    String::from_utf8(out).unwrap_or_default()
}

/// One sheet's cells as rows, with gaps filled with nothing.
fn sheet_rows(xml: &[u8], strings: &[String], styles: &[Shown], system_1904: bool) -> Result<Vec<Vec<Cell>>, String> {
    struct Open {
        column: usize,
        kind: String,
        style: usize,
        value: String,
        inline: String,
    }
    let mut rows: Vec<Vec<Cell>> = Vec::new();
    let (mut row_at, mut next_row) = (0usize, 0usize);
    let mut cell: Option<Open> = None;
    let (mut in_value, mut in_inline, mut in_phonetic, mut in_formula) = (false, false, 0, false);
    let mut next_column = 0usize;
    parts(xml, |part| match part {
        Part::Start(name, attributes) => match name.as_str() {
            "row" => {
                row_at = attribute(&attributes, "r").and_then(|r| r.parse::<usize>().ok()).map_or(next_row, |r| r.saturating_sub(1));
                next_row = row_at + 1;
                next_column = 0;
            }
            "c" if row_at < MOST_ROWS => {
                let column = attribute(&attributes, "r").and_then(column_of).unwrap_or(next_column);
                next_column = column + 1;
                cell = Some(Open { column, kind: attribute(&attributes, "t").unwrap_or("n").to_string(), style: attribute(&attributes, "s").and_then(|s| s.parse().ok()).unwrap_or(0), value: String::new(), inline: String::new() });
            }
            "v" => in_value = true,
            "f" => in_formula = true,
            "is" => in_inline = true,
            "rPh" => in_phonetic += 1,
            _ => {}
        },
        Part::Text(text) => {
            if let Some(open) = cell.as_mut() {
                if in_value && !in_formula {
                    open.value.push_str(&text);
                } else if in_inline && in_phonetic == 0 && !in_value {
                    open.inline.push_str(&text);
                }
            }
        }
        Part::End(name) => match name.as_str() {
            "v" => in_value = false,
            "f" => in_formula = false,
            "is" => in_inline = false,
            "rPh" => in_phonetic -= 1,
            "c" => {
                let Some(open) = cell.take() else { return };
                if open.column >= MOST_COLUMNS {
                    return;
                }
                let value = match open.kind.as_str() {
                    "s" => open.value.trim().parse::<usize>().ok().and_then(|i| strings.get(i)).map_or(Cell::Null, |s| Cell::Text(s.clone())),
                    "inlineStr" => Cell::Text(open.inline),
                    "str" | "d" => {
                        if open.value.is_empty() && open.inline.is_empty() {
                            Cell::Null
                        } else {
                            Cell::Text(if open.value.is_empty() { open.inline } else { open.value })
                        }
                    }
                    "b" => Cell::Bool(open.value.trim() == "1"),
                    "e" => Cell::Text(open.value),
                    _ if open.value.trim().is_empty() => Cell::Null,
                    _ => shown_number(&open.value, styles.get(open.style).copied().unwrap_or(Shown::Plain), system_1904),
                };
                if value == Cell::Null && open.column > 0 {
                    return;
                }
                while rows.len() <= row_at {
                    rows.push(Vec::new());
                }
                let row = &mut rows[row_at];
                if row.len() <= open.column {
                    row.resize(open.column + 1, Cell::Null);
                }
                row[open.column] = value;
            }
            _ => {}
        },
    })?;
    Ok(rows)
}

/// A table from rows of cells. If the first row is all words it names the columns; otherwise they're lettered, as
/// a spreadsheet does.
fn table_of(mut rows: Vec<Vec<Cell>>) -> DataTable {
    while rows.last().is_some_and(|r| r.iter().all(|c| *c == Cell::Null)) {
        rows.pop();
    }
    let width = rows.iter().map(Vec::len).max().unwrap_or(0);
    let header = rows.len() > 1 && rows[0].iter().any(|c| *c != Cell::Null) && rows[0].iter().all(|c| matches!(c, Cell::Text(_) | Cell::Null));
    let names: Vec<String> = (0..width)
        .map(|i| match rows.first().and_then(|r| r.get(i)) {
            Some(Cell::Text(t)) if header && !t.trim().is_empty() => t.clone(),
            _ => column_letters(i),
        })
        .collect();
    if header {
        rows.remove(0);
    }
    for row in &mut rows {
        row.resize(width, Cell::Null);
    }
    DataTable { columns: names.into_iter().map(|name| DataColumn { name, kind: ColumnKind::Unknown }).collect(), rows, note: None }
}

/// Where a relationship's target is in the ZIP: relative to the folder the relationships belong to.
fn resolve(target: &str) -> String {
    match target.strip_prefix('/') {
        Some(absolute) => absolute.to_string(),
        None => format!("xl/{target}"),
    }
}

/// Every sheet of a workbook, as its name and its table. Sheets that are charts have no cells and are left out.
pub fn read(bytes: Vec<u8>) -> Result<Vec<(String, DataTable)>, String> {
    let zip = Zip::open(Arc::new(bytes)).map_err(|_| crate::i18n::tr("This isn't an Excel workbook: it isn't a ZIP archive."))?;
    let get = |path: &str| -> Result<Option<Vec<u8>>, String> { zip.read_path(path).transpose() };
    let workbook = get("xl/workbook.xml")?.ok_or(crate::i18n::tr("This ZIP archive isn't an Excel workbook: it has no xl/workbook.xml."))?;
    let relationships = get("xl/_rels/workbook.xml.rels")?.unwrap_or_default();
    let mut targets = HashMap::new();
    parts(&relationships, |part| {
        if let Part::Start(name, attributes) = part
            && name == "Relationship"
            && let (Some(id), Some(target)) = (attribute(&attributes, "Id"), attribute(&attributes, "Target"))
        {
            targets.insert(id.to_string(), resolve(target));
        }
    })?;
    let (mut sheets, mut system_1904) = (Vec::new(), false);
    parts(&workbook, |part| {
        if let Part::Start(name, attributes) = part {
            match name.as_str() {
                "workbookPr" => system_1904 = attribute(&attributes, "date1904").is_some_and(|v| v == "1" || v == "true"),
                "sheet" => {
                    let id = attribute(&attributes, "id").unwrap_or("");
                    sheets.push((attribute(&attributes, "name").unwrap_or("Sheet").to_string(), targets.get(id).cloned()));
                }
                _ => {}
            }
        }
    })?;
    let strings = get("xl/sharedStrings.xml")?.map(|xml| shared_strings(&xml)).transpose()?.unwrap_or_default();
    let styles = get("xl/styles.xml")?.map(|xml| styles(&xml)).transpose()?.unwrap_or_default();
    let mut out = Vec::new();
    for (name, target) in sheets {
        let Some(target) = target else { continue };
        let Some(xml) = get(&target)? else { continue };
        let table = table_of(sheet_rows(&xml, &strings, &styles, system_1904)?);
        // A sheet with no cells (an empty one, or a chart) has nothing to show.
        if !table.columns.is_empty() {
            out.push((name, table));
        }
    }
    if out.is_empty() {
        return Err(crate::i18n::tr("This workbook has no sheets with cells in them.").into());
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn read_file(name: &str) -> Vec<(String, DataTable)> {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src/data_testdata").join(name);
        read(std::fs::read(path).unwrap()).unwrap()
    }

    /// A ZIP of stored files, enough for the reader (which doesn't check the checksums).
    pub(crate) fn stored_zip(files: &[(&str, &str)]) -> Vec<u8> {
        let (mut out, mut directory) = (Vec::new(), Vec::new());
        for (name, body) in files {
            let offset = out.len() as u32;
            let header = |out: &mut Vec<u8>, central: bool| {
                out.extend_from_slice(if central { b"PK\x01\x02" } else { b"PK\x03\x04" });
                if central {
                    out.extend_from_slice(&[20, 0]);
                }
                out.extend_from_slice(&[20, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
                out.extend_from_slice(&(body.len() as u32).to_le_bytes());
                out.extend_from_slice(&(body.len() as u32).to_le_bytes());
                out.extend_from_slice(&(name.len() as u16).to_le_bytes());
                out.extend_from_slice(&[0, 0]);
            };
            header(&mut out, false);
            out.extend_from_slice(name.as_bytes());
            out.extend_from_slice(body.as_bytes());
            header(&mut directory, true);
            directory.extend_from_slice(&[0, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
            directory.extend_from_slice(&offset.to_le_bytes());
            directory.extend_from_slice(name.as_bytes());
        }
        let start = out.len() as u32;
        out.extend_from_slice(&directory);
        out.extend_from_slice(b"PK\x05\x06\0\0\0\0");
        out.extend_from_slice(&(files.len() as u16).to_le_bytes());
        out.extend_from_slice(&(files.len() as u16).to_le_bytes());
        out.extend_from_slice(&(directory.len() as u32).to_le_bytes());
        out.extend_from_slice(&start.to_le_bytes());
        out.extend_from_slice(&[0, 0]);
        out
    }

    #[test]
    fn a_workbook_made_by_openpyxl_reads_with_its_dates_and_formulas() {
        let sheets = read_file("people.xlsx");
        let names: Vec<&str> = sheets.iter().map(|(n, _)| n.as_str()).collect();
        assert_eq!(names, ["People", "Plain"], "the empty sheet has no cells and isn't shown");
        let table = &sheets[0].1;
        let headings: Vec<&str> = table.columns.iter().map(|c| c.name.as_str()).collect();
        assert_eq!(headings, ["Name", "Age", "Score", "Born", "Seen", "Member", "Note"]);
        assert_eq!(table.rows[0], vec![Cell::Text("Ada".into()), Cell::Int(36), Cell::Real(9.5), Cell::Text("1815-12-10".into()), Cell::Text("2026-01-02 13:45:30".into()), Cell::Bool(true), Cell::Text("a & b <c>".into())]);
        assert_eq!(table.rows[1][3], Cell::Text("1906-12-09".into()));
        assert_eq!(table.rows[1][4], Cell::Text("2026-03-04 00:00:00".into()));
        assert_eq!(table.rows[1][5], Cell::Bool(false));
        assert_eq!(table.rows[2][1], Cell::Null);
        assert_eq!(table.rows[2][6], Cell::Text("line\nbreak".into()));
    }

    #[test]
    fn a_sheet_with_no_heading_row_has_lettered_columns_and_formats_decide_dates() {
        let sheets = read_file("people.xlsx");
        let table = &sheets[1].1;
        let headings: Vec<&str> = table.columns.iter().map(|c| c.name.as_str()).collect();
        assert_eq!(headings, ["A", "B", "C", "D", "E"]);
        assert_eq!(table.rows[0], vec![Cell::Int(1), Cell::Int(2), Cell::Int(3), Cell::Null, Cell::Null]);
        assert_eq!(table.rows[3][4], Cell::Int(99));
        assert_eq!(table.rows[5][0], Cell::Text("2000-02-29".into()));
        assert_eq!(table.rows[5][1], Cell::Text("06:30:00".into()), "built-in id 21 is a time");
        assert_eq!(table.rows[5][2], Cell::Text("2024-07-04".into()), "a custom 'd mmm yy'");
    }

    #[test]
    fn the_1904_date_system_counts_from_a_different_day() {
        let sheets = read_file("mac1904.xlsx");
        assert_eq!(sheets[0].1.rows[0][0], Cell::Text("2020-05-17".into()));
    }

    #[test]
    fn the_1900_system_keeps_its_leap_day_mistake() {
        assert_eq!(date_text(1, false), "1900-01-01");
        assert_eq!(date_text(59, false), "1900-02-28");
        assert_eq!(date_text(60, false), "1900-02-29");
        assert_eq!(date_text(61, false), "1900-03-01");
        assert_eq!(date_text(25569, false), "1970-01-01");
        assert_eq!(date_text(45000, false), "2023-03-15");
        assert_eq!(date_text(0, true), "1904-01-01");
    }

    #[test]
    fn number_formats_are_told_apart() {
        assert_eq!(shown_by_format("yyyy-mm-dd"), Shown::Date);
        assert_eq!(shown_by_format("dd/mm/yyyy hh:mm"), Shown::DateTime);
        assert_eq!(shown_by_format("h:mm:ss AM/PM"), Shown::Time);
        assert_eq!(shown_by_format("0.00"), Shown::Plain);
        assert_eq!(shown_by_format("0.00E+00"), Shown::Plain);
        assert_eq!(shown_by_format("#,##0 \"days\""), Shown::Plain, "quoted words are not date letters");
        assert_eq!(shown_by_format("[Red]0.0"), Shown::Plain);
        assert_eq!(shown_by_format("mmmm yyyy"), Shown::Date);
        assert_eq!(shown_by_format("General"), Shown::Plain);
        assert_eq!(shown_by_id(14), Shown::Date);
        assert_eq!(shown_by_id(22), Shown::DateTime);
        assert_eq!(shown_by_id(0), Shown::Plain);
    }

    #[test]
    fn shared_strings_booleans_errors_formula_values_and_gaps() {
        let workbook = r#"<workbook xmlns:r="x"><workbookPr/><sheets><sheet name="Data &amp; more" sheetId="1" r:id="rId1"/></sheets></workbook>"#;
        let rels = r#"<Relationships><Relationship Id="rId1" Target="worksheets/sheet1.xml"/></Relationships>"#;
        let strings = r#"<sst><si><t>Item</t></si><si><r><t>Rich </t></r><r><t>text</t></r><rPh><t>ignored</t></rPh></si><si><t xml:space="preserve"> padded </t></si></sst>"#;
        let styles = r#"<styleSheet><cellXfs><xf numFmtId="0"/><xf numFmtId="14"/></cellXfs></styleSheet>"#;
        let sheet = r#"<worksheet><sheetData>
            <row r="1"><c r="A1" t="s"><v>0</v></c><c r="C1" t="s"><v>0</v></c></row>
            <row r="3"><c r="A3" t="s"><v>1</v></c><c r="B3" t="b"><v>1</v></c><c r="C3" t="e"><v>#DIV/0!</v></c><c r="D3"><f>1+2</f><v>3</v></c><c r="E3" t="str"><f>"a"&amp;"b"</f><v>ab</v></c><c r="F3" s="1"><v>43831</v></c><c r="G3" t="s"><v>2</v></c><c r="H3"><v>1.5E3</v></c><c r="I3"><v>0.1</v></c></row>
        </sheetData></worksheet>"#;
        let zip = stored_zip(&[("xl/workbook.xml", workbook), ("xl/_rels/workbook.xml.rels", rels), ("xl/sharedStrings.xml", strings), ("xl/styles.xml", styles), ("xl/worksheets/sheet1.xml", sheet)]);
        let sheets = read(zip).unwrap();
        assert_eq!(sheets[0].0, "Data & more");
        let table = &sheets[0].1;
        // Row 1 is words, so it names the columns; the blank column B is lettered.
        assert_eq!(table.columns[0].name, "Item");
        assert_eq!(table.columns[1].name, "B");
        assert_eq!(table.rows.len(), 2, "row 2 is empty but kept, so rows keep their places");
        assert_eq!(table.rows[0], vec![Cell::Null; 9]);
        assert_eq!(
            table.rows[1],
            vec![Cell::Text("Rich text".into()), Cell::Bool(true), Cell::Text("#DIV/0!".into()), Cell::Int(3), Cell::Text("ab".into()), Cell::Text("2020-01-01".into()), Cell::Text(" padded ".into()), Cell::Int(1500), Cell::Real(0.1)]
        );
    }

    #[test]
    fn things_that_are_not_workbooks_say_so() {
        assert!(read(b"plain text".to_vec()).is_err());
        let zip = stored_zip(&[("hello.txt", "hi")]);
        assert!(read(zip).is_err());
    }

    #[test]
    fn column_letters_count_like_a_spreadsheet() {
        assert_eq!(column_letters(0), "A");
        assert_eq!(column_letters(25), "Z");
        assert_eq!(column_letters(26), "AA");
        assert_eq!(column_letters(701), "ZZ");
        assert_eq!(column_letters(702), "AAA");
        assert_eq!(column_of("AA12"), Some(26));
        assert_eq!(column_of("12"), None);
    }
}
