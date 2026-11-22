//! Databases: SQLite files, and PostgreSQL and MySQL (or MariaDB) servers, reached through Limoniter's own clients
//! for their wire protocols. This holds what's common to all three: a connection's settings, the results a query
//! gives back, the catalogue of what's in a database, and the worker thread each open connection runs on, so a
//! slow server never holds up typing.

pub mod md5;
pub mod mysql;
pub mod postgres;
pub mod sqlite;
pub mod statement;
#[cfg(test)]
pub mod test_server;
mod tls;
pub mod worker;

use crate::data_table::{Cell, ColumnKind, DataColumn, DataTable};
use serde_json::{Value, json};
use std::path::{Path, PathBuf};

/// The keychain's name for the database passwords Limoniter keeps.
const CREDENTIAL_SERVICE: &str = "Limoniter databases";

/// The most rows a query in the editor brings back; the rest are counted, not kept.
pub const MOST_ROWS: usize = 100_000;

/// The rows shown when a table is opened from the Database panel.
pub const TABLE_ROWS: usize = 1_000;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Engine {
    #[default]
    Sqlite,
    Postgres,
    MySql,
}

impl Engine {
    pub const ALL: [Engine; 3] = [Engine::Sqlite, Engine::Postgres, Engine::MySql];

    pub fn id(self) -> &'static str {
        match self {
            Engine::Sqlite => "sqlite",
            Engine::Postgres => "postgres",
            Engine::MySql => "mysql",
        }
    }

    pub fn from_id(id: &str) -> Option<Engine> {
        Engine::ALL.into_iter().find(|e| e.id() == id)
    }

    /// The name shown for it; not translated, as they're names.
    pub fn name(self) -> &'static str {
        match self {
            Engine::Sqlite => "SQLite",
            Engine::Postgres => "PostgreSQL",
            Engine::MySql => "MySQL / MariaDB",
        }
    }

    pub fn default_port(self) -> u16 {
        match self {
            Engine::Sqlite => 0,
            Engine::Postgres => 5432,
            Engine::MySql => 3306,
        }
    }

    /// Whether it's a server, reached over the network, rather than a file.
    pub fn is_server(self) -> bool {
        self != Engine::Sqlite
    }
}

/// Whether to talk to a server over TLS.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum TlsMode {
    /// Never: everything, passwords aside where the login hides them, goes in the clear.
    Disable,
    /// When the server offers it, else in the clear, as psql does by default.
    #[default]
    Prefer,
    /// Always: a server without TLS is refused.
    Require,
}

impl TlsMode {
    pub const ALL: [TlsMode; 3] = [TlsMode::Disable, TlsMode::Prefer, TlsMode::Require];

    pub fn id(self) -> &'static str {
        match self {
            TlsMode::Disable => "disable",
            TlsMode::Prefer => "prefer",
            TlsMode::Require => "require",
        }
    }

    pub fn from_id(id: &str) -> TlsMode {
        TlsMode::ALL.into_iter().find(|m| m.id() == id).unwrap_or_default()
    }

    pub fn label(self) -> &'static str {
        match self {
            TlsMode::Disable => "Never",
            TlsMode::Prefer => "When the server offers it",
            TlsMode::Require => "Always",
        }
    }
}

/// A database to connect to, as it's kept with the project or in the settings. The password isn't here: it's in
/// the system's keychain.
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct Connection {
    /// What it's called in the Database panel and the status bar; each connection's is different.
    pub name: String,
    pub engine: Engine,
    pub host: String,
    pub port: u16,
    pub user: String,
    pub database: String,
    pub tls: TlsMode,
    /// Accept a server's own (self-signed) certificate without an authority vouching for it, as psql's
    /// sslmode=require does.
    pub trust_own_certificate: bool,
    /// An SQLite database's file.
    pub file: PathBuf,
}

