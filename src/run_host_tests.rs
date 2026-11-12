//! Running programs: where they run, the box that asks, and real emulators where one is installed.

use super::tests::app_with;
use super::*;
use crate::deploy::{Device, Place};

fn listing(app: &mut Limoniter, lines: &[&str], machine: basic::Machine) {
    app.new_document();
    app.doc().text.set_content(lines.iter().map(|l| l.to_string()).collect());
    app.doc().listing_of = Some((machine, "test".into()));
}

#[test]
fn the_first_run_asks_where_and_the_answer_is_remembered() {
    let (mut app, _ctx) = app_with("");
    listing(&mut app, &["10 PRINT \"HI\""], basic::Machine::Spectrum);
    app.run_on_machine();
    let Dialog::Run(boxed) = &app.dialog else { panic!("it asks where") };
    let run_host::RunBox::Where { runnable, .. } = boxed.as_ref() else { panic!("it asks where") };
    assert_eq!(runnable.machine, crate::running::Machine::Spectrum48);
    // The built-in Spectrum takes it; with nothing else set up, it's the only place.
    let targets = app.targets_for(runnable);
    assert_eq!(targets.len(), 1);
    assert!(targets[0].2.is_ok());
    let run = (**runnable).clone();
    app.close_dialog();
    app.running.defaults.push(("spectrum48".into(), run_host::BUILT_IN_NAME.into()));
    app.run_on_machine();
    assert!(matches!(app.dialog, Dialog::None), "the second time it doesn't ask");
    assert!(app.running.sessions.iter().any(|s| s.target == run_host::BUILT_IN_NAME && s.file == run.name));
    assert!(app.running.to_settings().contains("run_on=spectrum48\u{1}Built-in Spectrum"));
    app.stop_all_sessions();
    assert!(app.running.sessions.is_empty());
}

#[test]
fn plain_text_is_run_as_the_basic_chosen() {
    let (mut app, _ctx) = app_with("run_on=spectrum48\u{1}Built-in Spectrum\n");
    app.new_document();
    app.doc().text.set_content(vec!["10 PRINT \"HI\"".into()]);
    app.run_on_machine();
    assert!(matches!(&app.dialog, Dialog::Run(b) if matches!(b.as_ref(), run_host::RunBox::Which { .. })), "it asks which BASIC");
    let id = app.docs[app.active].id;
    app.close_dialog();
    // As choosing ZX Spectrum 48K BASIC does.
    app.doc().listing_of = Some((basic::Machine::Spectrum, "test".into()));
    app.run_on_machine();
    assert!(app.running.sessions.iter().any(|s| matches!(s.kind, run_host::SessionKind::BuiltIn { doc_id, .. } if doc_id == id)));
}

#[test]
fn places_that_cant_take_a_program_say_why() {
    let (mut app, _ctx) = app_with("");
    app.sends.devices = vec![
        Device { name: "VICE".into(), place: Place::Emulator { emulator: "vice".into(), program: "x64sc".into(), options: String::new() }, preset: None, eject: false },
        Device { name: "Next".into(), place: Place::NextSync { folder: "/".into() }, preset: None, eject: false },
        Device { name: "Card".into(), place: Place::Folder { path: "/tmp".into() }, preset: None, eject: false },
    ];
    let next = crate::running::Runnable::from_listing(&["10 LAYER 2,1"], basic::Machine::Next, "n").unwrap();
    let targets = app.targets_for(&next);
    let said = |name: &str| targets.iter().find(|(_, n, _)| n == name).map(|(.., s)| s.clone()).unwrap();
    assert!(said(run_host::BUILT_IN_NAME).is_err(), "the built-in Spectrum isn't a Next");
    assert_eq!(said("VICE").unwrap_err(), "VICE doesn't run ZX Spectrum Next programs.");
    assert!(said("Next").is_ok());
    assert!(said("Card").is_ok(), "a card or folder takes anything");
}

/// A real ZEsarUX, where LIMONITER_ZESARUX says where one is: run starts it with the program, which prints; run
/// again loads the next program into the same one.
#[test]
fn a_program_runs_on_a_real_zesarux() {
    let Some(program) = std::env::var_os("LIMONITER_ZESARUX") else {
        return;
    };
    let (mut app, _ctx) = app_with("");
    app.sends.devices = vec![Device { name: "ZEsarUX".into(), place: Place::Emulator { emulator: "zesarux".into(), program: program.into(), options: "--vo null --ao null".into() }, preset: None, eject: false }];
    listing(&mut app, &["10 PRINT \"HELLO\"", "20 GO TO 20"], basic::Machine::Spectrum);
    app.run_on_target(run_host::Target::Device(0));
    assert!(matches!(app.dialog, Dialog::None));
    let port = match &app.running.sessions[0].kind {
        run_host::SessionKind::App { port: Some(port), .. } => *port,
        _ => panic!("it's reachable"),
    };
    let screen_says = |text: &str| {
        let began = std::time::Instant::now();
        while began.elapsed() < Duration::from_secs(40) {
            if let Ok(mut link) = crate::machine_link::Zrcp::connect(port)
                && link.screen_text().is_ok_and(|screen| screen.contains(text))
            {
                return true;
            }
            std::thread::sleep(Duration::from_millis(250));
        }
        false
    };
    assert!(screen_says("HELLO"), "the program ran and printed");
    let pid = match &app.running.sessions[0].kind {
        run_host::SessionKind::App { child, .. } => child.id(),
        _ => unreachable!(),
    };
    listing(&mut app, &["10 PRINT \"AGAIN\"", "20 GO TO 20"], basic::Machine::Spectrum);
    app.run_on_target(run_host::Target::Device(0));
    assert_eq!(app.running.sessions.len(), 1);
    assert!(matches!(&app.running.sessions[0].kind, run_host::SessionKind::App { child, .. } if child.id() == pid), "loaded into the same ZEsarUX");
    assert!(screen_says("AGAIN"), "the second program ran there");
    app.stop_all_sessions();
}
