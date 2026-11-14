//! The Atari ST picture converter's window: the machine chosen at the top, a picture brought in and its settings,
//! the result as the machine would show it, and buttons for what's to be saved; software sprites and a font from the
//! same picture; and raw planes from a file shown as a picture. The work is in `st_planar`, `st_spectrum` and
//! `st_sprites`; the window's host does the files.

use crate::amiga_sprites::Transparency;
use crate::quantise::{Dither, Filter, Fit, Image, identifier};
use crate::st_planar::{self as st, Converted, Machine, RawView, Screen, Settings};
use crate::st_sprites::{self as sprites, Font, FontSettings, Ink, MaskKind, Sprite, SpriteSettings};
use eframe::egui::{self, Color32, RichText, Ui, vec2};
use std::sync::{Arc, mpsc};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tab {
    Picture,
    Sprite,
    Font,
    Raw,
}

/// What the palette is saved as.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PaletteKind {
    Raw,
    Asm,
    C,
}

/// What the window asks its host to do.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Asked {
    OpenPicture,
    OpenPalette,
    OpenRaw,
    SaveScreen,
    SavePalette(PaletteKind),
    SaveDegas,
    SaveNeo,
    SaveIlbm,
    SaveSpectrum,
    SaveSource,
    SaveAll,
    SaveSprite,
    SaveFont,
    SavePng,
}

/// Which colours raw planes are shown in.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RawColours {
    Greys,
    Converted,
    Given,
    InFile,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Transparent {
    Alpha,
    TopLeft,
    Opaque,
}

pub struct Converter {
    pub tab: Tab,
    pub picture: Option<(String, Arc<Image>)>,
    pub settings: Settings,
    /// Degas and Spectrum pictures packed (.PC1-3, .SPC).
    pub packed: bool,
    pub name: String,
    pub palette_name: Option<String>,
    pub result: Option<(Settings, Converted)>,
    working: Option<(Settings, mpsc::Receiver<Converted>)>,
    texture: Option<egui::TextureHandle>,
    shown: Option<(Tab, u64)>,
    pub true_shape: bool,
    pub sprite_settings: SpriteSettings,
    pub sprite_size: (usize, usize),
    transparent: Transparent,
    pub sprite_screen_palette: bool,
    pub sprite: Option<Sprite>,
    pub font_settings: FontSettings,
    pub font: Option<Font>,
    /// Sprites and fonts as `dc.w` in the source rather than INCBIN from raw files.
    pub inline: bool,
    made: Option<u64>,
    pub raw: RawView,
    pub raw_file: Option<(String, Vec<u8>)>,
    pub raw_colours: RawColours,
    pub raw_shown: Option<Vec<u8>>,
    serial: u64,
}

impl Default for Converter {
    fn default() -> Self {
        Converter {
            tab: Tab::Picture,
            picture: None,
            settings: Settings::default(),
            packed: false,
            name: "picture".into(),
            palette_name: None,
            result: None,
            working: None,
            texture: None,
            shown: None,
            true_shape: true,
            sprite_settings: SpriteSettings::default(),
            sprite_size: (32, 32),
            transparent: Transparent::Alpha,
            sprite_screen_palette: false,
            sprite: None,
            font_settings: FontSettings::default(),
            font: None,
            inline: true,
            made: None,
            raw: RawView::default(),
            raw_file: None,
            raw_colours: RawColours::Greys,
            raw_shown: None,
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

fn dim(text: impl Into<String>) -> RichText {
    RichText::new(text.into()).color(crate::theme::text_dim()).size(12.0)
}

/// A row of colour swatches, each named and its palette word shown when pointed at.
fn swatches(ui: &mut Ui, palette: &[[u8; 3]], levels: st::Levels) {
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing = vec2(2.0, 2.0);
        for (i, &c) in palette.iter().enumerate() {
            let (rect, response) = ui.allocate_exact_size(vec2(14.0, 14.0), egui::Sense::hover());
            ui.painter().rect_filled(rect, 1.0, Color32::from_rgb(c[0], c[1], c[2]));
            let word = if levels == st::Levels::Falcon { format!("${:08X}", st::falcon_long(c)) } else { format!("${:03X}", st::st_word(c, levels)) };
            response.on_hover_text(format!("Colour {i}: #{:02X}{:02X}{:02X} ({word})", c[0], c[1], c[2]));
        }
    });
}

impl Converter {
    /// A picture brought in, named for the files made from it.
    pub fn set_picture(&mut self, name: &str, image: Image) {
        let stem = std::path::Path::new(name).file_stem().map_or_else(|| "picture".to_string(), |s| s.to_string_lossy().into_owned());
        self.name = identifier(&stem.to_lowercase());
        self.sprite_size = (image.width.min(64), image.height.min(64));
        self.picture = Some((name.to_string(), Arc::new(image)));
        self.result = None;
        self.serial += 1;
    }

