//! A tab for each open file, drawn as a soft pill with a line under the one in front.

use crate::theme;
use eframe::egui::{self, Align2, CornerRadius, FontId, Sense, Stroke, Ui, pos2, vec2};

/// One open file's tab. Returns (chosen, close asked for). Unsaved changes show as a dot, which becomes the close
/// button when the pointer is over the tab.
pub fn file_tab(ui: &mut Ui, title: &str, file_name: &str, modified: bool, selected: bool) -> (bool, bool) {
    let font = FontId::proportional(13.0);
    let text_colour = if selected { theme::text() } else { theme::text_dim() };
    let galley = ui.painter().layout_no_wrap(title.to_string(), font, text_colour);
    // The file's badge before its name.
    let icon = crate::file_icons::width(13.0) + 4.0;
    let size = vec2(galley.size().x + 46.0 + icon, 30.0);
    let (rect, response) = ui.allocate_exact_size(size, Sense::click());
    let top_corners = CornerRadius { nw: 8, ne: 8, sw: 0, se: 0 };
    let hovered = response.hovered();
    if selected {
        ui.painter().rect_filled(rect, top_corners, theme::field());
        let line = egui::Rect::from_min_max(pos2(rect.min.x + 10.0, rect.max.y - 2.5), pos2(rect.max.x - 10.0, rect.max.y - 0.5));
        ui.painter().rect_filled(line, 1.0, theme::accent());
    } else if hovered {
        ui.painter().rect_filled(rect.shrink2(vec2(0.0, 2.0)), 7.0, theme::hover());
    }
    crate::file_icons::paint(ui.painter(), egui::Rect::from_min_size(pos2(rect.min.x + 11.0, rect.min.y), vec2(icon, rect.height())), file_name, 13.0);
    ui.painter().galley(pos2(rect.min.x + 11.0 + icon, rect.center().y - galley.size().y / 2.0), galley, text_colour);

    let close_rect = egui::Rect::from_center_size(pos2(rect.max.x - 17.0, rect.center().y), vec2(18.0, 18.0));
    let close = ui.interact(close_rect, response.id.with("close"), Sense::click());
    let showing_close = close.hovered() || hovered || (selected && !modified);
    if modified && !showing_close {
        ui.painter().circle_filled(close_rect.center(), 3.5, theme::accent());
    } else if showing_close {
        if close.hovered() {
            ui.painter().rect_filled(close_rect, 5.0, theme::border());
        }
        let stroke = Stroke::new(1.4, if close.hovered() { theme::text() } else { theme::text_faint() });
        let c = close_rect.center();
        ui.painter().line_segment([c + vec2(-3.5, -3.5), c + vec2(3.5, 3.5)], stroke);
        ui.painter().line_segment([c + vec2(3.5, -3.5), c + vec2(-3.5, 3.5)], stroke);
    }
    crate::access::button(&close, &format!("Close {title}"));
    let closing = close.on_hover_text(if modified { crate::i18n::tr("Close (it has unsaved changes)") } else { crate::i18n::tr("Close") }).clicked();
    let name = if modified { crate::i18n::tf!("{title}, with unsaved changes") } else { title.to_string() };
    crate::access::chosen(&response, egui::Role::Tab, selected, &name);
    let response = if modified { response.on_hover_text(name) } else { response };
    let _ = Align2::CENTER_CENTER;
    (response.clicked() && !closing, closing)
}
