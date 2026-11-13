//! The window's side of knowing the code: Go to definition (F12), Find all uses (Shift+F12) and Rename (Ctrl+F12)
//! for the labels in assembler source, across the files it INCLUDEs.

use super::*;
use crate::asm::Place;

/// The box renaming a label.
pub(super) struct RenameBox {
    pub name: String,
    pub written: String,
    pub new: String,
    pub error: String,
}

/// Where a whole word is on a line, by characters: each start.
fn word_starts(line: &str, word: &str) -> Vec<usize> {
    let chars: Vec<char> = line.chars().collect();
    let target: Vec<char> = word.chars().collect();
    let part = |c: char| c.is_alphanumeric() || matches!(c, '_' | '.' | '@' | '?');
    let mut found = Vec::new();
    let mut i = 0;
    while i + target.len() <= chars.len() {
        let matches = chars[i..i + target.len()].iter().zip(&target).all(|(a, b)| a == b);
        let before = i == 0 || !part(chars[i - 1]);
        let after = chars.get(i + target.len()).is_none_or(|c| !part(*c));
        if matches && before && after {
            found.push(i);
            i += target.len();
        } else {
            i += 1;
        }
    }
    found
}

impl Limoniter {
    /// The label at the cursor, in full and as written, with what's known about the code, brought up to date first.
    fn label_at_cursor(&mut self, what: &str) -> Option<(String, String, crate::code_intel::CodeIntel)> {
        self.refresh_intel();
        let doc = &self.docs[self.active];
        let Some(intel) = doc.text.intel.as_deref().filter(|i| i.assembly.is_some()) else {
            self.message(what, "This works in Z80, 6502, 6809 and 68000 source.".into());
            return None;
        };
        let (line, col) = (doc.text.caret_line, doc.text.caret_col);
        let text = doc.text.line(line);
        let found = intel.symbol_at(line, &text, col).zip(crate::code_intel::word_at(&text, col).map(|w| w.2));
        let Some((name, written)) = found else {
            self.message(what, "Put the cursor on a label first.".into());
            return None;
        };
        Some((name, written.trim_end_matches(':').to_string(), intel.clone()))
    }

    /// Assembles the source in front now, if it's changed since it was last, rather than waiting for typing to stop.
    pub(super) fn refresh_intel(&mut self) {
        let doc = &self.docs[self.active];
        if doc.text.intel.as_ref().is_some_and(|i| i.cpu.is_some() && i.version != doc.text.version()) {
            self.intel_wait = Some((doc.id, doc.text.version(), std::time::Instant::now() - Duration::from_secs(1)));
        }
        self.assembly_tick();
    }

    /// The path of one of an assembly's files, as Go to place wants it.
    fn place_file(intel: &crate::code_intel::CodeIntel, place: &Place) -> String {
        intel.assembly.as_ref().and_then(|a| a.files.get(place.file)).map(|f| f.display().to_string()).unwrap_or_default()
    }

    /// Goes to where the label at the cursor is defined, in whichever file that is.
    pub(super) fn go_to_definition(&mut self) {
        if self.doc_cpu(self.active).is_none() && self.has_lsp() {
            return self.lsp_go_to_definition();
        }
        let Some((name, _, intel)) = self.label_at_cursor("Go to definition") else {
            return;
        };
        let Some(place) = intel.definition(&name).cloned() else {
            return self.message("Go to definition", format!("{name} isn't defined anywhere Limoniter's assembler can see."));
        };
        let file = Self::place_file(&intel, &place);
        self.go_to_place(&file, place.line + 1);
        let doc = &mut self.docs[self.active];
        if !doc.hex_mode {
            let text = doc.text.line(place.line);
            let col = word_starts(&text, &place.written).first().copied().unwrap_or(0);
            doc.text.move_caret(place.line, col, false);
            doc.text.reveal = true;
        }
    }

    /// Lists where the label at the cursor is defined and used in the Output panel, each line a click away.
    pub(super) fn find_uses(&mut self) {
        if self.doc_cpu(self.active).is_none() && self.has_lsp() {
            return self.lsp_find_uses();
        }
        let Some((name, _, intel)) = self.label_at_cursor("Find all uses") else {
            return;
        };
        let places = intel.places(&name);
        let defined = intel.definition(&name).cloned();
        self.script_print(format!("{name}: {} place{}", places.len(), if places.len() == 1 { "" } else { "s" }), true);
        for place in places {
            let file = Self::place_file(&intel, &place);
            let shown = Path::new(&file).file_name().map_or_else(|| file.clone(), |f| f.to_string_lossy().into_owned());
            let text = self.line_of_file(&file, place.line).unwrap_or_default();
            let mark = if defined.as_ref().is_some_and(|d| d.file == place.file && d.line == place.line) { "  (defined)" } else { "" };
            self.script_print(format!("{shown}({}): {}{mark}", place.line + 1, text.trim()), false);
        }
    }

