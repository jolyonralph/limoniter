//! Python and R in the window, run as the whole app with no screen: the environment found and used by F9 and the
//! tasks, Ctrl+Enter and a script's cells sent to the console (on the stand-in kernel), and knitting R Markdown.

use super::*;
use super::python_host::PyAction;

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

fn folder(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("limoniter-pyr-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn app_on(file: &Path, text: &str) -> (Limoniter, Context) {
    std::fs::write(file, text).unwrap();
    let mut app = Limoniter::build(String::new(), None, Some(PathBuf::from("never-written.png")));
    assert!(app.open_path(file));
    crate::highlight::load_now();
    (app, Context::default())
}

/// What the Python console has been sent, entry by entry.
fn sent(app: &mut Limoniter) -> Vec<String> {
    app.python_console_sources(crate::repl::Lang::Python)
}

/// Waits for the command run to finish, taking in what it printed. (Only the Unix-only tests run commands.)
#[cfg(unix)]
fn wait_for_run(app: &mut Limoniter) {
    let began = std::time::Instant::now();
    while app.check_run_tool() && began.elapsed() < Duration::from_secs(60) {
        std::thread::sleep(Duration::from_millis(20));
    }
    app.check_run_tool();
}

#[test]
fn ctrl_enter_sends_the_statement_and_moves_on() {
    let dir = folder("send");
    let (mut app, ctx) = app_on(&dir.join("work.py"), "def f(x):\n    return x * 2\n\nprint(f(21))\n");
    frame(&mut app, &ctx, Vec::new());
    let ctrl_enter = vec![egui::Event::Key { key: Key::Enter, physical_key: None, pressed: true, repeat: false, modifiers: Modifiers::COMMAND }];
    frame(&mut app, &ctx, ctrl_enter.clone());
    assert_eq!(sent(&mut app), ["def f(x):\n    return x * 2"], "the whole function");
    assert_eq!(app.doc().text.caret_line, 3, "on to the next line with code");
    assert!(app.python.open, "the console opened to show it");
    frame(&mut app, &ctx, ctrl_enter);
    assert_eq!(sent(&mut app).len(), 2);
    assert_eq!(app.doc().text.line(1).as_ref(), "    return x * 2", "nothing was typed into the file");
    // A selection is sent as it is.
    app.doc().text.move_caret(1, 4, false);
    app.doc().text.move_caret(1, 10, true);
    give(&mut app, &ctx, Command::PyR(PyAction::SendToConsole));
    assert_eq!(sent(&mut app).last().map(String::as_str), Some("return"));
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn a_scripts_cells_run_in_the_console() {
    let dir = folder("cells");
    let (mut app, ctx) = app_on(&dir.join("cells.py"), "import math\n# %% First\nx = 1\n\n# %% Second\ny = x + 1\n# %% Third\nprint(y)\n");
    frame(&mut app, &ctx, Vec::new());
    assert_eq!(app.doc().text_state.cells, Some(crate::code_cells::Style::Python), "the markers are drawn");
    give(&mut app, &ctx, Command::PyR(PyAction::RunCellAt(4)));
    assert_eq!(sent(&mut app), ["y = x + 1"], "a marker's link runs its own cell");
    give(&mut app, &ctx, Command::PyR(PyAction::RunAboveAt(4)));
    assert_eq!(sent(&mut app)[1..], ["import math", "x = 1", "y = x + 1"]);
    app.doc().text.move_caret(7, 0, false);
    give(&mut app, &ctx, Command::PyR(PyAction::RunCell));
    assert_eq!(sent(&mut app).last().map(String::as_str), Some("print(y)"));
    give(&mut app, &ctx, Command::PyR(PyAction::RunAllCells));
    assert_eq!(sent(&mut app).len(), 1 + 3 + 1 + 4);
    // The console's panel draws them, with what the stand-in kernel gave back.
    for _ in 0..5 {
        frame(&mut app, &ctx, Vec::new());
    }
    assert!(app.python_console_transcript(crate::repl::Lang::Python).contains(">>> print(y)"));
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn an_r_script_has_its_own_console() {
    let dir = folder("r");
    let (mut app, ctx) = app_on(&dir.join("analysis.R"), "## ---- load\nx <- c(1, 2, 3)\nmean(x) |>\n  print()\n");
    frame(&mut app, &ctx, Vec::new());
    assert_eq!(app.doc().text_state.cells, Some(crate::code_cells::Style::R));
    app.doc().text.move_caret(2, 0, false);
    give(&mut app, &ctx, Command::PyR(PyAction::SendToConsole));
    assert_eq!(app.python_console_sources(crate::repl::Lang::R), ["mean(x) |>\n  print()"]);
    assert!(sent(&mut app).is_empty(), "not to Python's");
    let _ = std::fs::remove_dir_all(dir);
}

#[cfg(unix)]
#[test]
fn a_projects_environment_is_used_by_f9_and_the_tasks() {
    let dir = folder("env");
    // A real environment, made without pip as that's quicker: F9 runs the script with its Python.
    let made = crate::platform::command("python3").args(["-m", "venv", "--without-pip"]).arg(dir.join(".venv")).status();
    if !made.is_ok_and(|s| s.success()) {
        eprintln!("python3 -m venv didn't work here, so F9 wasn't tried with an environment.");
        return;
    }
    std::fs::write(dir.join("requirements.txt"), "requests\n").unwrap();
    std::fs::write(dir.join("test_maths.py"), "def test_add():\n    assert 1 + 1 == 2\n").unwrap();
    let (mut app, ctx) = app_on(&dir.join("where.py"), "import sys\nprint('prefix', sys.prefix)\n");
    frame(&mut app, &ctx, Vec::new());
    let env = app.python.current.as_ref().map(|c| c.env.clone()).expect("found");
    assert_eq!(env.name, ".venv");
    assert_eq!(env.kind, crate::py_env::Kind::Venv);
    let tasks = app.tasks_here();
    let pytest = tasks.iter().find(|t| t.name == "pytest").expect("pytest's task");
    assert!(pytest.command.contains(".venv") && pytest.command.ends_with("-m pytest -v"), "{}", pytest.command);
    assert!(tasks.iter().any(|t| t.command.contains(".venv") && t.command.ends_with("-m pip install -r requirements.txt")), "{tasks:?}");
    give(&mut app, &ctx, Command::RunOnMachine);
    wait_for_run(&mut app);
    let printed = app.script_output.join("\n");
    // Python says the real path, which on a Mac is /private/var/… for the temporary folder at /var/….
    let real = std::fs::canonicalize(dir.join(".venv")).unwrap();
    assert!(printed.contains(&format!("prefix {}", real.display())), "{printed}");
    // The computer's own Python chosen instead: kept for the folder, and used from then on.
    app.python_use_env(Path::new("python3"));
    assert_eq!(app.python.current.as_ref().map(|c| c.env.kind), Some(crate::py_env::Kind::System));
    assert!(app.python.chosen.get(&dir).is_some());
    assert_eq!(app.python_env_for(&dir.join("where.py")).map(|e| e.kind), Some(crate::py_env::Kind::System));
    let _ = std::fs::remove_dir_all(dir);
}

#[cfg(unix)]
#[test]
fn r_markdown_is_knitted_and_listed_as_a_task() {
    let dir = folder("knit");
    let rmd = dir.join("report.Rmd");
    let (mut app, ctx) = app_on(&rmd, "---\ntitle: Report\noutput: html_document\n---\n\nSome words.\n\n```{r}\n6 * 7\n```\n");
    let has_rmarkdown = crate::lsp::on_path("Rscript")
        && crate::platform::command("Rscript").args(["-e", "quit(status = if (requireNamespace('rmarkdown', quietly = TRUE) && rmarkdown::pandoc_available()) 0 else 1)"]).status().is_ok_and(|s| s.success());
    if !has_rmarkdown {
        eprintln!("R with rmarkdown and Pandoc isn't here, so knitting wasn't tried.");
        return;
    }
    assert!(app.tasks_here().iter().any(|t| t.name == "Knit / Render to HTML" && t.command.contains("rmarkdown::render")));
    give(&mut app, &ctx, Command::PyR(PyAction::KnitRender));
    wait_for_run(&mut app);
    let page = std::fs::read_to_string(dir.join("report.html")).unwrap_or_default();
    assert!(page.contains("42"), "the chunk ran: {:?}", app.script_output);
    frame(&mut app, &ctx, Vec::new());
    let _ = std::fs::remove_dir_all(dir);
}
