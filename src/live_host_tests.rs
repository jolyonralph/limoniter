//! The live link, with the built-in Spectrum: memory, screen and BASIC, out and back in.

use super::*;
use crate::app::tests::app_with;

/// Runs frames of the built-in Spectrum running for the file in front.
fn run_frames(app: &mut Limoniter, frames: usize) {
    let doc_id = app.docs[app.active].id;
    for session in &mut app.running.sessions {
        if let SessionKind::BuiltIn { machine, doc_id: d, .. } = &mut session.kind
            && *d == doc_id
        {
            for _ in 0..frames {
                machine.frame().unwrap();
            }
        }
    }
}

fn spectrum_running(lines: &[&str]) -> Limoniter {
    let (mut app, _ctx) = app_with("run_on=spectrum48\u{1}Built-in Spectrum\n");
    app.new_document();
    app.doc().text.set_content(lines.iter().map(|l| l.to_string()).collect());
    app.doc().listing_of = Some((basic::Machine::Spectrum, "test".into()));
    app.run_on_machine();
    run_frames(&mut app, 60);
    app
}

#[test]
fn the_machines_basic_comes_out_as_a_listing_and_goes_back_in() {
    let mut app = spectrum_running(&["10 PRINT \"ONE\"", "20 STOP"]);
    let running = app.active;
    assert!(app.reachable().is_some());
    app.take_machine_basic();
    assert_ne!(app.active, running, "the listing opens as a file of its own");
    let lines: Vec<String> = (0..app.docs[app.active].text.line_count()).map(|i| app.docs[app.active].text.line(i).to_string()).collect();
    assert_eq!(lines, ["  10 PRINT \"ONE\"", "  20 STOP"]);
    assert_eq!(app.docs[app.active].listing_of.as_ref().map(|l| l.0), Some(basic::Machine::Spectrum));
    // Changed, and put back: the machine's program is the new one, and its variables area is where it should be.
    app.doc().text.set_content(vec!["  10 PRINT \"TWO\"".into(), "  20 PRINT \"MORE\"".into()]);
    let listing = app.active;
    app.active = running;
    run_frames(&mut app, 10);
    app.active = listing;
    // The listing isn't the file the Spectrum was run for, so it reaches the one most recently run.
    app.put_machine_basic();
    assert!(matches!(app.dialog, Dialog::None), "nothing went wrong");
    app.take_machine_basic();
    let lines: Vec<String> = (0..app.docs[app.active].text.line_count()).map(|i| app.docs[app.active].text.line(i).to_string()).collect();
    assert_eq!(lines, ["  10 PRINT \"TWO\"", "  20 PRINT \"MORE\""]);
    // And it runs: on a 48K, R in keyword mode is RUN.
    app.active = running;
    let doc_id = app.docs[running].id;
    let Some(SessionKind::BuiltIn { machine, .. }) = app.running.sessions.iter_mut().map(|s| &mut s.kind).find(|k| matches!(k, SessionKind::BuiltIn { doc_id: d, .. } if *d == doc_id)) else { panic!() };
    machine.type_char('r');
    machine.type_char('\n');
    for _ in 0..100 {
        machine.frame().unwrap();
    }
    let screen = machine.screen_text();
    assert!(screen.iter().any(|row| row == "TWO") && screen.iter().any(|row| row == "MORE"), "{screen:?}");
}

#[test]
fn its_memory_follows_the_machine_and_edits_go_back() {
    let mut app = spectrum_running(&["10 GO TO 10"]);
    app.open_machine_memory();
    let memory = app.active;
    assert!(app.docs[memory].hex_mode);
    assert_eq!(app.docs[memory].hex.len(), 0x10000);
    // An edit in the hex view goes to the machine at once.
    app.docs[memory].hex.move_caret(0x8000, false);
    app.docs[memory].hex.type_hex_digit(0xA);
    app.docs[memory].hex.type_hex_digit(0xB);
    assert!(app.follow_live_views());
    let reach = app.docs[memory].live.as_ref().unwrap().reach;
    let seen = app.reach_live(reach, |live| live.read(0x8000, 1)).unwrap();
    assert_eq!(seen, [0xAB]);
    assert!(!app.docs[memory].hex.is_modified(), "once it's in the machine, it's no longer an unsaved change");
    // And a change in the machine shows in the hex view when it's looked at again.
    app.reach_live(reach, |live| live.write(0x9000, &[0x42])).unwrap();
    if let Some(view) = &mut app.docs[memory].live {
        view.looked = Instant::now() - LOOK_EVERY;
    }
    app.follow_live_views();
    assert_eq!(app.docs[memory].hex.byte(0x9000), 0x42);
    assert_eq!(app.docs[memory].hex.caret, 0x8001, "the cursor stays where it was");
}

#[test]
fn its_screen_reads_as_text_and_says_when_the_machine_stops() {
    let mut app = spectrum_running(&["10 PRINT \"ON THE SCREEN\"", "20 GO TO 20"]);
    run_frames(&mut app, 20);
    app.open_machine_screen();
    let screen = app.active;
    let text: Vec<String> = (0..app.docs[screen].text.line_count()).map(|i| app.docs[screen].text.line(i).to_string()).collect();
    assert!(text.iter().any(|line| line == "ON THE SCREEN"), "{text:?}");
    assert!(app.docs[screen].text.read_only);
    app.stop_all_sessions();
    app.follow_live_views();
    assert!(app.docs[screen].live.as_ref().unwrap().problem.is_some());
}
