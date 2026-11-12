//! The window's side of a part's history: choosing the part, following it as the text changes, the panel listing
//! what changed it, and taking one of those changes back. How changes are traced is in `region_history.rs`.

use super::*;
use crate::region_history::{self, Kind, Step, Touch};
use crate::undo_tree::NodeId;
use std::collections::HashMap;
use std::ops::Range;

/// Larger files than this have their history traced too slowly to keep up with typing.
const MOST_LINES: usize = 200_000;
const MOST_SHOWN_LINES: usize = 40;
/// How far up from the cursor a function's first line is looked for.
const MOST_LINES_UP: usize = 5_000;
/// Stands for the text as it is now, where that isn't a kept state.
const NOW: NodeId = NodeId::MAX;

pub(super) struct RegionState {
    doc: u64,
    pub range: Range<usize>,
    /// The text the range is in, so that the range can follow the text as it changes.
    text: Vec<Arc<str>>,
    version: u64,
    pub name: String,
    /// Each step between two states, by the states' ids. The text as it is now has its version instead.
    steps: HashMap<(NodeId, u64), Arc<Step>>,
    /// The states the list was worked out from, and the steps between them.
    keys: Vec<NodeId>,
    line: Vec<Arc<Step>>,
    worked_out: Option<(u64, Vec<NodeId>, Range<usize>)>,
    pub found: Vec<Shown>,
    pub chosen: Option<usize>,
    pub note: String,
}

/// One change to the part, as listed.
pub(super) struct Shown {
    pub touch: Touch,
    pub when: u64,
    pub summary: String,
    pub removed: Vec<String>,
    pub added: Vec<String>,
}

enum PartAction {
    Close,
    ChooseAgain,
    Pick(usize),
    TakeBack(usize),
    PutBack(usize),
}

fn indent(line: &str) -> Option<usize> {
    let trimmed = line.trim_start();
    (!trimmed.is_empty()).then(|| crate::text_buffer::visual_len(&line[..line.len() - trimmed.len()]))
}

/// Whether a line reads like the start of a function, rather than of an `if` or a loop.
fn looks_like_a_function(line: &str) -> bool {
    let line = line.trim().to_lowercase();
    let first = line.split(|c: char| !c.is_alphanumeric() && c != '_').next().unwrap_or("");
    const STARTS: [&str; 16] = ["fn", "def", "function", "func", "sub", "proc", "procedure", "async", "pub", "public", "private", "protected", "static", "export", "impl", "class"];
    const NOT: [&str; 17] = ["if", "for", "while", "switch", "else", "match", "loop", "catch", "try", "do", "foreach", "case", "when", "elif", "unless", "with", "return"];
    if STARTS.contains(&first) {
        return true;
    }
    line.contains('(') && !NOT.contains(&first) && !line.ends_with(';')
}

/// The part around a line: the function it's in, or the block it's in, or its paragraph.
pub(super) fn part_around(text: &TextBuffer, line: usize) -> Range<usize> {
    let count = text.line_count();
    let get = |at: usize| text.line(at);
    // Each block holding the line, innermost first, found going up through lines indented less and less.
    let mut blocks: Vec<Range<usize>> = Vec::new();
    let mut threshold = (line..count).take(200).find_map(|at| indent(&get(at))).unwrap_or(0) + 1;
    for start in (line.saturating_sub(MOST_LINES_UP)..=line).rev() {
        let Some(here) = indent(&get(start)) else {
            continue;
        };
        if here >= threshold {
            continue;
        }
        if let Some(end) = text.fold_end(start).filter(|end| *end >= line || start == line) {
            let mut first = start;
            let mut last = end;
            // A brace on a line of its own goes with the line above it.
            if get(first).trim() == "{" && first > 0 && indent(&get(first - 1)) == Some(here) {
                first -= 1;
            }
            // So does the line that closes it.
            if let Some(next) = (last + 1 < count).then(|| get(last + 1))
                && indent(&next) == Some(here)
                && next.trim_start().starts_with(['}', ')', ']'])
            {
                last += 1;
            } else if let Some(next) = (last + 1 < count).then(|| get(last + 1))
                && indent(&next) == Some(here)
                && next.trim().to_lowercase().starts_with("end")
            {
                last += 1;
            }
            blocks.push(first..last + 1);
        }
        threshold = here;
        if here == 0 {
            break;
        }
    }
    if let Some(function) = blocks.iter().find(|block| looks_like_a_function(&get(block.start))) {
        return function.clone();
    }
    if let Some(block) = blocks.first() {
        return block.clone();
    }
    // No blocks, as in prose: the paragraph.
    if indent(&get(line)).is_none() {
        return line..line + 1;
    }
    let first = (0..line).rev().take_while(|at| indent(&get(*at)).is_some()).last().unwrap_or(line);
    let last = (line + 1..count).take_while(|at| indent(&get(*at)).is_some()).last().unwrap_or(line);
    first..last + 1
}

