use super::*;

/// The canvas driven by the mouse: each step's (cell x, cell y, press or let go of the left button). A cell of
/// -1 across is the first column of swatches beside the grid, -2 the second.
fn mouse(e: &mut Editor, frames: &[&[(i32, i32, Option<bool>)]]) {
    let ctx = egui::Context::default();
    let run = |events: Vec<egui::Event>, e: &mut Editor| -> egui::Pos2 {
        let input = egui::RawInput { events, screen_rect: Some(Rect::from_min_size(egui::Pos2::ZERO, vec2(1600.0, 1000.0))), ..Default::default() };
        let mut origin = egui::Pos2::ZERO;
        let _ = ctx.run_ui(input, |ui| {
            origin = ui.cursor().min;
            e.canvas(ui);
        });
        origin
    };
    let origin = run(Vec::new(), e);
    let (w, _) = e.size();
    for frame in frames {
        let mut events = Vec::new();
        for &(x, y, press) in *frame {
            let across = if x < 0 { w as f32 * e.cell.x + 6.0 + (-x - 1) as f32 * 20.0 + 9.0 } else { (x as f32 + 0.5) * e.cell.x };
            let pos = origin + vec2(across, (y as f32 + 0.5) * e.cell.y);
            events.push(egui::Event::PointerMoved(pos));
            if let Some(pressed) = press {
                events.push(egui::Event::PointerButton { pos, button: egui::PointerButton::Primary, pressed, modifiers: egui::Modifiers::NONE });
            }
        }
        run(events, e);
    }
    run(Vec::new(), e);
}

#[test]
fn the_mouse_draws_sprites_and_gives_lines_their_colours() {
    let mut e = Editor::default();
    e.sprites.frames[0] = Sprites::blank(8);
    mouse(&mut e, &[&[(0, 0, Some(true))], &[(7, 0, None)], &[(7, 0, Some(false))]]);
    assert_eq!(e.sprites.frames[0].rows[0], 0xFF, "a stroke across the top line");
    e.tool = Tool::Box;
    mouse(&mut e, &[&[(1, 2, Some(true))], &[(4, 6, None)]]);
    assert_eq!(e.sprites.frames[0].rows[2], 0, "not until it's let go");
    mouse(&mut e, &[&[(4, 6, Some(false))]]);
    assert_eq!(e.sprites.frames[0].rows[2..7], [0x78, 0x48, 0x48, 0x48, 0x78]);
    e.tool = Tool::Fill;
    mouse(&mut e, &[&[(2, 4, Some(true))], &[(2, 4, Some(false))]]);
    assert_eq!(e.sprites.frames[0].rows[2..7], [0x78, 0x78, 0x78, 0x78, 0x78], "filled inside the box");
    // Colours dragged down the swatches.
    e.colour = 0x46;
    mouse(&mut e, &[&[(-1, 1, Some(true))], &[(-1, 3, None)], &[(-1, 3, Some(false))]]);
    assert_eq!(e.sprites.frames[0].colours[..5], [0x0E, 0x46, 0x46, 0x46, 0x0E]);
    e.undo();
    assert_eq!(e.sprites.frames[0].colours[1], 0x0E, "the colours undone at once");
    e.redo();
    // Turned about.
    e.flip_across();
    assert_eq!(e.sprites.frames[0].rows[3], 0x1E);
    e.shift(1, 0);
    assert_eq!(e.sprites.frames[0].rows[3], 0x0F);
    e.flip_down();
    assert_eq!(e.sprites.frames[0].rows[7], 0xFF);
    assert_eq!(e.sprites.frames[0].colours[6], 0x46);
}

#[test]
fn the_playfield_is_drawn_through_its_halves() {
    let mut e = Editor { part: Part::Playfield, ..Editor::default() };
    e.field = Playfield::new(Halves::Mirrored, 10, 8);
    // Drawn on the right half, it's the left block mirrored that's set.
    mouse(&mut e, &[&[(35, 2, Some(true))], &[(35, 2, Some(false))]]);
    assert!(e.field.pixel(4, 2) && e.field.pixel(35, 2));
    assert_eq!(e.field.row_registers(2)[1], 0x80);
    e.tool = Tool::Line;
    mouse(&mut e, &[&[(0, 5, Some(true))], &[(19, 5, None)], &[(19, 5, Some(false))]]);
    assert!((0..40).all(|x| e.field.pixel(x, 5)));
    e.colour = 0x34;
    mouse(&mut e, &[&[(-2, 0, Some(true))], &[(-2, 0, Some(false))]]);
    assert_eq!(e.field.backgrounds[0], 0x34);
    e.field.set_halves(Halves::Asymmetric);
    e.shift(1, 0);
    assert!(e.field.pixel(5, 2) && e.field.pixel(36, 2) && e.field.pixel(0, 5));
    e.invert();
    assert!(!e.field.pixel(5, 2) && e.field.pixel(4, 2));
}

#[test]
fn frames_are_taken_from_a_cartridge() {
    let mut e = Editor::default();
    e.sprites.set_height(4);
    let mut bytes = vec![0u8; 64];
    bytes[16..24].copy_from_slice(&[1, 2, 3, 4, 5, 6, 7, 8]);
    e.rip = Some(Rip { name: "t.a26".into(), bytes, offset: 16, count: 2 });
    e.take_frames();
    assert_eq!(e.sprites.frames.iter().map(|f| f.rows.clone()).collect::<Vec<_>>(), vec![vec![1, 2, 3, 4], vec![5, 6, 7, 8]]);
    e.order = Order::BottomFirst;
    e.take_frames();
    assert_eq!(e.sprites.frames[0].rows, [4, 3, 2, 1], "kept bottom line first, turned the right way up");
    // The whole window drawn, looking through the cartridge.
    let ctx = egui::Context::default();
    for _ in 0..2 {
        let _ = ctx.run_ui(Default::default(), |ui| {
            e.draw(ui);
        });
    }
}
