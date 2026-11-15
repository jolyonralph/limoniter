//! Syncing in the compare tab, saved comparisons in a project and beside the settings, rules as the tab and
//! Settings use them, applying a patch from the box, and the extra reports.

use super::super::super::tests::{app_with, draw};
use super::*;
use std::time::{Duration, Instant};

fn folder(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("limoniter-sync-tab-{}-{tag}", std::process::id()));
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

fn folder_tab(app: &mut Limoniter) -> &mut FolderCompare {
    match app.docs[app.active].compare.as_deref_mut() {
        Some(CompareTab::Folders(folders)) => folders,
        _ => panic!("no folder comparison is in front"),
    }
}

fn settle(app: &mut Limoniter, ctx: &Context) {
    let began = Instant::now();
    loop {
        draw(app, ctx, Vec::new());
        let folders = folder_tab(app);
        if !folders.busy() && !folders.snapshot_wanted {
            draw(app, ctx, Vec::new());
            return;
        }
        assert!(began.elapsed() < Duration::from_secs(20), "it never finished");
        std::thread::sleep(Duration::from_millis(5));
    }
}

fn two_folders(dir: &Path) -> (PathBuf, PathBuf) {
    let (left, right) = (dir.join("left"), dir.join("right"));
    write(&left.join("same.txt"), "same");
    write(&right.join("same.txt"), "same");
    write(&left.join("changed.txt"), "new version");
    write(&right.join("changed.txt"), "old");
    write(&left.join("sub/only-left.txt"), "mine");
    write(&right.join("only-right.txt"), "theirs");
    write(&right.join("old/stale.txt"), "stale");
    (left, right)
}