    /// Starts converting in the background when the settings or picture have changed, and takes the result in.
    fn keep_up(&mut self, ctx: &egui::Context) {
        if let Some((settings, receiver)) = &self.working {
            match receiver.try_recv() {
                Ok(converted) => {
                    self.result = Some((settings.clone(), converted));
                    self.working = None;
                    self.shown = None;
                }
                Err(mpsc::TryRecvError::Empty) => {
                    ctx.request_repaint_after(std::time::Duration::from_millis(50));
                    return;
                }
                Err(mpsc::TryRecvError::Disconnected) => self.working = None,
            }
        }
        let Some((_, image)) = &self.picture else {
            return;
        };
        if self.result.as_ref().is_some_and(|(s, _)| *s == self.settings) {
            return;
        }
        let (sender, receiver) = mpsc::channel();
        let (image, settings) = (Arc::clone(image), self.settings.clone());
        let repaint = ctx.clone();
        std::thread::spawn(move || {
            let _ = sender.send(st::convert(&image, &settings));
            repaint.request_repaint();
        });
        self.working = Some((self.settings.clone(), receiver));
    }

    /// Remakes the sprite and font when what they're made from has changed.
    fn make_small(&mut self) {
        let Some((_, image)) = &self.picture else {
            return;
        };
        let transparency = match self.transparent {
            Transparent::Alpha => Transparency::Alpha,
            Transparent::Opaque => Transparency::Opaque,
            Transparent::TopLeft => Transparency::Colour(image.rgba.get(..3).map_or([0, 0, 0], |c| [c[0], c[1], c[2]])),
        };
        let screen_palette: Vec<[u8; 3]> = if self.sprite_screen_palette { self.result.as_ref().map(|(_, c)| c.palette.clone()).unwrap_or_default() } else { Vec::new() };
        use std::hash::{Hash, Hasher};
        let mut key = std::collections::hash_map::DefaultHasher::new();
        let s = &self.settings;
        (self.serial, format!("{:?}{:?}{:?}{:?}{:?}{:?}{:?}{:?}", self.sprite_settings, self.sprite_size, self.transparent, screen_palette, s.dither, s.filter, s.machine, self.font_settings), s.strength.to_bits()).hash(&mut key);
        let key = key.finish();
        if self.made == Some(key) {
            return;
        }
        self.made = Some(key);
        self.shown = None;
        let cut = sprites::cut_sprite(image, self.sprite_size.0, self.sprite_size.1, s.filter, transparency);
        self.sprite = Some(sprites::make_sprite(&cut, self.sprite_settings, &screen_palette, s.machine.st_levels(), s.dither, s.strength));
        self.font = Some(sprites::make_font(image, self.font_settings));
    }

    /// The raw planes drawn again.
    fn redraw_raw(&mut self) {
        let Some((_, bytes)) = &self.raw_file else {
            self.raw_shown = None;
            return;
        };
        let palette: Vec<[u8; 3]> = match self.raw_colours {
            RawColours::Greys => Vec::new(),
            RawColours::Converted => self.result.as_ref().map(|(_, c)| c.palette.clone()).unwrap_or_default(),
            RawColours::Given => self.settings.given.clone(),
            RawColours::InFile => self.raw.file_palette(bytes).unwrap_or_default(),
        };
        self.raw_shown = Some(self.raw.draw(bytes, &palette));
    }

