//! The ripper's window: the music found in a file (and in the files inside it, if it's an image), each to open,
//! hear and save; and the file's bytes seen as each machine's pictures, to look through for graphics, save, copy
//! as source, or send to the editor for that kind of picture.

use crate::ripper::{self, Found, Layout, View};
use eframe::egui::{self, ColorImage, RichText, TextureHandle, TextureOptions, Ui, vec2};
use std::sync::Arc;
use std::sync::mpsc::Receiver;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Tab {
    Music,
    Graphics,
}

/// Where graphics can be sent.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Target {
    Characters,
    C64Characters,
    C64MultiCharacters,
    C64Sprites,
    C64MultiSprites,
    NextSprites,
    SpectrumScreen,
}

impl Target {
    pub fn name(self) -> &'static str {
        match self {
            Target::Characters => "Open as Spectrum UDGs or a font",
            Target::C64Characters | Target::C64MultiCharacters => "Open as a C64 charset",
            Target::C64Sprites | Target::C64MultiSprites => "Open as C64 sprites",
            Target::NextSprites => "Open as Next sprites",
            Target::SpectrumScreen => "Open in the screen designer",
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Asked {
    UseFront,
    OpenFile,
    OpenFind(usize),
    SaveFind(usize),
    SaveAll,
    SavePicture,
    CopySource,
    Send(Target),
    Close,
}

pub struct Ripper {
    pub tab: Tab,
    /// The file being looked through, and its name.
    pub name: String,
    pub bytes: Arc<Vec<u8>>,
    pub finds: Vec<Found>,
    /// The search, while it runs.
    searching: Option<Receiver<Vec<Found>>>,
    pub view: View,
    pub zoom: usize,
    texture: Option<TextureHandle>,
    shown: Option<View>,
}

impl Default for Ripper {
    fn default() -> Ripper {
        Ripper {
            tab: Tab::Music,
            name: String::new(),
            bytes: Arc::new(Vec::new()),
            finds: Vec::new(),
            searching: None,
            view: View { layout: Layout::Characters, offset: 0, across: 16, down: 16, planes: 4 },
            zoom: 2,
            texture: None,
            shown: None,
        }
    }
}

impl Ripper {
    /// A file to look through: its music searched for at once, on its own thread.
    pub fn look_at(&mut self, name: String, bytes: Vec<u8>) {
        // A Spectrum snapshot is looked through as the memory it holds, each byte at its address.
        let lower = name.to_ascii_lowercase();
        let (name, bytes) = match crate::media::spectrum::snapshot_memory(&bytes, lower.ends_with(".sna")).filter(|_| lower.ends_with(".z80") || lower.ends_with(".sna")) {
            Some(memory) => (format!("{name} (its memory)"), memory),
            None => (name, bytes),
        };
        self.name = name.clone();
        self.bytes = Arc::new(bytes);
        self.finds.clear();
        self.view.offset = 0;
        self.shown = None;
        let (send, receive) = std::sync::mpsc::channel();
        let bytes = self.bytes.clone();
        std::thread::spawn(move || {
            let _ = send.send(ripper::find_music_everywhere(&bytes, &name));
        });
        self.searching = Some(receive);
    }

    /// Whether it's still looking.
    pub fn searching(&mut self) -> bool {
        if let Some(receive) = &self.searching {
            match receive.try_recv() {
                Ok(finds) => {
                    self.finds = finds;
                    self.searching = None;
                }
                Err(std::sync::mpsc::TryRecvError::Disconnected) => self.searching = None,
                Err(std::sync::mpsc::TryRecvError::Empty) => {}
            }
        }
        self.searching.is_some()
    }

    /// Waits for the search, for tests.
    #[cfg(test)]
    pub fn wait(&mut self) {
        while self.searching() {
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
    }

    /// Where the view's graphics can go.
    pub fn targets(&self) -> Vec<Target> {
        match self.view.layout {
            Layout::Characters => vec![Target::Characters, Target::C64Characters],
            Layout::C64MultiCharacters => vec![Target::C64MultiCharacters],
            Layout::C64Sprites => vec![Target::C64Sprites],
            Layout::C64MultiSprites => vec![Target::C64MultiSprites],
            Layout::Bytes if self.view.across == 16 => vec![Target::NextSprites],
            Layout::Spectrum => vec![Target::SpectrumScreen],
            _ => Vec::new(),
        }
    }

    /// The bytes the view shows.
    pub fn view_bytes(&self) -> Vec<u8> {
        let used = ripper::picture(&self.bytes, &self.view).bytes;
        let from = self.view.offset.min(self.bytes.len());
        self.bytes[from..(from + used).min(self.bytes.len())].to_vec()
    }

    pub fn draw(&mut self, ui: &mut Ui) -> Option<Asked> {
        let mut asked = None;
        ui.horizontal(|ui| {
            if ui.button(crate::i18n::tr("The file in front")).on_hover_text(crate::i18n::tr("Look through the file in front, and the files inside it if it's a disk, tape or archive")).clicked() {
                asked = Some(Asked::UseFront);
            }
            if ui.button(crate::i18n::tr("Open a file\u{2026}")).clicked() {
                asked = Some(Asked::OpenFile);
            }
            if !self.name.is_empty() {
                ui.label(RichText::new(crate::i18n::tf!("{}: {} bytes", self.name, self.bytes.len())).strong());
            }
        });
        ui.horizontal(|ui| {
            ui.selectable_value(&mut self.tab, Tab::Music, crate::i18n::tf!("Music ({})", self.finds.len()));
            ui.selectable_value(&mut self.tab, Tab::Graphics, crate::i18n::tr("Graphics"));
        });
        ui.separator();
        match self.tab {
            Tab::Music => {
                if let Some(a) = self.music(ui) {
                    asked = Some(a);
                }
            }
            Tab::Graphics => {
                if let Some(a) = self.graphics(ui) {
                    asked = Some(a);
                }
            }
        }
        asked
    }

    fn music(&mut self, ui: &mut Ui) -> Option<Asked> {
        let mut asked = None;
        if self.searching() {
            ui.horizontal(|ui| {
                ui.spinner();
                ui.label(crate::i18n::tr("Looking for music\u{2026}"));
            });
            ui.ctx().request_repaint_after(std::time::Duration::from_millis(100));
            return None;
        }
        if self.name.is_empty() {
            ui.label(crate::i18n::tr("Choose a file to look through: a game, a demo, a disk, tape or archive image, or a memory snapshot."));
            return None;
        }
        if self.finds.is_empty() {
            ui.label(crate::i18n::tr("No music found: no tracker modules (ProTracker and its kin, Scream Tracker 3, FastTracker 2, Impulse Tracker), SID, SNDH, YM, PT3 or AY files."));
            return None;
        }
        egui::ScrollArea::vertical().id_salt("rip-music").max_height(380.0).show(ui, |ui| {
            egui::Grid::new("rip-finds").striped(true).num_columns(6).spacing(vec2(10.0, 4.0)).show(ui, |ui| {
                for heading in ["Where", "At", "What", "Title", "Bytes", ""] {
                    ui.label(RichText::new(heading).strong());
                }
                ui.end_row();
                for (i, f) in self.finds.iter().enumerate() {
                    ui.label(&f.place);
                    ui.label(RichText::new(crate::i18n::tf!("${:X}", f.offset)).monospace());
                    ui.label(crate::i18n::tr(f.kind.name()));
                    ui.label(&f.title);
                    ui.label(crate::i18n::tf!("{}{}", f.bytes.len(), if f.sized { "" } else { " or less" })).on_hover_text(if f.sized { crate::i18n::tr("As long as its header says") } else { crate::i18n::tr("Its header doesn't say how long it is: this runs to the next find or the file's end") });
                    ui.horizontal(|ui| {
                        if ui.small_button(crate::i18n::tr("Open")).on_hover_text(crate::i18n::tr("In a tab of its own, to hear and look at")).clicked() {
                            asked = Some(Asked::OpenFind(i));
                        }
                        if ui.small_button(crate::i18n::tr("Save\u{2026}")).clicked() {
                            asked = Some(Asked::SaveFind(i));
                        }
                    });
                    ui.end_row();
                }
            });
        });
        if ui.button(crate::i18n::tr("Save them all\u{2026}")).on_hover_text(crate::i18n::tr("Every one, into a folder")).clicked() {
            asked = Some(Asked::SaveAll);
        }
        asked
    }

    fn graphics(&mut self, ui: &mut Ui) -> Option<Asked> {
        let mut asked = None;
        let length = self.bytes.len();
        let v = &mut self.view;
        ui.horizontal_wrapped(|ui| {
            let before = v.layout;
            egui::ComboBox::from_id_salt("rip-layout").width(210.0).selected_text(crate::i18n::tr(v.layout.name())).show_ui(ui, |ui| {
                for l in Layout::ALL {
                    ui.selectable_value(&mut v.layout, l, crate::i18n::tr(l.name()));
                }
            });
            if v.layout != before {
                v.across = v.layout.usual_across();
                v.down = match v.layout {
                    Layout::Bits | Layout::C64Multi | Layout::Bytes => 128,
                    Layout::Amiga | Layout::AtariSt => 200,
                    Layout::C64Sprites | Layout::C64MultiSprites => 6,
                    _ => 16,
                };
            }
            if v.layout != Layout::Spectrum {
                ui.add(egui::DragValue::new(&mut v.across).range(1..=256)).on_hover_text(v.layout.unit());
                ui.label(v.layout.unit());
                ui.add(egui::DragValue::new(&mut v.down).range(1..=512));
                ui.label(crate::i18n::tr("down"));
            }
            if matches!(v.layout, Layout::Amiga | Layout::AtariSt) {
                ui.add(egui::DragValue::new(&mut v.planes).range(1..=if v.layout == Layout::AtariSt { 4 } else { 8 }));
                ui.label(crate::i18n::tr("planes"));
            }
            ui.label(crate::i18n::tr("Zoom"));
            ui.add(egui::DragValue::new(&mut self.zoom).range(1..=8));
        });
        let picture = ripper::picture(&self.bytes, &self.view);
        let step_row = (picture.bytes / self.view.down.max(1)).max(1);
        ui.horizontal(|ui| {
            ui.label(crate::i18n::tr("From"));
            ui.add(egui::DragValue::new(&mut self.view.offset).range(0..=length.saturating_sub(1)).hexadecimal(6, false, true).prefix("$"));
            for (label, by, tip) in [("\u{23EA}", -(picture.bytes as i64), "Back a screenful"), ("\u{25C0}", -(step_row as i64), "Back a row"), ("-1", -1, "Back a byte"), ("+1", 1, "On a byte"), ("\u{25B6}", step_row as i64, "On a row"), ("\u{23E9}", picture.bytes as i64, "On a screenful")] {
                if ui.small_button(crate::i18n::tr(label)).on_hover_text(crate::i18n::tr(tip)).clicked() {
                    self.view.offset = (self.view.offset as i64 + by).clamp(0, length.saturating_sub(1) as i64) as usize;
                }
            }
            ui.label(RichText::new(crate::i18n::tf!("{} bytes shown", picture.bytes)).size(11.0).color(crate::theme::text_dim()));
        });
        let mut through = self.view.offset as f64;
        if length > 1 && ui.add(egui::Slider::new(&mut through, 0.0..=(length - 1) as f64).show_value(false)).changed() {
            self.view.offset = through as usize;
        }
        // The picture, redrawn when the view changes.
        if self.shown != Some(self.view) || self.texture.is_none() {
            let image = ColorImage::from_rgba_unmultiplied([picture.width.max(1), picture.height.max(1)], &picture.rgba);
            match &mut self.texture {
                Some(t) => t.set(image, TextureOptions::NEAREST),
                None => self.texture = Some(ui.ctx().load_texture("ripper-view", image, TextureOptions::NEAREST)),
            }
            self.shown = Some(self.view);
        }
        if let Some(texture) = &self.texture {
            let size = vec2(picture.width as f32, picture.height as f32) * self.zoom as f32;
            egui::ScrollArea::both().id_salt("rip-picture").max_height(420.0).max_width(760.0).show(ui, |ui| ui.add(egui::Image::new((texture.id(), size))));
        }
        ui.horizontal_wrapped(|ui| {
            if ui.button(crate::i18n::tr("Save as a picture\u{2026}")).on_hover_text(crate::i18n::tr("A PNG of what's shown")).clicked() {
                asked = Some(Asked::SavePicture);
            }
            if ui.button(crate::i18n::tr("Copy as source")).on_hover_text(crate::i18n::tr("The bytes shown, as DB lines")).clicked() {
                asked = Some(Asked::CopySource);
            }
            for target in self.targets() {
                if ui.button(crate::i18n::tr(target.name())).clicked() {
                    asked = Some(Asked::Send(target));
                }
            }
        });
        asked
    }
}
