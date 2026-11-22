//! The window's side of databases: the Database panel with its tree of connections, schemas, tables and columns;
//! the box connections are added and changed in; running the SQL statement at the cursor with Ctrl+Enter on the
//! connection chosen for the file; and that choice in the status bar. Connecting and running happen on each
//! connection's own thread (`database::worker`), and what comes back is picked up each frame.

use super::*;
use crate::database::worker::{Event, Job, Worker};
use crate::database::{self as db, Connection, Engine, Outcome, TlsMode};
use std::collections::HashMap;
use std::time::Instant;

/// Where the status bar's menu leaves the connection chosen for the file in front: its name, or empty for none.
pub(super) const STATUS_PICKED: &str = "database-status-picked";

/// The Database menu's commands.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Act {
    /// Run the SQL statement at the cursor, or the selection (Ctrl+Enter in an SQL file).
    Run,
    /// Stop the query running for the file in front.
    Stop,
    /// Add a connection.
    Add,
}

impl Act {
    pub(super) fn works_without_file(self) -> bool {
        self == Act::Add
    }
}

/// What a query was sent for, so its reply goes to the right place.
enum Purpose {
    /// A table's first rows, opened from the panel.
    Table { title: String, table: String },
    /// SQL from a file: the document, where in it the SQL starts (line, character), and the SQL.
    Editor { doc: u64, start: (usize, usize), sql: String },
}

/// An open connection, or one that was tried and failed.
struct Session {
    worker: Option<Worker>,
    engine: Engine,
    /// What the server is, once connected.
    server: Option<String>,
    /// Why connecting failed, or why the connection was lost.
    problem: Option<String>,
    /// What's in the database, once read: or why it couldn't be.
    catalogue: Option<Result<Vec<db::Schema>, String>>,
    /// The queries sent and not yet answered, by their tags.
    waiting: Vec<(u64, Purpose)>,
    /// The database chosen in the panel for a MySQL server's queries, when it's been changed.
    using: Option<String>,
}

impl Session {
    fn connecting(&self) -> bool {
        self.worker.is_some() && self.server.is_none()
    }

    fn busy(&self) -> bool {
        self.connecting() || !self.waiting.is_empty() || (self.worker.is_some() && self.catalogue.is_none())
    }
}

#[derive(Default)]
pub(super) struct DatabaseState {
    /// The connections kept in the settings, used when no project is open.
    pub kept: Vec<Connection>,
    /// The connection chosen for each SQL file, by its path.
    pub for_file: Vec<(PathBuf, String)>,
    /// The same for files not saved yet, by the document.
    for_doc: HashMap<u64, String>,
    /// Open connections, by the connection's name.
    sessions: HashMap<String, Session>,
    /// Passwords the keychain couldn't keep, by their keys: in memory only, until Limoniter closes.
    passwords: HashMap<String, String>,
    next_tag: u64,
    /// The problems marked in each document by the last query run from it, to take away when it's run again.
    marked: Vec<(u64, Vec<(usize, String)>)>,
    /// For a picture: every connection and schema shown opened.
    open_all: bool,
    /// The window's context, so a connection's thread can wake it, and a picture's steps can connect.
    pub ctx: Option<Context>,
}

impl DatabaseState {
    pub(super) fn from_settings(settings: &str) -> Self {
        let mut state = DatabaseState::default();
        for line in settings.lines() {
            if let Some(json) = line.strip_prefix("database_connection=")
                && let Some(connection) = serde_json::from_str(json).ok().and_then(|v| Connection::from_json(&v, None))
            {
                state.kept.push(connection);
            } else if let Some((path, name)) = line.strip_prefix("database_for_file=").and_then(|rest| rest.split_once('\u{1}')) {
                state.for_file.push((PathBuf::from(path), name.to_string()));
            }
        }
        state
    }

    pub(super) fn to_settings(&self) -> String {
        let connections = self.kept.iter().map(|c| format!("database_connection={}\n", c.to_json(None)));
        let files = self.for_file.iter().map(|(path, name)| format!("database_for_file={}\u{1}{name}\n", path.display()));
        connections.chain(files).collect()
    }

    /// Whether anything is being connected to or run, for a picture to wait for.
    pub(super) fn busy(&self) -> bool {
        self.sessions.values().any(Session::busy)
    }
}

/// The box a connection is added or changed in.
pub(super) struct ConnectionBox {
    connection: Connection,
    password: String,
    /// The name it had, when an existing one is being changed.
    editing: Option<String>,
    /// Whether a password is kept for it already.
    kept_password: bool,
    problem: String,
    /// A trial connection, and what it said.
    trying: Option<Worker>,
    tried: Option<Result<String, String>>,
}

/// What the panel asks for.
enum PanelAct {
    Add,
    Edit(String),
    Remove(String),
    Connect(String),
    Disconnect(String),
    Refresh(String),
    Stop(String),
    Use { name: String, database: String },
    OpenTable { name: String, schema: String, table: String },
}

/// A number with commas between its thousands: 1,000.
fn thousands(n: u64) -> String {
    let digits = n.to_string();
    let mut out = String::new();
    for (i, c) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(c);
    }
    out
}

