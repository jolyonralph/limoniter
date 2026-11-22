//! SQLite databases, through rusqlite with SQLite built in. Statements are run one after another, as the sqlite3
//! program runs them, so a selection of several gives each its own result.

use super::{Client, Connection, DbError, Gathering, Outcome, Reply, Schema, Stopper};
use crate::data_table::{Cell, ColumnKind, DataColumn};
use rusqlite::types::ValueRef;

pub struct Sqlite {
    database: rusqlite::Connection,
}

/// What a column holds, from its declared type, by SQLite's own rules for type affinity (with dates, times,
/// truths and JSON, which SQLite keeps as text or numbers, told apart by the names people give them).
pub fn kind_of(declared: Option<&str>) -> ColumnKind {
    let Some(declared) = declared else {
        return ColumnKind::Unknown;
    };
    let name = declared.to_ascii_uppercase();
    if name.contains("DATETIME") || name.contains("TIMESTAMP") {
        ColumnKind::DateTime
    } else if name.contains("DATE") {
        ColumnKind::Date
    } else if name.contains("BOOL") {
        ColumnKind::Bool
    } else if name.contains("JSON") {
        ColumnKind::Json
    } else if name.contains("INT") {
        ColumnKind::Integer
    } else if name.contains("CHAR") || name.contains("CLOB") || name.contains("TEXT") {
        ColumnKind::Text
    } else if name.contains("BLOB") {
        ColumnKind::Binary
    } else if name.contains("REAL") || name.contains("FLOA") || name.contains("DOUB") || name.contains("NUMERIC") || name.contains("DECIMAL") {
        ColumnKind::Real
    } else {
        ColumnKind::Unknown
    }
}

fn cell_of(value: ValueRef<'_>, kind: ColumnKind) -> Cell {
    match value {
        ValueRef::Null => Cell::Null,
        ValueRef::Integer(n) if kind == ColumnKind::Bool => Cell::Bool(n != 0),
        ValueRef::Integer(n) => Cell::Int(n),
        ValueRef::Real(x) => Cell::Real(x),
        ValueRef::Text(text) => Cell::Text(String::from_utf8_lossy(text).into_owned()),
        ValueRef::Blob(bytes) => Cell::Bytes(bytes.to_vec()),
    }
}

/// rusqlite's error as Limoniter's, with the place in the SQL when SQLite gives one (in bytes; made characters).
fn error_from(problem: rusqlite::Error, sql: &str, start: usize) -> DbError {
    match problem {
        rusqlite::Error::SqlInputError { msg, offset, .. } => {
            let at = start + usize::try_from(offset).unwrap_or(0);
            DbError { message: msg, position: sql.get(..at.min(sql.len())).map(|before| before.chars().count()), ..DbError::default() }
        }
        rusqlite::Error::SqliteFailure(code, message) if code.code == rusqlite::ErrorCode::OperationInterrupted => DbError::new(message.unwrap_or_else(|| crate::i18n::tr("The query was stopped.").into())),
        other => DbError::new(other.to_string()),
    }
}

impl Sqlite {
    pub fn open(connection: &Connection) -> Result<Sqlite, DbError> {
        // A file that isn't there isn't made: a mistyped name would otherwise give an empty database.
        if !connection.file.exists() {
            return Err(DbError::lost(crate::i18n::tf!("There's no database at {}.", connection.file.display())));
        }
        let database = rusqlite::Connection::open(&connection.file).map_err(|e| DbError::lost(e.to_string()))?;
        Ok(Sqlite { database })
    }

    /// An SQLite database kept in memory, for tests.
    #[cfg(test)]
    pub fn in_memory() -> Sqlite {
        Sqlite { database: rusqlite::Connection::open_in_memory().expect("an in-memory database") }
    }

    /// Runs one statement, which starts `start` bytes into the whole SQL.
    fn one(&self, statement: &mut rusqlite::Statement<'_>, sql: &str, most: usize) -> Result<Outcome, rusqlite::Error> {
        if statement.column_count() == 0 {
            let changed = statement.raw_execute()?;
            let command = sql.split_whitespace().next().unwrap_or_default().trim_end_matches(';').to_ascii_uppercase();
            let counted = matches!(command.as_str(), "INSERT" | "UPDATE" | "DELETE" | "REPLACE");
            return Ok(Outcome::Done { command, rows: counted.then_some(changed as u64) });
        }
        let columns: Vec<DataColumn> = statement.columns().map(|c| DataColumn { name: c.name().to_string(), kind: kind_of(c.decl_type()) }).collect();
        let kinds: Vec<ColumnKind> = columns.iter().map(|c| c.kind).collect();
        let mut gathering = Gathering::new(columns, most);
        let mut rows = statement.raw_query();
        while let Some(row) = rows.next()? {
            if gathering.wants_more() {
                gathering.push((0..kinds.len()).map(|i| row.get_ref(i).map_or(Cell::Null, |value| cell_of(value, kinds[i]))).collect());
            } else {
                gathering.more += 1;
            }
        }
        Ok(gathering.finish())
    }
}

