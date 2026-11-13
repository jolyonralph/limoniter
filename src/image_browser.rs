//! Browsing a disk, tape or CD image or an archive, read only, as a tab of its own. A folder opens in place, an
//! image or archive inside it opens a level further in, and a file opens as a read-only document of its own,
//! named for where it came from, so the previews, listings and encodings all work on it. Nothing is written back.

use super::*;
use crate::legacy_docs::{self, Target};
use crate::media::{self, Entry, InfoSection, Volume};

/// One image or archive being browsed, and the folder showing in it.
struct Level {
    name: String,
    volume: Box<dyn Volume>,
    folders: Vec<Entry>,
    entries: Vec<Entry>,
    /// What the image says about itself, read once, for File properties.
    info: Vec<InfoSection>,
    picture: Option<Vec<u8>>,
}

pub(super) struct ImageBrowser {
    levels: Vec<Level>,
    selected: Option<usize>,
    problem: Option<String>,
    /// Whether the selected file is a document that can be converted or a picture that can be saved as PNG, kept
    /// for the place and entry it was found for.
    document: Option<(String, Option<legacy_docs::Kind>, Option<crate::pictures::Format>)>,
}

/// Files no larger than this are looked at, when selected, to see if they're documents to convert.
const MOST_DOCUMENT_BYTES: u64 = 4 * 1024 * 1024;

enum BrowserAction {
    Open(usize),
    Up,
    SaveCopy(usize),
    SavePng(usize),
    ImageInHex,
    /// Shows or hides File properties, which says all the image says about itself.
    ToggleInfo,
    /// Converts a document: opened as text with None, or saved as Markdown, plain text or RTF.
    Convert(usize, Option<Target>),
}

impl ImageBrowser {
    pub(super) fn new(name: String, volume: Box<dyn Volume>) -> Self {
        let mut browser = Self { levels: Vec::new(), selected: None, problem: None, document: None };
        browser.push(name, volume);
        browser
    }

    fn push(&mut self, name: String, volume: Box<dyn Volume>) {
        let mut rows = vec![("Format".to_string(), volume.describe())];
        rows.extend(volume.notes().into_iter().map(|note| (String::new(), note)));
        let mut info = vec![InfoSection { title: "Image".into(), rows }];
        info.extend(volume.info());
        let picture = volume.picture();
        self.levels.push(Level { name, volume, folders: Vec::new(), entries: Vec::new(), info, picture });
        self.refresh();
    }

