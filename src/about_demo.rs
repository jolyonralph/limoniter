//! The top of the About box, as an 80s demo would have it: the logo in the middle of a black screen, copper bars
//! bouncing behind it, and a sine scroller along the bottom carrying the line that says what Limoniter is.
//!
//! The copper bars are drawn as an Amiga's copper makes them, a line at a time, each line one colour, shaded from
//! dark at a bar's edges to a white-hot middle. Each bar swings on its own sine, a step behind the one before.

use eframe::egui::{self, Color32, FontId, Pos2, Rect, Sense, Stroke, Ui, Vec2};

/// The screen's height in points; it takes the About box's whole width.
const HEIGHT: f32 = 190.0;
/// The copper bars' colours, as the classic ones went: red, orange, yellow, green, cyan, blue, magenta.
const BARS: [(u8, u8, u8); 7] = [(255, 40, 40), (255, 140, 0), (255, 230, 40), (60, 230, 60), (40, 220, 230), (60, 100, 255), (230, 60, 230)];
/// Each bar's height, in lines of 2 points (the Amiga's low-resolution lines, as the About box draws them).
const BAR_LINES: usize = 9;
const LINE: f32 = 2.0;

/// Draws the screen, and asks for the next frame while it's shown.
pub fn show(ui: &mut Ui, message: &str) {
    let (rect, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), HEIGHT), Sense::hover());
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, 6.0, Color32::BLACK);
    // Time from when the box was opened, so every opening starts the same: bars from the top and the message
    // scrolling in from the right. A gap of more than a moment since the last frame drawn means it was just opened.
    let now = ui.input(|input| input.time);
    let id = egui::Id::unique("about-demo-clock");
    let (mut opened, last) = ui.ctx().data(|d| d.get_temp::<(f64, f64)>(id)).unwrap_or((now, f64::NEG_INFINITY));
    if now - last > 0.5 {
        opened = now;
    }
    ui.ctx().data_mut(|d| d.insert_temp(id, (opened, now)));
    let time = (now - opened) as f32;

    // The copper bars, the last drawn over the others, swinging through the top of the screen behind the logo.
    let swing_top = rect.top() + 8.0;
    let swing = 120.0 - BAR_LINES as f32 * LINE;
    for (index, (red, green, blue)) in BARS.iter().enumerate() {
        let phase = time * 1.7 - index as f32 * 0.42;
        let top = swing_top + swing * (0.5 + 0.5 * phase.sin());
        for line in 0..BAR_LINES {
            let middle = (BAR_LINES - 1) as f32 / 2.0;
            let light = 1.0 - (line as f32 - middle).abs() / (middle + 1.0);
            // Shaded towards black at the edges, and towards white in the middle line, as copper bars are.
            let white = if (line as f32 - middle).abs() < 0.5 { 0.55 } else { 0.0 };
            let shade = |c: u8| (c as f32 * (0.18 + 0.82 * light) * (1.0 - white) + 255.0 * white) as u8;
            let y = top + line as f32 * LINE;
            painter.rect_filled(Rect::from_min_max(Pos2::new(rect.left(), y), Pos2::new(rect.right(), y + LINE)), 0.0, Color32::from_rgb(shade(*red), shade(*green), shade(*blue)));
        }
    }

    // The logo, in the middle of the bars.
    let logo_middle = Pos2::new(rect.center().x, rect.top() + 72.0);
    match logo(ui.ctx()) {
        Some(logo) => {
            let size = logo.size_vec2();
            let width = (rect.width() - 40.0).min(500.0);
            let shown = Vec2::new(width, width * size.y / size.x);
            painter.image(logo.id(), Rect::from_center_size(logo_middle, shown), Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)), Color32::WHITE);
        }
        None => {
            painter.text(logo_middle, egui::Align2::CENTER_CENTER, "LIMONITER", FontId::proportional(48.0), Color32::from_rgb(255, 170, 30));
        }
    }

    // The scroller, as demos did it: the message drawn once in the gold font, then each narrow vertical strip of it
    // put where the sine wave is at that point of the screen, so every letter bends round the wave.
    match scroller(ui.ctx(), message) {
        Some((texture, width)) => {
            let scale = 0.5; // the font's 64-pixel cells shown 32 points high
            let shown_width = width as f32 * scale;
            let travelled = (time * 95.0) % (shown_width + rect.width());
            let start = rect.right() - travelled;
            let height = FONT_CELL as f32 * scale;
            let top = rect.bottom() - height - 18.0;
            let mut shadow = egui::Mesh::with_texture(texture.id());
            let mut letters = egui::Mesh::with_texture(texture.id());
            let mut x = rect.left().max(start);
            while x < rect.right().min(start + shown_width) {
                let u0 = (x - start) / shown_width;
                let u1 = ((x + STRIP - start) / shown_width).min(1.0);
                let y = top + 9.0 * (x * 0.021 + time * 3.2).sin();
                let uv = Rect::from_min_max(Pos2::new(u0, 0.0), Pos2::new(u1, 1.0));
                let strip = Rect::from_min_size(Pos2::new(x, y), Vec2::new(STRIP, height));
                shadow.add_rect_with_uv(strip.translate(Vec2::new(3.0, 3.0)), uv, Color32::from_black_alpha(170));
                letters.add_rect_with_uv(strip, uv, Color32::WHITE);
                x += STRIP;
            }
            painter.add(shadow);
            painter.add(letters);
        }
        None => {
            let font = FontId::monospace(20.0);
            let x = rect.right() - (time * 95.0) % (rect.width() * 4.0);
            painter.text(Pos2::new(x, rect.bottom() - 28.0), egui::Align2::LEFT_CENTER, message, font, Color32::from_rgb(255, 190, 60));
        }
    }
    painter.rect_stroke(rect, 6.0, Stroke::new(1.0, Color32::from_gray(40)), egui::StrokeKind::Inside);
    ui.ctx().request_repaint();
}

