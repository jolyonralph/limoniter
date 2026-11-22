//! The database browser, run as the whole app with no screen: an SQLite file as the connection, SQL run from a
//! .sql file with Ctrl+Enter, rows and counts and errors going where they should, and the connections kept.

use super::*;

fn sized(events: Vec<egui::Event>) -> egui::RawInput {
    egui::RawInput { screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1600.0, 900.0))), events, ..Default::default() }
}

fn frame(app: &mut Limoniter, ctx: &Context, events: Vec<egui::Event>) {
    let mut output = ctx.run_ui(sized(events), |ui| app.draw_frame(ui));
    output.textures_delta.clear();
}

fn give(app: &mut Limoniter, ctx: &Context, command: Command) {
    let mut output = ctx.run_ui(sized(Vec::new()), |ui| app.run(command, ui));
    output.textures_delta.clear();
}

/// Draws frames until the connections have nothing left to do.
fn settle(app: &mut Limoniter, ctx: &Context) {
    let began = Instant::now();
    frame(app, ctx, Vec::new());
    while app.database.busy() && began.elapsed() < std::time::Duration::from_secs(20) {
        std::thread::sleep(std::time::Duration::from_millis(5));
        frame(app, ctx, Vec::new());
    }
    frame(app, ctx, Vec::new());
}

