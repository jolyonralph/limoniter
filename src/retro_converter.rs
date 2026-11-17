//! The 8-bit picture converter's window: a picture brought in and made into a ZX Spectrum, Timex hi-colour,
//! Spectrum Next, Amstrad CPC, MSX or BBC Micro screen, shown as the machine would show it, with its colours, how
//! it's scaled and dithered, and its brightness, contrast, saturation and gamma to set. The work is in
//! `retro_convert`; the window's host does the files.

use crate::c64_converter::{Job, combo};
use crate::quantise::{Dither, Filter, Fit, Image, identifier};
use crate::retro_convert::{self as convert, Bright, Machine, Mode, Screen, Settings};
use eframe::egui::{self, Color32, RichText, Ui, vec2};
use std::sync::Arc;

/// What the window asks its host to do.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Asked {
    OpenPicture,
    /// The file the machine loads: a .scr, .nxi, .sc2 or screen dump.
    SaveScreen,
    SavePng,
    SaveAll,
    /// The Spectrum screen, into the screen designer to work on (or a Next screen, into the Layer 2 painter).
    ToDesigner,
    /// A Next program that shows it.
    SaveNex,
}

/// The CPC's firmware colour names, by number.
const CPC_NAMES: [&str; 27] = [
    "Black", "Blue", "Bright blue", "Red", "Magenta", "Mauve", "Bright red", "Purple", "Bright magenta", "Green", "Cyan", "Sky blue", "Yellow", "White", "Pastel blue", "Orange", "Pink", "Pastel magenta", "Bright green", "Sea green", "Bright cyan", "Lime", "Pastel green", "Pastel cyan", "Bright yellow", "Pastel yellow", "Bright white",
];

const BBC_NAMES: [&str; 8] = ["Black", "Red", "Green", "Yellow", "Blue", "Magenta", "Cyan", "White"];

pub struct Converter {
    pub picture: Option<(String, Arc<Image>)>,
    pub name: String,
    pub settings: Settings,
    job: Job<Settings, Screen>,
    texture: Option<egui::TextureHandle>,
    shown: Option<u64>,
    serial: u64,
    pub true_shape: bool,
}

impl Default for Converter {
    fn default() -> Self {
        Converter { picture: None, name: "picture".into(), settings: Settings::default(), job: Job::new(), texture: None, shown: None, serial: 0, true_shape: true }
    }
}

fn dim(text: impl Into<String>) -> RichText {
    RichText::new(text.into()).color(crate::theme::text_dim()).size(12.0)
}

impl Converter {
    /// A picture brought in, named for the files made from it.
    pub fn set_picture(&mut self, name: &str, image: Image) {
        let stem = std::path::Path::new(name).file_stem().map_or_else(|| "picture".to_string(), |s| s.to_string_lossy().into_owned());
        self.name = identifier(&stem.to_lowercase());
        self.picture = Some((name.to_string(), Arc::new(image)));
        self.job.reset();
        self.serial += 1;
    }

    pub fn screen(&self) -> Option<&Screen> {
        self.job.result()
    }

    /// Whether the screen shown is the one for the settings now (not one still being made).
    pub fn up_to_date(&self) -> bool {
        self.job.is_for(&self.settings)
    }

    /// Whether a picture is being converted.
    pub fn working(&self) -> bool {
        self.picture.is_some() && !self.up_to_date()
    }

