//! The window's side of text objects, narrowing and the history of edits. Finding text objects is in `text_objects.rs`,
//! the tree of states is in `undo_tree.rs`, and keeping it between runs is in `history_file.rs`.

use super::*;
use crate::history_file;
use crate::panels::HistoryAction;

/// The most lines that can be narrowed to, which then have to be held in memory.
const MOST_NARROWED_LINES: usize = 2_000_000;

/// While narrowed, the document's `text` holds only the chosen lines, so every command is limited to them without
/// knowing it. The whole file waits here, untouched, until the lines are put back.
pub(super) struct Narrow {
    pub outer: TextBuffer,
    pub outer_state: ViewState,
    pub top: usize,
    /// The last line of the part, in the whole file as it now stands.
    pub bottom: usize,
    /// The narrowed text's version when it last matched the whole file.
    pub synced: u64,
}

/// A narrowed part set aside during a save, to go back to afterwards.
pub(super) struct HeldNarrow {
    inner: TextBuffer,
    state: ViewState,
    top: usize,
    bottom: usize,
}

impl Document {
    fn reset_views_of_text(&mut self) {
        self.resolved_syntax = None;
        self.fold_view.clear();
        self.fold_view_of = None;
    }

    /// Puts any changes in the narrowed part into the whole file, as one undoable step there.
    fn sync_narrow(&mut self) {
        let version = self.text.version();
        let Some(narrow) = &mut self.narrow else {
            return;
        };
        // Always, because a bookmark can change without the text changing. If the lines are the same, that's no edit.
        narrow.outer.replace_region(narrow.top, narrow.bottom, &self.text);
        narrow.bottom = narrow.top + self.text.line_count() - 1;
        narrow.synced = version;
    }

    pub(super) fn narrow_to(&mut self, top: usize, bottom: usize) {
        let inner = self.text.region_copy(top, bottom);
        let synced = inner.version();
        let outer = std::mem::replace(&mut self.text, inner);
        let outer_state = std::mem::take(&mut self.text_state);
        self.narrow = Some(Narrow { outer, outer_state, top, bottom, synced });
        self.reset_views_of_text();
    }

    /// Back to the whole file, with the cursor on the same text.
    pub(super) fn widen(&mut self) {
        self.sync_narrow();
        let Some(narrow) = self.narrow.take() else {
            return;
        };
        let inner = std::mem::replace(&mut self.text, narrow.outer);
        self.text_state = narrow.outer_state;
        let top = narrow.top;
        self.text.set_selection((top + inner.anchor_line, inner.anchor_col), (top + inner.caret_line, inner.caret_col), false);
        self.text.reveal = true;
        self.reset_views_of_text();
    }

    /// For saving: the whole file comes to the front, and the part waits to go back.
    fn step_out_of_narrow(&mut self) -> Option<HeldNarrow> {
        self.sync_narrow();
        let narrow = self.narrow.take()?;
        let inner = std::mem::replace(&mut self.text, narrow.outer);
        let state = std::mem::replace(&mut self.text_state, narrow.outer_state);
        Some(HeldNarrow { inner, state, top: narrow.top, bottom: narrow.bottom })
    }

    fn step_back_into_narrow(&mut self, held: HeldNarrow) {
        let outer = std::mem::replace(&mut self.text, held.inner);
        let outer_state = std::mem::replace(&mut self.text_state, held.state);
        self.text.mark_saved();
        let synced = self.text.version();
        self.narrow = Some(Narrow { outer, outer_state, top: held.top, bottom: held.bottom, synced });
    }

    /// The whole file as it would be if the part went back now, for a recovery copy.
    pub(super) fn whole_store(&self) -> crate::line_store::LineStore {
        match &self.narrow {
            Some(narrow) => narrow.outer.store_with_region(narrow.top, narrow.bottom, &self.text),
            None => self.text.store().clone(),
        }
    }
}

impl Limoniter {
    // ---------- Text objects ----------

