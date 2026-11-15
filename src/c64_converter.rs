//! The Commodore 64 picture, sprite and charset converter's window: a picture brought in, and from it a bitmap
//! (hires, multicolour, AFLI or FLI), a character screen (hires, multicolour, extended background colour or the ROM's
//! PETSCII), or sprites, each shown as the C64 would show it with buttons for what's to be saved. The work is in
//! `c64_gfx` and `c64_chars`; the window's host does the files.

use crate::c64_chars::{self as chars, CharMode, CharScreen, CharSettings, SeeThrough, SpriteSettings, Sprites};
use crate::c64_gfx::{self as gfx, Bitmap, Mode, NAMES, Palette, Settings};
use crate::quantise::{Dither, Filter, Fit, Image, identifier};
use eframe::egui::{self, Color32, RichText, Ui, vec2};
use std::sync::{Arc, mpsc};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tab {
    Bitmap,
    Characters,
    Sprites,
}

/// What the window asks its host to do.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Asked {
    OpenPicture,
    OpenRom,
    /// The picture file the mode is kept as (Koala, Art Studio, FLI, AFLI).
    SavePicture,
    SaveAdvancedArtStudio,
    SaveRaw,
    SaveSource,
    SaveViewer,
    SaveAll,
    SaveSpd,
}

/// Work done in the background, and the settings it was started for.
struct Job<S, T> {
    done: Option<(S, T)>,
    working: Option<(S, mpsc::Receiver<T>)>,
}

impl<S: Clone + PartialEq + Send + 'static, T: Send + 'static> Job<S, T> {
    fn new() -> Self {
        Job { done: None, working: None }
    }

    /// Takes in what's finished, and starts again if the settings have changed. Whether something new came in.
    fn keep_up(&mut self, ctx: &egui::Context, settings: &S, image: Option<&Arc<Image>>, work: impl FnOnce(&Image, &S) -> T + Send + 'static) -> bool {
        let mut fresh = false;
        if let Some((s, receiver)) = &self.working {
            match receiver.try_recv() {
                Ok(result) => {
                    self.done = Some((s.clone(), result));
                    self.working = None;
                    fresh = true;
                }
                Err(mpsc::TryRecvError::Empty) => {
                    ctx.request_repaint_after(std::time::Duration::from_millis(50));
                    return false;
                }
                Err(mpsc::TryRecvError::Disconnected) => self.working = None,
            }
        }
        let Some(image) = image else {
            return fresh;
        };
        if self.done.as_ref().is_some_and(|(s, _)| s == settings) {
            return fresh;
        }
        let (sender, receiver) = mpsc::channel();
        let (image, s) = (Arc::clone(image), settings.clone());
        let repaint = ctx.clone();
        std::thread::spawn(move || {
            let _ = sender.send(work(&image, &s));
            repaint.request_repaint();
        });
        self.working = Some((settings.clone(), receiver));
        fresh
    }

    fn result(&self) -> Option<&T> {
        self.done.as_ref().map(|d| &d.1)
    }

    fn reset(&mut self) {
        self.done = None;
    }
}

pub struct Converter {
    pub tab: Tab,
    pub picture: Option<(String, Arc<Image>)>,
    pub name: String,
    pub settings: Settings,
    pub char_settings: CharSettings,
    pub sprite_settings: SpriteSettings,
    /// The character ROM's shapes, for PETSCII, and where they came from.
    pub rom: Option<(String, Arc<Vec<u8>>)>,
    bitmap: Job<Settings, Bitmap>,
    chars: Job<(CharSettings, usize), Result<CharScreen, String>>,
    sprites: Job<SpriteSettings, Sprites>,
    texture: Option<egui::TextureHandle>,
    shown: Option<(Tab, u64)>,
    pub true_shape: bool,
    serial: u64,
}

