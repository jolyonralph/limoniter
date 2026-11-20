//! Files from Explorer, switching by use, swapping with the clipboard, saving and splitting, renaming, snippets,
//! running commands and spelling, in the whole window.

use super::*;
use super::file_tools::{Fill, Snippet, SplitBy, expand_run_line, expand_snippet, part_names, part_starts, snippets_from_settings, snippets_to_settings};
use crate::spelling;

fn app_with_text(lines: &[&str]) -> (Limoniter, Context) {
    let mut app = Limoniter::build(String::new(), None, Some(PathBuf::from("never-written.png")));
    app.new_document();
    app.doc().text.set_content(lines.iter().map(|line| line.to_string()).collect());
    (app, Context::default())
}

fn sized(events: Vec<egui::Event>, ctrl: bool) -> egui::RawInput {
    // Which keys are held carries on from one frame to the next, as it does in the window.
    let held = egui::Event::ModifiersChanged(if ctrl { Modifiers::CTRL } else { Modifiers::NONE });
    let events = std::iter::once(held).chain(events).collect();
    egui::RawInput { screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1600.0, 900.0))), events, ..Default::default() }
}

fn frame(app: &mut Limoniter, ctx: &Context, events: Vec<egui::Event>) -> egui::FullOutput {
    held_frame(app, ctx, events, false)
}

fn held_frame(app: &mut Limoniter, ctx: &Context, events: Vec<egui::Event>, ctrl: bool) -> egui::FullOutput {
    let mut output = ctx.run_ui(sized(events, ctrl), |ui| app.draw_frame(ui));
    output.textures_delta.clear();
    output
}

