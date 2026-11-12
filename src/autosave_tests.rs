//! Saving files by themselves, and keeping a backup on the first save.

use super::tests::{app_with, draw};
use super::*;

fn folder(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("limoniter-autosave-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// Opens a file, types at its start, and makes autosave think the change is old enough to save.
fn edited(app: &mut Limoniter, ctx: &Context, path: &Path) {
    assert!(app.open_path(path));
    draw(app, ctx, Vec::new());
    app.doc().text.type_text("new ");
    draw(app, ctx, Vec::new());
    long_ago(app);
}

fn long_ago(app: &mut Limoniter) {
    let doc = app.doc();
    doc.unsaved_since = Some(std::time::Instant::now() - std::time::Duration::from_secs(3600));
}

#[test]
fn a_changed_file_saves_itself_once_its_time_is_up() {
    let dir = folder("saves");
    let path = dir.join("notes.txt");
    std::fs::write(&path, "text").unwrap();
    let (mut app, ctx) = app_with("autosave=true\nautosave_seconds=60\n");
    assert!(app.open_path(&path));
    draw(&mut app, &ctx, Vec::new());
    app.doc().text.type_text("new ");
    draw(&mut app, &ctx, Vec::new());
    assert!(app.docs[app.active].unsaved_since.is_some(), "the wait has started");
    assert_eq!(std::fs::read_to_string(&path).unwrap(), "text", "not before its time");
    long_ago(&mut app);
    draw(&mut app, &ctx, Vec::new());
    assert_eq!(std::fs::read_to_string(&path).unwrap(), "new text");
    assert!(!app.docs[app.active].is_modified());
    assert_eq!(app.print_status, "Saved notes.txt by itself");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn nothing_is_saved_by_itself_when_autosave_is_off() {
    let dir = folder("off");
    let path = dir.join("notes.txt");
    std::fs::write(&path, "text").unwrap();
    let (mut app, ctx) = app_with("");
    assert!(!app.prefs.autosave, "it starts off");
    edited(&mut app, &ctx, &path);
    draw(&mut app, &ctx, Vec::new());
    assert_eq!(std::fs::read_to_string(&path).unwrap(), "text");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn files_that_would_need_asking_are_left_for_the_person() {
    let dir = folder("left");
    let (mut app, ctx) = app_with("autosave=true\n");
    // Never saved: there's nowhere to save it.
    app.new_document();
    app.doc().text.type_text("draft");
    draw(&mut app, &ctx, Vec::new());
    assert_eq!(app.docs[app.active].autosave_skipped, Some("it hasn't been saved before"));
    // Changed by another program since it was opened: saving would lose that.
    let changed = dir.join("changed.txt");
    std::fs::write(&changed, "mine").unwrap();
    edited(&mut app, &ctx, &changed);
    std::fs::write(&changed, "theirs, and longer").unwrap();
    draw(&mut app, &ctx, Vec::new());
    assert_eq!(std::fs::read_to_string(&changed).unwrap(), "theirs, and longer");
    assert_eq!(app.docs[app.active].autosave_skipped, Some("another program has changed the file"));
    // A character its encoding can't hold: saving would have to ask what to do.
    let ascii = dir.join("ascii.txt");
    std::fs::write(&ascii, "plain").unwrap();
    assert!(app.open_path(&ascii));
    draw(&mut app, &ctx, Vec::new());
    app.doc().text.format.charset = charset::known("us-ascii");
    app.doc().text.type_text("\u{e9}");
    draw(&mut app, &ctx, Vec::new());
    long_ago(&mut app);
    draw(&mut app, &ctx, Vec::new());
    assert_eq!(std::fs::read_to_string(&ascii).unwrap(), "plain");
    assert_eq!(app.docs[app.active].autosave_skipped, Some("some characters can't be written in its encoding"));
    assert!(matches!(app.dialog, Dialog::None), "nothing is asked");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_backup_is_kept_the_first_time_a_file_is_saved() {
    let dir = folder("backup");
    let path = dir.join("notes.txt");
    let backup = dir.join("notes.txt.bak");
    std::fs::write(&path, "first").unwrap();
    let (mut app, ctx) = app_with("backup_on_save=true\n");
    assert!(app.open_path(&path));
    draw(&mut app, &ctx, Vec::new());
    app.doc().text.type_text("second ");
    assert!(app.save(app.active));
    assert_eq!(std::fs::read_to_string(&backup).unwrap(), "first");
    // Later saves keep the file as it was when the session began.
    app.doc().text.type_text("third ");
    assert!(app.save(app.active));
    assert_eq!(std::fs::read_to_string(&backup).unwrap(), "first");
    assert_eq!(std::fs::read_to_string(&path).unwrap(), "second third first");
    // Without the setting, none is kept.
    let other = dir.join("other.txt");
    std::fs::write(&other, "x").unwrap();
    let (mut plain, ctx) = app_with("");
    assert!(plain.open_path(&other));
    draw(&mut plain, &ctx, Vec::new());
    plain.doc().text.type_text("y");
    assert!(plain.save(plain.active));
    assert!(!dir.join("other.txt.bak").exists());
    let _ = std::fs::remove_dir_all(&dir);
}
