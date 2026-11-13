//! The window's side of the character and sprite editor: its window, kept open beside the text, and what it asks
//! for: files, the source at the cursor, the selection, and a Spectrum listing's UDG pictures.

use super::*;
use crate::pixel_editor::{Asked, Editor, Kind, Sheet};

impl Limoniter {
    pub(super) fn pixel_editor_window(&mut self, ctx: &Context) {
        let Some(mut editor) = self.pixel_editor.take() else {
            return;
        };
        let spectrum_basic = !self.no_file() && self.docs[self.active].text.zx.is_some();
        let mut open = true;
        let mut asked = None;
        egui::Window::new("Characters and sprites").open(&mut open).resizable(false).default_pos([200.0, 120.0]).show(ctx, |ui| {
            asked = editor.draw(ui, spectrum_basic);
        });
        if !open {
            asked = Some(Asked::Close);
        }
        let Some(asked) = asked else {
            self.pixel_editor = Some(editor);
            return;
        };
        match asked {
            Asked::Close => return,
            Asked::Load => {
                if let Some(path) = rfd::FileDialog::new().set_title("Open characters or sprites").add_filter("UDGs, fonts and sprites", &["udg", "chr", "ch8", "fnt", "spr", "bin"]).add_filter("Any file", &["*"]).pick_file() {
                match std::fs::read(&path) {
                    Ok(bytes) => {
                        let sprites = path.extension().is_some_and(|e| e.eq_ignore_ascii_case("spr"));
                        let kind = if sprites { Kind::Sprites } else { editor.sheet.kind };
                        *editor = Editor::new(kind);
                        editor.sheet = Sheet::from_bytes(kind, &bytes);
                    }
                    Err(e) => self.message("Characters and sprites", format!("Couldn't read {}.\n\n{e}", path.display())),
                }
                }
            }
            Asked::Save => {
                let (filter, ending) = if editor.sheet.kind == Kind::Sprites { ("Next sprites", "spr") } else { ("UDGs or a font", "udg") };
                if let Some(path) = rfd::FileDialog::new().set_title("Save the characters or sprites").set_file_name(format!("{}.{ending}", editor.label)).add_filter(filter, &[ending, "chr", "bin"]).save_file()
                    && let Err(e) = codec::write_file(&path, &editor.sheet.to_bytes())
                {
                    self.message("Characters and sprites", format!("Couldn't save {}.\n\n{e}", path.display()));
                }
            }
            Asked::Insert => {
                let text = editor.sheet.to_db(&editor.label);
                if self.no_file() || self.docs[self.active].hex_mode || self.docs[self.active].text.read_only {
                    self.message("Characters and sprites", "Open the source to put them in, with the cursor where they go.".into());
                } else {
                    let doc = &mut self.docs[self.active];
                    doc.text.begin_undo_group(false);
                    doc.text.type_text(&text);
                    doc.text.end_undo_group();
                }
            }
            Asked::FromSelection => {
                let kind = editor.sheet.kind;
                let selected = if self.no_file() { String::new() } else { self.docs[self.active].text.selection_text() };
                match Sheet::from_text(kind, &selected) {
                    Some(sheet) => {
                        editor.sheet = sheet;
                        editor.current = 0;
                    }
                    None => self.message("Characters and sprites", "Select DB lines (DB, DEFB, !byte or FCB) whose numbers are the pictures.".into()),
                }
            }
            Asked::UseAsUdgs => {
                let bytes = editor.sheet.to_bytes();
                let index = self.active;
                self.give_udg_pictures(index, crate::zx_graphics::udgs_from_bytes(&bytes, 0), "the character editor".into());
            }
        }
        self.pixel_editor = Some(editor);
    }
}

#[cfg(test)]
mod tests {
    use super::super::tests::app_with;
    use super::*;

    #[test]
    fn the_window_draws_in_both_kinds() {
        let (mut app, ctx) = app_with("");
        for kind in [Kind::Characters, Kind::Sprites] {
            app.pixel_editor = Some(Box::new(Editor::new(kind)));
            for _ in 0..3 {
                ctx.run_ui(Default::default(), |ui| app.pixel_editor_window(ui.ctx())).textures_delta.clear();
            }
            assert!(app.pixel_editor.is_some());
        }
    }

    #[test]
    fn characters_go_into_the_source_and_come_back_from_it() {
        let (mut app, _ctx) = app_with("");
        app.new_document();
        let mut editor = Editor::new(Kind::Characters);
        editor.sheet = Sheet::from_bytes(Kind::Characters, &[0x18, 0x3C, 0x7E, 0xFF, 0xFF, 0x7E, 0x3C, 0x18]);
        editor.label = "ship".into();
        app.pixel_editor = Some(Box::new(editor));
        // Put in the source, as the button does.
        let text = app.pixel_editor.as_ref().unwrap().sheet.to_db("ship");
        app.doc().text.type_text(&text);
        assert_eq!(&*app.doc().text.line(1), "        db $18,$3C,$7E,$FF,$FF,$7E,$3C,$18");
        // Read back from a selection.
        app.doc().text.move_caret(0, 0, false);
        app.doc().text.move_caret(2, 0, true);
        let back = Sheet::from_text(Kind::Characters, &app.doc().text.selection_text()).unwrap();
        assert_eq!(back.to_bytes(), [0x18, 0x3C, 0x7E, 0xFF, 0xFF, 0x7E, 0x3C, 0x18]);
    }
}
