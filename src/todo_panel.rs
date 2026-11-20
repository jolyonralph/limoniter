//! The TODO panel: the TODO, FIXME, BUG, HACK and XXX notes left in comments, in the file in front or across the
//! project, each a click from its line (as Todo Tree does). A project's files are read on a thread of their own.

use eframe::egui::{self, RichText, Ui};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{Receiver, channel};
use std::time::{Duration, Instant};

/// The words looked for, and the colour tag (1 red to 6 purple) each is shown in, as the comments are coloured.
pub const WORDS: [(&str, u8); 5] = [("TODO", 2), ("FIXME", 1), ("BUG", 1), ("HACK", 6), ("XXX", 1)];

/// What starts a comment, in most languages: a note only counts after one of these on its line.
const MARKERS: [&str; 10] = ["//", "#", ";", "--", "/*", "*", "<!--", "'", "REM", "%"];

/// The largest file read, and the most files and notes, so a huge folder can't hold it up.
const LARGEST_FILE: u64 = 2 * 1024 * 1024;
const MOST_FILES: usize = 20_000;
const MOST_NOTES: usize = 5_000;

/// How often the file in front is looked through again.
const LOOK_AGAIN: Duration = Duration::from_millis(800);

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Note {
    /// The file it's in; None for the file in front.
    pub path: Option<PathBuf>,
    /// From 0.
    pub line: usize,
    pub word: &'static str,
    pub text: String,
}

/// The notes in some lines of text.
pub fn notes_in<S: AsRef<str>>(lines: impl Iterator<Item = S>, path: Option<&Path>) -> Vec<Note> {
    let mut found = Vec::new();
    for (number, line) in lines.enumerate() {
        let line = line.as_ref();
        if line.len() > 4000 {
            continue;
        }
        if let Some((word, text)) = note_in_line(line) {
            found.push(Note { path: path.map(Path::to_path_buf), line: number, word, text });
        }
    }
    found
}

/// A note in one line: its word, and what follows it.
fn note_in_line(line: &str) -> Option<(&'static str, String)> {
    for (word, _) in WORDS {
        let mut from = 0;
        while let Some(found) = line[from..].find(word) {
            let at = from + found;
            let end = at + word.len();
            from = end;
            let before_ok = line[..at].chars().last().is_none_or(|c| !c.is_alphanumeric() && c != '_');
            let after_ok = line[end..].chars().next().is_none_or(|c| !c.is_alphanumeric() && c != '_');
            // After a comment's marker, so a variable called TODO_COUNT, or "TODO" in a string, isn't one.
            let in_comment = MARKERS.iter().any(|marker| line[..at].contains(marker));
            if before_ok && after_ok && in_comment {
                let rest = line[end..].trim_start_matches(|c: char| c == ':' || c == '(' || c.is_whitespace());
                let rest = rest.trim_end_matches("*/").trim_end_matches("-->").trim();
                return Some((word, rest.to_string()));
            }
        }
    }
    None
}

/// Whether to look in the file in front, or across the project.
#[derive(Clone, Copy, PartialEq, Eq, Default)]
pub enum Scope {
    #[default]
    ThisFile,
    Project,
}

#[derive(Default)]
pub struct TodoPanel {
    pub scope: Scope,
    filter: String,
    /// The file in front's notes, and when and for which file they were found.
    here: Vec<Note>,
    looked: Option<(Instant, u64, usize)>,
    /// The project's notes, and the search for them if it's still going.
    across: Vec<Note>,
    job: Option<Receiver<Vec<Note>>>,
    across_from: Option<PathBuf>,
}

/// What the panel asks of the editor.
pub enum TodoAct {
    /// The file in front, at a line from 0.
    GoToLine(usize),
    /// Another file, at a line from 0.
    Open(PathBuf, usize),
}

/// Reads a project's files for notes, on a thread of its own.
fn look_through(files: Vec<PathBuf>) -> Receiver<Vec<Note>> {
    let (send, receive) = channel();
    std::thread::spawn(move || {
        let mut found = Vec::new();
        for path in files.into_iter().take(MOST_FILES) {
            if std::fs::metadata(&path).map_or(true, |m| m.len() > LARGEST_FILE) {
                continue;
            }
            let Ok(bytes) = std::fs::read(&path) else { continue };
            if crate::codec::looks_binary(&bytes) {
                continue;
            }
            let text = String::from_utf8_lossy(&bytes);
            found.extend(notes_in(text.lines(), Some(&path)));
            if found.len() >= MOST_NOTES {
                found.truncate(MOST_NOTES);
                break;
            }
        }
        let _ = send.send(found);
    });
    receive
}

