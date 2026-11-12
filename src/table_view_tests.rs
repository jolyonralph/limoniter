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

fn press_with(key: Key, modifiers: Modifiers) -> Vec<egui::Event> {
    vec![egui::Event::Key { key, physical_key: None, pressed: true, repeat: false, modifiers }, egui::Event::Key { key, physical_key: None, pressed: false, repeat: false, modifiers }]
}

/// Shift held down for a frame's keys; it's let go in the next frame, as a keyboard does.
fn shifted(events: Vec<egui::Event>) -> Vec<egui::Event> {
    std::iter::once(egui::Event::ModifiersChanged(Modifiers::SHIFT)).chain(events).collect()
}

#[test]
fn a_block_is_selected_copied_pasted_and_cleared_in_one_step_each() {
    let (mut app, ctx, dir) = app_with_csv("t6", "name,price,qty\nApple,0.50,3\nPear,0.75,\"1,000\"\nPlum,1.20,7\n");
    give(&mut app, &ctx, Command::ToggleTable);
    frame(&mut app, &ctx, Vec::new());
    frame(&mut app, &ctx, Vec::new());
    // Shift and the arrows select a block from the cursor.
    frame(&mut app, &ctx, press(Key::ArrowRight));
    frame(&mut app, &ctx, shifted(press_with(Key::ArrowDown, Modifiers::SHIFT)));
    frame(&mut app, &ctx, shifted(press_with(Key::ArrowRight, Modifiers::SHIFT)));
    assert_eq!(state(&mut app).selection(), ((0, 1), (1, 2)));
    frame(&mut app, &ctx, vec![egui::Event::ModifiersChanged(Modifiers::NONE)]);
    assert!(state(&mut app).figures.as_ref().unwrap().1.starts_with("2 by 2 cells: 4 filled, 4 numbers: total 1004.25"), "{:?}", state(&mut app).figures);
    // A move without Shift leaves just the cursor's cell, whose column's figures are shown.
    frame(&mut app, &ctx, press(Key::ArrowLeft));
    assert_eq!(state(&mut app).anchor, None);
    frame(&mut app, &ctx, Vec::new());
    assert!(state(&mut app).figures.as_ref().unwrap().1.starts_with("Column price: 3 filled, 3 numbers: total 2.45"), "{:?}", state(&mut app).figures);

    // Pasting a block, as a spreadsheet copies one, fills from the cursor and adds rows past the end, in one step.
    let all = app.docs[app.active].text.all_lines();
    let (records, shown) = (state(&mut app).index.clone().unwrap().records, state(&mut app).shown.clone());
    app.table_action(Action::Fill { at: (1, 1), block: table::parse_block("9.99\t2\r\n8.88\t4\r\n7.77\t6\r\n") }, &all, &records, &shown, true);
    assert_eq!(lines(&app), ["name,price,qty", "Apple,0.50,3", "Pear,9.99,\"2\"", "Plum,8.88,4", ",7.77,6", ""], "a field in quotes stays in quotes");
    assert_eq!(state(&mut app).selection(), ((1, 1), (3, 2)), "what was pasted is selected");
    give(&mut app, &ctx, Command::Undo);
    assert_eq!(lines(&app), ["name,price,qty", "Apple,0.50,3", "Pear,0.75,\"1,000\"", "Plum,1.20,7", ""], "one Undo takes the whole paste back");

    // Clearing a block empties only fields that are there.
    frame(&mut app, &ctx, Vec::new());
    state(&mut app).anchor = Some((0, 1));
    state(&mut app).cursor = (2, 4);
    let all = app.docs[app.active].text.all_lines();
    let (records, shown) = (state(&mut app).index.clone().unwrap().records, state(&mut app).shown.clone());
    app.table_action(Action::Clear, &all, &records, &shown, true);
    assert_eq!(lines(&app), ["name,price,qty", "Apple,,", "Pear,,\"\"", "Plum,,", ""]);
    give(&mut app, &ctx, Command::Undo);
    assert_eq!(lines(&app)[2], "Pear,0.75,\"1,000\"");

    // Select All takes in every cell shown.
    frame(&mut app, &ctx, Vec::new());
    give(&mut app, &ctx, Command::SelectAll);
    assert_eq!(state(&mut app).selection(), ((0, 0), (2, 2)));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn csv_files_can_open_straight_into_a_table() {
    let dir = std::env::temp_dir().join(format!("limoniter-table-auto-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("a.csv"), "x,y\n1,2\n").unwrap();
    std::fs::write(dir.join("b.txt"), "x,y\n1,2\n").unwrap();
    let mut app = Limoniter::build(String::new(), None, Some(PathBuf::from("never-written.png")));
    let ctx = Context::default();
    app.prefs.open_tables = true;
    assert!(app.open_path(&dir.join("a.csv")));
    frame(&mut app, &ctx, Vec::new());
    assert!(app.docs[app.active].table.is_some(), "a .csv file opens as a table");
    assert!(app.open_path(&dir.join("b.txt")));
    frame(&mut app, &ctx, Vec::new());
    assert!(app.docs[app.active].table.is_none(), "other files are only offered");
    assert!(app.docs[app.active].table_offer.is_some());
    // F6 shows it as a table, and again as text.
    frame(&mut app, &ctx, press(Key::F6));
    assert!(app.docs[app.active].table.is_some());
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn the_first_column_stays_in_view_when_scrolled_sideways() {
    let wide: String = (0..30).map(|c| format!("column{c}")).collect::<Vec<_>>().join(",") + "\n" + (0..20).map(|_| (0..30).map(|c| format!("value-{c}-xxxxxxxx")).collect::<Vec<_>>().join(",") + "\n").collect::<String>().as_str();
    let (mut app, ctx, dir) = app_with_csv("t8", &wide);
    give(&mut app, &ctx, Command::ToggleTable);
    state(&mut app).frozen = true;
    let narrow = |events: Vec<egui::Event>| egui::RawInput { screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(700.0, 500.0))), events, ..Default::default() };
    let run = |app: &mut Limoniter, events: Vec<egui::Event>| {
        let mut output = ctx.run_ui(narrow(events), |ui| app.draw_frame(ui));
        output.textures_delta.clear();
    };
    run(&mut app, Vec::new());
    run(&mut app, Vec::new());
    // Scroll a long way to the right, over the table.
    let over = egui::pos2(400.0, 300.0);
    run(&mut app, vec![egui::Event::PointerMoved(over), egui::Event::MouseWheel { unit: egui::MouseWheelUnit::Point, delta: egui::vec2(-3000.0, 0.0), modifiers: Modifiers::NONE, phase: egui::TouchPhase::Move, source: egui::MouseWheelSource::Wheel }]);
    for _ in 0..5 {
        run(&mut app, vec![egui::Event::PointerMoved(over)]);
    }
    // Just right of the row numbers is the first column, pinned there, so a click there picks it.
    let numbers_and_a_bit = egui::pos2(70.0, 300.0);
    let click = |pressed| egui::Event::PointerButton { pos: numbers_and_a_bit, button: egui::PointerButton::Primary, pressed, modifiers: Modifiers::NONE };
    state(&mut app).cursor = (0, 5);
    run(&mut app, vec![egui::Event::PointerMoved(numbers_and_a_bit), click(true)]);
    run(&mut app, vec![click(false)]);
    assert_eq!(state(&mut app).cursor.1, 0, "the pinned first column took the click");
    // Without pinning, the same place is some column far to the right.
    state(&mut app).frozen = false;
    run(&mut app, vec![egui::Event::PointerMoved(numbers_and_a_bit), click(true)]);
    run(&mut app, vec![click(false)]);
    assert!(state(&mut app).cursor.1 > 5, "{:?}", state(&mut app).cursor);
    let _ = std::fs::remove_dir_all(&dir);
}
