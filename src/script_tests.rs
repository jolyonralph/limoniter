//! Macros and scripts in the whole window, from the keys pressed to the file changed.

use super::script_host::{ScriptBox, SCRIPT_COMMANDS};
use super::*;
use crate::scripting::{self, Answer, Repeat};
use std::time::Instant;

fn app_with_text(lines: &[&str]) -> (Limoniter, Context) {
    let mut app = Limoniter::build(String::new(), None, Some(PathBuf::from("never-written.png")));
    app.new_document();
    app.doc().text.set_content(lines.iter().map(|line| line.to_string()).collect());
    (app, Context::default())
}

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

/// Plays a script and draws frames until it's done. Any box left open by it is for the test to look at.
fn play(app: &mut Limoniter, ctx: &Context, source: &str) {
    play_as(app, ctx, source, Repeat::Once);
}

fn play_as(app: &mut Limoniter, ctx: &Context, source: &str, repeat: Repeat) {
    let mut output = ctx.run_ui(sized(Vec::new()), |ui| app.play_source("Test", source, repeat, ui));
    output.textures_delta.clear();
    finish(app, ctx);
}

fn finish(app: &mut Limoniter, ctx: &Context) {
    let began = Instant::now();
    while app.script.is_some() && matches!(app.dialog, Dialog::None) {
        frame(app, ctx, Vec::new());
        assert!(began.elapsed() < Duration::from_secs(60), "the script never finished");
    }
}

fn lines(app: &Limoniter) -> Vec<String> {
    app.docs[app.active].text.all_lines().iter().map(|line| line.to_string()).collect()
}

fn told(app: &Limoniter) -> String {
    match &app.dialog {
        Dialog::Message { body, .. } => body.clone(),
        _ => String::new(),
    }
}

/// Puts the keyboard on the text, as it is when someone's typing.
fn focus_text(app: &mut Limoniter, ctx: &Context) {
    app.pending_focus = true;
    frame(app, ctx, Vec::new());
    frame(app, ctx, Vec::new());
}

fn pressed(key: Key, modifiers: Modifiers) -> egui::Event {
    egui::Event::Key { key, physical_key: None, pressed: true, repeat: false, modifiers }
}

fn panel_action(app: &mut Limoniter, ctx: &Context, action: scripting::LibraryAction) {
    let mut action = Some(action);
    let mut output = ctx.run_ui(sized(Vec::new()), |ui| {
        if let Some(action) = action.take() {
            app.library_action(action, ui);
        }
    });
    output.textures_delta.clear();
}

