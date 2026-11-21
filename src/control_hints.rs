//! Control hints for the built-in machines: which of the computer's keys are each machine's joystick, fire
//! buttons and console switches, said in a line under its screen and in a small box over it, and changeable in
//! a window of their own. The choices are kept in the settings file, one line for each key moved from where it
//! starts.
//!
//! The Atari 2600 and the Vectrex have joysticks of their own, which start on the arrow keys as Stella and vecx
//! have them. The Spectrum's is a Kempston joystick, and the ZX80's, ZX81's and Jupiter Ace's the keys their
//! games most often read (5, 6, 7 and 8 for the directions, 0 to fire); those start with no keys, since every key
//! types on them, and the arrows already type the cursor keys. A key given to a joystick there stops typing.
//! No gamepads: Limoniter doesn't read them yet.

use eframe::egui::{self, Key};

/// The built-in machines that have controls to show.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Pad {
    Atari2600,
    Vectrex,
    Spectrum,
    Zx80,
    Zx81,
    JupiterAce,
}

impl Pad {
    pub const ALL: [Pad; 6] = [Pad::Atari2600, Pad::Vectrex, Pad::Spectrum, Pad::Zx80, Pad::Zx81, Pad::JupiterAce];

    pub fn id(self) -> &'static str {
        match self {
            Pad::Atari2600 => "2600",
            Pad::Vectrex => "vectrex",
            Pad::Spectrum => "spectrum",
            Pad::Zx80 => "zx80",
            Pad::Zx81 => "zx81",
            Pad::JupiterAce => "ace",
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Pad::Atari2600 => "Atari 2600",
            Pad::Vectrex => "Vectrex",
            Pad::Spectrum => "ZX Spectrum",
            Pad::Zx80 => "ZX80",
            Pad::Zx81 => "ZX81",
            Pad::JupiterAce => "Jupiter Ace",
        }
    }

    /// Each of the machine's controls: its id in the settings, what it's called, and the key it starts on.
    pub fn controls(self) -> &'static [(&'static str, &'static str, Option<Key>)] {
        match self {
            Pad::Atari2600 => &[
                ("up", "Left joystick up", Some(Key::ArrowUp)),
                ("down", "Left joystick down", Some(Key::ArrowDown)),
                ("left", "Left joystick left", Some(Key::ArrowLeft)),
                ("right", "Left joystick right", Some(Key::ArrowRight)),
                ("fire", "Left joystick fire", Some(Key::Space)),
                ("up2", "Right joystick up", Some(Key::Y)),
                ("down2", "Right joystick down", Some(Key::H)),
                ("left2", "Right joystick left", Some(Key::G)),
                ("right2", "Right joystick right", Some(Key::J)),
                ("fire2", "Right joystick fire", Some(Key::F)),
                ("reset", "Game Reset", Some(Key::Enter)),
                ("select", "Game Select", Some(Key::Backspace)),
            ],
            Pad::Vectrex => &[
                ("up", "Joystick up", Some(Key::ArrowUp)),
                ("down", "Joystick down", Some(Key::ArrowDown)),
                ("left", "Joystick left", Some(Key::ArrowLeft)),
                ("right", "Joystick right", Some(Key::ArrowRight)),
                ("button1", "Button 1", Some(Key::A)),
                ("button2", "Button 2", Some(Key::S)),
                ("button3", "Button 3", Some(Key::D)),
                ("button4", "Button 4", Some(Key::F)),
            ],
            Pad::Spectrum => &[
                ("up", "Kempston joystick up", None),
                ("down", "Kempston joystick down", None),
                ("left", "Kempston joystick left", None),
                ("right", "Kempston joystick right", None),
                ("fire", "Kempston joystick fire", None),
            ],
            Pad::Zx80 | Pad::Zx81 | Pad::JupiterAce => &[
                ("up", "Up (the 7 key)", None),
                ("down", "Down (the 6 key)", None),
                ("left", "Left (the 5 key)", None),
                ("right", "Right (the 8 key)", None),
                ("fire", "Fire (the 0 key)", None),
            ],
        }
    }

    /// The machine's own keys, as the computer's keyboard types them, for the hints.
    fn own_keys(self) -> &'static str {
        match self {
            Pad::Atari2600 | Pad::Vectrex => "",
            Pad::Spectrum => "Typing goes to the Spectrum: Ctrl is Symbol Shift, Esc is Break, Backspace deletes, and the arrows are its cursor keys (and a cursor joystick).",
            Pad::Zx80 => "Typing goes to the ZX80: Shift is its SHIFT, Backspace rubs out, the arrows move the cursor, and Esc or Space is BREAK.",
            Pad::Zx81 => "Typing goes to the ZX81: Shift is its SHIFT, Backspace rubs out, the arrows move the cursor, and Esc or Space is BREAK.",
            Pad::JupiterAce => "Typing goes to the Jupiter Ace: Shift is its SHIFT, Ctrl or Alt its SYMBOL SHIFT, Backspace deletes, the arrows move the cursor and Esc is BREAK.",
        }
    }
}

