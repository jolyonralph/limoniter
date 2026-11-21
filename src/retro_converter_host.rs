//! The window's side of the 8-bit picture converter: its window, kept open beside the text, the files it opens and
//! saves, and a Spectrum screen handed to the screen designer.

use super::*;
use crate::retro_converter::{Asked, Converter};

const TITLE: &str = "Picture converter: Spectrum, Next, CPC, MSX and BBC";

/// What the open dialogs offer as pictures.
pub(super) const PICTURES: &[&str] = &["png", "jpg", "jpeg", "bmp", "iff", "ilbm", "lbm", "pcx", "gif", "koa", "kla", "art", "ocp", "fli", "afl", "neo", "pi1", "pc1", "scr", "sl2", "nxi", "hgr"];

impl Limoniter {
    /// Opens the converter, with the picture in front brought in if it is one.
    pub(super) fn open_retro_converter(&mut self) {
        if self.retro_pictures.is_some() {
            return;
        }
        let mut converter = Converter::default();
        if !self.no_file() {
            let name = self.docs[self.active].path_or_name().file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
            if let Ok(image) = crate::quantise::read_picture(&name, &self.document_bytes()) {
                converter.set_picture(&name, image);
            }
        }
        self.retro_pictures = Some(Box::new(converter));
    }

    /// A picture chosen and opened in the converter as a Spectrum screen, for the screen designer.
    pub(super) fn bring_picture_to_designer(&mut self) {
        let Some(path) = rfd::FileDialog::new().set_title(crate::i18n::tr("Bring in a picture")).add_filter(crate::i18n::tr("Pictures"), PICTURES).add_filter(crate::i18n::tr("Any file"), &["*"]).pick_file() else { return };
        let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        match std::fs::read(&path).map_err(|e| e.to_string()).and_then(|bytes| crate::quantise::read_picture(&name, &bytes)) {
            Ok(image) => {
                let converter = self.retro_pictures.get_or_insert_with(Box::default);
                converter.settings.mode = crate::retro_convert::Mode::Spectrum;
                converter.set_picture(&name, image);
            }
            Err(problem) => self.message("Spectrum screen", crate::i18n::tf!("Couldn't open {}.\n\n{problem}", path.display())),
        }
    }

    pub(super) fn retro_converter_window(&mut self, ctx: &Context) {
        let Some(mut converter) = self.retro_pictures.take() else {
            return;
        };
        let mut open = true;
        let mut asked = None;
        crate::fit::Dialog::new(crate::i18n::tr(TITLE)).open(&mut open).resizable(true).default_pos([180.0, 90.0]).default_width(900.0).show(ctx, |ui| {
            asked = converter.draw(ui);
        });
        if !open {
            return;
        }
        if let Some(asked) = asked
            && let Err(problem) = self.retro_converter_asked(&mut converter, asked)
        {
            self.message(TITLE, problem);
        }
        self.retro_pictures = Some(converter);
    }

