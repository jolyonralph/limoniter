// A window with no console behind it, in the version people actually run.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod amiga_palette_ui;
mod ansi_art;
mod addons;
mod app;
mod platform;
#[cfg(target_os = "macos")]
mod mac_open;
mod remote;
mod deploy;
mod big_jobs;
mod associations;
mod basic;
mod basic_more;
mod char_table;
mod charset;
mod code_format;
mod codec;
mod compare;
mod columns;
mod console;
mod ebcdic;
mod emulators;
mod encoding_prefs;
mod encoding_ui;
#[cfg(test)]
mod fold_multi_tests;
mod find_files;
mod formatters;
mod git;
mod byte_store;
mod hex_buffer;
mod highlight;
mod history_file;
mod instance;
mod large_edits;
mod legacy_docs;
mod line_store;
mod media;
mod navigation;
mod panels;
mod pictures;
mod preview;
mod print;
mod project;
mod reformat;
mod running;
mod speaker;
mod zx_keyboard;
mod zx_machine;
mod machine_link;
mod retro_screens;
mod region_history;
mod recovery;
mod rainbow;
mod screen;
mod settings;
mod spelling;
mod strings_explorer;
mod tabs;
mod table;
mod scripting;
mod text_buffer;
mod text_objects;
#[cfg(test)]
mod stress_tests;
mod theme;
mod tools;
mod undo_tree;
mod views;
mod wrap;

use eframe::egui;

/// Where a failure is noted.
pub fn crash_log() -> std::path::PathBuf {
    std::env::temp_dir().join("limoniter-crash.log")
}

/// Says what happened, and that the edits are safe.
pub fn tell_about_crash() {
    rfd::MessageDialog::new()
        .set_title("Limoniter")
        .set_level(rfd::MessageLevel::Error)
        .set_description(format!(
            "Limoniter ran into a problem and has to close.\n\nYour unsaved changes have been kept, and will be offered back the next time Limoniter starts.\n\nDetails are in {}",
            crash_log().display()
        ))
        .show();
}

/// If the app ever fails, it leaves a note and gets the edits safe, rather than silently vanishing.
/// A background thread failing doesn't stop the window, so that's only noted.
fn report_crashes() {
    std::panic::set_hook(Box::new(|info| {
        use std::io::Write;
        let thread = std::thread::current();
        if let Ok(mut log) = std::fs::OpenOptions::new().create(true).append(true).open(crash_log()) {
            let _ = writeln!(log, "{} thread {:?}: {info}\n{}\n", chrono::Local::now().format("%Y-%m-%d %H:%M:%S"), thread.name(), std::backtrace::Backtrace::force_capture());
        }
        let on_main = thread.name() == Some("main");
        // A failure while drawing is caught by the window, which saves the very latest first.
        if on_main && !recovery::IN_FRAME.load(std::sync::atomic::Ordering::SeqCst) {
            recovery::flush_for_crash();
            tell_about_crash();
        }
    }));
}

/// The icon for the window and the taskbar. The program file has the same one built in, for Explorer.
fn window_icon() -> Option<egui::IconData> {
    let mut reader = png::Decoder::new(std::io::Cursor::new(&include_bytes!("../data/icon.png")[..])).read_info().ok()?;
    let mut rgba = vec![0; reader.output_buffer_size()?];
    let frame = reader.next_frame(&mut rgba).ok()?;
    rgba.truncate(frame.buffer_size());
    (frame.color_type == png::ColorType::Rgba).then_some(egui::IconData { rgba, width: frame.width, height: frame.height })
}

fn main() -> eframe::Result {
    report_crashes();
    // The uninstaller's last request of the program. There's no window, and nobody to tell if it can't be done.
    if std::env::args_os().any(|arg| arg == "--unregister-file-types") {
        let _ = associations::remove_if_mine();
        return Ok(());
    }
    let mut files = Vec::new();
    let mut screenshot = None;
    let mut args = std::env::args_os().skip(1);
    while let Some(arg) = args.next() {
        if arg == "--screenshot" {
            let Some(path) = args.next() else {
                eprintln!("--screenshot needs a file to save the picture to.");
                std::process::exit(2);
            };
            screenshot = Some(std::path::PathBuf::from(path));
            // A window that can never draw shouldn't leave this run hanging for ever.
            std::thread::spawn(|| {
                std::thread::sleep(std::time::Duration::from_secs(120));
                eprintln!("The picture wasn't ready in time.");
                std::process::exit(1);
            });
        } else {
            files.push(std::path::PathBuf::from(arg));
        }
    }
    // Files opened from Explorer go to the window already open, unless each is to have a window of its own.
    if screenshot.is_none()
        && !files.is_empty()
        && !settings::Prefs::from_settings(&app::read_settings_file()).new_window_per_file
        && let Some(folder) = app::settings_folder()
        && instance::hand_off(&folder, &files)
    {
        return Ok(());
    }
    // Files double-clicked in Finder arrive as an Apple event, not on the command line.
    #[cfg(target_os = "macos")]
    mac_open::start();
    let mut viewport = egui::ViewportBuilder::default();
    if let Some(icon) = window_icon() {
        viewport = viewport.with_icon(icon);
    }
    let options = eframe::NativeOptions {
        viewport: viewport
            .with_title("Limoniter")
            .with_inner_size([1280.0, 720.0])
            .with_min_inner_size([480.0, 300.0])
            .with_drag_and_drop(true)
            // A run that only takes a picture mustn't take the keyboard from whatever's being typed in.
            .with_active(screenshot.is_none()),
        ..Default::default()
    };
    eframe::run_native("Limoniter", options, Box::new(move |cc| Ok(Box::new(app::Limoniter::new(cc, files, screenshot)))))
}

#[cfg(test)]
mod tests {
    #[test]
    fn the_window_has_its_icon() {
        let icon = super::window_icon().expect("the icon reads");
        assert_eq!((icon.width, icon.height), (64, 64));
        assert_eq!(icon.rgba.len(), 64 * 64 * 4);
        assert!(icon.rgba.chunks(4).any(|pixel| pixel[3] == 255) && icon.rgba[3] == 0, "solid in the middle and clear in the corner");
    }
}
