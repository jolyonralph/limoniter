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
    let time = ui.input(|input| input.time) as f32;

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

    // The scroller: the message in capitals, right to left, each letter on a sine and in a colour of its own.
    let font = FontId::monospace(20.0);
    let text: Vec<char> = format!("{}          ", message.to_uppercase()).chars().collect();
    let advances: Vec<f32> = ui.fonts_mut(|fonts| text.iter().map(|&c| fonts.glyph_width(&font, c)).collect());
    let length: f32 = advances.iter().sum();
    let travelled = (time * 95.0) % (length + rect.width());
    let baseline = rect.bottom() - 28.0;
    let mut x = rect.right() - travelled;
    for (c, advance) in text.iter().zip(&advances) {
        if x > rect.right() {
            break;
        }
        if x + advance >= rect.left() && !c.is_whitespace() {
            let y = baseline + 7.0 * (x * 0.025 + time * 4.0).sin();
            let hue = (x / rect.width() + time * 0.15).rem_euclid(1.0);
            let colour: Color32 = egui::ecolor::Hsva::new(hue, 0.75, 1.0, 1.0).into();
            // A dark shadow first, so the letters stand out over the bars' colours.
            painter.text(Pos2::new(x + 2.0, y + 2.0), egui::Align2::LEFT_CENTER, c.to_string(), font.clone(), Color32::from_black_alpha(200));
            painter.text(Pos2::new(x, y), egui::Align2::LEFT_CENTER, c.to_string(), font.clone(), colour);
        }
        x += advance;
    }
    painter.rect_stroke(rect, 6.0, Stroke::new(1.0, Color32::from_gray(40)), egui::StrokeKind::Inside);
    ui.ctx().request_repaint();
}

/// The logo as a texture, made once.
fn logo(ctx: &egui::Context) -> Option<egui::TextureHandle> {
    let id = egui::Id::unique("about-demo-logo");
    if let Some(logo) = ctx.data(|d| d.get_temp::<egui::TextureHandle>(id)) {
        return Some(logo);
    }
    let mut reader = png::Decoder::new(std::io::Cursor::new(&include_bytes!("../data/about-demo-logo.png")[..])).read_info().ok()?;
    let mut rgba = vec![0; reader.output_buffer_size()?];
    let frame = reader.next_frame(&mut rgba).ok()?;
    rgba.truncate(frame.buffer_size());
    if frame.color_type != png::ColorType::Rgba {
        return None;
    }
    let image = egui::ColorImage::from_rgba_unmultiplied([frame.width as usize, frame.height as usize], &rgba);
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
}