    pub(super) fn text_object(&mut self, action: text_objects::Action, object: text_objects::Object) {
        use text_objects::Action;
        let text = &self.docs[self.active].text;
        if text.column_mode() {
            self.message("Text objects", "These don't work in column mode. Turn it off with Alt+C first.".into());
            return;
        }
        let (start, end) = text.selection_char_bounds();
        // Bytes kept as they are read as the characters they stand for, or a bracket in an old encoding wouldn't be seen.
        let read = |line: usize| -> Arc<str> { Arc::from(text.format.readable_text(&text.line(line))) };
        let Some(found) = text_objects::find(&read, text.line_count(), start, end, object) else {
            self.message("Text objects", object.not_found().into());
            return;
        };
        let range = match action {
            // Asking again goes on to the whole thing, and from there to whatever is around it.
            Action::SelectInside if (start, end) == found.inside => found.around,
            Action::SelectInside | Action::ReplaceInside | Action::DeleteInside => found.inside,
            Action::SelectAround | Action::DeleteAround => found.around,
        };
        let text = &mut self.docs[self.active].text;
        text.select_chars(range.0, range.1);
        if matches!(action, Action::ReplaceInside | Action::DeleteInside | Action::DeleteAround) {
            text.delete_selection();
        }
        self.pending_focus = true;
    }

    // ---------- Narrowing ----------

    pub(super) fn narrow_command(&mut self) {
        let doc = &self.docs[self.active];
        let refusal = if doc.narrow.is_some() {
            Some("This is already narrowed. Show the whole file first.")
        } else if doc.hex_mode {
            Some("Narrowing is for text. Switch to text mode first.")
        } else if doc.filter.is_some() {
            Some("End the filter first.")
        } else if doc.split.is_some() || !doc.windows.is_empty() {
            Some("End the split, and close other windows on this file, first.")
        } else {
            None
        };
        if let Some(refusal) = refusal {
            self.message("Narrow", refusal.into());
            return;
        }
        let text = &doc.text;
        let (top, bottom) = if text.has_selection() {
            let (top, bottom) = (text.caret_line.min(text.anchor_line), text.caret_line.max(text.anchor_line));
            let end_col = if text.caret_line >= text.anchor_line { text.caret_col } else { text.anchor_col };
            // A selection ending at the very start of a line doesn't really include that line.
            (top, if end_col == 0 && bottom > top { bottom - 1 } else { bottom })
        } else {
            let read = |line: usize| text.line(line);
            let place = (text.caret_line, 0);
            let found = text_objects::find(&read, text.line_count(), place, place, text_objects::Object::Paragraph);
            found.map_or((text.caret_line, text.caret_line), |found| (found.inside.0.0, found.inside.1.0))
        };
        if top == 0 && bottom + 1 == text.line_count() {
            self.message("Narrow", "That's the whole file. Select the part you want to work on, then narrow.".into());
            return;
        }
        if bottom - top + 1 > MOST_NARROWED_LINES {
            self.message("Narrow", "That's too many lines to narrow to, because they'd all have to be held in memory.".into());
            return;
        }
        self.doc().narrow_to(top, bottom);
        self.pending_focus = true;
    }

    pub(super) fn widen_command(&mut self) {
        self.doc().widen();
        self.pending_focus = true;
    }

    /// Saves the whole file, whatever is narrowed. The part goes back in if the save has finished, and if it hasn't, or
    /// needs asking about, the whole file stays in front, because that's what's being saved.
    pub(super) fn write_narrowed(&mut self, index: usize, path: &Path) -> bool {
        let Some(held) = self.docs[index].step_out_of_narrow() else {
            return self.write_to(index, path);
        };
        let saved = self.write_to(index, path);
        if saved && !self.docs[index].is_busy() && matches!(self.dialog, Dialog::None) {
            self.docs[index].step_back_into_narrow(held);
        } else if matches!(self.dialog, Dialog::None) && !self.docs[index].is_busy() {
            self.docs[index].text.notice = Some("The file wasn't saved, so the whole of it is showing again. Nothing has been lost.".into());
        }
        saved
    }

