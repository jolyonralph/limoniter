use super::*;

#[test]
fn a_screen_goes_to_a_scr_and_back() {
    let mut screen = Screen::default();
    screen.pixels[0] = true;
    screen.pixels[8 * 256 + 1] = true;
    screen.pixels[191 * 256 + 255] = true;
    screen.attributes[0] = 0x47;
    let scr = screen.to_scr();
    assert_eq!(scr.len(), 6912);
    assert_eq!(scr[0], 0x80, "the top left pixel");
    assert_eq!(scr[0x20], 0x40, "pixel row 8 is the second character row's first: 32 bytes on");
    assert_eq!(scr[6143], 0x01, "the bottom right");
    assert_eq!(scr[6144], 0x47);
    assert_eq!(Screen::from_scr(&scr), Some(screen.clone()));
    // As the built-in Spectrum's screen code reads it, the same picture.
    let picture = crate::pictures::decode(crate::pictures::Format::SpectrumScreen, &scr).unwrap();
    assert_eq!(picture.width, 256);
    assert_eq!(row_offset(1), 256);
    assert_eq!(row_offset(64), 2048);
}

#[test]
fn a_picture_is_brought_in_two_colours_a_cell() {
    // Left half red, right half blue, with a white line through the middle of the screen's height.
    let (w, h) = (512, 384);
    let rgba: Vec<u8> = (0..w * h)
        .flat_map(|i| {
            let (x, y) = (i % w, i / w);
            if y == 192 || y == 193 { [255, 255, 255, 255] } else if x < 256 { [215, 0, 0, 255] } else { [0, 0, 215, 255] }
        })
        .collect();
    let screen = Screen::from_image(&crate::quantise::Image { width: w, height: h, rgba });
    let colours = |a: u8| [a & 7, (a >> 3) & 7];
    assert!(colours(screen.attributes[0]).contains(&2), "red on the left");
    assert!(colours(screen.attributes[31]).contains(&1), "blue on the right");
    let a = screen.attributes[12 * 32];
    assert!(colours(a).contains(&7), "white in the cells the line goes through: {a:02X}");
}

#[test]
fn the_roms_letters_are_the_spectrums() {
    assert_eq!(glyph('A'), [0x00, 0x3C, 0x42, 0x42, 0x7E, 0x42, 0x42, 0x00]);
    assert_eq!(glyph('£'), [0x00, 0x1C, 0x22, 0x78, 0x20, 0x20, 0x7E, 0x00]);
    assert_eq!(glyph('\u{263A}'), glyph('?'));
}

#[test]
fn text_is_written_as_print_writes_it() {
    let mut d = Designer { text: "AB".into(), ink: 2, paper: 6, ..Designer::default() };
    d.screen.pixels.iter_mut().for_each(|p| *p = true);
    d.write(8, 16, true);
    // PRINT AT 2,1;"AB": the cell's top row is clear, and the A's second row is 0x3C.
    let row = |s: &Screen, y: usize, cx: usize| -> u8 { (0..8).fold(0, |b, x| b << 1 | u8::from(s.pixels[y * 256 + cx * 8 + x])) };
    assert_eq!(row(&d.screen, 16, 1), 0x00);
    assert_eq!(row(&d.screen, 17, 1), 0x3C);
    assert_eq!(row(&d.screen, 17, 2), glyph('B')[1]);
    assert_eq!(d.screen.attributes[2 * 32 + 1], 2 | 6 << 3);
    // With only its INK, what's under it stays.
    d.write(8, 40, false);
    assert_eq!(row(&d.screen, 40, 1), 0xFF);
}

#[test]
fn shapes_fills_and_scrolls() {
    let mut d = Designer { colour_cells: false, ..Designer::default() };
    let mut screen = d.screen.clone();
    d.tool = Tool::Rectangle;
    d.shape(&mut screen, Drag { from: (10, 10), to: (20, 30), ink: true }, false);
    assert!(screen.pixels[10 * 256 + 15] && screen.pixels[30 * 256 + 20] && !screen.pixels[20 * 256 + 15]);
    assert_eq!(screen.attributes, Screen::default().attributes, "drawing left the colours");
    d.screen = screen;
    // Filling inside the box stays inside it.
    d.fill(15, 20, true);
    assert!(d.screen.pixels[20 * 256 + 15]);
    assert!(!d.screen.pixels[5 * 256 + 5]);
    assert_eq!(d.screen.pixels.iter().filter(|p| **p).count(), 11 * 21);
    // A circle's edge is its radius from the centre.
    let points = ellipse_points((100, 100), 30, 30);
    assert!(points.iter().all(|&(x, y)| (f64::from((x - 100).pow(2) + (y - 100).pow(2)).sqrt() - 30.0).abs() < 1.0));
    assert!(points.contains(&(130, 100)) && points.contains(&(100, 70)));
    // Scrolled right a pixel, and back round.
    let before = d.screen.clone();
    d.scroll(1, 0);
    assert!(d.screen.pixels[10 * 256 + 21]);
    d.scroll(-1, 0);
    assert_eq!(d.screen, before);
    // A cell at a time moves the colours too.
    d.screen.attributes[0] = 0x47;
    d.scroll_cells = true;
    d.scroll(1, 1);
    assert_eq!(d.screen.attributes[33], 0x47);
}

