//! A project's own file templates, from the menus to the files made.

use super::tests::{app_with, draw};
use super::*;

fn folder(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("limoniter-templates-host-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn with_project(name: &str) -> (Limoniter, Context, PathBuf) {
    let (mut app, ctx) = app_with("");
    let dir = folder(name);
    app.new_project(dir.join("shop"));
    draw(&mut app, &ctx, Vec::new());
    (app, ctx, dir)
}

#[test]
fn templates_beside_the_project_file_are_listed_for_the_menu() {
    let (app, _ctx, dir) = with_project("listed");
    assert!(app.project_templates().is_empty());
    std::fs::create_dir_all(dir.join("templates")).unwrap();
    std::fs::write(dir.join("templates").join("page.html"), "<h1>${name}</h1>").unwrap();
    let found = app.project_templates();
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].name, "page.html");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn new_from_template_makes_the_file_asks_for_blanks_and_puts_the_caret_at_the_cursor() {
    let (mut app, ctx, dir) = with_project("make");
    std::fs::create_dir_all(dir.join("templates")).unwrap();
    std::fs::write(dir.join("templates").join("view.txt"), "${Name} by ${owner:nobody}\n  ${cursor}!\n").unwrap();
    std::fs::create_dir_all(dir.join("src")).unwrap();
    app.open_template_box(Some(dir.join("src")), 0);
    draw(&mut app, &ctx, Vec::new());
    let Dialog::Template(state) = &mut app.dialog else { panic!("the box is open") };
    assert_eq!(state.asked.len(), 1, "the template's own blank is asked for");
    state.name = "my-view".into();
    state.asked[0].1 = "Sam".into();
    app.make_from_template();
    assert!(matches!(app.dialog, Dialog::None));
    let made = dir.join("src").join("my-view.txt");
    assert_eq!(std::fs::read_to_string(&made).unwrap(), "MyView by Sam\n  !\n");
    assert_eq!(app.docs[app.active].path.as_deref(), Some(made.as_path()), "it's open, in front");
    assert_eq!((app.docs[app.active].text.caret_line, app.docs[app.active].text.caret_col), (1, 2));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_name_already_taken_is_refused_in_the_box() {
    let (mut app, ctx, dir) = with_project("taken");
    std::fs::create_dir_all(dir.join("templates")).unwrap();
    std::fs::write(dir.join("templates").join("a.txt"), "new").unwrap();
    std::fs::write(dir.join("a.txt"), "mine").unwrap();
    app.open_template_box(Some(dir.clone()), 0);
    draw(&mut app, &ctx, Vec::new());
    let Dialog::Template(state) = &mut app.dialog else { panic!() };
    state.name = "a".into();
    app.make_from_template();
    let Dialog::Template(state) = &app.dialog else { panic!("still open") };
    assert!(state.problem.contains("already"));
    assert_eq!(std::fs::read_to_string(dir.join("a.txt")).unwrap(), "mine");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_template_chosen_for_a_folder_fills_in_new_files_made_in_the_explorer() {
    let (mut app, ctx, dir) = with_project("rule");
    let templates = dir.join("templates");
    std::fs::create_dir_all(&templates).unwrap();
    std::fs::write(templates.join("controller.php"), "<?php\nnamespace ${namespace};\nclass ${name} {}\n").unwrap();
    crate::file_templates::set_rule(&templates, "src/Controller", Some("controller.php")).unwrap();
    let made = dir.join("src").join("Controller").join("Home.php");
    let _ = ctx.clone().run_ui(Default::default(), |ui| app.explorer_act(crate::explorer::Act::Create { path: made.clone(), folder: false }, ui));
    assert_eq!(std::fs::read_to_string(&made).unwrap(), "<?php\nnamespace Controller;\nclass Home {}\n");
    // Elsewhere, the file is empty as before.
    let other = dir.join("src").join("Other.php");
    let _ = ctx.clone().run_ui(Default::default(), |ui| app.explorer_act(crate::explorer::Act::Create { path: other.clone(), folder: false }, ui));
    assert_eq!(std::fs::read_to_string(&other).unwrap(), "");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_file_is_saved_as_a_template_and_the_menus_offer_them() {
    let (mut app, ctx, dir) = with_project("save");
    std::fs::write(dir.join("Invoice.php"), "<?php class Invoice {}").unwrap();
    assert!(app.open_path(&dir.join("Invoice.php")));
    draw(&mut app, &ctx, Vec::new());
    assert!(app.can(Command::SaveAsTemplate));
    command_run(&mut app, &ctx, Command::SaveAsTemplate);
    let Dialog::SaveTemplate(state) = &mut app.dialog else { panic!("the box is open") };
    assert_eq!(state.name, "Invoice.php", "it starts with the file's name");
    state.name = "class.php".into();
    draw(&mut app, &ctx, Vec::new());
    let Dialog::SaveTemplate(state) = &mut app.dialog else { panic!() };
    // The box's Save button is pressed through the same call it makes.
    let saved = crate::file_templates::save_as(&state.dir, &state.name, &state.text, &state.stem, state.name_blank);
    assert!(saved.is_ok());
    assert_eq!(std::fs::read_to_string(dir.join("templates").join("class.php")).unwrap(), "<?php class ${Name} {}");
    assert_eq!(app.project_templates()[0].name, "class.php");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn the_template_commands_need_a_project() {
    let (app, _ctx) = app_with("");
    assert!(!app.can(Command::NewFromTemplate(0)));
    assert!(!app.can(Command::SaveAsTemplate));
}

fn command_run(app: &mut Limoniter, ctx: &Context, command: Command) {
    super::tests::command(app, ctx, command);
}