impl Default for Converter {
    fn default() -> Self {
        Converter {
            tab: Tab::Bitmap,
            picture: None,
            name: "picture".into(),
            settings: Settings::default(),
            char_settings: CharSettings::default(),
            sprite_settings: SpriteSettings::default(),
            rom: None,
            bitmap: Job::new(),
            chars: Job::new(),
            sprites: Job::new(),
            texture: None,
            shown: None,
            true_shape: true,
            serial: 0,
        }
    }
}

fn combo<T: Copy + PartialEq>(ui: &mut Ui, id: &str, label: &str, value: &mut T, all: &[T], name: impl Fn(T) -> String) {
    ui.label(label);
    egui::ComboBox::from_id_salt(id).selected_text(name(*value)).show_ui(ui, |ui| {
        for &item in all {
            ui.selectable_value(value, item, name(item));
        }
    });
    ui.end_row();
}

/// A choice of one of the C64's colours, or none ("found from the picture") if `auto` is given.
fn colour_choice(ui: &mut Ui, id: &str, label: &str, value: &mut Option<u8>, auto: Option<&str>) {
    ui.label(label);
    let text = value.map_or_else(|| auto.unwrap_or("").to_string(), |c| format!("{c}: {}", NAMES[usize::from(c & 15)]));
    egui::ComboBox::from_id_salt(id).selected_text(text).show_ui(ui, |ui| {
        if let Some(auto) = auto {
            ui.selectable_value(value, None, auto);
        }
        for c in 0..16u8 {
            ui.selectable_value(value, Some(c), format!("{c}: {}", NAMES[usize::from(c)]));
        }
    });
    ui.end_row();
}

fn dim(text: impl Into<String>) -> RichText {
    RichText::new(text.into()).color(crate::theme::text_dim()).size(12.0)
}

/// The colours used, as swatches.
fn swatches(ui: &mut Ui, colours: &[[u8; 3]; 16], used: &[(String, u8)]) {
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing = vec2(6.0, 2.0);
        for (what, c) in used {
            let rgb = colours[usize::from(c & 15)];
            let (rect, response) = ui.allocate_exact_size(vec2(14.0, 14.0), egui::Sense::hover());
            ui.painter().rect_filled(rect, 1.0, Color32::from_rgb(rgb[0], rgb[1], rgb[2]));
            response.on_hover_text(format!("{what}: colour {c}, {}", NAMES[usize::from(c & 15)]));
            ui.label(dim(what.clone()));
        }
    });
}

impl Converter {
    /// A picture brought in, named for the files made from it.
    pub fn set_picture(&mut self, name: &str, image: Image) {
        let stem = std::path::Path::new(name).file_stem().map_or_else(|| "picture".to_string(), |s| s.to_string_lossy().into_owned());
        self.name = identifier(&stem.to_lowercase());
        self.picture = Some((name.to_string(), Arc::new(image)));
        self.bitmap.reset();
        self.chars.reset();
        self.sprites.reset();
        self.serial += 1;
    }

    pub fn set_rom(&mut self, name: &str, bytes: Vec<u8>) {
        self.rom = Some((name.to_string(), Arc::new(bytes)));
        self.serial += 1;
        self.chars.reset();
    }

    pub fn bitmap(&self) -> Option<&Bitmap> {
        self.bitmap.result()
    }

    pub fn char_screen(&self) -> Option<&CharScreen> {
        self.chars.result().and_then(|r| r.as_ref().ok())
    }

    pub fn sprites(&self) -> Option<&Sprites> {
        self.sprites.result()
    }