/// The note above a table opened from the panel, saying how much of it is shown.
pub(super) fn table_note(count: usize, table: &str) -> String {
    if count >= db::TABLE_ROWS {
        crate::i18n::tf!("The first {} rows of {table}.", thousands(db::TABLE_ROWS as u64))
    } else if count == 1 {
        crate::i18n::tf!("The only row of {table}.")
    } else {
        crate::i18n::tf!("All {} rows of {table}.", thousands(count as u64))
    }
}

/// The SQL in a few words, for a tab's title.
fn short(sql: &str) -> String {
    let flat: String = sql.split_whitespace().collect::<Vec<_>>().join(" ");
    if flat.chars().count() > 40 { format!("{}\u{2026}", flat.chars().take(40).collect::<String>()) } else { flat }
}

/// Where a place counted in characters from the start of some SQL is in the file, given where the SQL starts.
pub(super) fn place_in_file(sql: &str, start: (usize, usize), position: usize) -> (usize, usize) {
    let before: String = sql.chars().take(position).collect();
    let lines = before.matches('\n').count();
    let along = before.rsplit('\n').next().map_or(0, |last| last.chars().count());
    if lines == 0 { (start.0, start.1 + along) } else { (start.0 + lines, along) }
}

/// The SQL to run from a file's text: the statement the cursor (line, character) is in, and where it starts.
pub(super) fn statement_at(lines: &[String], cursor: (usize, usize), engine: Engine) -> Option<(String, (usize, usize))> {
    let text = lines.join("\n");
    let line_starts: Vec<usize> = std::iter::once(0).chain(lines.iter().scan(0, |at, line| { *at += line.len() + 1; Some(*at) })).collect();
    let line = lines.get(cursor.0)?;
    let byte = line_starts[cursor.0] + line.char_indices().nth(cursor.1).map_or(line.len(), |(at, _)| at);
    let range = db::statement::at_cursor(&text, byte, engine)?;
    let start_line = line_starts.iter().rposition(|s| *s <= range.start).unwrap_or(0);
    let start_char = text[line_starts[start_line]..range.start].chars().count();
    Some((text[range].to_string(), (start_line, start_char)))
}

impl Limoniter {
    /// The connections in use: the project's when one is open, else the settings'.
    pub(super) fn database_connections(&self) -> &[Connection] {
        match &self.project {
            Some(project) => &project.databases,
            None => &self.database.kept,
        }
    }

    fn database_connection(&self, name: &str) -> Option<Connection> {
        self.database_connections().iter().find(|c| c.name == name).cloned()
    }

    /// Changes the connections in use, and keeps them where they belong.
    fn change_database_connections(&mut self, change: impl FnOnce(&mut Vec<Connection>)) {
        match &mut self.project {
            Some(project) => {
                change(&mut project.databases);
                self.save_project();
            }
            None => {
                change(&mut self.database.kept);
                self.save_settings_if_changed();
            }
        }
    }

    /// Whether a document is SQL, which Ctrl+Enter runs.
    pub(super) fn is_sql_doc(&self, index: usize) -> bool {
        let doc = &self.docs[index];
        !doc.hex_mode && !doc.placeholder && doc.browser.is_none() && (doc.path.as_deref().and_then(Path::extension).is_some_and(|e| e.eq_ignore_ascii_case("sql")) || doc.language_name() == "SQL")
    }

    /// The connection chosen for a document, if it's still there.
    fn database_for_doc(&self, index: usize) -> Option<String> {
        let doc = &self.docs[index];
        let chosen = match &doc.path {
            Some(path) => self.database.for_file.iter().find(|(p, _)| p == path).map(|(_, name)| name.clone()),
            None => self.database.for_doc.get(&doc.id).cloned(),
        };
        chosen.filter(|name| self.database_connections().iter().any(|c| c.name == *name))
    }

    fn choose_database_for_doc(&mut self, index: usize, name: Option<String>) {
        let doc = &self.docs[index];
        match doc.path.clone() {
            Some(path) => {
                self.database.for_file.retain(|(p, _)| *p != path);
                if let Some(name) = name {
                    self.database.for_file.push((path, name));
                }
                self.save_settings_if_changed();
            }
            None => {
                let id = doc.id;
                match name {
                    Some(name) => self.database.for_doc.insert(id, name),
                    None => self.database.for_doc.remove(&id),
                };
            }
        }
    }

    pub(super) fn database_can(&self, act: Act) -> bool {
        match act {
            Act::Run => !self.no_file() && self.is_sql_doc(self.active),
            Act::Stop => self.database_for_doc(self.active).is_some_and(|name| self.database.sessions.get(&name).is_some_and(|s| !s.waiting.is_empty())),
            Act::Add => true,
        }
    }

    pub(super) fn database_command(&mut self, act: Act, ctx: &Context) {
        match act {
            Act::Run => self.run_sql_at_cursor(ctx),
            Act::Stop => {
                if let Some(name) = self.database_for_doc(self.active) {
                    self.stop_database_query(&name);
                }
            }
            Act::Add => self.open_connection_box(None),
        }
    }

    // ---------- Connecting ----------

    /// Connects, if it isn't connected or connecting already.
    fn connect_database(&mut self, name: &str, ctx: &Context) {
        if self.database.sessions.get(name).is_some_and(|s| s.worker.is_some()) {
            return;
        }
        let Some(connection) = self.database_connection(name) else { return };
        let password = self.database.passwords.get(&connection.password_key()).cloned();
        let wake = ctx.clone();
        let engine = connection.engine;
        let worker = Worker::start(connection, password, move || wake.request_repaint());
        worker.send(Job::Catalogue);
        self.database.sessions.insert(name.to_string(), Session { worker: Some(worker), engine, server: None, problem: None, catalogue: None, waiting: Vec::new(), using: None });
    }

