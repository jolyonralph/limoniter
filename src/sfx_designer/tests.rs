use super::*;

/// The window drawn, then the graph driven by the mouse: each step's (frame, height from 0 at the graph's top to 1
/// at its bottom, press or let go of the left button).
fn mouse(e: &mut Editor, steps: &[(usize, f32, Option<bool>)]) {
    let ctx = egui::Context::default();
    let run = |events: Vec<egui::Event>, e: &mut Editor| {
        let input = egui::RawInput { events, screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(1600.0, 1000.0))), ..Default::default() };
        let _ = ctx.run_ui(input, |ui| {
            e.draw(ui);
        });
    };
    run(Vec::new(), e);
    for &(frame, height, press) in steps {
        let pos = Pos2::new(e.graph_rect.left() + (frame as f32 + 0.5) * e.column, e.graph_rect.top() + height * e.graph_rect.height());
        let mut events = vec![egui::Event::PointerMoved(pos)];
        if let Some(pressed) = press {
            events.push(egui::Event::PointerButton { pos, button: egui::PointerButton::Primary, pressed, modifiers: egui::Modifiers::NONE });
        }
        run(events, e);
    }
    run(Vec::new(), e);
}

fn quiet_editor(machine: Machine, frames: usize) -> Editor {
    let chip = machine.chip();
    let mut e = Editor { play_changes: false, ..Editor::default() };
    e.bank = Bank { machine, effects: vec![Effect { name: "test".into(), frames: vec![Frame { tone: chip != Chip::Sid, ..Frame::quiet(chip) }; frames], adsr: [0, 0xF0] }] };
    e
}

#[test]
fn dragging_across_the_graph_sets_the_volumes_and_notes_in_one_undo() {
    let mut e = quiet_editor(Machine::Spectrum128, 10);
    // Along the bottom part near its top: loud, from frame 2 to frame 6.
    mouse(&mut e, &[(2, 0.66, Some(true)), (4, 0.66, None), (6, 0.66, None), (6, 0.66, Some(false))]);
    let volumes: Vec<u8> = e.effect().frames.iter().map(|f| f.volume).collect();
    assert_eq!(volumes[..2], [0, 0]);
    assert!(volumes[2..7].iter().all(|&v| v >= 14), "{volumes:?}");
    assert_eq!(volumes[7..], [0, 0, 0]);
    e.undo();
    assert!(e.effect().frames.iter().all(|f| f.volume == 0), "the whole drag undone at once");
    e.redo();
    // A note drawn in the top part: high up is a high note.
    mouse(&mut e, &[(1, 0.1, Some(true)), (1, 0.1, Some(false))]);
    mouse(&mut e, &[(8, 0.3, Some(true)), (8, 0.3, Some(false))]);
    let high = sfx::hz(Machine::Spectrum128, &e.effect().frames[1]).unwrap();
    let lower = sfx::hz(Machine::Spectrum128, &e.effect().frames[8]).unwrap();
    assert!(high > 3000.0 && lower < 1000.0 && lower > 100.0, "{high} {lower}");
    assert_eq!(e.frame, 8, "the frame drawn on is the one shown");
}

#[test]
fn every_chip_draws_its_kind_of_frame() {
    for machine in [Machine::C64Pal, Machine::Atari2600Ntsc, Machine::Spectrum48Beeper] {
        let mut e = quiet_editor(machine, 6);
        mouse(&mut e, &[(0, 0.3, Some(true)), (5, 0.3, None), (5, 0.3, Some(false))]);
        mouse(&mut e, &[(0, 0.7, Some(true)), (5, 0.7, None), (5, 0.7, Some(false))]);
        for f in &e.effect().frames {
            assert!(f.sounding(machine.chip()), "{machine:?}: {f:?}");
            assert!(sfx::hz(machine, f).is_some(), "{machine:?}: {f:?}");
        }
    }
}

#[test]
fn the_shape_makes_the_effect_and_the_machine_carries_it_across() {
    let mut e = Editor { play_changes: false, ..Editor::default() };
    e.shape = Shape::preset(Preset::Pickup);
    e.make_from_shape();
    assert_eq!(e.effect().frames.len(), 12);
    let before = sfx::hz(Machine::Spectrum128, &e.effect().frames[0]).unwrap();
    // To the beeper and back: the notes kept as near as each can play them.
    e.set_machine(Machine::Spectrum48Beeper);
    let beeper = sfx::hz(Machine::Spectrum48Beeper, &e.effect().frames[0]).unwrap();
    assert!((beeper / before - 1.0).abs() < 0.03, "{before} {beeper}");
    e.set_machine(Machine::C64Pal);
    let sid = sfx::hz(Machine::C64Pal, &e.effect().frames[0]).unwrap();
    assert!((sid / before - 1.0).abs() < 0.03, "{before} {sid}");
    e.undo();
    e.undo();
    assert_eq!(e.bank.machine, Machine::Spectrum128);
    // Effects added, copied and read back from their source.
    e.add_effect(Effect::from_shape("boom", Machine::Spectrum128, &Shape::preset(Preset::Explosion)));
    assert_eq!((e.bank.effects.len(), e.chosen), (2, 1));
    let source = e.source().unwrap();
    let mut other = Editor::default();
    other.read_source(&source).unwrap();
    assert_eq!(other.bank.bytes().unwrap(), e.bank.bytes().unwrap());
    assert!(other.read_source("nothing here").is_err());
}