/// The selected lines, or the function or paragraph the cursor is in.
pub(super) fn chosen_lines(text: &TextBuffer) -> Range<usize> {
    if text.has_selection() && !text.has_multi() {
        let (top, bottom) = (text.caret_line.min(text.anchor_line), text.caret_line.max(text.anchor_line));
        // A selection ending at the very start of a line doesn't take that line.
        let (end_line, end_col) = if text.caret_line > text.anchor_line { (text.caret_line, text.caret_col) } else { (text.anchor_line, text.anchor_col) };
        let bottom = if bottom > top && end_line == bottom && end_col == 0 { bottom - 1 } else { bottom };
        top..bottom + 1
    } else {
        part_around(text, text.caret_line)
    }
}

/// What a change did, in a few words, and the lines it took away and put in, without those it left alone.
fn describe(touch: &Touch, before: &[Arc<str>], after: &[Arc<str>]) -> (String, Vec<String>, Vec<String>) {
    let same_start = before.iter().zip(after).take_while(|(a, b)| a == b).count();
    let room = before.len().min(after.len()) - same_start;
    let same_end = before.iter().rev().zip(after.iter().rev()).take(room).take_while(|(a, b)| a == b).count();
    let removed: Vec<String> = before[same_start..before.len() - same_end].iter().map(|line| line.to_string()).collect();
    let added: Vec<String> = after[same_start..after.len() - same_end].iter().map(|line| line.to_string()).collect();
    let lines = |n: usize| format!("{n} line{}", if n == 1 { "" } else { "s" });
    let at = |range: &Option<Range<usize>>| range.as_ref().map_or(0, |r| r.start + 1);
    let summary = match touch.kind {
        Kind::Written => format!("Written, {}", lines(after.len())),
        Kind::MovedHere => "Put in here, after being cut".to_string(),
        Kind::CutFromHere => format!("Cut from line {}", at(&touch.before)),
        Kind::Moved if removed.is_empty() && added.is_empty() => format!("Moved from line {}", at(&touch.before)),
        _ => match (removed.len(), added.len()) {
            (0, 0) => "Changed".to_string(),
            (0, n) => format!("Added {}", lines(n)),
            (n, 0) => format!("Took away {}", lines(n)),
            (1, 1) => format!("Changed its line {}", same_start + 1),
            (a, b) if a == b => format!("Changed its lines {} to {}", same_start + 1, same_start + a),
            (a, b) => format!("Changed {}, now {}", lines(a), b),
        },
    };
    (summary, removed, added)
}

fn short_time(when: u64) -> String {
    use chrono::TimeZone;
    let Some(time) = chrono::Local.timestamp_opt(when as i64, 0).single() else {
        return String::new();
    };
    let today = chrono::Local::now().date_naive();
    if time.date_naive() == today { time.format("%H:%M:%S").to_string() } else { time.format("%d/%m %H:%M").to_string() }
}

impl Limoniter {
    /// Starts following the selected lines, or the function around the cursor, and opens the panel.
    pub(super) fn choose_part(&mut self) {
        let doc = &self.docs[self.active];
        if doc.narrow.is_some() {
            self.message("History of this part", "Press Show the whole file first. A part's history is traced through the whole file.".into());
            return;
        }
        if doc.text.is_disk_backed() || doc.text.line_count() > MOST_LINES {
            self.message("History of this part", "This file is too large to trace its history by part. Edit, Edit history still shows every state.".into());
            return;
        }
        let text = &doc.text;
        let range = chosen_lines(text);
        let name: String = text.line(range.start).trim().chars().take(60).collect();
        let lines = text.all_lines();
        let state = RegionState { doc: doc.id, range: range.clone(), text: lines, version: text.version(), name, steps: HashMap::new(), keys: Vec::new(), line: Vec::new(), worked_out: None, found: Vec::new(), chosen: None, note: String::new() };
        self.region = Some(state);
        let last = range.end - 1;
        let width = self.docs[self.active].text.line(last).chars().count();
        self.doc().text.select_chars((range.start, 0), (last, width));
        self.refresh_part();
    }