    fn disconnect_database(&mut self, name: &str) {
        if let Some(session) = self.database.sessions.remove(name)
            && let Some(worker) = &session.worker
            && !session.waiting.is_empty()
        {
            worker.stop_query();
        }
    }

    fn stop_database_query(&mut self, name: &str) {
        if let Some(worker) = self.database.sessions.get(name).and_then(|s| s.worker.as_ref()) {
            worker.stop_query();
            self.running.note = Some((crate::i18n::tf!("Stopping the query on {name}\u{2026}"), Instant::now()));
        }
    }

    /// Sends SQL to a connection, connecting first if need be.
    fn send_sql(&mut self, name: &str, sql: String, most: usize, purpose: Purpose, ctx: &Context) {
        self.connect_database(name, ctx);
        self.database.next_tag += 1;
        let tag = self.database.next_tag;
        if let Some(session) = self.database.sessions.get_mut(name)
            && let Some(worker) = &session.worker
        {
            worker.send(Job::Run { tag, sql, most });
            session.waiting.push((tag, purpose));
        }
    }

    /// Picks up what the connections have sent back. True while any is busy.
    pub(super) fn database_tick(&mut self) -> bool {
        let mut news = Vec::new();
        for (name, session) in &mut self.database.sessions {
            if let Some(worker) = &session.worker {
                news.extend(worker.poll().into_iter().map(|event| (name.clone(), event)));
            }
        }
        for (name, event) in news {
            self.database_event(&name, event);
        }
        if let Dialog::Database(state) = &mut self.dialog
            && let Some(worker) = &state.trying
        {
            for event in worker.poll() {
                match event {
                    Event::Connected { server } => {
                        state.tried = Some(Ok(server));
                        state.trying = None;
                    }
                    Event::Gone(problem) => {
                        state.tried = Some(Err(problem.message));
                        state.trying = None;
                    }
                    _ => {}
                }
                if state.trying.is_none() {
                    break;
                }
            }
        }
        self.database.busy()
    }

    fn database_event(&mut self, name: &str, event: Event) {
        let Some(session) = self.database.sessions.get_mut(name) else { return };
        match event {
            Event::Connected { server } => session.server = Some(server),
            Event::Catalogue(result) => session.catalogue = Some(result.map_err(|e| e.message)),
            Event::Gone(problem) => {
                session.worker = None;
                session.problem = Some(problem.message.clone());
                let waiting = std::mem::take(&mut session.waiting);
                if session.catalogue.is_none() {
                    session.catalogue = Some(Err(problem.message.clone()));
                }
                for (_, purpose) in waiting {
                    self.database_reply(name, purpose, Err(problem.clone()));
                }
            }
            Event::Using(Ok(database)) => {
                self.running.note = Some((crate::i18n::tf!("{name} is using {database} now."), Instant::now()));
                session.using = Some(database);
            }
            Event::Using(Err(problem)) => self.message("Database", problem.message),
            Event::Ran { tag, result } => {
                let Some(at) = session.waiting.iter().position(|(t, _)| *t == tag) else { return };
                let (_, purpose) = session.waiting.remove(at);
                self.database_reply(name, purpose, result);
            }
        }
    }

    /// Shows what a query gave back: rows in the data viewer, everything else in the Output panel, and an error at
    /// its place in the SQL.
    fn database_reply(&mut self, name: &str, purpose: Purpose, result: Result<db::Reply, db::DbError>) {
        match purpose {
            Purpose::Table { title, table } => match result {
                Ok(reply) => {
                    if let Some(Outcome::Rows { table: mut rows, .. }) = reply.outcomes.into_iter().find(|o| matches!(o, Outcome::Rows { .. })) {
                        rows.note = Some(table_note(rows.rows.len(), &table));
                        self.open_data_table(&title, rows);
                    }
                }
                Err(problem) => self.message("Database", crate::i18n::tf!("{table} couldn't be opened.\n\n{}", problem.message)),
            },
            Purpose::Editor { doc, start, sql } => {
                let (reply, problem) = match result {
                    Ok(reply) => (reply, None),
                    Err(mut problem) => (std::mem::take(&mut problem.before), Some(problem)),
                };
                self.show_sql_reply(name, &sql, reply);
                // What was marked last time goes.
                let before = self.database.marked.iter().position(|(d, _)| *d == doc).map(|at| self.database.marked.remove(at).1).unwrap_or_default();
                if let Some(index) = self.docs.iter().position(|d| d.id == doc)
                    && let Some(intel) = self.docs[index].text.intel.as_mut()
                {
                    intel.build_problems.retain(|p| !before.contains(p));
                }
                match problem {
                    Some(problem) => self.mark_sql_problem(name, doc, start, &sql, &problem),
                    None => self.running.note = Some((crate::i18n::tf!("Ran on {name}: {}", short(&sql)), Instant::now())),
                }
            }
        }
    }

