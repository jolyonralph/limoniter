//! The notebook window, run as the whole app with no screen: opening a notebook, keys, running cells on the stand-in
//! kernel, writing changes back into the text, Undo, and saving.

use super::*;
use crate::notebook::Notebook;
use super::notebook_host::{Act, NotebookState};

const OUTPUTS: &str = include_str!("notebook_testdata/outputs.ipynb");

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

fn press(key: Key, modifiers: Modifiers) -> Vec<egui::Event> {
    vec![egui::Event::Key { key, physical_key: None, pressed: true, repeat: false, modifiers }, egui::Event::Key { key, physical_key: None, pressed: false, repeat: false, modifiers }]
}

fn app_with(name: &str, text: &str) -> (Limoniter, Context, PathBuf) {
    let dir = std::env::temp_dir().join(format!("limoniter-notebook-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join("work.ipynb");
    std::fs::write(&file, text).unwrap();
    let mut app = Limoniter::build(String::new(), None, Some(PathBuf::from("never-written.png")));
    assert!(app.open_path(&file));
    (app, Context::default(), dir)
}

fn state(app: &mut Limoniter) -> &mut NotebookState {
    app.docs[app.active].notebook.as_deref_mut().expect("shown as a notebook")
}

fn frames(app: &mut Limoniter, ctx: &Context, count: usize) {
    for _ in 0..count {
        frame(app, ctx, Vec::new());
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
}

#[test]
fn an_ipynb_file_opens_as_a_notebook_and_is_left_unchanged_by_looking_at_it() {
    let (mut app, ctx, dir) = app_with("opens", OUTPUTS);
    frames(&mut app, &ctx, 3);
    assert!(app.notebook_shown(app.active));
    assert_eq!(state(&mut app).cells().len(), 6);
    assert!(!app.docs[app.active].is_modified(), "drawing a notebook doesn't edit it");
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn a_notebook_that_is_saved_without_changes_is_the_same_file() {
    let (mut app, ctx, dir) = app_with("same", OUTPUTS);
    frames(&mut app, &ctx, 2);
    give(&mut app, &ctx, Command::Save);
    frames(&mut app, &ctx, 2);
    assert_eq!(std::fs::read_to_string(dir.join("work.ipynb")).unwrap(), OUTPUTS);
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn a_change_is_written_into_the_text_saved_as_jupyter_would_and_undone() {
    let (mut app, ctx, dir) = app_with("change", OUTPUTS);
    frames(&mut app, &ctx, 2);
    state(&mut app).select(1);
    give(&mut app, &ctx, Command::Notebook(Act::InsertBelow));
    frames(&mut app, &ctx, 2);
    assert_eq!(state(&mut app).cells().len(), 7);
    assert!(app.docs[app.active].is_modified());
    let after: Vec<String> = app.docs[app.active].text.all_lines().iter().map(|l| l.to_string()).collect();
    assert_eq!(after, state(&mut app).notebook().to_lines(), "the text is the notebook, written out");
    give(&mut app, &ctx, Command::Save);
    let saved = std::fs::read_to_string(dir.join("work.ipynb")).unwrap();
    assert_eq!(Notebook::parse(&saved).unwrap().cells.len(), 7);
    assert_eq!(saved, state(&mut app).notebook().to_json());
    // Undo is the editor's own, and the notebook follows the text back.
    give(&mut app, &ctx, Command::Undo);
    frames(&mut app, &ctx, 2);
    assert_eq!(state(&mut app).cells().len(), 6);
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn jupyters_command_keys_work_with_no_cell_being_typed_in() {
    let (mut app, ctx, dir) = app_with("keys", OUTPUTS);
    frames(&mut app, &ctx, 2);
    frame(&mut app, &ctx, press(Key::B, Modifiers::NONE));
    frames(&mut app, &ctx, 1);
    assert_eq!(state(&mut app).cells().len(), 7, "B adds a cell below");
    assert_eq!(state(&mut app).chosen(), 1);
    frame(&mut app, &ctx, press(Key::M, Modifiers::NONE));
    frames(&mut app, &ctx, 1);
    assert_eq!(state(&mut app).cells()[1].kind, crate::notebook::CellKind::Markdown);
    frame(&mut app, &ctx, press(Key::D, Modifiers::NONE));
    frames(&mut app, &ctx, 1);
    assert_eq!(state(&mut app).cells().len(), 7, "one D does nothing yet");
    frame(&mut app, &ctx, press(Key::D, Modifiers::NONE));
    frames(&mut app, &ctx, 1);
    assert_eq!(state(&mut app).cells().len(), 6, "D D deletes");
    frame(&mut app, &ctx, press(Key::ArrowDown, Modifiers::NONE));
    assert_eq!(state(&mut app).chosen(), 2);
    frame(&mut app, &ctx, press(Key::A, Modifiers::NONE));
    assert_eq!(state(&mut app).cells().len(), 7);
    assert_eq!(state(&mut app).chosen(), 2, "A adds above and chooses the new cell");
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn shift_enter_runs_the_cell_on_the_kernel_and_the_answer_lands_in_the_cell() {
    let (mut app, ctx, dir) = app_with("run", OUTPUTS);
    frames(&mut app, &ctx, 2);
    state(&mut app).select(5);
    state(&mut app).set_source(5, "print('hello')");
    give(&mut app, &ctx, Command::Notebook(Act::RunCellStay));
    for _ in 0..200 {
        frame(&mut app, &ctx, Vec::new());
        if state(&mut app).cells()[5].execution_count.is_some() {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    let cell = state(&mut app).cells()[5].clone();
    assert_eq!(cell.execution_count, Some(1));
    let text = cell.outputs[0]["text"].as_str().unwrap().to_string();
    assert!(text.contains("print('hello')"), "{text}");
    let text_lines: Vec<String> = app.docs[app.active].text.all_lines().iter().map(|l| l.to_string()).collect();
    assert!(text_lines.iter().any(|l| l.contains("print('hello')")), "the output was written into the file's text too");
    // Shift+Enter on the last cell runs it and adds one after.
    frame(&mut app, &ctx, press(Key::Enter, Modifiers::SHIFT));
    frames(&mut app, &ctx, 1);
    assert_eq!(state(&mut app).cells().len(), 7);
    assert_eq!(state(&mut app).chosen(), 6);
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn view_notebook_as_text_shows_the_json_and_back_again() {
    let (mut app, ctx, dir) = app_with("astext", OUTPUTS);
    frames(&mut app, &ctx, 2);
    assert!(app.can(Command::Notebook(Act::AsText)));
    give(&mut app, &ctx, Command::Notebook(Act::AsText));
    assert!(!app.notebook_shown(app.active));
    frames(&mut app, &ctx, 2);
    // Edited as text, the notebook reads that when it comes back.
    app.docs[app.active].text.replace_lines(0, 0, vec!["{".to_string()]);
    let line = app.docs[app.active].text.all_lines().iter().position(|l| l.contains("\"nbformat_minor\": 5")).unwrap();
    app.docs[app.active].text.replace_lines(line, line, vec![" \"nbformat_minor\": 4".to_string()]);
    give(&mut app, &ctx, Command::Notebook(Act::AsText));
    frames(&mut app, &ctx, 2);
    assert!(app.notebook_shown(app.active));
    assert_eq!(state(&mut app).notebook().nbformat_minor, 4);
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn an_old_or_broken_notebook_says_so_and_can_be_shown_as_text() {
    let old = r#"{"nbformat": 3, "nbformat_minor": 0, "worksheets": [{"cells": []}], "metadata": {}}"#;
    let (mut app, ctx, dir) = app_with("old", old);
    frames(&mut app, &ctx, 2);
    assert!(state(&mut app).problem().is_some());
    assert!(!app.docs[app.active].is_modified());
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn other_files_are_not_notebooks() {
    let (mut app, ctx, dir) = app_with("plain", OUTPUTS);
    frames(&mut app, &ctx, 1);
    assert!(app.docs[app.active].notebook.is_some());
    assert!(!super::notebook_host::is_notebook_name(Path::new("a.json")));
    assert!(super::notebook_host::is_notebook_name(Path::new("A.IPYNB")));
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn new_python_and_r_notebooks_are_offered_and_valid() {
    let kinds = new_kinds::NewKind::usual();
    for (name, kernel, language) in [("Python notebook (Jupyter)", "python3", "python"), ("R notebook (Jupyter)", "ir", "R")] {
        let kind = kinds.iter().find(|k| k.name == name).unwrap_or_else(|| panic!("{name} is a kind of new file"));
        assert_eq!(kind.ending, "ipynb");
        let nb = Notebook::parse(&kind.starter).unwrap();
        assert_eq!((nb.kernel_name(), nb.language()), (Some(kernel), Some(language)));
        assert_eq!(nb.cells.len(), 1);
        assert_eq!(nb.to_json(), kind.starter);
        assert_eq!(kind.group(), "Modern languages");
    }
}
