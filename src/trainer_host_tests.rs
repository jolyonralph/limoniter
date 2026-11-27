//! The trainer and the debug console port on built-in machines running tiny programs: what a program writes to its
//! console port reaches the Output panel, and what's frozen is held or blocked, for the program it was kept for.

use super::run_host::{Session, SessionKind};
use super::tests::app_with;
use crate::running::{Format, Machine, Runnable};
use crate::trainer::{Freeze, Width};

fn freeze(address: u32, value: u32, block: bool) -> Freeze {
    Freeze { address, value, width: Width::Byte, name: "Lives".into(), on: true, block }
}

/// A Spectrum program that prints "score 120" to the console port, then counts up at `count` for ever.
fn spectrum() -> (Session, u16) {
    let source = "  org 32768\nstart: ld hl,msg\nprint: ld a,(hl)\n  out ($BB),a\n  inc hl\n  or a\n  jr nz,print\nloop: ld a,(count)\n  inc a\n  ld (count),a\n  jr loop\nmsg: db \"score 120\", 10, 0\ncount: db 0\n";
    let assembly = crate::asm::assemble(&crate::asm::z80::Z80, std::path::Path::new("t.asm"), source, &|_| Err(crate::i18n::tr("no").into()));
    assert!(assembly.ok(), "{:?}", assembly.problems);
    let tape = crate::asm::output::spectrum_tape(&assembly, "t").unwrap();
    let run = Runnable { machine: Machine::Spectrum48, format: Format::Tap, name: "t.tap".into(), bytes: std::sync::Arc::new(tape) };
    let machine = crate::zx_machine::BuiltIn::start(&run).unwrap();
    let mut session = Session { target: "Built-in Spectrum".into(), machine: Machine::Spectrum48, file: "t.tap".into(), kind: SessionKind::BuiltIn { machine: Box::new(machine), doc_id: 1, run: Box::new(run), texture: None, error: None }, started: std::time::Instant::now() };
    let count = assembly.symbols["count"] as u16;
    // The tape loads, and the program runs, once its counter starts to move.
    for _ in 0..50 * 90 {
        frames(&mut session, 1);
        if spectrum_peek(&session, count) != 0 {
            break;
        }
    }
    assert_ne!(spectrum_peek(&session, count), 0, "the program is running");
    (session, count)
}

fn frames(session: &mut Session, n: usize) {
    for _ in 0..n {
        match &mut session.kind {
            SessionKind::BuiltIn { machine, .. } => machine.frame().unwrap(),
            SessionKind::Atari2600 { machine, .. } => machine.frame().unwrap(),
            SessionKind::Vectrex { machine, .. } => {
                if let super::vectrex_run_host::VectrexRun::Running(v) = machine.as_mut() {
                    v.frame().unwrap();
                }
            }
            _ => panic!("not a machine in these tests"),
        }
    }
}

fn spectrum_peek(session: &Session, address: u16) -> u8 {
    let SessionKind::BuiltIn { machine, .. } = &session.kind else { panic!() };
    machine.peek(address)
}

#[test]
fn what_a_spectrum_program_writes_to_the_console_port_reaches_the_output_panel() {
    let (mut app, _ctx) = app_with("");
    let (session, _) = spectrum();
    app.running.sessions.push(session);
    app.trainer_tick();
    assert_eq!(app.script_output, vec!["Spectrum: score 120"]);
    assert!(app.show_results);
    // It's printed once, not again.
    app.trainer_tick();
    assert_eq!(app.script_output.len(), 1);
}

#[test]
fn a_frozen_value_is_written_back_every_frame_and_only_for_its_program() {
    let (mut app, _ctx) = app_with("");
    let (session, count) = spectrum();
    app.running.sessions.push(session);
    app.trainer_kept.set("t.tap", vec![freeze(u32::from(count), 7, false)]);
    app.trainer_kept.set("other.tap", vec![freeze(u32::from(count), 200, false)]);
    for _ in 0..3 {
        frames(&mut app.running.sessions[0], 5);
        app.trainer_tick();
        assert_eq!(spectrum_peek(&app.running.sessions[0], count), 7, "held at 7, not 200 (that's another program's)");
    }
    // Switched off, it counts on.
    let mut off = freeze(u32::from(count), 7, false);
    off.on = false;
    app.trainer_kept.set("t.tap", vec![off]);
    frames(&mut app.running.sessions[0], 5);
    app.trainer_tick();
    assert_ne!(spectrum_peek(&app.running.sessions[0], count), 7);
}

