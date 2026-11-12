//! Throws long random runs of commands at the editors, looking for anything that panics.
//! Every run is repeatable from its seed.

use crate::charset::{self, raw_char};
use crate::codec::{self, Eol, Lossy, NewlinePolicy, TextFormat};
use crate::hex_buffer::HexBuffer;
use crate::line_store::{LineStore, Source};
use crate::text_buffer::{self, SearchOptions, TextBuffer};
use crate::views::{self, ClipState, ViewOptions, ViewState};
use eframe::egui;
use std::sync::Arc;
use std::sync::atomic::AtomicU64;

struct Dice(u64);

impl Dice {
    fn roll(&mut self, limit: usize) -> usize {
        self.0 = self.0.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        ((self.0 >> 33) as usize) % limit.max(1)
    }

    fn text(&mut self) -> String {
        let pieces = ["a", "word ", " ", "\t", "\u{e9}", "\u{4e2d}", "\u{1F600}", "(", ")", "{", "}", "0", "42", "e\u{301}", "\u{1B}", "\u{85}", "line one\r\nline two", "\n", "\r", "x\ny\nz"];
        let count = 1 + self.roll(4);
        let mut out = String::new();
        for _ in 0..count {
            let pick = self.roll(pieces.len() + 2);
            if pick >= pieces.len() {
                out.push(raw_char(self.roll(256) as u8));
            } else {
                out.push_str(pieces[pick]);
            }
        }
        out
    }
}

