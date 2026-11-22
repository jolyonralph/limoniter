//! The window's side of the Amiga bitplane converter: its window, kept open beside the text, and the files it
//! opens and saves.

use super::*;
use crate::amiga_planar as planar;
use crate::amiga_sprites as sprites;
use crate::bitplane_tool::{Asked, Converter, PaletteKind};

const TITLE: &str = "Amiga bitplane converter";

impl Limoniter {
    /// Opens the converter, with the picture in front brought in if it is one.
    pub(super) fn open_bitplane_converter(&mut self) {
        if self.bitplanes.is_some() {
            return;
        }
        let mut converter = Converter::default();
        if !self.no_file() {
            let name = self.docs[self.active].path_or_name().file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
            let bytes = self.document_bytes();
            if let Ok(image) = planar::read_picture(&name, &bytes) {
                converter.set_picture(&name, image);
            }
        }
        self.bitplanes = Some(Box::new(converter));
    }

    pub(super) fn bitplane_window(&mut self, ctx: &Context) {
        let Some(mut converter) = self.bitplanes.take() else {
            return;
        };
        let mut open = true;
        let mut asked = None;
        crate::fit::Dialog::new(crate::i18n::tr(TITLE)).open(&mut open).resizable(true).default_pos([140.0, 70.0]).default_width(900.0).show(ctx, |ui| {
            asked = converter.draw(ui);
        });
        if !open {
            return;
        }
        if let Some(asked) = asked
            && let Err(problem) = self.bitplane_asked(&mut converter, asked)
        {
            self.message(TITLE, problem);
        }
        self.bitplanes = Some(converter);
    }

