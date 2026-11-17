//! The window's side of the Commodore 64 picture, sprite and charset converter: its window, kept open beside the
//! text, and the files it opens and saves.

use super::*;
use crate::c64_converter::{Asked, Converter, Tab};

const TITLE: &str = "Commodore 64 picture, sprite and charset converter";

/// Where a C64 character ROM is likely to be: beside VICE, or where Linux packages and VICE's own folders keep it.
fn find_chargen() -> Option<(String, Vec<u8>)> {
    let mut places: Vec<PathBuf> = Vec::new();
    if let Some((_, program)) = crate::emulators::find_installed().into_iter().find(|(e, _)| e.id == "vice") {
        for up in program.ancestors().skip(1).take(3) {
            places.push(up.join("C64"));
            places.push(up.join("share/vice/C64"));
            places.push(up.join("data/C64"));
        }
    }
    if let Some(home) = std::env::var_os("HOME") {
        places.push(PathBuf::from(home).join(".local/share/vice/C64"));
    }
    places.extend(["/usr/share/vice/C64", "/usr/lib/vice/C64", "/usr/local/share/vice/C64", "/usr/local/lib/vice/C64", "/opt/homebrew/share/vice/C64"].map(PathBuf::from));
    for folder in places {
        for name in ["chargen-901225-01.bin", "chargen"] {
            let path = folder.join(name);
            if let Ok(bytes) = std::fs::read(&path)
                && bytes.len() == 4096
            {
                return Some((path.display().to_string(), bytes));
            }
        }
    }
    None
}