    /// The bar saying that part of the file is hidden, with the way back.
    pub(super) fn narrow_bar(&mut self, ui: &mut Ui) {
        let doc = &self.docs[self.active];
        let Some(narrow) = &doc.narrow else {
            return;
        };
        let count = doc.text.line_count();
        let total = narrow.outer.line_count() + count - (narrow.bottom - narrow.top + 1);
        let (first, last) = (narrow.top + 1, narrow.top + count);
        let mut widen = false;
        Panel::top("narrow-bar").frame(egui::Frame::new().fill(theme::selected()).inner_margin(egui::Margin::symmetric(8, 4))).show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label(RichText::new(format!("Narrowed to lines {first} to {last} of {total}.")).strong());
                ui.label(RichText::new("Only these can be seen or changed. The rest of the file is safe, and saving saves all of it.").color(theme::text_dim()));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    widen = ui.button("Show the whole file").on_hover_text("Ctrl+Shift+N").clicked();
                });
            });
        });
        if widen {
            self.run(Command::Widen, ui);
        }
    }

    // ---------- The history of edits ----------

    pub(super) fn history_action(&mut self, action: HistoryAction, ui: &Ui) {
        let _ = ui;
        let doc = &self.docs[self.active];
        match action {
            HistoryAction::Jump(id) => {
                if doc.is_busy() || self.pending.is_some() {
                    self.message("Please wait", "The file is still loading or saving. This will be available when it finishes.".into());
                } else if doc.text.read_only {
                    self.tell_read_only();
                } else {
                    self.doc().text.history_jump(id);
                    self.pending_focus = true;
                }
            }
            HistoryAction::Compare(id) => {
                let other = if doc.text.is_disk_backed() {
                    Err("This file is kept on disk, so it can't be compared.".to_string())
                } else {
                    doc.text.history_lines(id).map(|lines| (format!("{} as it was", doc.name()), crate::big_jobs::Side::Lines(crate::line_store::LineStore::from_vec(lines)))).ok_or_else(|| "That state is no longer kept.".to_string())
                };
                self.show_comparison(other);
            }
            HistoryAction::Forget => {
                if ask_yes_no("Forget this history", "Forget all the earlier states of this file, including any kept on disk?\n\nThis can't be undone. The text itself stays as it is.") {
                    let path = doc.path.clone();
                    self.doc().text.forget_history();
                    if let (Some(root), Some(path)) = (&self.history_root, path) {
                        history_file::forget(root, &path);
                    }
                }
            }
        }
    }

    /// Keeps the history of a file just saved, on another thread, since it can take a moment.
    pub(super) fn keep_history(&mut self, index: usize, bytes: &[u8]) {
        let doc = &mut self.docs[index];
        let (Some(root), Some(path)) = (self.history_root.clone(), doc.path.clone()) else {
            return;
        };
        if !self.prefs.keep_undo_history || doc.hex_mode {
            return;
        }
        let Some(image) = doc.text.history_image(history_file::hash_of(bytes)) else {
            return;
        };
        self.finish_history_job();
        self.history_job = std::thread::Builder::new().name("limoniter-history".into()).spawn(move || drop(history_file::write(&root, &path, &image))).ok();
    }

    /// Waits for any history still being written.
    pub(super) fn finish_history_job(&mut self) {
        if let Some(job) = self.history_job.take() {
            let _ = job.join();
        }
    }

    /// Brings back what's kept for a file just opened, as long as the file still holds what it did then.
    pub(super) fn recall_history(&self, doc: &mut Document, path: &Path) {
        let Some(root) = &self.history_root else {
            return;
        };
        if !self.prefs.keep_undo_history || doc.hex_mode || doc.text.is_disk_backed() || !history_file::exists(root, path) {
            return;
        }
        if let Ok(bytes) = std::fs::read(path)
            && let Some(image) = history_file::read(root, path, &bytes)
        {
            doc.text.load_history(image);
        }
    }

    /// Sets up a state to take a picture of.
    pub(super) fn edit_tools_screenshot_step(&mut self, step: &str) {
        match step {
            "history" => {
                self.panels.history = true;
                let text = &mut self.doc().text;
                for (line, words) in [(0, "First change. "), (2, "Second change. "), (4, "Third change. ")] {
                    text.move_caret(line, 0, false);
                    text.type_text(words);
                    text.type_text(" ");
                }
                text.undo();
                text.undo();
                text.move_caret(6, 0, false);
                text.type_text("Went another way. ");
                text.type_text(" ");
                text.undo();
                text.move_caret(7, 0, false);
                text.type_text("And a third way. ");
            }
            "narrow" => {
                let text = &mut self.doc().text;
                text.move_caret(5, 0, false);
                text.move_caret(16, 0, true);
                self.narrow_command();
            }
            "text-object" => {
                let text = &mut self.doc().text;
                text.move_caret(0, 12, false);
                self.text_object(text_objects::Action::SelectInside, text_objects::Object::Word);
            }
            _ => {}
        }
    }
}