    fn retro_converter_asked(&mut self, c: &mut Converter, asked: Asked) -> Result<(), String> {
        let write = |path: &Path, bytes: &[u8]| codec::write_file(path, bytes).map_err(|e| crate::i18n::tf!("Couldn't save {}.\n\n{e}", path.display()));
        if asked == Asked::OpenPicture {
            if let Some(path) = rfd::FileDialog::new().set_title(crate::i18n::tr("Open a picture to convert")).add_filter(crate::i18n::tr("Pictures"), PICTURES).add_filter(crate::i18n::tr("Any file"), &["*"]).pick_file() {
                let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
                let bytes = std::fs::read(&path).map_err(|e| crate::i18n::tf!("Couldn't read {}.\n\n{e}", path.display()))?;
                let image = crate::quantise::read_picture(&name, &bytes).map_err(|e| crate::i18n::tf!("Couldn't open {}.\n\n{e}", path.display()))?;
                c.set_picture(&name, image);
            }
            return Ok(());
        }
        let Some(screen) = c.screen().filter(|_| c.up_to_date()) else {
            return Ok(());
        };
        if asked == Asked::ToDesigner {
            if let Some(made) = screen.spectrum_screen() {
                let designer = self.screen_designer.get_or_insert_with(Box::default);
                designer.remember();
                designer.screen = made;
            }
            if let Some(sprites) = screen.sprites.as_ref().filter(|sp| !sp.four_bit && !sp.own_palette) {
                let kind = crate::pixel_editor::Kind::Sprites;
                let mut editor = crate::pixel_editor::Editor::new(kind);
                editor.sheet = crate::pixel_editor::Sheet::from_bytes(kind, &sprites.pattern_bytes());
                editor.current = 0;
                self.pixel_editor = Some(Box::new(editor));
            }
            if let Some(made) = screen.next_screen() {
                let painter = self.next_painter.get_or_insert_with(Box::default);
                painter.remember();
                painter.screen = made;
                painter.name = c.name.clone();
            }
            return Ok(());
        }
        let all = screen.everything(&c.name);
        match asked {
            Asked::SaveAll => {
                if let Some(folder) = rfd::FileDialog::new().set_title(crate::i18n::tr("Save everything into a folder")).pick_folder() {
                    for (file, bytes) in &all {
                        write(&folder.join(file), bytes)?;
                    }
                }
            }
            Asked::SaveScreen | Asked::SavePng | Asked::SaveNex => {
                let item = match asked {
                    Asked::SavePng => all.iter().find(|(n, _)| n.ends_with(".png")),
                    Asked::SaveNex => all.iter().find(|(n, _)| n.ends_with(".nex")),
                    _ => all.first(),
                };
                if let Some((file, bytes)) = item {
                    let ending = file.rsplit_once('.').map_or("", |(_, e)| e).to_string();
                    let mut dialog = rfd::FileDialog::new().set_title(crate::i18n::tr("Save the screen")).set_file_name(file);
                    if !ending.is_empty() {
                        dialog = dialog.add_filter(ending.to_uppercase(), &[ending.as_str()]);
                    }
                    if let Some(path) = dialog.add_filter(crate::i18n::tr("Any file"), &["*"]).save_file() {
                        write(&path, bytes)?;
                    }
                }
            }
            _ => {}
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::super::tests::app_with;
    use super::*;

    #[test]
    fn the_converter_draws_every_machine_and_hands_the_spectrum_to_the_designer() {
        let (mut app, ctx) = app_with("");
        let mut converter = Converter::default();
        let rgba: Vec<u8> = (0..64 * 48).flat_map(|i| [(i % 64 * 4) as u8, (i / 64 * 5) as u8, 128, 255]).collect();
        converter.set_picture("test.jpg", crate::quantise::Image { width: 64, height: 48, rgba });
        app.retro_pictures = Some(Box::new(converter));
        for machine in crate::retro_convert::Machine::ALL {
            app.retro_pictures.as_mut().unwrap().settings.mode = machine.modes()[0];
            let started = std::time::Instant::now();
            loop {
                ctx.run_ui(Default::default(), |ui| app.retro_converter_window(ui.ctx())).textures_delta.clear();
                if app.retro_pictures.as_ref().unwrap().up_to_date() || started.elapsed().as_secs() > 60 {
                    break;
                }
                std::thread::sleep(std::time::Duration::from_millis(20));
            }
            ctx.run_ui(Default::default(), |ui| app.retro_converter_window(ui.ctx())).textures_delta.clear();
            let c = app.retro_pictures.as_ref().unwrap();
            assert_eq!(c.screen().unwrap().mode.machine(), machine);
        }
        let mut c = app.retro_pictures.take().unwrap();
        c.settings.mode = crate::retro_convert::Mode::Spectrum;
        let started = std::time::Instant::now();
        while !c.up_to_date() && started.elapsed().as_secs() < 60 {
            let _ = ctx.run_ui(Default::default(), |ui| {
                c.draw(ui);
            });
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        app.retro_converter_asked(&mut c, Asked::ToDesigner).unwrap();
        let designer = app.screen_designer.as_ref().expect("the designer opened");
        assert_eq!(Some(&designer.screen), c.screen().and_then(|s| s.spectrum_screen()).as_ref());
        // And the designer draws.
        ctx.run_ui(Default::default(), |ui| app.screen_designer_window(ui.ctx())).textures_delta.clear();
    }

    #[test]
    fn sprites_cut_in_the_nexts_palette_go_into_the_sprite_editor() {
        let (mut app, ctx) = app_with("");
        let mut c = Converter::default();
        let rgba: Vec<u8> = (0..32 * 16).flat_map(|i| if i % 32 < 16 { [255, 0, 0, 255] } else { [0, 0, 255, 255] }).collect();
        c.set_picture("sheet.png", crate::quantise::Image { width: 32, height: 16, rgba });
        c.settings.mode = crate::retro_convert::Mode::Sprites;
        c.settings.sprites.across = 2;
        c.settings.sprites.down = 1;
        let started = std::time::Instant::now();
        while !c.up_to_date() && started.elapsed().as_secs() < 60 {
            let _ = ctx.run_ui(Default::default(), |ui| {
                c.draw(ui);
            });
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        app.retro_converter_asked(&mut c, Asked::ToDesigner).unwrap();
        let editor = app.pixel_editor.as_ref().expect("the sprite editor opened");
        assert_eq!(editor.sheet.kind, crate::pixel_editor::Kind::Sprites);
        assert_eq!(editor.sheet.items.len(), 2);
        assert_eq!((editor.sheet.items[0][0], editor.sheet.items[1][0]), (0xE0, 0x03), "red, then blue, in the Next's own palette");
    }
}
