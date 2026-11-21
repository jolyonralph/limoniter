//! The window's side of the ripper: its window, the file it looks through, the finds it opens and saves, and the
//! editors it sends graphics to.

use super::*;
use crate::ripper_ui::{Asked, Ripper, Target};

const TITLE: &str = "Ripper";

impl Limoniter {
    pub(super) fn open_ripper(&mut self) {
        if self.ripper.is_none() {
            let mut ripper = Ripper::default();
            if let Some((name, bytes)) = self.front_file_bytes() {
                ripper.look_at(name, bytes);
            }
            self.ripper = Some(Box::new(ripper));
        }
    }

    /// The file in front, as its bytes on disk where it has a file, else as the document holds it.
    fn front_file_bytes(&self) -> Option<(String, Vec<u8>)> {
        if self.no_file() {
            return None;
        }
        let doc = &self.docs[self.active];
        if let Some(path) = &doc.path
            && let Ok(bytes) = std::fs::read(path)
        {
            return Some((doc.name(), bytes));
        }
        let bytes = if doc.hex_mode { doc.hex.read(0, doc.hex.len()) } else { doc.text.encode(&doc.text.format, crate::codec::Lossy::Omit).unwrap_or_default() };
        (!bytes.is_empty()).then(|| (doc.name(), bytes))
    }

    pub(super) fn ripper_window(&mut self, ctx: &Context) {
        let Some(mut ripper) = self.ripper.take() else {
            return;
        };
        let mut open = true;
        let mut asked = None;
        crate::fit::Dialog::new(crate::i18n::tr(TITLE)).open(&mut open).resizable(false).default_pos([100.0, 60.0]).show(ctx, |ui| {
            asked = ripper.draw(ui);
        });
        if !open || asked == Some(Asked::Close) {
            return;
        }
        if asked == Some(Asked::CopySource) {
            ctx.copy_text(ripper_source(&ripper));
        } else if let Some(asked) = asked
            && let Err(problem) = self.ripper_asked(&mut ripper, asked)
        {
            self.message(TITLE, problem);
        }
        self.ripper = Some(ripper);
    }

    fn ripper_asked(&mut self, ripper: &mut Ripper, asked: Asked) -> Result<(), String> {
        let write = |path: &Path, bytes: &[u8]| codec::write_file(path, bytes).map_err(|e| crate::i18n::tf!("Couldn't save {}.\n\n{e}", path.display()));
        match asked {
            Asked::UseFront => {
                let (name, bytes) = self.front_file_bytes().ok_or(crate::i18n::tr("There's no file in front to look through."))?;
                ripper.look_at(name, bytes);
            }
            Asked::OpenFile => {
                if let Some(path) = rfd::FileDialog::new().set_title(crate::i18n::tr("Look through a file")).pick_file() {
                    let bytes = std::fs::read(&path).map_err(|e| crate::i18n::tf!("Couldn't read {}.\n\n{e}", path.display()))?;
                    ripper.look_at(path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default(), bytes);
                }
            }
            Asked::OpenFind(i) => {
                let found = ripper.finds.get(i).ok_or(crate::i18n::tr("That find has gone."))?;
                let name = found.file_name();
                let mut doc = Document::from_image_file(format!("{} \u{203A} {name}", found.place), found.bytes.to_vec(), None);
                doc.preview_view = self.opening_view(Some(Path::new(&name)), matches!(Self::preview_kind(&doc), Some(preview::Kind::Module | preview::Kind::Sound)));
                if self.no_file() {
                    self.docs.clear();
                }
                self.docs.push(doc);
                self.active = self.docs.len() - 1;
            }
            Asked::SaveFind(i) => {
                let found = ripper.finds.get(i).ok_or(crate::i18n::tr("That find has gone."))?;
                if let Some(path) = rfd::FileDialog::new().set_title(crate::i18n::tr("Save the music")).set_file_name(found.file_name()).save_file() {
                    write(&path, &found.bytes)?;
                }
            }
            Asked::SaveAll => {
                if let Some(folder) = rfd::FileDialog::new().set_title(crate::i18n::tr("Save all the music into a folder")).pick_folder() {
                    let mut used = std::collections::HashSet::new();
                    for found in &ripper.finds {
                        let mut name = found.file_name();
                        let mut n = 2;
                        while !used.insert(name.to_ascii_lowercase()) || (folder.join(&name).exists() && n < 1000) {
                            name = format!("{} ({n}).{}", name.rsplit_once('.').map_or(name.as_str(), |p| p.0).trim_end_matches(&format!(" ({})", n - 1)), found.kind.extension());
                            n += 1;
                        }
                        write(&folder.join(&name), &found.bytes)?;
                    }
                    self.message(TITLE, crate::i18n::tf!("{} saved in {}.", ripper.finds.len(), folder.display()));
                }
            }
            Asked::SavePicture => {
                let picture = crate::ripper::picture(&ripper.bytes, &ripper.view);
                let image = egui::ColorImage::from_rgba_unmultiplied([picture.width, picture.height], &picture.rgba);
                if let Some(path) = rfd::FileDialog::new().set_title(crate::i18n::tr("Save as a picture")).set_file_name(format!("{}-{:06X}.png", crate::sfx::label_of(&ripper.name), ripper.view.offset)).add_filter(crate::i18n::tr("PNG"), &["png"]).save_file() {
                    save_png(&path, &image).map_err(|e| crate::i18n::tf!("Couldn't save {}.\n\n{e}", path.display()))?;
                }
            }
            Asked::CopySource | Asked::Close => {}
            Asked::Send(target) => {
                let bytes = ripper.view_bytes();
                use crate::pixel_editor::{Editor as PixelEditor, Kind, Sheet};
                let kind = match target {
                    Target::SpectrumScreen => {
                        let screen = crate::screen_designer::Screen::from_scr(&bytes).ok_or(crate::i18n::tr("A Spectrum screen needs 6912 bytes from where it starts."))?;
                        let mut designer = crate::screen_designer::Designer::default();
                        designer.screen = screen;
                        self.screen_designer = Some(Box::new(designer));
                        return Ok(());
                    }
                    Target::Characters => Kind::Characters,
                    Target::C64Characters => Kind::C64Characters,
                    Target::C64MultiCharacters => Kind::C64MultiCharacters,
                    Target::C64Sprites => Kind::C64Sprites,
                    Target::C64MultiSprites => Kind::C64MultiSprites,
                    Target::NextSprites => Kind::Sprites,
                };
                let mut editor = PixelEditor::new(kind);
                editor.sheet = Sheet::from_bytes(kind, &bytes);
                self.pixel_editor = Some(Box::new(editor));
            }
        }
        Ok(())
    }
}

