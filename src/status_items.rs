//! The status bar's steady items, shown, hidden and put in the order the person likes (a right-click on the status
//! bar). Notes that only show when they matter (a macro recording, saving, overwriting) aren't among them: they come
//! and go by themselves.

use eframe::egui::{self, Ui};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Item {
    /// Where the cursor is: line, column and the selection.
    Position,
    Language,
    Encoding,
    LineEndings,
    /// The display profile, when it's not plain.
    Profile,
    Size,
    /// Editable or read-only.
    Lock,
}

impl Item {
    /// The order they start in, as they're seen from left to right among the items at the right.
    pub const ALL: [Item; 7] = [Item::Position, Item::Language, Item::Encoding, Item::LineEndings, Item::Profile, Item::Size, Item::Lock];

    pub fn label(self) -> &'static str {
        match self {
            Item::Position => "Where the cursor is",
            Item::Language => "Language",
            Item::Encoding => "Encoding",
            Item::LineEndings => "Line endings",
            Item::Profile => "Display profile",
            Item::Size => "File size",
            Item::Lock => "Editable or read-only",
        }
    }

    fn setting(self) -> &'static str {
        match self {
            Item::Position => "position",
            Item::Language => "language",
            Item::Encoding => "encoding",
            Item::LineEndings => "line_endings",
            Item::Profile => "profile",
            Item::Size => "size",
            Item::Lock => "lock",
        }
    }
}

/// Which items show, and in what order.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Layout {
    /// Every item, in order from left to right.
    pub order: Vec<Item>,
    pub hidden: Vec<Item>,
}

impl Default for Layout {
    fn default() -> Self {
        Layout { order: Item::ALL.to_vec(), hidden: Vec::new() }
    }
}

impl Layout {
    pub fn shows(&self, item: Item) -> bool {
        !self.hidden.contains(&item)
    }

    /// The items at the right of the bar, shown, from the right edge inwards (as they're laid out).
    pub fn right_to_left(&self) -> Vec<Item> {
        self.order.iter().rev().copied().filter(|i| *i != Item::Position && self.shows(*i)).collect()
    }

    /// Moves an item one place left (-1) or right (+1).
    pub fn shift(&mut self, item: Item, by: isize) {
        let Some(at) = self.order.iter().position(|i| *i == item) else { return };
        let to = at as isize + by;
        if to >= 0 && (to as usize) < self.order.len() {
            self.order.swap(at, to as usize);
        }
    }

    pub fn to_settings(&self) -> String {
        let names = |items: &[Item]| items.iter().map(|i| i.setting()).collect::<Vec<_>>().join(",");
        format!("status_order={}\nstatus_hidden={}\n", names(&self.order), names(&self.hidden))
    }

    /// Reads one setting, if it's one of these. An order missing some items gets them back at its end.
    pub fn read_setting(&mut self, name: &str, value: &str) {
        let items = || value.split(',').filter_map(|word| Item::ALL.into_iter().find(|i| i.setting() == word.trim()));
        match name {
            "status_order" => {
                let mut order: Vec<Item> = Vec::new();
                for item in items() {
                    if !order.contains(&item) {
                        order.push(item);
                    }
                }
                let missing: Vec<Item> = Item::ALL.into_iter().filter(|i| !order.contains(i)).collect();
                order.extend(missing);
                self.order = order;
            }
            "status_hidden" => self.hidden = items().collect(),
            _ => {}
        }
    }

    /// The menu a right-click on the status bar opens. Returns whether anything changed.
    pub fn menu(&mut self, ui: &mut Ui) -> bool {
        let before = self.clone();
        ui.label(egui::RichText::new(crate::i18n::tr("Shown in the status bar")).strong());
        let order = self.order.clone();
        for (index, item) in order.iter().enumerate() {
            ui.horizontal(|ui| {
                let mut shown = self.shows(*item);
                if ui.checkbox(&mut shown, crate::i18n::tr(item.label())).changed() {
                    if shown {
                        self.hidden.retain(|i| i != item);
                    } else {
                        self.hidden.push(*item);
                    }
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.add_enabled(index + 1 < order.len(), egui::Button::new(crate::i18n::tr("\u{25B6}")).small()).on_hover_text(crate::i18n::tr("Further right")).clicked() {
                        self.shift(*item, 1);
                    }
                    if ui.add_enabled(index > 0, egui::Button::new(crate::i18n::tr("\u{25C0}")).small()).on_hover_text(crate::i18n::tr("Further left")).clicked() {
                        self.shift(*item, -1);
                    }
                });
            });
        }
        ui.separator();
        if ui.button(crate::i18n::tr("Put them back as they were")).clicked() {
            *self = Layout::default();
        }
        *self != before
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn items_are_ordered_hidden_and_remembered() {
        let mut layout = Layout::default();
        assert_eq!(layout.right_to_left(), [Item::Lock, Item::Size, Item::Profile, Item::LineEndings, Item::Encoding, Item::Language]);
        layout.shift(Item::Lock, -1);
        layout.shift(Item::Language, 1);
        layout.hidden.push(Item::Size);
        layout.shift(Item::Position, -1);
        assert_eq!(layout.right_to_left(), [Item::Lock, Item::Profile, Item::LineEndings, Item::Language, Item::Encoding]);
        let mut read = Layout::default();
        for line in layout.to_settings().lines() {
            let (name, value) = line.split_once('=').unwrap();
            read.read_setting(name, value);
        }
        assert_eq!(read, layout);
        // An old or broken order still has every item.
        read.read_setting("status_order", "size,nonsense,size");
        assert_eq!(read.order.len(), Item::ALL.len());
        assert_eq!(read.order[0], Item::Size);
    }
}