    /// A line of a file: from its tab if it's open, otherwise from the disk.
    fn line_of_file(&self, file: &str, line: usize) -> Option<String> {
        let path = Path::new(file);
        if let Some(doc) = self.docs.iter().find(|d| d.path.as_deref().is_some_and(|p| same_path(p, path)) || d.name() == file) {
            return (line < doc.text.line_count()).then(|| doc.text.line(line).to_string());
        }
        std::fs::read_to_string(path).ok()?.lines().nth(line).map(str::to_string)
    }

    pub(super) fn open_rename_box(&mut self) {
        // With a language server, it says what's renamed: the box asks only for the new name.
        if self.doc_cpu(self.active).is_none() && self.has_lsp() {
            let doc = &self.docs[self.active];
            let word = crate::code_intel::word_at(&doc.text.line(doc.text.caret_line), doc.text.caret_col).map(|w| w.2).unwrap_or_default();
            self.dialog = Dialog::RenameLabel(Box::new(RenameBox { name: String::new(), new: word.clone(), written: word, error: String::new() }));
            return;
        }
        let Some((name, written, _)) = self.label_at_cursor("Rename") else {
            return;
        };
        self.dialog = Dialog::RenameLabel(Box::new(RenameBox { name, new: written.clone(), written, error: String::new() }));
    }

    pub(super) fn rename_dialog(&mut self, ctx: &Context) -> bool {
        let Dialog::RenameLabel(state) = &mut self.dialog else {
            return false;
        };
        let mut close = ctx.input(|i| i.key_pressed(Key::Escape));
        let mut go = false;
        egui::Window::new("Rename").collapsible(false).resizable(false).anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0]).show(ctx, |ui| {
            ui.label(format!("Rename {} everywhere it's defined and used, in every file the source includes:", state.written));
            let field = ui.text_edit_singleline(&mut state.new);
            field.request_focus();
            go = field.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter));
            if !state.error.is_empty() {
                ui.colored_label(theme::change_unsaved(), &state.error);
            }
            ui.horizontal(|ui| {
                go |= ui.button("Rename").clicked();
                close |= ui.button("Cancel").clicked();
            });
        });
        if go {
            let Dialog::RenameLabel(state) = &mut self.dialog else {
                return true;
            };
            let new = state.new.trim().to_string();
            let local = state.written.chars().next().filter(|c| matches!(c, '.' | '@' | '_') && state.written.len() > 1);
            let valid = !new.is_empty() && new.chars().all(|c| c.is_alphanumeric() || matches!(c, '_' | '.' | '@' | '?')) && !new.starts_with(|c: char| c.is_ascii_digit());
            if !valid {
                state.error = "A label is letters, digits and _, and doesn't start with a digit.".into();
                return true;
            }
            if !state.name.is_empty() && local.is_some() != new.starts_with(['.', '@']) && local != Some('_') {
                state.error = format!("{} is a local label, so its new name starts with {} too.", state.written, local.unwrap_or('.'));
                return true;
            }
            let (name, written) = (state.name.clone(), state.written.clone());
            self.dialog = Dialog::None;
            if name.is_empty() {
                self.lsp_rename(&new);
            } else {
                self.rename_label(&name, &written, &new);
            }
        } else if close {
            self.dialog = Dialog::None;
            self.pending_focus = true;
        }
        true
    }

    /// Renames a label wherever it's defined and used. Files not open are opened, so the change can be seen and
    /// undone; each file's change comes off with one Undo.
    pub(super) fn rename_label(&mut self, name: &str, written: &str, new: &str) {
        self.refresh_intel();
        let start = self.active;
        let Some(intel) = self.docs[start].text.intel.as_deref().cloned() else {
            return;
        };
        if intel.assembly.as_ref().is_some_and(|a| a.symbols.keys().any(|k| k == &name.replacen(written, new, 1)) && written != new) {
            return self.message("Rename", format!("There's already a label called {new}."));
        }
        let mut by_file: std::collections::BTreeMap<String, std::collections::BTreeSet<usize>> = Default::default();
        for place in intel.places(name) {
            by_file.entry(Self::place_file(&intel, &place)).or_default().insert(place.line);
        }
        let mut changed = 0;
        for (file, lines) in by_file {
            let own = intel.assembly.as_ref().and_then(|a| a.files.get(intel.file)).map(|f| f.display().to_string());
            let index = if own.as_deref() == Some(file.as_str()) {
                Some(start)
            } else {
                let path = Path::new(&file);
                match self.docs.iter().position(|d| d.path.as_deref().is_some_and(|p| same_path(p, path))) {
                    Some(i) => Some(i),
                    None => self.open_path(path).then_some(self.active),
                }
            };
            let Some(index) = index else {
                continue;
            };
            let buffer = &mut self.docs[index].text;
            buffer.begin_undo_group(false);
            for line in lines {
                let text = buffer.line(line).to_string();
                // From the right, so earlier places on the line stay where they are.
                for col in word_starts(&text, written).into_iter().rev() {
                    buffer.move_caret(line, col, false);
                    buffer.move_caret(line, col + written.chars().count(), true);
                    buffer.type_text(new);
                    changed += 1;
                }
            }
            buffer.end_undo_group();
        }
        self.active = start;
        self.script_print(format!("Renamed {written} to {new} in {changed} place{}.", if changed == 1 { "" } else { "s" }), true);
    }
}