    /// What File properties shows of this tab.
    pub(super) fn view(&self) -> panels::ImageView<'_> {
        let level = self.level();
        panels::ImageView { place: self.place(), sections: &level.info, picture: level.picture.as_deref(), selected: self.selected.and_then(|i| level.entries.get(i)) }
    }

    fn level(&self) -> &Level {
        self.levels.last().expect("a browser always has its image")
    }

    fn refresh(&mut self) {
        let level = self.levels.last_mut().expect("a browser always has its image");
        match level.volume.list(level.folders.last()) {
            Ok(entries) => {
                level.entries = entries;
                self.problem = None;
            }
            Err(problem) => {
                level.entries.clear();
                self.problem = Some(problem);
            }
        }
        self.selected = (!self.level().entries.is_empty()).then_some(0);
    }

    /// Where the browser is, from the image's own name down: images, archives and folders.
    pub(super) fn place(&self) -> String {
        let mut parts = Vec::new();
        for level in &self.levels {
            parts.push(level.name.clone());
            parts.extend(level.folders.iter().map(|f| f.name.clone()));
        }
        parts.join(" \u{203A} ")
    }

    /// Goes up a folder, or out of an image opened inside another. False at the top.
    fn up(&mut self) -> bool {
        let level = self.levels.last_mut().expect("a browser always has its image");
        if level.folders.pop().is_some() {
            self.refresh();
            return true;
        }
        if self.levels.len() > 1 {
            self.levels.pop();
            self.refresh();
            return true;
        }
        false
    }

    /// The document format of the file at this place, if it's one that can be converted, and its picture format, if
    /// it's a picture; read once when selected.
    fn document_kind(&mut self, index: usize) -> (Option<legacy_docs::Kind>, Option<crate::pictures::Format>) {
        let key = format!("{}#{index}", self.place());
        if let Some((known, kind, picture)) = &self.document
            && *known == key
        {
            return (*kind, *picture);
        }
        let Some(entry) = self.level().entries.get(index) else {
            return (None, None);
        };
        // Documents are small; pictures, such as a Photo CD's, can be larger.
        let (kind, picture) = if entry.is_dir || entry.size > crate::pictures::MOST_PICTURE_BYTES as u64 {
            (None, None)
        } else {
            self.take(index).ok().map_or((None, None), |(name, bytes)| {
                let document = if bytes.len() as u64 <= MOST_DOCUMENT_BYTES { legacy_docs::detect(&name, &bytes) } else { None };
                (document, crate::pictures::detect(&name, &bytes))
            })
        };
        self.document = Some((key, kind, picture));
        (kind, picture)
    }

    /// A file's bytes and the name to open it under.
    fn take(&self, index: usize) -> Result<(String, Vec<u8>), String> {
        let level = self.level();
        let entry = level.entries.get(index).ok_or("Nothing is selected.")?;
        let bytes = level.volume.read(entry)?;
        Ok((entry.open_name.clone(), bytes))
    }

    /// The file selected, taken out, if a file (not a folder) is selected.
    pub(super) fn selected_file(&self) -> Option<Result<(String, Vec<u8>), String>> {
        let index = self.selected?;
        let entry = self.level().entries.get(index)?;
        (!entry.is_dir).then(|| self.take(index))
    }

    /// The text preset the image has for a file in this folder, such as the Amiga's.
    fn text_preset(&self, index: usize) -> Option<&'static str> {
        let level = self.level();
        level.entries.get(index).and_then(|entry| level.volume.text_preset(entry))
    }

    /// Opens a folder in place, or an image inside this one. A file is handed back to open as a document.
    fn open(&mut self, index: usize) -> Option<Result<(String, Vec<u8>), String>> {
        let entry = self.level().entries.get(index)?.clone();
        if entry.is_dir {
            self.levels.last_mut().expect("a browser always has its image").folders.push(entry);
            self.refresh();
            return None;
        }
        // A file opened to read may be cut short of what's saved, as a CP/M text file is at its Ctrl+Z.
        let taken = self.take(index).map(|(name, bytes)| {
            let level = self.level();
            (name, level.volume.reading_bytes(&level.entries[index], bytes))
        });
        if let Ok((name, bytes)) = &taken
            && media::is_nested_image(name, bytes)
            && let Ok(volume) = media::open(Arc::new(bytes.clone()), name)
        {
            self.push(name.clone(), volume);
            return None;
        }
        Some(taken)
    }

    fn draw(&mut self, ui: &mut Ui, info_open: bool) -> Option<BrowserAction> {
        let mut action = None;
        let (document, picture) = self.selected.map_or((None, None), |i| self.document_kind(i));
        let level = self.levels.last().expect("a browser always has its image");
        ui.add_space(6.0);
        ui.horizontal_wrapped(|ui| {
            ui.label(RichText::new(self.place()).strong().size(14.0));
        });
        ui.label(RichText::new(level.volume.describe()).color(theme::text_dim()));
        for note in level.volume.notes() {
            ui.label(RichText::new(note).color(theme::text_faint()).size(11.5));
        }
        if let Some(problem) = &self.problem {
            ui.label(RichText::new(problem).color(theme::rainbow_mistake()));
        }
        ui.add_space(4.0);
        ui.horizontal(|ui| {
            let at_top = self.levels.len() == 1 && level.folders.is_empty();
            if ui.add_enabled(!at_top, Button::new("\u{2191} Up")).on_hover_text("Up a folder (Backspace)").clicked() {
                action = Some(BrowserAction::Up);
            }
            let chosen = self.selected.filter(|i| *i < level.entries.len());
            if ui.add_enabled(chosen.is_some(), Button::new("Open")).on_hover_text("Open the file, folder or image inside (Enter, or double-click)").clicked() {
                action = chosen.map(BrowserAction::Open);
            }
            let is_file = chosen.is_some_and(|i| !level.entries[i].is_dir);
            if ui.add_enabled(is_file, Button::new("Save a copy\u{2026}")).on_hover_text("Write the file out of the image, exactly as it is").clicked() {
                action = chosen.map(BrowserAction::SaveCopy);
            }
            if let (Some(i), Some(format)) = (chosen, picture)
                && ui.button("Save as PNG\u{2026}").on_hover_text(format!("Save this {} as a PNG, pixel for pixel", format.name())).clicked()
            {
                action = Some(BrowserAction::SavePng(i));
            }
            if let (Some(i), Some(kind)) = (chosen, document) {
                ui.menu_button(format!("Convert ({})", kind.name()), |ui| {
                    if ui.button("Open as text").on_hover_text("The document's text in a new tab, its formatting codes taken out").clicked() {
                        action = Some(BrowserAction::Convert(i, None));
                        ui.close();
                    }
                    for (label, target) in [("Save as Markdown\u{2026}", Target::Markdown), ("Save as plain text\u{2026}", Target::Text), ("Save as RTF\u{2026}", Target::Rtf)] {
                        if ui.button(label).clicked() {
                            action = Some(BrowserAction::Convert(i, Some(target)));
                            ui.close();
                        }
                    }
                });
            }
            if self.levels.len() == 1 && ui.button("Show the image in hex").on_hover_text("Open the image file itself, read only, as bytes").clicked() {
                action = Some(BrowserAction::ImageInHex);
            }
            if ui.selectable_label(info_open, "\u{2139} Info").on_hover_text("Show everything the image says about itself, and about the chosen file, in File properties").clicked() {
                action = Some(BrowserAction::ToggleInfo);
            }
        });
        ui.separator();
        let entries = &self.levels.last().expect("a browser always has its image").entries;
        if entries.is_empty() && self.problem.is_none() {
            ui.label(RichText::new("This folder is empty.").color(theme::text_faint()));
        }
        egui::ScrollArea::vertical().id_salt("image-browser").auto_shrink([false, false]).show(ui, |ui| {
            egui::Grid::new("image-browser-grid").num_columns(4).spacing([18.0, 3.0]).striped(true).show(ui, |ui| {
                for header in ["Name", "Type", "Size", "Details"] {
                    ui.label(RichText::new(header).color(theme::text_dim()).size(12.0));
                }
                ui.end_row();
                for (index, entry) in entries.iter().enumerate() {
                    let icon = if entry.is_dir { "\u{1F4C1} " } else { "" };
                    let row = ui.add(egui::Button::selectable(self.selected == Some(index), format!("{icon}{}", entry.name)));
                    if row.clicked() {
                        self.selected = Some(index);
                    }
                    if row.double_clicked() {
                        action = Some(BrowserAction::Open(index));
                    }
                    ui.label(&entry.kind);
                    ui.label(if entry.is_dir { String::new() } else { short_size(entry.size) });
                    ui.label(RichText::new(&entry.detail).color(theme::text_dim()));
                    ui.end_row();
                }
            });
        });
        // The keyboard moves through the list when nothing else wants it.
        if ui.ctx().memory(|m| m.focused().is_none()) {
            let count = entries.len();
            ui.ctx().input(|input| {
                if input.key_pressed(Key::ArrowDown) && count > 0 {
                    self.selected = Some(self.selected.map_or(0, |i| (i + 1).min(count - 1)));
                }
                if input.key_pressed(Key::ArrowUp) && count > 0 {
                    self.selected = Some(self.selected.map_or(0, |i| i.saturating_sub(1)));
                }
                if input.key_pressed(Key::Enter) {
                    action = self.selected.map(BrowserAction::Open);
                }
                if input.key_pressed(Key::Backspace) {
                    action = Some(BrowserAction::Up);
                }
            });
        }
        action
    }
}