    fn show_sql_reply(&mut self, name: &str, sql: &str, reply: db::Reply) {
        let mut said = Vec::new();
        let single = db::statement::split(sql, self.database.sessions.get(name).map_or(Engine::Sqlite, |s| s.engine)).len() <= 1;
        for outcome in reply.outcomes {
            match outcome {
                Outcome::Rows { mut table, more } => {
                    if more > 0 {
                        let kept = table.rows.len() as u64;
                        table.note = Some(crate::i18n::tf!("The first {} of {} rows. The rest were left out.", thousands(kept), thousands(kept + more)));
                    }
                    self.open_data_table(&format!("{name}: {}", short(sql)), table);
                }
                Outcome::Done { command, rows } => {
                    // MySQL doesn't say what was done; for a single statement, its first word does.
                    let command = if command.is_empty() && single { sql.split_whitespace().next().unwrap_or_default().trim_end_matches(';').to_ascii_uppercase() } else { command };
                    let what = if command.is_empty() { crate::i18n::tr("Done").to_string() } else { command };
                    said.push(match rows {
                        Some(1) => crate::i18n::tf!("{name}: {what}, 1 row."),
                        Some(n) => crate::i18n::tf!("{name}: {what}, {} rows.", thousands(n)),
                        None => crate::i18n::tf!("{name}: {what}."),
                    });
                }
            }
        }
        said.extend(reply.notices.into_iter().map(|notice| format!("{name}: {notice}")));
        for (i, line) in said.into_iter().enumerate() {
            self.script_print(line, i == 0);
        }
    }

    /// Marks an error on its line, puts the cursor at its place, and says it in the Output panel as a compiler
    /// would, so clicking it goes there.
    fn mark_sql_problem(&mut self, name: &str, doc: u64, start: (usize, usize), sql: &str, problem: &db::DbError) {
        let place = problem.position.map(|p| place_in_file(sql, start, p));
        let first_line = problem.message.lines().next().unwrap_or_default().to_string();
        // Not being able to connect isn't the SQL's fault, so nothing's marked in it.
        let index = self.docs.iter().position(|d| d.id == doc).filter(|_| !problem.lost || problem.position.is_some());
        let Some(index) = index else {
            self.running.note = Some((crate::i18n::tf!("{name}: {first_line}"), Instant::now()));
            self.script_print(format!("{name}: {}", problem.message), true);
            return;
        };
        let line = place.map_or(start.0, |(line, _)| line);
        let shown = self.docs[index].path.as_deref().map_or_else(|| self.docs[index].name(), |p| p.display().to_string());
        self.script_print(format!("{shown}:{}: {name}: {}", line + 1, problem.message.replace('\n', " ")), true);
        let mark = (line, first_line.clone());
        let text = &mut self.docs[index].text;
        text.intel.get_or_insert_with(Box::default).build_problems.push(mark.clone());
        if let Some((line, column)) = place {
            text.move_caret_to_char(line, column);
        }
        self.database.marked.push((doc, vec![mark]));
        self.running.note = Some((crate::i18n::tf!("{name}: {first_line}"), Instant::now()));
    }

    /// Ctrl+Enter: runs the selection, or the statement the cursor's in, on the file's connection.
    fn run_sql_at_cursor(&mut self, ctx: &Context) {
        let index = self.active;
        if self.no_file() || !self.is_sql_doc(index) {
            return;
        }
        let name = match self.database_for_doc(index) {
            Some(name) => name,
            // With only one connection there's nothing to choose.
            None if self.database_connections().len() == 1 => {
                let name = self.database_connections()[0].name.clone();
                self.choose_database_for_doc(index, Some(name.clone()));
                name
            }
            None => {
                let words = if self.database_connections().is_empty() { crate::i18n::tr("There's no database to run this on. Add a connection in the Database panel (View, Panels, Database).") } else { crate::i18n::tr("Choose the database to run this file's SQL on: click \u{201C}No database\u{201D} in the status bar.") };
                self.running.note = Some((words.to_string(), Instant::now()));
                self.panels.database = true;
                return;
            }
        };
        let Some(connection) = self.database_connection(&name) else { return };
        let text = &self.docs[index].text;
        let found = if text.has_selection() && !text.is_block() {
            let (anchor, caret) = (text.anchor_char_position(), text.caret_char_position());
            Some((text.selection_text(), anchor.min(caret)))
        } else {
            let lines: Vec<String> = (0..text.line_count()).map(|i| text.line(i).to_string()).collect();
            statement_at(&lines, text.caret_char_position(), connection.engine)
        };
        let Some((sql, start)) = found.filter(|(sql, _)| !sql.trim().is_empty()) else {
            self.running.note = Some((crate::i18n::tr("There's no SQL at the cursor to run.").to_string(), Instant::now()));
            return;
        };
        let doc = self.docs[index].id;
        self.running.note = Some((crate::i18n::tf!("Running on {name}\u{2026}"), Instant::now()));
        self.send_sql(&name, sql.clone(), db::MOST_ROWS, Purpose::Editor { doc, start, sql }, ctx);
    }

    // ---------- The status bar ----------