    pub fn draw(&mut self, ui: &mut Ui) -> Option<Asked> {
        let mut asked = None;
        let image = self.picture.as_ref().map(|p| &p.1);
        if self.job.keep_up(ui.ctx(), &self.settings, image, convert::convert) {
            self.serial += 1;
        }
        ui.horizontal(|ui| {
            if ui.button(crate::i18n::tr("Open a picture\u{2026}")).on_hover_text(crate::i18n::tr("JPEG, PNG, IFF ILBM, or any older machine's picture Limoniter reads")).clicked() {
                asked = Some(Asked::OpenPicture);
            }
            match &self.picture {
                Some((name, image)) => ui.label(RichText::new(crate::i18n::tf!("{name}, {}\u{D7}{}", image.width, image.height)).color(crate::theme::text_dim())),
                None => ui.label(RichText::new(crate::i18n::tr("No picture yet")).color(crate::theme::text_dim())),
            };
        });
        ui.horizontal(|ui| {
            let machine = self.settings.mode.machine();
            for m in Machine::ALL {
                if ui.selectable_label(machine == m, crate::i18n::tr(m.name())).clicked() && machine != m {
                    self.settings.mode = m.modes()[0];
                }
            }
        });
        ui.separator();
        ui.horizontal_top(|ui| {
            ui.vertical(|ui| {
                ui.set_width(330.0);
                self.settings_ui(ui, &mut asked);
            });
            ui.vertical(|ui| self.result_view(ui));
        });
        asked
    }