#[test]
fn a_mirror_is_previewed_asks_again_with_the_count_and_backs_up_what_it_deletes() {
    let dir = folder("mirror");
    let (left, right) = two_folders(&dir);
    let (mut app, ctx) = app_with("");
    app.open_folder_compare(&left, &right);
    folder_tab(&mut app).mode = Mode::Thorough;
    let id = app.docs[app.active].id;
    app.folder_compare_act(id, FolderAct::Start);
    settle(&mut app, &ctx);
    let tab = folder_tab(&mut app);
    tab.sync_kind = SyncKind::Mirror;
    tab.sync_way = SyncWay::LeftToRight;
    app.folder_compare_act(id, FolderAct::Sync);
    settle(&mut app, &ctx);
    let tab = folder_tab(&mut app);
    let preview = tab.sync.as_mut().expect("a preview first");
    let counts = preview.plan.counts();
    assert_eq!((counts.create, counts.replace, counts.delete, counts.folders_removed), (1, 1, 2, 1), "{counts:?}");
    assert_eq!(std::fs::read_to_string(right.join("changed.txt")).unwrap(), "old", "nothing happens until it's agreed to");
    // Unticking the stale file keeps it, and its folder with it.
    preview.plan.items.iter_mut().find(|item| item.path == "old/stale.txt").unwrap().ticked = false;
    assert_eq!(preview.plan.deletions(), 2);
    assert_eq!(sync_pressed(preview), None, "a sync that deletes asks again");
    assert!(preview.confirming);
    draw(&mut app, &ctx, Vec::new());
    let tab = folder_tab(&mut app);
    assert_eq!(sync_pressed(tab.sync.as_mut().unwrap()), Some(true));
    tab.confirm_sync(None);
    settle(&mut app, &ctx);
    assert_eq!(std::fs::read_to_string(right.join("changed.txt")).unwrap(), "new version");
    assert_eq!(std::fs::read_to_string(right.join("sub/only-left.txt")).unwrap(), "mine");
    assert!(!right.join("only-right.txt").exists());
    assert!(right.join("old/stale.txt").exists(), "unticked, so kept");
    let tab = folder_tab(&mut app);
    let notice = tab.notice.clone().unwrap();
    assert!(notice.contains("kept in"), "{notice}");
    let backups = compare_backups_root();
    let kept: Vec<PathBuf> = std::fs::read_dir(&backups).unwrap().flatten().map(|entry| entry.path().join("right/only-right.txt")).filter(|path| path.is_file()).collect();
    assert_eq!(kept.len(), 1, "the deleted file is in a backup folder named for when");
    assert_eq!(std::fs::read_to_string(&kept[0]).unwrap(), "theirs");
    assert!(tab.snapshot.as_ref().is_some_and(|snapshot| snapshot.files.contains_key("changed.txt")), "how both sides were is remembered");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn an_update_with_nothing_to_do_says_so_and_a_disk_image_is_refused() {
    let dir = folder("in-step");
    write(&dir.join("l/a.txt"), "a");
    write(&dir.join("r/a.txt"), "a");
    let (mut app, ctx) = app_with("");
    app.open_folder_compare(dir.join("l"), dir.join("r"));
    settle(&mut app, &ctx);
    let id = app.docs[app.active].id;
    app.folder_compare_act(id, FolderAct::Sync);
    settle(&mut app, &ctx);
    assert!(folder_tab(&mut app).notice.as_deref().is_some_and(|notice| notice.contains("in step")));
    // A Git revision on the left can be read, but not written to.
    let tab = folder_tab(&mut app);
    let result = tab.result.as_mut().unwrap();
    result.left = Side::Git { repo: dir.clone(), revision: "HEAD".into(), folder: String::new() };
    write(&dir.join("r/extra.txt"), "x");
    tab.sync_way = SyncWay::RightToLeft;
    app.folder_compare_act(id, FolderAct::Sync);
    settle(&mut app, &ctx);
    let problem = folder_tab(&mut app).problem.clone().unwrap_or_default();
    assert!(problem.contains("history"), "{problem}");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_comparison_is_saved_in_the_project_and_opens_again_as_it_was() {
    let dir = folder("project-session");
    let (left, right) = two_folders(&dir);
    let (mut app, ctx) = app_with("");
    let project_file = dir.join("work.limproj");
    app.project = Some(Project::new(&project_file));
    app.open_folder_compare(&left, &right);
    settle(&mut app, &ctx);
    let id = app.docs[app.active].id;
    let tab = folder_tab(&mut app);
    tab.filters.exclude = "*.bak".into();
    tab.hide_same = true;
    tab.sync_kind = SyncKind::Update;
    tab.sync_way = SyncWay::Both;
    tab.filters.rules.list.insert(0, Rule::new(RuleKind::SizeAndTime, "*.log"));
    // Saving asks for a name, offering to keep it in the project.
    app.folder_compare_act(id, FolderAct::SaveSession);
    let Dialog::CompareSession(state) = &app.dialog else { panic!("it asks for a name") };
    assert!(state.in_project && state.name == "left and right", "{}", state.name);
    draw(&mut app, &ctx, Vec::new());
    app.dialog = Dialog::None;
    app.save_compare_session_as(id, "Website", true).unwrap();
    assert!(app.save_compare_session_as(id, "website", true).is_err(), "a name already taken");
    assert_eq!(app.docs[app.active].name(), "Website (comparison)");
    let saved = Project::load(&project_file).unwrap();
    assert_eq!(saved.comparisons.len(), 1);
    let session = &saved.comparisons[0];
    assert_eq!((session.left.clone(), session.right.clone()), (Side::Folder(left.clone()), Side::Folder(right.clone())));
    assert_eq!((session.filters.exclude.as_str(), session.hide_same, session.sync_way), ("*.bak", true, SyncWay::Both));
    assert_eq!(session.filters.rules.list[0], Rule::new(RuleKind::SizeAndTime, "*.log"));
    assert!(std::fs::read_to_string(&project_file).unwrap().contains("\"path\": \"left\""), "kept relative to the project");
    // The panel lists it, and opens it.
    let mut state = crate::project::PanelState::default();
    let mut output = ctx.run_ui(egui::RawInput::default(), |ui| {
        crate::project::panel(ui, &mut state, app.project.as_ref(), None);
    });
    output.textures_delta.clear();
    app.project_compare(crate::project::PanelAction::OpenComparison("Website".into()));
    settle(&mut app, &ctx);
    let reopened = folder_tab(&mut app);
    assert_eq!(reopened.session, Some(("Website".into(), true)));
    assert_eq!((reopened.filters.exclude.as_str(), reopened.hide_same, reopened.sync_way), ("*.bak", true, SyncWay::Both));
    assert!(reopened.result.is_some(), "it compares at once");
    // After a sync, what's remembered is stored with it.
    let reopened_id = app.docs[app.active].id;
    app.folder_compare_act(reopened_id, FolderAct::Sync);
    settle(&mut app, &ctx);
    folder_tab(&mut app).confirm_sync(None);
    settle(&mut app, &ctx);
    let stored = Project::load(&project_file).unwrap();
    assert!(stored.comparisons[0].snapshot.as_ref().is_some_and(|snapshot| snapshot.files.contains_key("same.txt") && snapshot.files.contains_key("sub/only-left.txt")), "the last sync is kept in the project");
    // Renamed and deleted from the box.
    app.rename_session("Website", true, "Site").unwrap();
    assert_eq!(app.project.as_ref().unwrap().comparisons[0].name, "Site");
    assert_eq!(folder_tab(&mut app).session, Some(("Site".into(), true)), "the open tab keeps up");
    app.open_sessions_box(Some("Site".into()));
    draw(&mut app, &ctx, Vec::new());
    app.remove_session("Site", true).unwrap();
    assert!(Project::load(&project_file).unwrap().comparisons.is_empty());
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn without_a_project_a_comparison_is_kept_beside_the_settings() {
    let dir = folder("app-session");
    let (left, right) = two_folders(&dir);
    let (mut app, ctx) = app_with("");
    let file = dir.join("data").join(crate::compare_session::FILE_NAME);
    app.compare_sessions_file = Some(file.clone());
    app.open_folder_compare(&left, &right);
    settle(&mut app, &ctx);
    let id = app.docs[app.active].id;
    app.save_compare_session_as(id, "Backup run", false).unwrap();
    let kept = crate::compare_session::load(&file);
    assert_eq!(kept.len(), 1);
    assert_eq!(kept[0].name, "Backup run");
    // Saving again stores it under the same name.
    folder_tab(&mut app).mode = Mode::Thorough;
    app.folder_compare_act(id, FolderAct::SaveSession);
    assert!(matches!(app.dialog, Dialog::None), "no question the second time");
    assert_eq!(crate::compare_session::load(&file)[0].mode, Mode::Thorough);
    // Opened again from the list of saved comparisons.
    app.run_now_folder(&ctx, Command::CompareSessions);
    assert!(matches!(&app.dialog, Dialog::CompareSession(state) if state.saving.is_none() && state.selected == Some(("Backup run".into(), false))));
    draw(&mut app, &ctx, Vec::new());
    app.dialog = Dialog::None;
    app.open_compare_session("Backup run", false);
    settle(&mut app, &ctx);
    assert_eq!(folder_tab(&mut app).mode, Mode::Thorough);
    app.rename_session("Backup run", false, "Nightly").unwrap();
    assert_eq!(crate::compare_session::load(&file)[0].name, "Nightly");
    app.remove_session("Nightly", false).unwrap();
    assert!(crate::compare_session::load(&file).is_empty());
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn rules_in_settings_start_each_comparison_and_decide_how_a_pair_opens() {
    let dir = folder("rules");
    write(&dir.join("l/build.log"), "Started 10:00\nok\n");
    write(&dir.join("r/build.log"), "Started 11:30\nok\n");
    write(&dir.join("l/data.bin"), "\u{1}\u{2}\u{3}");
    write(&dir.join("r/data.bin"), "\u{1}\u{9}\u{3}");
    write(&dir.join("l/main.o"), "object");
    let (mut app, ctx) = app_with("");
    let mut rules = Rules::usual();
    rules.list.insert(0, Rule { patterns: "*.log".into(), kind: RuleKind::Text, text: TextOptions::default(), ignore_lines: "^Started".into() });
    rules.list.insert(0, Rule::new(RuleKind::Binary, "*.bin"));
    app.compare_rules = rules.clone();
    assert!(app.current_settings().contains("compare_rules="), "kept in the settings");
    app.open_folder_compare(dir.join("l"), dir.join("r"));
    folder_tab(&mut app).mode = Mode::Thorough;
    let id = app.docs[app.active].id;
    app.folder_compare_act(id, FolderAct::Start);
    settle(&mut app, &ctx);
    let tab = folder_tab(&mut app);
    assert_eq!(tab.filters.rules, rules);
    let result = tab.result.as_ref().unwrap();
    let status = |path: &str| result.rows.iter().find(|row| row.path == path).map(|row| row.status);
    assert_eq!(status("build.log"), Some(Status::Same), "the timestamp line is left out");
    assert_eq!(status("main.o"), None, "always left out");
    let log = result.rows.iter().position(|row| row.path == "build.log").unwrap();
    let bin = result.rows.iter().position(|row| row.path == "data.bin").unwrap();
    app.folder_compare_act(id, FolderAct::OpenPair(bin));
    assert!(matches!(app.docs[app.active].compare.as_deref(), Some(CompareTab::Bytes(_))), "bytes open as bytes");
    app.active = app.docs.iter().position(|doc| doc.id == id).unwrap();
    app.folder_compare_act(id, FolderAct::OpenPair(log));
    let Some(CompareTab::Files(files)) = app.docs[app.active].compare.as_deref_mut() else { panic!("text opens side by side") };
    assert!(files.diff.ignore_lines.is_some());
    draw(&mut app, &ctx, Vec::new());
    let Some(CompareTab::Files(files)) = app.docs[app.active].compare.as_deref() else { panic!() };
    assert_eq!(files.diff.difference_count(), 0, "the same, with the line left out");
    // The rules box, for Settings and for one comparison.
    app.open_rules_box(None);
    assert!(matches!(&app.dialog, Dialog::CompareRules(state) if state.tab.is_none() && state.rules == rules));
    draw(&mut app, &ctx, Vec::new());
    app.open_rules_box(Some(id));
    assert!(matches!(&app.dialog, Dialog::CompareRules(state) if state.tab == Some(id)));
    draw(&mut app, &ctx, Vec::new());
    app.dialog = Dialog::None;
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_patch_is_checked_then_applied_with_backups() {
    let dir = folder("patch");
    let (left, right) = (dir.join("l"), dir.join("r"));
    write(&left.join("a.txt"), "one\ntwo\nthree\n");
    write(&right.join("a.txt"), "one\nTWO\nthree\n");
    write(&right.join("new.txt"), "fresh\n");
    let (mut app, ctx) = app_with("");
    app.open_folder_compare(&left, &right);
    settle(&mut app, &ctx);
    let id = app.docs[app.active].id;
    folder_tab(&mut app).mode = Mode::Thorough;
    app.folder_compare_act(id, FolderAct::Start);
    settle(&mut app, &ctx);
    let patch = dir.join("changes.patch");
    app.start_folder_patch(id, patch.clone());
    settle(&mut app, &ctx);
    // The patch from left to right, applied to the left, makes it like the right.
    app.open_patch_box(&patch, Some(left.clone()), Some(id));
    let Dialog::ApplyPatch(state) = &app.dialog else { panic!("the box opens") };
    let Some(Ok(checks)) = &state.checks else { panic!("checked at once") };
    assert_eq!(checks.len(), 2);
    assert!(checks.iter().all(FileCheck::ready));
    draw(&mut app, &ctx, Vec::new());
    let Dialog::ApplyPatch(state) = std::mem::replace(&mut app.dialog, Dialog::None) else { panic!() };
    let said = app.carry_out_patch(&state);
    assert!(said.contains("changed 1") && said.contains("made 1"), "{said}");
    assert_eq!(std::fs::read_to_string(left.join("a.txt")).unwrap(), "one\nTWO\nthree\n");
    assert_eq!(std::fs::read_to_string(left.join("new.txt")).unwrap(), "fresh\n");
    settle(&mut app, &ctx);
    assert_eq!(folder_tab(&mut app).result.as_ref().unwrap().counts().same, 2, "compared again");
    // A second time, nothing fits, and nothing is written.
    app.open_patch_box(&patch, Some(left.clone()), None);
    let Dialog::ApplyPatch(state) = std::mem::replace(&mut app.dialog, Dialog::None) else { panic!() };
    let said = app.carry_out_patch(&state);
    assert!(said.contains("changed 0"), "{said}");
    // One that isn't a patch says so.
    write(&dir.join("not.patch"), "hello");
    app.open_patch_box(&dir.join("not.patch"), None, None);
    assert!(matches!(&app.dialog, Dialog::Message { body, .. } if body.contains("unified diff")));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn reports_can_be_the_counts_only_or_a_csv_file() {
    let dir = folder("reports");
    let (left, right) = two_folders(&dir);
    let (mut app, ctx) = app_with("");
    app.open_folder_compare(&left, &right);
    settle(&mut app, &ctx);
    let id = app.docs[app.active].id;
    let summary = dir.join("summary.txt");
    app.save_folder_report_as(id, &summary, ReportKind::Summary).unwrap().unwrap();
    let text = std::fs::read_to_string(&summary).unwrap();
    assert!(text.starts_with("Folder comparison summary\n") && text.contains("sub/") && !text.contains("only-left.txt"), "{text}");
    let html = dir.join("summary.html");
    app.save_folder_report_as(id, &html, ReportKind::Summary).unwrap().unwrap();
    assert!(std::fs::read_to_string(&html).unwrap().contains("<table>"));
    let csv = dir.join("all.csv");
    app.save_folder_report_as(id, &csv, ReportKind::Csv).unwrap().unwrap();
    let text = std::fs::read_to_string(&csv).unwrap();
    assert!(text.starts_with("Status,Path,Kind,Left size"));
    assert!(text.contains("Only on the right,only-right.txt,File,,,6,"), "{text}");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_saved_site_fills_in_a_server_side_with_its_key() {
    let (mut app, ctx) = app_with("");
    app.remote_sites = vec![remote_host::SavedSite { name: "web".into(), site: crate::remote::Site { host: "example.com".into(), port: 2222, user: "me".into(), key_file: Some(PathBuf::from("/keys/id")) } }];
    app.open_folder_compare_sides(None, None);
    folder_tab(&mut app).pickers[1].kind = SideKind::Server;
    draw(&mut app, &ctx, Vec::new());
    // As choosing it in the list does.
    let saved = app.remote_sites[0].site.clone();
    let picker = &mut folder_tab(&mut app).pickers[1];
    picker.path = saved.url("/~/");
    picker.key_file = saved.key_file.clone();
    let Ok(Side::Sftp { site, folder }) = picker.side() else { panic!("a server side") };
    assert_eq!(site, saved, "with its key file");
    assert_eq!(folder, "/~/", "starting in the home folder");
}

impl Limoniter {
    fn run_now_folder(&mut self, ctx: &Context, command: Command) {
        let mut output = ctx.run_ui(egui::RawInput::default(), |ui| self.run(command, ui));
        output.textures_delta.clear();
    }
}
