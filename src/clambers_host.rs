//! Clambers in a tab: type "clambers" into Find a command and it opens. The game runs at 50 frames a second while
//! its tab is in front, drawn from its 256×192 screen, scaled up by whole steps.

use super::{Document, Limoniter};
use crate::clambers::{Controls, Game, HEIGHT, PALETTE, WIDTH};
use eframe::egui::{self, Color32, ColorImage, Key, TextureHandle, TextureOptions, Ui};
use std::time::Instant;

/// One frame of the game, in seconds.
const FRAME: f64 = 1.0 / 50.0;

pub(super) struct Playing {
    pub(super) game: Game,
    texture: Option<TextureHandle>,
    last: Option<Instant>,
    /// Time not yet run as frames.
    carry: f64,
}

impl Playing {
    fn new() -> Self {
        Playing { game: Game::new(), texture: None, last: None, carry: 0.0 }
    }
}

/// What the keys say this frame: arrows or WASD to fly, Space to fire (and start), Shift to bomb.
fn controls(ui: &Ui) -> Controls {
    ui.input(|i| Controls {
        up: i.key_down(Key::ArrowUp) || i.key_down(Key::W),
        down: i.key_down(Key::ArrowDown) || i.key_down(Key::S),
        left: i.key_down(Key::ArrowLeft) || i.key_down(Key::A),
        right: i.key_down(Key::ArrowRight) || i.key_down(Key::D),
        fire: i.key_down(Key::Space),
        bomb: i.modifiers.shift,
        start: i.key_pressed(Key::Space) || i.key_pressed(Key::Enter),
    })
}

impl Limoniter {
    /// Opens the game in a tab of its own, or brings its tab to the front if it's open already.
    pub(super) fn open_clambers(&mut self) {
        if let Some(index) = self.docs.iter().position(|d| d.game.is_some()) {
            self.active = index;
            return;
        }
        if self.no_file() {
            self.docs.clear();
        }
        let mut doc = Document::new("Clambers".into());
        doc.text.read_only = true;
        doc.game = Some(Box::new(Playing::new()));
        self.docs.push(doc);
        self.active = self.docs.len() - 1;
    }

    /// Runs the frames due since last time and draws the screen in the text's place.
    pub(super) fn clambers_view(&mut self, ui: &mut Ui) {
        let index = self.active;
        let input = controls(ui);
        let Some(playing) = self.docs[index].game.as_mut() else {
            return;
        };
        let now = Instant::now();
        // Away from the tab, the game waits: no catching up on the time it was behind.
        let gap = playing.last.map_or(FRAME, |last| now.duration_since(last).as_secs_f64()).min(0.1);
        playing.last = Some(now);
        playing.carry += gap;
        let mut first = true;
        while playing.carry >= FRAME {
            playing.carry -= FRAME;
            // A key pressed counts once, not for every frame run to catch up.
            let step = if first { input } else { Controls { start: false, ..input } };
            playing.game.tick(step);
            first = false;
        }
        let screen = playing.game.screen();
        let rgba: Vec<u8> = screen.iter().flat_map(|&c| {
            let [r, g, b] = PALETTE[c as usize & 15];
            [r, g, b, 255]
        }).collect();
        let image = ColorImage::from_rgba_unmultiplied([WIDTH as usize, HEIGHT as usize], &rgba);
        match &mut playing.texture {
            Some(texture) => texture.set(image, TextureOptions::NEAREST),
            None => playing.texture = Some(ui.ctx().load_texture("clambers", image, TextureOptions::NEAREST)),
        }
        let room = ui.available_rect_before_wrap();
        ui.painter().rect_filled(room, 0.0, Color32::from_rgb(4, 4, 8));
        let scale = ((room.width() / WIDTH as f32).min(room.height() / HEIGHT as f32)).floor().max(1.0);
        let size = egui::vec2(WIDTH as f32 * scale, HEIGHT as f32 * scale);
        let rect = egui::Rect::from_center_size(room.center(), size);
        if let Some(texture) = &playing.texture {
            ui.painter().image(texture.id(), rect, egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)), Color32::WHITE);
        }
        ui.ctx().request_repaint();
    }
}

#[cfg(test)]
mod tests {
    use super::super::tests::app_with;

    #[test]
    fn typing_the_name_into_find_a_command_offers_the_game_and_it_opens_in_a_tab() {
        let offered = super::super::menus::hidden_commands("Clambers ");
        assert_eq!(offered.len(), 1);
        assert!(super::super::menus::hidden_commands("scramble").is_empty(), "only the scrambled name opens it");
        assert!(super::super::menus::hidden_commands("clamber").is_empty());
        let (mut app, ctx) = app_with("");
        let _ = ctx;
        let before = app.docs.len();
        app.open_clambers();
        assert!(app.docs[app.active].game.is_some());
        assert_eq!(app.docs[app.active].name(), "Clambers");
        app.open_clambers();
        assert!(app.docs.len() <= before + 1, "a second call brings the same tab to the front");
        assert!(!app.docs[app.active].is_modified());
    }
}