/// The keys chosen for every machine's controls: only those moved from where they start are kept.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ControlKeys {
    /// (machine, control id) to the key it's on now, None for none.
    moved: Vec<((Pad, &'static str), Option<Key>)>,
}

impl ControlKeys {
    /// The key a control is on.
    pub fn key(&self, pad: Pad, control: &str) -> Option<Key> {
        if let Some((_, key)) = self.moved.iter().find(|((p, c), _)| *p == pad && *c == control) {
            return *key;
        }
        pad.controls().iter().find(|(id, ..)| *id == control).and_then(|(_, _, key)| *key)
    }

    /// The control a key is on, if any.
    pub fn control_for(&self, pad: Pad, key: Key) -> Option<&'static str> {
        pad.controls().iter().map(|(id, ..)| *id).find(|id| self.key(pad, id) == Some(key))
    }

    /// Puts a control on a key (or on none). A key can be only one control's, so any other on it is taken off.
    pub fn set(&mut self, pad: Pad, control: &'static str, key: Option<Key>) {
        if let Some(key) = key {
            for (other, ..) in pad.controls() {
                if *other != control && self.key(pad, other) == Some(key) {
                    self.put(pad, other, None);
                }
            }
        }
        self.put(pad, control, key);
    }

    fn put(&mut self, pad: Pad, control: &'static str, key: Option<Key>) {
        self.moved.retain(|((p, c), _)| !(*p == pad && *c == control));
        let start = pad.controls().iter().find(|(id, ..)| *id == control).and_then(|(_, _, key)| *key);
        if key != start {
            self.moved.push(((pad, control), key));
        }
    }

    /// Every control of a machine back on the key it starts on.
    pub fn reset(&mut self, pad: Pad) {
        self.moved.retain(|((p, _), _)| *p != pad);
    }

    pub fn to_settings(&self) -> String {
        self.moved.iter().map(|((pad, control), key)| format!("control_key={}.{control}={}\n", pad.id(), key.map_or("none", |k| k.name()))).collect()
    }

    pub fn from_settings(text: &str) -> Self {
        let mut keys = Self::default();
        for value in text.lines().filter_map(|line| line.strip_prefix("control_key=")) {
            let Some((place, key)) = value.split_once('=') else { continue };
            let Some((pad, control)) = place.split_once('.') else { continue };
            let Some(pad) = Pad::ALL.into_iter().find(|p| p.id() == pad) else { continue };
            let Some((control, ..)) = pad.controls().iter().find(|(id, ..)| *id == control) else { continue };
            let key = if key == "none" {
                None
            } else {
                match Key::from_name(key) {
                    Some(key) => Some(key),
                    None => continue,
                }
            };
            keys.put(pad, control, key);
        }
        keys
    }

    /// The controls as words: each key and what it is, for the line under the screen and the box over it.
    pub fn hint(&self, pad: Pad) -> String {
        let shown: Vec<String> = pad.controls().iter().filter_map(|(id, label, _)| self.key(pad, id).map(|key| crate::i18n::tf!("{} {}", key_name(key), crate::i18n::tr(label)))).collect();
        let mut words = String::from(crate::i18n::tr(pad.own_keys()));
        if !words.is_empty() {
            words.push(' ');
        }
        if shown.is_empty() {
            words.push_str(crate::i18n::tr("No keys are its joystick: Controls\u{2026} chooses some."));
        } else {
            words.push_str(&crate::i18n::tf!("{}.", shown.join(", ")));
        }
        words
    }
}

/// A key's name as it's printed on the keyboard.
pub fn key_name(key: Key) -> String {
    match key {
        Key::ArrowUp => "\u{2191}".into(),
        Key::ArrowDown => "\u{2193}".into(),
        Key::ArrowLeft => "\u{2190}".into(),
        Key::ArrowRight => "\u{2192}".into(),
        other => crate::i18n::tr(other.name()).into(),
    }
}

/// The controls window: which machine's, and the control waiting for a key to be pressed, if one is.
pub struct Editor {
    pub pad: Pad,
    waiting: Option<&'static str>,
}

impl Editor {
    pub fn new(pad: Pad) -> Self {
        Self { pad, waiting: None }
    }