/// A read-only tab browsing the image at this path. Its size and time are noted, so it isn't taken for changed
/// by another program the next time the window comes back into focus.
pub(super) fn image_document(path: &Path) -> Result<Document, String> {
    let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let volume = media::open_file(path)?;
    let mut doc = Document::new(String::new());
    doc.path = Some(path.to_path_buf());
    doc.set_read_only(true);
    doc.disk_stamp = file_stamp(path);
    doc.browser = Some(Box::new(ImageBrowser::new(name, volume)));
    Ok(doc)
}

impl Limoniter {
    /// Opens an image or archive in a browser tab. False if it isn't one this can read, to be opened as a file.
    pub(super) fn open_image(&mut self, path: &Path) -> bool {
        let Ok(mut file) = std::fs::File::open(path) else {
            return false;
        };
        let len = file.metadata().map_or(0, |m| m.len());
        let mut head = vec![0u8; len.min(0x8800) as usize];
        if std::io::Read::read_exact(&mut file, &mut head).is_err() {
            return false;
        }
        let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        if !media::is_image(&name, &head, len) {
            return false;
        }
        let doc = match image_document(path) {
            Ok(doc) => doc,
            Err(problem) => {
                self.message("Open an image", format!("{name} couldn't be read as an image, so it opens as a file instead.\n\n{problem}"));
                return false;
            }
        };
        if self.docs.len() == 1 && self.docs[0].is_blank() {
            self.docs.clear();
        }
        self.docs.push(doc);
        self.active = self.docs.len() - 1;
        panels::note_recent(&mut self.panels.recent, path);
        true
    }