    /// Keeps the part where its lines are as the text changes, and works out its history again when that changes.
    pub(super) fn refresh_part(&mut self) {
        let Some(state) = &mut self.region else {
            return;
        };
        let Some(doc) = self.docs.iter().find(|doc| doc.id == state.doc) else {
            self.region = None;
            return;
        };
        let text = &doc.text;
        let version = text.version();
        if version != state.version {
            if text.is_disk_backed() || text.line_count() > MOST_LINES || doc.narrow.is_some() {
                return;
            }
            let now = text.all_lines();
            state.range = region_history::diff(&state.text, &now).map_range(state.range.clone()).range;
            state.text = now;
            state.version = version;
        }
        let (path, moved_on) = text.history_line();
        let mut keys: Vec<NodeId> = path.iter().map(|(id, _)| *id).collect();
        if moved_on {
            keys.push(NOW);
        }
        let wanted = (version, keys.clone(), state.range.clone());
        if state.worked_out.as_ref() == Some(&wanted) {
            return;
        }
        let current = state.text.clone();
        let lines_of = |index: usize| -> Option<Vec<Arc<str>>> {
            match keys.get(index) {
                Some(&NOW) => Some(current.clone()),
                Some(id) => text.history_lines(*id),
                None => None,
            }
        };
        state.steps.retain(|(_, to), _| *to < NOW / 2 || *to == NOW - version);
        let mut line = Vec::with_capacity(keys.len().saturating_sub(1));
        let mut previous: Option<Vec<Arc<str>>> = None;
        for index in 0..keys.len().saturating_sub(1) {
            let key = (keys[index], if keys[index + 1] == NOW { NOW - version } else { keys[index + 1] });
            if let Some(step) = state.steps.get(&key) {
                line.push(step.clone());
                previous = None;
                continue;
            }
            let (Some(before), Some(after)) = (previous.take().or_else(|| lines_of(index)), lines_of(index + 1)) else {
                break;
            };
            let step = Arc::new(region_history::diff(&before, &after));
            state.steps.insert(key, step.clone());
            line.push(step);
            previous = Some(after);
        }
        let touches = region_history::touches(&line, &lines_of, state.range.clone());
        let mut found = Vec::with_capacity(touches.len());
        for touch in touches {
            let part = |index: usize, range: &Option<Range<usize>>| -> Vec<Arc<str>> {
                match range {
                    Some(r) if !r.is_empty() => lines_of(index).and_then(|lines| lines.get(r.clone()).map(<[Arc<str>]>::to_vec)).unwrap_or_default(),
                    _ => Vec::new(),
                }
            };
            let (before, after) = (part(touch.step, &touch.before), part(touch.step + 1, &touch.after));
            let (summary, removed, added) = describe(&touch, &before, &after);
            let when = path.get(touch.step).map_or(0, |(_, when)| *when);
            found.push(Shown { touch, when, summary, removed, added });
        }
        state.chosen = state.chosen.filter(|chosen| *chosen < found.len());
        state.found = found;
        state.keys = keys.clone();
        state.line = line;
        state.worked_out = Some(wanted);
    }

    /// The lines of one of the states the list was worked out from.
    fn part_state_lines(&self, index: usize) -> Option<Vec<Arc<str>>> {
        let state = self.region.as_ref()?;
        let doc = self.docs.iter().find(|doc| doc.id == state.doc)?;
        match state.keys.get(index) {
            Some(&NOW) => Some(state.text.clone()),
            Some(id) => doc.text.history_lines(*id),
            None => None,
        }
    }

    /// Whether the part's file can be changed now, and says why not if it can't.
    fn part_editable(&mut self) -> Option<usize> {
        let id = self.region.as_ref()?.doc;
        let index = self.docs.iter().position(|doc| doc.id == id)?;
        let doc = &self.docs[index];
        if doc.is_busy() || self.pending.is_some() || self.script.is_some() {
            self.message("Please wait", "The file is still loading or saving. This will be available when it finishes.".into());
            return None;
        }
        if doc.text.read_only {
            self.active = index;
            self.tell_read_only();
            return None;
        }
        if doc.hex_mode || doc.narrow.is_some() {
            self.message("History of this part", "This works on the whole text. Go back to the text, and show the whole file, first.".into());
            return None;
        }
        Some(index)
    }