#[test]
fn cells_are_copied_flipped_and_swapped() {
    let mut d = Designer::default();
    d.screen.pixels[0] = true;
    d.screen.attributes[0] = 0x17;
    d.selection = Some((0, 0, 2, 1));
    d.flip_across();
    assert!(d.screen.pixels[15] && !d.screen.pixels[0]);
    assert_eq!(d.screen.attributes[1], 0x17);
    d.copy();
    d.paste_at(30, 23);
    assert!(d.screen.pixels[184 * 256 + 255]);
    assert_eq!(d.screen.attributes[23 * 32 + 31], 0x17);
    // Swapping INK and PAPER looks the same.
    let looks: Vec<Color32> = (0..16).map(|x| d.screen.shown(x, 0)).collect();
    d.swap_colours();
    assert_eq!((0..16).map(|x| d.screen.shown(x, 0)).collect::<Vec<_>>(), looks);
    assert_eq!(d.screen.attributes[1], 0x3A);
    d.undo();
    assert_eq!(d.screen.attributes[1], 0x17);
    d.redo();
    assert_eq!(d.screen.attributes[1], 0x3A);
    // Pasting part off the screen keeps what's on it.
    d.selection = None;
    d.copy();
    d.paste_at(31, 23);
}

/// The canvas driven by the mouse: each frame's events, the pixel each is at.
fn mouse(d: &mut Designer, frames: &[&[(i32, i32, Option<bool>)]]) {
    let ctx = egui::Context::default();
    let run = |events: Vec<egui::Event>, d: &mut Designer| -> egui::Pos2 {
        let input = egui::RawInput { events, screen_rect: Some(Rect::from_min_size(egui::Pos2::ZERO, vec2(1200.0, 900.0))), ..Default::default() };
        let mut origin = egui::Pos2::ZERO;
        let _ = ctx.run_ui(input, |ui| {
            origin = ui.cursor().min;
            d.canvas(ui, 2.0);
        });
        origin
    };
    let origin = run(Vec::new(), d);
    for frame in frames {
        let mut events = Vec::new();
        for &(x, y, press) in *frame {
            let pos = origin + vec2(x as f32 * 2.0 + 1.0, y as f32 * 2.0 + 1.0);
            events.push(egui::Event::PointerMoved(pos));
            if let Some(pressed) = press {
                events.push(egui::Event::PointerButton { pos, button: egui::PointerButton::Primary, pressed, modifiers: egui::Modifiers::NONE });
            }
        }
        run(events, d);
    }
    run(Vec::new(), d);
}

#[test]
fn the_mouse_draws_with_each_tool() {
    let mut d = Designer::default();
    // A pencil stroke, joined up between where the mouse was seen.
    mouse(&mut d, &[&[(10, 10, Some(true))], &[(30, 10, None)], &[(30, 10, Some(false))]]);
    assert!((10..=30).all(|x| d.screen.pixels[10 * 256 + x]), "the stroke is unbroken");
    // A line, drawn when the button's let go, and only then.
    d.tool = Tool::Line;
    mouse(&mut d, &[&[(40, 40, Some(true))], &[(80, 60, None)]]);
    assert!(!d.screen.pixels[60 * 256 + 80], "not yet");
    mouse(&mut d, &[&[(80, 60, Some(false))]]);
    assert!(d.screen.pixels[40 * 256 + 40] && d.screen.pixels[60 * 256 + 80] && d.screen.pixels[50 * 256 + 60]);
    // Cells selected by a drag, copied and pasted by a click.
    d.tool = Tool::Select;
    mouse(&mut d, &[&[(9, 9, Some(true))], &[(31, 12, None)], &[(31, 12, Some(false))]]);
    assert_eq!(d.selection, Some((1, 1, 3, 1)));
    d.copy();
    d.pasting = true;
    mouse(&mut d, &[&[(160, 160, Some(true))], &[(160, 160, Some(false))]]);
    assert!(!d.pasting);
    assert!((0..24).all(|x| d.screen.pixels[(160 + 2) * 256 + 160 + x] == d.screen.pixels[10 * 256 + 8 + x]));
    assert!(d.screen.pixels[162 * 256 + 162]);
}
