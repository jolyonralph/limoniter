//! The compare tabs, as the window uses them: opening each kind, editing and saving both sides, copying a change
//! across, comparing folders and copying between them, and saving patches and reports.

use super::super::tests::{app_with, draw};
use super::*;

fn folder(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("limoniter-compare-tab-{}-{tag}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn write(path: &Path, text: &str) {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).unwrap();
    }
    std::fs::write(path, text).unwrap();
}

fn file_tab(app: &mut Limoniter) -> &mut FileCompare {
    match app.docs[app.active].compare.as_deref_mut() {
        Some(CompareTab::Files(files)) => files,
        _ => panic!("no file comparison is in front"),
    }
}

fn folder_tab(app: &mut Limoniter) -> &mut FolderCompare {
    match app.docs[app.active].compare.as_deref_mut() {
        Some(CompareTab::Folders(folders)) => folders,
        _ => panic!("no folder comparison is in front"),
    }
}

/// The lines of one side of the file comparison in front.
fn side_lines(app: &mut Limoniter, side: usize) -> Vec<String> {
    let index = app.active;
    let mut tab = app.docs[index].compare.take().unwrap();
    let CompareTab::Files(files) = &mut *tab else { panic!() };
    let pair = two_docs(&mut app.docs, &mut files.sources).unwrap();
    let lines = pair[side].text.all_lines().iter().map(|line| line.to_string()).collect();
    app.docs[index].compare = Some(tab);
    lines
}

fn wait_for_folders(app: &mut Limoniter, ctx: &Context) {
    let began = std::time::Instant::now();
    loop {
        draw(app, ctx, Vec::new());
        let folders = folder_tab(app);
        if folders.job.is_none() && folders.copy_job.is_none() && folders.patch_job.is_none() {
            return;
        }
        assert!(began.elapsed() < Duration::from_secs(20), "the comparison never finished");
        std::thread::sleep(Duration::from_millis(5));
    }
}

