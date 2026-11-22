//! A notebook's variables, for the Variables panel: the code that asks a Python or R kernel what it holds and
//! for the first rows of a table-like one, and what is made of the kernel's answers. The code is run on the
//! kernel quietly (see `Session::ask`), so it isn't a cell and leaves the kernel's history and counts alone.

use crate::data_table::{Cell, ColumnKind, DataColumn, DataTable};
use serde_json::Value;

/// The most rows taken from a variable into the data viewer.
pub const MOST_ROWS: usize = 100_000;

const MARKER: &str = "@@LIMONITER@@";

/// What a variable is, as far as opening it in the data viewer goes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Shape {
    /// Anything that isn't a table.
    Plain,
    /// A data frame (pandas, polars, R's data.frame and tibble).
    Frame,
    /// A two-dimensional array or matrix.
    Array,
    /// A list of dictionaries, one for each row.
    Rows,
    /// A dictionary (or named list) of equally long lists, one for each column.
    Columns,
}

impl Shape {
    pub fn opens(self) -> bool {
        self != Shape::Plain
    }
}

/// One variable of the kernel.
#[derive(Clone, Debug, PartialEq)]
pub struct Var {
    pub name: String,
    pub type_name: String,
    /// Its shape or length, as the kernel says: "3 × 5", "12".
    pub size: String,
    pub shape: Shape,
    /// A short look at a single value, or nothing.
    pub value: String,
}

/// Whether the notebook's kernel is one the variables can be listed for, from its language ("python", "R").
pub fn can_list(language: &str) -> bool {
    matches!(language.to_lowercase().as_str(), "python" | "r")
}

fn is_r(language: &str) -> bool {
    language.eq_ignore_ascii_case("r")
}

/// The code that lists the variables, for a kernel of this language.
pub fn list_code(language: &str) -> Option<String> {
    can_list(language).then(|| if is_r(language) { include_str!("kernel_code/vars_list.R") } else { include_str!("kernel_code/vars_list.py") }.to_string())
}

/// The code that gets the first `limit` rows of the variable `name`.
pub fn fetch_code(language: &str, name: &str, limit: usize) -> Option<String> {
    can_list(language).then(|| {
        let template = if is_r(language) { include_str!("kernel_code/vars_fetch.R") } else { include_str!("kernel_code/vars_fetch.py") };
        // A name as a string literal: JSON's escapes are valid in both languages for the names that variables can have.
        let literal = serde_json::to_string(name).unwrap_or_else(|_| "\"\"".to_string());
        template.replace("NAME", &literal).replace("LIMIT", &limit.to_string())
    })
}

/// What the kernel printed between its markers.
fn between_markers(output: &str) -> Result<&str, String> {
    let start = output.find(MARKER).ok_or_else(|| crate::i18n::tr("The kernel didn't answer in the way expected.").to_string())? + MARKER.len();
    let end = output[start..].find(MARKER).ok_or_else(|| crate::i18n::tr("The kernel's answer was cut short.").to_string())?;
    Ok(&output[start..start + end])
}

/// The variables, from what the listing code printed. They're put in order by name, ignoring case.
pub fn parse_list(output: &str) -> Result<Vec<Var>, String> {
    let json: Value = serde_json::from_str(between_markers(output)?).map_err(|e| crate::i18n::tf!("The kernel's list of variables couldn't be read: {e}"))?;
    let text = |item: &Value, key: &str| item.get(key).and_then(Value::as_str).unwrap_or("").to_string();
    let mut vars: Vec<Var> = json
        .as_array()
        .map(|items| {
            items
                .iter()
                .map(|item| Var {
                    name: text(item, "name"),
                    type_name: text(item, "type"),
                    size: text(item, "size"),
                    shape: match text(item, "kind").as_str() {
                        "frame" => Shape::Frame,
                        "array" => Shape::Array,
                        "rows" => Shape::Rows,
                        "columns" => Shape::Columns,
                        _ => Shape::Plain,
                    },
                    value: text(item, "value"),
                })
                .collect()
        })
        .unwrap_or_default();
    vars.sort_by_key(|v| (v.name.to_lowercase(), v.name.clone()));
    Ok(vars)
}

