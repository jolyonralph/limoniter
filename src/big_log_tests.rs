//! Trials on real multi-gigabyte logs, run by hand because they take minutes and gigabytes of disk:
//!     cargo test --release big_log -- --ignored --nocapture --test-threads=1
//! Each prints how long each step took and how much memory the program held. LIMONITER_BIG_LOG_GB sets the size.

use super::*;
use std::io::{BufRead, Write};
use std::time::Instant;

#[repr(C)]
#[derive(Default)]
struct MemoryCounters {
    size: u32,
    page_faults: u32,
    peak_working_set: usize,
    working_set: usize,
    peak_paged_pool: usize,
    paged_pool: usize,
    peak_nonpaged_pool: usize,
    nonpaged_pool: usize,
    private_bytes: usize,
    peak_private_bytes: usize,
}

#[link(name = "kernel32")]
unsafe extern "system" {
    fn GetCurrentProcess() -> isize;
    fn K32GetProcessMemoryInfo(process: isize, counters: *mut MemoryCounters, size: u32) -> i32;
}

fn memory() -> MemoryCounters {
    let mut counters = MemoryCounters { size: std::mem::size_of::<MemoryCounters>() as u32, ..MemoryCounters::default() };
    // SAFETY: the structure is the size it says it is, and is only written during this call.
    unsafe {
        K32GetProcessMemoryInfo(GetCurrentProcess(), &raw mut counters, counters.size);
    }
    counters
}

struct Trial {
    began: Instant,
    worst_frame: Duration,
    /// The most any step may hold, in MB.
    largest_mb: usize,
}

impl Trial {
    fn new() -> Self {
        Self { began: Instant::now(), worst_frame: Duration::ZERO, largest_mb: 0 }
    }

    fn report(&mut self, step: &str) {
        let now = memory();
        let held = now.private_bytes / (1024 * 1024);
        self.largest_mb = self.largest_mb.max(now.peak_private_bytes / (1024 * 1024));
        println!("{step:<46} {:>9.2?}  worst frame {:>9.2?}  holds {held:>6} MB  peak {:>6} MB", self.began.elapsed(), self.worst_frame, now.peak_private_bytes / (1024 * 1024));
        self.began = Instant::now();
        self.worst_frame = Duration::ZERO;
    }

    fn frame(&mut self, app: &mut Limoniter, ctx: &Context, events: Vec<egui::Event>) {
        let raw = egui::RawInput { screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1600.0, 900.0))), events, ..Default::default() };
        let start = Instant::now();
        let mut output = ctx.run_ui(raw, |ui| app.draw_frame(ui));
        output.textures_delta.clear();
        self.worst_frame = self.worst_frame.max(start.elapsed());
    }

    fn command(&mut self, app: &mut Limoniter, ctx: &Context, command: Command) {
        let raw = egui::RawInput { screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1600.0, 900.0))), ..Default::default() };
        let mut output = ctx.run_ui(raw, |ui| app.run(command, ui));
        output.textures_delta.clear();
    }

    fn settle(&mut self, app: &mut Limoniter, ctx: &Context) {
        for _ in 0..3 {
            self.frame(app, ctx, Vec::new());
        }
    }

    fn wait(&mut self, app: &mut Limoniter, ctx: &Context, busy: impl Fn(&Limoniter) -> bool) {
        while busy(app) {
            self.frame(app, ctx, Vec::new());
            std::thread::sleep(Duration::from_millis(5));
        }
    }
}

fn log_line(number: usize) -> String {
    let status = if number.is_multiple_of(50) { 500 } else { 200 };
    let mark = if number.is_multiple_of(1_000_000) { " NEEDLE" } else { "" };
    format!("2026-11-04 12:{:02}:{:02}.{:03} INFO  [worker-{:02}] request {number} handled in {} ms from 10.0.{}.{} status={status}{mark}", number / 60 % 60, number % 60, number % 1000, number % 16, number % 977, number % 255, number % 253)
}

/// Writes a log of about this many gigabytes, and says how many lines it has.
fn write_log(path: &Path, gigabytes: u64) -> usize {
    let mut out = std::io::BufWriter::with_capacity(1 << 20, std::fs::File::create(path).unwrap());
    let (mut written, mut number) = (0_u64, 0);
    while written < gigabytes * 1024 * 1024 * 1024 {
        let line = log_line(number);
        out.write_all(line.as_bytes()).unwrap();
        out.write_all(b"\n").unwrap();
        written += line.len() as u64 + 1;
        number += 1;
    }
    out.flush().unwrap();
    number
}