fn give(app: &mut Limoniter, ctx: &Context, command: Command) {
    let mut output = ctx.run_ui(sized(Vec::new(), false), |ui| app.run(command, ui));
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

fn copied(output: &egui::FullOutput) -> Option<String> {
    output.platform_output.commands.iter().find_map(|command| match command {
        egui::OutputCommand::CopyText(text) => Some(text.clone()),
        _ => None,
    })
}

fn key(key: Key, modifiers: Modifiers) -> egui::Event {
    egui::Event::Key { key, physical_key: None, pressed: true, repeat: false, modifiers: if modifiers.ctrl { modifiers | Modifiers::COMMAND } else { modifiers } }
}

fn folder(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("limoniter-files-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn wait_until(app: &mut Limoniter, ctx: &Context, done: impl Fn(&Limoniter) -> bool) {
    for _ in 0..600 {
        frame(app, ctx, Vec::new());
        if done(app) {
            return;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    panic!("it didn't finish in time");
}

// ---------- One window ----------

#[test]
fn opening_files_in_a_new_window_each_time_is_a_setting_that_starts_off() {
    let prefs = Prefs::default();
    assert!(!prefs.new_window_per_file, "files go to the window already open");
    let changed = Prefs { new_window_per_file: true, spelling: true, ..prefs };
    assert_eq!(Prefs::from_settings(&changed.to_settings()), changed);
}

#[test]
fn files_handed_over_open_as_tabs_in_this_window() {
    let dir = folder("handed");
    let file = dir.join("from explorer.txt");
    std::fs::write(&file, "opened from Explorer\r\n").unwrap();
    let (mut app, ctx) = app_with_text(&["already here"]);
    app.listening = crate::instance::listen(&dir, || {});
    assert!(crate::instance::hand_off(&dir, std::slice::from_ref(&file)));
    wait_until(&mut app, &ctx, |app| app.docs.len() == 2);
    assert_eq!(app.docs[app.active].path.as_deref(), Some(file.as_path()));
    assert_eq!(lines(&app)[0], "opened from Explorer");
    drop(app);
    let _ = std::fs::remove_dir_all(dir);
}

// ---------- Switching in the order files were used ----------

fn three_files() -> (Limoniter, Context) {
    let (mut app, ctx) = app_with_text(&["A"]);
    app.new_document();
    app.doc().text.set_content(vec!["B".into()]);
    app.new_document();
    app.doc().text.set_content(vec!["C".into()]);
    // Used in the order A, C, B, so B is the latest.
    for index in [0, 2, 1] {
        app.active = index;
        frame(&mut app, &ctx, Vec::new());
    }
    (app, ctx)
}

#[test]
fn ctrl_tab_goes_to_the_file_used_last_and_back_again() {
    let (mut app, ctx) = three_files();
    assert_eq!(lines(&app), ["B"]);
    frame(&mut app, &ctx, vec![key(Key::Tab, Modifiers::CTRL)]);
    assert_eq!(lines(&app), ["C"], "the one used before, not the next tab along");
    frame(&mut app, &ctx, Vec::new());
    frame(&mut app, &ctx, vec![key(Key::Tab, Modifiers::CTRL)]);
    assert_eq!(lines(&app), ["B"], "and back again");
}

#[test]
fn holding_ctrl_goes_further_back_and_letting_go_settles_there() {
    let (mut app, ctx) = three_files();
    held_frame(&mut app, &ctx, vec![key(Key::Tab, Modifiers::CTRL)], true);
    held_frame(&mut app, &ctx, Vec::new(), true);
    held_frame(&mut app, &ctx, vec![key(Key::Tab, Modifiers::CTRL)], true);
    assert_eq!(lines(&app), ["A"], "two back, while Ctrl is held");
    assert!(app.cycle.is_some());
    frame(&mut app, &ctx, Vec::new());
    assert!(app.cycle.is_none(), "letting go of Ctrl settles on it");
    assert_eq!(app.used_order[0], app.docs[0].id);
    frame(&mut app, &ctx, vec![key(Key::Tab, Modifiers::CTRL)]);
    assert_eq!(lines(&app), ["B"]);
}

#[test]
fn ctrl_shift_tab_goes_the_other_way_and_one_file_goes_nowhere() {
    let (mut app, ctx) = three_files();
    frame(&mut app, &ctx, vec![key(Key::Tab, Modifiers::CTRL | Modifiers::SHIFT)]);
    assert_eq!(lines(&app), ["A"], "the file used least recently");
    let (mut alone, ctx) = app_with_text(&["only"]);
    frame(&mut alone, &ctx, Vec::new());
    assert!(!alone.can(Command::UsedFileBack));
    give(&mut alone, &ctx, Command::UsedFileBack);
    assert_eq!(lines(&alone), ["only"]);
}

#[test]
fn the_list_of_files_shows_while_ctrl_is_held() {
    let (mut app, ctx) = three_files();
    held_frame(&mut app, &ctx, vec![key(Key::Tab, Modifiers::CTRL)], true);
    let output = held_frame(&mut app, &ctx, Vec::new(), true);
    assert!(!output.shapes.is_empty());
    assert!(app.cycle.is_some());
}

// ---------- Swap with clipboard ----------

#[test]
fn swap_with_clipboard_trades_the_selection_for_what_was_copied() {
    let (mut app, ctx) = app_with_text(&["old text here"]);
    frame(&mut app, &ctx, Vec::new());
    app.doc().text.set_selection((0, 0), (0, 3), false);
    let mut output = ctx.run_ui(sized(Vec::new(), false), |ui| app.swap_in("new", ui));
    output.textures_delta.clear();
    assert_eq!(lines(&app), ["new text here"]);
    assert_eq!(copied(&output).as_deref(), Some("old"));
    give(&mut app, &ctx, Command::Undo);
    assert_eq!(lines(&app), ["old text here"], "one step to undo");
    // Even with Copy before paste off, which is how it starts.
    assert!(!app.prefs.edit.copy_before_paste);
}

#[test]
fn swap_needs_a_selection_and_something_on_the_clipboard() {
    let (mut app, ctx) = app_with_text(&["old text here"]);
    frame(&mut app, &ctx, Vec::new());
    assert!(!app.can(Command::SwapClipboard), "greyed with nothing selected");
    let mut output = ctx.run_ui(sized(Vec::new(), false), |ui| app.swap_in("new", ui));
    output.textures_delta.clear();
    assert_eq!(lines(&app), ["old text here"]);
    assert_eq!(copied(&output), None);
    app.doc().text.set_selection((0, 0), (0, 3), false);
    assert!(app.can(Command::SwapClipboard));
    ctx.run_ui(sized(Vec::new(), false), |ui| app.swap_in("", ui)).textures_delta.clear();
    assert_eq!(lines(&app), ["old text here"]);
    assert!(told(&app).contains("no text on the clipboard"));
    app.doc().set_read_only(true);
    give(&mut app, &ctx, Command::SwapClipboard);
    assert!(told(&app).contains("read-only"));
}

#[test]
fn swap_is_under_paste_in_the_edit_menu() {
    let (_, edit) = menus::MENUS[1];
    let at = |wanted: Command| edit.iter().position(|entry| matches!(entry, menus::Entry::Item(_, command) | menus::Entry::Keyed(_, command, _) if *command == wanted)).unwrap();
    assert_eq!(at(Command::SwapClipboard), at(Command::Paste) + 1);
}

// ---------- Save selection and Split ----------

#[test]
fn the_selection_is_saved_in_the_files_encoding_and_line_endings() {
    let (mut app, ctx) = app_with_text(&["one", "two caf\u{e9}", "three"]);
    frame(&mut app, &ctx, Vec::new());
    assert!(app.selection_bytes().is_err(), "nothing selected");
    app.doc().text.set_selection((0, 1), (1, 8), false);
    let eol = codec::eol_bytes(app.docs[app.active].text.insert_eol(), app.docs[app.active].text.format.charset);
    let mut wanted = b"ne".to_vec();
    wanted.extend(&eol);
    wanted.extend("two caf\u{e9}".as_bytes());
    assert_eq!(app.selection_bytes().unwrap(), wanted);
    let dir = folder("selection");
    let path = dir.join("part.txt");
    let bytes = app.selection_bytes().unwrap();
    app.write_selection(&path, &bytes);
    assert_eq!(std::fs::read(&path).unwrap(), wanted);
    assert_eq!(lines(&app), ["one", "two caf\u{e9}", "three"], "the file itself isn't changed");
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn parts_start_where_asked_and_an_ending_never_makes_one_alone() {
    let text: Vec<Arc<str>> = ["a", "b", "# two", "c", "# three", "d", ""].iter().map(|line| Arc::from(*line)).collect();
    let line = |at: usize| text[at].clone();
    assert_eq!(part_starts(text.len(), &line, &SplitBy::Lines(2)), [0, 2, 4]);
    assert_eq!(part_starts(text.len(), &line, &SplitBy::Lines(3)), [0, 3], "the last empty line stays with the part before");
    assert_eq!(part_starts(text.len(), &line, &SplitBy::Containing("#".into())), [0, 2, 4]);
    assert_eq!(part_starts(text.len(), &line, &SplitBy::Containing("nowhere".into())), [0]);
    let names = part_names(Path::new(r"C:\logs\big.log"), 3);
    assert_eq!(names[0], Path::new(r"C:\logs\big.part-001.log"));
    assert_eq!(part_names(Path::new(r"C:\x\README"), 1200)[1199], Path::new(r"C:\x\README.part-1200"));
}

fn opened(dir: &Path, name: &str, contents: &[u8]) -> (Limoniter, Context, PathBuf) {
    let path = dir.join(name);
    std::fs::write(&path, contents).unwrap();
    let mut app = Limoniter::build(String::new(), None, Some(PathBuf::from("never-written.png")));
    assert!(app.open_path(&path));
    let ctx = Context::default();
    frame(&mut app, &ctx, Vec::new());
    (app, ctx, path)
}

#[test]
fn split_writes_parts_that_join_back_into_the_file() {
    let dir = folder("split");
    let original = b"line 1\r\nline 2\r\nline 3\r\nline 4\r\nline 5\r\n";
    let (mut app, ctx, path) = opened(&dir, "notes.txt", original);
    give(&mut app, &ctx, Command::SplitFile);
    assert!(matches!(app.dialog, Dialog::SplitFile { .. }));
    app.close_dialog();
    app.start_split(SplitBy::Lines(2));
    wait_until(&mut app, &ctx, |app| app.split_job.is_none());
    assert!(told(&app).contains("3 files"), "{}", told(&app));
    let names = part_names(&path, 3);
    assert_eq!(std::fs::read(&names[0]).unwrap(), b"line 1\r\nline 2\r\n");
    let joined: Vec<u8> = names.iter().flat_map(|name| std::fs::read(name).unwrap()).collect();
    assert_eq!(joined, original);
    // Doing it again would write over them, so it doesn't.
    std::fs::write(&names[2], b"keep me").unwrap();
    app.close_dialog();
    app.start_split(SplitBy::Lines(2));
    wait_until(&mut app, &ctx, |app| app.split_job.is_none());
    assert!(told(&app).contains("there already"));
    assert_eq!(std::fs::read(&names[2]).unwrap(), b"keep me");
    drop(app);
    let _ = std::fs::remove_dir_all(dir);
}

// ---------- Rename ----------

#[test]
fn rename_moves_the_file_and_the_tab_follows_it() {
    let dir = folder("rename");
    let (mut app, ctx, path) = opened(&dir, "draft.txt", b"words\r\n");
    give(&mut app, &ctx, Command::RenameFile);
    assert!(matches!(&app.dialog, Dialog::Rename { name, .. } if name == "draft.txt"));
    frame(&mut app, &ctx, Vec::new());
    app.close_dialog();
    assert!(app.rename_to("bad:name.txt").unwrap_err().contains(':'));
    std::fs::write(dir.join("taken.txt"), b"other").unwrap();
    assert!(app.rename_to("taken.txt").unwrap_err().contains("already"));
    app.rename_to("final.md").unwrap();
    let moved = dir.join("final.md");
    assert!(moved.is_file() && !path.exists());
    assert_eq!(app.docs[app.active].path.as_deref(), Some(moved.as_path()));
    assert_eq!(app.panels.recent.first().map(PathBuf::as_path), Some(moved.as_path()));
    app.rename_to("FINAL.md").expect("only the case changing is allowed");
    drop(app);
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn file_actions_need_a_saved_file() {
    let (mut app, ctx) = app_with_text(&["never saved"]);
    frame(&mut app, &ctx, Vec::new());
    for command in [Command::RenameFile, Command::ShowInExplorer, Command::OpenWithDefault, Command::SplitFile] {
        assert!(!app.can(command));
    }
    assert!(app.rename_to("x.txt").is_err());
}

// ---------- Snippets ----------

#[test]
fn snippets_keep_every_character_through_the_settings_file() {
    let starters = snippets_from_settings("");
    assert!(!starters.is_empty(), "a few to show how they work");
    let kept = vec![Snippet { name: "Odd one".into(), body: "tab\there\nback\\slash\r\nend \\n literally".into() }];
    let settings = snippets_to_settings(&kept);
    assert_eq!(settings.lines().count(), 2, "one line each");
    assert_eq!(snippets_from_settings(&settings), kept);
    assert!(snippets_from_settings("snippets_set=true\n").is_empty(), "all deleted stays deleted");
}

#[test]
fn a_snippets_blanks_are_filled_and_the_cursor_goes_where_marked() {
    let fill = Fill { date: "06/11/2026", time: "09:30", file: "notes.md", selection: "picked" };
    assert_eq!(expand_snippet("[$SELECTION$CURSOR] in $FILE on $DATE at $TIME", &fill), ("[picked".to_string(), "] in notes.md on 06/11/2026 at 09:30".to_string()));
    assert_eq!(expand_snippet("no blanks", &fill), ("no blanks".to_string(), String::new()));
}

#[test]
fn inserting_a_snippet_wraps_the_selection_and_undoes_in_one_step() {
    let (mut app, ctx) = app_with_text(&["say hello now"]);
    frame(&mut app, &ctx, Vec::new());
    app.snippets = vec![Snippet { name: "Bold".into(), body: "**$SELECTION$CURSOR**\nnext".into() }];
    app.doc().text.set_selection((0, 4), (0, 9), false);
    app.insert_snippet(0);
    assert_eq!(lines(&app), ["say **hello**", "next now"]);
    assert_eq!(app.docs[app.active].text.caret_char_position(), (0, 11), "where $CURSOR was");
    give(&mut app, &ctx, Command::Undo);
    assert_eq!(lines(&app), ["say hello now"]);
    app.doc().set_read_only(true);
    app.insert_snippet(0);
    assert_eq!(lines(&app), ["say hello now"]);
    assert!(told(&app).contains("read-only"));
}

#[test]
fn the_snippets_box_finds_makes_and_inserts() {
    let (mut app, ctx) = app_with_text(&["x"]);
    frame(&mut app, &ctx, Vec::new());
    app.snippets = vec![Snippet { name: "Greeting".into(), body: "Dear $CURSOR,".into() }, Snippet { name: "Sign off".into(), body: "Yours".into() }];
    frame(&mut app, &ctx, vec![key(Key::J, Modifiers::CTRL)]);
    assert!(matches!(app.dialog, Dialog::Snippets(_)), "Ctrl+J opens it");
    frame(&mut app, &ctx, Vec::new());
    if let Dialog::Snippets(state) = &mut app.dialog {
        state.filter = "sign".into();
    }
    frame(&mut app, &ctx, Vec::new());
    assert!(matches!(&app.dialog, Dialog::Snippets(state) if state.chosen == Some(1)), "the first that matches is chosen");
    app.close_dialog();
    app.insert_snippet(1);
    assert_eq!(lines(&app), ["Yoursx"]);
}

// ---------- Running a command ----------

#[test]
fn a_command_line_gets_the_files_details() {
    #[cfg(windows)]
    let file = Path::new(r"C:\work\my notes.txt");
    #[cfg(windows)]
    assert_eq!(expand_run_line("type \"%FILE%\" > \"%DIR%\\%STEM%.bak\" & echo %NAME%", Some(file)).unwrap(), "type \"C:\\work\\my notes.txt\" > \"C:\\work\\my notes.bak\" & echo my notes.txt");
    #[cfg(not(windows))]
    let file = Path::new("/work/my notes.txt");
    #[cfg(not(windows))]
    assert_eq!(expand_run_line("cat \"%FILE%\" > \"%DIR%/%STEM%.bak\"; echo %NAME%", Some(file)).unwrap(), "cat \"/work/my notes.txt\" > \"/work/my notes.bak\"; echo my notes.txt");
    assert_eq!(expand_run_line("  dir  ", None).unwrap(), "dir");
    assert!(expand_run_line("type %FILE%", None).unwrap_err().contains("hasn't been saved"));
    assert!(expand_run_line("   ", Some(file)).is_err());
}

#[test]
#[cfg(windows)]
fn a_command_runs_on_the_file_and_what_it_prints_is_in_the_output() {
    let dir = folder("run");
    let (mut app, ctx, _) = opened(&dir, "data.txt", b"first\r\nsecond\r\n");
    app.doc().text.type_text("edited ");
    app.run_tool("type \"%NAME%\" & echo done with %STEM% 1>&2", true).unwrap();
    assert!(app.can(Command::StopTool));
    wait_until(&mut app, &ctx, |app| app.run_job.is_none());
    let output = app.script_output.join("\n");
    assert!(output.contains("edited first"), "saved first, so the command saw the edit: {output}");
    assert!(output.contains("done with data"), "{output}");
    assert!(output.ends_with("Finished."), "{output}");
    assert!(app.show_results);
    assert_eq!(app.run_history, ["type \"%NAME%\" & echo done with %STEM% 1>&2"], "remembered as typed");
    app.run_tool("exit 3", false).unwrap();
    wait_until(&mut app, &ctx, |app| app.run_job.is_none());
    assert!(app.script_output.last().unwrap().contains("exit code 3"));
    assert!(app.current_settings().contains("run_command=exit 3\n"));
    drop(app);
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
#[cfg(windows)]
fn a_long_command_can_be_stopped() {
    let (mut app, ctx) = app_with_text(&["x"]);
    app.run_tool("ping -n 30 127.0.0.1", false).unwrap();
    frame(&mut app, &ctx, Vec::new());
    assert!(app.run_tool("echo second", false).unwrap_err().contains("still running"));
    give(&mut app, &ctx, Command::StopTool);
    wait_until(&mut app, &ctx, |app| app.run_job.is_none());
    assert_eq!(app.script_output.last().map(String::as_str), Some("Stopped."));
}

// ---------- Spelling ----------

#[test]
fn mistakes_are_underlined_once_known_and_only_when_switched_on() {
    let (mut app, ctx) = app_with_text(&["teh cat", "fine"]);
    spelling::forget_all();
    frame(&mut app, &ctx, Vec::new());
    assert!(spelling::take_wanted().is_empty(), "nothing is checked while it's off");
    give(&mut app, &ctx, Command::Toggle(menus::Flag::Spelling));
    assert!(app.prefs.spelling && app.flag_on(menus::Flag::Spelling));
    frame(&mut app, &ctx, Vec::new());
    let shapes_before = frame(&mut app, &ctx, Vec::new()).shapes.len();
    // No checker runs in a test, so the lines are still waiting. Give them their answer.
    let wanted = spelling::take_wanted();
    assert!(wanted.iter().any(|(_, text)| text == "teh cat"));
    spelling::remember(wanted.into_iter().map(|(key, text)| (key, if text == "teh cat" { vec![(0, 3)] } else { Vec::new() })).collect());
    let shapes_after = frame(&mut app, &ctx, Vec::new()).shapes.len();
    assert!(shapes_after > shapes_before, "the wavy line is drawn");
    spelling::forget_all();
}

#[test]
#[cfg(windows)]
fn f7_finds_the_next_mistake_and_change_puts_the_suggestion_in() {
    let (mut app, ctx) = app_with_text(&["This is fine.", "But this has a mistaek in it."]);
    app.speller = spelling::Speller::start(|| {});
    frame(&mut app, &ctx, vec![key(Key::F7, Modifiers::NONE)]);
    for _ in 0..600 {
        frame(&mut app, &ctx, Vec::new());
        if !matches!(app.dialog, Dialog::None) {
            break;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    let Dialog::Spelling { found, .. } = &app.dialog else {
        // A computer without an English or local dictionary says so, and that's all there is to check.
        assert!(told(&app).contains("dictionary") || told(&app).contains("spell checker"), "{}", told(&app));
        return;
    };
    assert_eq!(found.word, "mistaek");
    let chosen = found.suggestions.iter().position(|word| word == "mistake").expect("mistake is suggested");
    assert_eq!(app.docs[app.active].text.selection_text(), "mistaek", "the word is selected");
    let word = found.suggestions[chosen].clone();
    app.spelling_answer(super::file_tools::SpellAnswer::Change(word));
    assert_eq!(lines(&app)[1], "But this has a mistake in it.");
    give(&mut app, &ctx, Command::Undo);
    assert_eq!(lines(&app)[1], "But this has a mistaek in it.", "a change undoes");
    give(&mut app, &ctx, Command::Redo);
    for _ in 0..600 {
        frame(&mut app, &ctx, Vec::new());
        if !matches!(app.dialog, Dialog::None) {
            break;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    assert!(told(&app).contains("No spelling mistakes"), "{}", told(&app));
}

// ---------- In the menus ----------

#[test]
fn the_new_commands_are_in_the_menus_and_the_command_box() {
    let all = menus::every_command();
    for wanted in [Command::SwapClipboard, Command::UsedFileBack, Command::UsedFileForward, Command::SaveSelection, Command::SplitFile, Command::RenameFile, Command::ShowInExplorer, Command::OpenWithDefault, Command::Snippets, Command::RunTool, Command::StopTool, Command::NextMistake, Command::Toggle(menus::Flag::Spelling)] {
        assert!(all.iter().any(|found| found.command == wanted), "a menu has it");
    }
    let found = menus::matching("swap", menus::every_command());
    assert!(found.first().is_some_and(|first| first.command == Command::SwapClipboard));
    let (mut app, ctx) = app_with_text(&["x"]);
    for index in 0..menus::MENUS.len() {
        app.shown_menu = Some((index, true));
        frame(&mut app, &ctx, Vec::new());
    }
}

// ---------- Several clipboards ----------

#[test]
fn copy_and_paste_use_the_clipboard_chosen_and_leave_the_computer_s_alone() {
    crate::clipboards::reset();
    let (mut app, ctx) = app_with_text(&["one two three"]);
    frame(&mut app, &ctx, Vec::new());
    // Ctrl+2 chooses clipboard 2; copying puts "one" there, not on the computer's clipboard.
    frame(&mut app, &ctx, vec![key(Key::Num2, Modifiers::COMMAND)]);
    assert_eq!(crate::clipboards::active(), 2);
    app.doc().text.set_selection((0, 0), (0, 3), false);
    let output = frame(&mut app, &ctx, vec![egui::Event::Copy]);
    assert_eq!(copied(&output), None);
    assert_eq!(crate::clipboards::holding(2).as_deref(), Some("one"));
    // Clipboard 5 gets "three".
    give(&mut app, &ctx, Command::UseClipboard(5));
    app.doc().text.set_selection((0, 8), (0, 13), false);
    frame(&mut app, &ctx, vec![egui::Event::Copy]);
    assert_eq!(crate::clipboards::holding(5).as_deref(), Some("three"));
    // Pasting from 2 at the end, whatever the computer's clipboard says.
    give(&mut app, &ctx, Command::UseClipboard(2));
    app.doc().text.move_caret(0, 13, false);
    frame(&mut app, &ctx, vec![egui::Event::Paste("from elsewhere".into())]);
    assert_eq!(lines(&app), ["one two threeone"]);
    // The computer's clipboard again: what it gives is pasted, and a copy goes to it.
    frame(&mut app, &ctx, vec![key(Key::Num0, Modifiers::COMMAND)]);
    assert_eq!(crate::clipboards::active(), 0);
    frame(&mut app, &ctx, vec![egui::Event::Paste("!".into())]);
    assert_eq!(lines(&app), ["one two threeone!"]);
    app.doc().text.set_selection((0, 4), (0, 7), false);
    let output = frame(&mut app, &ctx, vec![egui::Event::Copy]);
    assert_eq!(copied(&output).as_deref(), Some("two"));
    crate::clipboards::reset();
}

// ---------- .editorconfig ----------

#[test]
fn editorconfig_sets_the_tab_and_tidies_the_file_as_it_s_saved() {
    let folder = std::env::temp_dir().join(format!("limoniter-editorconfig-app-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&folder);
    std::fs::create_dir_all(&folder).unwrap();
    std::fs::write(folder.join(".editorconfig"), "root = true\n[*.c]\nindent_style = space\nindent_size = 3\ntrim_trailing_whitespace = true\ninsert_final_newline = true\n").unwrap();
    let path = folder.join("main.c");
    std::fs::write(&path, "int main() {   \nreturn 0;\n}").unwrap();
    let (mut app, ctx) = app_with_text(&[""]);
    assert!(app.open_path(&path));
    frame(&mut app, &ctx, Vec::new());
    assert_eq!(crate::text_buffer::tab_size(), 3);
    assert!(crate::text_buffer::edit_prefs().spaces_for_tab);
    give(&mut app, &ctx, Command::Save);
    assert_eq!(std::fs::read_to_string(&path).unwrap(), "int main() {\nreturn 0;\n}\n");
    let _ = std::fs::remove_dir_all(&folder);
}

/// Another tab beside the one in front: a click in it brings it forward on its own side, typing goes to it, and
/// choosing the other's tab swaps them back. Ctrl+\ again puts it away.
#[test]
fn another_file_shows_beside_and_a_click_brings_it_forward() {
    let (mut app, ctx) = app_with_text(&["left file"]);
    let first = app.docs[app.active].id;
    app.new_document();
    app.doc().text.set_content(vec!["right file".into()]);
    let second = app.docs[app.active].id;
    // The one used before goes beside, on the right.
    app.active = app.docs.iter().position(|d| d.id == first).unwrap();
    app.used_order = vec![first, second];
    give(&mut app, &ctx, Command::OpenBeside);
    for _ in 0..3 {
        frame(&mut app, &ctx, Vec::new());
    }
    let beside = app.beside.as_ref().unwrap();
    assert_eq!((beside.front, beside.other, beside.right), (first, second, true));
    // A press on the right half brings the second forward, still on the right.
    let at = egui::pos2(1300.0, 300.0);
    frame(&mut app, &ctx, vec![egui::Event::PointerMoved(at), egui::Event::PointerButton { pos: at, button: egui::PointerButton::Primary, pressed: true, modifiers: Modifiers::NONE }]);
    frame(&mut app, &ctx, vec![egui::Event::PointerButton { pos: at, button: egui::PointerButton::Primary, pressed: false, modifiers: Modifiers::NONE }]);
    assert_eq!(app.docs[app.active].id, second);
    let beside = app.beside.as_ref().unwrap();
    assert_eq!((beside.front, beside.other, beside.right), (second, first, false), "the first stays on the left");
    // Chosen from the tabs, the first comes forward on the left again.
    app.active = app.docs.iter().position(|d| d.id == first).unwrap();
    frame(&mut app, &ctx, Vec::new());
    let beside = app.beside.as_ref().unwrap();
    assert_eq!((beside.front, beside.other, beside.right), (first, second, true));
    give(&mut app, &ctx, Command::OpenBeside);
    assert!(app.beside.is_none());
}

/// With nothing open, a folder can still be opened and worked in: Open folder and its kin aren't greyed out.
#[test]
fn a_folder_can_be_opened_with_no_file_open() {
    let (mut app, ctx) = app_with_text(&["x"]);
    app.close_tab(0);
    frame(&mut app, &ctx, Vec::new());
    assert!(app.no_file());
    let standalone = [Command::OpenFolder, Command::GoToFile, Command::GoToSymbol(true), Command::SearchPanel, Command::ToggleTerminal, Command::DragonDisk, Command::AmigaBootDisk, Command::NewImage, Command::OpenUrl, Command::UseClipboard(3), Command::Features, Command::Disassemble, Command::SaveDecrunched];
    for (n, command) in standalone.into_iter().enumerate() {
        assert!(app.can(command), "command {n}");
    }
    // Tasks need somewhere to run: a project (or folder) open.
    assert!(!app.can(Command::BuildTask) && !app.can(Command::EditTasks));
    let root = std::env::temp_dir();
    app.project = Some(crate::project::Project::of_folder(&root.join("t.limproj"), &root));
    assert!(app.can(Command::BuildTask) && app.can(Command::TestTask) && app.can(Command::EditTasks));
    // What works on the file in front stays greyed out.
    for (n, command) in [Command::Save, Command::Find, Command::ConvertNumber, Command::StFloppy, Command::WordCount].into_iter().enumerate() {
        assert!(!app.can(command), "document command {n}");
    }
}

/// The window's size is its own: the window zooms, the text stays the size chosen, and it's kept with the settings.
#[test]
fn the_window_has_its_own_size_apart_from_the_text() {
    let (mut app, ctx) = app_with_text(&["x"]);
    let text_size = app.view.font_size;
    app.window_scale = 1.5;
    // It takes effect the frame after it's set.
    frame(&mut app, &ctx, Vec::new());
    frame(&mut app, &ctx, Vec::new());
    assert!((ctx.zoom_factor() - 1.5).abs() < 0.001, "the window's zoomed");
    assert!((app.text_view_options().font_size * 1.5 - text_size).abs() < 0.01, "the text is drawn at the size it was");
    let kept = app.current_settings();
    assert!(kept.contains("window_scale=1.5"), "{kept}");
    let back = Limoniter::build(kept, None, Some(PathBuf::from("never-written.png")));
    assert!((back.window_scale - 1.5).abs() < 0.001);
    // The box draws.
    app.dialog = Dialog::WindowFont { error: String::new() };
    frame(&mut app, &ctx, Vec::new());
    assert!(matches!(app.dialog, Dialog::WindowFont { .. }));
}

/// With focus following the mouse, moving the pointer into the file beside brings it to the front, no click needed;
/// without it, only a click does.
#[test]
fn focus_follows_the_mouse_between_files_side_by_side() {
    let (mut app, ctx) = app_with_text(&["left"]);
    let first = app.docs[app.active].id;
    app.new_document();
    let second = app.docs[app.active].id;
    app.active = app.docs.iter().position(|d| d.id == first).unwrap();
    app.used_order = vec![first, second];
    give(&mut app, &ctx, Command::OpenBeside);
    frame(&mut app, &ctx, Vec::new());
    // Off: the pointer moving over the right half changes nothing.
    frame(&mut app, &ctx, vec![egui::Event::PointerMoved(egui::pos2(1300.0, 300.0))]);
    frame(&mut app, &ctx, vec![egui::Event::PointerMoved(egui::pos2(1310.0, 310.0))]);
    assert_eq!(app.docs[app.active].id, first);
    // On: it brings the second forward.
    app.view.focus_follows_mouse = true;
    frame(&mut app, &ctx, vec![egui::Event::PointerMoved(egui::pos2(1320.0, 320.0))]);
    frame(&mut app, &ctx, vec![egui::Event::PointerMoved(egui::pos2(1330.0, 330.0))]);
    assert_eq!(app.docs[app.active].id, second);
}

#[test]
fn the_keys_for_a_command_from_a_menu_are_noted_now_and_then() {
    let (mut app, _ctx) = app_with_text(&["text"]);
    for _ in 0..5 {
        app.key_note = None;
        app.note_keys(Command::New);
    }
    assert_eq!(app.key_notes_given.values().copied().collect::<Vec<_>>(), [3], "three times, then no more");
    app.key_notes_given.clear();
    app.note_keys(Command::New);
    assert!(app.key_note.as_ref().is_some_and(|(words, _)| words.contains("Ctrl+N")));
    // Nothing for a command without keys, or with the notes turned off.
    app.key_note = None;
    app.note_keys(Command::Features);
    assert!(app.key_note.is_none());
    app.view.learn_keys = false;
    app.key_notes_given.clear();
    app.note_keys(Command::New);
    assert!(app.key_note.is_none());
}

#[test]
fn panels_that_hide_themselves_come_out_at_the_edge() {
    let (mut app, ctx) = app_with_text(&["x"]);
    app.panels.bookmarks = true;
    app.view.panels_hide = true;
    frame(&mut app, &ctx, vec![egui::Event::PointerMoved(egui::pos2(600.0, 300.0))]);
    assert_eq!(panels::sides_drawn(), ([false, false], [true, false]), "tucked away at the left");
    frame(&mut app, &ctx, vec![egui::Event::PointerMoved(egui::pos2(1.0, 300.0))]);
    assert_eq!(panels::sides_drawn().0, [true, false], "out at the edge");
    frame(&mut app, &ctx, vec![egui::Event::PointerMoved(egui::pos2(60.0, 300.0))]);
    assert_eq!(panels::sides_drawn().0, [true, false], "and staying while the pointer's over it");
    frame(&mut app, &ctx, vec![egui::Event::PointerMoved(egui::pos2(700.0, 300.0))]);
    assert_eq!(panels::sides_drawn().0, [false, false], "gone again after");
    app.view.panels_hide = false;
    frame(&mut app, &ctx, Vec::new());
    assert_eq!(panels::sides_drawn(), ([true, false], [false, false]), "always there with it off");
}

#[test]
fn go_to_file_shows_the_file_chosen() {
    let dir = std::env::temp_dir().join(format!("limoniter-preview-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("a.txt"), "first\n\tsecond\n").unwrap();
    std::fs::write(dir.join("b.bin"), [0u8, 159, 146, 150, 0, 0, 1]).unwrap();
    assert_eq!(workspace_host::preview_of(&dir.join("a.txt")), workspace_host::Preview::Lines(vec!["first".into(), "    second".into()]));
    assert!(matches!(workspace_host::preview_of(&dir.join("b.bin")), workspace_host::Preview::Not(_)));
    assert!(matches!(workspace_host::preview_of(&dir.join("missing")), workspace_host::Preview::Not(_)));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn code_is_made_into_a_picture() {
    let (mut app, ctx) = app_with_text(&["fn main() {", "\tlet x = 1;", "}"]);
    app.open_code_picture();
    assert!(app.code_picture.is_some(), "the box opens");
    frame(&mut app, &ctx, Vec::new());
    let state = app.code_picture.as_ref().expect("still open");
    assert_eq!(binary_tools_host::state_lines(state), 3);
    assert!(binary_tools_host::state_made(state), "a picture is made");
}

#[test]
fn an_sftp_address_opens_by_fetching_the_file() {
    let (mut app, _ctx) = app_with_text(&["x"]);
    assert!(app.open_path(Path::new("sftp://ann@127.0.0.1:1/home/ann/notes.txt")));
    assert_eq!(app.remote_jobs.len(), 1, "a fetch is under way");
    assert_eq!(app.remote_opening.as_deref(), Some("/home/ann/notes.txt"));
    // A folder's address opens the box on it instead.
    assert!(app.open_path(Path::new("sftp://ann@127.0.0.1:1/home/ann/")));
    assert!(matches!(app.dialog, Dialog::Remote(_)));
}
