//! The window's side of the sound effects designer: its window, the files it opens and saves, the document it reads
//! from and writes into, and the program it opens in a new tab.

use super::*;
use crate::sfx_designer::{Asked, Editor};

const TITLE: &str = "Sound effects";

impl Limoniter {
    pub(super) fn open_sound_effects(&mut self) {
        if self.sound_effects.is_none() {
            self.sound_effects = Some(Box::default());
        }
    }

    pub(super) fn sound_effects_window(&mut self, ctx: &Context) {
        let Some(mut editor) = self.sound_effects.take() else {
            return;
        };
        let mut open = true;
        let mut asked = None;
        crate::fit::Dialog::new(crate::i18n::tr(TITLE)).open(&mut open).resizable(false).default_pos([80.0, 50.0]).show(ctx, |ui| {
            asked = editor.draw(ui);
        });
        if !open || asked == Some(Asked::Close) {
            editor.stop();
            return;
        }
        if let Some(asked) = asked
            && let Err(problem) = self.sound_effects_asked(&mut editor, asked)
        {
            self.message(TITLE, problem);
        }
        self.sound_effects = Some(editor);
    }

    fn sound_effects_asked(&mut self, editor: &mut Editor, asked: Asked) -> Result<(), String> {
        let write = |path: &Path, bytes: &[u8]| codec::write_file(path, bytes).map_err(|e| crate::i18n::tf!("Couldn't save {}.\n\n{e}", path.display()));
        let read = |path: &Path| std::fs::read(path).map_err(|e| crate::i18n::tf!("Couldn't read {}.\n\n{e}", path.display()));
        let name = crate::sfx::label_of(&editor.label);
        let ay = editor.chip() == crate::sfx::Chip::Ay;
        match asked {
            Asked::Open => {
                let mut dialog = crate::sys_dialogs::file_dialog().set_title(crate::i18n::tr("Open sound effects"));
                if ay {
                    dialog = dialog.add_filter(crate::i18n::tr("AYFX banks and effects"), &["afb", "afx"]);
                }
                if let Some(path) = dialog.add_filter(crate::i18n::tr("Source"), &["asm", "s", "a", "z80", "inc", "txt"]).add_filter(crate::i18n::tr("Any file"), &["*"]).pick_file() {
                    let bytes = read(&path)?;
                    let file = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
                    let lower = file.to_ascii_lowercase();
                    if lower.ends_with(".afb") || lower.ends_with(".afx") {
                        let effects = crate::sfx::read_ayfx_file(&bytes, &file).map_err(|e| crate::i18n::tf!("{} isn't an AYFX bank Limoniter can read.\n\n{e}", path.display()))?;
                        editor.remember();
                        let machine = if ay { editor.bank.machine } else { crate::sfx::Machine::Spectrum128 };
                        editor.bank = crate::sfx::Bank { machine, effects };
                        editor.chosen = 0;
                        editor.frame = 0;
                    } else {
                        editor.read_source(&String::from_utf8_lossy(&bytes)).map_err(|e| crate::i18n::tf!("{}: {e}", path.display()))?;
                    }
                }
            }
            Asked::FromSelection => {
                let selected = if self.no_file() { String::new() } else { self.docs[self.active].text.selection_text() };
                if selected.trim().is_empty() {
                    return Err(crate::i18n::tr("Select the effects' source in the document first: from their first line to their end label.").into());
                }
                editor.read_source(&selected)?;
            }
            Asked::Insert => self.insert_from_tool(TITLE, &editor.source()?),
            Asked::SaveSource => {
                let source = editor.source()?;
                if let Some(path) = crate::sys_dialogs::file_dialog().set_title(crate::i18n::tr("Save the source")).set_file_name(format!("{name}.asm")).add_filter(crate::i18n::tr("Source"), &["asm", "s", "a", "inc"]).save_file() {
                    write(&path, crate::i18n::comments(&source).as_bytes())?;
                }
            }
            Asked::SaveBank => {
                let bytes = editor.bank.bytes()?;
                let (title, extension, kind) = if ay { ("Save the AYFX bank", "afb", "AYFX bank") } else { ("Save the bytes", "bin", "Bytes") };
                if let Some(path) = crate::sys_dialogs::file_dialog().set_title(title).set_file_name(format!("{name}.{extension}")).add_filter(kind, &[extension]).save_file() {
                    write(&path, &bytes)?;
                }
            }
            Asked::OpenProgram => {
                let program = editor.program()?;
                if self.no_file() {
                    self.docs.clear();
                }
                let mut doc = Document::untitled(format!("{name}.asm"), &PRESETS[0]);
                doc.text.set_content(crate::i18n::comments(&program).lines().map(str::to_string).collect());
                highlight::load_now();
                let language = if editor.bank.machine.processor() == "Z80" { "Assembly Z80" } else { "Assembly 6502" };
                if let Some((_, syntax)) = highlight::engine().and_then(|e| e.names().iter().find(|(n, _)| n == language)) {
                    doc.set_language(Language::Named(*syntax));
                }
                doc.update_syntax();
                self.docs.push(doc);
                self.active = self.docs.len() - 1;
                self.pending_focus = true;
            }
            Asked::Close => {}
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::super::tests::app_with;
    use super::*;
    use crate::sfx::{Chip, Machine};

    fn app_language(doc: &Document) -> String {
        doc.language_name()
    }

    #[test]
    fn every_chip_s_window_draws_and_its_program_opens_to_run() {
        let (mut app, ctx) = app_with("");
        app.open_sound_effects();
        for chip in Chip::ALL {
            app.sound_effects.as_mut().unwrap().set_machine(chip.machines()[0]);
            for _ in 0..3 {
                ctx.run_ui(Default::default(), |ui| app.sound_effects_window(ui.ctx())).textures_delta.clear();
            }
            let mut editor = app.sound_effects.take().unwrap();
            app.sound_effects_asked(&mut editor, Asked::OpenProgram).unwrap();
            app.sound_effects = Some(editor);
            let doc = &app.docs[app.active];
            let text = (0..doc.text.line_count()).map(|i| doc.text.line(i).to_string()).collect::<Vec<_>>().join("\n");
            assert!(text.contains("Limoniter sound effects"), "{chip:?}");
            let machine = app.sound_effects.as_ref().unwrap().bank.machine;
            if machine == Machine::Spectrum128 || chip == Chip::Beeper {
                assert!(app_language(doc).contains("Z80"), "{chip:?}: {}", app_language(doc));
            }
        }
        // Put in the document and read back from a selection of it.
        app.new_document();
        let mut editor = app.sound_effects.take().unwrap();
        editor.set_machine(Machine::C64Pal);
        app.sound_effects_asked(&mut editor, Asked::Insert).unwrap();
        let before = editor.bank.clone();
        editor.bank.effects[0].frames.truncate(1);
        app.docs[app.active].text.select_all();
        app.sound_effects_asked(&mut editor, Asked::FromSelection).unwrap();
        assert_eq!(editor.bank.bytes().unwrap(), before.bytes().unwrap());
    }
}