fn workspace() -> (PathBuf, u64) {
    let dir = std::env::temp_dir().join("limoniter-big-log");
    std::fs::create_dir_all(&dir).unwrap();
    let gigabytes = std::env::var("LIMONITER_BIG_LOG_GB").ok().and_then(|text| text.parse().ok()).unwrap_or(6);
    (dir, gigabytes)
}

fn app_on(path: &Path, settings: &str, trial: &mut Trial) -> (Limoniter, Context) {
    let mut app = Limoniter::build(settings.to_string(), None, Some(PathBuf::from("never-written.png")));
    let ctx = Context::default();
    assert!(app.open_path(path));
    trial.report("opened: the file is showing");
    trial.settle(&mut app, &ctx);
    trial.report("first frames drawn");
    trial.wait(&mut app, &ctx, |app| app.docs[app.active].scanning.is_some());
    trial.report("all the lines found, window free meanwhile");
    (app, ctx)
}

const EVERY_PANEL: &str = "file_properties=true\njson_manager=true\nxml_manager=true\nascii_table=true\nencoding_preview=true\nfunctions=true\nbookmarks=true\nproject_panel=true\ndocument_map=true\nline_numbers=true\nruler=true\npreview_open=true\n";

#[test]
#[ignore = "needs gigabytes of disk and minutes"]
fn big_log_open_move_edit_find_save() {
    let (dir, gigabytes) = workspace();
    let path = dir.join("big.log");
    let mut trial = Trial::new();
    let lines = write_log(&path, gigabytes);
    println!("{gigabytes} GB, {lines} lines");
    trial.report("wrote the test log");
    let (mut app, ctx) = app_on(&path, EVERY_PANEL, &mut trial);
    app.docs[0].preview_view = settings::PreviewView::Split;
    assert_eq!(app.docs[0].text.line_count(), lines + 1);
    assert!(app.docs[0].text.is_disk_backed());

    let key_event = |key: Key, modifiers: Modifiers| egui::Event::Key { key, physical_key: None, pressed: true, repeat: false, modifiers: if modifiers.ctrl { modifiers | Modifiers::COMMAND } else { modifiers } };
    trial.frame(&mut app, &ctx, vec![key_event(Key::End, Modifiers::CTRL)]);
    trial.settle(&mut app, &ctx);
    assert_eq!(app.docs[0].text.caret_line, lines);
    trial.report("Ctrl+End");
    app.doc().text.go_to(&(lines / 2).to_string());
    trial.settle(&mut app, &ctx);
    trial.report("go to the middle");
    for _ in 0..200 {
        trial.frame(&mut app, &ctx, vec![key_event(Key::PageDown, Modifiers::NONE)]);
    }
    trial.report("200 Page Downs");
    for (word_wrap, map) in [(true, true), (false, false), (false, true)] {
        app.view.word_wrap = word_wrap;
        app.view.document_map = map;
        trial.settle(&mut app, &ctx);
    }
    trial.report("word wrap and map, on and off");

    let (first, middle, last) = (0, lines / 2, lines - 1);
    app.doc().text.move_caret(first, 0, false);
    app.doc().text.type_text("first ");
    app.doc().text.move_caret(middle, 0, false);
    app.doc().text.type_text("middle ");
    app.doc().text.move_caret(last, 0, false);
    app.doc().text.type_text("last ");
    trial.settle(&mut app, &ctx);
    assert_eq!(app.docs[0].text.store().memory_lines(), 3, "only the edited lines are in memory");
    trial.report("edit the first, middle and last lines");

    for _ in 0..500 {
        trial.frame(&mut app, &ctx, vec![egui::Event::Text("x".into())]);
    }
    trial.report("type 500 characters, a frame each");
    // Typing close together undoes in steps, so it goes back until they've gone.
    while app.docs[0].text.line(last).starts_with("last x") {
        trial.command(&mut app, &ctx, Command::Undo);
        trial.frame(&mut app, &ctx, Vec::new());
    }
    trial.report("undo them all, a frame each");
    assert!(app.docs[0].text.line(last).starts_with("last "));

    app.doc().text.move_caret(middle, 0, false);
    let block: String = "pasted line\n".repeat(100_000);
    app.doc().text.paste_text(&block, false);
    trial.settle(&mut app, &ctx);
    trial.report("paste 100,000 lines in the middle");
    trial.command(&mut app, &ctx, Command::Undo);
    trial.settle(&mut app, &ctx);
    assert_eq!(app.docs[0].text.line_count(), lines + 1);
    trial.report("undo that");

    app.doc().text.select_all();
    trial.settle(&mut app, &ctx);
    trial.report("select all, and draw it");
    app.doc().text.delete_selection();
    trial.settle(&mut app, &ctx);
    assert_eq!(app.docs[0].text.line_count(), 1);
    trial.report("delete everything");
    trial.command(&mut app, &ctx, Command::Undo);
    trial.settle(&mut app, &ctx);
    assert_eq!(app.docs[0].text.line_count(), lines + 1);
    trial.report("undo that");

    let began = app.docs[0].snapshot();
    drop(began);
    trial.report("take a crash recovery copy");

    app.find.find = "NEEDLE".into();
    app.find.match_case = true;
    app.doc().text.move_caret(1, 0, false);
    app.run_find(false);
    trial.wait(&mut app, &ctx, |app| app.pending.is_some());
    assert_eq!(app.docs[0].text.caret_line, 1_000_000);
    trial.report("find the next match, 1,000,000 lines on");
    app.find.find = "NOT ANYWHERE".into();
    app.run_find(false);
    trial.wait(&mut app, &ctx, |app| app.pending.is_some());
    assert!(!app.find.status.is_empty());
    trial.report("search the whole file for what isn't there");

    app.find.find = "NEEDLE".into();
    app.list_matching_lines();
    let job = app.job.clone().unwrap();
    trial.wait(&mut app, &ctx, |_| !job.finished.load(Ordering::SeqCst));
    assert!(job.hits.lock().unwrap().len() >= lines / 1_000_000);
    trial.report("list the matching lines");

    app.find.replace = "PIN".into();
    let matches = lines / 1_000_000 + 1;
    app.doc().text.move_caret(0, 0, false);
    app.run_replace_all();
    trial.wait(&mut app, &ctx, |app| app.pending.is_some());
    trial.settle(&mut app, &ctx);
    assert!(app.find.status.contains(&matches.to_string()), "{}", app.find.status);
    trial.report("replace all, a few dozen matches");

    assert!(app.write_to(0, &path));
    trial.wait(&mut app, &ctx, |app| app.docs[0].saving.is_some());
    trial.report("save over the same file, window free meanwhile");
    trial.wait(&mut app, &ctx, |app| app.docs[0].scanning.is_some());
    trial.report("read its lines again");
    assert!(!app.docs[0].is_modified());
    assert_eq!(app.docs[0].text.store().memory_lines(), 0, "after saving, nothing's held but what's on screen");

    let reader = std::io::BufReader::with_capacity(1 << 20, std::fs::File::open(&path).unwrap());
    let mut checked = 0;
    for (number, line) in reader.lines().enumerate() {
        let line = line.unwrap();
        let mut wanted = log_line(number).replace("NEEDLE", "PIN");
        if number == first {
            wanted.insert_str(0, "first ");
        } else if number == middle {
            wanted.insert_str(0, "middle ");
        } else if number == last {
            wanted.insert_str(0, "last ");
        }
        assert_eq!(line, wanted, "line {number}");
        checked += 1;
    }
    assert_eq!(checked, lines);
    trial.report("every line of the saved file is exactly right");
    println!("the most held at any point: {} MB", trial.largest_mb);
    drop(app);
    std::fs::remove_dir_all(&dir).unwrap();
}