    /// Opens a file from the image a browser shows, in a tab of its own. A BASIC program from a Spectrum tape or
    /// snapshot brings the UDGs the tape loads or the snapshot has, for its listing to draw.
    pub(super) fn open_from_browser(&mut self, index: usize, i: usize) {
        let Some(browser) = self.docs[index].browser.as_mut() else {
            return;
        };
        match browser.open(i) {
            None => {}
            Some(Ok((name, bytes))) => {
                let place = browser.place();
                let mut doc = Document::from_image_file(format!("{place} \u{203A} {name}"), bytes, browser.text_preset(i));
                doc.preview_view = self.opening_view(Some(Path::new(&name)), doc.picture.is_some());
                let image = self.docs[index].path.clone();
                let ending = image.as_deref().and_then(Path::extension).and_then(|e| e.to_str()).map(str::to_lowercase).unwrap_or_default();
                let program = Path::new(&name).extension().is_some_and(|e| e.eq_ignore_ascii_case("bas"));
                if program
                    && matches!(ending.as_str(), "tap" | "tzx" | "sna" | "z80")
                    && let Some(udgs) = image.and_then(|path| std::fs::read(path).ok()).and_then(|image| zx_host::udgs_in_file(&image, &ending))
                {
                    let from = if matches!(ending.as_str(), "tap" | "tzx") { "the tape it came from" } else { "the snapshot it came from" };
                    doc.text.zx = Some(Box::new(crate::zx_graphics::ZxText { udgs, udgs_from: Some(from.into()), ..Default::default() }));
                }
                self.docs.push(doc);
                self.active = self.docs.len() - 1;
            }
            Some(Err(problem)) => self.message("Open from an image", problem),
        }
    }