    /// The status bar's item for an SQL file: its connection, with a menu to choose another.
    pub(super) fn database_status(&self, ui: &mut Ui) {
        if self.no_file() || !self.is_sql_doc(self.active) {
            return;
        }
        let chosen = self.database_for_doc(self.active);
        let running = chosen.as_ref().and_then(|name| self.database.sessions.get(name)).is_some_and(|s| !s.waiting.is_empty());
        let label = match &chosen {
            Some(name) if running => crate::i18n::tf!("Database: {name} (running)"),
            Some(name) => crate::i18n::tf!("Database: {name}"),
            None => crate::i18n::tr("No database").to_string(),
        };
        let shown = ui.add(egui::Label::new(RichText::new(label.clone()).color(if chosen.is_some() { theme::text_faint() } else { theme::text_dim() })).sense(egui::Sense::click()));
        crate::access::button(&shown, &label);
        let shown = shown.on_hover_text(crate::i18n::tr("The database Ctrl+Enter runs this file's SQL on. Click to choose another"));
        crate::fit::popup_menu(&shown, |ui| {
            ui.set_min_width(200.0);
            let pick = |ui: &mut Ui, name: &str| ui.ctx().data_mut(|d| d.insert_temp(egui::Id::unique(STATUS_PICKED), name.to_string()));
            for connection in self.database_connections() {
                if ui.add(egui::Button::selectable(chosen.as_deref() == Some(connection.name.as_str()), &connection.name)).on_hover_text(connection.place()).clicked() {
                    pick(ui, &connection.name);
                    ui.close();
                }
            }
            if ui.add(egui::Button::selectable(chosen.is_none(), crate::i18n::tr("No database"))).clicked() {
                pick(ui, "");
                ui.close();
            }
            ui.separator();
            if running && ui.button(crate::i18n::tr("Stop the query")).clicked() {
                pick(ui, "\u{1}stop");
                ui.close();
            }
            if ui.button(crate::i18n::tr("Add a connection\u{2026}")).clicked() {
                pick(ui, "\u{1}add");
                ui.close();
            }
        });
    }

    /// Acts on a choice made in the status bar's menu.
    pub(super) fn database_status_picked(&mut self, ctx: &Context) {
        let Some(picked) = ctx.data_mut(|d| d.remove_temp::<String>(egui::Id::unique(STATUS_PICKED))) else { return };
        match picked.as_str() {
            "\u{1}add" => self.open_connection_box(None),
            "\u{1}stop" => self.database_command(Act::Stop, ctx),
            "" => self.choose_database_for_doc(self.active, None),
            name => self.choose_database_for_doc(self.active, Some(name.to_string())),
        }
    }

    // ---------- The panel ----------

    /// Draws the Database panel, if it's open.
    pub(super) fn database_panel(&mut self, ui: &mut Ui) {
        if !self.panels.database {
            return;
        }
        let mut open = true;
        let mut acts = Vec::new();
        let connections = match &self.project {
            Some(project) => &project.databases,
            None => &self.database.kept,
        };
        let in_project = self.project.is_some();
        let state = &self.database;
        panels::dock(ui, false, "database-panel", "Database", 260.0, &mut open, |ui| database_tree(ui, connections, state, in_project, &mut acts));
        self.panels.database = open;
        let ctx = ui.ctx().clone();
        for act in acts {
            match act {
                PanelAct::Add => self.open_connection_box(None),
                PanelAct::Edit(name) => self.open_connection_box(Some(&name)),
                PanelAct::Remove(name) => {
                    self.disconnect_database(&name);
                    let Some(gone) = self.database_connection(&name) else { continue };
                    self.change_database_connections(|list| list.retain(|c| c.name != name));
                    // Its password goes too, unless another connection logs in the same way.
                    if gone.engine.is_server() && !self.database_connections().iter().any(|c| c.password_key() == gone.password_key()) {
                        db::forget_password(&gone);
                        self.database.passwords.remove(&gone.password_key());
                    }
                }
                PanelAct::Connect(name) => self.connect_database(&name, &ctx),
                PanelAct::Disconnect(name) => self.disconnect_database(&name),
                PanelAct::Refresh(name) => {
                    match self.database.sessions.get_mut(&name) {
                        Some(session) if session.worker.is_some() => {
                            session.catalogue = None;
                            if let Some(worker) = &session.worker {
                                worker.send(Job::Catalogue);
                            }
                        }
                        _ => {
                            self.database.sessions.remove(&name);
                            self.connect_database(&name, &ctx);
                        }
                    }
                }
                PanelAct::Stop(name) => self.stop_database_query(&name),
                PanelAct::Use { name, database } => {
                    if let Some(worker) = self.database.sessions.get(&name).and_then(|s| s.worker.as_ref()) {
                        worker.send(Job::UseDatabase(database));
                    }
                }
                PanelAct::OpenTable { name, schema, table } => {
                    let Some(engine) = self.database_connection(&name).map(|c| c.engine) else { continue };
                    let sql = db::first_rows_sql(engine, &schema, &table, db::TABLE_ROWS);
                    let shown = if engine == Engine::Sqlite && schema == "main" { table.clone() } else { format!("{schema}.{table}") };
                    self.send_sql(&name, sql, db::TABLE_ROWS, Purpose::Table { title: format!("{name}: {shown}"), table: shown }, &ctx);
                }
            }
        }
    }

    // ---------- The connection box ----------

    fn open_connection_box(&mut self, name: Option<&str>) {
        let existing = name.and_then(|n| self.database_connection(n));
        let kept_password = existing.as_ref().is_some_and(|c| c.engine.is_server() && (self.database.passwords.contains_key(&c.password_key()) || db::stored_password(c).is_some()));
        let connection = existing.clone().unwrap_or_else(|| {
            let mut fresh = Connection::new(Engine::Sqlite);
            fresh.name = crate::i18n::tr("New connection").to_string();
            fresh
        });
        self.dialog = Dialog::Database(Box::new(ConnectionBox { connection, password: String::new(), editing: existing.map(|c| c.name), kept_password, problem: String::new(), trying: None, tried: None }));
    }

