use super::super::tests::{app_with, draw};
use super::*;

fn open_named(app: &mut Limoniter, name: &str, text: &str) {
    let dir = std::env::temp_dir().join(format!("limoniter-themes-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join(name);
    std::fs::write(&file, text).unwrap();
    assert!(app.open_path(&file));
}

#[test]
fn each_document_gets_the_theme_its_rules_give_it() {
    let (mut app, ctx) = app_with("");
    app.themes.document = "Nord".into();
    app.themes.rules = vec![Rule::new(When::Ending, ".seq, PRG", "Commodore 64"), Rule::new(When::Language, "rust", "Monokai"), Rule::new(When::Hex, "", "Green screen")];
    open_named(&mut app, "notes.txt", "plain words\n");
    assert_eq!(app.themes.document_for(&app.docs[app.active]).name, "Nord", "no rule fits, so the usual theme");
    open_named(&mut app, "game.seq", "HELLO\r");
    assert_eq!(app.themes.document_for(&app.docs[app.active]).name, "Commodore 64", "endings are matched without dots or case");
    // Languages are known once the grammars have loaded and the document has been drawn.
    crate::highlight::start_loading(ctx.clone());
    let began = std::time::Instant::now();
    while crate::highlight::engine().is_none() {
        assert!(began.elapsed() < std::time::Duration::from_secs(60), "the grammars never loaded");
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    open_named(&mut app, "main.rs", "fn main() {}\n");
    draw(&mut app, &ctx, Vec::new());
    assert_eq!(app.themes.document_for(&app.docs[app.active]).name, "Monokai");
    app.doc().hex_mode = true;
    assert_eq!(app.themes.document_for(&app.docs[app.active]).name, "Green screen", "hex view has no language, so the hex rule fits");
    app.doc().hex_mode = false;
    // A theme chosen for one document comes before every rule.
    app.doc().theme_override = Some("Dracula".into());
    assert_eq!(app.themes.document_for(&app.docs[app.active]).name, "Dracula");
    // Drawing sets the theme of the document in front, and leaves the window's alone.
    app.themes.window = "Paper".into();
    draw(&mut app, &ctx, Vec::new());
    assert_eq!(theme::editor_back(), app.themes.document_named("Dracula").unwrap().editor_back);
    assert_eq!(theme::window(), app.themes.window_theme().window);
    assert!(theme::is_light() && !theme::document_is_light());
}

#[test]
fn character_sets_pick_their_machines_theme_as_suggested() {
    let (mut app, _ctx) = app_with("");
    assert_eq!(app.themes.rules, suggested_rules(), "a first run has the suggested rules");
    app.new_document();
    for (id, theme) in [("petscii-c64", "Commodore 64"), ("screen-vic20", "Commodore 64"), ("atascii", "Atari 8-bit"), ("zx81", "ZX Spectrum"), ("teletext-de", "BBC Micro and teletext"), ("bbc-mode7", "BBC Micro and teletext"), ("amiga-text", "Amiga Workbench"), ("cp437", "Limoniter Light")] {
        app.doc().text.format.charset = crate::charset::known(id);
        assert_eq!(app.themes.document_for(&app.docs[app.active]).name, theme, "{id}");
    }
}

#[test]
fn themes_and_rules_are_kept_with_the_settings() {
    let (mut app, _ctx) = app_with("");
    let mut mine = theme::document_from("Mine", false, Color32::from_rgb(1, 2, 3), Color32::WHITE, Color32::YELLOW, "Nord");
    mine.caret = Color32::RED;
    app.themes.documents.push(Rc::new(mine.clone()));
    app.themes.windows.push(Rc::new(theme::window_from("My window", true, Color32::WHITE, Color32::BLACK, Color32::BLUE)));
    app.themes.window = "My window".into();
    app.themes.document = "Mine".into();
    app.themes.rules = vec![Rule::new(When::CharacterSet, "ebcdic", "Green screen")];
    let (again, _) = app_with(&app.current_settings());
    assert_eq!(again.themes.window, "My window");
    assert_eq!(again.themes.document, "Mine");
    assert_eq!(*again.themes.document_named("Mine").unwrap(), mine);
    assert_eq!(again.themes.rules, app.themes.rules);
    // No rules, once chosen, stays no rules.
    app.themes.rules.clear();
    let (again, _) = app_with(&app.current_settings());
    assert!(again.themes.rules.is_empty());
    // Someone who chose dark before there were themes keeps it.
    let (old, _) = app_with("theme=dark\n");
    assert_eq!((old.themes.window.as_str(), old.themes.document.as_str()), ("Limoniter Dark", "Limoniter Dark"));
}

#[test]
fn the_themes_box_draws_every_page_and_every_theme() {
    let (mut app, ctx) = app_with("");
    app.doc().text.type_text("some text\nmore");
    for page in [ThemePage::Documents, ThemePage::Window, ThemePage::Rules] {
        app.open_theme_box(page);
        draw(&mut app, &ctx, Vec::new());
        assert!(matches!(app.dialog, Dialog::Themes(_)));
    }
    // Every theme in use, window and document, draws the window and every menu.
    let windows: Vec<String> = app.themes.windows.iter().map(|t| t.name.clone()).collect();
    let documents: Vec<String> = app.themes.documents.iter().map(|t| t.name.clone()).collect();
    app.close_dialog();
    for (window, document) in windows.iter().zip(documents.iter().cycle()) {
        app.themes.window.clone_from(window);
        app.themes.document.clone_from(document);
        for menu in 0..menus::MENUS.len() {
            app.shown_menu = Some((menu, true));
            draw(&mut app, &ctx, Vec::new());
        }
        assert_eq!(theme::window(), app.themes.window_theme().window);
    }
}