/// The gold font: 64-pixel cells, sixteen to a row, holding these characters in this order.
const FONT_CHARS: &str = "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789./+!?-,";
const FONT_CELL: usize = 64;
/// How wide each strip of the scroller is, in points: narrow enough that the letters curve smoothly.
const STRIP: f32 = 2.0;

/// A letter the font hasn't got, as the nearest one it has: accents dropped (é is e), so other languages' messages
/// scroll too. None for a space or anything else.
fn in_font(c: char) -> Option<char> {
    if FONT_CHARS.contains(c) || c == '\'' {
        return Some(c);
    }
    let plain = match c {
        'À'..='Å' => 'A',
        'à'..='å' => 'a',
        'Ç' => 'C',
        'ç' => 'c',
        'È'..='Ë' => 'E',
        'è'..='ë' => 'e',
        'Ì'..='Ï' => 'I',
        'ì'..='ï' => 'i',
        'Ñ' => 'N',
        'ñ' => 'n',
        'Ò'..='Ö' | 'Ø' => 'O',
        'ò'..='ö' | 'ø' => 'o',
        'Ù'..='Ü' => 'U',
        'ù'..='ü' => 'u',
        'Ý' => 'Y',
        'ý' | 'ÿ' => 'y',
        'ß' => 's',
        // An apostrophe is the comma raised to the top of the letters (see `message_pixels`).
        '\'' | '\u{2019}' | '\u{2018}' => '\'',
        ';' | ':' => ',',
        _ => return None,
    };
    Some(plain)
}

/// Pixels of an RGBA picture made from a PNG in the program, or None if it isn't one.
fn rgba_png(bytes: &[u8]) -> Option<(usize, usize, Vec<u8>)> {
    let mut reader = png::Decoder::new(std::io::Cursor::new(bytes)).read_info().ok()?;
    let mut rgba = vec![0; reader.output_buffer_size()?];
    let frame = reader.next_frame(&mut rgba).ok()?;
    rgba.truncate(frame.buffer_size());
    (frame.color_type == png::ColorType::Rgba).then_some((frame.width as usize, frame.height as usize, rgba))
}

/// The message drawn in the gold font, each letter as wide as its own pixels with a little space after: the pixels,
/// and how wide they are.
fn message_pixels(message: &str) -> Option<(usize, Vec<u8>)> {
    let (font_width, _, font) = rgba_png(include_bytes!("../data/about-font.png"))?;
    let per_row = font_width / FONT_CELL;
    let alpha = |x: usize, y: usize| font[(y * font_width + x) * 4 + 3];
    let cell = |c: char| -> Option<(usize, usize)> {
        let index = FONT_CHARS.chars().position(|f| f == c)?;
        Some(((index % per_row) * FONT_CELL, (index / per_row) * FONT_CELL))
    };
    // The first and last rows of a cell that have something in them.
    let rows_used = |(cx, cy): (usize, usize)| -> Option<(usize, usize)> {
        let used: Vec<usize> = (0..FONT_CELL).filter(|&y| (0..FONT_CELL).any(|x| alpha(cx + x, cy + y) > 16)).collect();
        Some((*used.first()?, *used.last()?))
    };
    // The font has no apostrophe, so the comma stands in for it, moved up until its top is level with the capitals'.
    let raise = match (cell(',').and_then(rows_used), cell('A').and_then(rows_used)) {
        (Some((comma_top, _)), Some((capital_top, _))) => comma_top.saturating_sub(capital_top),
        _ => 0,
    };
    // Each character's columns that have something in them, and how far it's moved up.
    let glyph = |c: char| -> Option<(usize, usize, usize, usize)> {
        let (cx, cy) = cell(if c == '\'' { ',' } else { c })?;
        let used: Vec<usize> = (0..FONT_CELL).filter(|&x| (0..FONT_CELL).any(|y| alpha(cx + x, cy + y) > 16)).collect();
        Some((cx + *used.first()?, cy, used.last()? - used.first()? + 1, if c == '\'' { raise } else { 0 }))
    };
    const GAP: usize = 1;
    const SPACE: usize = 24;
    let pieces: Vec<Option<(usize, usize, usize, usize)>> = message.chars().map(|c| in_font(c).and_then(glyph)).collect();
    let width: usize = pieces.iter().map(|piece| piece.map_or(SPACE, |(_, _, w, _)| w + GAP)).sum::<usize>().max(1);
    let mut pixels = vec![0u8; width * FONT_CELL * 4];
    let mut x = 0;
    for piece in pieces {
        match piece {
            Some((gx, gy, w, up)) => {
                for row in up..FONT_CELL {
                    let from = ((gy + row) * font_width + gx) * 4;
                    let to = ((row - up) * width + x) * 4;
                    pixels[to..to + w * 4].copy_from_slice(&font[from..from + w * 4]);
                }
                x += w + GAP;
            }
            None => x += SPACE,
        }
    }
    Some((width, pixels))
}

