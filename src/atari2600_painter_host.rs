//! The window's side of the Atari 2600 graphics editor: its window, the files it opens and saves, the document
//! it reads from and writes into, and the program it opens in a new tab.

use super::*;
use crate::atari2600_painter::{Asked, Editor, Part, Rip};

const TITLE: &str = "Atari 2600 graphics";

impl Limoniter {
    pub(super) fn open_atari2600_graphics(&mut self) {
        if self.atari2600_graphics.is_none() {
            self.atari2600_graphics = Some(Box::default());
        }
    }

    pub(super) fn atari2600_graphics_window(&mut self, ctx: &Context) {
        let Some(mut editor) = self.atari2600_graphics.take() else {
            return;
        };
        let mut open = true;
        let mut asked = None;
        crate::fit::Dialog::new(crate::i18n::tr(TITLE)).open(&mut open).resizable(false).default_pos([120.0, 60.0]).show(ctx, |ui| {
            asked = editor.draw(ui);
        });
        if !open || asked == Some(Asked::Close) {
            return;
        }
        if let Some(asked) = asked
            && let Err(problem) = self.atari2600_asked(&mut editor, asked)
        {
            self.message(TITLE, problem);
        }
        self.atari2600_graphics = Some(editor);
    }

    fn atari2600_asked(&mut self, editor: &mut Editor, asked: Asked) -> Result<(), String> {
        let write = |path: &Path, bytes: &[u8]| codec::write_file(path, bytes).map_err(|e| crate::i18n::tf!("Couldn't save {}.\n\n{e}", path.display()));
        let read = |path: &Path| std::fs::read(path).map_err(|e| crate::i18n::tf!("Couldn't read {}.\n\n{e}", path.display()));
        let name = crate::quantise::identifier(&editor.label);
        let what = if editor.part == Part::Sprites { "sprites" } else { "a playfield" };
        match asked {
            Asked::Open => {
                if let Some(path) = rfd::FileDialog::new().set_title(crate::i18n::tr("Open source")).add_filter(crate::i18n::tr("Source"), &["asm", "s", "a", "inc", "h", "txt"]).add_filter(crate::i18n::tr("Any file"), &["*"]).pick_file() {
                    let text = String::from_utf8_lossy(&read(&path)?).into_owned();
                    if !editor.read_source(&text) {
                        return Err(crate::i18n::tf!("{} has no tables of {what} in it (.byte lines{}).", path.display(), if editor.part == Part::Playfield { " labelled pf0, pf1 and pf2" } else { "" }));
                    }
                }
            }
            Asked::OpenCartridge => {
                if let Some(path) = rfd::FileDialog::new().set_title(crate::i18n::tr("Look through a cartridge")).add_filter(crate::i18n::tr("Cartridges"), &["a26", "bin", "rom"]).add_filter(crate::i18n::tr("Any file"), &["*"]).pick_file() {
                    let bytes = read(&path)?;
                    let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
                    editor.rip = Some(Rip { name, bytes, offset: 0, count: 1 });
                }
            }
            Asked::BringIn => {
                if let Some(path) = rfd::FileDialog::new().set_title(crate::i18n::tr("Bring in a picture")).add_filter(crate::i18n::tr("Pictures"), retro_converter_host::PICTURES).add_filter(crate::i18n::tr("Any file"), &["*"]).pick_file() {
                    let file = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
                    let image = crate::quantise::read_picture(&file, &read(&path)?).map_err(|e| crate::i18n::tf!("Couldn't open {}.\n\n{e}", path.display()))?;
                    editor.remember();
                    let f = &editor.field;
                    editor.field = crate::atari2600_gfx::Playfield::from_picture(&image, editor.tv, f.halves, f.rows, f.lines, editor.fit, editor.one_background);
                }
            }
            Asked::Insert => self.insert_from_tool(TITLE, &editor.source()),
            Asked::FromSelection => {
                let selected = if self.no_file() { String::new() } else { self.docs[self.active].text.selection_text() };
                if !editor.read_source(&selected) {
                    return Err(crate::i18n::tf!("Select the .byte lines of {what} in the document first{}.", if editor.part == Part::Playfield { ", their labels ending pf0, pf1 and pf2" } else { "" }));
                }
            }
            Asked::SaveSource => {
                if let Some(path) = rfd::FileDialog::new().set_title(crate::i18n::tr("Save the source")).set_file_name(format!("{name}.asm")).add_filter(crate::i18n::tr("Source"), &["asm", "s", "inc"]).save_file() {
                    write(&path, crate::i18n::comments(&editor.source()).as_bytes())?;
                }
            }
            Asked::SaveBytes => {
                if let Some(path) = rfd::FileDialog::new().set_title(crate::i18n::tr("Save the bytes")).set_file_name(format!("{name}.bin")).add_filter(crate::i18n::tr("Bytes"), &["bin"]).save_file() {
                    write(&path, &editor.bytes())?;
                }
            }
            Asked::OpenProgram => {
                if self.no_file() {
                    self.docs.clear();
                }
                let mut doc = Document::untitled(format!("{name}.asm"), &PRESETS[0]);
                doc.text.set_content(crate::i18n::comments(&editor.program()).lines().map(str::to_string).collect());
                highlight::load_now();
                if let Some((_, syntax)) = highlight::engine().and_then(|e| e.names().iter().find(|(n, _)| n == "Assembly 6502")) {
                    doc.set_language(Language::Named(*syntax));
                }
                doc.update_syntax();
                self.docs.push(doc);
                self.active = self.docs.len() - 1;
                self.pending_focus = true;
            }
            Asked::SaveCartridge => {
                let cartridge = cartridge(&editor.program())?;
                if let Some(path) = rfd::FileDialog::new().set_title(crate::i18n::tr("Save the cartridge")).set_file_name(format!("{name}.a26")).add_filter(crate::i18n::tr("Atari 2600 cartridge"), &["a26", "bin"]).save_file() {
                    write(&path, &cartridge)?;
                }
            }
            Asked::Close => {}
        }
        Ok(())
    }
}