/// A log with a few very long lines, as a program dumping JSON or a stack can leave.
#[test]
#[ignore = "needs gigabytes of disk and minutes"]
fn big_log_with_very_long_lines() {
    let (dir, _) = workspace();
    let path = dir.join("long.log");
    let megabytes: usize = std::env::var("LIMONITER_LONG_LINE_MB").ok().and_then(|text| text.parse().ok()).unwrap_or(200);
    let mut trial = Trial::new();
    {
        let mut out = std::io::BufWriter::new(std::fs::File::create(&path).unwrap());
        for number in 0..1000 {
            writeln!(out, "{}", log_line(number)).unwrap();
        }
        let piece = "abcdefghij klmnopqrs ".repeat(50);
        for _ in 0..(megabytes * 1024 * 1024 / piece.len()) {
            out.write_all(piece.as_bytes()).unwrap();
        }
        out.write_all(b"\n").unwrap();
        for number in 1000..2_000_000 {
            writeln!(out, "{}", log_line(number)).unwrap();
        }
        out.flush().unwrap();
    }
    trial.report(&format!("wrote the log, with a {megabytes} MB line"));
    let (mut app, ctx) = app_on(&path, "", &mut trial);
    app.doc().text.go_to("990");
    trial.settle(&mut app, &ctx);
    trial.report("go to just above the long line");
    app.doc().text.go_to("1001");
    trial.settle(&mut app, &ctx);
    trial.report("go to the long line");
    let key_event = |key: Key, modifiers: Modifiers| egui::Event::Key { key, physical_key: None, pressed: true, repeat: false, modifiers: if modifiers.ctrl { modifiers | Modifiers::COMMAND } else { modifiers } };
    trial.frame(&mut app, &ctx, vec![key_event(Key::End, Modifiers::NONE)]);
    trial.settle(&mut app, &ctx);
    trial.report("to the end of it");
    trial.frame(&mut app, &ctx, vec![egui::Event::Text("x".into())]);
    trial.settle(&mut app, &ctx);
    trial.report("type a character at the end of it");
    trial.command(&mut app, &ctx, Command::Undo);
    app.doc().text.go_to("5000");
    trial.settle(&mut app, &ctx);
    trial.report("go on past it");
    println!("the most held at any point: {} MB", trial.largest_mb);
    drop(app);
    std::fs::remove_dir_all(&dir).unwrap();
}