    /// The colours shown and saved in.
    fn colours(&self) -> &'static [[u8; 3]; 16] {
        match self.tab {
            Tab::Bitmap => self.settings.palette.colours(),
            Tab::Characters => self.char_settings.palette.colours(),
            Tab::Sprites => self.sprite_settings.palette.colours(),
        }
    }

    fn keep_up(&mut self, ctx: &egui::Context) {
        let image = self.picture.as_ref().map(|p| &p.1);
        let fresh = match self.tab {
            Tab::Bitmap => self.bitmap.keep_up(ctx, &self.settings, image, gfx::convert),
            Tab::Characters => {
                let rom = self.rom.as_ref().map(|r| Arc::clone(&r.1));
                self.chars.keep_up(ctx, &(self.char_settings.clone(), self.serial as usize), image, move |i, (s, _)| chars::convert_chars(i, s, rom.as_ref().map(|r| r.as_slice())))
            }
            Tab::Sprites => self.sprites.keep_up(ctx, &self.sprite_settings, image, chars::convert_sprites),
        };
        if fresh {
            self.shown = None;
        }
    }

    /// The picture shown, at its width and height and with the pixel's width for its height.
    fn preview(&mut self, ui: &mut Ui, width: usize, height: usize, rgba: &[u8], aspect: f32) {
        if width == 0 || height == 0 || rgba.len() < width * height * 4 {
            return;
        }
        let options = egui::TextureOptions { magnification: egui::TextureFilter::Nearest, minification: egui::TextureFilter::Linear, ..egui::TextureOptions::default() };
        let key = self.serial;
        if self.shown != Some((self.tab, key)) || self.texture.is_none() {
            let image = egui::ColorImage::from_rgba_unmultiplied([width, height], &rgba[..width * height * 4]);
            match &mut self.texture {
                Some(texture) => texture.set(image, options),
                None => self.texture = Some(ui.ctx().load_texture("c64-picture", image, options)),
            }
            self.shown = Some((self.tab, key));
        }
        let Some(texture) = &self.texture else {
            return;
        };
        let aspect = if self.true_shape { aspect } else { 1.0 };
        let shown = vec2(width as f32 * aspect, height as f32);
        let room = vec2(ui.available_width().max(200.0), 520.0);
        let fit = (room.x / shown.x).min(room.y / shown.y);
        let scale = if fit >= 1.0 { fit.floor().min(8.0) } else { fit };
        let (rect, _) = ui.allocate_exact_size(shown * scale, egui::Sense::hover());
        let painter = ui.painter_at(rect);
        painter.rect_filled(rect, 0.0, Color32::from_gray(90));
        let step = 8.0;
        for row in 0..(rect.height() / step).ceil() as usize {
            for column in 0..(rect.width() / step).ceil() as usize {
                if (row + column) % 2 == 0 {
                    let min = rect.min + vec2(column as f32 * step, row as f32 * step);
                    painter.rect_filled(egui::Rect::from_min_size(min, vec2(step, step)).intersect(rect), 0.0, Color32::from_gray(120));
                }
            }
        }
        painter.image(texture.id(), rect, egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)), Color32::WHITE);
    }

    pub fn draw(&mut self, ui: &mut Ui) -> Option<Asked> {
        let mut asked = None;
        let tab = self.tab;
        self.keep_up(ui.ctx());
        ui.horizontal(|ui| {
            if ui.button("Open a picture\u{2026}").on_hover_text("PNG, JPEG, IFF ILBM, or any older machine's picture Limoniter reads").clicked() {
                asked = Some(Asked::OpenPicture);
            }
            match &self.picture {
                Some((name, image)) => ui.label(RichText::new(format!("{name}, {}\u{D7}{}", image.width, image.height)).color(crate::theme::text_dim())),
                None => ui.label(RichText::new("No picture yet").color(crate::theme::text_dim())),
            };
        });
        ui.horizontal(|ui| {
            ui.selectable_value(&mut self.tab, Tab::Bitmap, "Bitmap picture");
            ui.selectable_value(&mut self.tab, Tab::Characters, "Characters");
            ui.selectable_value(&mut self.tab, Tab::Sprites, "Sprites");
        });
        if self.tab != tab {
            self.shown = None;
        }
        ui.separator();
        ui.horizontal_top(|ui| {
            ui.vertical(|ui| {
                ui.set_width(340.0);
                match self.tab {
                    Tab::Bitmap => self.bitmap_settings(ui, &mut asked),
                    Tab::Characters => self.char_settings_ui(ui, &mut asked),
                    Tab::Sprites => self.sprite_settings_ui(ui, &mut asked),
                }
            });
            ui.vertical(|ui| self.result_view(ui));
        });
        asked
    }

    fn common(ui: &mut Ui, id: &str, palette: &mut Palette, dither: &mut Dither, strength: &mut f32, scaling: Option<(&mut Fit, &mut Filter)>) {
        combo(ui, &format!("{id}-palette"), "Colours", palette, &Palette::ALL, |p| p.name().to_string());
        if let Some((fit, filter)) = scaling {
            combo(ui, &format!("{id}-fit"), "Fitting", fit, &Fit::ALL, |f| f.name().to_string());
            combo(ui, &format!("{id}-filter"), "Scaling", filter, &Filter::ALL, |f| f.name().to_string());
        }
        combo(ui, &format!("{id}-dither"), "Dithering", dither, &Dither::ALL, |d| d.name().to_string());
        ui.label("Strength");
        ui.add_enabled(*dither != Dither::None, egui::Slider::new(strength, 0.0..=1.0));
        ui.end_row();
    }

    fn name_row(&mut self, ui: &mut Ui) {
        ui.label("Name");
        ui.text_edit_singleline(&mut self.name);
        ui.end_row();
    }

    fn bitmap_settings(&mut self, ui: &mut Ui, asked: &mut Option<Asked>) {
        let s = &mut self.settings;
        egui::Grid::new("c64-bitmap").num_columns(2).spacing([8.0, 4.0]).show(ui, |ui| {
            combo(ui, "c64-mode", "Mode", &mut s.mode, &Mode::ALL, |m| m.name().to_string());
            Self::common(ui, "c64", &mut s.palette, &mut s.dither, &mut s.strength, Some((&mut s.fit, &mut s.filter)));
            if s.mode == Mode::Multicolour {
                colour_choice(ui, "c64-background", "Background", &mut s.background, Some("Found from the picture"));
            }
            let mut border = Some(s.border);
            colour_choice(ui, "c64-border", "Border", &mut border, None);
            s.border = border.unwrap_or(0);
        });
        if s.mode.fli() {
            ui.label(dim("The first 3 columns can't show colours of their own (the FLI bug): they're left plain. FLI's background is black; the last 3 lines keep line 196's colours."));
        }
        egui::Grid::new("c64-bitmap-name").num_columns(2).spacing([8.0, 4.0]).show(ui, |ui| self.name_row(ui));
        let Some(b) = self.bitmap.result() else { return };
        ui.add_space(4.0);
        let (picture, ending) = match b.mode {
            Mode::Multicolour => ("Koala\u{2026}", ".koa"),
            Mode::Hires => ("Art Studio\u{2026}", ".art"),
            Mode::Fli => ("FLI Designer\u{2026}", ".fli"),
            Mode::Afli => ("AFLI\u{2026}", ".afl"),
        };
        ui.horizontal_wrapped(|ui| {
            if ui.button("Save everything\u{2026}").on_hover_text("Into a folder: the picture file, raw bitmap, screen and colour RAM, the source and the viewer").clicked() {
                *asked = Some(Asked::SaveAll);
            }
            if ui.button(picture).on_hover_text(ending).clicked() {
                *asked = Some(Asked::SavePicture);
            }
            if b.mode == Mode::Multicolour && ui.button("Advanced Art Studio\u{2026}").on_hover_text(".ocp").clicked() {
                *asked = Some(Asked::SaveAdvancedArtStudio);
            }
            if ui.button("Raw data\u{2026}").on_hover_text("The bitmap, screen RAM and colour RAM, each a file").clicked() {
                *asked = Some(Asked::SaveRaw);
            }
            if ui.button("Source\u{2026}").on_hover_text("ACME assembler: a program showing it, with the data").clicked() {
                *asked = Some(Asked::SaveSource);
            }
            if ui.button("Viewer (.prg)\u{2026}").on_hover_text("A program that shows it: LOAD and RUN").clicked() {
                *asked = Some(Asked::SaveViewer);
            }
        });
    }

    fn char_settings_ui(&mut self, ui: &mut Ui, asked: &mut Option<Asked>) {
        let s = &mut self.char_settings;
        egui::Grid::new("c64-chars").num_columns(2).spacing([8.0, 4.0]).show(ui, |ui| {
            combo(ui, "c64c-mode", "Mode", &mut s.mode, &CharMode::ALL, |m| m.name().to_string());
            ui.label("Size");
            ui.horizontal(|ui| {
                ui.add(egui::DragValue::new(&mut s.columns).range(1..=40)).on_hover_text("Characters across");
                ui.label("\u{D7}");
                ui.add(egui::DragValue::new(&mut s.rows).range(1..=25)).on_hover_text("Characters down");
                ui.label(dim("characters"));
            });
            ui.end_row();
            Self::common(ui, "c64c", &mut s.palette, &mut s.dither, &mut s.strength, Some((&mut s.fit, &mut s.filter)));
            colour_choice(ui, "c64c-background", "Background", &mut s.background, Some("Found from the picture"));
            if s.mode != CharMode::Petscii {
                let limit = if s.mode == CharMode::Extended { 64 } else { 256 };
                ui.label("At most");
                ui.add(egui::Slider::new(&mut s.most, 1..=limit).text("characters"));
                ui.end_row();
            }
            if s.mode == CharMode::Hires {
                ui.label("");
                ui.checkbox(&mut s.inverses, "128 and their inverses").on_hover_text("Characters 128 to 255 are 0 to 127 inverted, as in the ROM, so a cell can use a shape inverted");
                ui.end_row();
            }
            if s.mode == CharMode::Petscii {
                ui.label("Set");
                ui.horizontal(|ui| {
                    ui.selectable_value(&mut s.lower_case, false, "Upper case and graphics");
                    ui.selectable_value(&mut s.lower_case, true, "Lower case");
                });
                ui.end_row();
                ui.label("Character ROM");
                ui.horizontal(|ui| {
                    if ui.button("Open\u{2026}").on_hover_text("The C64's 4096-byte character ROM (chargen), as VICE and other emulators keep it").clicked() {
                        *asked = Some(Asked::OpenRom);
                    }
                    if let Some((name, _)) = &self.rom {
                        ui.label(dim(name.clone()));
                    }
                });
                ui.end_row();
            }
        });
        if s.mode == CharMode::Multicolour {
            ui.label(dim("Every cell multicolour: its own colour is one of the first eight."));
        }
        egui::Grid::new("c64-chars-name").num_columns(2).spacing([8.0, 4.0]).show(ui, |ui| self.name_row(ui));
        match self.chars.result() {
            Some(Err(problem)) => {
                ui.label(RichText::new(problem.clone()).color(crate::theme::problem_mark()));
            }
            Some(Ok(c)) => {
                if c.mode != CharMode::Petscii {
                    ui.label(dim(format!("{} characters ({} different cells{})", c.charset.len(), c.unique, if c.unique > c.charset.len() { ", made to fit" } else { "" })));
                }
                ui.horizontal_wrapped(|ui| {
                    if ui.button("Save everything\u{2026}").on_hover_text("Into a folder: the characters, screen and colour maps, the source and the viewer").clicked() {
                        *asked = Some(Asked::SaveAll);
                    }
                    if ui.button("Raw data\u{2026}").on_hover_text("The characters, screen map and colour map, each a file").clicked() {
                        *asked = Some(Asked::SaveRaw);
                    }
                    if ui.button("Source\u{2026}").clicked() {
                        *asked = Some(Asked::SaveSource);
                    }
                    if ui.button("Viewer (.prg)\u{2026}").clicked() {
                        *asked = Some(Asked::SaveViewer);
                    }
                });
            }
            None => {}
        }
    }

    fn sprite_settings_ui(&mut self, ui: &mut Ui, asked: &mut Option<Asked>) {
        let s = &mut self.sprite_settings;
        egui::Grid::new("c64-sprites").num_columns(2).spacing([8.0, 4.0]).show(ui, |ui| {
            ui.label("Kind");
            ui.horizontal(|ui| {
                ui.selectable_value(&mut s.multicolour, true, "Multicolour (12\u{D7}21)");
                ui.selectable_value(&mut s.multicolour, false, "Hires (24\u{D7}21)");
            });
            ui.end_row();
            if s.multicolour {
                ui.label("");
                ui.checkbox(&mut s.overlay, "A hires sprite on each").on_hover_text("Each frame two sprites: the multicolour one and a hires one on it, in one more colour at full width");
                ui.end_row();
            }
            ui.label("See-through");
            egui::ComboBox::from_id_salt("c64s-clear")
                .selected_text(match s.see_through {
                    SeeThrough::Alpha => "Where the picture is",
                    SeeThrough::TopLeft => "The top left pixel's colour",
                    SeeThrough::Nowhere => "Nowhere",
                })
                .show_ui(ui, |ui| {
                    ui.selectable_value(&mut s.see_through, SeeThrough::Alpha, "Where the picture is");
                    ui.selectable_value(&mut s.see_through, SeeThrough::TopLeft, "The top left pixel's colour");
                    ui.selectable_value(&mut s.see_through, SeeThrough::Nowhere, "Nowhere");
                });
            ui.end_row();
            Self::common(ui, "c64s", &mut s.palette, &mut s.dither, &mut s.strength, None);
            if s.multicolour {
                let mut first = s.shared.map(|p| p.0);
                let mut second = s.shared.map(|p| p.1);
                colour_choice(ui, "c64s-m1", "Shared 1 ($D025)", &mut first, Some("Found from the picture"));
                colour_choice(ui, "c64s-m2", "Shared 2 ($D026)", &mut second, Some("Found from the picture"));
                s.shared = match (first, second) {
                    (Some(a), Some(b)) => Some((a, b)),
                    (Some(a), None) => Some((a, 1)),
                    (None, Some(b)) => Some((0, b)),
                    _ => None,
                };
            }
            ui.label("Expanded");
            ui.horizontal(|ui| {
                ui.checkbox(&mut s.expand_x, "Across ($D01D)");
                ui.checkbox(&mut s.expand_y, "Down ($D017)");
            });
            ui.end_row();
            ui.label("First sprite at");
            ui.add(egui::DragValue::new(&mut s.address).range(0..=0xFFC0).speed(64.0).hexadecimal(4, false, true)).on_hover_text("Where the sprites go in memory, on a 64-byte boundary: their pointers follow from it");
            s.address &= 0xFFC0;
            ui.end_row();
            ui.label("Screen RAM at");
            ui.add(egui::DragValue::new(&mut s.screen).range(0..=0xFC00).speed(1024.0).hexadecimal(4, false, true)).on_hover_text("The pointers are its last 8 bytes");
            s.screen &= 0xFC00;
            ui.end_row();
        });
        ui.label(dim("Cut from the picture at its own pixels, 24\u{D7}21 at a time, left to right and down."));
        egui::Grid::new("c64-sprites-name").num_columns(2).spacing([8.0, 4.0]).show(ui, |ui| self.name_row(ui));
        if let Some(sp) = self.sprites.result() {
            ui.label(dim(format!("{} sprites, {} bytes; the first's pointer ${:02X} in VIC bank {}{}", sp.set.sprites.len(), sp.set.sprites.len() * 64, sp.first_pointer(), sp.bank(), if (self.sprite_settings.address >> 14) != (self.sprite_settings.screen >> 14) { " \u{2014} not the screen RAM's bank" } else { "" })));
            ui.horizontal_wrapped(|ui| {
                if ui.button("Save everything\u{2026}").on_hover_text("Into a folder: the sprites raw, as SpritePad's .spd, the source and the viewer").clicked() {
                    *asked = Some(Asked::SaveAll);
                }
                if ui.button("Raw sprites\u{2026}").on_hover_text("64 bytes each").clicked() {
                    *asked = Some(Asked::SaveRaw);
                }
                if ui.button("SpritePad (.spd)\u{2026}").clicked() {
                    *asked = Some(Asked::SaveSpd);
                }
                if ui.button("Source\u{2026}").on_hover_text("ACME: the pointers and registers, a program showing the first ones, and the sprites").clicked() {
                    *asked = Some(Asked::SaveSource);
                }
                if ui.button("Viewer (.prg)\u{2026}").clicked() {
                    *asked = Some(Asked::SaveViewer);
                }
            });
        }
    }

    fn result_view(&mut self, ui: &mut Ui) {
        let colours = self.colours();
        let busy = match self.tab {
            Tab::Bitmap => self.bitmap.working.is_some(),
            Tab::Characters => self.chars.working.is_some(),
            Tab::Sprites => self.sprites.working.is_some(),
        };
        let working = if busy { " \u{2014} working\u{2026}" } else { "" };
        let shown: Option<(String, usize, usize, Vec<u8>, f32, Vec<(String, u8)>)> = match self.tab {
            Tab::Bitmap => self.bitmap.result().map(|b| {
                let mut used = vec![("background".to_string(), b.background), ("border".to_string(), b.border)];
                if !b.mode.multicolour() {
                    used.remove(0);
                }
                (format!("{}, average difference {:.3} (Oklab)", b.mode.name(), b.error), 320, 200, b.rgba(colours), gfx::PIXEL_ASPECT, used)
            }),
            Tab::Characters => match self.chars.result() {
                Some(Ok(c)) => {
                    let rom = self.rom.as_ref().and_then(|r| chars::rom_set(&r.1, c.lower_case));
                    let rgba = c.indices(rom.as_deref()).iter().flat_map(|&i| {
                        let p = colours[usize::from(i & 15)];
                        [p[0], p[1], p[2], 255]
                    }).collect();
                    let names: &[&str] = match c.mode {
                        CharMode::Multicolour => &["background", "$D022", "$D023"],
                        CharMode::Extended => &["background 0", "1", "2", "3"],
                        _ => &["background"],
                    };
                    let used = names.iter().zip(c.backgrounds).map(|(n, b)| (n.to_string(), b)).collect();
                    Some((format!("{}\u{D7}{} characters, average difference {:.3} (Oklab)", c.columns, c.rows, c.error), c.columns * 8, c.rows * 8, rgba, gfx::PIXEL_ASPECT, used))
                }
                _ => None,
            },
            Tab::Sprites => self.sprites.result().map(|sp| {
                let (w, h, rgba) = sp.sheet_rgba(colours);
                let used = if sp.set.sprites.first().is_some_and(|b| b[63] & 0x80 != 0) { vec![("$D025".to_string(), sp.set.multicolour1), ("$D026".to_string(), sp.set.multicolour2)] } else { Vec::new() };
                (format!("{} frames, average difference {:.3} (Oklab)", sp.across * sp.down, sp.error), w, h, rgba, gfx::PIXEL_ASPECT, used)
            }),
        };
        match shown {
            Some((what, w, h, rgba, aspect, used)) => {
                ui.horizontal(|ui| {
                    ui.label(RichText::new(format!("{what}{working}")).color(crate::theme::text_dim()));
                    ui.checkbox(&mut self.true_shape, "Pixels their true shape");
                });
                self.preview(ui, w, h, &rgba, aspect);
                swatches(ui, colours, &used);
            }
            None if busy => {
                ui.spinner();
            }
            None => {
                ui.label("Open a picture to convert it.");
            }
        }
    }
}
