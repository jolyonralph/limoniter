//! The Amiga bitplane converter's window: a picture brought in and its settings, the result as the Amiga would show
//! it, and buttons for what's to be saved; sprites and bobs from the same picture; and raw planes from a file shown
//! as a picture. The work is in `amiga_planar` and `amiga_sprites`; the window's host does the files.

use crate::amiga_planar::{self as planar, Chipset, Converted, Dither, Fetch, Filter, Fit, Image, Mode, RawView, Resolution, Settings};
use crate::amiga_sprites::{self as sprites, Bob, BobSettings, SpriteSettings, SpriteWidth, Sprites, Transparency};
use eframe::egui::{self, Color32, RichText, Ui, vec2};
use std::sync::{Arc, mpsc};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tab {
    Picture,
    Sprites,
    Bob,
    Raw,
}

/// What the palette is saved as.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PaletteKind {
    Words,
    Rgb,
    Copper,
    C,
}

/// What the window asks its host to do.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Asked {
    OpenPicture,
    OpenPalette,
    OpenRaw,
    SavePlanes,
    SavePalette(PaletteKind),
    SaveIlbm,
    SaveSource,
    SaveAll,
    SaveSprites,
    SaveBob,
    SavePng,
}

/// Which colours raw planes are shown in.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RawColours {
    Greys,
    Converted,
    Given,
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
    pub interleaved: bool,
    pub fetch: Fetch,
    pub packed: bool,
    /// The name the files and labels are given.
    pub name: String,
    pub palette_name: Option<String>,
    pub result: Option<(Settings, Converted)>,
    working: Option<(Settings, mpsc::Receiver<Converted>)>,
    texture: Option<egui::TextureHandle>,
    shown: Option<(Tab, usize)>,
    /// Pixels shown their shape on the screen, rather than square.
    pub true_shape: bool,
    pub sprite: SpriteSettings,
    pub sprite_size: (usize, usize),
    transparent: Transparent,
    pub sprites: Option<Sprites>,
    pub bob_settings: BobSettings,
    pub bob_size: (usize, usize),
    /// The bob takes the converted picture's palette, as a bob on that screen would.
    pub bob_screen_palette: bool,
    pub bob: Option<Bob>,
    /// Sprites and bobs as `dc.w` in the source rather than INCBIN from raw files.
    pub inline: bool,
    made: Option<u64>,
    pub raw: RawView,
    pub raw_file: Option<(String, Vec<u8>)>,
    pub raw_colours: RawColours,
    pub raw_chipset: Chipset,
    pub raw_shown: Option<Vec<u8>>,
    serial: u64,
}

impl Default for Converter {
    fn default() -> Self {
        Converter {
            tab: Tab::Picture,
            picture: None,
            settings: Settings::default(),
            interleaved: false,
            fetch: Fetch::Words,
            packed: true,
            name: "picture".into(),
            palette_name: None,
            result: None,
            working: None,
            texture: None,
            shown: None,
            true_shape: true,
            sprite: SpriteSettings::default(),
            sprite_size: (16, 16),
            transparent: Transparent::Alpha,
            sprites: None,
            bob_settings: BobSettings::default(),
            bob_size: (32, 32),
            bob_screen_palette: false,
            bob: None,
            inline: true,
            made: None,
            raw: RawView::default(),
            raw_file: None,
            raw_colours: RawColours::Greys,
            raw_chipset: Chipset::Ocs,
            raw_shown: None,
            serial: 0,
        }
    }
}

fn hex(c: [u8; 3]) -> String {
    format!("#{:02X}{:02X}{:02X}", c[0], c[1], c[2])
}

fn combo<T: Copy + PartialEq>(ui: &mut Ui, id: &str, label: &str, value: &mut T, all: &[T], name: impl Fn(T) -> String) {
    ui.label(crate::i18n::tr(label));
    egui::ComboBox::from_id_salt(id).selected_text(name(*value)).show_ui(ui, |ui| {
        for &item in all {
            ui.selectable_value(value, item, name(item));
        }
    });
    ui.end_row();
}