/// The bytes shown, as source: ACME's !byte for the 6502 machines' pictures, DB for the rest.
fn ripper_source(ripper: &Ripper) -> String {
    use crate::ripper::Layout::*;
    let directive = if matches!(ripper.view.layout, C64Multi | C64MultiCharacters | C64Sprites | C64MultiSprites | Nes) { "!byte" } else { "db" };
    crate::ripper::as_source(&ripper.bytes, &ripper.view, directive)
}

#[cfg(test)]
mod tests {
    use super::super::tests::app_with;
    use super::*;

    #[test]
    fn a_module_inside_a_file_is_found_opened_and_its_graphics_sent_on() {
        let (mut app, ctx) = app_with("");
        app.open_ripper();
        let mut ripper = app.ripper.take().unwrap();
        // A small module in the middle of other bytes.
        let mut module = vec![0u8; 1084];
        module[..9].copy_from_slice(b"tiny tune");
        module[950] = 1;
        module[1080..1084].copy_from_slice(b"M.K.");
        module[20 + 22..20 + 24].copy_from_slice(&8u16.to_be_bytes());
        module[20 + 25] = 64;
        module.extend(vec![0u8; 1024]);
        module.extend(vec![0x40u8; 16]);
        let mut bytes = vec![0x55u8; 777];
        bytes.extend(&module);
        bytes.extend(vec![0xAAu8; 500]);
        ripper.look_at("game.bin".into(), bytes);
        ripper.wait();
        assert_eq!(ripper.finds.len(), 1);
        assert_eq!((ripper.finds[0].offset, ripper.finds[0].bytes.len(), ripper.finds[0].title.as_str()), (777, module.len(), "tiny tune"));
        app.ripper_asked(&mut ripper, Asked::OpenFind(0)).unwrap();
        assert!(app.docs[app.active].name().ends_with("tiny tune.mod"), "{}", app.docs[app.active].name());
        // Both tabs drawn.
        app.ripper = Some(ripper);
        for tab in [crate::ripper_ui::Tab::Music, crate::ripper_ui::Tab::Graphics] {
            app.ripper.as_mut().unwrap().tab = tab;
            for _ in 0..2 {
                ctx.run_ui(Default::default(), |ui| app.ripper_window(ui.ctx())).textures_delta.clear();
            }
        }
        // Characters sent to the character editor, as many as are shown.
        let mut ripper = app.ripper.take().unwrap();
        ripper.view = crate::ripper::View { layout: crate::ripper::Layout::Characters, offset: 0, across: 4, down: 2, planes: 1 };
        app.ripper_asked(&mut ripper, Asked::Send(Target::Characters)).unwrap();
        let editor = app.pixel_editor.as_ref().unwrap();
        assert_eq!(editor.sheet.items.len(), 8);
        assert!(editor.sheet.items[0].iter().all(|&p| p == 0) || editor.sheet.items[0].contains(&1));
    }
}
