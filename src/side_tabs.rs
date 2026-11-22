//! Tabs down the side: the open files as a list in a panel at the left or right, in place of the row along the top,
//! grouped by the folder each file is in (the Open files panel groups them the same way).
//!
//! This is the part that doesn't need the window: how files are grouped, where a dragged tab ends up, how the
//! setting is kept, and how one tab's row is drawn.

use crate::theme;
use eframe::egui::{self, Color32, FontId, Sense, Stroke, Ui, pos2, vec2};
use egui::text::LayoutJob;
use std::path::{Path, PathBuf};

/// Whether the tabs are down the side, and which side.
#[derive(Clone, Copy, Default, PartialEq, Eq, Debug)]
pub struct SideTabs {
    pub on: bool,
    pub right: bool,
}

impl SideTabs {
    pub fn from_settings(settings: &str) -> Self {
        let value = |name: &str| settings.lines().filter_map(|line| line.split_once('=')).find(|(key, _)| key.trim() == name).map(|(_, value)| value.trim() == "true");
        Self { on: value("side_tabs").unwrap_or(false), right: value("side_tabs_right").unwrap_or(false) }
    }

    pub fn to_settings(self) -> String {
        format!("side_tabs={}\nside_tabs_right={}\n", self.on, self.right)
    }
}

/// The files in one folder, as the tab list and the Open files panel show them.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Folder {
    /// The folder's own name, or its whole path where two folders share a name. Empty for files with no place yet.
    pub name: String,
    /// The whole path, to show when the pointer rests on the heading. None for files that haven't been saved.
    pub full: Option<PathBuf>,
    /// Which of the files given, by their numbers in that list.
    pub members: Vec<usize>,
}

/// Groups files by the folder they're in. Folders come in the order their first file appears, so the tabs keep the
/// order they were opened in; files that haven't been saved yet are last, in a group of their own.
pub fn group_by_folder(paths: &[Option<&Path>]) -> Vec<Folder> {
    let mut groups: Vec<Folder> = Vec::new();
    let mut unsaved = Vec::new();
    for (number, path) in paths.iter().enumerate() {
        let Some(parent) = path.and_then(Path::parent) else {
            unsaved.push(number);
            continue;
        };
        match groups.iter_mut().find(|group| group.full.as_deref() == Some(parent)) {
            Some(group) => group.members.push(number),
            None => groups.push(Folder { name: folder_name(parent), full: Some(parent.to_path_buf()), members: vec![number] }),
        }
    }
    // Two folders with one name (two "src" folders) are told apart by their whole paths.
    let names: Vec<String> = groups.iter().map(|group| group.name.clone()).collect();
    for group in &mut groups {
        if names.iter().filter(|name| **name == group.name).count() > 1
            && let Some(full) = &group.full
        {
            group.name = full.display().to_string();
        }
    }
    if !unsaved.is_empty() {
        groups.push(Folder { name: String::new(), full: None, members: unsaved });
    }
    groups
}

fn folder_name(folder: &Path) -> String {
    folder.file_name().map_or_else(|| folder.display().to_string(), |name| name.to_string_lossy().into_owned())
}

/// Where a tab dragged from place `from` ends up when it's dropped in the gap `slot`: the gap before the tab with
/// that number, so `len` is the gap after the last one.
pub fn after_move(from: usize, slot: usize) -> usize {
    if slot > from { slot - 1 } else { slot }
}

/// Moves the item at `from` into the gap `slot`, and says where it ended up.
pub fn move_into_gap<T>(items: &mut Vec<T>, from: usize, slot: usize) -> usize {
    let to = after_move(from, slot.min(items.len())).min(items.len().saturating_sub(1));
    if from < items.len() && to != from {
        let item = items.remove(from);
        items.insert(to, item);
    }
    to
}