    /// Takes back one change to the part, and nothing done anywhere else since.
    pub(super) fn take_back_part_change(&mut self, chosen: usize) -> Result<(), String> {
        self.refresh_part();
        let state = self.region.as_ref().ok_or("No part is being followed.")?;
        let shown = state.found.get(chosen).ok_or("That change is no longer listed.")?;
        let touch = shown.touch.clone();
        let line = state.line.clone();
        let before = self.part_state_lines(touch.step).ok_or("That state is no longer kept.")?;
        let edits = region_history::undo_one(&line, &touch, &before)?;
        let index = self.part_editable().ok_or(String::new())?;
        self.active = index;
        let text = &mut self.docs[index].text;
        text.begin_undo_group(false);
        for (range, lines) in edits {
            text.splice_lines(range.start, range.end, lines);
        }
        text.end_undo_group();
        self.pending_focus = true;
        Ok(())
    }

    /// Puts the whole part back as it was before one change, which takes back that change and every later one to it.
    pub(super) fn put_part_back(&mut self, chosen: usize) -> Result<(), String> {
        self.refresh_part();
        let state = self.region.as_ref().ok_or("No part is being followed.")?;
        let shown = state.found.get(chosen).ok_or("That change is no longer listed.")?;
        let Some(before) = shown.touch.before.clone() else {
            return Err("The part wasn't in the file before this.".into());
        };
        let (step, range) = (shown.touch.step, state.range.clone());
        let lines = self.part_state_lines(step).and_then(|lines| lines.get(before).map(<[Arc<str>]>::to_vec)).ok_or("That state is no longer kept.")?;
        let index = self.part_editable().ok_or(String::new())?;
        self.active = index;
        self.docs[index].text.splice_lines(range.start, range.end, lines);
        self.pending_focus = true;
        Ok(())
    }

