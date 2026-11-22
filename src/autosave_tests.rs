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

/// A frame drawn with the window having the focus or not, as the window system says each frame.
fn focus(app: &mut Limoniter, ctx: &Context, has: bool) {
    let mut input = super::tests::input(Vec::new());
    input.focused = has;
    let _ = ctx.run_ui(input, |ui| app.draw_frame(ui));
}

fn typed_in(app: &mut Limoniter, ctx: &Context, path: &Path) {
    assert!(app.open_path(path));
    draw(app, ctx, Vec::new());
    app.doc().text.type_text("new ");
    draw(app, ctx, Vec::new());
}

#[test]
fn the_delay_can_be_as_short_as_a_second_and_the_new_settings_are_kept() {
    let prefs = crate::settings::Prefs::from_settings("autosave=true\nautosave_seconds=1\nautosave_when=window\nautosave_skip_errors=true\nhot_exit=false\n");
    assert_eq!(prefs.autosave_seconds, 1);
    assert_eq!(prefs.autosave_when, crate::settings::AutosaveWhen::OnWindowChange);
    assert!(prefs.autosave_skip_errors && !prefs.hot_exit);
    assert_eq!(crate::settings::Prefs::from_settings(&prefs.to_settings()), prefs);
    let usual = crate::settings::Prefs::default();
    assert!(usual.hot_exit && !usual.autosave_skip_errors);
    assert_eq!(usual.autosave_when, crate::settings::AutosaveWhen::AfterDelay);
    assert_eq!(crate::settings::Prefs::from_settings("autosave_when=sideways\n").autosave_when, crate::settings::AutosaveWhen::AfterDelay);
}

