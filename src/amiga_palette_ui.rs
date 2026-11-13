//! The colour table under an Amiga ANSI preview: the sixteen pens, presets for Workbench 1.0–1.3 and 2.04–3.x,
//! and for the pen chosen, its colour from the Amiga's 4096, picked from them all or typed as three hex digits.

use crate::ansi_art::{AMIGA_PRESETS, AmigaPalette, amiga_colour};
use crate::theme;
use eframe::egui::{self, Color32, RichText, Sense, Ui, vec2};

/// What's being edited: the pen chosen, and what's typed for it.
#[derive(Default)]
pub struct PaletteEditor {
    pub pen: Option<usize>,
    pub hex: String,
}

/// An Amiga colour from what's typed: three hex digits, with or without a #; or six, rounded to the Amiga's four
/// bits a channel.
pub fn parse_colour(text: &str) -> Option<u16> {
    let digits = text.trim().trim_start_matches('#');
    if !digits.chars().all(|c| c.is_ascii_hexdigit()) {
        return None;
    }
    match digits.len() {
        3 => u16::from_str_radix(digits, 16).ok(),
        6 => {
            let value = u32::from_str_radix(digits, 16).ok()?;
            let four = |shift: u32| ((((value >> shift) & 0xFF) + 8) / 17).min(15) as u16;
            Some(four(16) << 8 | four(8) << 4 | four(0))
        }
        _ => None,
    }
}

