//! The window's side of the Atari ST picture converter: its window, kept open beside the text, and the files it
//! opens and saves.

use super::*;
use crate::st_converter::{Asked, Converter, PaletteKind, RawColours};
use crate::st_planar::{self as st, Screen};
use crate::st_sprites as sprites;

const TITLE: &str = "Atari ST picture converter";

/// The files a converted picture is saved as, by name: the raw screen, the palette as words, assembler and C, the
/// pictures this screen can be (Degas, NEOchrome, Spectrum, IFF) and the source.
pub(super) fn everything(c: &st::Converted, name: &str, packed: bool) -> Vec<(String, Vec<u8>)> {
    let raw_name = format!("{name}.raw");
    let mut files = Vec::new();
    if c.screen == Screen::Spectrum {
        let spu = format!("{name}.spu");
        files.push((spu.clone(), crate::st_spectrum::to_spu(c)));
        files.push((format!("{name}.spc"), crate::st_spectrum::to_spc(c)));
        files.push((format!("{name}.s"), st::asm_source(c, name, &spu).into_bytes()));
        return files;
    }
    files.push((raw_name.clone(), st::screen_bytes(c)));
    if c.screen != Screen::FalconTrue {
        files.push((format!("{name}.pal"), st::palette_raw(c)));
        files.push((format!("{name}_palette.s"), st::palette_asm(c, name).into_bytes()));
        files.push((format!("{name}_palette.h"), st::palette_c(c, name).into_bytes()));
    }
    if let (Some(resolution), Some(degas)) = (c.screen.st_resolution(), st::to_degas(c, packed)) {
        files.push((format!("{name}.{}{}", if packed { "pc" } else { "pi" }, resolution + 1), degas));
    }
    if let Some(neo) = st::to_neo(c) {
        files.push((format!("{name}.neo"), neo));
    }
    if let Some(ilbm) = st::to_ilbm(c, packed) {
        files.push((format!("{name}.iff"), ilbm));
    }
    files.push((format!("{name}.s"), st::asm_source(c, name, &raw_name).into_bytes()));
    files
}

impl Limoniter {
    /// Opens the converter, with the picture in front brought in if it is one.
    pub(super) fn open_st_converter(&mut self) {
        if self.st_pictures.is_some() {
            return;
        }
        let mut converter = Converter::default();
        if !self.no_file() {
            let name = self.docs[self.active].path_or_name().file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
            let bytes = self.document_bytes();
            if let Ok(image) = crate::quantise::read_picture(&name, &bytes) {
                converter.set_picture(&name, image);
            }
        }
        self.st_pictures = Some(Box::new(converter));
    }

    pub(super) fn st_converter_window(&mut self, ctx: &Context) {
        let Some(mut converter) = self.st_pictures.take() else {
            return;
        };
        let mut open = true;
        let mut asked = None;
        crate::fit::Dialog::new(crate::i18n::tr(TITLE)).open(&mut open).resizable(true).default_pos([160.0, 80.0]).default_width(900.0).show(ctx, |ui| {
            asked = converter.draw(ui);
        });
        if !open {
            return;
        }
        if let Some(asked) = asked
            && let Err(problem) = self.st_converter_asked(&mut converter, asked)
        {
            self.message(TITLE, problem);
        }
        self.st_pictures = Some(converter);
    }