/// A row of colour swatches, each named and its value shown when pointed at.
fn swatches(ui: &mut Ui, palette: &[[u8; 3]], first: usize) {
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing = vec2(2.0, 2.0);
        for (i, &c) in palette.iter().enumerate() {
            let (rect, response) = ui.allocate_exact_size(vec2(14.0, 14.0), egui::Sense::hover());
            ui.painter().rect_filled(rect, 1.0, Color32::from_rgb(c[0], c[1], c[2]));
            response.on_hover_text(crate::i18n::tf!("Colour {}: {} (${:03X})", first + i, hex(c), planar::word12(c)));
        }
    });
}

impl Converter {
    /// A picture brought in, named for the files made from it.
    pub fn set_picture(&mut self, name: &str, image: Image) {
        let stem = std::path::Path::new(name).file_stem().map_or_else(|| "picture".to_string(), |s| s.to_string_lossy().into_owned());
        self.name = planar::identifier(&stem.to_lowercase());
        self.sprite_size = (image.width.min(64), image.height.min(64));
        self.bob_size = (image.width.min(128), image.height.min(128));
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
            let _ = sender.send(planar::convert(&image, &settings));
            repaint.request_repaint();
        });
        self.working = Some((self.settings.clone(), receiver));
    }

    /// Remakes the sprites and bob when what they're made from has changed.
    fn make_small(&mut self) {
        let Some((_, image)) = &self.picture else {
            return;
        };
        let transparency = match self.transparent {
            Transparent::Alpha => Transparency::Alpha,
            Transparent::Opaque => Transparency::Opaque,
            Transparent::TopLeft => Transparency::Colour(image.rgba.get(..3).map_or([0, 0, 0], |c| [c[0], c[1], c[2]])),
        };
        let screen_palette: Vec<[u8; 3]> = if self.bob_screen_palette { self.result.as_ref().map(|(_, c)| c.palette.clone()).unwrap_or_default() } else { Vec::new() };
        use std::hash::{Hash, Hasher};
        let mut key = std::collections::hash_map::DefaultHasher::new();
        (self.serial, format!("{:?}{:?}{:?}{:?}{:?}{:?}{:?}{:?}", self.sprite, self.sprite_size, self.transparent, self.bob_settings, self.bob_size, screen_palette, self.settings.dither, self.settings.filter), self.settings.strength.to_bits(), self.settings.chipset == Chipset::Aga).hash(&mut key);
        let key = key.finish();
        if self.made == Some(key) {
            return;
        }
        self.made = Some(key);
        self.shown = None;
        let s = &self.settings;
        let cut = sprites::cut(image, self.sprite_size.0, self.sprite_size.1, s.filter, transparency);
        let chipset = if self.sprite.width == SpriteWidth::W16 { s.chipset } else { Chipset::Aga };
        self.sprites = Some(sprites::make_sprites(&cut, self.sprite, chipset, s.dither, s.strength));
        let cut = sprites::cut(image, self.bob_size.0, self.bob_size.1, s.filter, transparency);
        let bob_settings = if screen_palette.is_empty() { self.bob_settings } else { BobSettings { planes: self.result.as_ref().map_or(self.bob_settings.planes, |(_, c)| c.planes), ..self.bob_settings } };
        self.bob = Some(sprites::make_bob(&cut, bob_settings, &screen_palette, s.chipset, s.dither, s.strength));
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
        };
        self.raw_shown = Some(self.raw.draw(bytes, &palette, self.raw_chipset).1);
    }

    /// The picture shown, at its width and height and with the pixel's width for its height.
    fn preview(&mut self, ui: &mut Ui, width: usize, height: usize, rgba: &[u8], aspect: f32, key: usize) {
        if width == 0 || height == 0 || rgba.len() < width * height * 4 {
            return;
        }
        let options = egui::TextureOptions { magnification: egui::TextureFilter::Nearest, minification: egui::TextureFilter::Linear, ..egui::TextureOptions::default() };
        let image = egui::ColorImage::from_rgba_unmultiplied([width, height], &rgba[..width * height * 4]);
        if self.shown != Some((self.tab, key)) || self.texture.is_none() {
            match &mut self.texture {
                Some(texture) => texture.set(image, options),
                None => self.texture = Some(ui.ctx().load_texture("amiga-planes", image, options)),
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
        // A chequer under see-through pixels.
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
            if ui.button(crate::i18n::tr("Open a picture\u{2026}")).on_hover_text(crate::i18n::tr("PNG, JPEG, IFF ILBM, or any older machine's picture Limoniter reads")).clicked() {
                asked = Some(Asked::OpenPicture);
            }
            match &self.picture {
                Some((name, image)) => ui.label(RichText::new(crate::i18n::tf!("{name}, {}\u{D7}{}", image.width, image.height)).color(crate::theme::text_dim())),
                None => ui.label(RichText::new(crate::i18n::tr("No picture yet")).color(crate::theme::text_dim())),
            };
        });
        ui.horizontal(|ui| {
            ui.selectable_value(&mut self.tab, Tab::Picture, crate::i18n::tr("Screen picture"));
            ui.selectable_value(&mut self.tab, Tab::Sprites, crate::i18n::tr("Sprites"));
            ui.selectable_value(&mut self.tab, Tab::Bob, crate::i18n::tr("Bob"));
            ui.selectable_value(&mut self.tab, Tab::Raw, crate::i18n::tr("Raw planes to picture"));
        });
        ui.separator();
        match self.tab {
            Tab::Picture => self.draw_picture(ui, &mut asked),
            Tab::Sprites | Tab::Bob => self.draw_small(ui, &mut asked),
            Tab::Raw => self.draw_raw(ui, &mut asked),
        }
        asked
    }

    fn draw_picture(&mut self, ui: &mut Ui, asked: &mut Option<Asked>) {
        ui.horizontal_top(|ui| {
            ui.vertical(|ui| {
                ui.set_width(330.0);
                self.picture_settings(ui, asked);
            });
            ui.vertical(|ui| {
                let busy = self.working.is_some();
                if let Some((_, c)) = &self.result {
                    let (w, h, rgba, aspect) = (c.width, c.height, c.rgba.clone(), c.resolution.aspect(c.interlaced));
                    let (palette, error) = (c.palette.clone(), c.error);
                    let used = {
                        let mut seen = vec![false; 256];
                        c.values.iter().for_each(|&v| seen[usize::from(v)] = true);
                        seen.iter().filter(|&&s| s).count()
                    };
                    ui.horizontal(|ui| {
                        ui.label(RichText::new(crate::i18n::tf!("{w}\u{D7}{h}, {} values used, average difference {:.3} (Oklab){}", used, error, if busy { " \u{2014} working\u{2026}" } else { "" })).color(crate::theme::text_dim()));
                        ui.checkbox(&mut self.true_shape, crate::i18n::tr("Pixels their true shape"));
                    });
                    self.preview(ui, w, h, &rgba, aspect, 0);
                    swatches(ui, &palette, 0);
                } else if busy {
                    ui.spinner();
                } else {
                    ui.label(crate::i18n::tr("Open a picture to convert it."));
                }
            });
        });
    }

    fn picture_settings(&mut self, ui: &mut Ui, asked: &mut Option<Asked>) {
        let s = &mut self.settings;
        egui::Grid::new("planar-settings").num_columns(2).spacing([8.0, 4.0]).show(ui, |ui| {
            ui.label(crate::i18n::tr("Size"));
            ui.horizontal(|ui| {
                ui.add(egui::DragValue::new(&mut s.width).range(16..=1280)).on_hover_text(crate::i18n::tr("Pixels across"));
                ui.label(crate::i18n::tr("\u{D7}"));
                ui.add(egui::DragValue::new(&mut s.height).range(1..=1024)).on_hover_text(crate::i18n::tr("Pixels down"));
                crate::fit::menu_button(ui, crate::i18n::tr("\u{25BE}"), |ui| {
                    for (w, h, resolution, laced) in [(320, 256, Resolution::Lores, false), (320, 200, Resolution::Lores, false), (640, 256, Resolution::Hires, false), (640, 512, Resolution::Hires, true), (320, 512, Resolution::Lores, true), (1280, 512, Resolution::SuperHires, true)] {
                        let label = format!("{w}\u{D7}{h} {}{}", resolution.name(), if laced { ", interlaced" } else { "" });
                        if ui.button(crate::i18n::tr(&label)).clicked() {
                            (s.width, s.height, s.resolution, s.interlaced) = (w, h, resolution, laced);
                            ui.close();
                        }
                    }
                });
            });
            ui.end_row();
            combo(ui, "planar-res", "Pixels", &mut s.resolution, &Resolution::ALL, |r| r.name().to_string());
            ui.label(crate::i18n::tr(""));
            ui.checkbox(&mut s.interlaced, crate::i18n::tr("Interlaced"));
            ui.end_row();
            combo(ui, "planar-fit", "Fitting", &mut s.fit, &Fit::ALL, |f| f.name().to_string());
            combo(ui, "planar-filter", "Scaling", &mut s.filter, &Filter::ALL, |f| f.name().to_string());
            combo(ui, "planar-mode", "Colours", &mut s.mode, &Mode::ALL, |m| m.name().to_string());
            ui.label(crate::i18n::tr("Bit planes"));
            ui.add_enabled(s.mode == Mode::Plain, egui::Slider::new(&mut s.planes, 1..=8)).on_hover_text(crate::i18n::tr("OCS shows up to 5 (6 for extra half-brite and HAM), AGA 8"));
            ui.end_row();
            ui.label(crate::i18n::tr("Palette"));
            ui.horizontal(|ui| {
                ui.selectable_value(&mut s.chipset, Chipset::Ocs, crate::i18n::tr("12-bit (OCS/ECS)"));
                ui.selectable_value(&mut s.chipset, Chipset::Aga, crate::i18n::tr("24-bit (AGA)"));
            });
            ui.end_row();
            combo(ui, "planar-dither", "Dithering", &mut s.dither, &Dither::ALL, |d| d.name().to_string());
            ui.label(crate::i18n::tr("Strength"));
            ui.add_enabled(s.dither != Dither::None, egui::Slider::new(&mut s.strength, 0.0..=1.0));
            ui.end_row();
            ui.label(crate::i18n::tr(""));
            ui.checkbox(&mut s.black_background, crate::i18n::tr("Colour 0 black")).on_hover_text(crate::i18n::tr("Keep the background and border black"));
            ui.end_row();
            ui.label(crate::i18n::tr("Colours given"));
            ui.horizontal(|ui| {
                if ui.button(crate::i18n::tr("Palette file\u{2026}")).on_hover_text(crate::i18n::tr("An IFF, JASC or GIMP palette, assembler or C numbers, or raw words or bytes")).clicked() {
                    *asked = Some(Asked::OpenPalette);
                }
                if !s.given.is_empty() && ui.small_button(crate::i18n::tr("\u{D7}")).on_hover_text(crate::i18n::tr("Forget it")).clicked() {
                    s.given.clear();
                    s.keep_given = 0;
                    self.palette_name = None;
                }
            });
            ui.end_row();
            if !s.given.is_empty() {
                ui.label(crate::i18n::tr("Keep"));
                let most = s.given.len().min(256);
                ui.add(egui::Slider::new(&mut s.keep_given, 0..=most).text(crate::i18n::tf!("of {}", s.given.len()))).on_hover_text(crate::i18n::tr("These many of its first colours are kept as they are; the rest are found from the picture"));
                ui.end_row();
            }
        });
        let settled = s.settled();
        if settled.planes != s.planes && s.mode != Mode::Plain {
            s.planes = settled.planes;
        }
        if settled.chipset != s.chipset {
            ui.label(RichText::new(if settled.chipset == Chipset::Aga { crate::i18n::tr("Only AGA shows this: its 24-bit palette is used.") } else { crate::i18n::tr("HAM6 changes 4 bits a level: the 12-bit palette is used.") }).color(crate::theme::text_dim()).size(12.0));
        }
        ui.add_space(6.0);
        egui::Grid::new("planar-out").num_columns(2).spacing([8.0, 4.0]).show(ui, |ui| {
            ui.label(crate::i18n::tr("Name"));
            ui.text_edit_singleline(&mut self.name);
            ui.end_row();
            ui.label(crate::i18n::tr("Planes"));
            ui.horizontal(|ui| {
                ui.selectable_value(&mut self.interleaved, false, crate::i18n::tr("One after another"));
                ui.selectable_value(&mut self.interleaved, true, crate::i18n::tr("Interleaved"));
            });
            ui.end_row();
            combo(ui, "planar-fetch", "Rows rounded to", &mut self.fetch, &Fetch::ALL, |f| f.name().to_string());
        });
        if let Some((_, c)) = &self.result {
            let l = planar::layout(c, self.interleaved, self.fetch);
            ui.label(RichText::new(crate::i18n::tf!("{} bytes a row, {} a plane, {} in all; modulo {}", l.row, l.plane, l.total, l.modulo)).color(crate::theme::text_dim()).size(12.0));
        }
        ui.add_space(4.0);
        let ready = self.result.is_some();
        ui.add_enabled_ui(ready, |ui| {
            ui.horizontal_wrapped(|ui| {
                if ui.button(crate::i18n::tr("Save everything\u{2026}")).on_hover_text(crate::i18n::tr("Into a folder: the planes, the palette as words, copper moves and C, an IFF ILBM and the source")).clicked() {
                    *asked = Some(Asked::SaveAll);
                }
                if ui.button(crate::i18n::tr("Planes\u{2026}")).clicked() {
                    *asked = Some(Asked::SavePlanes);
                }
                crate::fit::menu_button(ui, crate::i18n::tr("Palette"), |ui| {
                    for (label, kind) in [("Raw words\u{2026}", PaletteKind::Words), ("Red, green, blue bytes\u{2026}", PaletteKind::Rgb), ("Copper moves (dc.w)\u{2026}", PaletteKind::Copper), ("C array\u{2026}", PaletteKind::C)] {
                        if ui.button(crate::i18n::tr(label)).clicked() {
                            *asked = Some(Asked::SavePalette(kind));
                            ui.close();
                        }
                    }
                });
                if ui.button(crate::i18n::tr("IFF ILBM\u{2026}")).clicked() {
                    *asked = Some(Asked::SaveIlbm);
                }
                ui.checkbox(&mut self.packed, crate::i18n::tr("Packed"));
                if ui.button(crate::i18n::tr("Source\u{2026}")).on_hover_text(crate::i18n::tr("Assembler: the sizes, a copper list and INCBIN of the planes")).clicked() {
                    *asked = Some(Asked::SaveSource);
                }
                if ui.button(crate::i18n::tr("Copy the copper moves")).clicked()
                    && let Some((_, c)) = &self.result
                {
                    ui.ctx().copy_text(planar::palette_copper(&c.palette, c.chipset));
                }
            });
        });
    }

    fn draw_small(&mut self, ui: &mut Ui, asked: &mut Option<Asked>) {
        self.make_small();
        ui.horizontal_top(|ui| {
            ui.vertical(|ui| {
                ui.set_width(330.0);
                egui::Grid::new("small-settings").num_columns(2).spacing([8.0, 4.0]).show(ui, |ui| {
                    ui.label(crate::i18n::tr("See-through"));
                    egui::ComboBox::from_id_salt("small-clear")
                        .selected_text(match self.transparent {
                            Transparent::Alpha => "Where the picture is",
                            Transparent::TopLeft => "The top left pixel's colour",
                            Transparent::Opaque => "Nowhere",
                        })
                        .show_ui(ui, |ui| {
                            ui.selectable_value(&mut self.transparent, Transparent::Alpha, crate::i18n::tr("Where the picture is"));
                            ui.selectable_value(&mut self.transparent, Transparent::TopLeft, crate::i18n::tr("The top left pixel's colour"));
                            ui.selectable_value(&mut self.transparent, Transparent::Opaque, crate::i18n::tr("Nowhere"));
                        });
                    ui.end_row();
                    if self.tab == Tab::Sprites {
                        ui.label(crate::i18n::tr("Size"));
                        ui.horizontal(|ui| {
                            ui.add(egui::DragValue::new(&mut self.sprite_size.0).range(1..=512));
                            ui.label(crate::i18n::tr("\u{D7}"));
                            ui.add(egui::DragValue::new(&mut self.sprite_size.1).range(1..=511));
                        });
                        ui.end_row();
                        combo(ui, "sprite-width", "Sprite width", &mut self.sprite.width, &SpriteWidth::ALL, |w| w.name().to_string());
                        ui.label(crate::i18n::tr("Colours"));
                        ui.horizontal(|ui| {
                            ui.selectable_value(&mut self.sprite.attached, false, crate::i18n::tr("3 a pair"));
                            ui.selectable_value(&mut self.sprite.attached, true, crate::i18n::tr("15, attached"));
                        });
                        ui.end_row();
                        ui.label(crate::i18n::tr("VSTART"));
                        ui.add(egui::DragValue::new(&mut self.sprite.vstart).range(0..=511).hexadecimal(2, false, true)).on_hover_text(crate::i18n::tr("The screen line of its top: $2C is the top of the usual PAL display"));
                        ui.end_row();
                        ui.label(crate::i18n::tr("HSTART"));
                        ui.add(egui::DragValue::new(&mut self.sprite.hstart).range(0..=511).hexadecimal(2, false, true)).on_hover_text(crate::i18n::tr("The lo-res pixel of its left: $81 is the left of the usual display"));
                        ui.end_row();
                    } else {
                        ui.label(crate::i18n::tr("Size"));
                        ui.horizontal(|ui| {
                            ui.add(egui::DragValue::new(&mut self.bob_size.0).range(1..=1008));
                            ui.label(crate::i18n::tr("\u{D7}"));
                            ui.add(egui::DragValue::new(&mut self.bob_size.1).range(1..=1024));
                        });
                        ui.end_row();
                        ui.label(crate::i18n::tr(""));
                        ui.checkbox(&mut self.bob_screen_palette, crate::i18n::tr("The screen picture's palette")).on_hover_text(crate::i18n::tr("Colours and planes from the screen picture, for a bob drawn on it"));
                        ui.end_row();
                        ui.label(crate::i18n::tr("Bit planes"));
                        ui.add_enabled(!self.bob_screen_palette, egui::Slider::new(&mut self.bob_settings.planes, 1..=8));
                        ui.end_row();
                        ui.label(crate::i18n::tr("Planes"));
                        ui.horizontal(|ui| {
                            ui.selectable_value(&mut self.bob_settings.interleaved, false, crate::i18n::tr("One after another"));
                            ui.selectable_value(&mut self.bob_settings.interleaved, true, crate::i18n::tr("Interleaved"));
                        });
                        ui.end_row();
                        ui.label(crate::i18n::tr(""));
                        ui.checkbox(&mut self.bob_settings.extra_word, crate::i18n::tr("A spare word a row, for shifting"));
                        ui.end_row();
                    }
                    ui.label(crate::i18n::tr("Name"));
                    ui.text_edit_singleline(&mut self.name);
                    ui.end_row();
                    ui.label(crate::i18n::tr("Source"));
                    ui.horizontal(|ui| {
                        ui.selectable_value(&mut self.inline, true, crate::i18n::tr("dc.w"));
                        ui.selectable_value(&mut self.inline, false, crate::i18n::tr("INCBIN"));
                    });
                    ui.end_row();
                });
                ui.label(RichText::new(crate::i18n::tr("Scaling and dithering are the screen picture's.")).color(crate::theme::text_dim()).size(12.0));
                ui.add_space(4.0);
                if self.tab == Tab::Sprites {
                    if let Some(s) = &self.sprites {
                        let note = format!("{} sprite{} {}{}", s.needed(), if s.needed() == 1 { "" } else { "s" }, sprites::sprites_raw(s).len(), " bytes");
                        ui.label(RichText::new(note).color(crate::theme::text_dim()).size(12.0));
                        if s.needed() > 8 {
                            ui.label(RichText::new(crate::i18n::tr("More than the eight sprites there are: narrow it, or reuse sprites further down.")).color(crate::theme::rainbow_mistake()).size(12.0));
                        }
                    }
                    if ui.add_enabled(self.sprites.is_some(), egui::Button::new(crate::i18n::tr("Save the sprites\u{2026}"))).on_hover_text(crate::i18n::tr("The raw sprites and their source")).clicked() {
                        *asked = Some(Asked::SaveSprites);
                    }
                } else {
                    if let Some(b) = &self.bob {
                        let note = match b.bltsize() {
                            Some(size) => crate::i18n::tf!("{} words a row; BLTSIZE ${size:04X}", b.words()),
                            None => crate::i18n::tf!("{} words a row; too big for one BLTSIZE", b.words()),
                        };
                        ui.label(RichText::new(note).color(crate::theme::text_dim()).size(12.0));
                    }
                    if ui.add_enabled(self.bob.is_some(), egui::Button::new(crate::i18n::tr("Save the bob\u{2026}"))).on_hover_text(crate::i18n::tr("Its planes, its mask and their source")).clicked() {
                        *asked = Some(Asked::SaveBob);
                    }
                }
            });
            ui.vertical(|ui| {
                let shown = if self.tab == Tab::Sprites { self.sprites.as_ref().map(|s| (s.width, s.height, s.rgba.clone(), s.palette.clone(), 16)) } else { self.bob.as_ref().map(|b| (b.width, b.height, b.rgba.clone(), b.palette.clone(), 0)) };
                match shown {
                    Some((w, h, rgba, palette, first)) => {
                        let aspect = self.settings.resolution.aspect(self.settings.interlaced);
                        let aspect = if self.tab == Tab::Sprites { if self.settings.interlaced { 2.0 } else { 1.0 } } else { aspect };
                        self.preview(ui, w, h, &rgba, aspect, self.made.unwrap_or(0) as usize);
                        swatches(ui, &palette, first);
                    }
                    None => {
                        ui.label(crate::i18n::tr("Open a picture to make them from it."));
                    }
                }
            });
        });
    }

    fn draw_raw(&mut self, ui: &mut Ui, asked: &mut Option<Asked>) {
        let before = (self.raw.clone(), self.raw_colours, self.raw_chipset);
        ui.horizontal_top(|ui| {
            ui.vertical(|ui| {
                ui.set_width(330.0);
                ui.horizontal(|ui| {
                    if ui.button(crate::i18n::tr("Open raw planes\u{2026}")).clicked() {
                        *asked = Some(Asked::OpenRaw);
                    }
                    if let Some((name, bytes)) = &self.raw_file {
                        ui.label(RichText::new(crate::i18n::tf!("{name}, {} bytes", bytes.len())).color(crate::theme::text_dim()));
                    }
                });
                let r = &mut self.raw;
                egui::Grid::new("raw-settings").num_columns(2).spacing([8.0, 4.0]).show(ui, |ui| {
                    ui.label(crate::i18n::tr("Starting at"));
                    ui.add(egui::DragValue::new(&mut r.offset).range(0..=usize::MAX).hexadecimal(1, false, true));
                    ui.end_row();
                    ui.label(crate::i18n::tr("Size"));
                    ui.horizontal(|ui| {
                        ui.add(egui::DragValue::new(&mut r.width).range(16..=2048).speed(16.0));
                        ui.label(crate::i18n::tr("\u{D7}"));
                        ui.add(egui::DragValue::new(&mut r.height).range(1..=2048));
                    });
                    ui.end_row();
                    combo(ui, "raw-mode", "Colours", &mut r.mode, &Mode::ALL, |m| m.name().to_string());
                    ui.label(crate::i18n::tr("Bit planes"));
                    ui.add_enabled(r.mode == Mode::Plain, egui::Slider::new(&mut r.planes, 1..=8));
                    ui.end_row();
                    if let Some(planes) = r.mode.planes() {
                        r.planes = planes;
                    }
                    ui.label(crate::i18n::tr("Planes"));
                    ui.horizontal(|ui| {
                        ui.selectable_value(&mut r.interleaved, false, crate::i18n::tr("One after another"));
                        ui.selectable_value(&mut r.interleaved, true, crate::i18n::tr("Interleaved"));
                    });
                    ui.end_row();
                    combo(ui, "raw-fetch", "Rows rounded to", &mut r.fetch, &Fetch::ALL, |f| f.name().to_string());
                    ui.label(crate::i18n::tr("Palette"));
                    egui::ComboBox::from_id_salt("raw-colours")
                        .selected_text(match self.raw_colours {
                            RawColours::Greys => "Shades of grey",
                            RawColours::Converted => "The screen picture's",
                            RawColours::Given => "The palette file's",
                        })
                        .show_ui(ui, |ui| {
                            ui.selectable_value(&mut self.raw_colours, RawColours::Greys, crate::i18n::tr("Shades of grey"));
                            ui.selectable_value(&mut self.raw_colours, RawColours::Converted, crate::i18n::tr("The screen picture's"));
                            ui.selectable_value(&mut self.raw_colours, RawColours::Given, crate::i18n::tr("The palette file's"));
                        });
                    ui.end_row();
                    ui.label(crate::i18n::tr("Half-brite as"));
                    ui.horizontal(|ui| {
                        ui.selectable_value(&mut self.raw_chipset, Chipset::Ocs, crate::i18n::tr("OCS"));
                        ui.selectable_value(&mut self.raw_chipset, Chipset::Aga, crate::i18n::tr("AGA"));
                    });
                    ui.end_row();
                });
                if let Some((_, bytes)) = &self.raw_file {
                    let rows = self.raw.rows_in(bytes.len());
                    ui.horizontal(|ui| {
                        ui.label(RichText::new(crate::i18n::tf!("{rows} rows fit")).color(crate::theme::text_dim()).size(12.0));
                        if ui.small_button(crate::i18n::tr("Use them")).clicked() && rows > 0 {
                            self.raw.height = rows;
                        }
                    });
                }
                if ui.button(crate::i18n::tr("Palette file\u{2026}")).clicked() {
                    *asked = Some(Asked::OpenPalette);
                }
                if ui.add_enabled(self.raw_shown.is_some(), egui::Button::new(crate::i18n::tr("Save as PNG\u{2026}"))).clicked() {
                    *asked = Some(Asked::SavePng);
                }
            });
            ui.vertical(|ui| {
                if (self.raw.clone(), self.raw_colours, self.raw_chipset) != before || self.raw_shown.is_none() {
                    self.redraw_raw();
                    self.shown = None;
                }
                if let Some(rgba) = self.raw_shown.clone() {
                    let (w, h) = (self.raw.width.clamp(1, 4096), self.raw.height.clamp(1, 4096));
                    self.preview(ui, w, h, &rgba, 1.0, 1);
                } else {
                    ui.label(crate::i18n::tr("Open a file of raw planes, ripped from a program or saved here, to see them as a picture."));
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