#[test]
fn blocked_writes_leave_the_value_alone_without_it_being_put_back() {
    let (mut app, _ctx) = app_with("");
    let (session, count) = spectrum();
    app.running.sessions.push(session);
    frames(&mut app.running.sessions[0], 5);
    app.trainer_kept.set("t.tap", vec![freeze(u32::from(count), 9, true)]);
    app.trainer_tick();
    // With no tick between, so nothing writes it back: only the block holds it, while the program goes on
    // incrementing it thousands of times.
    frames(&mut app.running.sessions[0], 20);
    assert_eq!(spectrum_peek(&app.running.sessions[0], count), 9);
    // Removed, the program's writes get through again.
    app.trainer_kept.set("t.tap", Vec::new());
    app.trainer_tick();
    frames(&mut app.running.sessions[0], 20);
    assert_ne!(spectrum_peek(&app.running.sessions[0], count), 9);
}

#[test]
fn the_2600_and_the_vectrex_have_console_ports_and_can_hold_and_block_ram() {
    let (mut app, _ctx) = app_with("");
    // The 2600: a program that writes "ok" to $2D and then adds one to $80 for ever.
    let source = "        processor 6502\n        org $F000\nReset:  sei\n        cld\n        ldx #0\nSay:    lda Text,x\n        beq Count\n        sta $2D\n        inx\n        jmp Say\nCount:  inc $80\n        jmp Count\nText:   .byte \"ok 5\", 10, 0\n        org $FFFC\n        .word Reset\n        .word Reset\n";
    let assembly = crate::asm::assemble(&crate::asm::m6502::M6502, std::path::Path::new("t.asm"), source, &|_| Err(crate::i18n::tr("no").into()));
    assert!(assembly.ok(), "{:?}", assembly.problems);
    let mut rom = assembly.image(0xF000, 0xFFFF);
    rom.resize(4096, 0);
    let run = Runnable { machine: Machine::Atari2600, format: Format::Rom, name: "g.bin".into(), bytes: std::sync::Arc::new(rom.clone()) };
    let machine = crate::atari2600_machine::Atari2600::from_rom(&rom);
    app.running.sessions.push(Session { target: "Built-in Atari 2600".into(), machine: Machine::Atari2600, file: "g.bin".into(), kind: SessionKind::Atari2600 { machine: Box::new(machine), doc_id: 2, run: Box::new(run), texture: None, error: None }, started: std::time::Instant::now() });
    frames(&mut app.running.sessions[0], 2);
    app.trainer_kept.set("g.bin", vec![freeze(0x80, 42, true)]);
    app.trainer_tick();
    frames(&mut app.running.sessions[0], 3);
    let SessionKind::Atari2600 { machine, .. } = &app.running.sessions[0].kind else { panic!() };
    assert_eq!(machine.peek(0x80), 42, "blocked at 42 while the program kept adding one");
    assert_eq!(app.script_output, vec!["Atari 2600: ok 5"]);

    // The Vectrex: a cartridge that writes "lives 3" to $C000, then adds one to $C880 for ever.
    let source = "        org 0\n        fcc \"g GCE 2026\"\n        fcb $80\n        fdb $FD0D\n        fcb $F8,$50,$20,-$40\n        fcc \"T\"\n        fcb $80\n        fcb 0\nstart:  lds #$CBEA\n        ldx #msg\nprint:  lda ,x+\n        sta $C000\n        bne print\nloop:   inc $C880\n        bra loop\nmsg:    fcc \"lives 3\"\n        fcb 10,0\n";
    let assembly = crate::asm::assemble(&crate::asm::m6809::M6809, std::path::Path::new("v.asm"), source, &|_| Err(crate::i18n::tr("no").into()));
    assert!(assembly.ok(), "{:?}", assembly.problems);
    let cart = assembly.image(0, assembly.highest().unwrap());
    let run = Runnable { machine: Machine::Vectrex, format: Format::Rom, name: "v.bin".into(), bytes: std::sync::Arc::new(cart.clone()) };
    let machine = super::vectrex_run_host::VectrexRun::Running(Box::new(crate::vectrex_machine::Vectrex::with_cartridge(cart, None, None)));
    app.running.sessions.push(Session { target: "Built-in Vectrex".into(), machine: Machine::Vectrex, file: "v.bin".into(), kind: SessionKind::Vectrex { machine: Box::new(machine), doc_id: 3, run: Box::new(run), texture: None, error: None }, started: std::time::Instant::now() });
    frames(&mut app.running.sessions[1], 3);
    // A word is held big-endian on the 6809.
    app.trainer_kept.set("v.bin", vec![Freeze { width: Width::Word, ..freeze(0xC880, 0x0102, false) }]);
    app.trainer_tick();
    let SessionKind::Vectrex { machine, .. } = &app.running.sessions[1].kind else { panic!() };
    let super::vectrex_run_host::VectrexRun::Running(vectrex) = machine.as_ref() else { panic!() };
    let memory = vectrex.memory();
    assert_eq!(&memory[0xC880..0xC882], &[1, 2]);
    assert_eq!(app.script_output, vec!["Atari 2600: ok 5", "Vectrex: lives 3"]);
}
