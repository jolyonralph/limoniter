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
    // Its BASIC comes out through the remote protocol, too.
    let running = app.active;
    app.take_machine_basic();
    let lines: Vec<String> = (0..app.docs[app.active].text.line_count()).map(|i| app.docs[app.active].text.line(i).to_string()).collect();
    assert_eq!(lines, ["  10 PRINT \"HELLO\"", "  20 GO TO 20"]);
    app.active = running;
    let pid = match &app.running.sessions[0].kind {
        run_host::SessionKind::App { child, .. } => child.id(),
        _ => unreachable!(),
    };
    listing(&mut app, &["10 PRINT \"AGAIN\"", "20 GO TO 20"], basic::Machine::Spectrum);
    app.run_on_target(run_host::Target::Device(0));
    assert_eq!(app.running.sessions.len(), 1);
    assert!(matches!(&app.running.sessions[0].kind, run_host::SessionKind::App { child, .. } if child.id() == pid), "loaded into the same ZEsarUX");
    assert!(screen_says("AGAIN"), "the second program ran there");
    // A listing put in while a program runs stops it with BREAK, goes in, and is RUN.
    listing(&mut app, &["10 PRINT \"PUT BACK\"", "20 GO TO 20"], basic::Machine::Spectrum);
    app.put_machine_basic();
    assert!(matches!(app.dialog, Dialog::None), "{:?}", if let Dialog::Message { body, .. } = &app.dialog { body.clone() } else { String::new() });
    assert!(screen_says("PUT BACK"), "the listing put in was run");
    app.stop_all_sessions();
}

/// A real VICE, where LIMONITER_VICE says where x64sc is (and LIMONITER_VICE_OPTIONS anything it needs, such as
/// where its ROMs are): run starts it with the program, which prints; its BASIC comes out as a listing, and an
/// edited one goes back in and runs.
#[test]
fn a_program_runs_on_a_real_vice_and_its_basic_goes_both_ways() {
    let Some(program) = std::env::var_os("LIMONITER_VICE") else {
        return;
    };
    let options = std::env::var("LIMONITER_VICE_OPTIONS").unwrap_or_default();
    let (mut app, _ctx) = app_with("");
    app.sends.devices = vec![Device { name: "VICE".into(), place: Place::Emulator { emulator: "vice".into(), program: program.into(), options }, preset: None, eject: false }];
    listing(&mut app, &["10 PRINT \"HELLO\"", "20 GOTO 20"], basic::Machine::Commodore(basic::CbmMachine::C64));
    app.run_on_target(run_host::Target::Device(0));
    assert!(matches!(app.dialog, Dialog::None), "{:?}", if let Dialog::Message { body, .. } = &app.dialog { body.clone() } else { String::new() });
    let (reach, machine, _) = app.reachable().expect("VICE can be reached");
    let screen_says = |app: &mut Limoniter, text: &str| {
        let began = std::time::Instant::now();
        while began.elapsed() < Duration::from_secs(60) {
            let seen = app.screen_of(reach, machine);
            if std::env::var_os("LIMONITER_DEBUG").is_some() {
                eprintln!("{seen:?}");
            }
            if seen.is_ok_and(|lines| lines.iter().any(|line| line.contains(text))) {
                return true;
            }
            std::thread::sleep(Duration::from_millis(500));
        }
        false
    };
    assert!(screen_says(&mut app, "HELLO"), "the program ran and printed");
    app.take_machine_basic();
    let lines: Vec<String> = (0..app.docs[app.active].text.line_count()).map(|i| app.docs[app.active].text.line(i).to_string()).collect();
    assert_eq!(lines, ["10 PRINT \"HELLO\"", "20 GOTO 20"]);
    // Putting it back while it runs is fine on a C64, as the RUN typed after starts it afresh.
    app.doc().text.set_content(vec!["10 PRINT \"AGAIN\"".into(), "20 GOTO 20".into()]);
    app.put_machine_basic();
    assert!(matches!(app.dialog, Dialog::None));
    assert!(screen_says(&mut app, "AGAIN"), "the program put back ran");
    app.stop_all_sessions();
}