#[test]
fn two_files_open_side_by_side_at_their_first_difference() {
    let dir = folder("pair");
    let (left, right) = (dir.join("left.txt"), dir.join("right.txt"));
    write(&left, "one\ntwo\nthree\nfour\n");
    write(&right, "one\nTWO\nthree\nfour\nfive\n");
    let (mut app, ctx) = app_with("");
    app.open_file_compare(&left, &right);
    assert_eq!(app.docs.len(), 1, "the blank tab gives way to it");
    assert_eq!(app.docs[app.active].name(), "left.txt \u{2194} right.txt");
    let files = file_tab(&mut app);
    assert_eq!(files.diff.summary(), "2 differences");
    assert_eq!(files.diff.current, Some(1), "it starts at the first difference");
    assert!(matches!(files.sources[0], Source2::Own(_)) && matches!(files.sources[1], Source2::Own(_)));
    draw(&mut app, &ctx, Vec::new());
    draw(&mut app, &ctx, Vec::new());
    // Next goes on to the extra line, and wraps round.
    app.run_now(&ctx, Command::NextChange);
    assert_eq!(file_tab(&mut app).diff.current, Some(3));
    app.run_now(&ctx, Command::NextChange);
    assert_eq!(file_tab(&mut app).diff.current, Some(1));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn both_sides_edit_undo_and_save_and_a_change_copies_across() {
    let dir = folder("edit");
    let (left, right) = (dir.join("a.txt"), dir.join("b.txt"));
    write(&left, "same\nours\nend\n");
    write(&right, "same\ntheirs\nend\n");
    let (mut app, ctx) = app_with("");
    app.open_file_compare(&left, &right);
    // Copying the difference to the right, then undoing it there.
    let index = app.active;
    let mut tab = app.docs[index].compare.take().unwrap();
    let CompareTab::Files(files) = &mut *tab else { panic!() };
    let pair = two_docs(&mut app.docs, &mut files.sources).unwrap();
    files.diff.copy_change(pair, 1, Direction::LeftToRight).unwrap();
    app.docs[index].compare = Some(tab);
    assert_eq!(side_lines(&mut app, 1)[1], "ours");
    draw(&mut app, &ctx, Vec::new());
    assert_eq!(file_tab(&mut app).diff.summary(), "No differences", "worked out again after the edit");
    assert!(app.docs[app.active].is_modified(), "the tab knows a side has unsaved changes");
    file_tab(&mut app).diff.active = 1;
    app.run_now(&ctx, Command::Undo);
    assert_eq!(side_lines(&mut app, 1)[1], "theirs", "Undo goes to the side in use");
    app.run_now(&ctx, Command::Redo);
    // Saving writes the changed side to its file.
    app.run_now(&ctx, Command::Save);
    assert_eq!(std::fs::read_to_string(&right).unwrap(), "same\nours\nend\n");
    assert_eq!(std::fs::read_to_string(&left).unwrap(), "same\nours\nend\n", "the other side was never changed");
    assert!(!app.docs[app.active].is_modified());
    draw(&mut app, &ctx, Vec::new());
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn an_open_file_is_compared_with_its_unsaved_edits_and_edited_in_its_tab() {
    let dir = folder("open");
    let (left, right) = (dir.join("a.txt"), dir.join("b.txt"));
    write(&left, "x\ny\n");
    write(&right, "x\ny\n");
    let (mut app, ctx) = app_with("");
    assert!(app.open_path(&left));
    app.doc().text.set_content(vec!["x".into(), "changed".into(), String::new()]);
    let open = app.docs[app.active].id;
    app.open_file_compare(&left, &right);
    let files = file_tab(&mut app);
    assert!(matches!(files.sources[0], Source2::Open(id) if id == open), "the tab's own text is used");
    assert_eq!(files.diff.difference_count(), 1);
    draw(&mut app, &ctx, Vec::new());
    // Its tab closes; the comparison carries on with the file as it is on disk.
    let at = app.docs.iter().position(|doc| doc.id == open).unwrap();
    app.docs[at].text.mark_saved();
    app.close_tab(at);
    draw(&mut app, &ctx, Vec::new());
    assert!(matches!(file_tab(&mut app).sources[0], Source2::Own(_)));
    draw(&mut app, &ctx, Vec::new());
    assert_eq!(file_tab(&mut app).diff.difference_count(), 0);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn ignoring_spaces_case_and_line_endings_changes_what_differs() {
    let dir = folder("ignore");
    let (left, right) = (dir.join("a.txt"), dir.join("b.txt"));
    write(&left, "Hello  World\r\nend\r\n");
    write(&right, "hello world\nend\n");
    let (mut app, ctx) = app_with("");
    app.open_file_compare(&left, &right);
    assert_eq!(file_tab(&mut app).diff.difference_count(), 1);
    file_tab(&mut app).diff.options = TextOptions { ignore_whitespace: true, ignore_case: true, ignore_line_endings: true };
    draw(&mut app, &ctx, Vec::new());
    assert_eq!(file_tab(&mut app).diff.difference_count(), 0);
    file_tab(&mut app).diff.options.ignore_line_endings = false;
    draw(&mut app, &ctx, Vec::new());
    assert!(file_tab(&mut app).diff.difference_count() > 0, "CR LF against LF");
    assert!(file_tab(&mut app).diff.note.contains("Line endings differ"));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_patch_of_two_files_is_a_unified_diff() {
    let dir = folder("patch");
    let (left, right) = (dir.join("a.txt"), dir.join("b.txt"));
    write(&left, "1\n2\n3\n");
    write(&right, "1\nTWO\n3\n");
    let (mut app, _ctx) = app_with("");
    app.open_file_compare(&left, &right);
    let index = app.active;
    let mut tab = app.docs[index].compare.take().unwrap();
    let CompareTab::Files(files) = &mut *tab else { panic!() };
    let names = files.names.clone();
    let pair = two_docs(&mut app.docs, &mut files.sources).unwrap();
    let patch = file_patch(&names, [&*pair[0], &*pair[1]], &TextOptions::default());
    assert!(patch.ends_with("@@ -1,3 +1,3 @@\n 1\n-2\n+TWO\n 3\n"), "{patch}");
    assert!(patch.starts_with("--- "));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn files_that_arent_text_are_compared_byte_by_byte() {
    let dir = folder("bytes");
    let (left, right) = (dir.join("a.bin"), dir.join("b.bin"));
    std::fs::write(&left, [0u8, 1, 2, 3, 4, 5, 6, 7, 8]).unwrap();
    std::fs::write(&right, [0u8, 1, 9, 3, 4, 5, 6, 7, 8, 10]).unwrap();
    let (mut app, ctx) = app_with("");
    app.open_file_compare(&left, &right);
    let Some(CompareTab::Bytes(bytes)) = app.docs[app.active].compare.as_deref_mut() else {
        panic!("a byte comparison opens");
    };
    assert_eq!(bytes.differences, [2..3, 9..10]);
    assert!(bytes.summary().contains("2 bytes differ, in 2 places"), "{}", bytes.summary());
    bytes.step(true);
    assert_eq!(bytes.current, Some(0));
    draw(&mut app, &ctx, Vec::new());
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn pictures_are_compared_as_pictures() {
    let dir = folder("pictures");
    let (left, right) = (dir.join("a.scr"), dir.join("b.scr"));
    let mut screen = vec![0u8; 6912];
    screen[6144..].fill(0x38);
    std::fs::write(&left, &screen).unwrap();
    screen[0] = 0xFF;
    std::fs::write(&right, &screen).unwrap();
    let (mut app, ctx) = app_with("");
    app.open_file_compare(&left, &right);
    let Some(CompareTab::Pictures(pictures)) = app.docs[app.active].compare.as_deref() else {
        panic!("a picture comparison opens");
    };
    assert_eq!(pictures.difference.3, 8, "eight pixels across the first byte");
    assert!(pictures.summary().contains("8 pixels differ"));
    draw(&mut app, &ctx, Vec::new());
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn the_same_file_twice_is_refused() {
    let dir = folder("same");
    let path = dir.join("a.txt");
    write(&path, "x");
    let (mut app, _ctx) = app_with("");
    app.open_file_compare(&path, &path);
    assert!(matches!(&app.dialog, Dialog::Message { body, .. } if body.contains("same file")));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn folders_are_compared_in_the_background_and_copied_between_with_a_preview_first() {
    let dir = folder("folders");
    let (left, right) = (dir.join("left"), dir.join("right"));
    write(&left.join("same.txt"), "same");
    write(&right.join("same.txt"), "same");
    write(&left.join("changed.txt"), "new version");
    write(&right.join("changed.txt"), "old");
    write(&left.join("sub/only-left.txt"), "mine");
    write(&right.join("only-right.txt"), "theirs");
    let (mut app, ctx) = app_with("");
    app.open_folder_compare(&left, &right);
    assert_eq!(app.docs[app.active].name(), "left \u{2194} right");
    folder_tab(&mut app).mode = Mode::Thorough;
    let known = app.known_hosts.clone();
    folder_tab(&mut app).start(known, None);
    wait_for_folders(&mut app, &ctx);
    let found = folder_tab(&mut app);
    let result = found.result.as_ref().expect("it finished");
    assert_eq!(result.counts(), engine::Counts { same: 1, different: 1, left_only: 1, right_only: 1, ..Default::default() });
    assert!(found.expanded.contains("sub"), "folders that differ start opened out");
    // Choosing everything and copying it to the right: a preview first, and only what differs.
    found.selected = result.rows.iter().filter(|row| row.parent.is_none()).map(|row| row.path.clone()).collect();
    let root = compare_backups_root();
    found.plan_copy(Direction::LeftToRight, &root).unwrap();
    let plan = &found.copy.as_ref().unwrap().plan;
    let names: Vec<(&str, bool)> = plan.files.iter().map(|item| (item.path.as_str(), item.replaces)).collect();
    assert_eq!(names, [("sub/only-left.txt", false), ("changed.txt", true)]);
    assert_eq!(std::fs::read_to_string(right.join("changed.txt")).unwrap(), "old", "nothing happens until it's confirmed");
    draw(&mut app, &ctx, Vec::new());
    folder_tab(&mut app).confirm_copy(None);
    wait_for_folders(&mut app, &ctx);
    assert_eq!(std::fs::read_to_string(right.join("changed.txt")).unwrap(), "new version");
    assert_eq!(std::fs::read_to_string(right.join("sub/only-left.txt")).unwrap(), "mine");
    assert!(right.join("only-right.txt").exists(), "nothing is deleted");
    let notice = folder_tab(&mut app).notice.clone().unwrap();
    assert!(notice.contains("Copied 2 files") && notice.contains("kept in"), "{notice}");
    // It compares again by itself afterwards.
    let counts = folder_tab(&mut app).result.as_ref().unwrap().counts();
    assert_eq!((counts.same, counts.different, counts.left_only, counts.right_only), (3, 0, 0, 1));
    // A report and a patch.
    let id = app.docs[app.active].id;
    let report = dir.join("report.html");
    app.save_folder_report(id, &report).unwrap().unwrap();
    assert!(std::fs::read_to_string(&report).unwrap().contains("Only on the right"));
    let text = dir.join("report.txt");
    app.save_folder_report(id, &text).unwrap().unwrap();
    assert!(std::fs::read_to_string(&text).unwrap().starts_with("Folder comparison\n"));
    let patch = dir.join("all.patch");
    app.start_folder_patch(id, patch.clone());
    wait_for_folders(&mut app, &ctx);
    assert!(std::fs::read_to_string(&patch).unwrap().contains("+++ b/only-right.txt"));
    let _ = std::fs::remove_dir_all(&dir);
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn a_pair_in_a_folder_comparison_opens_as_a_file_comparison() {
    let dir = folder("pair-from-folders");
    write(&dir.join("l/a.txt"), "1\n2\n");
    write(&dir.join("r/a.txt"), "1\n3\n");
    let (mut app, ctx) = app_with("");
    app.open_folder_compare(dir.join("l"), dir.join("r"));
    wait_for_folders(&mut app, &ctx);
    let id = app.docs[app.active].id;
    app.folder_compare_act(id, FolderAct::OpenPair(0));
    assert_eq!(file_tab(&mut app).diff.difference_count(), 1);
    assert_eq!(app.docs.len(), 2, "in a tab of its own");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_side_that_cant_be_read_says_why() {
    let (mut app, ctx) = app_with("");
    app.open_folder_compare("/no/such/left", "/no/such/right");
    wait_for_folders(&mut app, &ctx);
    assert!(folder_tab(&mut app).problem.as_deref().is_some_and(|p| p.contains("isn't a folder") || p.contains("couldn't be read")));
    // A folder comparison with nothing chosen yet just waits.
    app.open_folder_compare_sides(None, None);
    assert!(folder_tab(&mut app).job.is_none());
    let known = app.known_hosts.clone();
    folder_tab(&mut app).start(known, None);
    assert!(folder_tab(&mut app).problem.as_deref().is_some_and(|p| p.contains("Choose")));
    draw(&mut app, &ctx, Vec::new());
}

#[test]
fn every_way_in_opens_the_compare_tab() {
    let (mut app, ctx) = app_with("");
    // The commands are in the menus and the command box, and need no file open.
    for command in [Command::CompareFiles, Command::CompareFolders] {
        assert!(command.works_without_file());
        assert!(crate::app::script_host::SCRIPT_COMMANDS.iter().any(|(_, known)| *known == command));
    }
    app.run_now(&ctx, Command::CompareFolders);
    assert!(matches!(app.docs[app.active].compare.as_deref(), Some(CompareTab::Folders(_))));
    // Compare with... still asks which, from the file in front.
    app.new_document();
    app.doc().text.set_content(vec!["a".into()]);
    app.new_document();
    app.doc().text.set_content(vec!["b".into()]);
    app.run_now(&ctx, Command::Compare);
    let Dialog::ComparePick(others) = &app.dialog else { panic!("it asks which") };
    assert_eq!(others.len(), 1, "the other file, not this one or the compare tab");
    app.dialog = Dialog::None;
    // A pair of sides as a picker shows them, and back.
    let side = Side::Git { repo: PathBuf::from("/repo"), revision: "main".into(), folder: "src".into() };
    let picker = SidePicker::of(&side);
    assert_eq!(picker.kind, SideKind::Git);
    assert_eq!(picker.revision, "main");
    let server = SidePicker { kind: SideKind::Server, path: "sftp://me@example.com/srv".into(), revision: String::new() };
    assert!(matches!(server.side(), Ok(Side::Sftp { folder, .. }) if folder == "/srv"));
    assert!(SidePicker { kind: SideKind::Folder, path: " ".into(), revision: String::new() }.side().is_err());
    draw(&mut app, &ctx, Vec::new());
}

impl Limoniter {
    /// Gives a command as the menus would, for a test.
    fn run_now(&mut self, ctx: &Context, command: Command) {
        let mut output = ctx.run_ui(egui::RawInput::default(), |ui| self.run(command, ui));
        output.textures_delta.clear();
    }
}
