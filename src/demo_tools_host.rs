//! The window's side of the tools for Amiga demo code: the blitter minterm calculator and the table generator,
//! each a window kept open beside the text, putting what it makes at the cursor.

use super::*;

/// The demo tools' windows, while they're open.
#[derive(Default)]
pub(super) struct DemoTools {
    pub blitter: Option<Box<crate::blitter::Blitter>>,
    pub tables: Option<Box<crate::table_gen::Generator>>,
}

impl Limoniter {
    pub(super) fn open_blitter_calculator(&mut self) {
        self.demo_tools.blitter.get_or_insert_with(Box::default);
    }

    /// Opens the table generator, writing tables the way the file in front's language reads them when it first opens.
    pub(super) fn open_table_generator(&mut self) {
        if self.demo_tools.tables.is_none() {
            let mut generator = Box::<crate::table_gen::Generator>::default();
            if let Some(form) = crate::table_gen::Form::for_language(&self.docs[self.active].language_name()) {
                generator.form = form;
            }
            self.demo_tools.tables = Some(generator);
        }
    }

    /// Text from a tool, at the cursor in the file in front, as one undoable change.
    fn insert_from_tool(&mut self, title: &str, text: &str) {
        if self.no_file() || self.docs[self.active].hex_mode || self.docs[self.active].text.read_only {
            self.message(title, "Open the source to put it in, with the cursor where it goes.".into());
            return;
        }
        let doc = &mut self.docs[self.active];
        doc.text.begin_undo_group(false);
        doc.text.type_text(text);
        doc.text.end_undo_group();
    }

    pub(super) fn demo_tools_windows(&mut self, ctx: &Context) {
        if let Some(mut blitter) = self.demo_tools.blitter.take() {
            let mut open = true;
            let mut asked = None;
            egui::Window::new("Blitter minterms").open(&mut open).resizable(false).default_pos([220.0, 90.0]).show(ctx, |ui| {
                asked = blitter.draw(ui);
            });
            if let Some(crate::blitter::Asked::Insert(text)) = asked {
                self.insert_from_tool("Blitter minterms", &text);
            }
            if open {
                self.demo_tools.blitter = Some(blitter);
            }
        }
        if let Some(mut generator) = self.demo_tools.tables.take() {
            let mut open = true;
            let mut asked = None;
            egui::Window::new("Table generator").open(&mut open).resizable(false).default_pos([260.0, 110.0]).show(ctx, |ui| {
                asked = generator.draw(ui);
            });
            match asked {
                Some(crate::table_gen::Asked::Insert(text)) => self.insert_from_tool("Table generator", &text),
                Some(crate::table_gen::Asked::Save(bytes, name)) => {
                    if let Some(path) = rfd::FileDialog::new().set_title("Save the table").set_file_name(name).add_filter("Binary file", &["bin", "dat", "raw"]).add_filter("Any file", &["*"]).save_file()
                        && let Err(e) = codec::write_file(&path, &bytes)
                    {
                        self.message("Table generator", format!("Couldn't save {}.\n\n{e}", path.display()));
                    }
                }
                None => {}
            }
            if open {
                self.demo_tools.tables = Some(generator);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::tests::app_with;

    #[test]
    fn the_demo_tools_draw_and_insert() {
        let (mut app, ctx) = app_with("");
        app.open_blitter_calculator();
        app.open_table_generator();
        for _ in 0..3 {
            ctx.run_ui(Default::default(), |ui| app.demo_tools_windows(ui.ctx())).textures_delta.clear();
        }
        assert!(app.demo_tools.blitter.is_some() && app.demo_tools.tables.is_some());
        let table = app.demo_tools.tables.as_mut().unwrap().text();
        app.insert_from_tool("Table generator", &table);
        assert_eq!(&*app.doc().text.line(0), "sine:");
        assert_eq!(&*app.doc().text.line(1), "        dc.w    $0000,$0192,$0324,$04B5,$0646,$07D6,$0964,$0AF1");
    }

    #[test]
    fn the_amiga_starters_are_on_new_as() {
        let (mut app, _ctx) = app_with("");
        for (name, text) in [("Amiga demo (68000)", crate::amiga_starters::DEMO), ("Amiga copper only (68000)", crate::amiga_starters::COPPER)] {
            let index = app.new_kinds.iter().position(|k| k.name == name).unwrap();
            app.new_of_kind(index);
            assert_eq!(app.docs[app.active].text.all_lines().len(), text.lines().count());
            assert_eq!(app.doc_cpu(app.active), Some(crate::assemblers::Cpu::M68k));
        }
    }
}
