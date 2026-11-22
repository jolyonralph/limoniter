//! A table of data to look at: what the data viewer shows, whatever it came from. A spreadsheet's sheet, a
//! JSON-lines file, a data frame in a running Python or R session, or the rows a database query gave back all
//! become one of these, so they're shown, sorted, filtered and summed up the same way.

// Used once the data viewer and database browser join (0.16 batch 2).
#![allow(dead_code)]

/// What a column holds, as far as it's known: from a database's column types, a data frame's dtypes, or guessed
/// from the values by the data viewer.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ColumnKind {
    Text,
    Integer,
    Real,
    Bool,
    /// A date, shown as its text ("2026-11-22").
    Date,
    /// A date and time, shown as its text ("2026-11-22 14:05:00").
    DateTime,
    Binary,
    /// JSON, kept as its text.
    Json,
    /// Not said; the viewer may guess.
    Unknown,
}

#[derive(Clone, Debug, PartialEq)]
pub struct DataColumn {
    pub name: String,
    pub kind: ColumnKind,
}

/// One value. Dates, times, decimals and JSON come as `Text`, with their column's kind saying what they are.
#[derive(Clone, Debug, PartialEq)]
pub enum Cell {
    Null,
    Text(String),
    Int(i64),
    Real(f64),
    Bool(bool),
    Bytes(Vec<u8>),
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct DataTable {
    pub columns: Vec<DataColumn>,
    pub rows: Vec<Vec<Cell>>,
    /// Anything the person should know about it, such as "The first 10,000 of 1,204,311 rows". Already translated.
    pub note: Option<String>,
}

impl Cell {
    /// The value as text, as a table shows it: empty for Null, bytes in hexadecimal.
    pub fn text(&self) -> String {
        match self {
            Cell::Null => String::new(),
            Cell::Text(text) => text.clone(),
            Cell::Int(n) => n.to_string(),
            Cell::Real(x) => x.to_string(),
            Cell::Bool(b) => b.to_string(),
            Cell::Bytes(bytes) => bytes.iter().map(|b| format!("{b:02X}")).collect(),
        }
    }
}

impl DataTable {
    /// The table as tab-separated text with a heading row; tabs and line breaks in values become spaces.
    pub fn to_tsv(&self) -> String {
        let clean = |text: String| text.replace(['\t', '\n', '\r'], " ");
        let mut out = self.columns.iter().map(|c| clean(c.name.clone())).collect::<Vec<_>>().join("\t");
        out.push('\n');
        for row in &self.rows {
            out.push_str(&row.iter().map(|cell| clean(cell.text())).collect::<Vec<_>>().join("\t"));
            out.push('\n');
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_table_becomes_tab_separated_text() {
        let table = DataTable {
            columns: vec![DataColumn { name: "id".into(), kind: ColumnKind::Integer }, DataColumn { name: "name".into(), kind: ColumnKind::Text }],
            rows: vec![vec![Cell::Int(1), Cell::Text("a\tb".into())], vec![Cell::Null, Cell::Bytes(vec![0xAB, 1])]],
            note: None,
        };
        assert_eq!(table.to_tsv(), "id\tname\n1\ta b\n\tAB01\n");
    }
}