/// A program built into its cartridge by Limoniter's own assembler.
pub(super) fn cartridge(source: &str) -> Result<Vec<u8>, String> {
    let assembly = crate::asm::assemble(&crate::asm::m6502::M6502, Path::new("graphics.asm"), source, &|_| Err(crate::i18n::tr("no").into()));
    match (assembly.problems.first(), assembly.lowest(), assembly.highest()) {
        (None, Some(low), Some(high)) => Ok(assembly.image(low, high)),
        (Some(problem), _, _) => Err(crate::i18n::tf!("The program didn't build: {} (line {}).", problem.message, problem.line + 1)),
        _ => Err(crate::i18n::tr("The program made nothing.").into()),
    }
}

#[cfg(test)]
mod tests {
    use super::super::tests::app_with;
    use super::*;

    #[test]
    fn the_editor_draws_both_parts_and_its_program_opens_to_run() {
        let (mut app, ctx) = app_with("");
        app.open_atari2600_graphics();
        for part in [Part::Sprites, Part::Playfield] {
            app.atari2600_graphics.as_mut().unwrap().part = part;
            for _ in 0..3 {
                ctx.run_ui(Default::default(), |ui| app.atari2600_graphics_window(ui.ctx())).textures_delta.clear();
            }
            let mut editor = app.atari2600_graphics.take().unwrap();
            app.atari2600_asked(&mut editor, Asked::OpenProgram).unwrap();
            app.atari2600_graphics = Some(editor);
            let doc = &app.docs[app.active].text;
            let text = (0..doc.line_count()).map(|i| doc.line(i).to_string()).collect::<Vec<_>>().join("\n");
            assert!(text.starts_with("; machine: atari 2600"), "{text}");
            assert_eq!(cartridge(&text).unwrap().len(), 4096);
        }
        // Put in the document and read back from a selection of it.
        app.new_document();
        let mut editor = app.atari2600_graphics.take().unwrap();
        editor.part = Part::Sprites;
        app.atari2600_asked(&mut editor, Asked::Insert).unwrap();
        let before = editor.sprites.frames.clone();
        editor.sprites.frames[0].rows[0] = 0xFF;
        app.docs[app.active].text.select_all();
        app.atari2600_asked(&mut editor, Asked::FromSelection).unwrap();
        assert_eq!(editor.sprites.frames, before);
    }
}
