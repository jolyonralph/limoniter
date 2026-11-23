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
    // And an example of each, to try a kernel with: Markdown, code, and the same ids every time.
    for (name, kernel, cells) in [("Python notebook (Jupyter): an example", "python3", 8), ("R notebook (Jupyter): an example", "ir", 6)] {
        let kind = kinds.iter().find(|k| k.name == name).unwrap_or_else(|| panic!("{name} is a kind of new file"));
        let nb = Notebook::parse(&kind.starter).unwrap();
        assert_eq!((nb.kernel_name(), nb.cells.len()), (Some(kernel), cells));
        assert_eq!(nb.cells[0].kind, crate::notebook::CellKind::Markdown);
        assert!(nb.cells.iter().skip(1).any(|c| c.kind == crate::notebook::CellKind::Code && c.source.starts_with('#')), "code cells explain themselves");
        assert_eq!(nb.to_json(), kind.starter, "saved as Jupyter saves it");
        // A missing package's error is let through, so the button to install it shows under the cell.
        assert!(!kind.starter.contains("except ImportError"), "imports aren't caught: {name}");
        assert_eq!(new_kinds::NewKind::usual().iter().find(|k| k.name == name).unwrap().starter, kind.starter, "the same every time");
    }
}

#[test]
fn a_kernel_installed_from_the_menu_is_picked_up_by_itself() {
    let (mut app, ctx, dir) = app_with("watch", OUTPUTS);
    frames(&mut app, &ctx, 2);
    state(&mut app).installing();
    assert!(state(&mut app).notice_text().is_some_and(|n| n.contains("being installed")));
    // The stand-in kernels have Python, as if the install had just finished: the next look finds it.
    frames(&mut app, &ctx, 3);
    assert!(state(&mut app).notice_text().is_some_and(|n| n.contains("is installed")), "{:?}", state(&mut app).notice_text());
    let _ = std::fs::remove_dir_all(dir);
}

fn wait_for_install(app: &mut Limoniter, ctx: &Context) -> String {
    for _ in 0..200 {
        frame(app, ctx, Vec::new());
        let progress = state(app).install_progress().unwrap_or_default();
        if progress.starts_with("done") || progress.starts_with("failed") {
            return progress;
        }
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    state(app).install_progress().unwrap_or_default()
}

#[test]
fn the_install_box_installs_names_on_the_kernel_and_says_how_it_went() {
    let (mut app, ctx, dir) = app_with("install", OUTPUTS);
    frames(&mut app, &ctx, 2);
    give(&mut app, &ctx, Command::Notebook(Act::InstallPackages));
    frames(&mut app, &ctx, 2);
    assert_eq!(state(&mut app).install_progress().as_deref(), Some("typing"));
    // Text that isn't a package name is refused, and nothing is sent to the kernel.
    state(&mut app).install_typed("pandas; import os");
    let shown = state(&mut app).install_progress().unwrap();
    assert!(shown.starts_with("typing: ") && shown.contains("isn't a package name"), "{shown}");
    assert!(!state(&mut app).busy(), "nothing was asked of the kernel");
    state(&mut app).install_typed("pandas==2.2 matplotlib");
    frame(&mut app, &ctx, Vec::new());
    assert_eq!(state(&mut app).install_progress().as_deref(), Some("running"));
    let shown = wait_for_install(&mut app, &ctx);
    assert!(shown.starts_with("done") && shown.contains("Successfully installed"), "{shown}");
    // It was no run: no count, and nothing added to any cell.
    assert!(state(&mut app).cells().iter().all(|c| c.outputs.iter().all(|o| !o.to_string().contains("Collecting"))));
    // A package that can't be had says why.
    state(&mut app).install_typed("nosuchpackage");
    let shown = wait_for_install(&mut app, &ctx);
    assert_eq!(shown, "failed: No matching distribution found for nosuchpackage");
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn a_missing_module_error_offers_to_install_and_then_to_run_the_cell_again() {
    let (mut app, ctx, dir) = app_with("install-error", OUTPUTS);
    frames(&mut app, &ctx, 2);
    state(&mut app).select(5);
    state(&mut app).set_source(5, "import nosuchmodule");
    give(&mut app, &ctx, Command::Notebook(Act::RunCellStay));
    for _ in 0..200 {
        frame(&mut app, &ctx, Vec::new());
        if state(&mut app).cells()[5].execution_count.is_some() {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    assert_eq!(state(&mut app).install_button_for(5).as_deref(), Some("nosuchmodule"));
    assert_eq!(state(&mut app).install_button_for(0), None, "other cells have no such button");
    state(&mut app).press_install_button(5);
    let shown = wait_for_install(&mut app, &ctx);
    assert!(shown.starts_with("done"), "{shown}");
    let counted = state(&mut app).cells()[5].execution_count;
    state(&mut app).set_source(5, "print('now it is there')");
    state(&mut app).press_run_again();
    assert_eq!(state(&mut app).install_progress(), None, "the box closes");
    for _ in 0..200 {
        frame(&mut app, &ctx, Vec::new());
        if state(&mut app).cells()[5].execution_count != counted {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    assert!(state(&mut app).cells()[5].outputs[0]["text"].as_str().unwrap().contains("now it is there"));
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn an_r_notebook_installs_r_packages_and_other_languages_cant() {
    let (mut app, ctx, dir) = app_with("install-r", include_str!("notebook_testdata/r.ipynb"));
    frames(&mut app, &ctx, 2);
    give(&mut app, &ctx, Command::Notebook(Act::InstallPackages));
    state(&mut app).install_typed("ggplot2==3.0");
    assert!(state(&mut app).install_progress().unwrap().contains("no versions"), "R has no versions");
    state(&mut app).install_typed("ggplot2 data.table");
    let shown = wait_for_install(&mut app, &ctx);
    assert!(shown.starts_with("done"), "{shown}");
    // A language without installing: the box is drawn, with its explanation, and nothing can be sent.
    let julia = OUTPUTS.replace("\"name\": \"python\"", "\"name\": \"julia\"");
    let (mut app, ctx, dir2) = app_with("install-julia", &julia);
    frames(&mut app, &ctx, 2);
    give(&mut app, &ctx, Command::Notebook(Act::InstallPackages));
    state(&mut app).install_typed("Plots");
    frames(&mut app, &ctx, 2);
    assert_eq!(state(&mut app).install_progress().as_deref(), Some("typing"));
    let _ = (std::fs::remove_dir_all(dir), std::fs::remove_dir_all(dir2));
}

#[test]
fn a_notebook_isnt_offered_what_works_on_plain_text() {
    let (mut app, ctx, dir) = app_with("plain", OUTPUTS);
    frames(&mut app, &ctx, 2);
    for (name, command) in [("Spectrum tape", Command::SaveSpectrumTape), ("BASIC listing", Command::OpenBasicListing), ("GRAPH mode", Command::GraphMode), ("Format document", Command::FormatDocument), ("Format as HTML", Command::Format(crate::formatters::Kind::Html))] {
        assert!(!app.can(command), "{name} is greyed out for a notebook");
    }
    // Code as a picture is the chosen cell's, never the JSON underneath with its pictures' long lines.
    app.open_code_picture();
    let lines = app.code_picture.as_ref().map(|p| p.lines.len()).unwrap_or(0);
    assert!(lines > 0 && lines < 50, "{lines}");
    let _ = std::fs::remove_dir_all(dir);
}