    pub(super) fn database_dialog(&mut self, ctx: &Context) -> bool {
        let in_project = self.project.is_some();
        let names: Vec<String> = self.database_connections().iter().map(|c| c.name.clone()).collect();
        let Dialog::Database(state) = &mut self.dialog else {
            return false;
        };
        let mut close = ctx.input(|i| i.key_pressed(Key::Escape));
        let (mut save, mut try_it) = (false, false);
        let title = if state.editing.is_some() { crate::i18n::tr("Database connection") } else { crate::i18n::tr("Add a database connection") };
        crate::fit::Dialog::new(title).collapsible(false).resizable(false).anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0]).show_with_buttons(ctx, |ui, part| match part {
            crate::fit::Part::Body => {
                ui.set_max_width(480.0);
                let c = &mut state.connection;
                egui::Grid::new("database-connection").num_columns(2).spacing([10.0, 6.0]).show(ui, |ui| {
                    ui.label(crate::i18n::tr("Name"));
                    ui.add(egui::TextEdit::singleline(&mut c.name).desired_width(260.0));
                    ui.end_row();
                    ui.label(crate::i18n::tr("Kind"));
                    ui.horizontal(|ui| {
                        for engine in Engine::ALL {
                            if ui.radio(c.engine == engine, engine.name()).clicked() && c.engine != engine {
                                let name = std::mem::take(&mut c.name);
                                *c = Connection { name, ..Connection::new(engine) };
                            }
                        }
                    });
                    ui.end_row();
                    if c.engine.is_server() {
                        ui.label(crate::i18n::tr("Server"));
                        ui.horizontal(|ui| {
                            ui.add(egui::TextEdit::singleline(&mut c.host).desired_width(190.0));
                            ui.label(crate::i18n::tr("Port"));
                            let mut port = c.port.to_string();
                            if ui.add(egui::TextEdit::singleline(&mut port).desired_width(52.0)).changed() {
                                c.port = port.trim().parse().unwrap_or(c.port);
                            }
                        });
                        ui.end_row();
                        ui.label(crate::i18n::tr("User"));
                        ui.add(egui::TextEdit::singleline(&mut c.user).desired_width(260.0));
                        ui.end_row();
                        ui.label(crate::i18n::tr("Password"));
                        let hint = if state.kept_password { crate::i18n::tr("Kept; type to change it") } else { crate::i18n::tr("If one is needed") };
                        ui.add(egui::TextEdit::singleline(&mut state.password).password(true).hint_text(hint).desired_width(260.0)).on_hover_text(crate::i18n::tr("Kept in the system's keychain, never in a file"));
                        ui.end_row();
                        ui.label(crate::i18n::tr("Database"));
                        ui.add(egui::TextEdit::singleline(&mut c.database).hint_text(crate::i18n::tr("The user's own, if left empty")).desired_width(260.0));
                        ui.end_row();
                        ui.label(crate::i18n::tr("TLS"));
                        ui.vertical(|ui| {
                            egui::ComboBox::from_id_salt("database-tls").selected_text(crate::i18n::tr(c.tls.label())).show_ui(ui, |ui| {
                                for mode in TlsMode::ALL {
                                    ui.selectable_value(&mut c.tls, mode, crate::i18n::tr(mode.label()));
                                }
                            });
                            if c.tls != TlsMode::Disable {
                                ui.checkbox(&mut c.trust_own_certificate, crate::i18n::tr("Accept the server's own certificate"))
                                    .on_hover_text(crate::i18n::tr("As psql's sslmode=require does: the connection is private, but the certificate isn't checked against the authorities, which a server's own (self-signed) certificate wouldn't pass"));
                            }
                        });
                        ui.end_row();
                    } else {
                        ui.label(crate::i18n::tr("File"));
                        ui.horizontal(|ui| {
                            let shown = if c.file.as_os_str().is_empty() { crate::i18n::tr("(choose a file)").to_string() } else { c.file.display().to_string() };
                            ui.label(shown);
                            if ui.button(crate::i18n::tr("Choose\u{2026}")).clicked()
                                && let Some(file) = crate::sys_dialogs::file_dialog().set_title(crate::i18n::tr("An SQLite database")).add_filter("SQLite", &["db", "sqlite", "sqlite3", "db3"]).add_filter(crate::i18n::tr("Every file"), &["*"]).pick_file()
                            {
                                c.file = file;
                            }
                        });
                        ui.end_row();
                    }
                });
                ui.add_space(4.0);
                ui.label(RichText::new(if in_project { crate::i18n::tr("Kept with the project.") } else { crate::i18n::tr("Kept in Limoniter's settings.") }).color(theme::text_dim()));
            }
            crate::fit::Part::Buttons => {
                let clash = names.iter().any(|n| *n == state.connection.name.trim() && state.editing.as_deref() != Some(n.as_str()));
                let problem = state.connection.problem().or_else(|| clash.then(|| crate::i18n::tr("There's a connection with that name already.").to_string()));
                if let Some(problem) = &problem {
                    ui.colored_label(theme::change_unsaved(), problem);
                } else if !state.problem.is_empty() {
                    ui.colored_label(theme::change_unsaved(), &state.problem);
                }
                match &state.tried {
                    Some(Ok(server)) => {
                        ui.colored_label(theme::accent(), crate::i18n::tf!("Connected: {server}."));
                    }
                    Some(Err(why)) => {
                        ui.colored_label(theme::change_unsaved(), why);
                    }
                    None if state.trying.is_some() => {
                        ui.label(crate::i18n::tr("Connecting\u{2026}"));
                    }
                    None => {}
                }
                ui.add_space(6.0);
                ui.horizontal(|ui| {
                    save = ui.add_enabled(problem.is_none(), egui::Button::new(crate::i18n::tr("Save"))).clicked();
                    try_it = ui.add_enabled(problem.is_none() && state.trying.is_none(), egui::Button::new(crate::i18n::tr("Try connecting"))).clicked();
                    close |= ui.button(crate::i18n::tr("Cancel")).clicked();
                });
            }
        });
        if try_it {
            let mut connection = state.connection.clone();
            connection.name = connection.name.trim().to_string();
            let password = if state.password.is_empty() { self.database.passwords.get(&connection.password_key()).cloned() } else { Some(state.password.clone()) };
            let wake = ctx.clone();
            state.tried = None;
            state.trying = Some(Worker::start(connection, password, move || wake.request_repaint()));
        }
        if save {
            let mut connection = state.connection.clone();
            connection.name = connection.name.trim().to_string();
            let (password, editing) = (std::mem::take(&mut state.password), state.editing.clone());
            self.dialog = Dialog::None;
            self.pending_focus = true;
            if connection.engine.is_server() && !password.is_empty() {
                let key = connection.password_key();
                match db::store_password(&connection, &password) {
                    Ok(()) => {
                        self.database.passwords.remove(&key);
                    }
                    Err(why) => {
                        self.database.passwords.insert(key, password);
                        self.message("Database", crate::i18n::tf!("The system's keychain couldn't keep the password ({why}), so Limoniter will use it only until it's closed. Passwords are never written to a file."));
                    }
                }
            }
            // A changed connection is connected to again when it's next used.
            let old = editing.unwrap_or_else(|| connection.name.clone());
            self.disconnect_database(&old);
            self.disconnect_database(&connection.name);
            if old != connection.name {
                for (_, name) in &mut self.database.for_file {
                    if *name == old {
                        name.clone_from(&connection.name);
                    }
                }
            }
            self.change_database_connections(|list| match list.iter_mut().find(|c| c.name == old) {
                Some(kept) => *kept = connection,
                None => list.push(connection),
            });
            self.panels.database = true;
        } else if close {
            self.dialog = Dialog::None;
            self.pending_focus = true;
        }
        true
    }

    /// For a picture: "sqlite=PATH" adds an SQLite file (just for the picture) and opens the panel with it
    /// connected and opened out; "box" opens the box for a new connection; "panel" opens the panel.
    pub(super) fn database_screenshot_step(&mut self, value: &str) {
        let ctx = &self.database.ctx.clone().unwrap_or_default();
        if let Some(path) = value.strip_prefix("sqlite=") {
            let path = PathBuf::from(path);
            let name = path.file_stem().map_or_else(|| "SQLite".to_string(), |s| s.to_string_lossy().into_owned());
            let connection = Connection { name: name.clone(), file: path, ..Connection::new(Engine::Sqlite) };
            match &mut self.project {
                Some(project) => project.databases.push(connection),
                None => self.database.kept.push(connection),
            }
            self.database.open_all = true;
            self.panels.database = true;
            self.connect_database(&name, ctx);
            if !self.no_file() && self.is_sql_doc(self.active) {
                self.choose_database_for_doc(self.active, Some(name));
            }
        } else if value == "box" {
            self.open_connection_box(None);
        } else if value == "run" {
            self.run_sql_at_cursor(ctx);
        } else {
            self.panels.database = true;
        }
    }
}

