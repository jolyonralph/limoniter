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

fn press(key: Key) -> Vec<egui::Event> {
    vec![egui::Event::Key { key, physical_key: None, pressed: true, repeat: false, modifiers: Modifiers::NONE }, egui::Event::Key { key, physical_key: None, pressed: false, repeat: false, modifiers: Modifiers::NONE }]
}

fn lines(app: &Limoniter) -> Vec<String> {
    app.docs[app.active].text.all_lines().iter().map(|line| line.to_string()).collect()
}

fn app_with_csv(name: &str, text: &str) -> (Limoniter, Context, PathBuf) {
    let dir = std::env::temp_dir().join(format!("limoniter-table-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join("fruit.csv");
    std::fs::write(&file, text).unwrap();
    let mut app = Limoniter::build(String::new(), None, Some(PathBuf::from("never-written.png")));
    assert!(app.open_path(&file));
    (app, Context::default(), dir)
}

fn state(app: &mut Limoniter) -> &mut TableState {
    app.docs[app.active].table.as_deref_mut().expect("table view is on")
}

const FRUIT: &str = "name,price,notes\nApple,0.50,crisp\nPear,0.75,\"soft, sweet\"\nPlum,1.20,\"two\nlines\"\n";

#[test]
fn a_csv_file_is_offered_as_a_table_and_shown_as_one() {
    let (mut app, ctx, dir) = app_with_csv("t2", FRUIT);
    frame(&mut app, &ctx, Vec::new());
    assert_eq!(app.docs[app.active].table_offer, Some(table::Dialect { delimiter: ',', header: true }), "found when it's opened, and offered");
    give(&mut app, &ctx, Command::ToggleTable);
    frame(&mut app, &ctx, Vec::new());
    frame(&mut app, &ctx, Vec::new());
    let shown = state(&mut app).shown.clone();
    assert_eq!(shown, [1, 2, 3], "the rows under the headings, the two-line one as one row");
    // Sorting and filtering change what's shown, never the file.
    state(&mut app).order = Some((1, false));
    frame(&mut app, &ctx, Vec::new());
    assert_eq!(state(&mut app).shown, [3, 2, 1]);
    state(&mut app).filter = "SWEET".into();
    frame(&mut app, &ctx, Vec::new());
    assert_eq!(state(&mut app).shown, [2]);
    assert_eq!(lines(&app).join("\n"), FRUIT.trim_end_matches('\n').to_string() + "\n");
    give(&mut app, &ctx, Command::ToggleTable);
    assert!(app.docs[app.active].table.is_none());
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn typing_in_a_cell_changes_only_that_field_and_undo_takes_it_back() {
    let (mut app, ctx, dir) = app_with_csv("t3", FRUIT);
    give(&mut app, &ctx, Command::ToggleTable);
    frame(&mut app, &ctx, Vec::new());
    frame(&mut app, &ctx, Vec::new());
    // Down to Pear, across to its price, and type over it.
    frame(&mut app, &ctx, press(Key::ArrowDown));
    frame(&mut app, &ctx, press(Key::ArrowRight));
    assert_eq!(ctx.memory(|m| m.focused()), Some(views::table_id()), "the table keeps the keyboard");
    assert_eq!(state(&mut app).cursor, (1, 1));
    frame(&mut app, &ctx, vec![egui::Event::Text("0".into())]);
    assert!(state(&mut app).editing.is_some(), "typing starts editing, replacing what was there");
    frame(&mut app, &ctx, Vec::new());
    frame(&mut app, &ctx, vec![egui::Event::Text(".99".into())]);
    frame(&mut app, &ctx, press(Key::Enter));
    frame(&mut app, &ctx, Vec::new());
    assert!(state(&mut app).editing.is_none());
    assert_eq!(lines(&app)[2], "Pear,0.99,\"soft, sweet\"", "the quoted field after it is untouched");
    assert_eq!(state(&mut app).cursor, (2, 1), "Enter moves down, as in a spreadsheet");
    // A value that needs quotes gets them.
    let record = state(&mut app).shown[0];
    let all = app.docs[app.active].text.all_lines();
    let records = state(&mut app).index.clone().unwrap().records;
    app.table_action(Action::Commit { record, column: 2, value: "red, \"ripe\"".into() }, &all, &records, &[], true);
    assert_eq!(lines(&app)[1], "Apple,0.50,\"red, \"\"ripe\"\"\"");
    give(&mut app, &ctx, Command::Undo);
    assert_eq!(lines(&app)[1], "Apple,0.50,crisp");
    give(&mut app, &ctx, Command::Undo);
    assert_eq!(lines(&app)[2], "Pear,0.75,\"soft, sweet\"");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn rows_and_columns_go_in_and_out_and_the_file_can_be_sorted() {
    let (mut app, ctx, dir) = app_with_csv("t4", "name,price\nPlum,1.20\nApple,0.50\nPear,0.75\n");
    give(&mut app, &ctx, Command::ToggleTable);
    frame(&mut app, &ctx, Vec::new());
    let act = |app: &mut Limoniter, action: Action| {
        let all = app.docs[app.active].text.all_lines();
        let state = app.docs[app.active].table.as_deref().unwrap();
        let (records, shown) = (state.index.clone().unwrap().records, state.shown.clone());
        app.table_action(action, &all, &records, &shown, true);
    };
    state(&mut app).cursor = (0, 1);
    act(&mut app, Action::InsertColumn);
    assert_eq!(lines(&app)[..4], ["name,,price", "Plum,,1.20", "Apple,,0.50", "Pear,,0.75"]);
    frame(&mut app, &ctx, Vec::new());
    act(&mut app, Action::DeleteColumn);
    assert_eq!(lines(&app)[..4], ["name,price", "Plum,1.20", "Apple,0.50", "Pear,0.75"]);
    frame(&mut app, &ctx, Vec::new());
    act(&mut app, Action::SortFile);
    assert_eq!(lines(&app)[..4], ["name,price", "Apple,0.50", "Pear,0.75", "Plum,1.20"], "the headings stay at the top");
    frame(&mut app, &ctx, Vec::new());
    state(&mut app).cursor = (0, 0);
    act(&mut app, Action::InsertRow);
    assert_eq!(lines(&app)[..5], ["name,price", "Apple,0.50", ",", "Pear,0.75", "Plum,1.20"]);
    frame(&mut app, &ctx, Vec::new());
    assert_eq!(state(&mut app).cursor, (1, 0), "the cursor goes to the new row");
    act(&mut app, Action::DeleteRow);
    assert_eq!(lines(&app), ["name,price", "Apple,0.50", "Pear,0.75", "Plum,1.20", ""]);
    // Copy and Paste work on cells.
    frame(&mut app, &ctx, Vec::new());
    state(&mut app).cursor = (2, 0);
    give(&mut app, &ctx, Command::SelectAll);
    assert!(!app.docs[app.active].text.has_selection(), "Select All doesn't select the hidden text");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn find_moves_the_tables_cursor_to_the_row_it_found() {
    let (mut app, ctx, dir) = app_with_csv("t5", FRUIT);
    give(&mut app, &ctx, Command::ToggleTable);
    frame(&mut app, &ctx, Vec::new());
    // As Find leaves it: the text's cursor on the second line of Plum's notes.
    app.doc().text.move_caret(4, 2, false);
    frame(&mut app, &ctx, Vec::new());
    assert_eq!(state(&mut app).cursor.0, 2);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn columns_without_headings_are_lettered() {
    assert_eq!([0, 1, 25, 26, 27, 701, 702].map(column_letter), ["A", "B", "Z", "AA", "AB", "ZZ", "AAA"]);
}
