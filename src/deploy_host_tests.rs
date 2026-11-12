use super::super::tests::{app_with, draw};
use super::*;

fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("limoniter-send-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// Draws frames until sends and add-ons have finished.
fn settle(app: &mut Limoniter, ctx: &Context) {
    let began = Instant::now();
    loop {
        draw(app, ctx, Vec::new());
        if app.sends.jobs.is_empty() && app.script.is_none() && app.addon_queue.is_empty() && app.send_pending.is_none() {
            break;
        }
        assert!(began.elapsed() < Duration::from_secs(30), "the send never finished");
        std::thread::sleep(Duration::from_millis(10));
    }
}

fn folder_device(name: &str, path: &Path, preset: Option<&str>) -> Device {
    Device { name: name.into(), place: Place::Folder { path: path.to_path_buf() }, preset: preset.map(str::to_string), eject: false }
}

#[test]
fn a_file_is_sent_to_a_folder_and_sent_again_in_one_go() {
    let dir = scratch("folder");
    let file = dir.join("game.bas");
    std::fs::write(&file, "10 PRINT \"HELLO\"\n20 GOTO 10\n").unwrap();
    let (mut app, ctx) = app_with("");
    app.sends.devices = vec![folder_device("Share", &dir.join("share"), None), folder_device("Commodore", &dir.join("c64"), Some("commodore"))];
    assert!(app.open_path(&file));
    // The box offers the file's own name, and sends it as it's saved.
    app.open_send_box(0);
    let Dialog::Send(state) = &mut app.dialog else { panic!("the send box is open") };
    assert_eq!(state.name, "game.bas");
    state.name = "GAME.BAS".into();
    let name = state.name.clone();
    app.close_dialog();
    app.begin_send(app.active, app.sends.devices[0].clone(), name);
    settle(&mut app, &ctx);
    assert_eq!(std::fs::read(dir.join("share").join("GAME.BAS")).unwrap(), b"10 PRINT \"HELLO\"\n20 GOTO 10\n");
    assert!(app.send_status().unwrap().0.contains("Sent GAME.BAS to Share"));
    // The file itself is still where it was, and still saves there.
    assert_eq!(app.docs[app.active].path.as_deref(), Some(file.as_path()));
    // Changed and sent again: same place, same name, no questions.
    app.doc().text.set_line_keeping_cursor(1, vec!["20 GOTO 20".into()]);
    app.send_again();
    settle(&mut app, &ctx);
    assert_eq!(std::fs::read_to_string(dir.join("share").join("GAME.BAS")).unwrap(), "10 PRINT \"HELLO\"\n20 GOTO 20\n");
    // A device with its own encoding: PETSCII with CR line endings, upper case letters as PETSCII has them.
    app.open_send_box(1);
    app.close_dialog();
    app.begin_send(app.active, app.sends.devices[1].clone(), "game.seq".into());
    settle(&mut app, &ctx);
    let sent = std::fs::read(dir.join("c64").join("game.seq")).unwrap();
    assert!(sent.contains(&b'\r') && !sent.contains(&b'\n'), "{sent:?}");
    // Send again now goes to the device used last.
    assert_eq!(app.sends.last[0].1, "Commodore");
    // What was sent is remembered with the settings, and read back.
    let back = Sends::from_settings(&app.sends.to_settings());
    assert_eq!(back.devices, app.sends.devices);
    assert_eq!(back.last.first().map(|(_, device, name)| (device.as_str(), name.as_str())), Some(("Commodore", "game.seq")));
    assert_eq!(back.stamps.len(), 2);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn an_add_on_can_rename_a_file_on_its_way_out() {
    let dir = scratch("addon");
    std::fs::create_dir_all(dir.join(crate::addons::FOLDER)).unwrap();
    let addon = dir.join(crate::addons::FOLDER).join("Names.rhai");
    std::fs::write(&addon, "//! name: Eight-three names\n//! on_send: before_sending\nfn before_sending() { if send_device() == \"Card\" { set_send_name(\"GAME.BAS\"); } }\n").unwrap();
    let file = dir.join("my long game name.bas");
    std::fs::write(&file, "10 REM\n").unwrap();
    let (mut app, ctx) = app_with("");
    app.addons = crate::addons::AddOns::new(Some(dir.clone()));
    app.addons.refresh();
    app.addons.set_on(&addon, true);
    app.sends.devices = vec![folder_device("Card", &dir.join("card"), None)];
    assert!(app.open_path(&file));
    app.begin_send(app.active, app.sends.devices[0].clone(), "my long game name.bas".into());
    settle(&mut app, &ctx);
    assert!(dir.join("card").join("GAME.BAS").is_file());
    assert!(!dir.join("card").join("my long game name.bas").exists());
    assert_eq!(app.sends.last[0].2, "GAME.BAS");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_file_waits_for_the_next_to_fetch_it() {
    let dir = scratch("next");
    let file = dir.join("hello.bas");
    std::fs::write(&file, "10 PRINT 1\n").unwrap();
    let (mut app, ctx) = app_with("");
    app.sends.next_port = 0;
    app.sends.devices = vec![Device { name: "Next".into(), place: Place::NextSync { folder: "/home/me".into() }, preset: None, eject: false }];
    assert!(app.open_path(&file));
    app.begin_send(app.active, app.sends.devices[0].clone(), "hello.bas".into());
    settle(&mut app, &ctx);
    let server = app.sends.nextsync.as_ref().expect("listening for the Next");
    let waiting: Vec<(String, Vec<u8>)> = server.shared.lock().unwrap().waiting.iter().map(|w| (w.path.clone(), w.data.to_vec())).collect();
    assert_eq!(waiting, [("/home/me/hello.bas".to_string(), b"10 PRINT 1\n".to_vec())]);
    assert!(app.send_status().unwrap().0.contains("waiting for .sync"));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_basic_listing_is_sent_as_the_program_it_lists() {
    let dir = scratch("listing");
    let file = dir.join("stars.bas");
    let program = crate::basic::tokenise_spectrum(&["  10 BORDER 1: LAYER 2,1", "  20 PRINT \"hi\""], crate::basic::ZxDialect::Next).unwrap();
    std::fs::write(&file, crate::basic::plus3dos_file(&program, None)).unwrap();
    let (mut app, ctx) = app_with("");
    app.sends.devices = vec![folder_device("Card", &dir.join("card"), Some("commodore"))];
    assert!(app.open_path(&file));
    app.open_basic_listing();
    assert_eq!(app.docs[app.active].listing_of, Some((crate::basic::Machine::Next, "stars".to_string())));
    // Edited, and sent: it goes as the program, under the program's name, whatever the device does to text.
    app.doc().text.set_line_keeping_cursor(1, vec!["  20 PRINT \"changed\"".into()]);
    app.open_send_box(0);
    let Dialog::Send(state) = &app.dialog else { panic!("the send box is open") };
    assert_eq!(state.name, "stars.bas");
    let name = state.name.clone();
    app.close_dialog();
    app.begin_send(app.active, app.sends.devices[0].clone(), name);
    settle(&mut app, &ctx);
    let sent = std::fs::read(dir.join("card").join("stars.bas")).unwrap();
    let edited = crate::basic::tokenise_spectrum(&["  10 BORDER 1: LAYER 2,1", "  20 PRINT \"changed\""], crate::basic::ZxDialect::Next).unwrap();
    assert_eq!(sent, crate::basic::plus3dos_file(&edited, None));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_listing_knows_its_machine_only_when_that_is_clear() {
    use crate::basic::{CbmMachine, Machine, machine_of};
    let next = crate::basic::ZxDialect::Next.name();
    let spectrum = crate::basic::ZxDialect::Spectrum.name();
    assert_eq!(machine_of(next, "bas", &[]), Some(Machine::Next));
    assert_eq!(machine_of(spectrum, "tap", &[]), Some(Machine::Spectrum));
    assert_eq!(machine_of(spectrum, "bas", &[]), None, "a 48K program in a +3DOS file could go back either way");
    assert_eq!(machine_of("Commodore BASIC 2.0", "prg", &[0x01, 0x08]), Some(Machine::Commodore(CbmMachine::C64)));
    assert_eq!(machine_of("Commodore BASIC 2.0", "prg", &[0x01, 0x10]), Some(Machine::Commodore(CbmMachine::Vic20)));
    assert_eq!(machine_of("Commodore BASIC 7.0", "prg", &[0x01, 0x1C]), Some(Machine::Commodore(CbmMachine::C128)));
    assert_eq!(machine_of("Atari BASIC", "bas", &[]), None);
}