    /// Draws the browser in the text's place, and does what's asked of it.
    pub(super) fn browser_view(&mut self, ui: &mut Ui) {
        let index = self.active;
        let info_open = self.panels.file_properties;
        let Some(browser) = self.docs[index].browser.as_mut() else {
            return;
        };
        let action = egui::Frame::new().inner_margin(egui::Margin::symmetric(12, 4)).show(ui, |ui| browser.draw(ui, info_open)).inner;
        match action {
            Some(BrowserAction::Up) => {
                browser.up();
            }
            Some(BrowserAction::Open(i)) => self.open_from_browser(index, i),
            Some(BrowserAction::SaveCopy(i)) => match browser.take(i) {
                Ok((name, bytes)) => {
                    let mut picker = rfd::FileDialog::new().set_title("Save a copy of the file").set_file_name(&name);
                    if let Some(folder) = self.docs[index].path.as_deref().and_then(Path::parent) {
                        picker = picker.set_directory(folder);
                    }
                    if let Some(target) = picker.save_file()
                        && let Err(e) = codec::write_file(&target, &bytes)
                    {
                        self.message("Save a copy", format!("Couldn't save {}.\n\n{e}", target.display()));
                    }
                }
                Err(problem) => self.message("Save a copy", problem),
            },
            Some(BrowserAction::SavePng(i)) => {
                let png = browser.take(i).and_then(|(name, bytes)| {
                    let format = crate::pictures::detect(&name, &bytes).ok_or("This isn't a picture Limoniter can read.")?;
                    let png = crate::pictures::to_png(&crate::pictures::decode_named(format, &name, &bytes)?)?;
                    Ok((name, png))
                });
                match png {
                    Ok((name, png)) => {
                        let mut picker = rfd::FileDialog::new().set_title("Save the picture as PNG").set_file_name(crate::pictures::png_name(&name)).add_filter("PNG picture", &["png"]);
                        if let Some(folder) = self.docs[index].path.as_deref().and_then(Path::parent) {
                            picker = picker.set_directory(folder);
                        }
                        if let Some(target) = picker.save_file()
                            && let Err(e) = codec::write_file(&target, &png)
                        {
                            self.message("Save as PNG", format!("Couldn't save {}.\n\n{e}", target.display()));
                        }
                    }
                    Err(problem) => self.message("Save as PNG", problem),
                }
            }
            Some(BrowserAction::Convert(i, target)) => {
                let converted = browser.take(i).and_then(|(name, bytes)| {
                    let kind = legacy_docs::detect(&name, &bytes).ok_or("This isn't a document Limoniter can convert.")?;
                    Ok((name, legacy_docs::convert(kind, &bytes)?))
                });
                let (name, doc) = match converted {
                    Ok(done) => done,
                    Err(problem) => return self.message("Convert a document", problem),
                };
                let stem = Path::new(&name).file_stem().map_or(name.clone(), |s| s.to_string_lossy().into_owned());
                let lost = if doc.notes.is_empty() { String::new() } else { format!("\n\n{}", doc.notes.join("\n")) };
                match target {
                    None => {
                        let place = browser.place();
                        let mut opened = Document::from_image_file(format!("{place} \u{203A} {stem} (as text).txt"), Target::Text.write(&doc).into_bytes(), None);
                        opened.preview_view = self.opening_view(Some(Path::new("x.txt")), false);
                        self.docs.push(opened);
                        self.active = self.docs.len() - 1;
                    }
                    Some(target) => {
                        let mut picker = rfd::FileDialog::new().set_title(format!("Save as {}", match target {
                            Target::Markdown => "Markdown",
                            Target::Text => "plain text",
                            Target::Rtf => "RTF",
                        })).set_file_name(format!("{stem}.{}", target.ending()));
                        if let Some(folder) = self.docs[index].path.as_deref().and_then(Path::parent) {
                            picker = picker.set_directory(folder);
                        }
                        if let Some(path) = picker.save_file() {
                            match codec::write_file(&path, target.write(&doc).as_bytes()) {
                                Ok(()) if !lost.is_empty() => self.message("Convert a document", format!("Saved {}, from {}.{lost}", path.display(), doc.format)),
                                Ok(()) => {}
                                Err(e) => self.message("Convert a document", format!("Couldn't save {}.\n\n{e}", path.display())),
                            }
                        }
                    }
                }
            }
            Some(BrowserAction::ToggleInfo) => {
                self.panels.file_properties = !info_open;
            }
            Some(BrowserAction::ImageInHex) => {
                let Some(path) = self.docs[index].path.clone() else {
                    return;
                };
                match std::fs::metadata(&path) {
                    Ok(meta) if meta.len() <= codec::MAX_FILE_BYTES => match std::fs::read(&path) {
                        Ok(bytes) => {
                            let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
                            let mut doc = Document::from_image_file(format!("{name} (bytes)"), Vec::new(), None);
                            doc.hex.load(bytes);
                            doc.hex_mode = true;
                            self.docs.push(doc);
                            self.active = self.docs.len() - 1;
                        }
                        Err(e) => self.message("Show the image in hex", e.to_string()),
                    },
                    _ => self.message("Show the image in hex", "The image is larger than the 1 GB that can be shown in hex.".into()),
                }
            }
            None => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::tests::{app_with, draw};

    fn fixture_file(name: &str) -> PathBuf {
        let bytes = crate::media::tests::fixture(name);
        // Each call gets its own folder, so tests running at once don't write over each other's files.
        static CALLS: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        let call = CALLS.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!("limoniter-image-{}-{call}-{name}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join(name);
        std::fs::write(&path, bytes).unwrap();
        path
    }

    #[test]
    fn an_image_opens_as_a_browser_and_its_files_open_read_only_with_their_previews() {
        let (mut app, ctx) = app_with("");
        let path = fixture_file("nested.zip");
        assert!(app.open_path(&path));
        let browser = app.docs[app.active].browser.as_mut().expect("a ZIP opens in a browser");
        assert_eq!(browser.place(), "nested.zip");
        // The folder, then the disk inside it, then the program on the disk.
        assert!(browser.open(0).is_none());
        assert_eq!(browser.place(), "nested.zip \u{203A} disks");
        assert!(browser.open(0).is_none(), "the D64 opens a level further in");
        assert_eq!(browser.place(), "nested.zip \u{203A} disks \u{203A} test.d64");
        let (name, bytes) = browser.open(0).unwrap().unwrap();
        assert_eq!(name, "HELLO.prg");
        let place = browser.place();
        let doc = Document::from_image_file(format!("{place} \u{203A} {name}"), bytes.clone(), None);
        assert!(doc.text.read_only);
        assert!(doc.hex_mode, "a program is binary, so it opens in hex");
        assert_eq!(doc.name(), "nested.zip \u{203A} disks \u{203A} test.d64 \u{203A} HELLO.prg");
        app.docs.push(doc);
        app.active = app.docs.len() - 1;
        app.doc().preview_view = settings::PreviewView::Split;
        draw(&mut app, &ctx, Vec::new());
        assert!(app.preview_blocks.iter().any(|b| b.kind == preview::BlockKind::Code && b.spans[0].text.contains("10 PRINT\"HI\"")), "the BASIC listing works on it");
        // Up goes back out, a level at a time.
        let index = app.docs.iter().position(|d| d.browser.is_some()).unwrap();
        let browser = app.docs[index].browser.as_mut().unwrap();
        assert!(browser.up());
        assert_eq!(browser.place(), "nested.zip \u{203A} disks");
        assert!(browser.up() && !browser.up());
    }

    #[test]
    fn an_image_tab_is_not_taken_for_changed_and_reloads_as_a_browser() {
        let (mut app, _ctx) = app_with("");
        let path = fixture_file("ofs.adf");
        assert!(app.open_path(&path));
        let index = app.active;
        assert!(app.docs[index].disk_stamp.is_some(), "its size and time are noted when it opens");
        assert_eq!(file_stamp(&path), app.docs[index].disk_stamp, "so coming back to the window doesn't ask to reload it");
        let fresh = app.reopened(index, &path).unwrap();
        assert!(fresh.browser.is_some() && !fresh.hex_mode, "a reload opens it in the browser again, not as bytes");
    }

    #[test]
    fn files_off_an_amiga_disk_read_in_the_amiga_preset_and_programs_stay_binary() {
        let (mut app, _ctx) = app_with("");
        assert!(app.open_path(&fixture_file("ofs.adf")));
        let browser = app.docs[app.active].browser.as_mut().unwrap();
        let top: Vec<String> = browser.level().entries.iter().map(|e| e.name.clone()).collect();
        let at = |name: &str| top.iter().position(|n| n == name).unwrap();
        let hello = at("hello.txt");
        let (name, bytes) = browser.open(hello).unwrap().unwrap();
        let doc = Document::from_image_file(name, bytes, browser.text_preset(hello));
        assert_eq!(doc.text.format.preset, Some(media::AMIGA_TEXT_PRESET));
        assert_eq!(doc.text.format.charset.id, "amiga-text");
        assert!(!doc.hex_mode);
        let big = at("big.bin");
        let (name, bytes) = browser.open(big).unwrap().unwrap();
        let doc = Document::from_image_file(name, bytes, browser.text_preset(big));
        assert!(doc.hex_mode, "a binary file still opens in hex");
        // In a folder, a file with no ending at all.
        assert!(browser.open(at("docs")).is_none());
        let (name, bytes) = browser.open(0).unwrap().unwrap();
        let doc = Document::from_image_file(name, bytes, browser.text_preset(0));
        assert_eq!(doc.text.format.preset, Some(media::AMIGA_TEXT_PRESET));
        // Other images leave it to the bytes.
        assert!(app.open_path(&fixture_file("fat12.img")));
        let browser = app.docs[app.active].browser.as_ref().unwrap();
        assert!((0..browser.level().entries.len()).all(|i| browser.text_preset(i).is_none()));
    }

    #[test]
    fn a_document_on_a_disk_offers_to_convert_and_opens_as_its_text() {
        let (mut app, ctx) = app_with("");
        let path = std::env::temp_dir().join(format!("limoniter-letters-{}.imd", std::process::id()));
        std::fs::write(&path, crate::media::tests::hp150_imd_with_letter()).unwrap();
        assert!(app.open_path(&path));
        let browser = app.docs[app.active].browser.as_mut().unwrap();
        let names: Vec<String> = browser.level().entries.iter().map(|e| e.name.clone()).collect();
        let letter = names.iter().position(|n| n == "HELLO.TXT").unwrap();
        assert_eq!(browser.document_kind(letter).0, Some(legacy_docs::Kind::WordStar));
        let other = names.iter().position(|n| n == "NAMELESS").unwrap();
        assert_eq!(browser.document_kind(other).0, None, "plain bytes aren't a document");
        browser.selected = Some(letter);
        draw(&mut app, &ctx, Vec::new());
        // Opening it as text: what the Convert menu's first item does.
        let index = app.active;
        let browser = app.docs[index].browser.as_mut().unwrap();
        let (name, bytes) = browser.take(letter).unwrap();
        let doc = legacy_docs::convert(legacy_docs::detect(&name, &bytes).unwrap(), &bytes).unwrap();
        assert_eq!(Target::Text.write(&doc), "Dear Dad,\nThe deck chairs are late again.\n");
        assert_eq!(Target::Markdown.write(&doc), "Dear Dad,\\\nThe deck chairs are <u>late</u> again.\n");
    }

    #[test]
    fn a_browser_tab_never_saves_over_its_image() {
        let (mut app, ctx) = app_with("");
        let path = fixture_file("test.d64");
        let before = std::fs::read(&path).unwrap();
        assert!(app.open_path(&path));
        draw(&mut app, &ctx, Vec::new());
        assert!(!app.save(app.active));
        assert_eq!(std::fs::read(&path).unwrap(), before);
        // Something that only looks like an image by its ending opens as a file.
        let fake = path.with_file_name("data.bin");
        std::fs::write(&fake, b"not a cd").unwrap();
        assert!(app.open_path(&fake));
        assert!(app.docs[app.active].browser.is_none());
    }
}