/// The panel's tree: each connection, its schemas, their tables and views, and their columns.
fn database_tree(ui: &mut Ui, connections: &[Connection], state: &DatabaseState, in_project: bool, acts: &mut Vec<PanelAct>) {
    ui.horizontal(|ui| {
        if ui.small_button(crate::i18n::tr("Add a connection\u{2026}")).clicked() {
            acts.push(PanelAct::Add);
        }
    });
    ui.label(RichText::new(if in_project { crate::i18n::tr("Kept with the project") } else { crate::i18n::tr("Kept in the settings") }).size(11.5).color(theme::text_faint()));
    if connections.is_empty() {
        ui.add_space(6.0);
        ui.label(RichText::new(crate::i18n::tr("No databases yet. Add a connection to an SQLite file, or to a PostgreSQL or MySQL server.")).color(theme::text_dim()));
        return;
    }
    egui::ScrollArea::vertical().id_salt("database-tree").auto_shrink([false, false]).show(ui, |ui| {
        for connection in connections {
            let name = &connection.name;
            let session = state.sessions.get(name);
            let header = egui::CollapsingHeader::new(RichText::new(name).strong()).id_salt(("database-connection", name)).default_open(state.open_all);
            let shown = header.show(ui, |ui| {
                let Some(session) = session else {
                    acts.push(PanelAct::Connect(name.clone()));
                    ui.label(RichText::new(crate::i18n::tr("Connecting\u{2026}")).color(theme::text_dim()));
                    return;
                };
                if let Some(problem) = &session.problem {
                    ui.label(RichText::new(problem).color(theme::change_unsaved()));
                    if ui.small_button(crate::i18n::tr("Try again")).clicked() {
                        acts.push(PanelAct::Refresh(name.clone()));
                    }
                    return;
                }
                if session.connecting() {
                    ui.horizontal(|ui| {
                        ui.spinner();
                        ui.label(RichText::new(crate::i18n::tr("Connecting\u{2026}")).color(theme::text_dim()));
                    });
                    return;
                }
                if !session.waiting.is_empty() {
                    ui.horizontal(|ui| {
                        ui.spinner();
                        ui.label(RichText::new(crate::i18n::tr("Running\u{2026}")).color(theme::text_dim()));
                        if ui.small_button(crate::i18n::tr("Stop")).on_hover_text(crate::i18n::tr("Stops the query running on this connection")).clicked() {
                            acts.push(PanelAct::Stop(name.clone()));
                        }
                    });
                }
                match &session.catalogue {
                    None => {
                        ui.spinner();
                    }
                    Some(Err(why)) => {
                        ui.label(RichText::new(why).color(theme::change_unsaved()));
                    }
                    Some(Ok(schemas)) => {
                        let single = schemas.len() == 1;
                        for schema in schemas {
                            let using = session.using.as_deref() == Some(schema.name.as_str()) || (session.using.is_none() && session.engine == Engine::MySql && schema.name == connection.database);
                            let title = if using { RichText::new(crate::i18n::tf!("{} (in use)", schema.name)).strong() } else { RichText::new(&schema.name) };
                            let shown = egui::CollapsingHeader::new(title).id_salt(("database-schema", name, &schema.name)).default_open(single || state.open_all).show(ui, |ui| {
                                if schema.tables.is_empty() {
                                    ui.label(RichText::new(crate::i18n::tr("No tables")).color(theme::text_faint()));
                                }
                                for table in &schema.tables {
                                    table_node(ui, name, &schema.name, table, state.open_all, acts);
                                }
                            });
                            // A MySQL server's databases can be changed between, as USE does.
                            if session.engine == Engine::MySql {
                                crate::fit::context_menu(&shown.header_response, |ui| {
                                    if ui.button(crate::i18n::tr("Use this database for the SQL run")).clicked() {
                                        acts.push(PanelAct::Use { name: name.clone(), database: schema.name.clone() });
                                        ui.close();
                                    }
                                });
                            }
                        }
                    }
                }
            });
            let header = shown.header_response.on_hover_text(match session.and_then(|s| s.server.as_deref()) {
                Some(server) => format!("{} \u{2014} {server}", connection.place()),
                None => connection.place(),
            });
            crate::fit::context_menu(&header, |ui| {
                let connected = session.is_some_and(|s| s.worker.is_some());
                if !connected && ui.button(crate::i18n::tr("Connect")).clicked() {
                    acts.push(PanelAct::Connect(name.clone()));
                    ui.close();
                }
                if connected && ui.button(crate::i18n::tr("Disconnect")).clicked() {
                    acts.push(PanelAct::Disconnect(name.clone()));
                    ui.close();
                }
                if ui.button(crate::i18n::tr("Refresh")).clicked() {
                    acts.push(PanelAct::Refresh(name.clone()));
                    ui.close();
                }
                ui.separator();
                if ui.button(crate::i18n::tr("Edit\u{2026}")).clicked() {
                    acts.push(PanelAct::Edit(name.clone()));
                    ui.close();
                }
                if ui.button(crate::i18n::tr("Remove")).clicked() {
                    acts.push(PanelAct::Remove(name.clone()));
                    ui.close();
                }
            });
        }
    });
}