/// Whether SQLite stopped because the statement wasn't finished: a semicolon inside a trigger's body that the
/// splitting took for the end.
fn incomplete(problem: &rusqlite::Error) -> bool {
    problem.to_string().contains("incomplete input")
}

impl Client for Sqlite {
    fn run(&mut self, sql: &str, most: usize) -> Result<Reply, DbError> {
        let mut reply = Reply::default();
        let pieces = super::statement::split(sql, super::Engine::Sqlite);
        let mut i = 0;
        while i < pieces.len() {
            let mut range = pieces[i].clone();
            let prepared = loop {
                match self.database.prepare(&sql[range.clone()]) {
                    Err(problem) if incomplete(&problem) && i + 1 < pieces.len() => {
                        i += 1;
                        range.end = pieces[i].end;
                    }
                    other => break other,
                }
            };
            let result = prepared.and_then(|mut statement| self.one(&mut statement, &sql[range.clone()], most));
            match result {
                Ok(outcome) => reply.outcomes.push(outcome),
                Err(problem) => {
                    let mut problem = error_from(problem, sql, range.start);
                    problem.before = reply;
                    return Err(problem);
                }
            }
            i += 1;
        }
        Ok(reply)
    }

    fn catalogue(&mut self) -> Result<Vec<Schema>, DbError> {
        let fail = |e: rusqlite::Error| DbError::new(e.to_string());
        let mut schemas = Vec::new();
        let names: Vec<String> = self.database.prepare("SELECT name FROM pragma_database_list ORDER BY seq").map_err(fail)?.query_map([], |r| r.get(0)).map_err(fail)?.filter_map(Result::ok).collect();
        for schema in names {
            let mut rows = Vec::new();
            let (mut primary, mut foreign) = (Vec::new(), Vec::new());
            let quoted = super::quoted_name(&schema, super::Engine::Sqlite);
            let tables: Vec<(String, bool)> = self
                .database
                .prepare(&format!("SELECT name, type = 'view' FROM {quoted}.sqlite_schema WHERE type IN ('table', 'view') AND name NOT LIKE 'sqlite\\_%' ESCAPE '\\' ORDER BY name"))
                .map_err(fail)?
                .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))
                .map_err(fail)?
                .filter_map(Result::ok)
                .collect();
            for (table, view) in tables {
                let columns: Vec<(String, String, i64)> = self.database.prepare("SELECT name, type, pk FROM pragma_table_info(?1, ?2) ORDER BY cid").map_err(fail)?.query_map([&table, &schema], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?))).map_err(fail)?.filter_map(Result::ok).collect();
                if columns.is_empty() {
                    rows.push((schema.clone(), table.clone(), view, String::new(), String::new()));
                }
                for (column, kind, pk) in columns {
                    if pk > 0 && !view {
                        primary.push((schema.clone(), table.clone(), column.clone()));
                    }
                    rows.push((schema.clone(), table.clone(), view, column, kind));
                }
                let keys: Vec<(String, String, String)> = self.database.prepare("SELECT \"from\", \"table\", coalesce(\"to\", '') FROM pragma_foreign_key_list(?1, ?2)").map_err(fail)?.query_map([&table, &schema], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?))).map_err(fail)?.filter_map(Result::ok).collect();
                for (from, to_table, to_column) in keys {
                    foreign.push((schema.clone(), table.clone(), from, format!("{to_table}({to_column})")));
                }
            }
            let mut found = super::catalogue_from_rows(rows);
            super::mark_keys(&mut found, &primary, &foreign);
            if found.is_empty() {
                found.push(Schema { name: schema, tables: Vec::new() });
            }
            schemas.extend(found);
        }
        Ok(schemas)
    }

    fn stopper(&self) -> Stopper {
        Stopper::Sqlite(self.database.get_interrupt_handle())
    }

    fn server(&self) -> String {
        format!("SQLite {}", rusqlite::version())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rows(reply: &Reply, index: usize) -> &crate::data_table::DataTable {
        match &reply.outcomes[index] {
            Outcome::Rows { table, .. } => table,
            other => panic!("rows, not {other:?}"),
        }
    }

    #[test]
    fn declared_types_say_what_columns_hold() {
        let kinds = ["INTEGER", "varchar(20)", "BLOB", "DOUBLE PRECISION", "DATE", "DATETIME", "BOOLEAN", "JSON", "whatever"].map(|t| kind_of(Some(t)));
        assert_eq!(kinds, [ColumnKind::Integer, ColumnKind::Text, ColumnKind::Binary, ColumnKind::Real, ColumnKind::Date, ColumnKind::DateTime, ColumnKind::Bool, ColumnKind::Json, ColumnKind::Unknown]);
        assert_eq!(kind_of(None), ColumnKind::Unknown);
    }

    #[test]
    fn statements_run_one_after_another_with_rows_and_counts() {
        let mut db = Sqlite::in_memory();
        let reply = db.run("CREATE TABLE t (id INTEGER PRIMARY KEY, name TEXT, ok BOOLEAN, data BLOB, x REAL);\nINSERT INTO t VALUES (1, 'a', 1, x'00ff', 2.5), (2, NULL, 0, NULL, NULL);\nSELECT * FROM t ORDER BY id; UPDATE t SET name = 'b'", 100).unwrap();
        assert_eq!(reply.outcomes.len(), 4);
        assert_eq!(reply.outcomes[0], Outcome::Done { command: "CREATE".into(), rows: None });
        assert_eq!(reply.outcomes[1], Outcome::Done { command: "INSERT".into(), rows: Some(2) });
        let table = rows(&reply, 2);
        assert_eq!(table.rows[0], [Cell::Int(1), Cell::Text("a".into()), Cell::Bool(true), Cell::Bytes(vec![0, 255]), Cell::Real(2.5)]);
        assert_eq!(table.rows[1], [Cell::Int(2), Cell::Null, Cell::Bool(false), Cell::Null, Cell::Null]);
        assert_eq!(reply.outcomes[3], Outcome::Done { command: "UPDATE".into(), rows: Some(2) });
        // Rows past the most kept are counted.
        let reply = db.run("WITH RECURSIVE n(i) AS (SELECT 1 UNION ALL SELECT i + 1 FROM n WHERE i < 10) SELECT i FROM n", 4).unwrap();
        assert_eq!(reply.outcomes[0], Outcome::Rows { table: crate::data_table::DataTable { columns: vec![DataColumn { name: "i".into(), kind: ColumnKind::Unknown }], rows: (1..=4).map(|i| vec![Cell::Int(i)]).collect(), note: None }, more: 6 });
    }

    #[test]
    fn a_trigger_with_semicolons_in_its_body_is_one_statement() {
        let mut db = Sqlite::in_memory();
        let reply = db.run("CREATE TABLE a (n INT); CREATE TABLE log (n INT);\nCREATE TRIGGER t AFTER INSERT ON a BEGIN INSERT INTO log VALUES (new.n); INSERT INTO log VALUES (new.n * 10); END;\nINSERT INTO a VALUES (2); SELECT n FROM log ORDER BY n", 10).unwrap();
        assert_eq!(rows(&reply, 4).rows, [[Cell::Int(2)], [Cell::Int(20)]]);
    }

    #[test]
    fn an_error_says_where_it_is_and_keeps_what_worked_before_it() {
        let mut db = Sqlite::in_memory();
        let sql = "SELECT 1;\nSELECT é FROM nowhere";
        let problem = db.run(sql, 10).unwrap_err();
        assert!(problem.message.contains("nowhere") || problem.message.contains("é"), "{}", problem.message);
        assert_eq!(problem.before.outcomes.len(), 1);
        let problem = db.run("SELECT 1;\nSELECT 'é', FORM t", 10).unwrap_err();
        let at = problem.position.expect("a place");
        assert_eq!("SELECT 1;\nSELECT 'é', FORM t".chars().skip(at).take(4).collect::<String>(), "FORM", "{problem:?}");
    }

    #[test]
    fn the_catalogue_lists_tables_views_columns_and_keys() {
        let mut db = Sqlite::in_memory();
        db.run("CREATE TABLE parent (id INTEGER PRIMARY KEY, name TEXT); CREATE TABLE child (id INTEGER PRIMARY KEY, parent INT REFERENCES parent(id)); CREATE VIEW v AS SELECT name FROM parent", 0).unwrap();
        let schemas = db.catalogue().unwrap();
        assert_eq!(schemas[0].name, "main");
        let tables: Vec<(&str, bool)> = schemas[0].tables.iter().map(|t| (t.name.as_str(), t.view)).collect();
        assert_eq!(tables, [("child", false), ("parent", false), ("v", true)]);
        let child = &schemas[0].tables[0];
        assert!(child.columns[0].primary);
        assert_eq!(child.columns[1].refers_to.as_deref(), Some("parent(id)"));
        assert_eq!(child.columns[1].kind, "INT");
    }

    #[test]
    fn a_long_query_is_stopped_from_another_thread() {
        let mut db = Sqlite::in_memory();
        let stopper = db.stopper();
        let stopping = std::thread::spawn(move || {
            std::thread::sleep(std::time::Duration::from_millis(200));
            stopper.stop()
        });
        let began = std::time::Instant::now();
        let problem = db.run("WITH RECURSIVE n(i) AS (SELECT 1 UNION ALL SELECT i + 1 FROM n) SELECT count(*) FROM n", 10).unwrap_err();
        stopping.join().unwrap().unwrap();
        assert!(began.elapsed() < std::time::Duration::from_secs(10), "{problem:?}");
    }

    #[test]
    fn a_file_that_isnt_there_isnt_made() {
        let missing = std::env::temp_dir().join("limoniter-no-such-database.db");
        let connection = Connection { file: missing.clone(), ..Connection::new(crate::database::Engine::Sqlite) };
        assert!(Sqlite::open(&connection).is_err());
        assert!(!missing.exists());
    }
}
