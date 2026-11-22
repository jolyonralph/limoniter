//! The data viewer, run as the whole app with no screen: tables opening in tabs, files that open in it, the
//! clipboard, and the Variables panel on the stand-in kernel.

use super::*;
use crate::data_table::{Cell, ColumnKind, DataColumn, DataTable};
use crate::data_view::DataState;

fn sized(events: Vec<egui::Event>) -> egui::RawInput {
    egui::RawInput { screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1600.0, 900.0))), events, ..Default::default() }
}

fn frame(app: &mut Limoniter, ctx: &Context, events: Vec<egui::Event>) -> egui::FullOutput {
    let mut output = ctx.run_ui(sized(events), |ui| app.draw_frame(ui));
    output.textures_delta.clear();
    output
}

fn frames(app: &mut Limoniter, ctx: &Context, count: usize) {
    for _ in 0..count {
        frame(app, ctx, Vec::new());
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
}

fn give(app: &mut Limoniter, ctx: &Context, command: Command) -> egui::FullOutput {
    let mut output = ctx.run_ui(sized(Vec::new()), |ui| app.run(command, ui));
    output.textures_delta.clear();
    output
}

fn app() -> (Limoniter, Context) {
    (Limoniter::build(String::new(), None, Some(PathBuf::from("never-written.png"))), Context::default())
}

fn table(rows: usize) -> DataTable {
    DataTable {
        columns: vec![DataColumn { name: "id".into(), kind: ColumnKind::Unknown }, DataColumn { name: "name".into(), kind: ColumnKind::Text }],
        rows: (0..rows).map(|i| vec![Cell::Text(((i * 7) % 10).to_string()), Cell::Text(format!("n{i}"))]).collect(),
        note: Some("The first 3 of 99 rows".into()),
    }
}

fn data(app: &mut Limoniter) -> &mut DataState {
    app.docs[app.active].data.as_deref_mut().expect("shown in the data viewer")
}

/// Lets the sheet in front finish sorting or filtering.
fn settle(app: &mut Limoniter, ctx: &Context) {
    for _ in 0..400 {
        frame(app, ctx, Vec::new());
        if !data(app).busy() {
            return;
        }
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    panic!("the viewer never settled");
}

fn copied(output: &egui::FullOutput) -> Option<String> {
    output.platform_output.commands.iter().find_map(|command| match command {
        egui::OutputCommand::CopyText(text) => Some(text.clone()),
        _ => None,
    })
}

fn test_file(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("limoniter-data-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let to = dir.join(name);
    std::fs::copy(Path::new(env!("CARGO_MANIFEST_DIR")).join("src/data_testdata").join(name), &to).unwrap();
    to
}

#[test]
fn a_table_opens_in_a_tab_of_its_own_read_only_with_its_title() {
    let (mut app, ctx) = app();
    app.open_data_table("Query result", table(5));
    assert_eq!(app.docs.len(), 1, "the empty tab gives way");
    assert_eq!(app.docs[app.active].name(), "Query result");
    assert!(app.docs[app.active].text.read_only);
    assert!(!app.docs[app.active].is_modified());
    assert!(!app.docs[app.active].is_blank(), "the table isn't an empty tab to be replaced");
    frames(&mut app, &ctx, 3);
    assert_eq!(data(&mut app).sheets[0].table.columns[0].kind, ColumnKind::Integer, "guessed from the values");
    // A second table opens beside the first.
    app.open_data_table("Another", table(2));
    assert_eq!(app.docs.len(), 2);
    frames(&mut app, &ctx, 2);
}

#[test]
fn clicking_a_heading_sorts_and_typing_filters() {
    let (mut app, ctx) = app();
    app.open_data_table("T", table(10));
    frames(&mut app, &ctx, 2);
    app.data_step("sort-0");
    settle(&mut app, &ctx);
    let sheet = &data(&mut app).sheets[0];
    let ids: Vec<String> = sheet.rows.iter().map(|&r| sheet.table.rows[r as usize][0].text()).collect();
    let mut sorted = ids.clone();
    sorted.sort_by_key(|v| v.parse::<u32>().unwrap());
    assert_eq!(ids, sorted, "numbers in order");
    app.data_step("sort-0");
    settle(&mut app, &ctx);
    let sheet = &data(&mut app).sheets[0];
    assert_eq!(sheet.table.rows[sheet.rows[0] as usize][0].text(), "9", "then backwards");
    app.data_step("filter-n3");
    settle(&mut app, &ctx);
    assert_eq!(data(&mut app).sheets[0].rows.len(), 1);
    app.data_step("filter-");
    settle(&mut app, &ctx);
    assert_eq!(data(&mut app).sheets[0].rows.len(), 10);
}

#[test]
fn copy_and_select_all_give_the_cells_as_the_table_view_does() {
    let (mut app, ctx) = app();
    app.open_data_table("T", table(3));
    frames(&mut app, &ctx, 2);
    app.data_step("cell-1-1");
    let output = give(&mut app, &ctx, Command::Copy);
    assert_eq!(copied(&output).as_deref(), Some("n1"));
    give(&mut app, &ctx, Command::SelectAll);
    let output = give(&mut app, &ctx, Command::Copy);
    assert_eq!(copied(&output).as_deref(), Some("0\tn0\n7\tn1\n4\tn2"));
    // The keyboard's own Copy event does the same while the viewer has the keys.
    frames(&mut app, &ctx, 2);
    let output = frame(&mut app, &ctx, vec![egui::Event::Copy]);
    assert_eq!(copied(&output).as_deref(), Some("0\tn0\n7\tn1\n4\tn2"));
}

#[test]
fn a_spreadsheet_opens_in_the_viewer_with_its_sheets() {
    let (mut app, ctx) = app();
    let path = test_file("people.xlsx");
    assert!(app.open_path(&path));
    assert_eq!(app.docs.len(), 1);
    assert_eq!(app.docs[app.active].name(), "people.xlsx");
    assert!(app.docs[app.active].data.is_some() && app.docs[app.active].path.is_none());
    frames(&mut app, &ctx, 3);
    let state = data(&mut app);
    assert_eq!(state.sheets.iter().map(|s| s.name.as_str()).collect::<Vec<_>>(), ["People", "Plain"]);
    assert_eq!(state.sheets[0].table.columns[3].kind, ColumnKind::Date, "dates sort as dates");
    assert_eq!(state.sheets[0].table.columns[4].kind, ColumnKind::DateTime);
    assert_eq!(state.sheets[0].table.columns[1].kind, ColumnKind::Integer);
    app.data_step("sheet-1");
    frames(&mut app, &ctx, 2);
    assert_eq!(data(&mut app).active, 1);
    // Save as CSV and Save write the rows shown, never the empty text under the tab.
    assert!(!app.docs[app.active].is_modified());
    let _ = std::fs::remove_dir_all(path.parent().unwrap());
}

#[test]
fn a_json_lines_file_opens_in_the_viewer_and_can_be_shown_as_text() {
    let (mut app, ctx) = app();
    let path = test_file("small.jsonl");
    assert!(app.open_path(&path));
    frames(&mut app, &ctx, 2);
    let state = data(&mut app);
    assert_eq!(state.sheets.len(), 1);
    assert_eq!(state.sheets[0].table.rows.len(), 3);
    assert_eq!(state.from.as_deref(), Some(path.as_path()));
    app.open_data_as_text(&path);
    assert_eq!(app.docs.len(), 2);
    assert!(app.docs[app.active].data.is_none() && app.docs[app.active].text.line(0).starts_with("{\"id\":1"));
    let _ = std::fs::remove_dir_all(path.parent().unwrap());
}

#[test]
fn a_file_that_is_not_a_workbook_is_not_opened() {
    let (mut app, _ctx) = app();
    let dir = std::env::temp_dir().join(format!("limoniter-data-bad-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("broken.xlsx");
    std::fs::write(&path, b"this is not a workbook").unwrap();
    assert!(!app.open_path(&path));
    assert!(app.docs.iter().all(|d| d.data.is_none()));
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn delimited_text_opens_in_the_viewer_from_the_view_menu() {
    let (mut app, ctx) = app();
    app.new_document();
    app.doc().text.type_text("name,age\n\"Smith, J\",41\nLee,7\n");
    app.doc().untitled = "people.csv".into();
    assert!(app.can(Command::OpenInDataViewer), "a CSV by its name");
    give(&mut app, &ctx, Command::OpenInDataViewer);
    assert_eq!(app.docs.len(), 2, "the text stays beside it");
    let sheet = &data(&mut app).sheets[0];
    assert_eq!(sheet.table.columns.iter().map(|c| c.name.as_str()).collect::<Vec<_>>(), ["name", "age"]);
    assert_eq!(sheet.table.rows[0], vec![Cell::Text("Smith, J".into()), Cell::Text("41".into())]);
    assert_eq!(sheet.table.columns[1].kind, ColumnKind::Integer);
    assert!(!app.can(Command::OpenInDataViewer), "not for the viewer's own tab");
}

#[test]
fn a_million_rows_are_drawn_quickly() {
    let (mut app, ctx) = app();
    let rows = (0..1_000_000u32).map(|i| vec![Cell::Int(i64::from(i)), Cell::Text(format!("row {i}"))]).collect();
    app.open_data_table("Big", DataTable { columns: vec![DataColumn { name: "n".into(), kind: ColumnKind::Integer }, DataColumn { name: "w".into(), kind: ColumnKind::Text }], rows, note: None });
    let started = std::time::Instant::now();
    frames(&mut app, &ctx, 5);
    assert!(started.elapsed().as_secs() < 20, "five frames of a million rows took {:?}", started.elapsed());
    assert_eq!(data(&mut app).sheets[0].rows.len(), 1_000_000);
}

#[test]
fn the_variables_panel_lists_what_the_kernel_holds_and_opens_a_table() {
    let (mut app, ctx) = app();
    let dir = std::env::temp_dir().join(format!("limoniter-vars-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join("work.ipynb");
    std::fs::write(&file, include_str!("notebook_testdata/outputs.ipynb")).unwrap();
    assert!(app.open_path(&file));
    app.panels.variables = true;
    frames(&mut app, &ctx, 3);
    assert!(app.docs[app.active].notebook.as_deref().is_some_and(|n| n.vars.list.is_empty()), "no kernel yet, so nothing is asked");
    // Running a cell starts the (stand-in) kernel, and the variables are asked for when it has finished.
    app.notebook_command(notebook_host::Act::RunAll);
    for _ in 0..200 {
        frame(&mut app, &ctx, Vec::new());
        if app.docs[app.active].notebook.as_deref().is_some_and(|n| !n.vars.list.is_empty()) {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    let state = app.docs[app.active].notebook.as_deref_mut().unwrap();
    let names: Vec<&str> = state.vars.list.iter().map(|v| v.name.as_str()).collect();
    assert_eq!(names, ["frame", "n"]);
    assert!(state.vars.list[0].shape.opens() && !state.vars.list[1].shape.opens());
    // Opening the frame asks the kernel for its rows, and the viewer opens with them.
    state.open_variable("frame");
    for _ in 0..200 {
        frame(&mut app, &ctx, Vec::new());
        if app.docs[app.active].data.is_some() {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    let sheet = &data(&mut app).sheets[0];
    assert_eq!(sheet.table.rows.len(), 3);
    assert_eq!(sheet.table.note.as_deref(), Some("The first 3 of 250,000 rows"));
    assert!(app.docs[app.active].name().contains("frame"), "{}", app.docs[app.active].name());
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn the_variables_panel_says_so_when_no_notebook_is_in_front() {
    let (mut app, ctx) = app();
    app.panels.variables = true;
    app.open_data_table("T", table(2));
    frames(&mut app, &ctx, 3);
    assert!(app.panels.variables, "drawing it with no notebook leaves it open");
}
