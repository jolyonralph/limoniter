//! The window's side of PICO-8 and TIC-80 carts: a window beside the cart's code showing its sprite sheet, its
//! map and its sounds, read again from the text whenever it changes.

use super::*;
use crate::fantasy_carts::{Cart, Console};

const TITLE: &str = "PICO-8 and TIC-80 cart";

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum Tab {
    #[default]
    Sprites,
    Map,
    Sounds,
    Parts,
}

/// The window, and the cart as last read.
#[derive(Default)]
pub struct CartView {
    tab: Tab,
    /// The document and its text's version the cart was read from.
    read_from: Option<(u64, u64)>,
    cart: Option<Cart>,
    sheet: Option<egui::TextureHandle>,
    map: Option<egui::TextureHandle>,
}

impl Limoniter {
    pub(super) fn open_cart_view(&mut self) {
        if self.cart_view.is_none() {
            self.cart_view = Some(Box::default());
        }
    }

    pub(super) fn cart_view_window(&mut self, ctx: &Context) {
        let Some(mut view) = self.cart_view.take() else {
            return;
        };
        // Read again when the document in front, or its text, has changed.
        let doc = &self.docs[self.active];
        let now = (doc.id, doc.text.version());
        if view.read_from != Some(now) {
            view.read_from = Some(now);
            let text = if doc.hex_mode { String::new() } else { doc.text.all_lines().join("\n") };
            view.cart = crate::fantasy_carts::console_of(&doc.name(), &text).map(|console| crate::fantasy_carts::read(console, &text));
            view.sheet = None;
            view.map = None;
        }
        let mut open = true;
        let mut go_to = None;
        egui::Window::new(crate::i18n::tr(TITLE)).open(&mut open).default_pos([600.0, 80.0]).default_size([440.0, 480.0]).show(ctx, |ui| {
            go_to = view.draw(ui);
        });
        if let Some(line) = go_to {
            let doc = &mut self.docs[self.active];
            doc.text.move_caret(line, 0, false);
            self.pending_focus = true;
        }
        if open {
            self.cart_view = Some(view);
        }
    }
}

impl CartView {
    /// For screenshots: a tab by its name.
    pub(super) fn show_tab(&mut self, name: &str) {
        self.tab = match name {
            "map" => Tab::Map,
            "sounds" => Tab::Sounds,
            "parts" => Tab::Parts,
            _ => Tab::Sprites,
        };
    }

    /// Draws the window; gives back a line to go to, when a part's chosen.
    fn draw(&mut self, ui: &mut Ui) -> Option<usize> {
        let Some(cart) = &self.cart else {
            ui.label(crate::i18n::tr("The file in front isn't a PICO-8 cart (.p8) or a TIC-80 cart saved as text (a .lua with -- script: at its top)."));
            return None;
        };
        let console = cart.console;
        ui.horizontal(|ui| {
            ui.strong(console.name());
            ui.separator();
            for (tab, name) in [(Tab::Sprites, "Sprites"), (Tab::Map, "Map"), (Tab::Sounds, "Sounds"), (Tab::Parts, "Parts")] {
                ui.selectable_value(&mut self.tab, tab, crate::i18n::tr(name));
            }
        });
        ui.separator();
        let mut go_to = None;
        egui::ScrollArea::both().auto_shrink([false, false]).show(ui, |ui| match self.tab {
            Tab::Sprites => {
                let texture = self.sheet.get_or_insert_with(|| picture(ui, "cart-sheet", cart.sheet_width, cart.sheet_height(), &crate::fantasy_carts::sheet_rgb(cart)));
                let scale = if cart.sheet_height() > 128 { 2.0 } else { 3.0 };
                let size = egui::vec2(cart.sheet_width as f32, cart.sheet_height() as f32) * scale;
                let response = ui.add(egui::Image::new((texture.id(), size)).sense(egui::Sense::hover()));
                if let Some(at) = response.hover_pos() {
                    let cell = ((at - response.rect.min) / (8.0 * scale)).floor();
                    let number = cell.y as usize * (cart.sheet_width / 8) + cell.x as usize;
                    let what = if console == Console::Tic80 && number >= 256 { crate::i18n::tf!("Sprite {}", number - 256) } else { crate::i18n::tf!("Sprite {number}") };
                    response.on_hover_text(what);
                }
                let used = (0..cart.sheet.len() / 64).filter(|n| cart.sprite_used(*n)).count();
                ui.label(crate::i18n::tf!("{used} sprites drawn."));
            }
            Tab::Map => {
                let (width, height) = (cart.map_width * 8, cart.map_height() * 8);
                let texture = self.map.get_or_insert_with(|| picture(ui, "cart-map", width, height, &crate::fantasy_carts::map_rgb(cart)));
                ui.add(egui::Image::new((texture.id(), egui::vec2(width as f32, height as f32))));
                if console == Console::Pico8 {
                    ui.label(crate::i18n::tr("The map's lower half (rows 32 to 63) shares its memory with the sprite sheet's lower half."));
                }
            }
            Tab::Sounds => sounds(ui, cart),
            Tab::Parts => {
                for (name, line) in &cart.sections {
                    if ui.link(crate::i18n::tf!("{name}, line {}", line + 1)).clicked() {
                        go_to = Some(*line);
                    }
                }
            }
        });
        go_to
    }
}