/// Draws the table; true if the palette changed.
pub fn draw(ui: &mut Ui, palette: &mut AmigaPalette, editor: &mut PaletteEditor) -> bool {
    let mut changed = false;
    ui.horizontal_wrapped(|ui| {
        ui.label(RichText::new("Amiga palette").color(theme::text_dim()).size(12.5));
        let current = AMIGA_PRESETS.iter().find(|(_, pens)| *pens == palette.pens).map_or("Your own", |(name, _)| name);
        egui::ComboBox::from_id_salt("amiga-preset").selected_text(current).show_ui(ui, |ui| {
            for (name, pens) in AMIGA_PRESETS {
                if ui.selectable_label(palette.pens == pens, name).clicked() && palette.pens != pens {
                    palette.pens = pens;
                    changed = true;
                    if let Some(pen) = editor.pen {
                        editor.hex = format!("{:03X}", palette.pens[pen]);
                    }
                }
            }
        });
        changed |= ui.checkbox(&mut palette.bold_high, "Bold picks pens 8\u{2013}15").on_hover_text("As terminal programs on a 16-colour screen drew bold text. Without it, bold text keeps its pen, as the Amiga's console has it.").changed();
    });
    // Two rows of eight pens.
    for row in 0..2 {
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 3.0;
            for (pen, value) in palette.pens.iter().copied().enumerate().skip(row * 8).take(8) {
                let (rect, response) = ui.allocate_exact_size(vec2(34.0, 24.0), Sense::click());
                let colour = amiga_colour(value);
                ui.painter().rect_filled(rect, 2.0, colour);
                let chosen = editor.pen == Some(pen);
                let stroke = if chosen { egui::Stroke::new(2.0, theme::accent()) } else { egui::Stroke::new(1.0, theme::text_faint()) };
                ui.painter().rect_stroke(rect, 2.0, stroke, egui::StrokeKind::Inside);
                let ink = if colour.r() as u16 + colour.g() as u16 + colour.b() as u16 > 380 { Color32::BLACK } else { Color32::WHITE };
                ui.painter().text(rect.center(), egui::Align2::CENTER_CENTER, pen.to_string(), egui::FontId::proportional(11.0), ink);
                if response.on_hover_text(format!("Pen {pen}: {value:03X}. Click to change it.")).clicked() {
                    if chosen {
                        editor.pen = None;
                    } else {
                        editor.pen = Some(pen);
                        editor.hex = format!("{value:03X}");
                    }
                }
            }
        });
    }
    let Some(pen) = editor.pen else {
        return changed;
    };
    ui.add_space(4.0);
    let mut chosen: Option<u16> = None;
    let mut typed = false;
    ui.horizontal(|ui| {
        ui.label(format!("Pen {pen}:"));
        let field = ui.add(egui::TextEdit::singleline(&mut editor.hex).desired_width(60.0).hint_text("f8f"));
        if field.changed()
            && let Some(colour) = parse_colour(&editor.hex)
        {
            chosen = Some(colour);
            typed = true;
        }
        if parse_colour(&editor.hex).is_none() && !editor.hex.trim().is_empty() {
            ui.label(RichText::new("Three hex digits, such as f8f").color(theme::rainbow_mistake()).size(12.0));
        }
        if ui.small_button("Done").clicked() {
            editor.pen = None;
        }
    });
    let value = palette.pens[pen];
    let (mut r, mut g, mut b) = ((value >> 8) & 15, (value >> 4) & 15, value & 15);
    let mut slid = false;
    for (name, channel) in [("Red", &mut r), ("Green", &mut g), ("Blue", &mut b)] {
        slid |= ui.add(egui::Slider::new(channel, 0..=15).text(name)).changed();
    }
    if slid {
        chosen = Some(r << 8 | g << 4 | b);
    }
    // All 4096: sixteen blocks, one for each level of red, each sixteen greens down by sixteen blues across.
    let cell = 4.0;
    let (rect, response) = ui.allocate_exact_size(vec2(cell * 64.0 + 6.0, cell * 64.0 + 6.0), Sense::click());
    let painter = ui.painter_at(rect);
    let place = |red: u16, green: u16, blue: u16| {
        let block_x = f32::from(red % 4) * (cell * 16.0 + 2.0);
        let block_y = f32::from(red / 4) * (cell * 16.0 + 2.0);
        egui::pos2(rect.min.x + block_x + f32::from(blue) * cell, rect.min.y + block_y + f32::from(green) * cell)
    };
    for red in 0..16u16 {
        for green in 0..16u16 {
            for blue in 0..16u16 {
                painter.rect_filled(egui::Rect::from_min_size(place(red, green, blue), vec2(cell, cell)), 0.0, amiga_colour(red << 8 | green << 4 | blue));
            }
        }
    }
    let now = place(r, g, b);
    painter.rect_stroke(egui::Rect::from_min_size(now, vec2(cell, cell)).expand(1.0), 0.0, egui::Stroke::new(1.5, Color32::WHITE), egui::StrokeKind::Outside);
    if let Some(at) = response.interact_pointer_pos().filter(|_| response.clicked() || response.dragged()) {
        let x = at.x - rect.min.x;
        let y = at.y - rect.min.y;
        let block = cell * 16.0 + 2.0;
        let (bx, by) = ((x / block) as u16, (y / block) as u16);
        let (inner_x, inner_y) = (x - f32::from(bx) * block, y - f32::from(by) * block);
        if bx < 4 && by < 4 && inner_x < cell * 16.0 && inner_y < cell * 16.0 {
            let red = by * 4 + bx;
            chosen = Some(red << 8 | ((inner_y / cell) as u16).min(15) << 4 | ((inner_x / cell) as u16).min(15));
        }
    }
    response.on_hover_text("The Amiga's 4096 colours: a block for each level of red, green down and blue across");
    if let Some(colour) = chosen.filter(|colour| *colour != palette.pens[pen]) {
        palette.pens[pen] = colour;
        // What's typed stays as it's typed; a colour picked or slid to is shown in hex.
        if !typed {
            editor.hex = format!("{colour:03X}");
        }
        changed = true;
    }
    changed
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn colours_are_typed_as_the_amiga_writes_them() {
        assert_eq!(parse_colour("f8f"), Some(0xF8F));
        assert_eq!(parse_colour(" #05A "), Some(0x05A));
        assert_eq!(parse_colour("ff8800"), Some(0xF80), "six digits are rounded to the Amiga's four bits");
        assert_eq!(parse_colour("fg0"), None);
        assert_eq!(parse_colour("f8"), None);
    }
}