    /// The picture shown, at its width and height and with the pixel's width for its height.
    fn preview(&mut self, ui: &mut Ui, width: usize, height: usize, rgba: &[u8], aspect: f32, key: u64) {
        if width == 0 || height == 0 || rgba.len() < width * height * 4 {
            return;
        }
        let options = egui::TextureOptions { magnification: egui::TextureFilter::Nearest, minification: egui::TextureFilter::Linear, ..egui::TextureOptions::default() };
        if self.shown != Some((self.tab, key)) || self.texture.is_none() {
            let image = egui::ColorImage::from_rgba_unmultiplied([width, height], &rgba[..width * height * 4]);
            match &mut self.texture {
                Some(texture) => texture.set(image, options),
                None => self.texture = Some(ui.ctx().load_texture("st-picture", image, options)),
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
        self.keep_up(ui.ctx());
        ui.horizontal(|ui| {
            ui.label("Machine");
            for machine in Machine::ALL {
                ui.selectable_value(&mut self.settings.machine, machine, machine.name());
            }
        });
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
            ui.selectable_value(&mut self.tab, Tab::Picture, "Screen picture");
            ui.selectable_value(&mut self.tab, Tab::Sprite, "Sprite");
            ui.selectable_value(&mut self.tab, Tab::Font, "Font");
            ui.selectable_value(&mut self.tab, Tab::Raw, "Raw planes to picture");
        });
        ui.separator();
        match self.tab {
            Tab::Picture => self.draw_picture(ui, &mut asked),
            Tab::Sprite | Tab::Font => self.draw_small(ui, &mut asked),
            Tab::Raw => self.draw_raw(ui, &mut asked),
        }
        asked
    }

    fn draw_picture(&mut self, ui: &mut Ui, asked: &mut Option<Asked>) {
        ui.horizontal_top(|ui| {
            ui.vertical(|ui| {
                ui.set_width(340.0);
                self.picture_settings(ui, asked);
            });
            ui.vertical(|ui| {
                let busy = self.working.is_some();
                if let Some((_, c)) = &self.result {
                    let (w, h, rgba, aspect, levels) = (c.width, c.height, c.rgba.clone(), c.screen.aspect(), c.levels);
                    let (palette, error, spectrum) = (c.palette.clone(), c.error, c.screen == Screen::Spectrum);
                    let what = match c.screen {
                        Screen::FalconTrue => "true colour".to_string(),
                        Screen::Spectrum => {
                            let mut each: Vec<usize> = c.lines.iter().map(|l| {
                                let mut l = l.to_vec();
                                l.sort_unstable();
                                l.dedup();
                                l.len()
                            }).collect();
                            each.sort_unstable();
                            format!("up to {} colours a line", each.last().copied().unwrap_or(0))
                        }
                        _ => {
                            let mut seen = [false; 256];
                            c.values.iter().for_each(|&v| seen[usize::from(v)] = true);
                            format!("{} colours used", seen.iter().filter(|&&s| s).count())
                        }
                    };
                    ui.horizontal(|ui| {
                        ui.label(RichText::new(format!("{w}\u{D7}{h}, {what}, average difference {error:.3} (Oklab){}", if busy { " \u{2014} working\u{2026}" } else { "" })).color(crate::theme::text_dim()));
                        ui.checkbox(&mut self.true_shape, "Pixels their true shape");
                    });
                    self.preview(ui, w, h, &rgba, aspect, self.serial ^ 0x5A5A);
                    if spectrum {
                        ui.label(dim("The first line's first palette:"));
                    }
                    swatches(ui, &palette, levels);
                } else if busy {
                    ui.spinner();
                } else {
                    ui.label("Open a picture to convert it.");
                }
            });
        });
    }