fn picture(ui: &Ui, name: &str, width: usize, height: usize, rgb: &[u8]) -> egui::TextureHandle {
    ui.ctx().load_texture(name, egui::ColorImage::from_rgb([width, height], rgb), egui::TextureOptions::NEAREST)
}

/// PICO-8's sounds, each drawn as its notes' pitches (bars coloured by volume), and its music's patterns; for
/// TIC-80, which sounds there are.
fn sounds(ui: &mut Ui, cart: &Cart) {
    if cart.sounds.is_empty() {
        ui.label(crate::i18n::tr("This cart has no sounds."));
        return;
    }
    if cart.console == Console::Tic80 {
        let numbers: Vec<String> = cart.sounds.iter().map(|s| format!("{:02}", s.number)).collect();
        ui.label(crate::i18n::tf!("Sound effects: {}", numbers.join(", ")));
        return;
    }
    for sound in cart.sounds.iter().filter(|s| s.notes.iter().any(|n| n.volume > 0)) {
        ui.horizontal(|ui| {
            ui.monospace(format!("{:02}", sound.number));
            let (rect, response) = ui.allocate_exact_size(egui::vec2(32.0 * 6.0, 32.0), egui::Sense::hover());
            let painter = ui.painter_at(rect);
            painter.rect_filled(rect, 0.0, egui::Color32::from_gray(24));
            for (at, note) in sound.notes.iter().enumerate().filter(|(_, n)| n.volume > 0) {
                let height = (f32::from(note.pitch) + 1.0) / 64.0 * rect.height();
                let colour = crate::fantasy_carts::PICO8_PALETTE[[1, 13, 12, 12, 11, 10, 9, 8][usize::from(note.volume & 7)]];
                let x = rect.left() + at as f32 * 6.0;
                painter.rect_filled(egui::Rect::from_min_max(egui::pos2(x, rect.bottom() - height), egui::pos2(x + 5.0, rect.bottom())), 0.0, egui::Color32::from_rgb(colour[0], colour[1], colour[2]));
            }
            let first = sound.notes.iter().find(|n| n.volume > 0).map(|n| crate::fantasy_carts::note_name(n.pitch)).unwrap_or_default();
            response.on_hover_text(crate::i18n::tf!("Starts on {first}"));
            ui.label(crate::i18n::tf!("speed {}", sound.speed));
        });
    }
    let used: Vec<&crate::fantasy_carts::Pattern> = cart.music.iter().filter(|p| p.channels.iter().any(Option::is_some)).collect();
    if !used.is_empty() {
        ui.separator();
        ui.strong(crate::i18n::tr("Music"));
        for pattern in used {
            let channels: Vec<String> = pattern.channels.iter().map(|c| c.map_or_else(|| "--".to_string(), |n| format!("{n:02}"))).collect();
            ui.monospace(format!("{:02}: {}", pattern.number, channels.join(" ")));
        }
    }
}
