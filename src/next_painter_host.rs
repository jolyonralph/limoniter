//! The window's side of the Spectrum Next's Layer 2 painter: its window, kept open beside the text, and the files
//! it opens and saves.

use super::*;
use crate::next_painter::{Asked, Painter};
use crate::next_screen::{NextMode, NextScreen};

const TITLE: &str = "Spectrum Next Layer 2 painter";

impl Limoniter {
    /// Opens the painter, with the Next screen in front in it if it is one.
    pub(super) fn open_next_painter(&mut self) {
        if self.next_painter.is_some() {
            return;
        }
        let mut painter = Painter::default();
        if !self.no_file() {
            let path = self.docs[self.active].path_or_name();
            let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
            if let Ok(screen) = NextScreen::read(&name, &self.document_bytes()) {
                let stem = path.file_stem().map_or_else(|| "picture".to_string(), |s| s.to_string_lossy().into_owned());
                painter = Painter::new(screen, &crate::quantise::identifier(&stem.to_lowercase()));
            }
        }
        self.next_painter = Some(Box::new(painter));
    }

    pub(super) fn next_painter_window(&mut self, ctx: &Context) {
        let Some(mut painter) = self.next_painter.take() else {
            return;
        };
        let mut open = true;
        let mut asked = None;
        crate::fit::Dialog::new(crate::i18n::tr(TITLE)).open(&mut open).resizable(false).default_pos([150.0, 70.0]).show(ctx, |ui| {
            asked = painter.draw(ui);
        });
        if !open || asked == Some(Asked::Close) {
            return;
        }
        if asked == Some(Asked::BringIn) {
            self.next_painter = Some(painter);
            self.bring_picture_to_next_painter();
            return;
        }
        if let Some(asked) = asked
            && let Err(problem) = next_painter_asked(&mut painter, asked)
        {
            self.message(TITLE, problem);
        }
        self.next_painter = Some(painter);
    }

    /// A picture chosen and opened in the picture converter, in the painter's mode, to come back to it.
    fn bring_picture_to_next_painter(&mut self) {
        let Some(path) = rfd::FileDialog::new().set_title(crate::i18n::tr("Bring in a picture")).add_filter(crate::i18n::tr("Pictures"), retro_converter_host::PICTURES).add_filter(crate::i18n::tr("Any file"), &["*"]).pick_file() else { return };
        let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        match std::fs::read(&path).map_err(|e| e.to_string()).and_then(|bytes| crate::quantise::read_picture(&name, &bytes)) {
            Ok(image) => {
                let mode = self.next_painter.as_ref().map_or(NextMode::Layer2, |p| p.screen.mode);
                let converter = self.retro_pictures.get_or_insert_with(Box::default);
                converter.settings.mode = match mode {
                    NextMode::Layer2 => crate::retro_convert::Mode::Layer2,
                    NextMode::Layer2Wide => crate::retro_convert::Mode::Layer2Wide,
                    NextMode::Layer2Hi => crate::retro_convert::Mode::Layer2Hi,
                    NextMode::LoRes => crate::retro_convert::Mode::LoRes,
                };
                converter.set_picture(&name, image);
            }
            Err(problem) => self.message(TITLE, crate::i18n::tf!("Couldn't open {}.\n\n{problem}", path.display())),
        }
    }
}