#[test]
fn a_file_is_saved_when_the_window_loses_the_focus_without_waiting() {
    let dir = folder("window");
    let path = dir.join("a.txt");
    std::fs::write(&path, "text").unwrap();
    let (mut app, ctx) = app_with("autosave=true\nautosave_when=window\nautosave_seconds=600\n");
    focus(&mut app, &ctx, true);
    typed_in(&mut app, &ctx, &path);
    focus(&mut app, &ctx, true);
    assert_eq!(std::fs::read_to_string(&path).unwrap(), "text", "no delay saves it");
    focus(&mut app, &ctx, false);
    assert_eq!(std::fs::read_to_string(&path).unwrap(), "new text");
    assert!(!app.docs[app.active].is_modified());
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn choosing_another_tab_saves_the_one_left_only_in_the_focus_mode() {
    let dir = folder("tabs");
    let (a, b) = (dir.join("a.txt"), dir.join("b.txt"));
    std::fs::write(&a, "a").unwrap();
    std::fs::write(&b, "b").unwrap();
    for (settings, saved) in [("autosave=true\nautosave_when=focus\n", true), ("autosave=true\nautosave_when=window\n", false)] {
        std::fs::write(&a, "a").unwrap();
        let (mut app, ctx) = app_with(settings);
        typed_in(&mut app, &ctx, &a);
        assert!(app.open_path(&b));
        draw(&mut app, &ctx, Vec::new());
        draw(&mut app, &ctx, Vec::new());
        assert_eq!(std::fs::read_to_string(&a).unwrap() == "new a", saved, "{settings}");
        assert_eq!(std::fs::read_to_string(&b).unwrap(), "b", "the one now in front is left alone");
    }
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_project_can_say_how_it_saves_and_that_is_kept_with_it() {
    let dir = folder("project");
    let path = dir.join("a.txt");
    std::fs::write(&path, "text").unwrap();
    let (mut app, ctx) = app_with("autosave=true\nautosave_seconds=1\n");
    app.new_project(dir.join("shop"));
    assert_eq!(app.autosave_mode(), Some(crate::settings::AutosaveWhen::AfterDelay), "the settings' choice to begin with");
    super::tests::command(&mut app, &ctx, Command::ProjectAutosave(Some(crate::settings::AutosaveChoice::Off)));
    assert_eq!(app.autosave_mode(), None, "the project's own choice wins");
    edited(&mut app, &ctx, &path);
    draw(&mut app, &ctx, Vec::new());
    assert_eq!(std::fs::read_to_string(&path).unwrap(), "text");
    // It's in the project file, and read back.
    let text = std::fs::read_to_string(dir.join("shop.limproj")).unwrap();
    assert!(text.contains("\"autosave\": \"off\""), "{text}");
    let again = crate::project::Project::from_text(&dir.join("shop.limproj"), &text).unwrap();
    assert_eq!(again.autosave, Some(crate::settings::AutosaveChoice::Off));
    super::tests::command(&mut app, &ctx, Command::ProjectAutosave(Some(crate::settings::AutosaveChoice::When(crate::settings::AutosaveWhen::OnWindowChange))));
    assert_eq!(app.autosave_mode(), Some(crate::settings::AutosaveWhen::OnWindowChange));
    super::tests::command(&mut app, &ctx, Command::ProjectAutosave(None));
    assert_eq!(app.autosave_mode(), Some(crate::settings::AutosaveWhen::AfterDelay));
    // Without the setting, a project can still turn it on.
    let (mut off, ctx) = app_with("");
    off.new_project(dir.join("other"));
    assert_eq!(off.autosave_mode(), None);
    super::tests::command(&mut off, &ctx, Command::ProjectAutosave(Some(crate::settings::AutosaveChoice::When(crate::settings::AutosaveWhen::AfterDelay))));
    assert_eq!(off.autosave_mode(), Some(crate::settings::AutosaveWhen::AfterDelay));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_file_the_language_server_finds_errors_in_is_left_when_that_is_asked_for() {
    let dir = folder("errors");
    let path = dir.join("a.txt");
    for (setting, saved) in [("autosave=true\nautosave_skip_errors=true\n", false), ("autosave=true\n", true)] {
        std::fs::write(&path, "text").unwrap();
        let (mut app, ctx) = app_with(setting);
        edited(&mut app, &ctx, &path);
        let intel = app.doc().text.intel.get_or_insert_with(Box::default);
        intel.lsp = true;
        intel.lsp_problems = vec![serde_json::json!({ "severity": 1, "message": "oops", "range": { "start": { "line": 0, "character": 0 }, "end": { "line": 0, "character": 1 } } })];
        long_ago(&mut app);
        draw(&mut app, &ctx, Vec::new());
        assert_eq!(std::fs::read_to_string(&path).unwrap() == "new text", saved, "{setting}");
        if !saved {
            assert_eq!(app.docs[app.active].autosave_skipped, Some("the language server reports errors in it"));
            // Once the errors are gone, it goes.
            app.doc().text.intel.as_mut().unwrap().lsp_problems.clear();
            long_ago(&mut app);
            draw(&mut app, &ctx, Vec::new());
            assert_eq!(std::fs::read_to_string(&path).unwrap(), "new text");
        }
    }
    // Warnings don't count.
    std::fs::write(&path, "text").unwrap();
    let (mut app, ctx) = app_with("autosave=true\nautosave_skip_errors=true\n");
    edited(&mut app, &ctx, &path);
    app.doc().text.intel.get_or_insert_with(Box::default).lsp_problems = vec![serde_json::json!({ "severity": 2, "message": "hmm" })];
    long_ago(&mut app);
    draw(&mut app, &ctx, Vec::new());
    assert_eq!(std::fs::read_to_string(&path).unwrap(), "new text");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn quitting_with_unsaved_files_asks_nothing_and_they_come_back_as_they_were() {
    let dir = folder("hot");
    let root = dir.join("recovery");
    let path = dir.join("a.txt");
    std::fs::write(&path, "text").unwrap();
    let (mut app, ctx) = app_with("");
    app.start_recovery_in(root.clone());
    assert!(app.recovery.is_some());
    typed_in(&mut app, &ctx, &path);
    // A file never saved, as well.
    app.new_document();
    app.doc().text.type_text("scribble");
    draw(&mut app, &ctx, Vec::new());
    assert!(app.settle_unsaved_for_quit(), "nothing is asked, so nothing can be cancelled");
    assert!(app.hot_exit_kept);
    app.finish_recovery();
    assert_eq!(std::fs::read_to_string(&path).unwrap(), "text", "nothing was saved over the file");
    assert!(root.join(HOT_EXIT_MARKER).exists());
    drop(app);

    let (mut next, ctx) = app_with("");
    next.start_recovery_in(root.clone());
    assert!(matches!(next.dialog, Dialog::None), "no offer, it just comes back");
    draw(&mut next, &ctx, Vec::new());
    assert_eq!(next.docs.len(), 2);
    let lines: Vec<String> = next.docs.iter().map(|doc| doc.text.all_lines().iter().map(|l| l.to_string()).collect::<Vec<_>>().join("\n")).collect();
    assert!(lines.contains(&"new text".to_string()) && lines.contains(&"scribble".to_string()), "{lines:?}");
    let back = next.docs.iter().find(|doc| doc.path.is_some()).unwrap();
    assert!(back.is_modified(), "still unsaved, as it was");
    assert!(!root.join(HOT_EXIT_MARKER).exists(), "good for one start only");
    next.hot_exit_kept = false;
    next.finish_recovery();
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn hot_exit_can_be_turned_off_and_leaves_a_file_changed_meanwhile_to_the_usual_offer() {
    let dir = folder("hot-off");
    let root = dir.join("recovery");
    let path = dir.join("a.txt");
    std::fs::write(&path, "text").unwrap();
    // Off: the quit would ask, so it isn't kept.
    let (mut app, ctx) = app_with("hot_exit=false\n");
    app.start_recovery_in(root.clone());
    typed_in(&mut app, &ctx, &path);
    assert!(!app.keep_for_next_start(app.active));
    drop(app);
    let _ = std::fs::remove_dir_all(&root);
    // On, but the file is changed by something else before the next start: the person decides.
    let (mut app, ctx) = app_with("");
    app.start_recovery_in(root.clone());
    typed_in(&mut app, &ctx, &path);
    assert!(app.settle_unsaved_for_quit());
    app.finish_recovery();
    drop(app);
    std::fs::write(&path, "changed by someone else, and longer").unwrap();
    let (mut next, _ctx) = app_with("");
    next.start_recovery_in(root.clone());
    assert!(matches!(next.dialog, Dialog::Recovery(_)), "the usual offer");
    drop(next);
    let _ = std::fs::remove_dir_all(&dir);
}