/// A table or view, opened out to its columns; double-clicking it shows its first rows.
fn table_node(ui: &mut Ui, name: &str, schema: &str, table: &db::Table, open_all: bool, acts: &mut Vec<PanelAct>) {
    let label = if table.view { crate::i18n::tf!("{} (view)", table.name) } else { table.name.clone() };
    let shown = egui::CollapsingHeader::new(label).id_salt(("database-table", name, schema, &table.name)).default_open(open_all).show(ui, |ui| {
        for column in &table.columns {
            let mut words = format!("{}  {}", column.name, column.kind);
            if column.primary {
                words.push_str(&format!("  {}", crate::i18n::tr("(primary key)")));
            }
            if let Some(to) = &column.refers_to {
                words.push_str(&format!("  \u{2192} {to}"));
            }
            ui.label(RichText::new(words).size(12.0).color(if column.primary { theme::text() } else { theme::text_dim() }));
        }
    });
    let header = shown.header_response.on_hover_text(crate::i18n::tf!("Double-click to see the first {} rows", thousands(db::TABLE_ROWS as u64)));
    let open = || PanelAct::OpenTable { name: name.to_string(), schema: schema.to_string(), table: table.name.clone() };
    if header.double_clicked() {
        acts.push(open());
    }
    crate::fit::context_menu(&header, |ui| {
        if ui.button(crate::i18n::tf!("Show the first {} rows", thousands(db::TABLE_ROWS as u64))).clicked() {
            acts.push(open());
            ui.close();
        }
        if ui.button(crate::i18n::tr("Copy the name")).clicked() {
            ui.ctx().copy_text(table.name.clone());
            ui.close();
        }
    });
}

#[cfg(test)]
#[path = "database_host_tests.rs"]
mod database_host_tests;