impl Connection {
    pub fn new(engine: Engine) -> Connection {
        Connection { engine, host: if engine.is_server() { "localhost".into() } else { String::new() }, port: engine.default_port(), trust_own_certificate: true, ..Connection::default() }
    }

    /// The key its password is kept under in the keychain: the same server and user share one password.
    pub fn password_key(&self) -> String {
        format!("{}://{}@{}:{}", self.engine.id(), self.user, self.host, self.port)
    }

    /// Where it is, in a few words, for a tooltip: "limtest@localhost:5432/limtest" or the file.
    pub fn place(&self) -> String {
        if self.engine.is_server() {
            let database = if self.database.is_empty() { String::new() } else { format!("/{}", self.database) };
            format!("{}@{}:{}{database}", self.user, self.host, self.port)
        } else {
            self.file.display().to_string()
        }
    }

    /// What's missing before it can be connected to, if anything.
    pub fn problem(&self) -> Option<String> {
        if self.name.trim().is_empty() {
            return Some(crate::i18n::tr("Give the connection a name.").into());
        }
        if self.engine.is_server() {
            if self.host.trim().is_empty() {
                return Some(crate::i18n::tr("Say which computer the server is on.").into());
            }
            if self.user.trim().is_empty() {
                return Some(crate::i18n::tr("Say which user to log in as.").into());
            }
        } else if self.file.as_os_str().is_empty() {
            return Some(crate::i18n::tr("Choose the database's file.").into());
        }
        None
    }

    /// As JSON, with an SQLite file relative to `base` when there is one (a project's folder).
    pub fn to_json(&self, base: Option<&Path>) -> Value {
        let mut value = json!({ "name": self.name, "engine": self.engine.id() });
        if self.engine.is_server() {
            value["host"] = json!(self.host);
            value["port"] = json!(self.port);
            value["user"] = json!(self.user);
            value["database"] = json!(self.database);
            value["tls"] = json!(self.tls.id());
            value["trust_own_certificate"] = json!(self.trust_own_certificate);
        } else {
            value["file"] = json!(base.map_or_else(|| self.file.to_string_lossy().replace('\\', "/"), |base| crate::project::stored(base, &self.file)));
        }
        value
    }

    pub fn from_json(value: &Value, base: Option<&Path>) -> Option<Connection> {
        let text = |name: &str| value.get(name).and_then(Value::as_str).unwrap_or_default().to_string();
        let engine = Engine::from_id(value.get("engine")?.as_str()?)?;
        let file = text("file");
        let file = match base {
            Some(base) if !file.is_empty() && Path::new(&file).is_relative() => base.join(file),
            _ => PathBuf::from(file),
        };
        let port = value.get("port").and_then(Value::as_u64).and_then(|p| u16::try_from(p).ok()).unwrap_or(engine.default_port());
        Some(Connection {
            name: text("name"),
            engine,
            host: text("host"),
            port,
            user: text("user"),
            database: text("database"),
            tls: TlsMode::from_id(&text("tls")),
            trust_own_certificate: value.get("trust_own_certificate").and_then(Value::as_bool).unwrap_or(true),
            file,
        })
    }