    pub(super) fn part_history_panel(&mut self, ui: &mut Ui) {
        if self.region.is_none() {
            return;
        }
        self.refresh_part();
        let Some(state) = &mut self.region else {
            return;
        };
        let followed = state.doc;
        let this_file = self.docs[self.active].id == followed;
        let file_name = self.docs.iter().find(|doc| doc.id == state.doc).map(Document::name).unwrap_or_default();
        let mut action = None;
        Panel::right("part-history").default_size(340.0).size_range(220.0..=900.0).resizable(true).frame(egui::Frame::new().fill(theme::window()).inner_margin(egui::Margin::symmetric(8, 6))).show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label(RichText::new("History of this part").color(theme::text()).size(13.5));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.add(Button::new(RichText::new("\u{D7}").color(theme::text_dim())).frame(false)).on_hover_text("Close").clicked() {
                        action = Some(PartAction::Close);
                    }
                });
            });
            let where_now = if state.range.is_empty() { "No longer in the file".to_string() } else { format!("Lines {} to {}", state.range.start + 1, state.range.end) };
            ui.label(RichText::new(where_now).color(theme::accent()).size(12.5));
            ui.label(RichText::new(if this_file { state.name.clone() } else { format!("{}, in {file_name}", state.name) }).color(theme::text_dim()).monospace().size(12.0));
            if ui.button("Follow the selection or the cursor instead").on_hover_text("The selected lines, or the function the cursor is in").clicked() {
                action = Some(PartAction::ChooseAgain);
            }
            ui.add_space(4.0);
            ui.separator();
            if !state.note.is_empty() {
                ui.label(RichText::new(state.note.as_str()).color(theme::rainbow_mistake()).size(12.5));
            }
            if state.found.is_empty() {
                ui.label(RichText::new("No change that's kept has touched these lines. Changes are listed here as they're made, wherever the lines move to.").color(theme::text_faint()).size(12.5));
                return;
            }
            // The chosen change, with what it did and what can be done about it, stays at the bottom.
            if let Some(chosen) = state.chosen.and_then(|at| state.found.get(at).map(|shown| (at, shown))) {
                egui::Panel::bottom("part-history-detail").frame(egui::Frame::new()).show(ui, |ui| {
                    let (at, shown) = chosen;
                    ui.add_space(4.0);
                    egui::ScrollArea::both().id_salt("part-change").max_height(260.0).auto_shrink([false, true]).show(ui, |ui| {
                        let shown_lines = |ui: &mut Ui, lines: &[String], mark: &str, colour: egui::Color32| {
                            for line in lines.iter().take(MOST_SHOWN_LINES) {
                                ui.label(RichText::new(format!("{mark} {line}")).monospace().size(12.0).color(colour));
                            }
                            if lines.len() > MOST_SHOWN_LINES {
                                ui.label(RichText::new(format!("  and {} more", lines.len() - MOST_SHOWN_LINES)).color(theme::text_faint()).size(11.5));
                            }
                        };
                        shown_lines(ui, &shown.removed, "\u{2212}", theme::rainbow_mistake());
                        shown_lines(ui, &shown.added, "+", theme::change_saved());
                    });
                    ui.add_space(6.0);
                    let can_take_back = shown.touch.kind != Kind::CutFromHere;
                    if ui.add_enabled(can_take_back, Button::new("Take back just this change")).on_hover_text("Changes made since, here or anywhere else, stay as they are").clicked() {
                        action = Some(PartAction::TakeBack(at));
                    }
                    if ui.add_enabled(shown.touch.before.is_some() && !state.range.is_empty(), Button::new("Put this part back as it was before it")).on_hover_text("Takes back this change and every later one to this part, and nothing outside it").clicked() {
                        action = Some(PartAction::PutBack(at));
                    }
                    ui.label(RichText::new("Either comes off again with Undo.").color(theme::text_faint()).size(11.5));
                    ui.add_space(2.0);
                });
            }
            egui::ScrollArea::vertical().id_salt("part-history-list").auto_shrink([false, false]).show(ui, |ui| {
                for (index, shown) in state.found.iter().enumerate() {
                    let here = state.chosen == Some(index);
                    let (rect, response) = ui.allocate_exact_size(egui::vec2(ui.available_width(), 26.0), egui::Sense::click());
                    if here {
                        ui.painter().rect_filled(rect, 5.0, theme::selected());
                    } else if response.hovered() {
                        ui.painter().rect_filled(rect, 5.0, theme::hover());
                    }
                    let dot = egui::pos2(rect.min.x + 10.0, rect.center().y);
                    let colour = match shown.touch.kind {
                        Kind::Changed => theme::accent(),
                        Kind::Written => theme::change_saved(),
                        _ => theme::change_unsaved(),
                    };
                    ui.painter().circle_filled(dot, if here { 4.5 } else { 3.5 }, colour);
                    ui.painter().text(egui::pos2(rect.min.x + 22.0, rect.center().y), egui::Align2::LEFT_CENTER, short_time(shown.when), egui::FontId::proportional(11.5), theme::text_faint());
                    ui.painter().text(egui::pos2(rect.min.x + 92.0, rect.center().y), egui::Align2::LEFT_CENTER, shown.summary.as_str(), egui::FontId::proportional(12.5), theme::text());
                    if response.clicked() {
                        action = Some(PartAction::Pick(index));
                    }
                }
            });
        });
        match action {
            None => {}
            Some(PartAction::Close) => {
                self.region = None;
                self.pending_focus = true;
            }
            Some(PartAction::ChooseAgain) => {
                if this_file {
                    self.choose_part();
                } else if let Some(index) = self.docs.iter().position(|doc| doc.id == followed) {
                    self.active = index;
                }
            }
            Some(PartAction::Pick(index)) => {
                if let Some(state) = &mut self.region {
                    state.chosen = Some(index);
                    state.note.clear();
                }
            }
            Some(PartAction::TakeBack(index)) => {
                let result = self.take_back_part_change(index);
                self.note_part_result(result);
            }
            Some(PartAction::PutBack(index)) => {
                let result = self.put_part_back(index);
                self.note_part_result(result);
            }
        }
    }

    fn note_part_result(&mut self, result: Result<(), String>) {
        if let Some(state) = &mut self.region {
            state.note = result.err().unwrap_or_default();
            if state.note.is_empty() {
                state.chosen = None;
            }
        }
    }

    pub(super) fn part_screenshot_step(&mut self) {
        // Some history to show: a function changed, lines added above it, another function changed after.
        let text = &mut self.doc().text;
        text.set_content(vec!["fn parse_header(line: &str) -> Option<Header> {".into(), "    let (name, value) = line.split_once(':')?;".into(), "    Some(Header { name, value })".into(), "}".into(), String::new(), "fn print_all(headers: &[Header]) {".into(), "    for h in headers {".into(), "        println!(\"{h}\");".into(), "    }".into(), "}".into()]);
        let edits: [(usize, &str); 4] = [(2, "    Some(Header { name: name.trim(), value })"), (6, "    for header in headers {"), (1, "    let (name, value) = line.split_once(':').filter(|_| !line.is_empty())?;"), (2, "    Some(Header { name: name.trim(), value: value.trim() })")];
        for (line, with) in edits {
            text.splice_lines(line, line + 1, vec![Arc::from(with)]);
        }
        text.splice_lines(0, 0, vec![Arc::from("// Reading and writing headers."), Arc::from("")]);
        text.move_caret(3, 4, false);
        self.choose_part();
        if let Some(state) = &mut self.region {
            state.chosen = Some(0);
        }
    }
}