/// One random command. Returns false for those that can't be undone back to where they started.
fn run_command(b: &mut TextBuffer, dice: &mut Dice, on_disk: bool) {
    let lines = b.line_count();
    let line = dice.roll(lines + 2);
    let col = dice.roll(60);
    match dice.roll(46) {
        0..=5 => {
            let text = dice.text();
            b.type_text(&text);
        }
        6 => b.new_line(),
        7 => b.backspace(),
        8 => b.delete_forward(),
        9 => b.move_caret(line, col, false),
        10 => b.move_caret(line, col, true),
        11 => b.move_horizontal(dice.roll(2) == 0, dice.roll(2) == 0, dice.roll(2) == 0),
        12 => b.move_vertical(dice.roll(11) as isize - 5, dice.roll(2) == 0),
        13 => b.move_home(dice.roll(2) == 0),
        14 => b.move_end(dice.roll(2) == 0),
        15 => b.move_document_start(dice.roll(2) == 0),
        16 => b.move_document_end(dice.roll(2) == 0),
        17 => b.select_word_at_caret(),
        18 => b.select_all(),
        19 => b.press_for_block(line, col, dice.roll(2) == 0),
        20 => {
            let on = !b.column_mode();
            b.set_column_mode(on);
        }
        21 => b.end_temporary_block(),
        22 => {
            let text = dice.text();
            b.paste_text(&text, dice.roll(2) == 0);
        }
        23 => match dice.roll(13) {
            10 | 11 => {
                if let Some((top, bottom)) = b.rows_for_convert() {
                    let rows = b.rows_text(top, bottom);
                    let done = if dice.roll(2) == 0 { crate::reformat::wrap_to_line_ends(&rows, 5 + dice.roll(30)) } else { crate::reformat::line_ends_to_wrap(&rows) };
                    b.replace_lines(top, bottom, done);
                }
            }
            0 => {
                let _ = b.make_selections();
            }
            1 => b.add_cursor(line, col),
            2 => b.multi_move([crate::text_buffer::SpotMove::Left, crate::text_buffer::SpotMove::Right, crate::text_buffer::SpotMove::Up, crate::text_buffer::SpotMove::Down, crate::text_buffer::SpotMove::Home, crate::text_buffer::SpotMove::End][dice.roll(6)], dice.roll(2) == 0),
            3 => {
                b.multi_text();
                b.multi_on_line(line);
            }
            4 => {
                b.toggle_fold(line.min(b.line_count() - 1));
            }
            5 if !on_disk => b.collapse_all(),
            6 => b.expand_all(),
            7 => {
                b.hide_selected_lines();
            }
            8 => b.show_selected_lines(),
            9 => {
                b.reveal_line(line);
                assert!(b.unfolded_lines().iter().all(|shown| *shown < b.line_count()));
            }
            _ => b.delete_selection(),
        },
        24 => b.change_case([crate::text_buffer::CaseChange::Upper, crate::text_buffer::CaseChange::Lower, crate::text_buffer::CaseChange::Sentence, crate::text_buffer::CaseChange::Title][dice.roll(4)]),
        25 => b.move_lines(dice.roll(2) == 0),
        26 => b.duplicate_lines(),
        27 => b.join_lines(),
        28 => b.sort_lines(dice.roll(2) == 0),
        29 => b.trim_leading(),
        30 => b.trim_trailing(),
        31 => b.tabs_to_spaces(),
        32 => b.spaces_to_tabs(),
        33 => b.tab_key(dice.roll(2) == 0),
        34 => {
            let text = dice.text().replace(['\r', '\n'], "");
            match dice.roll(5) {
                0 => b.insert_column_fill(&text, dice.roll(2) == 0),
                1 => {
                    // Every way of asking, including far more columns than there is text.
                    let count = [0, 1, 3, 500, usize::MAX][dice.roll(5)];
                    if b.select_columns(count).is_ok() {
                        b.selection_text();
                        b.remove_selected_columns();
                    }
                }
                2 => b.justify_columns([crate::text_buffer::Justify::Left, crate::text_buffer::Justify::Center, crate::text_buffer::Justify::Right][dice.roll(3)]),
                3 => {
                    if let Some((top, bottom)) = b.rows_for_convert()
                        && let Ok(rows) = crate::columns::to_fixed(&b.rows_text(top, bottom), crate::columns::DELIMITERS[dice.roll(4)].1)
                    {
                        b.replace_rows(top, rows);
                    }
                }
                _ => {
                    if let Some((top, bottom)) = b.rows_for_convert() {
                        let rows = b.rows_text(top, bottom);
                        let starts = crate::columns::detect_starts(&rows);
                        b.replace_rows(top, crate::columns::to_csv(&rows, &starts, crate::columns::DELIMITERS[dice.roll(4)].1));
                    }
                }
            }
        }
        35 => {
            b.insert_column_numbers(dice.roll(100) as i64, dice.roll(5) as i64 - 2);
        }
        36 => {
            b.sum_column();
            b.selection_text();
            b.position_text();
            b.selection_for_preview();
            b.char_at_caret();
            b.state_at_caret();
            b.eol_counts();
        }
        37 => {
            b.go_to_brace(dice.roll(2) == 0);
        }
        38 => {
            b.go_to(&dice.roll(lines + 5).to_string());
        }
        39 => {
            let options = SearchOptions { pattern: ["a", "\u{e9}", "line", "(", "^", "."][dice.roll(6)].to_string(), replacement: dice.text().replace(['\r', '\n'], ""), match_case: dice.roll(2) == 0, whole_word: dice.roll(4) == 0, regex: dice.roll(3) == 0, hex_bytes: false, backward: dice.roll(2) == 0 };
            let _ = b.find(&options);
            let _ = b.replace_current(&options);
            if !on_disk {
                let _ = b.replace_all(&options);
            }
        }
        40 => b.undo(),
        41 => b.redo(),
        42 => {
            b.toggle_bookmark(line.min(lines - 1));
            b.go_to_bookmark(dice.roll(2) == 0);
        }
        43 => b.insert_exact(&raw_char(dice.roll(256) as u8).to_string()),
        44 => {
            b.convert_line_endings([Eol::CrLf, Eol::Lf, Eol::Cr][dice.roll(3)]);
        }
        _ => {
            b.overwrite = !b.overwrite;
            b.set_insert_eol([Eol::CrLf, Eol::Lf, Eol::Cr][dice.roll(3)]);
        }
    }
}

/// Draws the editor with no window, so the painting and input code run on every odd state too.
fn draw(ctx: &egui::Context, time: &mut f64, b: &mut TextBuffer, state: &mut ViewState, clip: &mut ClipState, options: ViewOptions) {
    *time += 1.0 / 60.0;
    let raw = egui::RawInput { screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(900.0, 500.0))), time: Some(*time), predicted_dt: 1.0 / 60.0, ..Default::default() };
    let mut output = ctx.run_ui(raw, |ui| {
        egui::CentralPanel::default().frame(egui::Frame::NONE).show(ui, |ui| {
            views::text_view(ui, b, state, clip, options, true);
        });
    });
    output.textures_delta.clear();
}

