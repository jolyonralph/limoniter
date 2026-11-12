//! A part's history in the whole window: choosing the part, following it, and taking back one change to it.

use super::region_panel::part_around;
use super::*;

fn app_with_text(lines: &[&str]) -> (Limoniter, Context) {
    let mut app = Limoniter::build(String::new(), None, Some(PathBuf::from("never-written.png")));
    app.new_document();
    app.doc().text.set_content(lines.iter().map(|line| line.to_string()).collect());
    (app, Context::default())
}

fn frame(app: &mut Limoniter, ctx: &Context, events: Vec<egui::Event>) {
    let raw = egui::RawInput { screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1600.0, 900.0))), events, ..Default::default() };
    let mut output = ctx.run_ui(raw, |ui| app.draw_frame(ui));
    output.textures_delta.clear();
}

fn give(app: &mut Limoniter, ctx: &Context, command: Command) {
    let mut output = ctx.run_ui(egui::RawInput::default(), |ui| app.run(command, ui));
    output.textures_delta.clear();
}

fn lines(app: &Limoniter) -> Vec<String> {
    app.docs[app.active].text.all_lines().iter().map(|line| line.to_string()).collect()
}

/// Changes one line, as its own step in the history.
fn change(app: &mut Limoniter, line: usize, to: &str) {
    app.doc().text.splice_lines(line, line + 1, vec![Arc::from(to)]);
}

const CODE: [&str; 9] = ["fn one() {", "    a();", "}", "", "fn two() {", "    if ready {", "        b();", "    }", "}"];

#[test]
fn the_part_is_the_function_the_cursor_is_in() {
    let (mut app, _) = app_with_text(&CODE);
    let text = &app.doc().text;
    assert_eq!(part_around(text, 1), 0..3);
    assert_eq!(part_around(text, 6), 4..9, "the function, not the if inside it");
    assert_eq!(part_around(text, 4), 4..9, "from its first line too");
    let (mut app, _) = app_with_text(&["void f()", "{", "    x();", "}", "", "int g()", "{", "    return 1;", "}"]);
    assert_eq!(part_around(&app.doc().text, 7), 5..9, "a brace on its own line goes with the line above");
    let (mut app, _) = app_with_text(&["def f(x):", "    if x:", "        return 1", "    return 2", "", "def g():", "    pass"]);
    assert_eq!(part_around(&app.doc().text, 2), 0..4);
    let (mut app, _) = app_with_text(&["One paragraph", "goes on here.", "", "Another one."]);
    assert_eq!(part_around(&app.doc().text, 1), 0..2, "in prose, the paragraph");
}

#[test]
fn one_change_to_a_function_is_taken_back_and_everything_since_stays() {
    let (mut app, ctx) = app_with_text(&CODE);
    frame(&mut app, &ctx, Vec::new());
    change(&mut app, 1, "    a(1);");
    change(&mut app, 6, "        b(2);");
    app.doc().text.splice_lines(0, 0, vec![Arc::from("// added above"), Arc::from("")]);
    // Put the cursor in fn one, which is now two lines further down, and ask for its history.
    app.doc().text.move_caret(3, 2, false);
    give(&mut app, &ctx, Command::PartHistory);
    let state = app.region.as_ref().expect("the panel opens");
    assert_eq!(state.range, 2..5);
    assert_eq!(state.found.len(), 1, "only the change to fn one, not to fn two or the lines above: {:?}", state.found.iter().map(|s| &s.summary).collect::<Vec<_>>());
    assert_eq!(state.found[0].removed, ["    a();"]);
    assert_eq!(state.found[0].added, ["    a(1);"]);
    frame(&mut app, &ctx, Vec::new());
    app.take_back_part_change(0).unwrap();
    assert_eq!(lines(&app), ["// added above", "", "fn one() {", "    a();", "}", "", "fn two() {", "    if ready {", "        b(2);", "    }", "}"]);
    give(&mut app, &ctx, Command::Undo);
    assert_eq!(lines(&app)[3], "    a(1);", "Undo puts it back in one step");
    assert_eq!(lines(&app)[8], "        b(2);");
}