    /// Draws the window. Gives back whether a key was changed, and whether it's still open.
    pub fn show(&mut self, ctx: &egui::Context, keys: &mut ControlKeys) -> (bool, bool) {
        let mut changed = false;
        let mut open = true;
        // A key pressed while a control waits for one goes to it (Esc gives up waiting).
        if let Some(control) = self.waiting {
            let pressed = ctx.input(|i| i.events.iter().find_map(|e| match e {
                egui::Event::Key { key, pressed: true, repeat: false, .. } => Some(*key),
                _ => None,
            }));
            if let Some(key) = pressed {
                if key != Key::Escape {
                    keys.set(self.pad, control, Some(key));
                    changed = true;
                }
                self.waiting = None;
                ctx.input_mut(|i| i.events.clear());
            }
        }
        let pad = self.pad;
        crate::fit::Dialog::new(crate::i18n::tf!("Controls of the {}", crate::i18n::tr(pad.name()))).open(&mut open).collapsible(false).resizable(false).show(ctx, |ui| {
            ui.label(crate::i18n::tr("Click a control's key, then press the key it's to be on. Esc leaves it as it was."));
            ui.add_space(6.0);
            egui::Grid::new("control-keys").num_columns(3).spacing([12.0, 6.0]).show(ui, |ui| {
                for (id, label, _) in pad.controls() {
                    ui.label(crate::i18n::tr(label));
                    let shown = if self.waiting == Some(*id) {
                        crate::i18n::tr("Press a key\u{2026}").to_string()
                    } else {
                        keys.key(pad, id).map_or_else(|| crate::i18n::tr("None").to_string(), key_name)
                    };
                    if ui.button(shown).clicked() {
                        self.waiting = Some(*id);
                    }
                    if keys.key(pad, id).is_some() && ui.small_button(crate::i18n::tr("None")).on_hover_text(crate::i18n::tr("Takes the key off this control")).clicked() {
                        keys.set(pad, id, None);
                        changed = true;
                    }
                    ui.end_row();
                }
            });
            ui.add_space(6.0);
            if ui.button(crate::i18n::tr("Keys as they started")).clicked() {
                keys.reset(pad);
                changed = true;
            }
        });
        (changed, open)
    }
}

/// The small box of hints drawn over a machine's screen, in its top left corner.
pub fn overlay(ui: &egui::Ui, screen: egui::Rect, words: &str) {
    let painter = ui.painter_at(screen);
    let font = egui::FontId::proportional(12.0);
    let wrap = (screen.width() - 16.0).max(80.0);
    let galley = painter.layout(words.to_string(), font, egui::Color32::WHITE, wrap);
    let rect = egui::Rect::from_min_size(screen.min + egui::vec2(6.0, 6.0), galley.size() + egui::vec2(8.0, 6.0));
    painter.rect_filled(rect, 4.0, egui::Color32::from_black_alpha(190));
    painter.galley(rect.min + egui::vec2(4.0, 3.0), galley, egui::Color32::WHITE);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn controls_start_on_their_keys_and_move_to_others() {
        let mut keys = ControlKeys::default();
        assert_eq!(keys.key(Pad::Atari2600, "fire"), Some(Key::Space));
        assert_eq!(keys.control_for(Pad::Atari2600, Key::ArrowLeft), Some("left"));
        assert_eq!(keys.key(Pad::Spectrum, "fire"), None, "the Spectrum's joystick starts with no keys");
        keys.set(Pad::Atari2600, "fire", Some(Key::Z));
        assert_eq!(keys.control_for(Pad::Atari2600, Key::Z), Some("fire"));
        assert_eq!(keys.control_for(Pad::Atari2600, Key::Space), None);
        // A key already a control's moves from it.
        keys.set(Pad::Atari2600, "reset", Some(Key::Z));
        assert_eq!(keys.key(Pad::Atari2600, "fire"), None);
        assert_eq!(keys.control_for(Pad::Atari2600, Key::Z), Some("reset"));
        // Each machine's are its own.
        assert_eq!(keys.key(Pad::Vectrex, "button4"), Some(Key::F));
        keys.reset(Pad::Atari2600);
        assert_eq!(keys, ControlKeys::default());
    }

    #[test]
    fn the_keys_chosen_survive_the_settings_file() {
        let mut keys = ControlKeys::default();
        keys.set(Pad::Spectrum, "fire", Some(Key::Space));
        keys.set(Pad::Spectrum, "up", Some(Key::Q));
        keys.set(Pad::Vectrex, "button1", None);
        keys.set(Pad::Zx81, "left", Some(Key::ArrowLeft));
        let text = keys.to_settings();
        assert!(text.contains("control_key=spectrum.fire=Space\n") && text.contains("control_key=vectrex.button1=none\n"), "{text}");
        let back = ControlKeys::from_settings(&format!("theme=dark\n{text}control_key=nonsense.up=Q\ncontrol_key=2600.up=NoSuchKey\n"));
        assert_eq!(back, keys);
        assert_eq!(keys.to_settings().lines().count(), 4, "keys left where they start aren't written");
    }

    #[test]
    fn the_hint_says_every_key_and_what_it_does() {
        let mut keys = ControlKeys::default();
        let hint = keys.hint(Pad::Atari2600);
        assert!(hint.starts_with("\u{2191} Left joystick up, \u{2193} Left joystick down"), "{hint}");
        assert!(hint.contains("Space Left joystick fire") && hint.ends_with("Backspace Game Select."), "{hint}");
        let hint = keys.hint(Pad::Zx81);
        assert!(hint.starts_with("Typing goes to the ZX81") && hint.contains("No keys are its joystick"), "{hint}");
        keys.set(Pad::Zx81, "fire", Some(Key::Space));
        assert!(keys.hint(Pad::Zx81).ends_with("Space Fire (the 0 key)."));
    }
}
