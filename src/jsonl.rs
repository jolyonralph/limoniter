//! Reading JSON lines (`.jsonl`, `.ndjson`): a file with one JSON object on each line. The keys make the
//! columns, in the order they first turn up, and a line that lacks a key leaves that cell empty. Lists and
//! objects inside a value are kept as their JSON text.

use crate::data_table::{Cell, ColumnKind, DataColumn, DataTable};
use serde_json::Value;

/// Whether a file's name says it's JSON lines.
pub fn is_jsonl_name(name: &str) -> bool {
    let lower = name.to_lowercase();
    lower.ends_with(".jsonl") || lower.ends_with(".ndjson")
}

fn cell_of(value: &Value) -> Cell {
    match value {
        Value::Null => Cell::Null,
        Value::Bool(b) => Cell::Bool(*b),
        Value::Number(n) => n.as_i64().map(Cell::Int).or_else(|| n.as_f64().map(Cell::Real)).unwrap_or_else(|| Cell::Text(n.to_string())),
        Value::String(s) => Cell::Text(s.clone()),
        nested => Cell::Text(nested.to_string()),
    }
}

/// The table a JSON-lines text makes. Blank lines are skipped; a line that isn't JSON is left out and counted in
/// the note. A line that is JSON but not an object (a number, a list) goes in a column called "value".
pub fn read(text: &str) -> Result<DataTable, String> {
    let mut names: Vec<String> = Vec::new();
    let mut index = std::collections::HashMap::<String, usize>::new();
    let mut rows: Vec<Vec<(usize, Cell)>> = Vec::new();
    let mut nested_columns: Vec<bool> = Vec::new();
    let (mut bad, mut first_bad) = (0usize, 0usize);
    for (number, line) in text.lines().enumerate() {
        let line = line.trim_start_matches('\u{feff}').trim();
        if line.is_empty() {
            continue;
        }
        let Ok(value) = serde_json::from_str::<Value>(line) else {
            bad += 1;
            if first_bad == 0 {
                first_bad = number + 1;
            }
            continue;
        };
        let pairs: Vec<(String, Value)> = match value {
            Value::Object(map) => map.into_iter().collect(),
            other => vec![("value".to_string(), other)],
        };
        let mut row = Vec::with_capacity(pairs.len());
        for (key, value) in pairs {
            let column = *index.entry(key.clone()).or_insert_with(|| {
                names.push(key);
                nested_columns.push(true);
                names.len() - 1
            });
            if !matches!(value, Value::Array(_) | Value::Object(_) | Value::Null) {
                nested_columns[column] = false;
            }
            row.push((column, cell_of(&value)));
        }
        rows.push(row);
    }
    if rows.is_empty() {
        return Err(if bad > 0 { crate::i18n::tf!("Line {first_bad} isn't JSON, and no line is: this isn't a JSON-lines file.") } else { crate::i18n::tr("This file has no lines with JSON in them.").into() });
    }
    let width = names.len();
    let rows = rows
        .into_iter()
        .map(|pairs| {
            let mut row = vec![Cell::Null; width];
            for (column, cell) in pairs {
                row[column] = cell;
            }
            row
        })
        .collect();
    let columns = names.into_iter().zip(nested_columns).map(|(name, nested)| DataColumn { name, kind: if nested { ColumnKind::Json } else { ColumnKind::Unknown } }).collect();
    let note = (bad > 0).then(|| crate::i18n::tf!("{bad} lines weren't JSON and are left out; the first is line {first_bad}."));
    Ok(DataTable { columns, rows, note })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keys_make_the_columns_in_the_order_they_appear() {
        let text = std::fs::read_to_string(std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src/data_testdata/small.jsonl")).unwrap();
        let table = read(&text).unwrap();
        let names: Vec<&str> = table.columns.iter().map(|c| c.name.as_str()).collect();
        assert_eq!(names, ["id", "name", "tags", "meta", "score", "ok"]);
        assert_eq!(table.rows.len(), 3, "the blank line is skipped");
        assert_eq!(table.rows[0][2], Cell::Text("[\"x\",\"y\"]".into()), "nested values are kept as JSON");
        assert_eq!(table.rows[0][3], Cell::Text("{\"a\":1}".into()));
        assert_eq!(table.rows[1][4], Cell::Real(8.5));
        assert_eq!(table.rows[1][2], Cell::Null, "a missing key is an empty cell");
        assert_eq!(table.rows[2][5], Cell::Bool(true));
        assert_eq!(table.rows[2][1], Cell::Null);
        assert_eq!(table.columns[2].kind, ColumnKind::Json);
        assert_eq!(table.columns[0].kind, ColumnKind::Unknown);
        assert!(table.note.is_none());
    }

    #[test]
    fn lines_that_are_not_objects_and_lines_that_are_not_json() {
        let table = read("1\n[2,3]\n{\"a\": \"x\"}\nnot json\n").unwrap();
        let names: Vec<&str> = table.columns.iter().map(|c| c.name.as_str()).collect();
        assert_eq!(names, ["value", "a"]);
        assert_eq!(table.rows.len(), 3);
        assert_eq!(table.rows[1][0], Cell::Text("[2,3]".into()));
        assert!(table.note.as_deref().unwrap().contains("line 4"));
        assert!(read("nothing\nuseful\n").is_err());
        assert!(read("\n\n").is_err());
    }

    #[test]
    fn names_are_recognised_whatever_their_case() {
        assert!(is_jsonl_name("Events.JSONL") && is_jsonl_name("a.ndjson"));
        assert!(!is_jsonl_name("a.json"));
    }
}