/// The scroller's message as a texture, made again only when the message changes (a new language).
fn scroller(ctx: &egui::Context, message: &str) -> Option<(egui::TextureHandle, usize)> {
    let id = egui::Id::unique("about-demo-scroller");
    if let Some((_, texture, width)) = ctx.data(|d| d.get_temp::<(String, egui::TextureHandle, usize)>(id)).filter(|(made_for, _, _)| made_for == message) {
        return Some((texture, width));
    }
    let (width, pixels) = message_pixels(message)?;
    let image = egui::ColorImage::from_rgba_unmultiplied([width, FONT_CELL], &pixels);
    let texture = ctx.load_texture("about-demo-scroller", image, egui::TextureOptions::LINEAR);
    ctx.data_mut(|d| d.insert_temp(id, (message.to_string(), texture.clone(), width)));
    Some((texture, width))
}

/// The logo as a texture, made once.
fn logo(ctx: &egui::Context) -> Option<egui::TextureHandle> {
    let id = egui::Id::unique("about-demo-logo");
    if let Some(logo) = ctx.data(|d| d.get_temp::<egui::TextureHandle>(id)) {
        return Some(logo);
    }
    let (width, height, rgba) = rgba_png(include_bytes!("../data/about-demo-logo.png"))?;
    let image = egui::ColorImage::from_rgba_unmultiplied([width, height], &rgba);
    let logo = ctx.load_texture("about-demo-logo", image, egui::TextureOptions::LINEAR);
    ctx.data_mut(|d| d.insert_temp(id, logo.clone()));
    Some(logo)
}

#[cfg(test)]
mod tests {
    use eframe::egui;

    /// The logo is a picture with transparency that decodes, so the About box never falls back to plain text.
    #[test]
    fn the_logo_decodes() {
        let ctx = egui::Context::default();
        let logo = super::logo(&ctx).expect("the logo decodes");
        assert!(logo.size()[0] > logo.size()[1] * 3, "a wide logo");
    }

    /// The gold font has every letter it says, each with pixels in its cell, and a message (accents and all) is
    /// drawn in it with spaces as gaps.
    #[test]
    fn the_scroller_font_draws_messages() {
        for c in super::FONT_CHARS.chars() {
            let (width, _) = super::message_pixels(&c.to_string()).expect("the font decodes");
            assert!(width > super::FONT_CELL / 8 && width < super::FONT_CELL + 8, "{c} is {width} wide");
        }
        let (one, _) = super::message_pixels("I").unwrap();
        let (wide, _) = super::message_pixels("W").unwrap();
        assert!(wide > one, "letters are as wide as they are");
        let (spaced, _) = super::message_pixels("A B").unwrap();
        let (close, _) = super::message_pixels("AB").unwrap();
        assert!(spaced > close);
        assert_eq!(super::in_font('é'), Some('e'));
        assert_eq!(super::in_font('Ü'), Some('U'));
        assert_eq!(super::in_font(' '), None);
        // An apostrophe is the comma, raised: its pixels are in the top half of the line, a comma's in the bottom.
        assert_eq!(super::in_font('\u{2019}'), Some('\''));
        let lit_rows = |text: &str| {
            let (width, pixels) = super::message_pixels(text).unwrap();
            (0..super::FONT_CELL).filter(|&y| (0..width).any(|x| pixels[(y * width + x) * 4 + 3] > 16)).collect::<Vec<_>>()
        };
        let (apostrophe, comma) = (lit_rows("'"), lit_rows(","));
        assert!(!apostrophe.is_empty() && apostrophe.len() == comma.len());
        assert!(apostrophe[0] < super::FONT_CELL / 2 && comma[0] >= super::FONT_CELL / 2, "{apostrophe:?} {comma:?}");
    }
}