fn kind_of(word: &str) -> ColumnKind {
    match word {
        "int" => ColumnKind::Integer,
        "real" => ColumnKind::Real,
        "bool" => ColumnKind::Bool,
        _ => ColumnKind::Unknown,
    }
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

/// The table from what the fetching code printed. If the variable has more rows than were taken, the note says so.
pub fn parse_table(output: &str) -> Result<DataTable, String> {
    let json: Value = serde_json::from_str(between_markers(output)?).map_err(|e| crate::i18n::tf!("The kernel's table couldn't be read: {e}"))?;
    let names: Vec<String> = json.get("columns").and_then(Value::as_array).map(|a| a.iter().map(|n| n.as_str().unwrap_or("").to_string()).collect()).unwrap_or_default();
    let kinds: Vec<&str> = json.get("kinds").and_then(Value::as_array).map(|a| a.iter().map(|k| k.as_str().unwrap_or("")).collect()).unwrap_or_default();
    let columns = names.iter().enumerate().map(|(i, name)| DataColumn { name: name.clone(), kind: kind_of(kinds.get(i).copied().unwrap_or("")) }).collect();
    let rows: Vec<Vec<Cell>> = json.get("rows").and_then(Value::as_array).map(|rows| rows.iter().map(|row| row.as_array().map(|cells| cells.iter().map(cell_of).collect()).unwrap_or_default()).collect()).unwrap_or_default();
    let total = json.get("total").and_then(Value::as_u64).unwrap_or(rows.len() as u64) as usize;
    let note = (total > rows.len()).then(|| crate::i18n::tf!("The first {} of {} rows", crate::data_view::grouped(rows.len()), crate::data_view::grouped(total)));
    Ok(DataTable { columns, rows, note })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::jupyter::link::KernelSpec;
    use crate::notebook::Notebook;
    use crate::notebook_run::{Kernels, Session};
    use std::path::Path;

    #[test]
    fn the_listing_is_read_and_put_in_order() {
        let output = format!("noise\n{MARKER}[{{\"name\":\"zed\",\"type\":\"int\",\"size\":\"\",\"kind\":\"\",\"value\":\"5\"}},{{\"name\":\"Df\",\"type\":\"pandas.DataFrame\",\"size\":\"3 \\u00d7 2\",\"kind\":\"frame\",\"value\":\"\"}},{{\"name\":\"a\",\"type\":\"dict\",\"size\":\"2\",\"kind\":\"columns\",\"value\":\"\"}}]{MARKER}\n");
        let vars = parse_list(&output).unwrap();
        let names: Vec<&str> = vars.iter().map(|v| v.name.as_str()).collect();
        assert_eq!(names, ["a", "Df", "zed"]);
        assert_eq!(vars[1].shape, Shape::Frame);
        assert_eq!(vars[1].size, "3 \u{d7} 2");
        assert!(vars[0].shape.opens() && !vars[2].shape.opens());
        assert_eq!(vars[2].value, "5");
        assert!(parse_list("nothing here").is_err());
        assert!(parse_list(&format!("{MARKER}[")).is_err(), "cut short");
        assert!(parse_list(&format!("{MARKER}not json{MARKER}")).is_err());
        assert_eq!(parse_list(&format!("{MARKER}[]{MARKER}")).unwrap(), vec![]);
    }

    #[test]
    fn a_table_is_read_with_its_kinds_and_a_note_when_it_was_cut() {
        let output = format!("{MARKER}{{\"columns\":[\"a\",\"b\",\"c\"],\"kinds\":[\"int\",\"real\",\"unknown\"],\"rows\":[[1,1.5,\"x\"],[2,null,[1]]],\"total\":2}}{MARKER}");
        let table = parse_table(&output).unwrap();
        assert_eq!(table.columns.iter().map(|c| c.kind).collect::<Vec<_>>(), [ColumnKind::Integer, ColumnKind::Real, ColumnKind::Unknown]);
        assert_eq!(table.rows[0], vec![Cell::Int(1), Cell::Real(1.5), Cell::Text("x".into())]);
        assert_eq!(table.rows[1], vec![Cell::Int(2), Cell::Null, Cell::Text("[1]".into())]);
        assert!(table.note.is_none());
        let cut = format!("{MARKER}{{\"columns\":[\"a\"],\"kinds\":[\"int\"],\"rows\":[[1],[2]],\"total\":1204311}}{MARKER}");
        assert_eq!(parse_table(&cut).unwrap().note.as_deref(), Some("The first 2 of 1,204,311 rows"));
    }

    #[test]
    fn code_is_made_for_python_and_r_only() {
        assert!(list_code("python").is_some() && list_code("R").is_some() && list_code("julia").is_none());
        let python = fetch_code("python", "my\"df", 50).unwrap();
        assert!(python.contains("__limoniter_fetch(\"my\\\"df\", 50)"), "{python}");
        let r = fetch_code("R", "df", 7).unwrap();
        assert!(r.contains("get(\"df\", envir") && r.contains("limit <- 7"), "{r}");
        assert!(fetch_code("julia", "x", 1).is_none());
    }

    fn real_session(language: &str) -> Option<Session> {
        let Some(spec) = crate::jupyter::kernel_specs().into_iter().find(|s: &KernelSpec| s.language.eq_ignore_ascii_case(language)) else {
            println!("No {language} kernel here, so its variables weren't listed on a real one.");
            return None;
        };
        let mut session = Session::new(Kernels { start: crate::jupyter::start, list: crate::jupyter::kernel_specs }, Path::new("."));
        session.start(spec);
        Some(session)
    }

    /// Runs the cells, then asks for the listing and a fetch, waiting for everything.
    fn on_real_kernel(language: &str, source: &str, fetch: &str) -> Option<(Notebook, Vec<Var>, DataTable)> {
        let mut session = real_session(language)?;
        let mut nb = Notebook::new("kernel", "Kernel", language);
        nb.cells[0].source = source.into();
        let uid = nb.cells[0].uid;
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(90);
        let (mut asked, mut list_token, mut fetch_token) = (false, None, None);
        let (mut vars, mut table) = (None, None);
        while std::time::Instant::now() < deadline && (vars.is_none() || table.is_none()) {
            session.pump(&mut nb);
            if !asked && session.has_kernel() && session.status != crate::jupyter::link::KernelStatus::Starting {
                asked = session.run(&mut nb, &[uid]);
                list_token = Some(session.ask(list_code(language).unwrap()));
                fetch_token = Some(session.ask(fetch_code(language, fetch, 100_000).unwrap()));
            }
            for (token, answer) in session.take_answers() {
                if Some(token) == list_token {
                    vars = Some(parse_list(&answer.expect("the listing ran")).expect("the listing reads"));
                } else if Some(token) == fetch_token {
                    table = Some(parse_table(&answer.expect("the fetch ran")).expect("the table reads"));
                }
            }
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        assert!(session.problem.is_none(), "{:?}", session.problem);
        Some((nb, vars.expect("the variables came"), table.expect("the table came")))
    }

    #[test]
    fn a_real_python_kernel_lists_its_variables_and_gives_a_data_frame() {
        let source = "import pandas as pd, numpy as np, os\ndf = pd.DataFrame({'a': [1, 2, 3], 'b': [1.5, None, 2.5], 'c': ['x', None, 'z'], 'when': pd.to_datetime(['2026-01-02', '2026-03-04', None])})\narr = np.arange(6).reshape(3, 2)\nrows = [{'x': 1}, {'x': 2, 'y': 'q'}]\ncols = {'p': [1, 2], 'q': ['a', 'b']}\nn = 5\ndef f(): pass\nclass K: pass";
        let Some((nb, vars, table)) = on_real_kernel("python", source, "df") else { return };
        let names: Vec<&str> = vars.iter().map(|v| v.name.as_str()).collect();
        assert_eq!(names, ["arr", "cols", "df", "n", "rows"], "no modules, functions, classes or the kernel's own names");
        let df = vars.iter().find(|v| v.name == "df").unwrap();
        assert_eq!((df.shape, df.size.as_str(), df.type_name.as_str()), (Shape::Frame, "3 \u{d7} 4", "pandas.DataFrame"));
        assert_eq!(vars.iter().find(|v| v.name == "arr").unwrap().shape, Shape::Array);
        assert_eq!(vars.iter().find(|v| v.name == "rows").unwrap().shape, Shape::Rows);
        assert_eq!(vars.iter().find(|v| v.name == "cols").unwrap().shape, Shape::Columns);
        let n = vars.iter().find(|v| v.name == "n").unwrap();
        assert_eq!((n.shape, n.value.as_str()), (Shape::Plain, "5"));
        assert_eq!(table.columns.iter().map(|c| c.name.as_str()).collect::<Vec<_>>(), ["a", "b", "c", "when"]);
        assert_eq!(table.rows[0][..3], [Cell::Int(1), Cell::Real(1.5), Cell::Text("x".into())]);
        assert_eq!(table.rows[1][1], Cell::Null);
        assert_eq!(table.rows[0][3], Cell::Text("2026-01-02 00:00:00".into()));
        assert!(table.note.is_none());
        // The listing wasn't an execution: the cell is still the first and only one.
        assert_eq!(nb.cells[0].execution_count, Some(1));
    }

    #[test]
    fn a_real_python_kernel_gives_arrays_lists_of_dicts_and_dicts_of_lists() {
        let source = "import numpy as np\narr = np.arange(6).reshape(3, 2)\nrows = [{'x': 1}, {'x': 2, 'y': 'q'}]\ncols = {'p': [1, 2], 'q': ['a', 'b']}";
        for (name, columns, first) in [("arr", vec!["0", "1"], vec![Cell::Int(0), Cell::Int(1)]), ("rows", vec!["x", "y"], vec![Cell::Int(1), Cell::Null]), ("cols", vec!["p", "q"], vec![Cell::Int(1), Cell::Text("a".into())])] {
            let Some((_, _, table)) = on_real_kernel("python", source, name) else { return };
            assert_eq!(table.columns.iter().map(|c| c.name.as_str()).collect::<Vec<_>>(), columns, "{name}");
            assert_eq!(table.rows[0], first, "{name}");
        }
    }

    #[test]
    fn a_real_r_kernel_lists_its_variables_and_gives_a_data_frame() {
        let source = "df <- data.frame(a = c(1L, 2L, 3L), b = c(1.5, NA, 2.5), c = c('x', NA, 'z'), stringsAsFactors = FALSE)\nm <- matrix(1:6, 3, 2)\nn <- 5\nf <- function(x) x";
        let Some((nb, vars, table)) = on_real_kernel("R", source, "df") else { return };
        let names: Vec<&str> = vars.iter().map(|v| v.name.as_str()).collect();
        assert_eq!(names, ["df", "m", "n"], "functions are left out");
        let df = vars.iter().find(|v| v.name == "df").unwrap();
        assert_eq!((df.shape, df.size.as_str(), df.type_name.as_str()), (Shape::Frame, "3 \u{d7} 3", "data.frame"));
        assert_eq!(vars.iter().find(|v| v.name == "m").unwrap().shape, Shape::Array);
        assert_eq!(table.columns.iter().map(|c| (c.name.as_str(), c.kind)).collect::<Vec<_>>(), [("a", ColumnKind::Integer), ("b", ColumnKind::Real), ("c", ColumnKind::Unknown)]);
        assert_eq!(table.rows[0], vec![Cell::Int(1), Cell::Real(1.5), Cell::Text("x".into())]);
        assert_eq!(table.rows[1][1], Cell::Null);
        assert_eq!(nb.cells[0].execution_count, Some(1));
    }
}