    /// Reads one from a web address: postgres://user:password@host:port/database, mysql://…, or sqlite:///path.
    /// The password, if there is one, comes back beside it, so tests can be pointed at a server by one setting.
    #[cfg(test)]
    pub fn from_url(url: &str) -> Option<(Connection, Option<String>)> {
        let (scheme, rest) = url.split_once("://")?;
        let engine = match scheme {
            "postgres" | "postgresql" => Engine::Postgres,
            "mysql" | "mariadb" => Engine::MySql,
            "sqlite" => return Some((Connection { name: "SQLite".into(), file: PathBuf::from(rest), ..Connection::new(Engine::Sqlite) }, None)),
            _ => return None,
        };
        let (rest, query) = rest.split_once('?').unwrap_or((rest, ""));
        let (login, place) = rest.rsplit_once('@').unwrap_or(("", rest));
        let (user, password) = login.split_once(':').map_or((login, None), |(u, p)| (u, Some(p.to_string())));
        let (address, database) = place.split_once('/').unwrap_or((place, ""));
        let (host, port) = match address.rsplit_once(':') {
            Some((host, port)) if !port.contains(']') => (host, port.parse().ok()?),
            _ => (address, engine.default_port()),
        };
        let mut connection = Connection { name: format!("{}@{host}", user), host: host.to_string(), port, user: user.to_string(), database: database.to_string(), ..Connection::new(engine) };
        for pair in query.split('&') {
            match pair.split_once('=') {
                Some(("sslmode", mode)) => connection.tls = TlsMode::from_id(mode),
                Some(("sslcheck", "1")) => connection.trust_own_certificate = false,
                _ => {}
            }
        }
        Some((connection, password))
    }
}

/// A list of connections as JSON, for a project's file.
pub fn list_to_json(list: &[Connection], base: Option<&Path>) -> Value {
    Value::Array(list.iter().map(|c| c.to_json(base)).collect())
}

pub fn list_from_json(value: Option<&Value>, base: Option<&Path>) -> Vec<Connection> {
    value.and_then(Value::as_array).map_or_else(Vec::new, |items| items.iter().filter_map(|item| Connection::from_json(item, base)).collect())
}

// ---------- Passwords ----------

/// The password kept for a connection in the system's keychain, if there is one and the keychain can be reached.
pub fn stored_password(connection: &Connection) -> Option<String> {
    keyring::Entry::new(CREDENTIAL_SERVICE, &connection.password_key()).ok()?.get_password().ok()
}

/// Keeps a password in the keychain. It's never written to a file: if there's no keychain, this says why.
pub fn store_password(connection: &Connection, password: &str) -> Result<(), String> {
    keyring::Entry::new(CREDENTIAL_SERVICE, &connection.password_key()).and_then(|entry| entry.set_password(password)).map_err(|e| e.to_string())
}

pub fn forget_password(connection: &Connection) {
    if let Ok(entry) = keyring::Entry::new(CREDENTIAL_SERVICE, &connection.password_key()) {
        let _ = entry.delete_credential();
    }
}

// ---------- Results ----------

/// What one statement gave back.
#[derive(Clone, Debug, PartialEq)]
pub enum Outcome {
    /// Rows. `more` counts the ones left out past the most kept.
    Rows { table: DataTable, more: u64 },
    /// A statement without rows: what it was ("INSERT", "CREATE TABLE") and how many rows it changed, when said.
    Done { command: String, rows: Option<u64> },
}

/// A message from the server that isn't an error: a notice, a warning.
pub type Notice = String;

/// Everything a query gave back.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Reply {
    pub outcomes: Vec<Outcome>,
    pub notices: Vec<Notice>,
}

/// Why a query, or connecting, failed.
#[derive(Clone, Debug, PartialEq, Default)]
pub struct DbError {
    pub message: String,
    /// Where in the SQL it went wrong, as a count of characters from its start, when the server said.
    pub position: Option<usize>,
    /// The server's code for it (PostgreSQL's SQLSTATE, MySQL's number), when there is one.
    pub code: Option<String>,
    /// Set when the connection itself is gone, so it has to be made again.
    pub lost: bool,
    /// What the server gave back before the error, from the statements that worked.
    pub before: Reply,
}

impl DbError {
    pub fn new(message: impl Into<String>) -> DbError {
        DbError { message: message.into(), ..DbError::default() }
    }

    pub fn lost(message: impl Into<String>) -> DbError {
        DbError { message: message.into(), lost: true, ..DbError::default() }
    }

    pub fn io(e: std::io::Error) -> DbError {
        DbError::lost(crate::i18n::tf!("The connection to the server was lost: {e}"))
    }
}

impl std::fmt::Display for DbError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}