    fn picture_settings(&mut self, ui: &mut Ui, asked: &mut Option<Asked>) {
        let s = &mut self.settings;
        let machine = s.machine;
        if !machine.screens().contains(&s.screen) {
            s.screen = Screen::Low;
        }
        egui::Grid::new("st-settings").num_columns(2).spacing([8.0, 4.0]).show(ui, |ui| {
            ui.label("Screen");
            let before = s.screen;
            egui::ComboBox::from_id_salt("st-screen").selected_text(s.screen.name(machine)).show_ui(ui, |ui| {
                for &screen in machine.screens() {
                    ui.selectable_value(&mut s.screen, screen, screen.name(machine));
                }
            });
            if s.screen != before {
                (s.width, s.height) = s.screen.size();
            }
            ui.end_row();
            ui.label("Size");
            ui.add_enabled_ui(s.screen != Screen::Spectrum, |ui| {
                ui.horizontal(|ui| {
                    ui.add(egui::DragValue::new(&mut s.width).range(16..=2048)).on_hover_text("Pixels across: the planes' rows are rounded up to 16");
                    ui.label("\u{D7}");
                    ui.add(egui::DragValue::new(&mut s.height).range(1..=2048)).on_hover_text("Pixels down");
                    if ui.small_button("Screen").on_hover_text("The whole screen's size").clicked() {
                        (s.width, s.height) = s.screen.size();
                    }
                });
            });
            ui.end_row();
            combo(ui, "st-fit", "Fitting", &mut s.fit, &Fit::ALL, |f| f.name().to_string());
            combo(ui, "st-filter", "Scaling", &mut s.filter, &Filter::ALL, |f| f.name().to_string());
            combo(ui, "st-dither", "Dithering", &mut s.dither, &Dither::ALL, |d| d.name().to_string());
            ui.label("Strength");
            ui.add_enabled(s.dither != Dither::None, egui::Slider::new(&mut s.strength, 0.0..=1.0));
            ui.end_row();
            let indexed = !matches!(s.screen, Screen::Spectrum | Screen::FalconTrue | Screen::High);
            if indexed {
                ui.label("");
                ui.checkbox(&mut s.black_background, "Colour 0 black").on_hover_text("Keep the background and border black");
                ui.end_row();
                ui.label("Colours given");
                ui.horizontal(|ui| {
                    if ui.button("Palette file\u{2026}").on_hover_text("A Degas or NEOchrome picture, ST words in assembler or C, raw words, or an IFF, JASC or GIMP palette").clicked() {
                        *asked = Some(Asked::OpenPalette);
                    }
                    if !s.given.is_empty() && ui.small_button("\u{D7}").on_hover_text("Forget it").clicked() {
                        s.given.clear();
                        s.keep_given = 0;
                        self.palette_name = None;
                    }
                });
                ui.end_row();
                if !s.given.is_empty() {
                    ui.label("Keep");
                    let most = s.given.len().min(1 << s.screen.planes().min(8));
                    ui.add(egui::Slider::new(&mut s.keep_given, 0..=most).text(format!("of {}", s.given.len()))).on_hover_text("These many of its first colours are kept as they are; the rest are found from the picture");
                    ui.end_row();
                }
            }
        });
        if s.screen == Screen::Spectrum {
            ui.label(dim("320\u{D7}199, three palettes a line changing along it; colours 0 and 15 black, as .SPC needs. Finding the colours takes a few seconds."));
        }
        ui.add_space(6.0);
        egui::Grid::new("st-out").num_columns(2).spacing([8.0, 4.0]).show(ui, |ui| {
            ui.label("Name");
            ui.text_edit_singleline(&mut self.name);
            ui.end_row();
        });
        let Some((_, c)) = &self.result else {
            return;
        };
        let bytes = st::screen_bytes(c).len();
        ui.label(dim(format!("The screen is {bytes} bytes{}", if c.screen == Screen::FalconTrue { ", a word a pixel" } else { ", its planes interleaved a word at a time" })));
        ui.add_space(4.0);
        let (st_screen, spectrum, indexed) = (c.is_st_screen(), c.screen == Screen::Spectrum, !matches!(c.screen, Screen::Spectrum | Screen::FalconTrue));
        ui.horizontal_wrapped(|ui| {
            if ui.button("Save everything\u{2026}").on_hover_text("Into a folder: the screen, the palette, the pictures this screen can be saved as and the source").clicked() {
                *asked = Some(Asked::SaveAll);
            }
            if ui.button("Raw screen\u{2026}").clicked() {
                *asked = Some(Asked::SaveScreen);
            }
            if c.screen != Screen::FalconTrue {
                ui.menu_button("Palette", |ui| {
                    for (label, kind) in [("Raw words\u{2026}", PaletteKind::Raw), ("Assembler (dc.w)\u{2026}", PaletteKind::Asm), ("C array\u{2026}", PaletteKind::C)] {
                        if ui.button(label).clicked() {
                            *asked = Some(Asked::SavePalette(kind));
                            ui.close();
                        }
                    }
                });
            }
            if st_screen {
                if ui.button("Degas\u{2026}").on_hover_text(".PI1-3, or .PC1-3 packed").clicked() {
                    *asked = Some(Asked::SaveDegas);
                }
                if ui.button("NEOchrome\u{2026}").clicked() {
                    *asked = Some(Asked::SaveNeo);
                }
            }
            if spectrum && ui.button("Spectrum\u{2026}").on_hover_text(".SPU, or .SPC packed").clicked() {
                *asked = Some(Asked::SaveSpectrum);
            }
            if indexed && ui.button("IFF ILBM\u{2026}").clicked() {
                *asked = Some(Asked::SaveIlbm);
            }
            if st_screen || spectrum || indexed {
                ui.checkbox(&mut self.packed, "Packed");
            }
            if ui.button("Source\u{2026}").on_hover_text("Assembler: the sizes, a routine showing it, the palette and INCBIN of the screen").clicked() {
                *asked = Some(Asked::SaveSource);
            }
            if c.screen != Screen::FalconTrue && ui.button("Copy the palette").clicked() {
                ui.ctx().copy_text(st::palette_asm(c, &self.name));
            }
        });
    }

