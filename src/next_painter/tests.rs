use super::*;

fn painter(mode: NextMode) -> Painter {
    Painter { left: 5, right: 0, ..Painter::new(NextScreen::new(mode), "t") }
}

fn at(p: &Painter, x: usize, y: usize) -> u8 {
    p.screen.pixels[y * p.screen.width() + x]
}

#[test]
fn shapes_fills_and_text() {
    let mut p = painter(NextMode::Layer2Wide);
    let mut screen = p.screen.clone();
    p.tool = Tool::FilledEllipse;
    p.shape(&mut screen, Drag { from: (100, 100), to: (130, 110), right: false }, false);
    assert_eq!(screen.pixels[100 * 320 + 130], 5, "the right end of the middle row");
    assert_eq!(screen.pixels[100 * 320 + 131], 0);
    assert_eq!(screen.pixels[110 * 320 + 100], 5, "the bottom");
    assert_eq!(screen.pixels[109 * 320 + 129], 0, "outside, near the corner");
    p.screen = screen;
    // A box, then filled inside: the fill stops at its edge.
    p.tool = Tool::Rectangle;
    let mut screen = p.screen.clone();
    p.shape(&mut screen, Drag { from: (10, 10), to: (40, 30), right: false }, false);
    p.screen = screen;
    p.fill(20, 20, 9);
    assert_eq!(at(&p, 20, 20), 9);
    assert_eq!(at(&p, 10, 20), 5);
    assert_eq!(at(&p, 5, 5), 0);
    assert_eq!(p.used()[9], 29 * 19);
    // Text, twice the size, on the right colour: the A's second row (0x3C) at 2×.
    p.text = "A".into();
    p.big_text = true;
    p.write(200, 200, true);
    let row: Vec<u8> = (0..16).map(|x| at(&p, 200 + x, 202)).collect();
    assert_eq!(row, [0, 0, 0, 0, 5, 5, 5, 5, 5, 5, 5, 5, 0, 0, 0, 0]);
}

#[test]
fn selections_copy_turn_and_paste() {
    let mut p = painter(NextMode::Layer2);
    p.screen.pixels[0] = 7;
    p.screen.pixels[1] = 8;
    p.selection = Some((0, 0, 3, 2));
    p.copy();
    p.turn_copy();
    let clip = p.clipboard.clone().unwrap();
    assert_eq!((clip.w, clip.h), (2, 3));
    // Turned clockwise: what was the top row is the right column.
    assert_eq!(clip.pixels, vec![0, 7, 0, 8, 0, 0]);
    p.paste_over = true;
    p.screen.pixels[100 * 256 + 100] = 3;
    p.paste_at(100, 100);
    assert_eq!(at(&p, 101, 100), 7);
    assert_eq!(at(&p, 100, 100), 3, "the right colour (0) left see-through");
    p.flip_across();
    assert_eq!(at(&p, 2, 0), 7);
    p.left = 7;
    p.right = 4;
    p.recolour();
    assert_eq!(at(&p, 2, 0), 4);
    assert_eq!(at(&p, 101, 100), 7, "only in the selection");
    p.undo();
    assert_eq!(at(&p, 2, 0), 7);
    p.redo();
    assert_eq!(at(&p, 2, 0), 4);
    p.selection = None;
    p.scroll_eight = true;
    p.scroll(-1, 0);
    assert_eq!(at(&p, 93, 100), 7);
}

#[test]
fn the_mode_changes_and_the_640_mode_keeps_to_16_colours() {
    let mut p = painter(NextMode::Layer2);
    p.left = 200;
    p.screen.pixels[0] = 200;
    p.set_mode(NextMode::Layer2Hi);
    assert_eq!(p.screen.pixels.len(), 640 * 256);
    assert!(p.left < 16 && p.screen.pixels[0] < 16);
    p.undo();
    assert_eq!(p.screen.mode, NextMode::Layer2);
}

/// The canvas driven by the mouse at 1:1.
fn mouse(p: &mut Painter, frames: &[&[(i32, i32, Option<bool>)]]) {
    let ctx = egui::Context::default();
    let run = |events: Vec<egui::Event>, p: &mut Painter| -> egui::Pos2 {
        let input = egui::RawInput { events, screen_rect: Some(Rect::from_min_size(egui::Pos2::ZERO, vec2(1400.0, 1000.0))), ..Default::default() };
        let mut origin = egui::Pos2::ZERO;
        let _ = ctx.run_ui(input, |ui| {
            origin = ui.cursor().min;
            p.canvas(ui, vec2(2.0, 2.0));
        });
        origin
    };
    let origin = run(Vec::new(), p);
    for frame in frames {
        let mut events = Vec::new();
        for &(x, y, press) in *frame {
            let pos = origin + vec2(x as f32 * 2.0 + 1.0, y as f32 * 2.0 + 1.0);
            events.push(egui::Event::PointerMoved(pos));
            if let Some(pressed) = press {
                events.push(egui::Event::PointerButton { pos, button: egui::PointerButton::Primary, pressed, modifiers: egui::Modifiers::NONE });
            }
        }
        run(events, p);
    }
    run(Vec::new(), p);
}

#[test]
fn the_mouse_draws_picks_and_selects() {
    let mut p = painter(NextMode::Layer2);
    p.brush = 3;
    mouse(&mut p, &[&[(10, 10, Some(true))], &[(40, 10, None)], &[(40, 10, Some(false))]]);
    assert!((10..=40).all(|x| at(&p, x, 9) == 5 && at(&p, x, 11) == 5), "a stroke three wide");
    p.tool = Tool::Line;
    p.brush = 1;
    mouse(&mut p, &[&[(50, 50, Some(true))], &[(90, 70, None)]]);
    assert_eq!(at(&p, 90, 70), 0, "not until it's let go");
    mouse(&mut p, &[&[(90, 70, Some(false))]]);
    assert_eq!(at(&p, 90, 70), 5);
    p.tool = Tool::Picker;
    p.screen.pixels[5 * 256 + 5] = 77;
    mouse(&mut p, &[&[(5, 5, Some(true))], &[(5, 5, Some(false))]]);
    assert_eq!(p.left, 77);
    p.tool = Tool::Select;
    mouse(&mut p, &[&[(20, 30, Some(true))], &[(29, 34, None)], &[(29, 34, Some(false))]]);
    assert_eq!(p.selection, Some((20, 30, 10, 5)));
}