    fn bitplane_asked(&mut self, c: &mut Converter, asked: Asked) -> Result<(), String> {
        let write = |path: &Path, bytes: &[u8]| codec::write_file(path, bytes).map_err(|e| crate::i18n::tf!("Couldn't save {}.\n\n{e}", path.display()));
        let read = |path: &Path| std::fs::read(path).map_err(|e| crate::i18n::tf!("Couldn't read {}.\n\n{e}", path.display()));
        let name = c.name.clone();
        match asked {
            Asked::OpenPicture => {
                if let Some(path) = crate::sys_dialogs::file_dialog().set_title(crate::i18n::tr("Open a picture to convert")).add_filter(crate::i18n::tr("Pictures"), &["png", "jpg", "jpeg", "iff", "ilbm", "lbm", "pcx", "neo", "pi1", "scr"]).add_filter(crate::i18n::tr("Any file"), &["*"]).pick_file() {
                    let file = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
                    let image = planar::read_picture(&file, &read(&path)?).map_err(|e| crate::i18n::tf!("Couldn't open {}.\n\n{e}", path.display()))?;
                    c.set_picture(&file, image);
                }
            }
            Asked::OpenPalette => {
                if let Some(path) = crate::sys_dialogs::file_dialog().set_title(crate::i18n::tr("Open a palette")).add_filter(crate::i18n::tr("Palettes"), &["pal", "iff", "ilbm", "gpl", "s", "asm", "i", "c", "h", "txt", "raw", "bin"]).add_filter(crate::i18n::tr("Any file"), &["*"]).pick_file() {
                    let colours = planar::read_palette(&read(&path)?).ok_or_else(|| crate::i18n::tf!("No palette was found in {}.", path.display()))?;
                    c.settings.keep_given = colours.len().min(c.settings.mode.registers(c.settings.settled().planes));
                    c.settings.given = colours;
                    c.palette_name = path.file_name().map(|n| n.to_string_lossy().into_owned());
                    c.raw_colours = crate::bitplane_tool::RawColours::Given;
                    c.raw_changed();
                }
            }
            Asked::OpenRaw => {
                if let Some(path) = crate::sys_dialogs::file_dialog().set_title(crate::i18n::tr("Open raw planes")).add_filter(crate::i18n::tr("Any file"), &["*"]).pick_file() {
                    let bytes = read(&path)?;
                    let file = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
                    let rows = c.raw.rows_in(bytes.len());
                    if rows > 0 {
                        c.raw.height = rows.min(1024);
                    }
                    c.raw_file = Some((file, bytes));
                    c.raw_changed();
                }
            }
            Asked::SavePng => {
                if let Some(picture) = c.raw_picture()
                    && let Some(path) = crate::sys_dialogs::file_dialog().set_title(crate::i18n::tr("Save as PNG")).set_file_name("planes.png").add_filter(crate::i18n::tr("PNG picture"), &["png"]).save_file()
                {
                    write(&path, &crate::pictures::to_png(&picture)?)?;
                }
            }
            Asked::SaveSprites => {
                let Some(s) = &c.sprites else { return Ok(()) };
                if let Some(path) = crate::sys_dialogs::file_dialog().set_title(crate::i18n::tr("Save the sprites' source")).set_file_name(format!("{name}_sprites.s")).add_filter(crate::i18n::tr("Assembler source"), &["s", "asm", "i"]).save_file() {
                    let raw = path.with_extension("spr");
                    let raw_name = raw.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
                    write(&raw, &sprites::sprites_raw(s))?;
                    write(&path, sprites::sprites_source(s, &name, (!c.inline).then_some(raw_name.as_str())).as_bytes())?;
                }
            }
            Asked::SaveBob => {
                let Some(b) = &c.bob else { return Ok(()) };
                if let Some(path) = crate::sys_dialogs::file_dialog().set_title(crate::i18n::tr("Save the bob's source")).set_file_name(format!("{name}_bob.s")).add_filter(crate::i18n::tr("Assembler source"), &["s", "asm", "i"]).save_file() {
                    let (planes, mask) = (path.with_extension("bpl"), path.with_extension("msk"));
                    let file = |p: &Path| p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
                    write(&planes, &b.planes())?;
                    write(&mask, &b.mask())?;
                    let (planes_name, mask_name) = (file(&planes), file(&mask));
                    write(&path, sprites::bob_source(b, &name, (!c.inline).then_some((planes_name.as_str(), mask_name.as_str()))).as_bytes())?;
                }
            }
            _ => {
                let Some((_, converted)) = &c.result else { return Ok(()) };
                let planes = || planar::to_planes(&converted.values, converted.width, converted.height, converted.planes, c.interleaved, c.fetch);
                let raw_name = format!("{name}.raw");
                match asked {
                    Asked::SavePlanes => {
                        if let Some(path) = crate::sys_dialogs::file_dialog().set_title(crate::i18n::tr("Save the planes")).set_file_name(&raw_name).add_filter(crate::i18n::tr("Raw planes"), &["raw", "bpl", "bin"]).save_file() {
                            write(&path, &planes())?;
                        }
                    }
                    Asked::SavePalette(kind) => {
                        let (ending, bytes) = match kind {
                            PaletteKind::Words => ("pal", planar::palette_words(&converted.palette, converted.chipset)),
                            PaletteKind::Rgb => ("rgb", planar::palette_rgb(&converted.palette)),
                            PaletteKind::Copper => ("s", planar::palette_copper(&converted.palette, converted.chipset).into_bytes()),
                            PaletteKind::C => ("h", planar::palette_c(&converted.palette, converted.chipset, &name).into_bytes()),
                        };
                        if let Some(path) = crate::sys_dialogs::file_dialog().set_title(crate::i18n::tr("Save the palette")).set_file_name(format!("{name}_palette.{ending}")).save_file() {
                            write(&path, &bytes)?;
                        }
                    }
                    Asked::SaveIlbm => {
                        if let Some(path) = crate::sys_dialogs::file_dialog().set_title(crate::i18n::tr("Save as IFF ILBM")).set_file_name(format!("{name}.iff")).add_filter(crate::i18n::tr("IFF ILBM picture"), &["iff", "ilbm", "lbm"]).save_file() {
                            write(&path, &planar::to_ilbm(converted, c.packed))?;
                        }
                    }
                    Asked::SaveSource => {
                        if let Some(path) = crate::sys_dialogs::file_dialog().set_title(crate::i18n::tr("Save the source")).set_file_name(format!("{name}.s")).add_filter(crate::i18n::tr("Assembler source"), &["s", "asm", "i"]).save_file() {
                            write(&path, planar::asm_source(converted, &name, &raw_name, c.interleaved, c.fetch).as_bytes())?;
                        }
                    }
                    Asked::SaveAll => {
                        if let Some(folder) = crate::sys_dialogs::file_dialog().set_title(crate::i18n::tr("Save everything into a folder")).pick_folder() {
                            write(&folder.join(&raw_name), &planes())?;
                            write(&folder.join(format!("{name}.pal")), &planar::palette_words(&converted.palette, converted.chipset))?;
                            write(&folder.join(format!("{name}_palette.s")), planar::palette_copper(&converted.palette, converted.chipset).as_bytes())?;
                            write(&folder.join(format!("{name}_palette.h")), planar::palette_c(&converted.palette, converted.chipset, &name).as_bytes())?;
                            write(&folder.join(format!("{name}.iff")), &planar::to_ilbm(converted, c.packed))?;
                            write(&folder.join(format!("{name}.s")), planar::asm_source(converted, &name, &raw_name, c.interleaved, c.fetch).as_bytes())?;
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

    #[test]
    fn the_converter_draws_every_tab_and_converts() {
        let (mut app, ctx) = app_with("");
        let mut converter = Converter::default();
        let rgba: Vec<u8> = (0..64 * 48).flat_map(|i| [(i % 64 * 4) as u8, (i / 64 * 5) as u8, 128, if i % 7 == 0 { 0 } else { 255 }]).collect();
        converter.set_picture("test.png", planar::Image { width: 64, height: 48, rgba });
        converter.settings.width = 64;
        converter.settings.height = 48;
        converter.raw_file = Some(("planes.raw".into(), vec![0x5A; 64 * 48]));
        app.bitplanes = Some(Box::new(converter));
        for tab in [crate::bitplane_tool::Tab::Picture, crate::bitplane_tool::Tab::Sprites, crate::bitplane_tool::Tab::Bob, crate::bitplane_tool::Tab::Raw] {
            app.bitplanes.as_mut().unwrap().tab = tab;
            for _ in 0..3 {
                ctx.run_ui(Default::default(), |ui| app.bitplane_window(ui.ctx())).textures_delta.clear();
            }
        }
        // The conversion runs in the background; it's taken in when the window is next drawn.
        let started = std::time::Instant::now();
        while app.bitplanes.as_ref().unwrap().result.is_none() && started.elapsed().as_secs() < 30 {
            std::thread::sleep(std::time::Duration::from_millis(20));
            ctx.run_ui(Default::default(), |ui| app.bitplane_window(ui.ctx())).textures_delta.clear();
        }
        let converter = app.bitplanes.as_ref().unwrap();
        assert_eq!(converter.result.as_ref().unwrap().1.width, 64);
        assert!(converter.sprites.is_some() && converter.bob.is_some() && converter.raw_shown.is_some());
    }
}