impl TodoPanel {
    /// `id` stands for the file in front, and `lines` gives its text; `files` are the project's, if one is open.
    pub fn draw(&mut self, ui: &mut Ui, id: u64, line_count: usize, lines: &dyn Fn(usize) -> String, root: Option<&Path>, files: &[PathBuf]) -> Vec<TodoAct> {
        let mut acts = Vec::new();
        ui.horizontal(|ui| {
            ui.selectable_value(&mut self.scope, Scope::ThisFile, crate::i18n::tr("This file"));
            ui.add_enabled_ui(root.is_some(), |ui| {
                ui.selectable_value(&mut self.scope, Scope::Project, crate::i18n::tr("The project")).on_disabled_hover_text(crate::i18n::tr("Open a folder or a project first"));
            });
            if self.scope == Scope::Project && ui.small_button(crate::i18n::tr("\u{27F3}")).on_hover_text(crate::i18n::tr("Look again")).clicked() {
                self.across_from = None;
            }
        });
        ui.add(egui::TextEdit::singleline(&mut self.filter).hint_text(crate::i18n::tr("Filter")).desired_width(f32::INFINITY));
        if root.is_none() {
            self.scope = Scope::ThisFile;
        }

        // The file in front: looked through again now and then, and when it changes.
        if self.scope == Scope::ThisFile {
            let stale = self.looked.is_none_or(|(when, was, count)| was != id || count != line_count || when.elapsed() >= LOOK_AGAIN);
            if stale {
                self.here = notes_in((0..line_count.min(200_000)).map(lines), None);
                self.looked = Some((Instant::now(), id, line_count));
            }
        } else if let Some(root) = root {
            if self.across_from.as_deref() != Some(root) {
                self.across_from = Some(root.to_path_buf());
                self.across.clear();
                self.job = Some(look_through(files.to_vec()));
            }
            if let Some(job) = &self.job {
                match job.try_recv() {
                    Ok(found) => {
                        self.across = found;
                        self.job = None;
                    }
                    Err(std::sync::mpsc::TryRecvError::Empty) => {
                        ui.horizontal(|ui| {
                            ui.spinner();
                            ui.label(crate::i18n::tr("Looking through the project\u{2026}"));
                        });
                        ui.ctx().request_repaint_after(Duration::from_millis(200));
                    }
                    Err(_) => self.job = None,
                }
            }
        }

        let notes = if self.scope == Scope::ThisFile { &self.here } else { &self.across };
        let filter = self.filter.to_lowercase();
        let shown: Vec<&Note> = notes.iter().filter(|n| filter.is_empty() || n.text.to_lowercase().contains(&filter) || n.word.to_lowercase().contains(&filter)).collect();
        if shown.is_empty() && self.job.is_none() {
            ui.label(RichText::new(crate::i18n::tr("No TODO, FIXME, BUG, HACK or XXX notes in comments.")).color(crate::theme::text_faint()).size(12.0));
            return acts;
        }
        egui::ScrollArea::both().id_salt("todo-scroll").auto_shrink([false, false]).show(ui, |ui| {
            let mut last_file: Option<&Path> = None;
            for note in shown {
                // Across the project, each file's name above its notes.
                if let Some(path) = note.path.as_deref()
                    && last_file != Some(path)
                {
                    last_file = Some(path);
                    let name = root.and_then(|r| path.strip_prefix(r).ok()).unwrap_or(path).display().to_string();
                    ui.add_space(3.0);
                    ui.label(RichText::new(name).strong().size(12.0));
                }
                let tag = WORDS.iter().find(|(w, _)| *w == note.word).map_or(2, |(_, t)| *t);
                let mut job = egui::text::LayoutJob::default();
                let font = egui::FontId::proportional(12.5);
                job.append(note.word, 0.0, egui::TextFormat::simple(font.clone(), crate::theme::tag_colour(tag)));
                job.append(&format!("  {}", note.text), 0.0, egui::TextFormat::simple(font.clone(), crate::theme::text()));
                job.append(&format!("  {}", note.line + 1), 0.0, egui::TextFormat::simple(font, crate::theme::text_faint()));
                let response = ui.add(egui::Button::selectable(false, job).frame_when_inactive(false).wrap_mode(egui::TextWrapMode::Extend));
                if response.clicked() {
                    acts.push(match &note.path {
                        Some(path) => TodoAct::Open(path.clone(), note.line),
                        None => TodoAct::GoToLine(note.line),
                    });
                }
            }
        });
        acts
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn notes_are_found_in_comments_and_nowhere_else() {
        let lines = [
            "let todo_count = 0; // TODO: count them",
            "x = TODO_LIMIT  # not one",
            "    ; FIXME(jo) the carry's wrong",
            "print(\"TODO\")",
            "/* HACK around the bug */",
            "<!-- XXX -->",
        ];
        let notes = notes_in(lines.iter(), None);
        let found: Vec<(usize, &str, &str)> = notes.iter().map(|n| (n.line, n.word, n.text.as_str())).collect();
        assert_eq!(found, [(0, "TODO", "count them"), (2, "FIXME", "jo) the carry's wrong"), (4, "HACK", "around the bug"), (5, "XXX", "")]);
    }

    #[test]
    fn a_projects_files_are_looked_through() {
        let dir = std::env::temp_dir().join(format!("limoniter-todo-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("a.c"), "int x; // TODO tidy\n").unwrap();
        std::fs::write(dir.join("b.bin"), [0u8, 1, 2, 0, 0, 0]).unwrap();
        let found = look_through(vec![dir.join("a.c"), dir.join("b.bin")]).recv().unwrap();
        assert_eq!(found, [Note { path: Some(dir.join("a.c")), line: 0, word: "TODO", text: "tidy".into() }]);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