    fn settings_ui(&mut self, ui: &mut Ui, asked: &mut Option<Asked>) {
        let s = &mut self.settings;
        egui::Grid::new("retro-settings").num_columns(2).spacing([8.0, 4.0]).show(ui, |ui| {
            let modes = s.mode.machine().modes();
            combo(ui, "retro-mode", "Mode", &mut s.mode, modes, |m| m.name().to_string());
            if s.mode == Mode::Sprites {
                let sp = &mut s.sprites;
                ui.label(crate::i18n::tr("Sprites"));
                ui.horizontal(|ui| {
                    ui.add(egui::DragValue::new(&mut sp.across).range(1..=16)).on_hover_text(crate::i18n::tr("Across"));
                    ui.label(crate::i18n::tr("\u{D7}"));
                    let most = if sp.four_bit { 128 } else { 64 } / sp.across.max(1);
                    ui.add(egui::DragValue::new(&mut sp.down).range(1..=most.max(1))).on_hover_text(crate::i18n::tr("Down"));
                    ui.label(dim("of 16\u{D7}16"));
                });
                ui.end_row();
                ui.label(crate::i18n::tr("From"));
                ui.horizontal(|ui| {
                    ui.add(egui::DragValue::new(&mut sp.x).range(0..=4096).prefix("x ")).on_hover_text(crate::i18n::tr("Where the first sprite's top left corner is in the picture"));
                    ui.add(egui::DragValue::new(&mut sp.y).range(0..=4096).prefix("y "));
                });
                ui.end_row();
                ui.label(crate::i18n::tr("Pixels"));
                ui.horizontal(|ui| {
                    ui.selectable_value(&mut sp.four_bit, false, crate::i18n::tr("8 bits")).on_hover_text(crate::i18n::tr("Up to 64 sprites, in 256 colours"));
                    ui.selectable_value(&mut sp.four_bit, true, crate::i18n::tr("4 bits")).on_hover_text(crate::i18n::tr("Up to 128 sprites, in 16 colours"));
                });
                ui.end_row();
                if !sp.four_bit {
                    ui.label(crate::i18n::tr("Palette"));
                    ui.horizontal(|ui| {
                        ui.selectable_value(&mut sp.own_palette, false, crate::i18n::tr("The Next's")).on_hover_text(crate::i18n::tr("As the Next has it at reset: nothing to set up, and the sprite editor draws in it"));
                        ui.selectable_value(&mut sp.own_palette, true, crate::i18n::tr("Found from the picture"));
                    });
                    ui.end_row();
                }
                combo(ui, "retro-clear", "See-through", &mut sp.see_through, &crate::next_tiles::SeeThrough::ALL, |c| c.name().to_string());
            } else {
                combo(ui, "retro-fit", "Fitting", &mut s.fit, &Fit::ALL, |f| f.name().to_string());
                combo(ui, "retro-filter", "Scaling", &mut s.filter, &Filter::ALL, |f| f.name().to_string());
            }
            if matches!(s.mode, Mode::Tiles | Mode::TilesWide) {
                ui.label(crate::i18n::tr("Tiles"));
                ui.add(egui::Slider::new(&mut s.tiles.most, 1..=256).text(crate::i18n::tr("at most"))).on_hover_text(crate::i18n::tr("The most alike are made one until there are no more than this"));
                ui.end_row();
                ui.label(crate::i18n::tr("Palettes"));
                ui.add(egui::Slider::new(&mut s.tiles.palettes, 1..=16)).on_hover_text(crate::i18n::tr("Each place shows its tile in one of these, of 15 colours (the 16th see-through)"));
                ui.end_row();
                ui.label(crate::i18n::tr(""));
                ui.checkbox(&mut s.tiles.turns, crate::i18n::tr("Turned and mirrored tiles shared")).on_hover_text(crate::i18n::tr("A tile that's another turned or mirrored is kept once, the place's attribute saying how it's shown"));
                ui.end_row();
            }
            combo(ui, "retro-dither", "Dithering", &mut s.dither, &Dither::ALL, |d| d.name().to_string());
            ui.label(crate::i18n::tr("Strength"));
            ui.add_enabled(s.dither != Dither::None, egui::Slider::new(&mut s.strength, 0.0..=1.0));
            ui.end_row();
            if matches!(s.mode, Mode::Spectrum | Mode::Timex) {
                combo(ui, "retro-bright", "BRIGHT", &mut s.bright, &Bright::ALL, |b| b.name().to_string());
            }
            if s.mode.next_mode().is_some_and(|m| m.colours() == 256) {
                ui.label(crate::i18n::tr("Colours"));
                ui.add(egui::Slider::new(&mut s.palette_size, 2..=256)).on_hover_text(crate::i18n::tr("How many of the palette's 256 places the picture takes; the rest are left as the Next has them at reset, for sprites and the other layers"));
                ui.end_row();
            }
            if !matches!(s.mode, Mode::Spectrum | Mode::Timex | Mode::TimexHires | Mode::Msx2 | Mode::Bbc2 | Mode::Sprites) {
                ui.label(crate::i18n::tr(""));
                ui.checkbox(&mut s.black_first, crate::i18n::tr("Colour 0 black")).on_hover_text(crate::i18n::tr("The first colour kept black, for the border and what's cleared"));
                ui.end_row();
            }
            ui.label(crate::i18n::tr("Brightness"));
            ui.add(egui::Slider::new(&mut s.brightness, -1.0..=1.0));
            ui.end_row();
            ui.label(crate::i18n::tr("Contrast"));
            ui.add(egui::Slider::new(&mut s.contrast, -1.0..=1.0));
            ui.end_row();
            ui.label(crate::i18n::tr("Saturation"));
            ui.add(egui::Slider::new(&mut s.saturation, -1.0..=1.0));
            ui.end_row();
            ui.label(crate::i18n::tr("Gamma"));
            ui.add(egui::Slider::new(&mut s.gamma, 0.5..=2.0));
            ui.end_row();
            ui.label(crate::i18n::tr(""));
            if ui.small_button(crate::i18n::tr("As the picture is")).on_hover_text(crate::i18n::tr("Brightness, contrast, saturation and gamma put back")).clicked() {
                (s.brightness, s.contrast, s.saturation, s.gamma) = (0.0, 0.0, 0.0, 1.0);
            }
            ui.end_row();
            ui.label(crate::i18n::tr("Name"));
            ui.text_edit_singleline(&mut self.name);
            ui.end_row();
        });
        ui.checkbox(&mut self.true_shape, crate::i18n::tr("Pixels their true shape"));
        let Some(screen) = self.job.result() else { return };
        // The colours chosen, by the machine's numbers, for its INK or VDU 19 commands.
        if let Some(t) = &screen.tilemap {
            let shared = if t.different > t.tiles.len() { format!("; {} different places, the most alike made one", t.different) } else { String::new() };
            ui.label(dim(format!("{} tiles in {} palettes{shared}", t.tiles.len(), t.palettes)));
        }
        if let Some(sp) = &screen.sprites {
            ui.label(dim(format!("{} sprites, {} bytes of patterns", sp.patterns.len(), sp.pattern_bytes().len())));
        }
        if screen.mode == Mode::TimexHires {
            let (ink, paper) = screen.cells[0];
            ui.label(dim(format!("INK {} on PAPER {} (BRIGHT): OUT 255,{}", ink & 7, paper & 7, 6 | (ink & 7) << 3)));
        }
        if !screen.palette.is_empty() && screen.palette.len() <= 16 {
            ui.add_space(4.0);
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing = vec2(4.0, 2.0);
                let gamut = screen.mode.gamut();
                for (i, &c) in screen.palette.iter().enumerate() {
                    let rgb = gamut[usize::from(c)];
                    let (rect, response) = ui.allocate_exact_size(vec2(14.0, 14.0), egui::Sense::hover());
                    ui.painter().rect_filled(rect, 1.0, Color32::from_rgb(rgb[0], rgb[1], rgb[2]));
                    let name = match screen.mode.machine() {
                        Machine::Cpc => format!("INK {i},{c}: {}", CPC_NAMES[usize::from(c)]),
                        Machine::Next => format!("Colour {i}: {:03b} {:03b} {:03b} (red, green, blue)", c >> 6, (c >> 3) & 7, c & 7),
                        _ => format!("VDU 19,{i},{c};0;: {}", BBC_NAMES[usize::from(c) & 7]),
                    };
                    response.on_hover_text(name);
                }
            });
        }
        ui.add_space(4.0);
        let (main, tip) = match screen.mode {
            Mode::Spectrum => ("Save as .scr\u{2026}", "6912 bytes: the screen as it is in memory"),
            Mode::Timex => ("Save as .scr\u{2026}", "12288 bytes: the bitmap at 16384, its colours at 24576"),
            Mode::TimexHires => ("Save as .scr\u{2026}", "12289 bytes: the even bytes of each row at 16384, the odd at 24576, then the value for port 255"),
            Mode::Layer2 | Mode::Layer2Wide | Mode::Layer2Hi | Mode::LoRes => ("Save as .nxi\u{2026}", "Its palette (512 bytes), then the pixels as the Next keeps them"),
            Mode::Tiles | Mode::TilesWide => ("Tiles (.til)\u{2026}", "32 bytes a tile; Save everything for the map and palette too"),
            Mode::Sprites => ("Patterns (.spr)\u{2026}", "As the Next's sprite memory takes them; Save everything for the palette too"),
            Mode::Cpc0 | Mode::Cpc1 | Mode::Cpc2 => ("Save as .scr\u{2026}", "16K from &C000, with AMSDOS's header: LOAD\"name.SCR\",&C000"),
            Mode::Msx2 => ("Save as .sc2\u{2026}", "BLOAD\"name.SC2\",S"),
            _ => ("Save the screen\u{2026}", "The screen's memory: *LOAD name 3000 (or 5800 in MODE 4 and 5)"),
        };
        ui.horizontal_wrapped(|ui| {
            if ui.button(crate::i18n::tr("Save everything\u{2026}")).on_hover_text(crate::i18n::tr("Into a folder: the screen, a loader or source to show it, and a PNG")).clicked() {
                *asked = Some(Asked::SaveAll);
            }
            if ui.button(main).on_hover_text(crate::i18n::tr(&*tip)).clicked() {
                *asked = Some(Asked::SaveScreen);
            }
            if ui.button(crate::i18n::tr("PNG\u{2026}")).clicked() {
                *asked = Some(Asked::SavePng);
            }
            if screen.mode.machine() == Machine::Next && ui.button(crate::i18n::tr("Next program (.nex)\u{2026}")).on_hover_text(crate::i18n::tr("Shows the picture: made from the source Save everything writes, which sjasmplus assembles the same")).clicked() {
                *asked = Some(Asked::SaveNex);
            }
            if screen.mode == Mode::Spectrum && ui.button(crate::i18n::tr("Edit in the screen designer")).on_hover_text(crate::i18n::tr("To draw on it, cell by cell")).clicked() {
                *asked = Some(Asked::ToDesigner);
            }
            if screen.sprites.as_ref().is_some_and(|sp| !sp.four_bit && !sp.own_palette) && ui.button(crate::i18n::tr("Edit in the sprite editor")).on_hover_text(crate::i18n::tr("The character and sprite editor, which draws in the Next's own palette")).clicked() {
                *asked = Some(Asked::ToDesigner);
            }
            if screen.mode.next_mode().is_some() && ui.button(crate::i18n::tr("Edit in the Layer 2 painter")).on_hover_text(crate::i18n::tr("To draw on it, and change its palette")).clicked() {
                *asked = Some(Asked::ToDesigner);
            }
        });
    }

    fn result_view(&mut self, ui: &mut Ui) {
        let Some(screen) = self.job.result() else {
            ui.label(dim(if self.picture.is_some() { "Converting\u{2026}" } else { "Open a picture to convert it." }));
            return;
        };
        let (width, height, aspect) = (screen.width, screen.height, screen.mode.aspect());
        let options = egui::TextureOptions { magnification: egui::TextureFilter::Nearest, minification: egui::TextureFilter::Linear, ..egui::TextureOptions::default() };
        if self.shown != Some(self.serial) || self.texture.is_none() {
            let image = egui::ColorImage::from_rgba_unmultiplied([width, height], &screen.rgba());
            match &mut self.texture {
                Some(texture) => texture.set(image, options),
                None => self.texture = Some(ui.ctx().load_texture("retro-picture", image, options)),
            }
            self.shown = Some(self.serial);
        }
        let Some(texture) = &self.texture else { return };
        let aspect = if self.true_shape { aspect } else { 1.0 };
        let shown = vec2(width as f32 * aspect, height as f32);
        let room = vec2(ui.available_width().max(200.0), 560.0);
        let fit = (room.x / shown.x).min(room.y / shown.y);
        let scale = if fit >= 2.0 { fit.floor().min(4.0) } else { fit };
        let (rect, _) = ui.allocate_exact_size(shown * scale, egui::Sense::hover());
        let painter = ui.painter_at(rect);
        let sprites = screen.sprites.is_some();
        if sprites {
            // What's see-through shown as a checkerboard.
            let step = 4.0 * scale;
            for row in 0..(rect.height() / step).ceil() as usize {
                for column in 0..(rect.width() / step).ceil() as usize {
                    let grey = if (row + column) % 2 == 0 { 90 } else { 130 };
                    let min = rect.min + vec2(column as f32 * step, row as f32 * step);
                    painter.rect_filled(egui::Rect::from_min_size(min, vec2(step, step)).intersect(rect), 0.0, Color32::from_gray(grey));
                }
            }
        }
        painter.image(texture.id(), rect, egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)), Color32::WHITE);
        if sprites {
            // Each sprite's edges.
            let stroke = egui::Stroke::new(1.0, crate::theme::accent());
            let cell = 16.0 * scale;
            for i in 0..=(width / 16) {
                let x = rect.min.x + i as f32 * cell;
                painter.line_segment([egui::pos2(x, rect.min.y), egui::pos2(x, rect.max.y)], stroke);
            }
            for i in 0..=(height / 16) {
                let y = rect.min.y + i as f32 * cell;
                painter.line_segment([egui::pos2(rect.min.x, y), egui::pos2(rect.max.x, y)], stroke);
            }
        }
        ui.label(dim(format!("{} \u{B7} {width}\u{D7}{height}", screen.mode.name())));
    }
}