    fn draw_small(&mut self, ui: &mut Ui, asked: &mut Option<Asked>) {
        self.make_small();
        ui.horizontal_top(|ui| {
            ui.vertical(|ui| {
                ui.set_width(340.0);
                egui::Grid::new("st-small").num_columns(2).spacing([8.0, 4.0]).show(ui, |ui| {
                    if self.tab == Tab::Sprite {
                        ui.label("See-through");
                        egui::ComboBox::from_id_salt("st-clear")
                            .selected_text(match self.transparent {
                                Transparent::Alpha => "Where the picture is",
                                Transparent::TopLeft => "The top left pixel's colour",
                                Transparent::Opaque => "Nowhere",
                            })
                            .show_ui(ui, |ui| {
                                ui.selectable_value(&mut self.transparent, Transparent::Alpha, "Where the picture is");
                                ui.selectable_value(&mut self.transparent, Transparent::TopLeft, "The top left pixel's colour");
                                ui.selectable_value(&mut self.transparent, Transparent::Opaque, "Nowhere");
                            });
                        ui.end_row();
                        ui.label("Size");
                        ui.horizontal(|ui| {
                            ui.add(egui::DragValue::new(&mut self.sprite_size.0).range(1..=320));
                            ui.label("\u{D7}");
                            ui.add(egui::DragValue::new(&mut self.sprite_size.1).range(1..=400));
                        });
                        ui.end_row();
                        ui.label("");
                        ui.checkbox(&mut self.sprite_screen_palette, "The screen picture's palette").on_hover_text("Colours from the screen picture, for a sprite drawn on it");
                        ui.end_row();
                        ui.label("Bit planes");
                        ui.add(egui::Slider::new(&mut self.sprite_settings.planes, 1..=4));
                        ui.end_row();
                        ui.label("");
                        ui.checkbox(&mut self.sprite_settings.preshifted, "16 copies, pre-shifted").on_hover_text("Each a pixel further right, and a word wider, so it's drawn at any pixel without shifting");
                        ui.end_row();
                        combo(ui, "st-mask", "Mask", &mut self.sprite_settings.mask, &MaskKind::ALL, |m| m.name().to_string());
                        ui.label("Mask bits set");
                        ui.horizontal(|ui| {
                            ui.selectable_value(&mut self.sprite_settings.mask_background, true, "On the background");
                            ui.selectable_value(&mut self.sprite_settings.mask_background, false, "On the sprite");
                        });
                        ui.end_row();
                    } else {
                        ui.label("Characters");
                        ui.horizontal(|ui| {
                            ui.selectable_value(&mut self.font_settings.size, 8, "8\u{D7}8");
                            ui.selectable_value(&mut self.font_settings.size, 16, "16\u{D7}16");
                        });
                        ui.end_row();
                        combo(ui, "st-ink", "Set where", &mut self.font_settings.ink, &Ink::ALL, |i| i.name().to_string());
                        ui.label("First is");
                        ui.add(egui::DragValue::new(&mut self.font_settings.first).range(0..=255)).on_hover_text("The first character's code: 32 is the space");
                        ui.end_row();
                        ui.label("At most");
                        ui.add(egui::DragValue::new(&mut self.font_settings.count).range(1..=256));
                        ui.end_row();
                    }
                    ui.label("Name");
                    ui.text_edit_singleline(&mut self.name);
                    ui.end_row();
                    ui.label("Source");
                    ui.horizontal(|ui| {
                        ui.selectable_value(&mut self.inline, true, "dc.w");
                        ui.selectable_value(&mut self.inline, false, "INCBIN");
                    });
                    ui.end_row();
                });
                if self.tab == Tab::Sprite {
                    ui.label(dim("Scaling and dithering are the screen picture's."));
                    if let Some(s) = &self.sprite {
                        ui.label(dim(format!("{} words a row, {} bytes a copy, {} in all", s.words(), s.copy_bytes(), s.copy_bytes() * s.copies())));
                    }
                    if ui.add_enabled(self.sprite.is_some(), egui::Button::new("Save the sprite\u{2026}")).on_hover_text("Its raw data and source").clicked() {
                        *asked = Some(Asked::SaveSprite);
                    }
                } else {
                    ui.label(dim("Cut from the picture at its own pixels, left to right and down."));
                    if let Some(f) = &self.font {
                        ui.label(dim(format!("{} characters, {} bytes", f.glyphs.len(), f.data().len())));
                    }
                    if ui.add_enabled(self.font.as_ref().is_some_and(|f| !f.glyphs.is_empty()), egui::Button::new("Save the font\u{2026}")).on_hover_text("Its raw data and source").clicked() {
                        *asked = Some(Asked::SaveFont);
                    }
                }
            });
            ui.vertical(|ui| {
                let shown = if self.tab == Tab::Sprite {
                    self.sprite.as_ref().map(|s| (s.width, s.height, s.rgba.clone(), s.palette.clone()))
                } else {
                    self.font.as_ref().map(|f| {
                        let (w, h, rgba) = f.picture();
                        (w, h, rgba, Vec::new())
                    })
                };
                match shown {
                    Some((w, h, rgba, palette)) => {
                        self.preview(ui, w, h, &rgba, 1.0, self.made.unwrap_or(0));
                        swatches(ui, &palette, self.settings.machine.st_levels());
                    }
                    None => {
                        ui.label("Open a picture to make them from it.");
                    }
                }
            });
        });
    }