#[test]
fn the_part_is_followed_as_the_text_changes_around_it() {
    let (mut app, ctx) = app_with_text(&CODE);
    change(&mut app, 6, "        b(2);");
    app.doc().text.move_caret(6, 0, false);
    give(&mut app, &ctx, Command::PartHistory);
    assert_eq!(app.region.as_ref().unwrap().range, 4..9);
    app.doc().text.splice_lines(0, 0, vec![Arc::from("use std::io;"), Arc::from("use std::fs;"), Arc::from("")]);
    frame(&mut app, &ctx, Vec::new());
    let state = app.region.as_ref().unwrap();
    assert_eq!(state.range, 7..12, "three lines further down");
    assert_eq!(state.found.len(), 1);
    // A change inside it is listed as it's made.
    change(&mut app, 8, "    if ready && set {");
    frame(&mut app, &ctx, Vec::new());
    let state = app.region.as_ref().unwrap();
    assert_eq!(state.found.len(), 2);
    assert_eq!(state.found[0].summary, "Changed its line 2");
}

#[test]
fn a_change_made_again_can_only_be_put_back_with_the_whole_part() {
    let (mut app, ctx) = app_with_text(&CODE);
    change(&mut app, 1, "    a(1);");
    change(&mut app, 1, "    a(1, 2);");
    change(&mut app, 6, "        b(3);");
    app.doc().text.move_caret(1, 0, false);
    give(&mut app, &ctx, Command::PartHistory);
    let found = app.region.as_ref().unwrap().found.len();
    assert_eq!(found, 2);
    let oldest = found - 1;
    assert!(app.take_back_part_change(oldest).unwrap_err().contains("changed again"));
    frame(&mut app, &ctx, Vec::new());
    app.put_part_back(oldest).unwrap();
    assert_eq!(lines(&app)[..3], ["fn one() {", "    a();", "}"]);
    assert_eq!(lines(&app)[6], "        b(3);", "fn two keeps its later change");
}

#[test]
fn a_function_moved_elsewhere_keeps_its_history() {
    let (mut app, ctx) = app_with_text(&CODE);
    change(&mut app, 1, "    a(1);");
    // Cut fn one, and paste it at the end, as two steps.
    app.doc().text.splice_lines(0, 4, Vec::new());
    let end = app.docs[app.active].text.line_count();
    app.doc().text.splice_lines(end, end, ["", "fn one() {", "    a(1);", "}"].map(Arc::from).to_vec());
    app.doc().text.move_caret(7, 0, false);
    give(&mut app, &ctx, Command::PartHistory);
    let state = app.region.as_ref().unwrap();
    let summaries: Vec<&str> = state.found.iter().map(|shown| shown.summary.as_str()).collect();
    assert_eq!(summaries, ["Put in here, after being cut", "Cut from line 1", "Changed its line 2"]);
    frame(&mut app, &ctx, Vec::new());
    app.put_part_back(2).unwrap();
    assert_eq!(lines(&app)[6..9], ["fn one() {", "    a();", "}"], "put back as it was, where it is now");
}

#[test]
fn the_panel_draws_and_closes_and_needs_text() {
    let (mut app, ctx) = app_with_text(&CODE);
    change(&mut app, 1, "    a(1);");
    app.doc().text.move_caret(1, 0, false);
    frame(&mut app, &ctx, vec![egui::Event::Key { key: Key::H, physical_key: None, pressed: true, repeat: false, modifiers: Modifiers::CTRL | Modifiers::ALT }]);
    assert!(app.region.is_some(), "Ctrl+Alt+H opens it");
    assert!(!app.docs[app.active].hex_mode, "and not hex mode, which is Ctrl+H");
    if let Some(state) = &mut app.region {
        state.chosen = Some(0);
    }
    frame(&mut app, &ctx, Vec::new());
    give(&mut app, &ctx, Command::ToggleHex);
    assert!(!app.can(Command::PartHistory));
    give(&mut app, &ctx, Command::ToggleHex);
    let id = app.docs[app.active].id;
    // Taken away directly, as closing it would ask about saving.
    app.new_document();
    app.docs.retain(|doc| doc.id != id);
    app.active = 0;
    frame(&mut app, &ctx, Vec::new());
    assert!(app.docs.iter().all(|doc| doc.id != id));
    assert!(app.region.is_none(), "it goes with its file");
}

#[test]
fn ctrl_shift_v_is_swap_with_clipboard() {
    let (mut app, ctx) = app_with_text(&["text"]);
    app.doc().text.select_chars((0, 0), (0, 4));
    app.doc().set_read_only(true);
    frame(&mut app, &ctx, vec![egui::Event::Key { key: Key::V, physical_key: None, pressed: true, repeat: false, modifiers: Modifiers::CTRL | Modifiers::SHIFT }]);
    assert!(matches!(&app.dialog, Dialog::Message { body, .. } if body.contains("read-only")), "it reached Swap, which a read-only file refuses");
    let found = menus::every_command().into_iter().find(|each| each.command == Command::SwapClipboard).unwrap();
    assert_eq!(found.shortcut, "Ctrl+Shift+V");
}