    fn st_converter_asked(&mut self, c: &mut Converter, asked: Asked) -> Result<(), String> {
        let write = |path: &Path, bytes: &[u8]| codec::write_file(path, bytes).map_err(|e| crate::i18n::tf!("Couldn't save {}.\n\n{e}", path.display()));
        let read = |path: &Path| std::fs::read(path).map_err(|e| crate::i18n::tf!("Couldn't read {}.\n\n{e}", path.display()));
        let file_name = |p: &Path| p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        let name = c.name.clone();
        let save = |title: &str, file: String, filter: Option<(&str, &[&str])>| {
            let mut dialog = rfd::FileDialog::new().set_title(title).set_file_name(file);
            if let Some((label, endings)) = filter {
                dialog = dialog.add_filter(label, endings);
            }
            dialog.save_file()
        };
        match asked {
            Asked::OpenPicture => {
                if let Some(path) = rfd::FileDialog::new().set_title(crate::i18n::tr("Open a picture to convert")).add_filter(crate::i18n::tr("Pictures"), &["png", "jpg", "jpeg", "iff", "ilbm", "lbm", "pcx", "neo", "pi1", "pi2", "pi3", "pc1", "pc2", "pc3", "spu", "spc"]).add_filter(crate::i18n::tr("Any file"), &["*"]).pick_file() {
                    let image = crate::quantise::read_picture(&file_name(&path), &read(&path)?).map_err(|e| crate::i18n::tf!("Couldn't open {}.\n\n{e}", path.display()))?;
                    c.set_picture(&file_name(&path), image);
                }
            }
            Asked::OpenPalette => {
                if let Some(path) = rfd::FileDialog::new().set_title(crate::i18n::tr("Open a palette")).add_filter(crate::i18n::tr("Palettes"), &["pal", "pi1", "pi2", "pc1", "neo", "s", "asm", "i", "c", "h", "txt", "iff", "gpl", "raw", "bin"]).add_filter(crate::i18n::tr("Any file"), &["*"]).pick_file() {
                    let colours = st::read_palette(&read(&path)?).ok_or_else(|| crate::i18n::tf!("No palette was found in {}.", path.display()))?;
                    c.settings.keep_given = colours.len().min(1 << c.settings.settled().screen.planes().min(8));
                    c.settings.given = colours;
                    c.palette_name = Some(file_name(&path));
                    c.raw_colours = RawColours::Given;
                    c.raw_changed();
                }
            }
            Asked::OpenRaw => {
                if let Some(path) = rfd::FileDialog::new().set_title(crate::i18n::tr("Open raw ST planes")).add_filter(crate::i18n::tr("Any file"), &["*"]).pick_file() {
                    let bytes = read(&path)?;
                    let rows = c.raw.rows_in(bytes.len());
                    if rows > 0 {
                        c.raw.height = rows.min(1024);
                    }
                    c.raw_file = Some((file_name(&path), bytes));
                    c.raw_changed();
                }
            }
            Asked::SavePng => {
                if let Some(picture) = c.raw_picture()
                    && let Some(path) = save("Save as PNG", "planes.png".into(), Some(("PNG picture", &["png"])))
                {
                    write(&path, &crate::pictures::to_png(&picture)?)?;
                }
            }
            Asked::SaveSprite => {
                let Some(s) = &c.sprite else { return Ok(()) };
                if let Some(path) = save("Save the sprite's source", format!("{name}_sprite.s"), Some(("Assembler source", &["s", "asm", "i"]))) {
                    let raw = path.with_extension("spr");
                    write(&raw, &s.data())?;
                    let raw_name = file_name(&raw);
                    write(&path, sprites::sprite_source(s, &name, c.settings.machine.st_levels(), (!c.inline).then_some(raw_name.as_str())).as_bytes())?;
                }
            }
            Asked::SaveFont => {
                let Some(f) = &c.font else { return Ok(()) };
                if let Some(path) = save("Save the font's source", format!("{name}_font.s"), Some(("Assembler source", &["s", "asm", "i"]))) {
                    let raw = path.with_extension("fnt");
                    write(&raw, &f.data())?;
                    let raw_name = file_name(&raw);
                    write(&path, sprites::font_source(f, &name, (!c.inline).then_some(raw_name.as_str())).as_bytes())?;
                }
            }
            _ => {
                let Some((_, converted)) = &c.result else { return Ok(()) };
                let all = everything(converted, &name, c.packed);
                let pick = |ending: &str| all.iter().find(|(n, _)| n.ends_with(ending)).cloned();
                let save_one = |title: &str, item: Option<(String, Vec<u8>)>, filter: Option<(&str, &[&str])>| -> Result<(), String> {
                    if let Some((file, bytes)) = item
                        && let Some(path) = save(title, file, filter)
                    {
                        write(&path, &bytes)?;
                    }
                    Ok(())
                };
                match asked {
                    Asked::SaveScreen => save_one("Save the screen", Some((format!("{name}.raw"), st::screen_bytes(converted))), Some(("Raw screen", &["raw", "bin", "scr"])))?,
                    Asked::SavePalette(kind) => {
                        let item = match kind {
                            PaletteKind::Raw => (format!("{name}.pal"), st::palette_raw(converted)),
                            PaletteKind::Asm => (format!("{name}_palette.s"), st::palette_asm(converted, &name).into_bytes()),
                            PaletteKind::C => (format!("{name}_palette.h"), st::palette_c(converted, &name).into_bytes()),
                        };
                        save_one("Save the palette", Some(item), None)?;
                    }
                    Asked::SaveDegas => save_one("Save as Degas", all.iter().find(|(n, _)| n.contains(".pi") || n.contains(".pc")).cloned(), Some(("Degas picture", &["pi1", "pi2", "pi3", "pc1", "pc2", "pc3"])))?,
                    Asked::SaveNeo => save_one("Save as NEOchrome", pick(".neo"), Some(("NEOchrome picture", &["neo"])))?,
                    Asked::SaveIlbm => save_one("Save as IFF ILBM", pick(".iff"), Some(("IFF ILBM picture", &["iff", "ilbm", "lbm"])))?,
                    Asked::SaveSpectrum => save_one("Save as Spectrum", pick(if c.packed { ".spc" } else { ".spu" }), Some(("Spectrum 512 picture", &["spu", "spc"])))?,
                    Asked::SaveSource => {
                        if let Some(path) = save("Save the source", format!("{name}.s"), Some(("Assembler source", &["s", "asm", "i"]))) {
                            // The screen it brings in is saved beside it.
                            let (screen_file, screen) = if converted.screen == Screen::Spectrum { (path.with_extension("spu"), crate::st_spectrum::to_spu(converted)) } else { (path.with_extension("raw"), st::screen_bytes(converted)) };
                            write(&screen_file, &screen)?;
                            write(&path, st::asm_source(converted, &name, &file_name(&screen_file)).as_bytes())?;
                        }
                    }
                    Asked::SaveAll => {
                        if let Some(folder) = rfd::FileDialog::new().set_title(crate::i18n::tr("Save everything into a folder")).pick_folder() {
                            for (file, bytes) in &all {
                                write(&folder.join(file), bytes)?;
                            }
                        }
                    }
                    _ => {}
                }
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::super::tests::app_with;
    use super::*;
    use crate::st_converter::Tab;

    #[test]
    fn the_converter_draws_every_tab_and_converts() {
        let (mut app, ctx) = app_with("");
        let mut converter = Converter::default();
        let rgba: Vec<u8> = (0..64 * 48).flat_map(|i| [(i % 64 * 4) as u8, (i / 64 * 5) as u8, 128, if i % 7 == 0 { 0 } else { 255 }]).collect();
        converter.set_picture("test.png", crate::quantise::Image { width: 64, height: 48, rgba });
        converter.settings.width = 64;
        converter.settings.height = 48;
        converter.raw_file = Some(("planes.raw".into(), vec![0x5A; 64 * 48]));
        app.st_pictures = Some(Box::new(converter));
        for tab in [Tab::Picture, Tab::Sprite, Tab::Font, Tab::Raw] {
            app.st_pictures.as_mut().unwrap().tab = tab;
            for _ in 0..3 {
                ctx.run_ui(Default::default(), |ui| app.st_converter_window(ui.ctx())).textures_delta.clear();
            }
        }
        let started = std::time::Instant::now();
        while app.st_pictures.as_ref().unwrap().result.is_none() && started.elapsed().as_secs() < 30 {
            std::thread::sleep(std::time::Duration::from_millis(20));
            ctx.run_ui(Default::default(), |ui| app.st_converter_window(ui.ctx())).textures_delta.clear();
        }
        let converter = app.st_pictures.as_ref().unwrap();
        let converted = &converter.result.as_ref().unwrap().1;
        assert_eq!(converted.width, 64);
        assert!(converter.sprite.is_some() && converter.font.is_some() && converter.raw_shown.is_some());
        // Not the whole screen: no Degas or NEOchrome, but the raw planes, palette, IFF and source.
        let names: Vec<String> = everything(converted, "test", false).into_iter().map(|f| f.0).collect();
        assert_eq!(names, ["test.raw", "test.pal", "test_palette.s", "test_palette.h", "test.iff", "test.s"]);
    }
}