/// Replace in Files on a log too big to read in, with its backup.
#[test]
#[ignore = "needs gigabytes of disk and minutes"]
fn big_log_replace_in_files() {
    let (dir, gigabytes) = workspace();
    let path = dir.join("files.log");
    let mut trial = Trial::new();
    let lines = write_log(&path, gigabytes);
    trial.report("wrote the test log");
    let options = SearchOptions { pattern: "status=500".into(), replacement: "status=501".into(), match_case: true, ..SearchOptions::default() };
    let (count, backed_up) = find_files::replace_in_file(&path, &options, true).unwrap();
    assert!(backed_up);
    assert_eq!(count, lines.div_ceil(50));
    trial.report("replace in a file on disk, on one line in fifty");
    let reader = std::io::BufReader::with_capacity(1 << 20, std::fs::File::open(&path).unwrap());
    let mut checked = 0;
    for (number, line) in reader.lines().enumerate() {
        assert_eq!(line.unwrap(), log_line(number).replace("status=500", "status=501"), "line {number}");
        checked += 1;
    }
    assert_eq!(checked, lines);
    let backup = std::io::BufReader::with_capacity(1 << 20, std::fs::File::open(dir.join("files.log.bak")).unwrap());
    assert_eq!(backup.lines().enumerate().map(|(number, line)| assert_eq!(line.unwrap(), log_line(number))).count(), lines);
    trial.report("every line is exactly right, and so is the backup");
    println!("the most held at any point: {} MB", trial.largest_mb);
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
#[ignore = "needs gigabytes of disk and minutes"]
fn big_log_replace_on_a_great_many_lines() {
    let (dir, gigabytes) = workspace();
    let path = dir.join("many.log");
    let mut trial = Trial::new();
    let lines = write_log(&path, gigabytes);
    trial.report("wrote the test log");
    let (mut app, ctx) = app_on(&path, "", &mut trial);
    app.find.find = "status=500".into();
    app.find.replace = "status=501".into();
    app.find.match_case = true;
    app.run_replace_all();
    trial.wait(&mut app, &ctx, |app| app.pending.is_some());
    trial.settle(&mut app, &ctx);
    println!("{}", app.find.status);
    trial.report("replace all, on one line in fifty");
    trial.frame(&mut app, &ctx, vec![egui::Event::Text("x".into())]);
    trial.report("type one character after that");
    trial.command(&mut app, &ctx, Command::Undo);
    trial.command(&mut app, &ctx, Command::Undo);
    trial.settle(&mut app, &ctx);
    assert!(app.docs[0].text.line(50).contains("status=500"));
    trial.report("undo the replace");
    trial.command(&mut app, &ctx, Command::Redo);
    trial.settle(&mut app, &ctx);
    trial.report("redo the replace");
    assert!(app.write_to(0, &path));
    trial.wait(&mut app, &ctx, |app| app.docs[0].saving.is_some() || app.docs[0].scanning.is_some());
    trial.report("save and read again");
    let reader = std::io::BufReader::with_capacity(1 << 20, std::fs::File::open(&path).unwrap());
    let mut checked = 0;
    for (number, line) in reader.lines().enumerate() {
        assert_eq!(line.unwrap(), log_line(number).replace("status=500", "status=501"), "line {number}");
        checked += 1;
    }
    assert_eq!(checked, lines);
    trial.report("every line of the saved file is exactly right");
    println!("the most held at any point: {} MB", trial.largest_mb);
    drop(app);
    std::fs::remove_dir_all(&dir).unwrap();
}