/// A folder with an SQLite database holding a table of three rows, and an SQL file.
fn folder(name: &str, sql: &str) -> (PathBuf, PathBuf, PathBuf) {
    let dir = std::env::temp_dir().join(format!("limoniter-database-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join("shop.db");
    let db = rusqlite::Connection::open(&file).unwrap();
    db.execute_batch("CREATE TABLE fruit (id INTEGER PRIMARY KEY, name TEXT NOT NULL, price REAL); INSERT INTO fruit VALUES (1, 'apple', 0.5), (2, 'pear', 0.75), (3, 'plum', NULL);").unwrap();
    drop(db);
    let script = dir.join("work.sql");
    std::fs::write(&script, sql).unwrap();
    (dir, file, script)
}

fn app_with(file: &Path, script: &Path) -> (Limoniter, Context) {
    let mut app = Limoniter::build(String::new(), None, Some(PathBuf::from("never-written.png")));
    app.database.kept.push(Connection { name: "shop".into(), file: file.to_path_buf(), ..Connection::new(Engine::Sqlite) });
    assert!(app.open_path(script));
    (app, Context::default())
}

fn press_ctrl_enter() -> Vec<egui::Event> {
    let modifiers = Modifiers::COMMAND;
    vec![egui::Event::Key { key: Key::Enter, physical_key: None, pressed: true, repeat: false, modifiers }, egui::Event::Key { key: Key::Enter, physical_key: None, pressed: false, repeat: false, modifiers }]
}

#[test]
fn ctrl_enter_runs_the_statement_at_the_cursor_and_shows_rows_counts_and_errors() {
    let sql = "SELECT * FROM fruit ORDER BY id;\n\nINSERT INTO fruit (name) VALUES ('fig');\nSELECT nope FROM fruit;\n";
    let (dir, file, script) = folder("run", sql);
    let (mut app, ctx) = app_with(&file, &script);
    let sql_doc = app.docs[app.active].id;
    frame(&mut app, &ctx, Vec::new());
    // Rows open in a tab of their own; with only one connection it's chosen without asking.
    let before = app.docs.len();
    frame(&mut app, &ctx, press_ctrl_enter());
    settle(&mut app, &ctx);
    assert_eq!(app.docs.len(), before + 1, "the rows open in a tab of their own");
    assert_eq!(app.database.for_file, [(script.clone(), "shop".to_string())], "the connection is remembered for the file");
    let back = |app: &mut Limoniter| app.active = app.docs.iter().position(|d| d.id == sql_doc).unwrap();
    back(&mut app);
    assert!(!app.docs[app.active].text.line(0).contains('\n') && app.docs[app.active].text.line_count() == 5, "Ctrl+Enter didn't break the line: {:?}", app.docs[app.active].text.line(0));
    // A statement without rows says how many it changed in the Output panel.
    app.docs[app.active].text.move_caret_to_char(2, 5);
    give(&mut app, &ctx, Command::Database(Act::Run));
    settle(&mut app, &ctx);
    assert_eq!(app.docs.len(), before + 1, "no rows, no new tab");
    assert!(app.show_results);
    assert_eq!(app.script_output, ["shop: INSERT, 1 row."]);
    // An error is marked on its line, the cursor goes to its place, and the Output panel says where.
    back(&mut app);
    app.docs[app.active].text.move_caret_to_char(3, 0);
    give(&mut app, &ctx, Command::Database(Act::Run));
    settle(&mut app, &ctx);
    let doc = &app.docs[app.active];
    assert_eq!(doc.id, sql_doc);
    let problems = &doc.text.intel.as_ref().expect("marked").build_problems;
    assert_eq!(problems.len(), 1);
    assert_eq!(problems[0].0, 3);
    assert!(problems[0].1.contains("nope"), "{problems:?}");
    assert_eq!(doc.text.caret_char_position(), (3, 7), "at the column's name");
    assert!(app.script_output[0].starts_with(&format!("{}:4: shop: ", script.display())), "{:?}", app.script_output);
    // Running it again once it's put right takes the mark away.
    app.docs[app.active].text.move_caret_to_char(0, 0);
    give(&mut app, &ctx, Command::Database(Act::Run));
    settle(&mut app, &ctx);
    let doc = app.docs.iter().find(|d| d.id == sql_doc).unwrap();
    assert!(doc.text.intel.as_ref().unwrap().build_problems.is_empty());
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn a_selection_is_run_as_it_is_and_several_statements_give_several_results() {
    let (dir, file, script) = folder("selection", "SELECT 1; SELECT 2;\nDELETE FROM fruit WHERE id > 1;\n");
    let (mut app, ctx) = app_with(&file, &script);
    frame(&mut app, &ctx, Vec::new());
    app.docs[app.active].text.select_chars((0, 0), (1, 31));
    let before = app.docs.len();
    give(&mut app, &ctx, Command::Database(Act::Run));
    settle(&mut app, &ctx);
    assert_eq!(app.docs.len(), before + 2, "two tables of rows");
    assert_eq!(app.script_output, ["shop: DELETE, 2 rows."]);
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn without_a_connection_chosen_it_says_how_to_choose_one_and_the_status_bar_chooses() {
    let (dir, file, script) = folder("choose", "SELECT 1;");
    let (mut app, ctx) = app_with(&file, &script);
    app.database.kept.push(Connection { name: "other".into(), file: file.clone(), ..Connection::new(Engine::Sqlite) });
    give(&mut app, &ctx, Command::Database(Act::Run));
    assert!(app.running.note.as_ref().is_some_and(|(words, _)| words.contains("status bar")), "{:?}", app.running.note);
    assert!(app.database.sessions.is_empty(), "nothing was run");
    // The status bar's menu leaves its choice for the frame to act on.
    ctx.data_mut(|d| d.insert_temp(egui::Id::unique(STATUS_PICKED), "other".to_string()));
    app.database_status_picked(&ctx);
    assert_eq!(app.database_for_doc(app.active).as_deref(), Some("other"));
    ctx.data_mut(|d| d.insert_temp(egui::Id::unique(STATUS_PICKED), String::new()));
    app.database_status_picked(&ctx);
    assert_eq!(app.database_for_doc(app.active), None);
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn opening_a_table_from_the_panel_shows_its_rows_with_a_note() {
    let (dir, file, script) = folder("table", "");
    let (mut app, ctx) = app_with(&file, &script);
    app.panels.database = true;
    app.connect_database("shop", &ctx);
    settle(&mut app, &ctx);
    let session = &app.database.sessions["shop"];
    let Some(Ok(schemas)) = &session.catalogue else { panic!("a catalogue") };
    assert_eq!(schemas[0].tables[0].name, "fruit");
    assert!(session.server.as_deref().is_some_and(|s| s.starts_with("SQLite 3.")));
    let before = app.docs.len();
    let sql = db::first_rows_sql(Engine::Sqlite, "main", "fruit", db::TABLE_ROWS);
    app.send_sql("shop", sql, db::TABLE_ROWS, Purpose::Table { title: "shop: fruit".into(), table: "fruit".into() }, &ctx);
    settle(&mut app, &ctx);
    assert_eq!(app.docs.len(), before + 1);
    assert_eq!(table_note(3, "fruit"), "All 3 rows of fruit.");
    assert_eq!(table_note(1, "fruit"), "The only row of fruit.");
    assert_eq!(table_note(1000, "fruit"), "The first 1,000 rows of fruit.");
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn a_connection_that_fails_says_why_and_its_waiting_queries_are_told() {
    let (dir, _, script) = folder("fails", "SELECT 1;");
    let (mut app, ctx) = app_with(&dir.join("missing.db"), &script);
    give(&mut app, &ctx, Command::Database(Act::Run));
    settle(&mut app, &ctx);
    let session = &app.database.sessions["shop"];
    assert!(session.problem.as_deref().is_some_and(|p| p.contains("missing.db")), "{:?}", session.problem);
    assert!(session.waiting.is_empty());
    assert!(app.script_output.iter().any(|line| line.contains("missing.db")), "{:?}", app.script_output);
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn connections_and_choices_are_kept_in_the_settings_without_passwords() {
    let mut state = DatabaseState::default();
    state.kept.push(Connection { name: "Live, \"main\"".into(), user: "me".into(), database: "shop".into(), ..Connection::new(Engine::Postgres) });
    state.kept.push(Connection { name: "Local".into(), file: PathBuf::from("/data/x.db"), ..Connection::new(Engine::Sqlite) });
    state.for_file.push((PathBuf::from("/work/a.sql"), "Local".into()));
    state.passwords.insert("k".into(), "hunter2".into());
    let text = state.to_settings();
    assert!(!text.contains("hunter2"));
    assert_eq!(text.lines().count(), 3, "{text}");
    let back = DatabaseState::from_settings(&format!("other=1\n{text}"));
    assert_eq!(back.kept, state.kept);
    assert_eq!(back.for_file, state.for_file);
}

#[test]
fn with_a_project_open_connections_are_kept_in_it() {
    let (dir, file, script) = folder("project", "SELECT 1;");
    let (mut app, _ctx) = app_with(&file, &script);
    let mut project = project::Project::new(&dir.join("shop.limproj"));
    project.databases.push(Connection { name: "in project".into(), file: file.clone(), ..Connection::new(Engine::Sqlite) });
    let text = project.to_text();
    assert!(text.contains("\"file\": \"shop.db\""), "relative to the project: {text}");
    let back = project::Project::from_text(&project.path, &text).unwrap();
    assert_eq!(back.databases, project.databases);
    app.project = Some(back);
    assert_eq!(app.database_connections().iter().map(|c| c.name.as_str()).collect::<Vec<_>>(), ["in project"], "the project's, not the settings'");
    app.change_database_connections(|list| list.clear());
    assert!(project::Project::load(&dir.join("shop.limproj")).unwrap().databases.is_empty(), "saved with the project");
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn the_statement_at_the_cursor_is_found_with_where_it_starts() {
    let lines: Vec<String> = ["-- notes", "SELECT 'é',", "  2;  SELECT 3;"].map(String::from).to_vec();
    assert_eq!(statement_at(&lines, (1, 3), Engine::Sqlite), Some(("SELECT 'é',\n  2;".to_string(), (1, 0))));
    assert_eq!(statement_at(&lines, (2, 9), Engine::Sqlite), Some(("SELECT 3;".to_string(), (2, 6))));
    assert_eq!(statement_at(&lines, (0, 2), Engine::Sqlite).map(|(_, start)| start), Some((1, 0)), "before them all, the first");
}

#[test]
fn a_place_in_the_sql_is_found_in_the_file() {
    let sql = "SELECT 'é',\n  nope";
    assert_eq!(place_in_file(sql, (4, 6), 7), (4, 13), "on the first line, along from where the SQL starts");
    assert_eq!(place_in_file(sql, (4, 6), 14), (5, 2), "on a later line, from its start");
}

#[test]
fn numbers_are_grouped_in_thousands() {
    assert_eq!([0, 999, 1000, 1_234_567].map(thousands), ["0", "999", "1,000", "1,234,567"]);
}