/// Where the item that was at `index` is once the one at `from` has moved to `to`.
pub fn follow(index: usize, from: usize, to: usize) -> usize {
    if index == from {
        to
    } else if from < index && index <= to {
        index - 1
    } else if to <= index && index < from {
        index + 1
    } else {
        index
    }
}

/// What's drawn for one tab.
pub struct Row<'a> {
    pub title: &'a str,
    pub file_name: &'a str,
    pub modified: bool,
    pub selected: bool,
    /// The colour the file was given, drawn as a stripe down the row's left edge.
    pub stripe: Option<Color32>,
}

/// The height of a tab's row.
pub const ROW_HEIGHT: f32 = 26.0;

/// One tab as a row of the list. Returns (chosen, close asked for, the row, for its menu and dragging). Unsaved
/// changes show as a dot that becomes the close button when the pointer is over the row.
pub fn tab_row(ui: &mut Ui, row: &Row) -> (bool, bool, egui::Response) {
    let (rect, response) = ui.allocate_exact_size(vec2(ui.available_width(), ROW_HEIGHT), Sense::click_and_drag());
    let hovered = response.hovered();
    if row.selected {
        ui.painter().rect_filled(rect, 5.0, theme::selected());
    } else if hovered {
        ui.painter().rect_filled(rect, 5.0, theme::hover());
    }
    if let Some(colour) = row.stripe {
        let band = egui::Rect::from_min_max(pos2(rect.min.x + 1.0, rect.min.y + 3.0), pos2(rect.min.x + 4.0, rect.max.y - 3.0));
        ui.painter().rect_filled(band, 1.5, colour);
    }
    let icon = crate::file_icons::width(13.0) + 4.0;
    crate::file_icons::paint(ui.painter(), egui::Rect::from_min_size(pos2(rect.min.x + 8.0, rect.min.y), vec2(icon, rect.height())), row.file_name, 13.0);
    let ink = if row.selected { theme::text() } else { theme::text_dim() };
    let room = (rect.width() - 8.0 - icon - 34.0).max(20.0);
    let mut job = LayoutJob::simple(row.title.to_string(), FontId::proportional(13.0), ink, room);
    job.wrap.max_rows = 1;
    job.wrap.break_anywhere = true;
    job.wrap.overflow_character = Some('\u{2026}');
    let galley = ui.painter().layout_job(job);
    ui.painter().galley(pos2(rect.min.x + 8.0 + icon, rect.center().y - galley.size().y / 2.0), galley, ink);

    let close_rect = egui::Rect::from_center_size(pos2(rect.max.x - 15.0, rect.center().y), vec2(18.0, 18.0));
    let close = ui.interact(close_rect, response.id.with("close"), Sense::click());
    let showing_close = close.hovered() || hovered || (row.selected && !row.modified);
    if row.modified && !showing_close {
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
    crate::access::button(&close, &format!("Close {}", row.title));
    let closing = close.on_hover_text(if row.modified { crate::i18n::tr("Close (it has unsaved changes)") } else { crate::i18n::tr("Close") }).clicked();
    let name = if row.modified { crate::i18n::tf!("{title}, with unsaved changes", title = row.title) } else { row.title.to_string() };
    crate::access::chosen(&response, egui::Role::Tab, row.selected, &name);
    let response = if row.modified { response.on_hover_text(name) } else { response };
    (response.clicked() && !closing, closing, response)
}

/// A folder's heading above its tabs.
pub fn folder_heading(ui: &mut Ui, folder: &Folder) {
    let name = if folder.full.is_some() { folder.name.clone() } else { crate::i18n::tr("Not saved yet").to_string() };
    let label = ui.add(egui::Label::new(egui::RichText::new(name).size(11.5).color(theme::text_faint()).strong()).truncate());
    if let Some(full) = &folder.full {
        label.on_hover_text(full.display().to_string());
    }
}

/// Dragging a tab over another: the gap it would drop into, marked with a line, and the drop itself. Returns
/// `(from, slot)` when a tab was dropped, with `index` the number of the tab this row belongs to.
pub fn drag_and_drop(ui: &Ui, response: &egui::Response, index: usize) -> Option<(usize, usize)> {
    response.dnd_set_drag_payload(index);
    let dragged = response.dnd_hover_payload::<usize>()?;
    let rect = response.rect;
    let below = ui.input(|i| i.pointer.interact_pos()).is_some_and(|at| at.y > rect.center().y);
    let slot = if below { index + 1 } else { index };
    let y = if below { rect.max.y } else { rect.min.y };
    ui.painter().line_segment([pos2(rect.min.x + 2.0, y), pos2(rect.max.x - 2.0, y)], Stroke::new(2.0, theme::accent()));
    let from = *dragged;
    response.dnd_release_payload::<usize>().map(|_| (from, slot))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn paths<'a>(list: &'a [Option<&'a str>]) -> Vec<Option<&'a Path>> {
        list.iter().map(|path| path.map(Path::new)).collect()
    }

    #[test]
    fn files_are_grouped_by_folder_in_the_order_the_folders_first_appear() {
        let list = paths(&[Some("/p/src/a.rs"), Some("/p/docs/b.md"), Some("/p/src/c.rs")]);
        let groups = group_by_folder(&list);
        assert_eq!(groups.len(), 2);
        assert_eq!((groups[0].name.as_str(), groups[0].members.clone()), ("src", vec![0, 2]));
        assert_eq!((groups[1].name.as_str(), groups[1].members.clone()), ("docs", vec![1]));
    }

    #[test]
    fn unsaved_files_come_last_in_a_group_with_no_folder() {
        let list = paths(&[None, Some("/p/a.txt"), None]);
        let groups = group_by_folder(&list);
        assert_eq!(groups.len(), 2);
        assert_eq!(groups[0].members, vec![1]);
        assert!(groups[1].full.is_none() && groups[1].members == vec![0, 2]);
    }

    #[test]
    fn two_folders_with_one_name_are_told_apart_by_their_paths() {
        let list = paths(&[Some("/a/src/x.rs"), Some("/b/src/y.rs")]);
        let groups = group_by_folder(&list);
        assert_eq!(groups[0].name, Path::new("/a/src").display().to_string());
        assert_eq!(groups[1].name, Path::new("/b/src").display().to_string());
    }

    #[test]
    fn dropping_in_a_gap_moves_the_tab_there() {
        let mut tabs = vec!['a', 'b', 'c', 'd'];
        assert_eq!(move_into_gap(&mut tabs, 0, 3), 2);
        assert_eq!(tabs, ['b', 'c', 'a', 'd']);
        assert_eq!(move_into_gap(&mut tabs, 3, 0), 0);
        assert_eq!(tabs, ['d', 'b', 'c', 'a']);
        // Into the gap it's already in, nothing changes; the last gap is after the last tab.
        assert_eq!(move_into_gap(&mut tabs, 1, 2), 1);
        assert_eq!(move_into_gap(&mut tabs, 0, 4), 3);
        assert_eq!(tabs, ['b', 'c', 'a', 'd']);
    }

    #[test]
    fn the_tab_in_front_stays_in_front_when_tabs_move() {
        for (from, slot) in [(0, 3), (3, 0), (1, 2), (2, 1), (0, 4), (1, 1)] {
            for active in 0..4 {
                let mut tabs = vec![0, 1, 2, 3];
                let to = move_into_gap(&mut tabs, from, slot);
                assert_eq!(tabs[follow(active, from, to)], active, "from {from} to gap {slot}, with {active} in front");
            }
        }
    }

    #[test]
    fn the_setting_is_kept_and_read_back() {
        let kept = SideTabs { on: true, right: true };
        assert_eq!(SideTabs::from_settings(&kept.to_settings()), kept);
        assert_eq!(SideTabs::from_settings(""), SideTabs::default());
        assert_eq!(SideTabs::from_settings("side_tabs=true\n"), SideTabs { on: true, right: false });
    }
}