/// Rows being gathered, keeping no more than the most asked for and counting the rest.
pub struct Gathering {
    pub table: DataTable,
    pub most: usize,
    pub more: u64,
}

impl Gathering {
    pub fn new(columns: Vec<DataColumn>, most: usize) -> Gathering {
        Gathering { table: DataTable { columns, rows: Vec::new(), note: None }, most, more: 0 }
    }

    pub fn wants_more(&self) -> bool {
        self.table.rows.len() < self.most
    }

    pub fn push(&mut self, row: Vec<Cell>) {
        if self.wants_more() {
            self.table.rows.push(row);
        } else {
            self.more += 1;
        }
    }

    pub fn finish(self) -> Outcome {
        Outcome::Rows { table: self.table, more: self.more }
    }
}

/// A value given as text, made into the cell its column's kind says: numbers and truths as themselves, the rest
/// (dates, decimals, JSON) kept as text with the kind saying what they are.
pub fn cell_from_text(kind: ColumnKind, text: &str) -> Cell {
    match kind {
        ColumnKind::Integer => text.parse().map_or_else(|_| Cell::Text(text.to_string()), Cell::Int),
        ColumnKind::Real => match text.parse::<f64>() {
            // Decimals stay as written, so 12.50 keeps its places and a long one keeps its digits.
            Ok(x) if text.contains(['e', 'E', 'n', 'N', 'i', 'I']) || x.to_string() == text => Cell::Real(x),
            _ => Cell::Text(text.to_string()),
        },
        ColumnKind::Bool => match text {
            "t" | "true" | "1" => Cell::Bool(true),
            "f" | "false" | "0" => Cell::Bool(false),
            _ => Cell::Text(text.to_string()),
        },
        _ => Cell::Text(text.to_string()),
    }
}

// ---------- The catalogue ----------

/// A database's schemas (PostgreSQL), databases (MySQL) or attached files (SQLite), and what's in them.
#[derive(Clone, Debug, PartialEq, Default)]
pub struct Schema {
    pub name: String,
    pub tables: Vec<Table>,
}

#[derive(Clone, Debug, PartialEq, Default)]
pub struct Table {
    pub name: String,
    pub view: bool,
    pub columns: Vec<Column>,
}

#[derive(Clone, Debug, PartialEq, Default)]
pub struct Column {
    pub name: String,
    /// Its type as the database names it: "integer", "varchar(40)".
    pub kind: String,
    pub primary: bool,
    /// The table and column it refers to, for a foreign key.
    pub refers_to: Option<String>,
}

/// Puts catalogue rows (schema, table, is it a view, column, type) into schemas and tables, in the order given.
pub fn catalogue_from_rows(rows: impl IntoIterator<Item = (String, String, bool, String, String)>) -> Vec<Schema> {
    let mut schemas: Vec<Schema> = Vec::new();
    for (schema, table, view, column, kind) in rows {
        if schemas.last().is_none_or(|s| s.name != schema) {
            schemas.push(Schema { name: schema, tables: Vec::new() });
        }
        let tables = &mut schemas.last_mut().expect("just pushed").tables;
        if tables.last().is_none_or(|t| t.name != table) {
            tables.push(Table { name: table, view, columns: Vec::new() });
        }
        if !column.is_empty() {
            tables.last_mut().expect("just pushed").columns.push(Column { name: column, kind, ..Column::default() });
        }
    }
    schemas
}

/// Marks the keys on a catalogue: (schema, table, column) for primary keys, and the same with what they refer to
/// for foreign keys.
pub fn mark_keys(schemas: &mut [Schema], primary: &[(String, String, String)], foreign: &[(String, String, String, String)]) {
    for schema in schemas.iter_mut() {
        for table in &mut schema.tables {
            for column in &mut table.columns {
                column.primary = primary.iter().any(|(s, t, c)| *s == schema.name && *t == table.name && *c == column.name);
                column.refers_to = foreign.iter().find(|(s, t, c, _)| *s == schema.name && *t == table.name && *c == column.name).map(|(_, _, _, to)| to.clone());
            }
        }
    }
}