/// An add-on's on_run runs just before a run, and what runs is the document as it left it.
#[test]
fn an_add_on_can_change_a_listing_before_it_runs() {
    let dir = std::env::temp_dir().join(format!("limoniter-run-addon-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join(crate::addons::FOLDER)).unwrap();
    let addon = dir.join(crate::addons::FOLDER).join("Shout.rhai");
    std::fs::write(&addon, "//! name: Shout\n//! on_run: before_running\nfn before_running() { replace_all(\"quiet\", \"LOUD\"); }\n").unwrap();
    let (mut app, ctx) = app_with("run_on=spectrum48\u{1}Built-in Spectrum\n");
    app.addons = crate::addons::AddOns::new(Some(dir.clone()));
    app.addons.refresh();
    app.addons.set_on(&addon, true);
    listing(&mut app, &["10 PRINT \"quiet\""], basic::Machine::Spectrum);
    app.run_on_machine();
    let began = std::time::Instant::now();
    while app.running.sessions.is_empty() {
        super::tests::draw(&mut app, &ctx, Vec::new());
        assert!(began.elapsed() < Duration::from_secs(30), "it never ran");
        std::thread::sleep(Duration::from_millis(10));
    }
    assert_eq!(app.docs[app.active].text.line(0).as_ref(), "10 PRINT \"LOUD\"");
    let run_host::SessionKind::BuiltIn { machine, .. } = &mut app.running.sessions[0].kind else { panic!() };
    for _ in 0..50 {
        machine.frame().unwrap();
    }
    let screen = crate::zx_machine::screen_text(|address| machine.peek(address));
    assert!(screen.iter().any(|row| row == "LOUD"), "{screen:?}");
    let _ = std::fs::remove_dir_all(&dir);
}

/// Emulators that can't be reached while they run, where LIMONITER_<NAME> says where each is: run starts it with
/// the program and it's still running a few seconds later. LIMONITER_GRAB names a folder to put a picture of the
/// screen in, to look at.
#[test]
fn programs_start_on_real_emulators_that_cant_be_reached() {
    let cases: [(&str, &str, &[&str], basic::Machine); 3] = [
        ("LIMONITER_FUSE", "fuse", &["10 BORDER 2: PRINT \"FUSE\"", "20 GO TO 20"], basic::Machine::Spectrum),
        ("LIMONITER_FUSE_128", "fuse", &["10 PRINT \"SECOND\"", "20 GO TO 20"], basic::Machine::Spectrum),
        ("LIMONITER_B2", "b2", &["10 PRINT \"B2\"", "20 GOTO 20"], basic::Machine::Bbc),
    ];
    for (variable, emulator, lines, machine) in cases {
        let Some(program) = std::env::var_os(variable) else {
            continue;
        };
        let (mut app, _ctx) = app_with("");
        app.sends.devices = vec![Device { name: emulator.into(), place: Place::Emulator { emulator: emulator.into(), program: program.into(), options: std::env::var(format!("{variable}_OPTIONS")).unwrap_or_default() }, preset: None, eject: false }];
        listing(&mut app, lines, machine);
        app.run_on_target(run_host::Target::Device(0));
        assert!(matches!(app.dialog, Dialog::None), "{variable}");
        std::thread::sleep(Duration::from_secs(6));
        app.check_sessions();
        assert_eq!(app.running.sessions.len(), 1, "{variable}: still running");
        assert!(matches!(app.dialog, Dialog::None), "{variable}: {:?}", if let Dialog::Message { body, .. } = &app.dialog { body.clone() } else { String::new() });
        if let Some(folder) = std::env::var_os("LIMONITER_GRAB") {
            let _ = std::process::Command::new("import").args(["-window", "root"]).arg(PathBuf::from(folder).join(format!("{variable}.png"))).status();
        }
        app.stop_all_sessions();
    }
}

/// openMSX, where LIMONITER_OPENMSX says where it is, runs a cartridge.
#[test]
fn a_cartridge_starts_on_a_real_openmsx() {
    let Some(program) = std::env::var_os("LIMONITER_OPENMSX") else {
        return;
    };
    let (mut app, _ctx) = app_with("");
    app.sends.devices = vec![Device { name: "openMSX".into(), place: Place::Emulator { emulator: "openmsx".into(), program: program.into(), options: String::new() }, preset: None, eject: false }];
    // A 16K cartridge that only says hello to the cartridge slot: "AB", its start address, and a loop there.
    let mut rom = vec![0u8; 0x4000];
    rom[0..4].copy_from_slice(&[b'A', b'B', 0x10, 0x40]);
    rom[0x10..0x13].copy_from_slice(&[0x18, 0xFE, 0x00]);
    app.new_document();
    app.docs[app.active].hex.load(rom);
    app.docs[app.active].hex_mode = true;
    app.docs[app.active].path = Some(std::env::temp_dir().join("hello.rom"));
    app.run_on_target(run_host::Target::Device(0));
    assert!(matches!(app.dialog, Dialog::None), "{:?}", if let Dialog::Message { body, .. } = &app.dialog { body.clone() } else { String::new() });
    std::thread::sleep(Duration::from_secs(6));
    app.check_sessions();
    assert_eq!(app.running.sessions.len(), 1, "still running");
    if let Some(folder) = std::env::var_os("LIMONITER_GRAB") {
        let _ = std::process::Command::new("import").args(["-window", "root"]).arg(PathBuf::from(folder).join("openmsx.png")).status();
    }
    app.stop_all_sessions();
}

fn z80_source(app: &mut Limoniter, lines: &[&str]) {
    crate::highlight::load_now();
    app.new_document();
    app.doc().untitled = "border.asm".into();
    app.doc().text.set_content(lines.iter().map(|l| l.to_string()).collect());
    app.doc().update_syntax();
}

#[test]
fn z80_source_is_assembled_and_runs_on_the_built_in_spectrum() {
    let (mut app, _ctx) = app_with("run_on=spectrum48\u{1}Built-in Spectrum\n");
    z80_source(&mut app, &["  org 32768", "start: ld a,2", "  out ($fe),a", "  ld hl,$4000", "  ld (hl),$ff", "loop: jr loop"]);
    assert_eq!(app.doc_cpu(app.active), Some(crate::assemblers::Cpu::Z80), "{}", app.doc().language_name());
    app.assembly_tick();
    assert!(app.doc().text.code.is_some(), "assembler source gets its marks");
    app.run_on_machine();
    assert!(matches!(app.dialog, Dialog::None));
    assert!(app.script_output.iter().any(|l| l.contains("for the ZX Spectrum 48K")), "{:?}", app.script_output);
    let session = app.running.sessions.iter_mut().find(|s| s.target == run_host::BUILT_IN_NAME).expect("it runs");
    let run_host::SessionKind::BuiltIn { machine, .. } = &mut session.kind else { panic!() };
    let mut ran = false;
    for _ in 0..50 * 20 {
        machine.frame().unwrap();
        if machine.peek(0x4000) == 0xFF {
            ran = true;
            break;
        }
    }
    assert!(ran, "the code loaded from the tape and ran");
    app.stop_all_sessions();
}

#[test]
fn assembler_problems_are_listed_marked_and_gone_to() {
    let (mut app, _ctx) = app_with("run_on=spectrum48\u{1}Built-in Spectrum\n");
    z80_source(&mut app, &["  org 32768", "  ld a,1", "  ld q,1", "  jp nowhere"]);
    app.run_on_machine();
    assert!(app.running.sessions.is_empty(), "nothing runs");
    assert!(matches!(app.dialog, Dialog::None), "no box: the problems are in the Output panel");
    let problems = app.doc().text.code.as_ref().unwrap().problems.clone();
    assert_eq!(problems.iter().map(|p| p.0).collect::<Vec<_>>(), vec![2, 3]);
    let listed: Vec<&String> = app.script_output.iter().filter(|l| crate::assemblers::problem_line(l).is_some()).collect();
    assert_eq!(listed.len(), 2, "{:?}", app.script_output);
    assert!(listed[1].contains("border.asm(4): nowhere isn't defined"), "{listed:?}");
    assert_eq!(app.doc().text.caret_line, 2, "the cursor goes to the first");
    app.go_to_place("border.asm", 4);
    assert_eq!(app.doc().text.caret_line, 3);
}

#[test]
fn next_source_becomes_a_nex_file() {
    let (mut app, _ctx) = app_with("");
    z80_source(&mut app, &["  device zxspectrumnext", "  org $8000", "start: nextreg 7,3", "  ld a,2", "  out ($fe),a", "  jr start"]);
    assert!(app.doc_cpu(app.active).is_some(), "{}", app.doc().language_name());
    let run = app.runnable().unwrap().unwrap();
    assert_eq!((run.machine, run.format, run.name.as_str()), (crate::running::Machine::Next, crate::running::Format::Nex, "border.nex"));
    assert!(app.suits(run_host::Target::BuiltIn, &run).is_err(), "the built-in Spectrum isn't a Next");
}

#[test]
fn source_for_the_6502_becomes_a_commodore_64_program() {
    crate::highlight::load_now();
    let (mut app, _ctx) = app_with("");
    app.new_document();
    app.doc().untitled = "border.asm".into();
    app.doc().text.set_content(["  *=$c000", "start lda #2", "  sta $d020", "  ldx #0", "- inx", "  bne -", "  rts"].iter().map(|l| l.to_string()).collect());
    app.doc().update_syntax();
    assert_eq!(app.doc_cpu(app.active), Some(crate::assemblers::Cpu::M6502), "{}", app.doc().language_name());
    let run = app.runnable().unwrap().unwrap();
    assert_eq!((run.machine, run.format, run.name.as_str()), (crate::running::Machine::C64, crate::running::Format::Prg, "border.prg"));
}