fn with_library(app: &mut Limoniter, tag: &str) -> PathBuf {
    let root = std::env::temp_dir().join(format!("limoniter-host-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    app.library = scripting::Library::new(Some(root.clone()));
    app.library.refresh();
    root
}

#[test]
fn every_scripted_command_has_its_own_name() {
    let mut names: Vec<&str> = SCRIPT_COMMANDS.iter().map(|(name, _)| *name).collect();
    names.sort_unstable();
    names.dedup();
    assert_eq!(names.len(), SCRIPT_COMMANDS.len());
    for (name, _) in SCRIPT_COMMANDS {
        assert!(name.chars().all(|c| c.is_ascii_lowercase() || c == '_'), "{name}");
    }
}

#[test]
fn a_whole_run_comes_off_in_one_undo() {
    let (mut app, ctx) = app_with_text(&["one", "two", "three"]);
    let version = app.docs[0].text.version();
    play(&mut app, &ctx, r#"for n in 1..=line_count() { set_line(n, "changed " + n); } press("Ctrl+End"); type_text("!"); command("upper_case"); replace_all("CHANGED", "new");"#);
    assert_eq!(told(&app), "");
    assert_ne!(lines(&app), ["one", "two", "three"]);
    give(&mut app, &ctx, Command::Undo);
    assert_eq!(lines(&app), ["one", "two", "three"]);
    assert_eq!(app.docs[0].text.version(), version);
    assert!(!app.docs[0].text.can_undo(), "there was only the one step");
    give(&mut app, &ctx, Command::Redo);
    assert!(lines(&app)[2].ends_with('!'));
}

#[test]
fn edits_before_a_run_keep_their_own_undo() {
    let (mut app, ctx) = app_with_text(&["start"]);
    app.doc().text.type_text("typed first");
    play(&mut app, &ctx, r#"set_line(1, "by script"); set_line(1, "by script again");"#);
    assert_eq!(lines(&app), ["by script again"]);
    give(&mut app, &ctx, Command::Undo);
    assert_eq!(lines(&app), ["typed firststart"]);
    give(&mut app, &ctx, Command::Undo);
    assert_eq!(lines(&app), ["start"]);
}

#[test]
fn a_run_that_changes_nothing_adds_no_undo_step() {
    let (mut app, ctx) = app_with_text(&["start"]);
    play(&mut app, &ctx, "let n = line_count(); go_to(1, 3); select_all();");
    assert_eq!(told(&app), "");
    assert!(!app.docs[0].text.can_undo());
    assert!(!app.docs[0].is_modified());
}

#[test]
fn a_script_using_undo_gets_its_own_steps_and_still_one_undo_after() {
    let (mut app, ctx) = app_with_text(&["start"]);
    play(&mut app, &ctx, r#"set_line(1, "first"); go_to(1, 1); type_text("x"); command("undo"); type_text("y");"#);
    assert_eq!(lines(&app), ["yfirst"], "only the last step came off");
    give(&mut app, &ctx, Command::Undo);
    assert_eq!(lines(&app), ["start"]);
}

#[test]
fn more_steps_than_undo_keeps_leave_the_start_still_reachable() {
    let (mut app, ctx) = app_with_text(&["start"]);
    play(&mut app, &ctx, r#"for n in 0..500 { type_text("z"); press("Left"); type_text("y"); } command("undo");"#);
    assert_eq!(told(&app), "");
    give(&mut app, &ctx, Command::Undo);
    assert_eq!(lines(&app), ["start"]);
}

#[test]
fn an_error_says_what_happened_and_that_undo_can_take_it_back() {
    let (mut app, ctx) = app_with_text(&["start"]);
    play(&mut app, &ctx, "set_line(1, \"changed\");\nline_text(99);\nset_line(1, \"never\");");
    let body = told(&app);
    assert!(body.contains("no line 99") && body.contains("line 2") && body.contains("Undo"), "{body}");
    assert_eq!(lines(&app), ["changed"]);
    app.dialog = Dialog::None;
    give(&mut app, &ctx, Command::Undo);
    assert_eq!(lines(&app), ["start"]);
}

#[test]
fn a_mistake_in_the_writing_changes_nothing_and_doesnt_mention_undo() {
    let (mut app, ctx) = app_with_text(&["start"]);
    play(&mut app, &ctx, "set_line(1, \"changed\");\nlet = 1;");
    let body = told(&app);
    assert!(body.contains("line 2") && !body.contains("Undo"), "{body}");
    assert_eq!(lines(&app), ["start"]);
}

#[test]
fn the_window_stays_alive_during_an_endless_script_and_stop_ends_it() {
    let (mut app, ctx) = app_with_text(&["start"]);
    play_no_wait(&mut app, &ctx, r#"set_line(1, "changed"); loop { let a = 1; }"#);
    let began = Instant::now();
    while lines(&app) != ["changed"] {
        frame(&mut app, &ctx, Vec::new());
        assert!(began.elapsed() < Duration::from_secs(10));
    }
    let drawn = Instant::now();
    frame(&mut app, &ctx, Vec::new());
    assert!(drawn.elapsed() < Duration::from_secs(2), "a frame took {:?}", drawn.elapsed());
    assert!(app.script.is_some());
    // Nothing else gets in while it's running.
    give(&mut app, &ctx, Command::Upper);
    app.doc().text.notice = None;
    frame(&mut app, &ctx, vec![egui::Event::Text("typed".into())]);
    assert_eq!(lines(&app), ["changed"]);
    app.stop_script();
    assert!(app.script.is_none());
    assert!(told(&app).contains("Undo"));
    app.dialog = Dialog::None;
    give(&mut app, &ctx, Command::Undo);
    assert_eq!(lines(&app), ["start"]);
}

fn play_no_wait(app: &mut Limoniter, ctx: &Context, source: &str) {
    let mut output = ctx.run_ui(sized(Vec::new()), |ui| app.play_source("Test", source, Repeat::Once, ui));
    output.textures_delta.clear();
}

#[test]
fn escape_stops_a_script_once_the_stop_button_is_showing() {
    let (mut app, ctx) = app_with_text(&["start"]);
    play_no_wait(&mut app, &ctx, "loop { let a = 1; }");
    let began = Instant::now();
    while began.elapsed() < Duration::from_millis(500) {
        frame(&mut app, &ctx, Vec::new());
    }
    frame(&mut app, &ctx, vec![pressed(Key::Escape, Modifiers::NONE)]);
    assert!(app.script.is_none());
    assert_eq!(told(&app), "", "it changed nothing, so there's nothing to say");
}

#[test]
fn recording_keys_and_commands_makes_a_macro_that_plays_the_same() {
    let (mut app, ctx) = app_with_text(&["alpha", "beta", "gamma"]);
    focus_text(&mut app, &ctx);
    give(&mut app, &ctx, Command::RecordMacro);
    assert!(scripting::recording());
    frame(&mut app, &ctx, vec![pressed(Key::End, Modifiers::NONE), egui::Event::Text("!".into()), egui::Event::Text("?".into())]);
    give(&mut app, &ctx, Command::SelectAll);
    give(&mut app, &ctx, Command::Upper);
    // A command given outside a frame leaves nothing with the keyboard, which a real frame always draws.
    focus_text(&mut app, &ctx);
    frame(&mut app, &ctx, vec![pressed(Key::Home, Modifiers::CTRL), pressed(Key::ArrowDown, Modifiers::NONE), pressed(Key::ArrowDown, Modifiers::NONE)]);
    frame(&mut app, &ctx, vec![pressed(Key::End, Modifiers::NONE), pressed(Key::Enter, Modifiers::NONE)]);
    give(&mut app, &ctx, Command::RecordMacro);
    assert!(!scripting::recording());
    let result = lines(&app);
    assert_eq!(result, ["ALPHA!?", "BETA", "GAMMA", ""]);
    let Dialog::Script(ScriptBox::SaveMacro { source, .. }) = &app.dialog else {
        panic!("it should ask for a name");
    };
    let source = source.clone();
    assert_eq!(source, "press(\"End\");\ntype_text(\"!?\");\ncommand(\"select_all\");\ncommand(\"upper_case\");\npress(\"Ctrl+Home\");\npress(\"Down\", 2);\npress(\"End\");\npress(\"Enter\");\n");
    app.dialog = Dialog::None;

    // On another copy of the same text, it comes out the same.
    let (mut other, other_ctx) = app_with_text(&["alpha", "beta", "gamma"]);
    other.current_macro = app.current_macro.take();
    give(&mut other, &other_ctx, Command::PlayMacro);
    finish(&mut other, &other_ctx);
    assert_eq!(lines(&other), result);
}

#[test]
fn recording_nothing_makes_nothing_and_says_so() {
    let (mut app, ctx) = app_with_text(&["alpha"]);
    give(&mut app, &ctx, Command::RecordMacro);
    frame(&mut app, &ctx, Vec::new());
    give(&mut app, &ctx, Command::RecordMacro);
    assert!(told(&app).contains("Nothing was recorded"));
    assert!(app.current_macro.is_none());
}

#[test]
fn searching_and_going_to_are_recorded_with_what_they_were_for() {
    let (mut app, ctx) = app_with_text(&["cat", "dog", "cat"]);
    give(&mut app, &ctx, Command::RecordMacro);
    app.find.find = "d\"og".into();
    app.find.match_case = true;
    app.run_find(false);
    app.find.find = "cat".into();
    app.find.replace = "cow".into();
    app.find.match_case = false;
    app.find.regex = true;
    app.run_replace_all();
    app.run_find(true);
    app.dialog = Dialog::GoTo { value: "2".into(), error: String::new() };
    frame(&mut app, &ctx, vec![pressed(Key::Enter, Modifiers::NONE)]);
    assert!(matches!(app.dialog, Dialog::None));
    let source = scripting::stop_recording().unwrap();
    assert_eq!(source, "find(\"d\\\"og\", #{ match_case: true });\nreplace_all(\"cat\", \"cow\", #{ regex: true });\nfind(\"cat\", #{ regex: true, backward: true });\ngo_to_place(\"2\");\n");
    let (mut other, other_ctx) = app_with_text(&["cat", "dog", "cat"]);
    play(&mut other, &other_ctx, &source);
    assert_eq!(told(&other), "");
    assert_eq!(lines(&other), ["cow", "dog", "cow"]);
    assert_eq!(other.docs[0].text.caret_line, 1);
}

#[test]
fn a_macro_cant_be_played_while_recording() {
    let (mut app, ctx) = app_with_text(&["alpha"]);
    give(&mut app, &ctx, Command::RecordMacro);
    play(&mut app, &ctx, r#"type_text("x");"#);
    assert!(told(&app).contains("Stop recording"));
    assert_eq!(lines(&app), ["alpha"]);
    scripting::stop_recording();
}

#[test]
fn a_recording_doesnt_start_in_hex_mode() {
    let (mut app, ctx) = app_with_text(&["alpha"]);
    give(&mut app, &ctx, Command::ToggleHex);
    give(&mut app, &ctx, Command::RecordMacro);
    assert!(!scripting::recording());
    assert!(told(&app).contains("text mode"));
}

#[test]
fn searching_in_a_script_stops_at_the_end_so_loops_end() {
    let (mut app, ctx) = app_with_text(&["cat one", "dog", "cat two"]);
    play(&mut app, &ctx, r#"let n = 0; while find("cat") { n += 1; type_text("cow" + n); } type_text("!");"#);
    assert_eq!(told(&app), "");
    assert_eq!(lines(&app), ["cow1 one", "dog", "cow2! two"]);
    play(&mut app, &ctx, r#"press("Ctrl+End"); let n = 0; while find("cow", #{ backward: true }) { n += 1; if n > 10 { break; } } set_line(2, "" + n);"#);
    assert_eq!(lines(&app)[1], "2");
    // With nothing to find, the selection stays where it was.
    play(&mut app, &ctx, r#"go_to(1, 2); if find("nothing here") { set_line(1, "wrong"); }"#);
    assert_eq!(lines(&app)[0], "cow1 one");
    assert_eq!(app.docs[0].text.caret_col, 1);
    // A pattern matching nowhere in particular doesn't go on forever.
    play(&mut app, &ctx, r#"go_to(1, 1); let n = 0; while find("^", #{ regex: true }) { n += 1; if n > 100 { set_line(1, "looped"); break; } }"#);
    assert_ne!(lines(&app)[0], "looped");
}

#[test]
fn replacing_counts_and_goes_by_its_options() {
    let (mut app, ctx) = app_with_text(&["Cat cat scatter"]);
    play(&mut app, &ctx, r#"let n = replace_all("cat", "dog", #{ match_case: true, whole_word: true }); type_text("" + n);"#);
    assert_eq!(told(&app), "");
    assert!(lines(&app)[0].contains("Cat dog scatter"), "{:?}", lines(&app));
    assert!(lines(&app)[0].starts_with('1'));
    play(&mut app, &ctx, r#"replace_all("(", "x", #{ regex: true });"#);
    assert!(!told(&app).is_empty(), "a bad pattern is explained");
}

#[test]
fn text_in_an_old_encoding_reads_and_writes_as_ordinary_characters() {
    let (mut app, ctx) = app_with_text(&[""]);
    let set = charset::all().iter().copied().find(|set| set.keep_bytes && set.table(0).is_some_and(|table| table.byte_of('\u{E9}').is_some())).expect("a set that keeps its bytes and has this");
    app.doc().text.format = TextFormat { charset: set, ..TextFormat::default() };
    app.doc().text.type_text("caf\u{E9}");
    let held = lines(&app)[0].clone();
    assert_ne!(held, "caf\u{E9}", "held as its byte");
    play(&mut app, &ctx, "if line_text(1) == \"caf\u{E9}\" { set_line(1, line_text(1) + \"\u{E9}\"); }\nif find(\"\u{E9}\") { type_text(\"e\"); }");
    assert_eq!(told(&app), "");
    let doc = &app.docs[0];
    assert_eq!(doc.text.format.readable_text(&lines(&app)[0]), "caf\u{E9}e", "the search began after the first, where the cursor was");
}

#[test]
fn a_locked_file_or_hex_mode_is_refused_in_words() {
    let (mut app, ctx) = app_with_text(&["alpha"]);
    app.doc().set_read_only(true);
    play(&mut app, &ctx, r#"let n = line_count(); set_line(1, "no");"#);
    assert!(told(&app).contains("locked"), "{}", told(&app));
    assert_eq!(lines(&app), ["alpha"]);
    app.dialog = Dialog::None;
    app.doc().set_read_only(false);
    give(&mut app, &ctx, Command::ToggleHex);
    play(&mut app, &ctx, "line_text(1);");
    assert!(told(&app).contains("hex mode"), "{}", told(&app));
}

#[test]
fn with_no_file_open_a_script_can_make_one_but_not_read_one() {
    let (mut app, ctx) = app_with_text(&["alpha"]);
    app.close_tab(0);
    assert!(app.no_file());
    play(&mut app, &ctx, "line_count();");
    assert!(told(&app).contains("No file is open"), "{}", told(&app));
    app.dialog = Dialog::None;
    play(&mut app, &ctx, r#"new_file(); type_text("made");"#);
    assert_eq!(told(&app), "");
    assert_eq!(lines(&app), ["made"]);
}

#[test]
fn a_box_holds_a_script_until_its_answered() {
    let (mut app, ctx) = app_with_text(&[""]);
    play(&mut app, &ctx, r#"let a = ask("Name?", "suggested"); type_text(a); if confirm("More?") { type_text(" yes"); } else { type_text(" no"); } message("Done"); type_text(" after");"#);
    assert!(matches!(&app.dialog, Dialog::Script(ScriptBox::Ask { value, .. }) if value == "suggested"));
    assert_eq!(lines(&app), [""], "it's waiting");
    frame(&mut app, &ctx, Vec::new());
    assert_eq!(lines(&app), [""]);
    app.script_answer = Some(Answer::Text("Jo".into()));
    app.dialog = Dialog::None;
    finish(&mut app, &ctx);
    assert!(matches!(&app.dialog, Dialog::Script(ScriptBox::Confirm { .. })));
    assert_eq!(lines(&app), ["Jo"]);
    app.script_answer = Some(Answer::Yes(false));
    app.dialog = Dialog::None;
    finish(&mut app, &ctx);
    assert_eq!(told(&app), "Done");
    assert_eq!(lines(&app), ["Jo no"]);
    app.dialog = Dialog::None;
    finish(&mut app, &ctx);
    assert_eq!(lines(&app), ["Jo no after"]);
    assert!(app.script.is_none());
}

#[test]
fn escape_on_a_scripts_question_gives_it_an_empty_answer() {
    let (mut app, ctx) = app_with_text(&[""]);
    play(&mut app, &ctx, r#"let a = ask("Name?", "suggested"); type_text("[" + a + "]");"#);
    frame(&mut app, &ctx, vec![pressed(Key::Escape, Modifiers::NONE)]);
    finish(&mut app, &ctx);
    assert_eq!(lines(&app), ["[]"]);
}

#[test]
fn printing_goes_to_the_output_window_replacing_what_was_there() {
    let (mut app, ctx) = app_with_text(&["alpha"]);
    play(&mut app, &ctx, r#"print("first"); print(1 + 2); debug("seen"); print("two\nlines");"#);
    assert!(app.show_results);
    assert_eq!(app.script_output.len(), 5);
    assert_eq!(app.script_output[..2], ["first", "3"]);
    frame(&mut app, &ctx, Vec::new());
    play(&mut app, &ctx, r#"print("again");"#);
    assert_eq!(app.script_output, ["again"]);
}

#[test]
fn a_macro_plays_a_number_of_times_or_to_the_end_of_the_file() {
    let (mut app, ctx) = app_with_text(&["a", "b", "c", "d"]);
    play_as(&mut app, &ctx, r#"press("Home"); type_text("> "); press("Down");"#, Repeat::Times(2));
    assert_eq!(lines(&app), ["> a", "> b", "c", "d"]);
    give(&mut app, &ctx, Command::Undo);
    assert_eq!(lines(&app), ["a", "b", "c", "d"], "all the plays come off together");
    play_as(&mut app, &ctx, r#"press("Home"); type_text("> "); press("Down");"#, Repeat::UntilEnd);
    assert_eq!(lines(&app), ["> a", "> b", "> c", "> d"], "every line, the last included, and it stopped");
}

#[test]
fn playing_to_the_end_counts_a_macro_that_removes_lines_as_moving_on() {
    let (mut app, ctx) = app_with_text(&["a", "b", "c", "d"]);
    play_as(&mut app, &ctx, "delete_line(current_line());", Repeat::UntilEnd);
    assert_eq!(lines(&app), [""]);
}

#[test]
fn a_macro_that_stays_put_only_plays_once_to_the_end() {
    let (mut app, ctx) = app_with_text(&["a", "b"]);
    play_as(&mut app, &ctx, r#"type_text("x");"#, Repeat::UntilEnd);
    assert_eq!(lines(&app), ["xa", "b"]);
}

#[test]
fn a_script_can_go_through_all_the_open_files() {
    let (mut app, ctx) = app_with_text(&["first"]);
    app.new_document();
    app.doc().text.type_text("second");
    play(&mut app, &ctx, r#"for n in 1..=file_count() { press("Ctrl+Home"); type_text("* "); next_file(); }"#);
    assert_eq!(told(&app), "");
    assert_eq!(app.docs[0].text.line(0).as_ref(), "* first");
    assert_eq!(app.docs[1].text.line(0).as_ref(), "* second");
    app.active = 0;
    give(&mut app, &ctx, Command::Undo);
    assert_eq!(app.docs[0].text.line(0).as_ref(), "first");
    assert_eq!(app.docs[1].text.line(0).as_ref(), "* second", "each file has its own undo");
}

#[test]
fn a_script_opens_and_saves_files_but_wont_write_over_another() {
    let (mut app, ctx) = app_with_text(&[""]);
    let dir = std::env::temp_dir().join(format!("limoniter-script-files-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let there = dir.join("there.txt");
    std::fs::write(&there, "already here\n").unwrap();
    let new = dir.join("new.txt");
    let name = |path: &Path| scripting::quoted(&path.display().to_string());
    play(&mut app, &ctx, &format!("type_text(\"words\"); if !save_as({}) {{ type_text(\" failed\"); }}", name(&new)));
    assert_eq!(told(&app), "");
    assert_eq!(std::fs::read_to_string(&new).unwrap(), "words");
    play(&mut app, &ctx, &format!("save_as({});", name(&there)));
    assert!(told(&app).contains("there already"), "{}", told(&app));
    assert_eq!(std::fs::read_to_string(&there).unwrap(), "already here\n");
    app.dialog = Dialog::None;
    play(&mut app, &ctx, &format!("if !open_file({}) {{ message(\"no\"); }} set_line(1, \"edited\"); save();", name(&there)));
    assert_eq!(told(&app), "");
    assert_eq!(app.docs[app.active].name(), "there.txt");
    assert!(std::fs::read_to_string(&there).unwrap().starts_with("edited"));
    play(&mut app, &ctx, &format!("if open_file({}) {{ message(\"opened\"); }}", name(&dir.join("missing.txt"))));
    assert_eq!(told(&app), "");
    let (mut unnamed, unnamed_ctx) = app_with_text(&["x"]);
    play(&mut unnamed, &unnamed_ctx, "save();");
    assert!(told(&unnamed).contains("no name"));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_new_line_at_the_end_of_a_file_is_saved_on_its_own_line() {
    let (mut app, ctx) = app_with_text(&["a", "b"]);
    let dir = std::env::temp_dir().join(format!("limoniter-script-end-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("end.txt");
    let _ = std::fs::remove_file(&path);
    play(&mut app, &ctx, &format!("insert_line(line_count() + 1, \"c\"); insert_line(1, \"top\"); save_as({});", scripting::quoted(&path.display().to_string())));
    assert_eq!(told(&app), "");
    let saved = std::fs::read_to_string(&path).unwrap().replace("\r\n", "\n");
    assert_eq!(saved, "top\na\nb\nc");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn the_buttons_save_macros_rename_them_and_delete_them() {
    let (mut app, ctx) = app_with_text(&["alpha"]);
    let root = with_library(&mut app, "buttons");
    give(&mut app, &ctx, Command::RecordMacro);
    scripting::note_typed("x");
    give(&mut app, &ctx, Command::RecordMacro);
    assert!(matches!(&app.dialog, Dialog::Script(ScriptBox::SaveMacro { name, .. }) if name == "Macro 1"));
    app.script_box_ok_for_test(&ctx);
    assert!(matches!(app.dialog, Dialog::None));
    assert_eq!(app.library.macros[0].name, "Macro 1");
    let path = app.library.macros[0].path.clone();
    assert_eq!(std::fs::read_to_string(&path).unwrap(), "type_text(\"x\");\n");
    assert_eq!(app.current_macro.as_ref().unwrap().path.as_deref(), Some(path.as_path()));
    assert!(app.current_settings().contains("current_macro="));

    give(&mut app, &ctx, Command::PlayMacro);
    finish(&mut app, &ctx);
    assert_eq!(lines(&app), ["xalpha"]);

    // Changing it on disk changes what plays.
    std::fs::write(&path, "type_text(\"y\");").unwrap();
    give(&mut app, &ctx, Command::PlayMacro);
    finish(&mut app, &ctx);
    assert!(lines(&app)[0].contains('y'));

    panel_action(&mut app, &ctx, scripting::LibraryAction::Rename(path.clone()));
    if let Dialog::Script(ScriptBox::Rename { name, .. }) = &mut app.dialog {
        *name = "Renamed".into();
    }
    app.script_box_ok_for_test(&ctx);
    assert_eq!(app.current_macro.as_ref().unwrap().name, "Renamed");
    assert_eq!(app.library.macros[0].name, "Renamed");

    let renamed = app.library.macros[0].path.clone();
    panel_action(&mut app, &ctx, scripting::LibraryAction::Delete(renamed.clone()));
    app.script_box_ok_for_test(&ctx);
    assert!(app.library.macros.is_empty());
    assert!(!renamed.exists());
    assert!(app.current_macro.is_none());
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn a_new_script_opens_in_a_tab_and_cant_run_on_itself() {
    let (mut app, ctx) = app_with_text(&["alpha"]);
    let root = with_library(&mut app, "new");
    give(&mut app, &ctx, Command::NewScript);
    app.script_box_ok_for_test(&ctx);
    assert!(matches!(app.dialog, Dialog::None), "{}", told(&app));
    let path = app.docs[app.active].path.clone().unwrap();
    assert_eq!(path.extension().unwrap(), scripting::EXTENSION);
    let mut output = ctx.run_ui(sized(Vec::new()), |ui| app.play_path(&path, Repeat::Once, ui));
    output.textures_delta.clear();
    assert!(told(&app).contains("working on itself"));
    app.dialog = Dialog::None;
    // Played from another file, with unsaved edits to the script counting.
    let script_tab = app.active;
    app.docs[script_tab].text.set_content(vec!["type_text(\"from the tab\");".to_string()]);
    app.active = 0;
    let mut output = ctx.run_ui(sized(Vec::new()), |ui| app.play_path(&path, Repeat::Once, ui));
    output.textures_delta.clear();
    finish(&mut app, &ctx);
    assert_eq!(lines(&app), ["from the tabalpha"]);
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn a_key_plays_what_its_been_given() {
    let (mut app, ctx) = app_with_text(&["alpha"]);
    let root = with_library(&mut app, "keys");
    let path = app.library.save(scripting::Kind::Script, "Keyed", "type_text(\"k\");", false).unwrap();
    app.library.set_key(&path, Some(4));
    focus_text(&mut app, &ctx);
    frame(&mut app, &ctx, vec![pressed(Key::Num4, Modifiers::CTRL | Modifiers::ALT)]);
    finish(&mut app, &ctx);
    assert_eq!(lines(&app), ["kalpha"]);
    frame(&mut app, &ctx, vec![pressed(Key::Num5, Modifiers::CTRL | Modifiers::ALT)]);
    finish(&mut app, &ctx);
    assert_eq!(lines(&app), ["kalpha"], "nothing has that one");
    assert!(app.current_settings().contains("script_key=4\u{1}"));
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn every_part_of_it_draws_in_every_state() {
    let (mut app, ctx) = app_with_text(&["alpha"]);
    let root = with_library(&mut app, "draws");
    app.panels.macros = true;
    app.panels.scripts = true;
    app.shown_menu = Some((5, true));
    frame(&mut app, &ctx, Vec::new());
    app.library.save(scripting::Kind::Macro, "One", "1;", false).unwrap();
    app.library.refresh();
    frame(&mut app, &ctx, Vec::new());
    for step in ["macro-record", "macro-panels", "macro-save", "macro-times", "script-output", "script-running"] {
        app.script_screenshot_step(step);
        frame(&mut app, &ctx, Vec::new());
        frame(&mut app, &ctx, Vec::new());
        app.dialog = Dialog::None;
    }
    scripting::stop_recording();
    app.stop_script();
    app.dialog = Dialog::Script(ScriptBox::Confirm { title: "T".into(), question: "Q".into() });
    frame(&mut app, &ctx, Vec::new());
    app.dialog = Dialog::Script(ScriptBox::Delete { path: PathBuf::from("x"), name: "x".into() });
    frame(&mut app, &ctx, Vec::new());
    app.dialog = Dialog::None;
    app.close_tab(0);
    frame(&mut app, &ctx, Vec::new());
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn panels_and_keys_are_remembered_between_runs() {
    let (mut app, _ctx) = app_with_text(&["alpha"]);
    app.panels.macros = true;
    app.panels.scripts = true;
    app.library.keys = vec![(2, PathBuf::from(r"C:\somewhere\A = B.rhai"))];
    let settings = app.current_settings();
    let again = Limoniter::build(settings, None, Some(PathBuf::from("never-written.png")));
    assert!(again.panels.macros && again.panels.scripts);
    assert_eq!(again.library.keys, [(2, PathBuf::from(r"C:\somewhere\A = B.rhai"))]);
}

#[test]
fn a_macro_recorded_with_copy_and_paste_waits_a_frame_for_the_clipboard() {
    // The clipboard itself isn't touched here: what matters is that the copy lands before the next step is answered.
    let (mut app, ctx) = app_with_text(&["alpha"]);
    play_no_wait(&mut app, &ctx, r#"select_all(); command("copy"); type_text("after");"#);
    if app.script.is_some() {
        assert_eq!(lines(&app), ["alpha"], "it stops for the frame straight after the copy");
    }
    finish(&mut app, &ctx);
    assert_eq!(lines(&app), ["after"]);
}

#[test]
fn big_files_kept_on_disk_can_be_searched_and_replaced_by_a_script() {
    let dir = std::env::temp_dir().join(format!("limoniter-script-big-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("big.log");
    let line = "2026-11-04 12:00:00 INFO a fairly ordinary line of a log file\n".repeat(1_100_000);
    std::fs::write(&path, format!("needle at the start\n{line}needle at the end\n")).unwrap();
    let (mut app, ctx) = app_with_text(&[""]);
    assert!(app.open_path(&path));
    let began = Instant::now();
    while app.docs[app.active].is_busy() {
        frame(&mut app, &ctx, Vec::new());
        assert!(began.elapsed() < Duration::from_secs(120));
    }
    assert!(app.docs[app.active].text.is_disk_backed());
    // The very first thing it does is search, from the top of a file that's only just finished opening.
    play(&mut app, &ctx, r#"
        let n = 0;
        while find("needle") { n += 1; print("at " + current_line() + " " + current_column()); }
        print("found " + n);
        go_to(1, 1);
    "#);
    finish(&mut app, &ctx);
    assert_eq!(app.script_output, ["at 1 7", "at 1100002 7", "found 2"]);
    play(&mut app, &ctx, r#"
        let n = 0;
        print("from " + current_line() + " " + current_column());
        while find("needle") { n += 1; print("at " + current_line() + " " + current_column()); }
        print("found " + n);
        print(line_text(1));
        print("lines " + line_count());
        whole_text();
    "#);
    finish(&mut app, &ctx);
    assert_eq!(app.script_output[..4], ["from 1 1", "at 1 7", "at 1100002 7", "found 2"]);
    assert_eq!(app.script_output[4], "needle at the start");
    assert_eq!(app.script_output[5], format!("lines {}", 1_100_003));
    assert!(told(&app).contains("too large"), "{}", told(&app));
    app.dialog = Dialog::None;
    play(&mut app, &ctx, r#"let n = replace_all("needle", "pin"); print("replaced " + n); go_to(1, 1); type_text("edited ");"#);
    finish(&mut app, &ctx);
    assert_eq!(app.script_output[0], "replaced 2");
    assert_eq!(app.docs[app.active].text.line(0).as_ref(), "edited pin at the start");
    give(&mut app, &ctx, Command::Undo);
    assert_eq!(app.docs[app.active].text.line(0).as_ref(), "needle at the start");
    assert!(app.docs[app.active].text.is_disk_backed());
    app.close_tab(app.active);
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn a_script_over_many_lines_doesnt_take_forever() {
    let rows: Vec<String> = (0..50_000).map(|n| format!("line {n}")).collect();
    let refs: Vec<&str> = rows.iter().map(String::as_str).collect();
    let (mut app, ctx) = app_with_text(&refs);
    let began = Instant::now();
    play(&mut app, &ctx, r#"for n in 1..=line_count() { set_line(n, line_text(n) + "!"); }"#);
    assert_eq!(told(&app), "");
    assert_eq!(lines(&app)[49_999], "line 49999!");
    let took = began.elapsed();
    assert!(took < Duration::from_secs(120), "took {took:?}");
    give(&mut app, &ctx, Command::Undo);
    assert_eq!(lines(&app)[49_999], "line 49999");
}

/// Draws frames until queued add-on functions and any script they started are done.
fn settle_addons(app: &mut Limoniter, ctx: &Context) {
    let began = Instant::now();
    while (app.script.is_some() || !app.addon_queue.is_empty()) && matches!(app.dialog, Dialog::None) {
        frame(app, ctx, Vec::new());
        assert!(began.elapsed() < Duration::from_secs(60), "the add-on never finished");
    }
}

#[test]
fn an_add_on_adds_commands_runs_on_open_and_save_and_shows_panels() {
    let root = std::env::temp_dir().join(format!("limoniter-addon-host-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(root.join(crate::addons::FOLDER)).unwrap();
    let addon = root.join(crate::addons::FOLDER).join("Stamp.rhai");
    std::fs::write(
        &addon,
        r#"//! name: Stamp
//! about: Marks files as they're opened and saved.
//! command: Count lines = count
//! on_open: opened
//! on_save: saving
fn count() { show_panel("Lines", "There are " + line_count() + " lines."); add_to_panel("Lines", " Counted."); }
fn opened() { insert_line(1, "opened"); }
fn saving() { set_line(line_count(), "saved"); }
"#,
    )
    .unwrap();
    let file = root.join("notes.txt");
    std::fs::write(&file, "first\nlast").unwrap();
    let (mut app, ctx) = app_with_text(&[]);
    app.addons = crate::addons::AddOns::new(Some(root.clone()));
    app.addons.refresh();
    // Off until it's turned on: opening the file leaves it alone, and the command isn't offered.
    assert!(app.open_path(&file));
    settle_addons(&mut app, &ctx);
    assert_eq!(lines(&app), ["first", "last"]);
    assert!(app.addons.commands().is_empty());
    give(&mut app, &ctx, Command::CloseTab);
    app.addons.set_on(&addon, true);
    assert!(app.open_path(&file));
    settle_addons(&mut app, &ctx);
    assert_eq!(told(&app), "");
    assert_eq!(lines(&app), ["opened", "first", "last"]);
    // Its command, as the menu or command box gives it.
    give(&mut app, &ctx, Command::AddOn(0));
    settle_addons(&mut app, &ctx);
    assert_eq!(app.addon_panels.len(), 1);
    assert_eq!(app.addon_panels[0].text, "There are 3 lines. Counted.");
    frame(&mut app, &ctx, Vec::new());
    // Saving runs on_save first, and what it changed is what's written.
    give(&mut app, &ctx, Command::Save);
    settle_addons(&mut app, &ctx);
    assert_eq!(told(&app), "");
    assert_eq!(std::fs::read_to_string(&file).unwrap(), "opened\nfirst\nsaved");
    assert!(!app.docs[app.active].is_modified());
    // Changed on disk since it was turned on: it's off again, and saving just saves.
    std::fs::write(&addon, std::fs::read_to_string(&addon).unwrap().replace("\"saved\"", "\"other\"")).unwrap();
    app.addons.refresh();
    app.doc().text.set_line_keeping_cursor(0, vec!["edited".into()]);
    give(&mut app, &ctx, Command::Save);
    settle_addons(&mut app, &ctx);
    assert_eq!(std::fs::read_to_string(&file).unwrap(), "edited\nfirst\nsaved");
    std::fs::remove_dir_all(&root).unwrap();
}

#[test]
fn the_example_add_on_runs() {
    let root = std::env::temp_dir().join(format!("limoniter-addon-example-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let (mut app, ctx) = app_with_text(&["a TODO here", "b c d", "TODO again"]);
    app.addons = crate::addons::AddOns::new(Some(root.clone()));
    let path = app.addons.create("Example").unwrap();
    app.addons.set_on(&path, true);
    app.doc().text.select_all();
    give(&mut app, &ctx, Command::AddOn(0));
    settle_addons(&mut app, &ctx);
    assert_eq!(told(&app), "The selection has 8 words.");
    app.close_dialog();
    settle_addons(&mut app, &ctx);
    give(&mut app, &ctx, Command::AddOn(1));
    settle_addons(&mut app, &ctx);
    assert_eq!(told(&app), "");
    assert_eq!(app.addon_panels[0].text, "1: a TODO here\n3: TODO again\n");
    std::fs::remove_dir_all(&root).unwrap();
}