/// The text of a cell for the catalogue's queries: empty for nothing.
fn text_of(cell: Option<&Cell>) -> String {
    cell.map(Cell::text).unwrap_or_default()
}

/// The first rows of a reply, for the catalogue's own queries.
fn rows_of(reply: Reply) -> Vec<Vec<Cell>> {
    reply.outcomes.into_iter().find_map(|o| if let Outcome::Rows { table, .. } = o { Some(table.rows) } else { None }).unwrap_or_default()
}

// ---------- Clients ----------

/// An open connection to a database.
pub trait Client: Send {
    /// Runs some SQL, which may be several statements, keeping at most `most` rows of each result.
    fn run(&mut self, sql: &str, most: usize) -> Result<Reply, DbError>;
    /// What's in the database.
    fn catalogue(&mut self) -> Result<Vec<Schema>, DbError>;
    /// Something another thread can use to stop the query running.
    fn stopper(&self) -> Stopper;
    /// What the server is, for the panel's tooltip: "PostgreSQL 14.24", "MariaDB 11.8.3".
    fn server(&self) -> String;
    /// Changes the database the connection uses, where the server allows that (MySQL's USE).
    fn use_database(&mut self, database: &str) -> Result<(), DbError> {
        let _ = database;
        Err(DbError::new(crate::i18n::tr("This connection can't change its database; add another connection for the other one.")))
    }
}

/// Stops a running query from another thread.
pub enum Stopper {
    /// PostgreSQL: a CancelRequest on a new connection.
    Postgres { connection: Connection, process: u32, secret: u32 },
    /// MySQL: KILL QUERY from a second connection.
    MySql { connection: Connection, password: String, thread: u32 },
    Sqlite(rusqlite::InterruptHandle),
}

impl Stopper {
    pub fn stop(&self) -> Result<(), String> {
        match self {
            Stopper::Postgres { connection, process, secret } => postgres::cancel(connection, *process, *secret),
            Stopper::MySql { connection, password, thread } => mysql::kill_query(connection, password, *thread),
            Stopper::Sqlite(handle) => {
                handle.interrupt();
                Ok(())
            }
        }
    }
}

/// Connects to a database, with the password given (none for SQLite).
pub fn connect(connection: &Connection, password: &str) -> Result<Box<dyn Client>, DbError> {
    Ok(match connection.engine {
        Engine::Sqlite => Box::new(sqlite::Sqlite::open(connection)?),
        Engine::Postgres => Box::new(postgres::Postgres::connect(connection, password)?),
        Engine::MySql => Box::new(mysql::MySql::connect(connection, password)?),
    })
}

/// The SQL that shows a table's first rows.
pub fn first_rows_sql(engine: Engine, schema: &str, table: &str, rows: usize) -> String {
    format!("SELECT * FROM {}.{} LIMIT {rows}", quoted_name(schema, engine), quoted_name(table, engine))
}

