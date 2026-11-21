use super::super::tests::app_with;
use super::*;

fn frame(app: &mut Limoniter, ctx: &Context) {
    let input = egui::RawInput { screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1400.0, 900.0))), ..Default::default() };
    ctx.run_ui(input, |ui| app.draw_frame(ui)).textures_delta.clear();
}

/// Waits for the panel's search to finish, drawing frames as the window would.
fn settle(app: &mut Limoniter, ctx: &Context) {
    for _ in 0..400 {
        frame(app, ctx);
        if app.search.typed_at.is_none() && app.search.job.as_ref().is_some_and(|j| j.finished.load(Ordering::SeqCst)) && app.replace_job.is_none() && !app.search.again_after_replace {
            return;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    panic!("the search didn't finish: typed {:?} job {:?} replace {} again {} problem {:?} open {}", app.search.typed_at.map(|t| t.elapsed()), app.search.job.as_ref().map(|j| j.finished.load(Ordering::SeqCst)), app.replace_job.is_some(), app.search.again_after_replace, app.search.problem, app.search.open);
}

fn found(app: &Limoniter) -> Vec<(String, usize)> {
    let hits = app.search.job.as_ref().map(|j| j.hits.lock().unwrap().clone()).unwrap_or_default();
    grouped(&hits, &app.search.dismissed, &app.search.exclude).into_iter().map(|(p, h)| (p.file_name().unwrap().to_string_lossy().into_owned(), h.len())).collect()
}

/// In an open folder: found as it's typed, grouped by file; narrowed by name; a file put away is left out of
/// Replace all; the replacement shown is what it'll be; and replacing changes the files and searches again.
#[test]
fn the_search_panel_finds_in_the_folder_and_replaces() {
    let (mut app, ctx) = app_with("");
    let root = std::env::temp_dir().join(format!("limoniter-search-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(root.join("src")).unwrap();
    std::fs::write(root.join("src/a.rs"), "fn count_up() {}\nlet n = count_up();\n").unwrap();
    std::fs::write(root.join("src/b.s"), "count_up:\n\trts\n").unwrap();
    std::fs::write(root.join("notes.txt"), "nothing to see\n").unwrap();
    std::fs::write(root.join("c.rs"), "// count_up here too\n").unwrap();
    app.open_folder(&root);
    if let Some(project) = app.project.as_mut() {
        project.files = crate::workspace::index(&root, &[]).0;
    }
    app.open_search_panel();
    assert!(app.search.open);
    app.search.query = "count_up".into();
    settle(&mut app, &ctx);
    let mut files = found(&app);
    files.sort();
    assert_eq!(files, [("a.rs".to_string(), 2), ("b.s".to_string(), 1), ("c.rs".to_string(), 1)]);
    // Only .rs files.
    app.search.include = "*.rs".into();
    settle(&mut app, &ctx);
    assert_eq!(found(&app).len(), 2);
    app.search.include.clear();
    settle(&mut app, &ctx);
    // A regular expression, its group put in the replacement as it will be.
    app.search.regex = true;
    app.search.query = r"(\w+)_up".into();
    app.search.replace = "${1}_down".into();
    app.search.show_replace = true;
    settle(&mut app, &ctx);
    let hits = app.search.job.as_ref().unwrap().hits.lock().unwrap().clone();
    assert!(hits.iter().all(|h| app.search.replacement(h) == "count_down"), "{:?}", hits.iter().map(|h| app.search.replacement(h)).collect::<Vec<_>>());
    // c.rs put away, the rest replaced.
    app.search.dismissed.insert(std::path::absolute(root.join("c.rs")).unwrap());
    let rest: Vec<PathBuf> = grouped(&hits, &app.search.dismissed, "").into_iter().map(|(p, _)| p).collect();
    assert_eq!(rest.len(), 2, "{rest:?}");
    app.find.keep_backups = false;
    app.replace_plan = Some(ReplacePlan { options: app.search.options(), backup: false });
    app.search.again_after_replace = true;
    app.start_replace_in_files(rest);
    settle(&mut app, &ctx);
    assert_eq!(std::fs::read_to_string(root.join("src/a.rs")).unwrap(), "fn count_down() {}\nlet n = count_down();\n");
    assert_eq!(std::fs::read_to_string(root.join("src/b.s")).unwrap(), "count_down:\n\trts\n");
    assert_eq!(std::fs::read_to_string(root.join("c.rs")).unwrap(), "// count_up here too\n", "put away, left as it was");
    // Searched again afterwards: only what's left.
    assert_eq!(found(&app), [("c.rs".to_string(), 1)]);
    let _ = std::fs::remove_dir_all(&root);
}
