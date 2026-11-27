// A window with no console behind it, in the version people actually run.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod about_demo;
mod amiga_bootdisk;
mod amiga_catalog;
mod amiga_project;
mod arm_disasm;
mod arm_modern;
mod amiga_bootdisk_ui;
mod amiga_copper;
mod asm_tests;
mod raster_designer;
mod amiga_debug;
mod amiga_hw;
mod amiga_icon;
mod amiga_vamos;
mod amiga_ndk;
mod autodocs;
mod gnu_as;
mod amiga_run;
mod st_run;
mod amiga_palette_ui;
mod amiga_source;
mod amiga_tools;
mod ansi_art;
mod addons;
mod vsix;
mod vsix_grammar;
mod vsix_theme;
mod access;
mod app;
mod platform;
#[cfg(target_os = "macos")]
mod mac_open;
mod remote;
mod ftp;
mod remote_project;
mod remote_debug;
mod deploy;
mod big_jobs;
mod associations;
mod basic;
mod basic_extra;
mod basic_more;
mod opticbasic;
mod opticos_rom;
mod char_table;
mod clambers;
mod cli_assemble;
mod clipboards;
mod charset;
mod code_format;
mod codec;
mod compare;
mod diff_engine;
mod compare_rules;
mod compare_session;
mod compare_sync;
mod patch_apply;
mod columns;
mod conversions;
mod console;
mod ebcdic;
mod explorer;
mod explorer_kept;
mod editorconfig;
mod emulators;
mod encoding_prefs;
mod encoding_ui;
#[cfg(test)]
mod fold_multi_tests;
mod find_files;
mod fit;
mod function_classes;
mod ext_formatters;
mod file_colours;
mod file_icons;
mod formatters;
mod git;
mod git_setup;
mod merge3;
mod byte_store;
mod hex_buffer;
mod howtocode;
mod howtocode_spectrum;
mod highlight;
mod i18n;
mod history_file;
mod instance;
mod large_edits;
mod legacy_docs;
mod line_store;
mod media;
mod memory_map;
mod music_export;
mod navigation;
mod panels;
mod pc_starters;
mod pictures;
mod pdf;
mod preview;
mod preview_pdf;
mod print;
mod protracker;
mod ql;
mod ql_run;
mod cpm_run;
mod agon_run;
mod neo_run;
mod ql_source;
mod st_music;
mod st_packers;
mod amiga_crunch;
mod st_tools;
mod sid;
mod c64_tools;
mod project;
mod project_import;
mod project_prefs;
mod reformat;
mod running;
mod sound;
mod speaker;
mod asm;
mod assemblers;
mod boriel;
mod boriel_get;
mod c_starters;
mod compilers;
mod control_hints;
mod sasc;
mod c64_assemblers;
mod c64_hw;
mod c64_source;
mod zx_keyboard;
mod zx_graphics;
mod code_intel;
mod debug_panel;
mod zx_basic_debug;
mod dzrp;
mod disasm;
mod icl2900;
mod workspace;
mod x86;
mod live_server;
mod php_tools;
mod markdown_tools;
mod mermaid;
mod instruction_ref;
mod call_graph;
mod code_panels;
mod diagram;
mod path_complete;
mod pixel_editor;
mod tile_ctm;
mod tile_map;
mod tile_map_editor;
mod screen_designer;
mod amiga_planar;
mod quantise;
mod c64_chars;
mod c64_converter;
mod c64_gfx;
mod next_painter;
mod next_screen;
mod next_tiles;
mod retro;
mod retro_convert;
mod retro_converter;
mod st_planar;
mod st_spectrum;
mod st_sprites;
mod st_converter;
mod amiga_sprites;
mod bitplane_tool;
mod blitter;
mod table_forms;
mod tags;
mod emmet;
mod env_file;
mod js_debug_get;
mod node_tools;
mod time_spent;
mod table_gen;
mod amiga_starters;
mod st_hw;
mod st_tos;
mod st_source;
mod st_blitter;
mod mfp_timer;
mod st_starters;
mod m6502_timing;
mod m6809_timing;
mod m6502_cpu;
mod c64_raster;
mod c64_irq;
mod c64_starters;
mod profiler;
mod data_view;
mod data_table;
mod database;
mod jupyter;
mod sys_dialogs;
mod jsonl;
mod notebook;
mod notebook_install;
mod notebook_output;
mod notebook_run;
mod notebook_vars;
mod py_env;
mod code_cells;
mod repl;
mod rmarkdown;
mod lsp;
mod dap;
mod doc_comments;
mod dragon_dos;
mod dragon_run;
mod vectrex;
mod m6809_cpu;
mod via6522;
mod vectrex_machine;
mod vectrex_screen;
mod sega;
mod gameboy;
mod nes;
mod apple2_run;
mod atari7800;
mod atari8_run;
mod cpc_run;
mod fantasy_carts;
mod binary_tools;
mod code_picture;
mod regex_tester;
mod status_items;
mod todo_panel;
mod view_extras;
mod terminal;
mod serial_link;
mod xmodem;
mod eprom;
mod tasks;
mod vscode_project;
mod compose;
mod outdated;
mod http_file;
mod http_send;
mod test_output;
mod xdebug_get;
mod test_runners;
mod zx_machine;
mod z80_micro;
mod micro_roms;
mod zx80;
mod zx80_program;
mod zx81;
mod zx81_program;
mod ssem;
mod edsac;
mod pdp8;
mod pal8;
mod jupiter_ace;
mod machine_link;
mod retro_screens;
mod region_history;
mod file_templates;
mod recovery;
mod rainbow;
mod csv_colours;
mod csv_query;
mod ribbon_icons;
mod ripper;
mod ripper_ui;
mod screen;
mod settings;
mod settings_file;
mod self_update;
mod update_check;
mod trash;
mod file_info;
mod inspect;
mod layouts;
mod emulator_get;
mod code_spelling;
mod spelling;
mod strings_explorer;
mod structure;
mod structure_library;
mod structure_panel;
mod side_tabs;
mod tabs;
mod table;
mod xlsx;
mod scripting;
mod text_buffer;
mod text_objects;
#[cfg(test)]
mod stress_tests;
mod theme;
mod tools;
mod tracker;
mod trse;
mod rgc_basic;
mod m68k_disasm;
mod m68k_timing;
mod atari2600;
mod atari2600_gfx;
mod atari2600_painter;
mod atari2600_machine;
mod atari2600_tia;
mod sfx;
mod sfx_designer;
mod amos;
mod stos;
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
    crate::sys_dialogs::message_dialog()
        .set_title(crate::i18n::tr("Limoniter"))
        .set_level(rfd::MessageLevel::Error)
        .set_description(crate::i18n::tf!("Limoniter ran into a problem and has to close.\n\nYour unsaved changes have been kept, and will be offered back the next time Limoniter starts.\n\nDetails are in {}",
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
/// The window's size: 1280 by 720, or for a picture, what LIMONITER_SHOT_SIZE says (1600x1000, say), for a panel
/// with more in it than fits the usual window.
fn screenshot_size(picture: bool) -> [f32; 2] {
    let asked = std::env::var("LIMONITER_SHOT_SIZE").ok().filter(|_| picture);
    asked.and_then(|size| {
        let (width, height) = size.split_once('x')?;
        Some([width.trim().parse::<f32>().ok()?.clamp(480.0, 4000.0), height.trim().parse::<f32>().ok()?.clamp(300.0, 3000.0)])
    })
    .unwrap_or([1280.0, 720.0])
}

fn window_icon() -> Option<egui::IconData> {
    let mut reader = png::Decoder::new(std::io::Cursor::new(&include_bytes!("../data/icon.png")[..])).read_info().ok()?;
    let mut rgba = vec![0; reader.output_buffer_size()?];
    let frame = reader.next_frame(&mut rgba).ok()?;
    rgba.truncate(frame.buffer_size());
    (frame.color_type == png::ColorType::Rgba).then_some(egui::IconData { rgba, width: frame.width, height: frame.height })
}

fn main() -> eframe::Result {
    self_update::take_updated_from_environment();
    report_crashes();
    // The uninstaller's last request of the program. There's no window, and nobody to tell if it can't be done.
    if std::env::args_os().any(|arg| arg == "--unregister-file-types") {
        let _ = associations::remove_if_mine();
        return Ok(());
    }
    let all: Vec<std::ffi::OsString> = std::env::args_os().skip(1).collect();
    // On Windows, a copy of Limoniter finishing an update once the window has closed: no window of its own.
    if all.first().is_some_and(|arg| arg == self_update::FINISH) {
        std::process::exit(all.get(1).map_or(1, |plan| self_update::finish(std::path::Path::new(plan))));
    }
    // 68000 source made into an Amiga program, with no window.
    if cli_assemble::wanted(&all) {
        std::process::exit(cli_assemble::run(&all));
    }
    if all.iter().any(|arg| arg == "--help" || arg == "-h") {
        println!("{}", app::USAGE);
        return Ok(());
    }
    // The version, and nothing else: no window, as scripts and installers expect.
    if all.iter().any(|arg| arg == "--version" || arg == "-V") {
        println!("Limoniter {}", env!("CARGO_PKG_VERSION"));
        return Ok(());
    }
    // An option Limoniter doesn't know is a mistake to be told, not a file to open in a window.
    if let Some(option) = app::unknown_option(&all) {
        eprintln!("{}\n\n{}", app::i18n_unknown_option(&option), app::USAGE);
        std::process::exit(2);
    }
    // A diff or merge for Git: a window of its own, which Git waits on, rather than a tab in one already open.
    let launch = match app::parse_tool_args(&all) {
        Ok(launch) => launch,
        Err(problem) => {
            eprintln!("{problem}\n\n{}", app::USAGE);
            std::process::exit(2);
        }
    };
    let mut files = Vec::new();
    let mut screenshot = None;
    let mut args = all.into_iter();
    while let Some(arg) = args.next() {
        if launch.is_some() && (arg == "--diff" || arg == "--merge") {
            for _ in 0..if arg == "--diff" { 2 } else { 4 } {
                args.next();
            }
        } else if arg == "--screenshot" {
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
        } else if arg == "--" || arg.to_string_lossy().starts_with("-psn_") {
            // The end of the options; or macOS's own, which is no file.
        } else {
            files.push(std::path::PathBuf::from(arg));
        }
    }
    // Files opened from Explorer go to the window already open, unless each is to have a window of its own.
    if screenshot.is_none()
        && launch.is_none()
        && !files.is_empty()
        // Started by an update, as the old window closes: the files are this one's to open again.
        && self_update::updated().is_none()
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
            // On a Wayland desktop, what ties the window to its .desktop file, for its icon and name in the dock.
            .with_app_id("com.limoniter.Limoniter")
            .with_inner_size(screenshot_size(screenshot.is_some()))
            .with_min_inner_size([480.0, 300.0])
            .with_drag_and_drop(true)
            // A run that only takes a picture mustn't take the keyboard from whatever's being typed in.
            .with_active(screenshot.is_none()),
        ..Default::default()
    };
    let result = eframe::run_native("Limoniter", options, Box::new(move |cc| Ok(Box::new(app::Limoniter::new(cc, files, screenshot, launch)))));
    // For git mergetool: whether the merge was finished, saved with no conflicts left.
    if let Some(code) = app::tool_exit_code() {
        std::process::exit(if result.is_err() { 1 } else { code });
    }
    result
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
