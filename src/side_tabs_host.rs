//! The window's side of tabs down the side: the panel that holds the list, and moving a tab to a new place.

use super::*;
use crate::side_tabs;

/// One tab of the list, with what's needed to draw it, found before any drawing so the window isn't borrowed while
/// the list is being drawn.
struct Entry {
    doc: usize,
    number: u32,
    name: String,
    file_name: String,
    modified: bool,
    selected: bool,
    stripe: Option<egui::Color32>,
}

impl Limoniter {
    /// Whether the tabs are down the side now.
    pub(super) fn side_tabs_showing(&self) -> bool {
        self.side_tabs.on && self.show_file_tabs && !self.no_file()
    }

    /// Moves the tab at `from` into the gap `slot` (the gap before the tab with that number), keeping the one in
    /// front in front.
    pub(super) fn move_tab(&mut self, from: usize, slot: usize) {
        let to = side_tabs::move_into_gap(&mut self.docs, from, slot);
        self.active = side_tabs::follow(self.active, from, to);
    }

    fn side_entries(&self) -> Vec<Entry> {
        let mut entries = Vec::new();
        for (i, doc) in self.docs.iter().enumerate() {
            let numbers = doc.window_numbers();
            let several = numbers.len() > 1;
            for number in numbers {
                let name = if several { format!("{}:{number}", doc.name()) } else { doc.name() };
                let name = if doc.merge.is_some() { format!("Merge: {name}") } else { name };
                entries.push(Entry {
                    doc: i,
                    number: number as u32,
                    name,
                    file_name: doc.name(),
                    modified: doc.is_modified(),
                    selected: i == self.active && number == doc.window_number,
                    stripe: self.doc_look(i).tint.map(crate::file_colours::Tint::mark),
                });
            }
        }
        entries
    }

    /// The list of tabs, in a panel at the left or the right.
    pub(super) fn side_tabs_panel(&mut self, ui: &mut Ui) {
        let entries = self.side_entries();
        let paths: Vec<Option<PathBuf>> = entries.iter().map(|entry| self.docs[entry.doc].path.clone()).collect();
        let borrowed: Vec<Option<&Path>> = paths.iter().map(|path| path.as_deref()).collect();
        let folders = side_tabs::group_by_folder(&borrowed);
        let (mut chosen, mut close, mut pick, mut moved) = (None, None, None, None);
        let panel = if self.side_tabs.right { Panel::right("side-tabs") } else { Panel::left("side-tabs") };
        panel.default_size(190.0).size_range(110.0..=520.0).resizable(true).show_separator_line(true).frame(egui::Frame::new().fill(theme::window()).inner_margin(egui::Margin { left: 4, right: 4, top: 6, bottom: 4 })).show(ui, |ui| {
            egui::ScrollArea::vertical().id_salt("side-tabs-scroll").auto_shrink([false, false]).show(ui, |ui| {
                ui.spacing_mut().item_spacing.y = 1.0;
                for folder in &folders {
                    if folders.len() > 1 {
                        side_tabs::folder_heading(ui, folder);
                    }
                    for &at in &folder.members {
                        let entry = &entries[at];
                        let row = side_tabs::Row { title: &entry.name, file_name: &entry.file_name, modified: entry.modified, selected: entry.selected, stripe: entry.stripe };
                        let (clicked, closing, response) = side_tabs::tab_row(ui, &row);
                        crate::fit::context_menu(&response, |ui| {
                            if let Some(choice) = self.tab_menu(ui, entry.doc) {
                                pick = Some((entry.doc, choice));
                            }
                        });
                        if let Some(drop) = side_tabs::drag_and_drop(ui, &response, entry.doc) {
                            moved = Some(drop);
                        }
                        if clicked {
                            chosen = Some((entry.doc, entry.number as usize));
                        }
                        if closing {
                            close = Some((entry.doc, entry.number as usize));
                        }
                    }
                    ui.add_space(4.0);
                }
            });
        });
        if let Some((from, slot)) = moved {
            self.move_tab(from, slot);
        }
        if let Some((i, number)) = chosen {
            self.choose_tab_window(i, number);
        }
        self.finish_tab_actions(pick, close);
    }

    /// A tab clicked: its file, and the window of it, come to the front.
    pub(super) fn choose_tab_window(&mut self, i: usize, number: usize) {
        if i < self.docs.len() {
            self.active = i;
            self.docs[i].switch_window(number);
            self.pending_focus = true;
        }
    }

    /// What a tab's right-click menu picked, and a tab's close button, done once the tabs are drawn.
    pub(super) fn finish_tab_actions(&mut self, pick: Option<(usize, file_colours_host::TabPick)>, close: Option<(usize, usize)>) {
        if let Some((i, choice)) = pick {
            self.tab_pick(i, choice);
        }
        if let Some((i, number)) = close
            && i < self.docs.len()
        {
            if self.docs[i].windows.is_empty() {
                self.close_tab(i);
            } else {
                self.docs[i].close_window(number);
                self.pending_focus = true;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::tests::{app_with, command, draw};
    use super::*;

    #[test]
    fn tabs_down_the_side_draw_group_and_reorder() {
        let (mut app, ctx) = app_with("side_tabs=true\n");
        assert!(app.side_tabs.on);
        let dir = std::env::temp_dir().join(format!("limoniter-side-tabs-{}", std::process::id()));
        let _ = std::fs::create_dir_all(dir.join("one"));
        let _ = std::fs::create_dir_all(dir.join("two"));
        let files = [dir.join("one/a.txt"), dir.join("two/b.txt"), dir.join("one/c.txt")];
        for file in &files {
            std::fs::write(file, "x\n").unwrap();
            assert!(app.open_path(file));
        }
        assert!(app.side_tabs_showing());
        draw(&mut app, &ctx, Vec::new());
        draw(&mut app, &ctx, Vec::new());
        let entries = app.side_entries();
        assert_eq!(entries.len(), app.docs.len());
        // Moving the front tab to the start keeps it in front.
        let front = app.docs[app.active].id;
        let last = app.docs.len() - 1;
        app.active = last;
        let id = app.docs[last].id;
        app.move_tab(last, 0);
        assert_eq!(app.docs[0].id, id);
        assert_eq!(app.docs[app.active].id, id);
        assert_ne!(front, 0);
        app.side_tabs.right = true;
        draw(&mut app, &ctx, Vec::new());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_row_of_tabs_goes_while_the_tabs_are_down_the_side() {
        let (mut app, _ctx) = app_with("");
        assert!(!app.side_tabs_showing());
        app.side_tabs.on = true;
        assert!(app.side_tabs_showing());
        app.show_file_tabs = false;
        assert!(!app.side_tabs_showing());
    }

    #[test]
    fn choosing_tabs_down_the_side_puts_file_tabs_on() {
        let (mut app, ctx) = app_with("file_tabs=false\n");
        command(&mut app, &ctx, Command::Toggle(menus::Flag::SideTabs));
        assert!(app.side_tabs.on && app.show_file_tabs);
    }
}
