//! Text objects, the edit history and narrowing, in the whole window.

use super::*;
use crate::history_file;
use crate::panels::HistoryAction;
use crate::text_objects::{Action as TextAction, Object};

fn app_with_text(lines: &[&str]) -> (Limoniter, Context) {
    let mut app = Limoniter::build(String::new(), None, Some(PathBuf::from("never-written.png")));
    app.new_document();
    app.doc().text.set_content(lines.iter().map(|line| line.to_string()).collect());
    (app, Context::default())
}

fn sized(events: Vec<egui::Event>) -> egui::RawInput {
    egui::RawInput { screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1600.0, 900.0))), events, ..Default::default() }
}

fn frame(app: &mut Limoniter, ctx: &Context, events: Vec<egui::Event>) -> egui::FullOutput {
    let mut output = ctx.run_ui(sized(events), |ui| app.draw_frame(ui));
    output.textures_delta.clear();
    output
}

fn give(app: &mut Limoniter, ctx: &Context, command: Command) {
    let mut output = ctx.run_ui(sized(Vec::new()), |ui| app.run(command, ui));
    output.textures_delta.clear();
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

fn pressed(key: Key, modifiers: Modifiers) -> egui::Event {
    egui::Event::Key { key, physical_key: None, pressed: true, repeat: false, modifiers: if modifiers.ctrl { modifiers | Modifiers::COMMAND } else { modifiers } }
}

fn selected(app: &Limoniter) -> String {
    app.docs[app.active].text.selection_text()
}

fn temp_folder(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("limoniter-tools-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

// ---------- Text objects ----------

#[test]
fn a_shortcut_selects_inside_then_around_then_further_out() {
    let (mut app, ctx) = app_with_text(&["call(one, (two), three)"]);
    frame(&mut app, &ctx, Vec::new());
    frame(&mut app, &ctx, Vec::new());
    app.doc().text.move_caret(0, 12, false);
    let shortcut = pressed(Key::B, Modifiers::CTRL | Modifiers::SHIFT);
    frame(&mut app, &ctx, vec![shortcut.clone()]);
    assert_eq!(selected(&app), "two");
    frame(&mut app, &ctx, vec![shortcut.clone()]);
    assert_eq!(selected(&app), "(two)");
    frame(&mut app, &ctx, vec![shortcut.clone()]);
    assert_eq!(selected(&app), "one, (two), three");
    frame(&mut app, &ctx, vec![shortcut.clone()]);
    assert_eq!(selected(&app), "(one, (two), three)");
    frame(&mut app, &ctx, vec![shortcut]);
    assert!(told(&app).contains("isn't inside"), "{}", told(&app));
    assert_eq!(lines(&app), ["call(one, (two), three)"]);
}

#[test]
fn typing_over_what_a_shortcut_selected_replaces_it() {
    let (mut app, ctx) = app_with_text(&["say \"old words\" now"]);
    frame(&mut app, &ctx, Vec::new());
    frame(&mut app, &ctx, Vec::new());
    app.doc().text.move_caret(0, 6, false);
    frame(&mut app, &ctx, vec![pressed(Key::Q, Modifiers::CTRL | Modifiers::SHIFT)]);
    frame(&mut app, &ctx, vec![egui::Event::Text("new".into())]);
    assert_eq!(lines(&app), ["say \"new\" now"]);
}

#[test]
fn ctrl_shift_w_selects_a_word_and_never_closes_the_tab() {
    let (mut app, ctx) = app_with_text(&["one two"]);
    frame(&mut app, &ctx, Vec::new());
    frame(&mut app, &ctx, Vec::new());
    frame(&mut app, &ctx, vec![pressed(Key::W, Modifiers::CTRL | Modifiers::SHIFT)]);
    assert_eq!(selected(&app), "one");
    assert!(!app.no_file());
    // With a box open it can't act, but it still mustn't reach Ctrl+W.
    app.message("Test", "open".into());
    frame(&mut app, &ctx, vec![pressed(Key::W, Modifiers::CTRL | Modifiers::SHIFT)]);
    assert!(!app.no_file());
}

#[test]
fn replacing_and_deleting_are_one_undo_each() {
    let (mut app, ctx) = app_with_text(&["keep (inner text) keep", "second"]);
    app.doc().text.move_caret(0, 8, false);
    give(&mut app, &ctx, Command::TextObject(TextAction::DeleteInside, Object::Brackets));
    assert_eq!(lines(&app)[0], "keep () keep");
    give(&mut app, &ctx, Command::Undo);
    assert_eq!(lines(&app)[0], "keep (inner text) keep");
    app.doc().text.move_caret(0, 8, false);
    give(&mut app, &ctx, Command::TextObject(TextAction::DeleteAround, Object::Brackets));
    assert_eq!(lines(&app)[0], "keep  keep");
    give(&mut app, &ctx, Command::Undo);
    app.doc().text.move_caret(0, 8, false);
    give(&mut app, &ctx, Command::TextObject(TextAction::ReplaceInside, Object::Brackets));
    app.doc().text.type_text("typed");
    assert_eq!(lines(&app)[0], "keep (typed) keep", "the cursor was left ready");
    give(&mut app, &ctx, Command::TextObject(TextAction::DeleteAround, Object::Line));
    assert_eq!(lines(&app), ["second"]);
}

#[test]
fn text_objects_respect_locks_and_modes_and_say_so() {
    let (mut app, ctx) = app_with_text(&["(a)"]);
    app.doc().set_read_only(true);
    app.doc().text.move_caret(0, 1, false);
    give(&mut app, &ctx, Command::TextObject(TextAction::SelectInside, Object::Brackets));
    assert_eq!(selected(&app), "a", "selecting is fine on a locked file");
    give(&mut app, &ctx, Command::TextObject(TextAction::DeleteInside, Object::Brackets));
    assert!(!told(&app).is_empty());
    assert_eq!(lines(&app), ["(a)"]);
    app.dialog = Dialog::None;
    app.doc().set_read_only(false);
    give(&mut app, &ctx, Command::ToggleColumn);
    give(&mut app, &ctx, Command::TextObject(TextAction::DeleteInside, Object::Brackets));
    assert!(told(&app).contains("column mode"));
    assert_eq!(lines(&app), ["(a)"]);
    app.dialog = Dialog::None;
    give(&mut app, &ctx, Command::ToggleColumn);
    give(&mut app, &ctx, Command::ToggleHex);
    give(&mut app, &ctx, Command::TextObject(TextAction::DeleteInside, Object::Brackets));
    assert!(app.docs[app.active].hex_mode);
    assert_eq!(app.docs[app.active].hex.len(), 3, "in hex mode it does nothing");
}

#[test]
fn brackets_are_seen_in_an_old_encoding() {
    let (mut app, ctx) = app_with_text(&[""]);
    let set = charset::all().iter().copied().find(|set| set.keep_bytes && set.table(0).is_some_and(|table| table.byte_of('(').is_some() && table.byte_of(')').is_some())).expect("a set that keeps its bytes");
    app.doc().text.format = TextFormat { charset: set, ..TextFormat::default() };
    app.doc().text.type_text("(AB)");
    app.doc().text.move_caret(0, 2, false);
    give(&mut app, &ctx, Command::TextObject(TextAction::DeleteInside, Object::Brackets));
    assert_eq!(app.docs[0].text.format.readable_text(&lines(&app)[0]), "()");
}

// ---------- The history ----------

#[test]
fn what_was_undone_can_be_got_back_after_typing_something_else() {
    let (mut app, ctx) = app_with_text(&["base"]);
    app.doc().text.type_text("first ");
    app.doc().text.type_text("abandoned ");
    give(&mut app, &ctx, Command::Undo);
    assert_eq!(lines(&app), ["first base"]);
    app.doc().text.type_text("chosen ");
    assert!(!app.docs[0].text.can_redo());
    let rows = app.docs[0].text.history_rows();
    assert_eq!(rows[0].id, None, "the text now is on top");
    let abandoned = rows.iter().filter(|row| !row.on_path).find_map(|row| row.id).expect("the branch left behind");
    assert_ne!(rows.iter().find(|row| row.id == Some(abandoned)).unwrap().lane, rows[0].lane);

    let mut output = ctx.run_ui(sized(Vec::new()), |ui| app.history_action(HistoryAction::Jump(abandoned), ui));
    output.textures_delta.clear();
    assert_eq!(lines(&app), ["first abandoned base"]);
    // Where it had got to wasn't lost by going there.
    let rows = app.docs[0].text.history_rows();
    let chosen = rows.iter().filter(|row| !row.on_path).find_map(|row| row.id).unwrap();
    let mut output = ctx.run_ui(sized(Vec::new()), |ui| app.history_action(HistoryAction::Jump(chosen), ui));
    output.textures_delta.clear();
    assert_eq!(lines(&app), ["first chosen base"]);
    // And plain undo goes on working down whichever line it's on.
    give(&mut app, &ctx, Command::Undo);
    assert_eq!(lines(&app), ["first base"]);
    give(&mut app, &ctx, Command::Undo);
    assert_eq!(lines(&app), ["base"]);
    assert!(!app.docs[0].text.can_undo());
}

#[test]
fn going_back_to_the_saved_state_counts_as_not_modified() {
    let (mut app, ctx) = app_with_text(&["base"]);
    app.doc().text.type_text("x");
    app.doc().text.mark_saved();
    assert!(!app.docs[0].is_modified());
    app.doc().text.move_caret(0, 0, false);
    app.doc().text.type_text("y");
    assert!(app.docs[0].is_modified());
    give(&mut app, &ctx, Command::Undo);
    assert!(!app.docs[0].is_modified(), "it's the saved text again");
    assert_eq!(lines(&app), ["xbase"]);
    give(&mut app, &ctx, Command::Redo);
    assert!(app.docs[0].is_modified());
}

#[test]
fn the_panel_draws_a_real_tree_and_comparing_opens_compare() {
    let (mut app, ctx) = app_with_text(&["base"]);
    app.panels.history = true;
    frame(&mut app, &ctx, Vec::new());
    for words in ["one ", "two ", "three "] {
        app.doc().text.type_text(words);
    }
    give(&mut app, &ctx, Command::Undo);
    app.doc().text.type_text("branch ");
    frame(&mut app, &ctx, Vec::new());
    let id = app.docs[0].text.history_rows().iter().find_map(|row| row.id).unwrap();
    let mut output = ctx.run_ui(sized(Vec::new()), |ui| app.history_action(HistoryAction::Compare(id), ui));
    output.textures_delta.clear();
    assert!(matches!(app.dialog, Dialog::Compare(_)));
    app.dialog = Dialog::None;
    // Saved, so that closing it has nothing to ask about: a test can't have a box waiting for an answer.
    app.doc().text.mark_saved();
    app.close_tab(0);
    frame(&mut app, &ctx, Vec::new());
}

#[test]
fn a_locked_file_cant_be_moved_through_its_history() {
    let (mut app, ctx) = app_with_text(&["base"]);
    app.doc().text.type_text("x ");
    app.doc().text.type_text("y ");
    let id = app.docs[0].text.history_rows().iter().find_map(|row| row.id).unwrap();
    app.doc().set_read_only(true);
    let before = lines(&app);
    let mut output = ctx.run_ui(sized(Vec::new()), |ui| app.history_action(HistoryAction::Jump(id), ui));
    output.textures_delta.clear();
    assert_eq!(lines(&app), before);
    assert!(!told(&app).is_empty());
}

/// A saved file with some history, in folders of its own.
fn saved_file(tag: &str) -> (Limoniter, Context, PathBuf, PathBuf) {
    let dir = temp_folder(tag);
    let path = dir.join("file.txt");
    std::fs::write(&path, "line one\r\nline two\r\n").unwrap();
    let (mut app, ctx) = app_with_text(&[""]);
    app.history_root = Some(dir.join("history"));
    assert!(app.open_path(&path));
    app.doc().text.type_text("first ");
    app.doc().text.type_text("abandoned ");
    give(&mut app, &ctx, Command::Undo);
    app.doc().text.type_text("second ");
    let active = app.active;
    assert!(app.save(active));
    app.finish_history_job();
    (app, ctx, path, dir)
}

fn reopened(path: &Path, dir: &Path) -> (Limoniter, Context) {
    let (mut app, ctx) = app_with_text(&[""]);
    app.history_root = Some(dir.join("history"));
    assert!(app.open_path(path));
    (app, ctx)
}

#[test]
fn the_history_comes_back_when_the_file_is_opened_again() {
    let (_, _, path, dir) = saved_file("persist");
    let (mut app, ctx) = reopened(&path, &dir);
    assert_eq!(lines(&app)[0], "first second line one");
    assert!(!app.docs[app.active].is_modified(), "opening it isn't a change");
    assert!(app.docs[app.active].text.can_undo());
    assert!(!app.docs[app.active].text.can_redo());
    let rows = app.docs[app.active].text.history_rows();
    assert!(rows.iter().any(|row| row.here));
    let abandoned = rows.iter().filter(|row| !row.on_path).find_map(|row| row.id).expect("the abandoned branch came back too");
    give(&mut app, &ctx, Command::Undo);
    assert_eq!(lines(&app)[0], "first line one");
    assert!(app.docs[app.active].is_modified(), "that's different from the file");
    give(&mut app, &ctx, Command::Undo);
    assert_eq!(lines(&app)[0], "line one");
    let mut output = ctx.run_ui(sized(Vec::new()), |ui| app.history_action(HistoryAction::Jump(abandoned), ui));
    output.textures_delta.clear();
    assert_eq!(lines(&app)[0], "first abandoned line one");
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn line_endings_come_back_with_the_history() {
    let dir = temp_folder("eols");
    let path = dir.join("mixed.txt");
    std::fs::write(&path, b"a\r\nb\nc\rd\r\n").unwrap();
    let (mut app, _) = app_with_text(&[""]);
    app.history_root = Some(dir.join("history"));
    assert!(app.open_path(&path));
    app.doc().text.move_caret(0, 0, false);
    app.doc().text.type_text("x");
    let active = app.active;
    assert!(app.save(active));
    app.finish_history_job();
    let (mut again, again_ctx) = reopened(&path, &dir);
    give(&mut again, &again_ctx, Command::Undo);
    let active = again.active;
    assert!(again.save(active));
    again.finish_history_job();
    assert_eq!(std::fs::read(&path).unwrap(), b"a\r\nb\nc\rd\r\n", "saving the earlier state gives back the exact bytes");
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn a_file_changed_by_another_program_has_moved_on_from_its_history() {
    let (_, _, path, dir) = saved_file("moved-on");
    std::fs::write(&path, "changed elsewhere\r\n").unwrap();
    let (app, _) = reopened(&path, &dir);
    assert_eq!(lines(&app)[0], "changed elsewhere");
    assert!(!app.docs[app.active].text.can_undo(), "no history that doesn't lead here");
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn a_damaged_history_file_is_ignored() {
    let (_, _, path, dir) = saved_file("damaged");
    let history = history_file::file_for(&dir.join("history"), &path);
    let mut bytes = std::fs::read(&history).unwrap();
    let middle = bytes.len() / 2;
    bytes[middle] ^= 0xFF;
    std::fs::write(&history, bytes).unwrap();
    let (app, _) = reopened(&path, &dir);
    assert_eq!(lines(&app)[0], "first second line one");
    assert!(!app.docs[app.active].text.can_undo());
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn turning_it_off_keeps_nothing_and_finds_nothing() {
    let dir = temp_folder("off");
    let path = dir.join("file.txt");
    std::fs::write(&path, "text\r\n").unwrap();
    let (mut app, ctx) = app_with_text(&[""]);
    app.history_root = Some(dir.join("history"));
    app.prefs.keep_undo_history = false;
    assert!(app.open_path(&path));
    app.doc().text.type_text("edit ");
    let active = app.active;
    assert!(app.save(active));
    app.finish_history_job();
    assert!(!history_file::exists(&dir.join("history"), &path));
    give(&mut app, &ctx, Command::Undo);
    assert_eq!(lines(&app)[0], "text", "undo itself carries on");
    let (_, _, kept, kept_dir) = saved_file("off-kept");
    let (mut off, _) = app_with_text(&[""]);
    off.history_root = Some(kept_dir.join("history"));
    off.prefs.keep_undo_history = false;
    assert!(off.open_path(&kept));
    assert!(!off.docs[off.active].text.can_undo());
    let _ = std::fs::remove_dir_all(dir);
    let _ = std::fs::remove_dir_all(kept_dir);
}

#[test]
fn forgetting_a_history_empties_it_in_memory() {
    let (mut app, ctx) = app_with_text(&["text"]);
    app.doc().text.type_text("edit ");
    app.doc().text.forget_history();
    assert!(!app.docs[0].text.can_undo());
    assert_eq!(lines(&app), ["edit text"], "the text stays");
    assert!(app.docs[0].text.history_rows().is_empty());
    app.doc().text.type_text("more ");
    give(&mut app, &ctx, Command::Undo);
    assert_eq!(lines(&app), ["edit text"], "and undo starts again from here");
}

#[test]
fn undo_steps_stay_within_their_limit_with_branches_counted() {
    let (mut app, ctx) = app_with_text(&["base"]);
    for n in 0..1000 {
        app.doc().text.type_text(&format!("{n} "));
        if n % 3 == 0 {
            give(&mut app, &ctx, Command::Undo);
        }
    }
    assert!(app.docs[0].text.history_rows().len() <= 202);
    assert!(app.docs[0].text.can_undo());
}

// ---------- Narrowing ----------

fn narrowed_app() -> (Limoniter, Context) {
    let (mut app, ctx) = app_with_text(&["one", "cat two", "cat three", "cat four", "five cat"]);
    app.doc().text.set_selection((1, 0), (3, 2), false);
    give(&mut app, &ctx, Command::Narrow);
    assert_eq!(told(&app), "");
    (app, ctx)
}

#[test]
fn narrowing_limits_every_command_to_those_lines() {
    let (mut app, ctx) = narrowed_app();
    assert_eq!(lines(&app), ["cat two", "cat three", "cat four"]);
    app.find.find = "cat".into();
    app.find.replace = "dog".into();
    app.run_replace_all();
    assert_eq!(lines(&app), ["dog two", "dog three", "dog four"]);
    give(&mut app, &ctx, Command::SelectAll);
    assert_eq!(selected(&app).lines().count(), 3);
    give(&mut app, &ctx, Command::Upper);
    give(&mut app, &ctx, Command::SortDescending);
    frame(&mut app, &ctx, Vec::new());
    give(&mut app, &ctx, Command::Widen);
    assert_eq!(lines(&app), ["one", "DOG TWO", "DOG THREE", "DOG FOUR", "five cat"], "what's outside wasn't touched");
}

#[test]
fn widening_with_nothing_changed_is_no_change_at_all() {
    let (mut app, ctx) = narrowed_app();
    assert!(!app.docs[0].is_modified());
    give(&mut app, &ctx, Command::Widen);
    assert!(!app.docs[0].is_modified());
    assert!(!app.docs[0].text.can_undo());
    assert_eq!(app.docs[0].text.caret_line, 3, "the cursor is on the same text");
}

#[test]
fn everything_done_while_narrowed_is_one_undo_afterwards() {
    let (mut app, ctx) = narrowed_app();
    app.doc().text.type_text("x");
    app.doc().text.move_caret(0, 0, false);
    app.doc().text.type_text("y");
    give(&mut app, &ctx, Command::Undo);
    assert!(lines(&app)[0].starts_with("cat"), "undo works on its own within the part");
    app.doc().text.type_text("z");
    give(&mut app, &ctx, Command::Widen);
    assert!(lines(&app)[1].starts_with('z'));
    give(&mut app, &ctx, Command::Undo);
    assert_eq!(lines(&app), ["one", "cat two", "cat three", "cat four", "five cat"]);
}

#[test]
fn lines_can_be_added_and_removed_inside() {
    let (mut app, ctx) = narrowed_app();
    app.doc().text.set_content(vec!["just".to_string(), "these".to_string(), "now".to_string(), "and".to_string(), "more".to_string()]);
    frame(&mut app, &ctx, Vec::new());
    give(&mut app, &ctx, Command::Widen);
    assert_eq!(lines(&app), ["one", "just", "these", "now", "and", "more", "five cat"]);
}

#[test]
fn line_numbers_are_the_ones_in_the_whole_file() {
    let (mut app, ctx) = narrowed_app();
    let text = &app.docs[0].text;
    assert!(text.position_text().starts_with("Line 4"), "{}", text.position_text());
    app.dialog = Dialog::GoTo { value: "3".into(), error: String::new() };
    frame(&mut app, &ctx, vec![pressed(Key::Enter, Modifiers::NONE)]);
    assert_eq!(app.docs[0].text.caret_line, 1, "line 3 of the file is the second one here");
    assert!(app.docs[0].text.position_text().starts_with("Line 3"));
}

#[test]
fn with_nothing_selected_it_narrows_to_the_paragraph() {
    let (mut app, ctx) = app_with_text(&["first", "para", "", "second", "para", "", "third"]);
    app.doc().text.move_caret(4, 1, false);
    give(&mut app, &ctx, Command::Narrow);
    assert_eq!(lines(&app), ["second", "para"]);
    assert_eq!(app.docs[0].text.caret_line, 1);
}

#[test]
fn a_selection_ending_at_a_line_start_leaves_that_line_out() {
    let (mut app, ctx) = app_with_text(&["a", "b", "c", "d"]);
    app.doc().text.set_selection((1, 0), (3, 0), false);
    give(&mut app, &ctx, Command::Narrow);
    assert_eq!(lines(&app), ["b", "c"]);
}

#[test]
fn the_whole_file_isnt_narrowed_to_and_nor_is_a_narrowed_part_again() {
    let (mut app, ctx) = app_with_text(&["a", "b"]);
    give(&mut app, &ctx, Command::SelectAll);
    give(&mut app, &ctx, Command::Narrow);
    assert!(told(&app).contains("whole file"));
    assert!(app.docs[0].narrow.is_none());
    let (mut narrowed, narrowed_ctx) = narrowed_app();
    give(&mut narrowed, &narrowed_ctx, Command::Narrow);
    assert!(told(&narrowed).contains("already"));
}

#[test]
fn commands_needing_the_whole_file_wait_and_say_so() {
    for command in [Command::ToggleHex, Command::ViewHex, Command::ViewEbcdic, Command::ReopenAs, Command::ConvertTo, Command::SaveCopyAs, Command::SplitHorizontal, Command::DuplicateWindow, Command::FilterSelection] {
        let (mut app, ctx) = narrowed_app();
        give(&mut app, &ctx, command);
        assert!(told(&app).contains("whole file"), "{}", told(&app));
        assert!(!app.docs[0].hex_mode && app.docs[0].split.is_none() && app.docs[0].filter.is_none() && app.docs[0].windows.is_empty());
    }
    let (mut app, _) = narrowed_app();
    app.run_menu_action(MenuAction::SetBom(true));
    assert!(told(&app).contains("whole file"));
    assert!(!app.docs[0].text.format.has_bom);
}

#[test]
fn saving_while_narrowed_saves_all_of_it_and_stays_narrowed() {
    let dir = temp_folder("narrow-save");
    let path = dir.join("file.txt");
    std::fs::write(&path, b"one\r\ntwo\nthree\r\nfour\nfive").unwrap();
    let (mut app, ctx) = app_with_text(&[""]);
    assert!(app.open_path(&path));
    app.doc().text.set_selection((1, 0), (2, 1), false);
    give(&mut app, &ctx, Command::Narrow);
    app.doc().text.move_caret(0, 0, false);
    app.doc().text.type_text("TWO ");
    assert!(app.docs[app.active].is_modified());
    let active = app.active;
    assert!(app.save(active));
    assert_eq!(std::fs::read(&path).unwrap(), b"one\r\nTWO two\nthree\r\nfour\nfive", "all of it, with its own line endings");
    assert!(app.docs[active].narrow.is_some(), "still narrowed");
    assert_eq!(lines(&app), ["TWO two", "three"]);
    assert!(!app.docs[active].is_modified());
    app.doc().text.move_caret(0, 0, false);
    app.doc().text.type_text("more ");
    assert!(app.docs[active].is_modified());
    assert!(app.save(active));
    assert_eq!(std::fs::read(&path).unwrap(), b"one\r\nmore TWO two\nthree\r\nfour\nfive");
    give(&mut app, &ctx, Command::Widen);
    assert!(!app.docs[active].is_modified());
    assert_eq!(std::fs::read(&path).unwrap(), b"one\r\nmore TWO two\nthree\r\nfour\nfive");
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn a_recovery_copy_while_narrowed_holds_the_whole_file_with_its_changes() {
    let (mut app, _) = narrowed_app();
    app.doc().text.move_caret(0, 0, false);
    app.doc().text.type_text("changed ");
    let snapshot = app.docs[0].snapshot();
    let recovery::Body::Text(image) = snapshot.body else {
        panic!("it's text");
    };
    let store = crate::line_store::LineStore::from_image(image, None).unwrap();
    let rows: Vec<String> = store.range_vec(0, store.len()).iter().map(|line| line.to_string()).collect();
    assert_eq!(rows, ["one", "changed cat two", "cat three", "cat four", "five cat"]);
    assert_eq!(snapshot.meta.caret.0, 1, "and the cursor is where it is in the whole file");
}

#[test]
fn bookmarks_stay_with_their_lines_through_narrowing() {
    let (mut app, ctx) = app_with_text(&["a", "b", "c", "d", "e"]);
    app.doc().text.move_caret(0, 0, false);
    give(&mut app, &ctx, Command::ToggleBookmark);
    app.doc().text.move_caret(2, 0, false);
    give(&mut app, &ctx, Command::ToggleBookmark);
    app.doc().text.set_selection((1, 0), (3, 1), false);
    give(&mut app, &ctx, Command::Narrow);
    assert!(app.docs[0].text.is_bookmarked(1), "the one on c, which is second here");
    app.doc().text.move_caret(2, 0, false);
    give(&mut app, &ctx, Command::ToggleBookmark);
    give(&mut app, &ctx, Command::Widen);
    let text = &app.docs[0].text;
    assert!(text.is_bookmarked(0) && text.is_bookmarked(2) && text.is_bookmarked(3));
    assert!(!text.is_bookmarked(1) && !text.is_bookmarked(4));
}

#[test]
fn a_big_file_kept_on_disk_can_be_narrowed_and_stays_on_disk() {
    let dir = temp_folder("narrow-big");
    let path = dir.join("big.log");
    let line = "2026-11-04 12:00:00 INFO a fairly ordinary line of a log file\n".repeat(1_100_000);
    std::fs::write(&path, &line).unwrap();
    let (mut app, ctx) = app_with_text(&[""]);
    assert!(app.open_path(&path));
    let began = std::time::Instant::now();
    while app.docs[app.active].is_busy() {
        frame(&mut app, &ctx, Vec::new());
        assert!(began.elapsed() < Duration::from_secs(120));
    }
    assert!(app.docs[app.active].text.is_disk_backed());
    app.doc().text.set_selection((500_000, 0), (500_003, 1), false);
    give(&mut app, &ctx, Command::Narrow);
    assert_eq!(app.docs[app.active].text.line_count(), 4);
    assert!(!app.docs[app.active].text.is_disk_backed());
    app.doc().text.move_caret(0, 0, false);
    app.doc().text.type_text("edited ");
    assert!(app.docs[app.active].text.position_text().starts_with("Line 500001"));
    give(&mut app, &ctx, Command::Widen);
    let text = &app.docs[app.active].text;
    assert!(text.is_disk_backed(), "the rest of it hasn't left the disk");
    assert!(text.line(500_000).starts_with("edited 2026"));
    assert!(text.line(500_001).starts_with("2026"));
    // Let go of the file so that it can be deleted, without closing the tab, which would ask about saving.
    drop(app);
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn a_macro_runs_on_the_narrowed_part_and_comes_off_in_one() {
    let (mut app, ctx) = narrowed_app();
    let mut output = ctx.run_ui(sized(Vec::new()), |ui| app.play_source("Test", "for n in 1..=line_count() { set_line(n, \"> \" + line_text(n)); }", scripting::Repeat::Once, ui));
    output.textures_delta.clear();
    while app.script.is_some() {
        frame(&mut app, &ctx, Vec::new());
    }
    assert_eq!(told(&app), "");
    assert_eq!(lines(&app), ["> cat two", "> cat three", "> cat four"]);
    give(&mut app, &ctx, Command::Widen);
    assert_eq!(lines(&app)[0], "one");
    assert_eq!(lines(&app)[1], "> cat two");
    assert_eq!(lines(&app)[4], "five cat");
}

#[test]
fn the_bar_draws_and_its_way_back_works_from_every_tab() {
    let (mut app, ctx) = narrowed_app();
    app.panels.history = true;
    for menu in 0..super::menus::MENUS.len() {
        app.shown_menu = Some((menu, true));
        frame(&mut app, &ctx, Vec::new());
    }
    app.close_tab(0);
    frame(&mut app, &ctx, Vec::new());
    assert!(app.no_file());
}

#[test]
fn every_step_of_a_random_session_leaves_the_whole_file_consistent() {
    // The same few edits, narrowed and not, have to come out the same.
    let (mut plain, plain_ctx) = app_with_text(&["a", "b", "c", "d", "e", "f"]);
    plain.doc().text.move_caret(2, 0, false);
    plain.doc().text.type_text("x\ny");
    plain.doc().text.move_caret(3, 0, false);
    give(&mut plain, &plain_ctx, Command::Duplicate);
    let (mut app, ctx) = app_with_text(&["a", "b", "c", "d", "e", "f"]);
    app.doc().text.set_selection((1, 0), (4, 1), false);
    give(&mut app, &ctx, Command::Narrow);
    app.doc().text.move_caret(1, 0, false);
    app.doc().text.type_text("x\ny");
    app.doc().text.move_caret(2, 0, false);
    give(&mut app, &ctx, Command::Duplicate);
    give(&mut app, &ctx, Command::Widen);
    assert_eq!(lines(&app), lines(&plain));
}
