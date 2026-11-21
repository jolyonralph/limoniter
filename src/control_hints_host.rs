//! The window's side of the control hints (see `control_hints`): the Controls… button and the line each built-in
//! machine's panel shows under its screen, the box of hints over the screen while the pointer is on it and it
//! hasn't the keyboard, and the window where the keys are changed.

use super::*;

impl Limoniter {
    /// The window that changes the keys, while it's open; the settings are saved when a key changes.
    pub(super) fn control_keys_window(&mut self, ctx: &egui::Context) {
        let Some(editor) = self.control_editor.as_mut() else {
            return;
        };
        let (changed, open) = editor.show(ctx, &mut self.control_keys);
        if !open {
            self.control_editor = None;
        }
        if changed {
            self.save_settings_if_changed();
        }
    }

    /// Whether the controls window is waiting for a key, which the machines then leave alone.
    pub(super) fn choosing_a_control_key(&self) -> bool {
        self.control_editor.is_some()
    }
}

/// The hints over a machine's screen: shown while the pointer is on it and the screen hasn't the keyboard, so
/// what the keys do is seen before playing.
pub(super) fn screen_hints(ui: &Ui, response: &egui::Response, focused: bool, words: &str) {
    if response.hovered() && !focused {
        crate::control_hints::overlay(ui, response.rect, words);
    }
}
