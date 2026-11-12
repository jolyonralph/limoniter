//! The whole-file commands on files kept on disk, as files of 64 MB and more are. A small file is made to stay on
//! disk the same way, so each can be checked quickly.

use super::tests::{app_with, draw};
use super::*;

/// Opens a file and has it kept on disk, as a large file is, and waits for its lines to be found.
fn open_on_disk(app: &mut Limoniter, ctx: &Context, name: &str, bytes: &[u8]) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("limoniter-large-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join(name);
    std::fs::write(&path, bytes).unwrap();
    assert!(app.open_path(&path));
    let source = Source::open(&path).unwrap();
    let doc = app.doc();
    doc.text.set_source(source.clone());
    doc.scanning = Some(source);
    settle(app, ctx);
    assert!(app.docs[app.active].text.is_disk_backed());
    path
}

/// Draws frames until background work on the file in front has finished.
fn settle(app: &mut Limoniter, ctx: &Context) {
    let began = std::time::Instant::now();
    loop {
        draw(app, ctx, Vec::new());
        let doc = &app.docs[app.active];
        if app.big_job.is_none() && doc.scanning.is_none() && !doc.rewriting && doc.saving.is_none() {
            break;
        }
        assert!(began.elapsed() < Duration::from_secs(30), "the work never finished");
        std::thread::sleep(Duration::from_millis(5));
    }
}

fn lines(app: &Limoniter) -> Vec<String> {
    let text = &app.docs[app.active].text;
    (0..text.line_count()).map(|i| text.line(i).to_string()).collect()
}

#[test]
fn a_large_file_is_converted_to_another_encoding_in_place() {
    let (mut app, ctx) = app_with("");
    let path = open_on_disk(&mut app, &ctx, "words.txt", "h\u{e9}llo\nw\u{f6}rld\nlast".as_bytes());
    let windows = TextFormat { charset: charset::known("windows-1252"), ..app.docs[app.active].text.format.clone() };
    app.convert_to(app.active, windows);
    assert!(app.docs[app.active].rewriting, "it's busy while every line is written");
    settle(&mut app, &ctx);
    let doc = &app.docs[app.active];
    assert_eq!(lines(&app), ["h\u{e9}llo", "w\u{f6}rld", "last"]);
    assert_eq!(doc.text.format.charset.id, "windows-1252");
    assert!(doc.text.is_disk_backed() && doc.is_modified());
    // Saved, the file is in the new encoding.
    assert!(app.save(app.active));
    settle(&mut app, &ctx);
    assert_eq!(std::fs::read(&path).unwrap(), b"h\xe9llo\nw\xf6rld\nlast");
    // A character the encoding can't hold stops it, and nothing changes.
    let ascii = TextFormat { charset: charset::known("us-ascii"), ..app.docs[app.active].text.format.clone() };
    app.convert_to(app.active, ascii);
    settle(&mut app, &ctx);
    assert!(matches!(app.dialog, Dialog::Lossy(..)), "the characters that won't fit are listed");
    assert_eq!(app.docs[app.active].text.format.charset.id, "windows-1252");
    let _ = std::fs::remove_dir_all(path.parent().unwrap());
}

#[test]
fn a_large_files_line_endings_are_converted_and_undone() {
    let (mut app, ctx) = app_with("");
    let path = open_on_disk(&mut app, &ctx, "lines.txt", b"one\ntwo\r\nthree\n");
    app.run_menu_action(MenuAction::ConvertEols(codec::Eol::CrLf));
    settle(&mut app, &ctx);
    assert_eq!(lines(&app), ["one", "two", "three", ""]);
    assert!((0..3).all(|i| app.docs[app.active].text.terminator(i) == codec::Eol::CrLf));
    // One Undo goes back to the lines as they were.
    app.doc().text.undo();
    assert_eq!(app.docs[app.active].text.terminator(0), codec::Eol::Lf);
    assert!(!app.docs[app.active].is_modified());
    app.doc().text.redo();
    assert!(app.save(app.active));
    settle(&mut app, &ctx);
    assert_eq!(std::fs::read(&path).unwrap(), b"one\r\ntwo\r\nthree\r\n");
    let _ = std::fs::remove_dir_all(path.parent().unwrap());
}

#[test]
fn a_large_file_is_reindented_from_the_top() {
    let (mut app, ctx) = app_with("");
    let path = open_on_disk(&mut app, &ctx, "code.c", b"int main() {\nif (x) {\n      y();\n  }\n}\n");
    app.reindent_lines();
    settle(&mut app, &ctx);
    let unit = app.indent_unit();
    assert_eq!(lines(&app), ["int main() {".to_string(), format!("{unit}if (x) {{"), format!("{unit}{unit}y();"), format!("{unit}}}"), "}".to_string(), String::new()]);
    app.doc().text.undo();
    assert_eq!(lines(&app)[2], "      y();");
    let _ = std::fs::remove_dir_all(path.parent().unwrap());
}

#[test]
fn a_large_file_is_filtered_to_the_lines_with_the_selection() {
    let (mut app, ctx) = app_with("");
    let path = open_on_disk(&mut app, &ctx, "log.txt", b"start\nerror one\nfine\nerror two\nend\n");
    app.doc().text.select_chars((1, 0), (1, 5));
    app.toggle_filter();
    settle(&mut app, &ctx);
    let doc = &app.docs[app.active];
    assert_eq!(doc.filter.as_ref().map(|f| f.lines.clone()), Some(vec![1, 3]));
    assert!(doc.text.read_only, "a filtered file is only read");
    app.toggle_filter();
    assert!(app.docs[app.active].filter.is_none() && !app.docs[app.active].text.read_only);
    let _ = std::fs::remove_dir_all(path.parent().unwrap());
}

#[test]
fn lines_are_hidden_in_a_large_file() {
    let (mut app, ctx) = app_with("");
    let path = open_on_disk(&mut app, &ctx, "many.txt", (0..100).map(|i| format!("line {i}\n")).collect::<String>().as_bytes());
    app.doc().text.select_chars((10, 0), (20, 0));
    command_now(&mut app, &ctx, Command::HideLines);
    draw(&mut app, &ctx, Vec::new());
    let doc = &app.docs[app.active];
    assert_eq!(doc.fold_view, [views::Hidden { first: 11, last: 19, shown_before: 11 }], "lines 11 to 19 are hidden under line 10");
    command_now(&mut app, &ctx, Command::ShowLines);
    draw(&mut app, &ctx, Vec::new());
    assert!(!app.docs[app.active].text.has_folds());
    let _ = std::fs::remove_dir_all(path.parent().unwrap());
}

fn command_now(app: &mut Limoniter, ctx: &Context, command: Command) {
    let mut output = ctx.run_ui(egui::RawInput::default(), |ui| app.run(command, ui));
    output.textures_delta.clear();
}