fn buffer_with(data: &[u8], format: TextFormat) -> TextBuffer {
    let mut b = TextBuffer::default();
    b.format = format;
    text_buffer::set_cell_profile(b.format.charset);
    let decoded = codec::decode(data, &mut b.format);
    b.set_decoded(decoded);
    b
}

fn check(b: &TextBuffer, seed: u64, step: usize) {
    assert!(b.line_count() >= 1, "seed {seed} step {step}: no lines");
    assert!(b.caret_line < b.line_count(), "seed {seed} step {step}: caret on line {} of {}", b.caret_line, b.line_count());
    assert!(b.anchor_line < b.line_count(), "seed {seed} step {step}: anchor on line {} of {}", b.anchor_line, b.line_count());
}

#[test]
fn random_commands_never_panic_and_undo_gets_every_byte_back() {
    let sample = "first line\r\n\tindented (brackets) {more}\nthird caf\u{e9} \u{4e2d}\rfourth 12 34\r\n\r\nlast".as_bytes().to_vec();
    let formats: Vec<TextFormat> = ["utf-8", "cp437", "windows-1252", "utf-16le", "petscii-c64", "amiga-text", "atascii"]
        .iter()
        .map(|id| TextFormat { charset: charset::known(id), ..TextFormat::default() })
        .chain([codec::preset_by_id("teletext").unwrap().format(), codec::preset_by_id("atari-8bit").unwrap().format(), codec::preset_by_id("commodore").unwrap().format()])
        .collect();
    let ctx = egui::Context::default();
    let mut time = 0.0;
    for seed in 0..60u64 {
        let format = formats[seed as usize % formats.len()].clone();
        let mut b = buffer_with(&sample, format);
        let original = b.encode(&b.format, Lossy::Omit).unwrap();
        let mut dice = Dice(seed.wrapping_mul(0x9E37_79B9) + 1);
        let (mut state, mut clip) = (ViewState::default(), ClipState::default());
        let options = ViewOptions { word_wrap: seed % 3 == 0, highlight_selected: seed % 2 == 1, whitespace: seed % 2 == 0, line_endings: seed % 3 == 0, active_col: seed % 5 == 0, rainbow_parentheses: seed % 4 == 0, source_view: seed % 7 != 0, ..ViewOptions::default() };
        text_buffer::set_source_view(options.source_view);
        // Few enough that the undo list never has to forget the start.
        for step in 0..120 {
            run_command(&mut b, &mut dice, false);
            check(&b, seed, step);
            let _ = b.encode(&b.format, Lossy::Refuse);
            if step % 10 == 0 {
                draw(&ctx, &mut time, &mut b, &mut state, &mut clip, options);
            }
        }
        draw(&ctx, &mut time, &mut b, &mut state, &mut clip, options);
        let mut guard = 0;
        while b.can_undo() && guard < 400 {
            b.undo();
            check(&b, seed, 1000 + guard);
            guard += 1;
        }
        assert_eq!(b.encode(&b.format, Lossy::Omit).unwrap(), original, "seed {seed}: undoing everything gets the file back");
        assert!(!b.is_modified(), "seed {seed}: and it's marked unmodified");
    }
    text_buffer::set_source_view(true);
    text_buffer::set_cell_profile(charset::utf8());
}