#[cfg(test)]
mod tests {
    use super::super::tests::app_with;
    use super::*;

    /// A program in two files, the second INCLUDEd, saved in a folder of their own, with the first open.
    fn two_files(app: &mut Limoniter, tag: &str) -> std::path::PathBuf {
        crate::highlight::load_now();
        let folder = std::env::temp_dir().join(format!("limoniter-intel-{}-{tag}", std::process::id()));
        let _ = std::fs::remove_dir_all(&folder);
        std::fs::create_dir_all(&folder).unwrap();
        std::fs::write(folder.join("lib.asm"), "; the library\nborder: ld a,2\n  out (254),a\n  ret\n").unwrap();
        std::fs::write(folder.join("main.asm"), "  org 32768\nmain: ld b,8\n.loop: call border\n  djnz .loop\n  ld a,(count)\n  jp main\ncount: db 3\n  include \"lib.asm\"\n").unwrap();
        assert!(app.open_path(&folder.join("main.asm")));
        app.doc().update_syntax();
        folder
    }

    #[test]
    fn labels_are_gone_to_found_and_renamed_across_files() {
        let (mut app, _ctx) = app_with("");
        let folder = two_files(&mut app, "rename");
        app.refresh_intel();
        let intel = app.doc().text.intel.clone().expect("assembler source is known");
        assert!(intel.assembly.as_ref().unwrap().ok(), "{:?}", intel.assembly.as_ref().unwrap().problems);

        // F12 on "border" goes into lib.asm.
        app.doc().text.move_caret(2, 13, false);
        app.go_to_definition();
        assert!(app.doc().path.as_deref().is_some_and(|p| p.ends_with("lib.asm")), "{:?}", app.doc().path);
        assert_eq!((app.doc().text.caret_line, app.doc().text.caret_col), (1, 0));

        // Shift+F12 on "main" lists both places.
        let main = app.docs.iter().position(|d| d.path.as_deref().is_some_and(|p| p.ends_with("main.asm"))).unwrap();
        app.active = main;
        app.doc().text.move_caret(5, 5, false);
        app.find_uses();
        assert!(app.script_output.iter().any(|l| l == "main: 2 places"), "{:?}", app.script_output);
        assert!(app.script_output.iter().any(|l| l.starts_with("main.asm(2): main: ld b,8  (defined)")), "{:?}", app.script_output);
        assert!(app.script_output.iter().any(|l| l.starts_with("main.asm(6): jp main")));

        // Ctrl+F12 renames border everywhere, in both files.
        app.doc().text.move_caret(2, 13, false);
        app.rename_label("border", "border", "set_border");
        assert_eq!(&*app.docs[main].text.line(2), ".loop: call set_border");
        let lib = app.docs.iter().find(|d| d.path.as_deref().is_some_and(|p| p.ends_with("lib.asm"))).unwrap();
        assert_eq!(&*lib.text.line(1), "set_border: ld a,2");
        assert_eq!(&*lib.text.line(0), "; the library", "a comment that doesn't mention it is left alone");
        // A local label is renamed only under its own label.
        app.active = main;
        app.refresh_intel();
        app.rename_label("main.loop", ".loop", ".again");
        assert_eq!(&*app.docs[main].text.line(3), "  djnz .again");
        let _ = std::fs::remove_dir_all(folder);
    }

    #[test]
    fn problems_are_marked_as_you_type_and_the_outline_follows() {
        let (mut app, _ctx) = app_with("");
        let folder = two_files(&mut app, "problems");
        app.refresh_intel();
        assert!(app.doc().text.code.as_ref().unwrap().problems.is_empty());
        app.doc().text.move_caret(4, 0, false);
        app.doc().text.type_text("  ld q,1\n");
        app.refresh_intel();
        let problems = app.doc().text.code.as_ref().unwrap().problems.clone();
        assert_eq!(problems.iter().map(|p| p.0).collect::<Vec<_>>(), [4], "marked without pressing F9: {problems:?}");
        let outline = crate::panels::assembler_outline(app.doc().text.intel.as_deref().unwrap()).unwrap();
        let names: Vec<&str> = outline.iter().map(|(_, n, _)| n.as_str()).collect();
        assert_eq!(names, ["at $8000", "main", "    .loop", "count"], "{outline:?}");
        assert_eq!(outline[1].2, "$8000");
        let _ = std::fs::remove_dir_all(folder);
    }

    #[test]
    fn whole_words_are_found_and_nothing_else() {
        assert_eq!(word_starts("main: jp main ; mainly", "main"), [0, 9]);
        assert_eq!(word_starts(".loop: djnz .loop", ".loop"), [0, 12]);
        assert_eq!(word_starts("  ld a,count+count2", "count"), [7]);
    }
}