/// A name quoted as SQL wants it: "name" in standard SQL, `name` in MySQL.
pub fn quoted_name(name: &str, engine: Engine) -> String {
    match engine {
        Engine::MySql => format!("`{}`", name.replace('`', "``")),
        _ => format!("\"{}\"", name.replace('"', "\"\"")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_connection_goes_to_json_and_back() {
        let server = Connection { name: "Live".into(), engine: Engine::Postgres, host: "db.example".into(), port: 6543, user: "me".into(), database: "shop".into(), tls: TlsMode::Require, trust_own_certificate: false, file: PathBuf::new() };
        assert_eq!(Connection::from_json(&server.to_json(None), None), Some(server.clone()));
        let text = server.to_json(None).to_string();
        assert!(!text.contains("password"), "no password in {text}");
        let base = Path::new("/projects/shop");
        let file = Connection { name: "Local".into(), file: base.join("data/shop.db"), ..Connection::new(Engine::Sqlite) };
        let value = file.to_json(Some(base));
        assert_eq!(value["file"], "data/shop.db", "relative to the project");
        assert_eq!(Connection::from_json(&value, Some(base)), Some(file));
        assert_eq!(list_from_json(Some(&list_to_json(std::slice::from_ref(&server), None)), None), vec![server]);
    }

    #[test]
    fn a_web_address_says_where_and_who() {
        let (c, password) = Connection::from_url("postgres://limtest:secret@127.0.0.1:5432/limtest").unwrap();
        assert_eq!((c.engine, c.host.as_str(), c.port, c.user.as_str(), c.database.as_str(), password.as_deref()), (Engine::Postgres, "127.0.0.1", 5432, "limtest", "limtest", Some("secret")));
        let (c, password) = Connection::from_url("mysql://root@db/shop?sslmode=require").unwrap();
        assert_eq!((c.engine, c.port, c.tls, password), (Engine::MySql, 3306, TlsMode::Require, None));
        assert!(Connection::from_url("ftp://x").is_none());
    }

    #[test]
    fn values_become_cells_of_their_kind() {
        assert_eq!(cell_from_text(ColumnKind::Integer, "42"), Cell::Int(42));
        assert_eq!(cell_from_text(ColumnKind::Integer, "18446744073709551615"), Cell::Text("18446744073709551615".into()), "too big for an i64 stays text");
        assert_eq!(cell_from_text(ColumnKind::Real, "2.5"), Cell::Real(2.5));
        assert_eq!(cell_from_text(ColumnKind::Real, "12.50"), Cell::Text("12.50".into()), "a decimal keeps its places");
        assert!(matches!(cell_from_text(ColumnKind::Real, "NaN"), Cell::Real(x) if x.is_nan()));
        assert_eq!(cell_from_text(ColumnKind::Bool, "t"), Cell::Bool(true));
        assert_eq!(cell_from_text(ColumnKind::Date, "2026-11-22"), Cell::Text("2026-11-22".into()));
    }

    #[test]
    fn catalogue_rows_become_schemas_and_tables_with_their_keys() {
        let rows = [("public", "a", false, "id", "integer"), ("public", "a", false, "b_id", "integer"), ("public", "v", true, "id", "integer"), ("other", "b", false, "id", "bigint"), ("other", "empty", false, "", "")];
        let mut schemas = catalogue_from_rows(rows.map(|(s, t, v, c, k)| (s.to_string(), t.to_string(), v, c.to_string(), k.to_string())));
        mark_keys(&mut schemas, &[("public".into(), "a".into(), "id".into())], &[("public".into(), "a".into(), "b_id".into(), "other.b(id)".into())]);
        assert_eq!(schemas.len(), 2);
        assert_eq!(schemas[0].tables.iter().map(|t| (t.name.as_str(), t.view, t.columns.len())).collect::<Vec<_>>(), [("a", false, 2), ("v", true, 1)]);
        assert!(schemas[0].tables[0].columns[0].primary);
        assert_eq!(schemas[0].tables[0].columns[1].refers_to.as_deref(), Some("other.b(id)"));
        assert!(!schemas[0].tables[1].columns[0].primary, "a view's id isn't the table's key");
        assert!(schemas[1].tables[1].columns.is_empty());
    }

    #[test]
    fn names_are_quoted_for_each_engine() {
        assert_eq!(quoted_name("my \"t\"", Engine::Postgres), "\"my \"\"t\"\"\"");
        assert_eq!(quoted_name("a`b", Engine::MySql), "`a``b`");
        assert_eq!(first_rows_sql(Engine::MySql, "shop", "order", 1000), "SELECT * FROM `shop`.`order` LIMIT 1000");
    }
}