fn next_painter_asked(p: &mut Painter, asked: Asked) -> Result<(), String> {
    let write = |path: &Path, bytes: &[u8]| codec::write_file(path, bytes).map_err(|e| crate::i18n::tf!("Couldn't save {}.\n\n{e}", path.display()));
    let read = |path: &Path| std::fs::read(path).map_err(|e| crate::i18n::tf!("Couldn't read {}.\n\n{e}", path.display()));
    let stem = |path: &Path| path.file_stem().map_or_else(|| "picture".to_string(), |s| crate::quantise::identifier(&s.to_string_lossy().to_lowercase()));
    let save = |title: &str, file: String, filter: (&str, &[&str])| rfd::FileDialog::new().set_title(title).set_file_name(file).add_filter(filter.0, filter.1).save_file();
    let name = p.name.clone();
    let ending = p.screen.mode.ending();
    match asked {
        Asked::Open => {
            if let Some(path) = rfd::FileDialog::new().set_title(crate::i18n::tr("Open a Next screen")).add_filter(crate::i18n::tr("Next screens"), &["nxi", "sl2", "slr", "bmp"]).add_filter(crate::i18n::tr("Any file"), &["*"]).pick_file() {
                let file = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
                let screen = NextScreen::read(&file, &read(&path)?).map_err(|e| crate::i18n::tf!("Couldn't open {}.\n\n{e}", path.display()))?;
                p.remember();
                p.screen = screen;
                p.name = stem(&path);
                p.selection = None;
            }
        }
        Asked::OpenPalette => {
            if let Some(path) = rfd::FileDialog::new().set_title(crate::i18n::tr("Open a palette")).add_filter(crate::i18n::tr("Next palettes"), &["nxp", "pal"]).add_filter(crate::i18n::tr("Any file"), &["*"]).pick_file() {
                let bytes = read(&path)?;
                if bytes.len() < 256 {
                    return Err(crate::i18n::tf!("{} is {} bytes: a Next palette is 512 (or 256, without the ninth bits).", path.display(), bytes.len()));
                }
                p.remember();
                p.screen.palette = crate::next_screen::palette_from_bytes(&bytes);
            }
        }
        Asked::SaveNxi => {
            if let Some(path) = save("Save the screen", format!("{name}.nxi"), ("Next picture", &["nxi"])) {
                write(&path, &p.screen.nxi())?;
                p.name = stem(&path);
            }
        }
        Asked::SavePixels => {
            if let Some(path) = save("Save the pixels (the palette beside them)", format!("{name}.{ending}"), ("Next screen", &[ending])) {
                write(&path, &p.screen.memory())?;
                write(&path.with_extension("nxp"), &p.screen.palette_bytes())?;
                p.name = stem(&path);
            }
        }
        Asked::SavePalette => {
            if let Some(path) = save("Save the palette", format!("{name}.nxp"), ("Next palette", &["nxp"])) {
                write(&path, &p.screen.palette_bytes())?;
            }
        }
        Asked::SaveBmp => {
            if let Some(path) = save("Save as BMP", format!("{name}.bmp"), ("BMP picture", &["bmp"])) {
                write(&path, &p.screen.bmp())?;
            }
        }
        Asked::SavePng => {
            if let Some((_, png)) = p.screen.everything(&name).into_iter().find(|(n, _)| n.ends_with(".png"))
                && let Some(path) = save("Save as PNG", format!("{name}.png"), ("PNG picture", &["png"]))
            {
                write(&path, &png)?;
            }
        }
        Asked::SaveNex => {
            if let Some(path) = save("Save a Next program that shows it", format!("{name}.nex"), ("Next program", &["nex"])) {
                write(&path, &p.screen.nex(&stem(&path))?)?;
            }
        }
        Asked::SaveAll => {
            if let Some(folder) = rfd::FileDialog::new().set_title(crate::i18n::tr("Save everything into a folder")).pick_folder() {
                for (file, bytes) in p.screen.everything(&name) {
                    write(&folder.join(file), &bytes)?;
                }
            }
        }
        Asked::BringIn | Asked::Close => {}
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::super::tests::app_with;
    use super::*;

    #[test]
    fn the_painter_draws_in_every_mode() {
        let (mut app, ctx) = app_with("");
        for mode in NextMode::ALL {
            app.next_painter = Some(Box::new(Painter::new(NextScreen::new(mode), "test")));
            for _ in 0..3 {
                ctx.run_ui(Default::default(), |ui| app.next_painter_window(ui.ctx())).textures_delta.clear();
            }
            assert_eq!(app.next_painter.as_ref().unwrap().screen.mode, mode);
        }
    }
}