    fn draw_raw(&mut self, ui: &mut Ui, asked: &mut Option<Asked>) {
        let before = (self.raw.clone(), self.raw_colours);
        ui.horizontal_top(|ui| {
            ui.vertical(|ui| {
                ui.set_width(340.0);
                ui.horizontal(|ui| {
                    if ui.button("Open raw planes\u{2026}").clicked() {
                        *asked = Some(Asked::OpenRaw);
                    }
                    if let Some((name, bytes)) = &self.raw_file {
                        ui.label(RichText::new(format!("{name}, {} bytes", bytes.len())).color(crate::theme::text_dim()));
                    }
                });
                let r = &mut self.raw;
                egui::Grid::new("st-raw").num_columns(2).spacing([8.0, 4.0]).show(ui, |ui| {
                    ui.label("Starting at");
                    ui.add(egui::DragValue::new(&mut r.offset).range(0..=usize::MAX).hexadecimal(1, false, true));
                    ui.end_row();
                    ui.label("Size");
                    ui.horizontal(|ui| {
                        ui.add(egui::DragValue::new(&mut r.width).range(16..=2048).speed(16.0));
                        ui.label("\u{D7}");
                        ui.add(egui::DragValue::new(&mut r.height).range(1..=2048));
                    });
                    ui.end_row();
                    ui.label("Planes");
                    ui.horizontal(|ui| {
                        for (planes, label) in [(1, "1"), (2, "2"), (4, "4"), (8, "8"), (16, "True colour")] {
                            ui.selectable_value(&mut r.planes, planes, label);
                        }
                    });
                    ui.end_row();
                    ui.label("Palette");
                    egui::ComboBox::from_id_salt("st-raw-colours")
                        .selected_text(match self.raw_colours {
                            RawColours::Greys => "Shades of grey",
                            RawColours::Converted => "The screen picture's",
                            RawColours::Given => "The palette file's",
                            RawColours::InFile => "Words in the file",
                        })
                        .show_ui(ui, |ui| {
                            ui.selectable_value(&mut self.raw_colours, RawColours::Greys, "Shades of grey");
                            ui.selectable_value(&mut self.raw_colours, RawColours::Converted, "The screen picture's");
                            ui.selectable_value(&mut self.raw_colours, RawColours::Given, "The palette file's");
                            ui.selectable_value(&mut self.raw_colours, RawColours::InFile, "Words in the file");
                        });
                    ui.end_row();
                    if self.raw_colours == RawColours::InFile {
                        ui.label("Palette at");
                        let mut at = r.palette_at.unwrap_or(0);
                        ui.add(egui::DragValue::new(&mut at).range(0..=usize::MAX).hexadecimal(1, false, true)).on_hover_text("Where 16 palette words are: 2 in a Degas picture, 4 in NEOchrome");
                        r.palette_at = Some(at);
                        ui.end_row();
                    }
                });
                if let Some((_, bytes)) = &self.raw_file {
                    let rows = self.raw.rows_in(bytes.len());
                    ui.horizontal(|ui| {
                        ui.label(dim(format!("{rows} rows fit")));
                        if ui.small_button("Use them").clicked() && rows > 0 {
                            self.raw.height = rows;
                        }
                    });
                    ui.horizontal(|ui| {
                        for (label, offset, planes, width, height, palette) in [("Degas low", 34, 4, 320, 200, 2), ("NEOchrome", 128, 4, 320, 200, 4), ("Screen", 0, 4, 320, 200, 0)] {
                            if ui.small_button(label).clicked() {
                                self.raw = RawView { offset, width, height, planes, palette_at: (palette > 0).then_some(palette) };
                                if palette > 0 {
                                    self.raw_colours = RawColours::InFile;
                                }
                            }
                        }
                    });
                }
                if ui.button("Palette file\u{2026}").clicked() {
                    *asked = Some(Asked::OpenPalette);
                }
                if ui.add_enabled(self.raw_shown.is_some(), egui::Button::new("Save as PNG\u{2026}")).clicked() {
                    *asked = Some(Asked::SavePng);
                }
            });
            ui.vertical(|ui| {
                if (self.raw.clone(), self.raw_colours) != before || self.raw_shown.is_none() {
                    self.redraw_raw();
                    self.shown = None;
                }
                if let Some(rgba) = self.raw_shown.clone() {
                    let (w, h) = (self.raw.width.clamp(1, 4096), self.raw.height.clamp(1, 4096));
                    self.preview(ui, w, h, &rgba, 1.0, 1);
                } else {
                    ui.label("Open a file of raw ST planes, ripped from a program or saved here, to see them as a picture.");
                }
            });
        });
    }

    /// The raw planes as shown, to save as a picture.
    pub fn raw_picture(&self) -> Option<crate::pictures::Picture> {
        let rgba = self.raw_shown.clone()?;
        let (w, h) = (self.raw.width.clamp(1, 4096), self.raw.height.clamp(1, 4096));
        Some(crate::pictures::Picture { width: w, height: h, rgba, aspect: 1.0, info: Vec::new(), cycling: None })
    }

    /// The raw file or palette changed: shown again.
    pub fn raw_changed(&mut self) {
        self.redraw_raw();
        self.shown = None;
    }
}