impl Limoniter {
    /// Opens the converter, with the picture in front brought in if it is one.
    pub(super) fn open_c64_converter(&mut self) {
        if self.c64_pictures.is_some() {
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
        if let Some((name, rom)) = find_chargen() {
            converter.set_rom(&name, rom);
        }
        self.c64_pictures = Some(Box::new(converter));
    }

    pub(super) fn c64_converter_window(&mut self, ctx: &Context) {
        let Some(mut converter) = self.c64_pictures.take() else {
            return;
        };
        let mut open = true;
        let mut asked = None;
        egui::Window::new(crate::i18n::tr(TITLE)).open(&mut open).resizable(true).default_pos([160.0, 80.0]).default_width(900.0).show(ctx, |ui| {
            asked = converter.draw(ui);
        });
        if !open {
            return;
        }
        if let Some(asked) = asked
            && let Err(problem) = self.c64_converter_asked(&mut converter, asked)
        {
            self.message(TITLE, problem);
        }
        self.c64_pictures = Some(converter);
    }

    fn c64_converter_asked(&mut self, c: &mut Converter, asked: Asked) -> Result<(), String> {
        let write = |path: &Path, bytes: &[u8]| codec::write_file(path, bytes).map_err(|e| crate::i18n::tf!("Couldn't save {}.\n\n{e}", path.display()));
        let read = |path: &Path| std::fs::read(path).map_err(|e| crate::i18n::tf!("Couldn't read {}.\n\n{e}", path.display()));
        let file_name = |p: &Path| p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        let name = c.name.clone();
        let save = |title: &str, file: &str, filter: (&str, &[&str])| rfd::FileDialog::new().set_title(title).set_file_name(file).add_filter(filter.0, filter.1).save_file();
        match asked {
            Asked::OpenPicture => {
                if let Some(path) = rfd::FileDialog::new().set_title(crate::i18n::tr("Open a picture to convert")).add_filter(crate::i18n::tr("Pictures"), &["png", "jpg", "jpeg", "iff", "ilbm", "lbm", "pcx", "koa", "kla", "art", "ocp", "fli", "afl", "neo", "pi1"]).add_filter(crate::i18n::tr("Any file"), &["*"]).pick_file() {
                    let image = crate::quantise::read_picture(&file_name(&path), &read(&path)?).map_err(|e| crate::i18n::tf!("Couldn't open {}.\n\n{e}", path.display()))?;
                    c.set_picture(&file_name(&path), image);
                }
            }
            Asked::OpenRom => {
                if let Some(path) = rfd::FileDialog::new().set_title(crate::i18n::tr("Open the C64's character ROM")).add_filter(crate::i18n::tr("Character ROM"), &["bin", "rom", "chr"]).add_filter(crate::i18n::tr("Any file"), &["*"]).pick_file() {
                    let bytes = read(&path)?;
                    if bytes.len() != 4096 && bytes.len() != 2048 {
                        return Err(crate::i18n::tf!("{} is {} bytes: the C64's character ROM is 4096 (2048 for one set).", path.display(), bytes.len()));
                    }
                    c.set_rom(&file_name(&path), bytes);
                }
            }
            _ => {
                // What the tab in front made, by name.
                let all: Vec<(String, Vec<u8>)> = match c.tab {
                    Tab::Bitmap => c.bitmap().map(|b| b.everything(&name)),
                    Tab::Characters => c.char_screen().map(|s| s.everything(&name)),
                    Tab::Sprites => c.sprites().map(|s| s.everything(&name)),
                }
                .unwrap_or_default();
                if all.is_empty() {
                    return Ok(());
                }
                let pick = |ending: &str| all.iter().find(|(n, _)| n.ends_with(ending)).cloned();
                let save_one = |title: &str, item: Option<(String, Vec<u8>)>, filter: (&str, &[&str])| -> Result<(), String> {
                    if let Some((file, bytes)) = item
                        && let Some(path) = save(title, &file, filter)
                    {
                        write(&path, &bytes)?;
                    }
                    Ok(())
                };
                match asked {
                    Asked::SavePicture => {
                        let item = all.first().cloned();
                        let ending = item.as_ref().and_then(|(n, _)| n.rsplit('.').next().map(str::to_string)).unwrap_or_default();
                        let endings: &[&str] = match ending.as_str() {
                            "koa" => &["koa", "kla"],
                            "art" => &["art", "aas"],
                            "fli" => &["fli"],
                            _ => &["afl"],
                        };
                        save_one("Save the picture", item, ("Commodore 64 picture", endings))?;
                    }
                    Asked::SaveAdvancedArtStudio => save_one("Save as Advanced Art Studio", pick(".ocp"), ("Advanced Art Studio picture", &["ocp", "mpic"]))?,
                    Asked::SaveSpd => save_one("Save as SpritePad", pick(".spd"), ("SpritePad sprites", &["spd"]))?,
                    Asked::SaveSource => save_one("Save the source", pick(".asm"), ("ACME source", &["asm", "a", "s"]))?,
                    Asked::SaveViewer => save_one("Save the viewer", pick(".prg"), ("Commodore 64 program", &["prg"]))?,
                    Asked::SaveRaw => {
                        if let Some(folder) = rfd::FileDialog::new().set_title(crate::i18n::tr("Save the raw data into a folder")).pick_folder() {
                            for (file, bytes) in all.iter().filter(|(n, _)| n.ends_with(".bin")) {
                                write(&folder.join(file), bytes)?;
                            }
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

    #[test]
    fn the_converter_draws_every_tab_and_converts() {
        let (mut app, ctx) = app_with("");
        let mut converter = Converter::default();
        let rgba: Vec<u8> = (0..48 * 42).flat_map(|i| [(i % 48 * 5) as u8, (i / 48 * 6) as u8, 128, if i % 7 == 0 { 0 } else { 255 }]).collect();
        converter.set_picture("test.png", crate::quantise::Image { width: 48, height: 42, rgba });
        converter.char_settings.columns = 8;
        converter.char_settings.rows = 5;
        app.c64_pictures = Some(Box::new(converter));
        for tab in [Tab::Bitmap, Tab::Characters, Tab::Sprites] {
            app.c64_pictures.as_mut().unwrap().tab = tab;
            let started = std::time::Instant::now();
            loop {
                ctx.run_ui(Default::default(), |ui| app.c64_converter_window(ui.ctx())).textures_delta.clear();
                let c = app.c64_pictures.as_ref().unwrap();
                let done = match tab {
                    Tab::Bitmap => c.bitmap().is_some(),
                    Tab::Characters => c.char_screen().is_some(),
                    Tab::Sprites => c.sprites().is_some(),
                };
                if done || started.elapsed().as_secs() > 120 {
                    break;
                }
                std::thread::sleep(std::time::Duration::from_millis(20));
            }
            ctx.run_ui(Default::default(), |ui| app.c64_converter_window(ui.ctx())).textures_delta.clear();
        }
        let c = app.c64_pictures.as_ref().unwrap();
        assert_eq!(c.bitmap().unwrap().bitmap.len(), 8000);
        assert_eq!(c.char_screen().unwrap().screen.len(), 40);
        let sprites = c.sprites().unwrap();
        assert_eq!((sprites.across, sprites.down), (2, 2));
    }
}