fn open_finished(path: &std::path::Path) -> Arc<Source> {
    let source = Source::open_as(path, None).unwrap();
    while !source.is_finished() {
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
    source
}

/// A file kept on disk has a different way of saving, so both ways are compared after every run.
#[test]
fn random_commands_on_a_file_kept_on_disk_save_the_same_bytes_both_ways() {
    let dir = std::env::temp_dir().join(format!("limoniter-stress-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("big.txt");
    let mut data = Vec::new();
    for i in 0..1500 {
        data.extend_from_slice(format!("line {i} (with text)").as_bytes());
        data.extend_from_slice(match i % 7 {
            0 => b"\n",
            3 => b"\r",
            _ => b"\r\n",
        });
    }
    std::fs::write(&path, &data).unwrap();
    text_buffer::set_source_view(true);
    text_buffer::set_cell_profile(charset::utf8());
    for seed in 0..25u64 {
        let source = open_finished(&path);
        let mut b = TextBuffer::default();
        b.format = source.format();
        b.set_source(source);
        let mut dice = Dice(seed + 77);
        for step in 0..150 {
            run_command(&mut b, &mut dice, true);
            check(&b, seed, step);
        }
        let format = TextFormat { charset: charset::utf8(), ..b.format.clone() };
        let in_memory = b.encode(&format, Lossy::Omit).unwrap();
        let out = dir.join("out.txt");
        b.store().write_file(&out, &format, &AtomicU64::new(0)).unwrap();
        assert_eq!(std::fs::read(&out).unwrap(), in_memory, "seed {seed}: streamed save and whole save differ");
    }
    // The recovery image of a store with disk pieces rebuilds the same document.
    let source = open_finished(&path);
    let mut b = TextBuffer::default();
    b.format = source.format();
    b.set_source(source.clone());
    b.move_caret(700, 3, false);
    b.type_text("EDIT");
    b.move_caret(10, 0, false);
    b.new_line();
    let image = b.store().image();
    let again = LineStore::from_image(image, Some(open_finished(&path))).unwrap();
    let format = TextFormat { charset: charset::utf8(), ..b.format.clone() };
    assert_eq!(again.encode(&format, Lossy::Omit).unwrap(), b.encode(&format, Lossy::Omit).unwrap());

    // Saving over the file, then editing again: a copy refers to the file as saved.
    b.store().clone().write_file(&path, &format, &AtomicU64::new(0)).unwrap();
    let saved = std::fs::read(&path).unwrap();
    b.rebase_on_source(open_finished(&path));
    b.refresh_from_source();
    assert_eq!(b.encode(&format, Lossy::Omit).unwrap(), saved);
    b.move_caret(5, 0, false);
    b.type_text("AFTER");
    let image = b.store().image();
    assert_eq!(image.source.as_ref().map(|s| s.len), Some(saved.len() as u64));
    let again = LineStore::from_image(image, Some(open_finished(&path))).unwrap();
    assert_eq!(again.encode(&format, Lossy::Omit).unwrap(), b.encode(&format, Lossy::Omit).unwrap());
    b.undo();
    assert_eq!(b.encode(&format, Lossy::Omit).unwrap(), saved, "undo still works after saving over the file");
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn random_commands_on_the_hex_editor_never_panic() {
    for seed in 0..80u64 {
        let mut dice = Dice(seed + 5);
        let mut h = HexBuffer::default();
        let original: Vec<u8> = (0..dice.roll(200)).map(|i| i as u8).collect();
        h.load(original.clone());
        for _ in 0..200 {
            let len = h.len();
            match dice.roll(14) {
                0 => h.type_hex_digit(dice.roll(16) as u8),
                1 => h.type_byte(dice.roll(256) as u8),
                2 => h.delete_selection(),
                3 => h.delete_forward(),
                4 => h.backspace(),
                5 => h.move_caret(dice.roll(len + 3), dice.roll(2) == 0),
                6 => h.move_by(dice.roll(41) as isize - 20, dice.roll(2) == 0),
                7 => h.move_left(dice.roll(2) == 0),
                8 => h.select_all(),
                9 => h.undo(),
                10 => h.redo(),
                11 => h.paste_text(&dice.text()),
                12 => h.insert_bytes(&[1, 2, 3]),
                _ => h.insert_mode = !h.insert_mode,
            }
            let _ = h.selection_text();
            let _ = h.position_text();
            assert!(h.caret <= h.len(), "seed {seed}");
        }
    }
}

#[test]
fn random_bytes_survive_every_set_and_policy() {
    let mut dice = Dice(4242);
    for _ in 0..400 {
        let data: Vec<u8> = (0..dice.roll(300)).map(|_| dice.roll(256) as u8).collect();
        let set = charset::all()[dice.roll(charset::all().len())];
        let policy = [NewlinePolicy::Standard, NewlinePolicy::CrOnly, NewlinePolicy::AtasciiEol, NewlinePolicy::FixedRows { width: 40, parity: false }][dice.roll(4)];
        let mut format = TextFormat { charset: set, newline: policy, ..TextFormat::default() };
        let decoded = codec::decode(&data, &mut format);
        let eol_of = |i: usize| decoded.exceptions.get(&i).copied().unwrap_or(decoded.base);
        assert_eq!(codec::encode(&decoded.lines, &eol_of, &format).unwrap(), data, "{} {policy:?}", set.id);
    }
}
