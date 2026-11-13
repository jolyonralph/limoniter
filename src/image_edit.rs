//! Changing the image a browser shows: adding files to it (from the Add button, or dropped on it), deleting,
//! renaming, making folders and reordering a tape's files, each saved over the image at once (written whole and
//! read back before it replaces the old file), with Undo to put the image back as it was before each change.
//!
//! Only the image itself is changed, not one opened inside it; and only kinds of image a writer exists for.

use super::*;
use crate::media::write::{self, Addition, Blank, ImageWriter, Kind};

/// How many changes can be undone.
const MOST_UNDO: usize = 20;

/// What a writable image can do, found when the browser first draws.
pub(super) struct Abilities {
    pub(super) kinds: Vec<Kind>,
    pub(super) folders: bool,
    pub(super) order: bool,
    pub(super) free: Option<u64>,
}

/// A file waiting to go on the image, as the Add panel shows it.
pub(super) struct Pending {
    pub(super) addition: Addition,
    pub(super) from: String,
    pub(super) address: String,
    pub(super) autostart: String,
    pub(super) problem: Option<String>,
    /// A file of the same name is there: the person can tick Replace.
    pub(super) clash: bool,
}

/// An edit in progress, shown in a strip above the list.
pub(super) enum Editing {
    Rename { index: usize, name: String },
    Delete { index: usize },
    NewFolder { name: String },
}

pub(super) enum EditAction {
    AddFiles,
    AddPaths(Vec<PathBuf>),
    ConfirmAdd,
    CancelAdd,
    StartRename(usize),
    StartDelete(usize),
    StartFolder,
    Finish,
    CancelEdit,
    Move(usize, i32),
    Undo,
}

/// A number as typed: $8000, 0x8000, #8000, 8000h or 32768.
fn number(text: &str) -> Option<u16> {
    let t = text.trim();
    let (digits, radix) = if let Some(h) = t.strip_prefix('$').or_else(|| t.strip_prefix('#')).or_else(|| t.strip_prefix("0x")).or_else(|| t.strip_prefix("0X")) {
        (h, 16)
    } else if let Some(h) = t.strip_suffix('h').or_else(|| t.strip_suffix('H')) {
        (h, 16)
    } else {
        (t, 10)
    };
    u32::from_str_radix(digits, radix).ok().and_then(|n| u16::try_from(n).ok())
}

impl ImageBrowser {
    /// Whether the image in front (not one opened inside it) can be changed, worked out once.
    pub(super) fn abilities(&mut self) -> Option<&Abilities> {
        if self.levels.len() != 1 {
            return None;
        }
        if self.abilities.is_none() {
            self.abilities = Some(self.work_out_abilities());
        }
        self.abilities.as_ref().and_then(|a| a.as_ref().ok())
    }

    /// Whether the image showing can be changed, as far as is known yet.
    pub(crate) fn writable(&self) -> bool {
        self.levels.len() == 1 && matches!(self.abilities, Some(Ok(_)))
    }

    /// Why the image can't be changed, if it was tried and can't.
    pub(super) fn not_writable(&self) -> Option<&str> {
        self.abilities.as_ref().and_then(|a| a.as_ref().err()).map(String::as_str)
    }

    fn work_out_abilities(&self) -> Result<Abilities, String> {
        let path = self.path.as_ref().ok_or("The image has no file to change.")?;
        let name = self.levels[0].name.clone();
        let len = std::fs::metadata(path).map_err(|e| e.to_string())?.len();
        let mut head = vec![0u8; len.min(0x8800) as usize];
        std::fs::File::open(path).and_then(|mut f| std::io::Read::read_exact(&mut f, &mut head)).map_err(|e| e.to_string())?;
        if !write::may_write(&name, &head, len) {
            return Err(String::new());
        }
        let writer = write::writer(&name, std::fs::read(path).map_err(|e| e.to_string())?)?;
        Ok(Abilities { kinds: writer.kinds(), folders: writer.has_folders(), order: writer.has_order(), free: writer.free() })
    }

    /// Makes a change to the image: read from its file, changed, written back whole, and opened again in the same
    /// folder. The image as it was is kept to undo to.
    fn change(&mut self, what: impl FnOnce(&mut dyn ImageWriter, Option<&Entry>) -> Result<(), String>) -> Result<(), String> {
        let path = self.path.clone().ok_or("The image has no file to change.")?;
        let name = self.levels[0].name.clone();
        let before = std::fs::read(&path).map_err(|e| format!("The image couldn't be read again: {e}"))?;
        let mut writer = write::writer(&name, before.clone())?;
        let folder = self.levels[0].folders.last().cloned();
        what(&mut *writer, folder.as_ref())?;
        let after = writer.bytes().to_vec();
        if after == before {
            return Ok(());
        }
        crate::codec::write_file(&path, &after).map_err(|e| format!("The image couldn't be saved: {e}"))?;
        self.undo.push(before);
        if self.undo.len() > MOST_UNDO {
            self.undo.remove(0);
        }
        self.free = writer.free();
        self.reopen(after)
    }

    /// Opens the image again from its new bytes, back in the folder it was showing.
    fn reopen(&mut self, bytes: Vec<u8>) -> Result<(), String> {
        let name = self.levels[0].name.clone();
        let volume = media::open(Arc::new(bytes), &name)?;
        let names: Vec<String> = self.levels[0].folders.iter().map(|f| f.name.clone()).collect();
        let mut folders = Vec::new();
        for folder in names {
            let found = volume.list(folders.last()).ok().and_then(|entries| entries.into_iter().find(|e| e.is_dir && e.name == folder));
            match found {
                Some(entry) => folders.push(entry),
                None => break,
            }
        }
        let selected = self.selected;
        self.levels.clear();
        self.push(name, volume);
        self.levels[0].folders = folders;
        self.refresh();
        self.selected = selected.filter(|i| *i < self.levels[0].entries.len()).or(self.selected);
        self.changed = true;
        Ok(())
    }

    fn undo_change(&mut self) -> Result<(), String> {
        let path = self.path.clone().ok_or("The image has no file.")?;
        let Some(before) = self.undo.pop() else {
            return Ok(());
        };
        crate::codec::write_file(&path, &before).map_err(|e| format!("The image couldn't be saved: {e}"))?;
        self.free = write::writer(&self.levels[0].name, before.clone()).ok().and_then(|w| w.free());
        self.reopen(before)
    }

    /// Files read and readied to add: each with the kind, name and address the image would most likely want.
    fn queue(&mut self, paths: Vec<PathBuf>) -> Result<(), String> {
        let path = self.path.clone().ok_or("The image has no file.")?;
        let writer = write::writer(&self.levels[0].name, std::fs::read(&path).map_err(|e| e.to_string())?)?;
        for file in paths {
            let name = file.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
            if file.is_dir() {
                self.problem = Some(format!("{name} is a folder; add the files in it instead."));
                continue;
            }
            match std::fs::read(&file) {
                Ok(bytes) if bytes.len() as u64 > write::MOST_WRITABLE_BYTES => self.problem = Some(format!("{name} is too large to add.")),
                Ok(bytes) => {
                    let addition = writer.guess(&name, &bytes);
                    let address = addition.address.map(|a| a.to_string()).unwrap_or_default();
                    let autostart = addition.autostart.map(|a| a.to_string()).unwrap_or_default();
                    self.pending.push(Pending { addition, from: name, address, autostart, problem: None, clash: false });
                }
                Err(e) => self.problem = Some(format!("{name} couldn't be read: {e}")),
            }
        }
        Ok(())
    }

    /// Adds every file waiting, in turn; those that can't go on stay in the panel with the reason.
    fn add_pending(&mut self) {
        let kinds = self.abilities.as_ref().and_then(|a| a.as_ref().ok()).map(|a| a.kinds.clone()).unwrap_or_default();
        let pending = std::mem::take(&mut self.pending);
        let mut left = Vec::new();
        for mut item in pending {
            let kind = kinds.iter().find(|k| k.name == item.addition.kind).copied();
            let mut addition = item.addition.clone();
            if kind.is_some_and(|k| k.address) {
                match number(&item.address) {
                    Some(a) => addition.address = Some(a),
                    None => {
                        item.problem = Some("Say where it loads: $8000, or 32768.".into());
                        left.push(item);
                        continue;
                    }
                }
            }
            if kind.is_some_and(|k| k.autostart) {
                addition.autostart = if item.autostart.trim().is_empty() { None } else { item.autostart.trim().parse().ok().filter(|l: &u16| *l < 10000) };
                if !item.autostart.trim().is_empty() && addition.autostart.is_none() {
                    item.problem = Some("The line to run from is a number below 10000, or left empty.".into());
                    left.push(item);
                    continue;
                }
            }
            match self.change(|writer, folder| writer.add(folder, &addition)) {
                Ok(()) => {}
                Err(problem) => {
                    item.clash = problem.starts_with(write::ALREADY);
                    item.problem = Some(problem);
                    left.push(item);
                }
            }
        }
        self.pending = left;
    }

    /// Adds what's waiting, for a picture of the image afterwards; a path that isn't a file is dropped first.
    pub(crate) fn confirm_add_for_picture(&mut self) {
        self.problem = None;
        self.act(EditAction::ConfirmAdd);
    }

    pub(super) fn act(&mut self, action: EditAction) {
        let done = match action {
            EditAction::AddFiles => {
                let picked = rfd::FileDialog::new().set_title("Add files to the image").pick_files().unwrap_or_default();
                self.queue(picked)
            }
            EditAction::AddPaths(paths) => self.queue(paths),
            EditAction::ConfirmAdd => {
                self.add_pending();
                Ok(())
            }
            EditAction::CancelAdd => {
                self.pending.clear();
                Ok(())
            }
            EditAction::StartRename(index) => {
                let name = self.level().entries.get(index).map(|e| e.name.clone()).unwrap_or_default();
                self.editing = Some(Editing::Rename { index, name });
                Ok(())
            }
            EditAction::StartDelete(index) => {
                self.editing = Some(Editing::Delete { index });
                Ok(())
            }
            EditAction::StartFolder => {
                self.editing = Some(Editing::NewFolder { name: String::new() });
                Ok(())
            }
            EditAction::CancelEdit => {
                self.editing = None;
                Ok(())
            }
            EditAction::Finish => match self.editing.take() {
                Some(Editing::Rename { index, name }) => match self.level().entries.get(index).cloned() {
                    Some(entry) => self.change(|writer, folder| writer.rename(folder, &entry, &name)).inspect_err(|_| self.editing = Some(Editing::Rename { index, name: name.clone() })),
                    None => Ok(()),
                },
                Some(Editing::Delete { index }) => match self.level().entries.get(index).cloned() {
                    Some(entry) => self.change(|writer, folder| writer.delete(folder, &entry)),
                    None => Ok(()),
                },
                Some(Editing::NewFolder { name }) => self.change(|writer, folder| writer.make_folder(folder, &name)).inspect_err(|_| self.editing = Some(Editing::NewFolder { name: name.clone() })),
                None => Ok(()),
            },
            EditAction::Move(index, by) => match self.level().entries.get(index).cloned() {
                Some(entry) => {
                    let result = self.change(|writer, _| writer.reorder(&entry, by));
                    let count = self.level().entries.len() as i64;
                    self.selected = Some((index as i64 + i64::from(by)).clamp(0, (count - 1).max(0)) as usize);
                    result
                }
                None => Ok(()),
            },
            EditAction::Undo => self.undo_change(),
        };
        if let Err(problem) = done {
            self.problem = Some(problem);
        }
    }

    /// The buttons for changing the image, beside the browser's own.
    pub(super) fn edit_buttons(&mut self, ui: &mut Ui, chosen: Option<usize>, action: &mut Option<EditAction>) {
        let Some(abilities) = self.abilities() else {
            return;
        };
        let (folders, order) = (abilities.folders, abilities.order);
        if ui.button("\u{2795} Add files\u{2026}").on_hover_text("Put files from this computer on the image (or drop them on it)").clicked() {
            *action = Some(EditAction::AddFiles);
        }
        if folders && ui.button("New folder").clicked() {
            *action = Some(EditAction::StartFolder);
        }
        let is_entry = chosen.is_some();
        if ui.add_enabled(is_entry, Button::new("Rename")).on_hover_text("Give the file a new name (F2)").clicked() {
            *action = chosen.map(EditAction::StartRename);
        }
        if ui.add_enabled(is_entry, Button::new("Delete")).on_hover_text("Take the file off the image (Delete)").clicked() {
            *action = chosen.map(EditAction::StartDelete);
        }
        if order {
            if ui.add_enabled(chosen.is_some_and(|i| i > 0), Button::new("\u{25B2}")).on_hover_text("Move the file earlier on the tape").clicked() {
                *action = chosen.map(|i| EditAction::Move(i, -1));
            }
            let count = self.level().entries.len();
            if ui.add_enabled(chosen.is_some_and(|i| i + 1 < count), Button::new("\u{25BC}")).on_hover_text("Move the file later on the tape").clicked() {
                *action = chosen.map(|i| EditAction::Move(i, 1));
            }
        }
        if ui.add_enabled(!self.undo.is_empty(), Button::new("\u{21B6} Undo")).on_hover_text("Put the image back as it was before the last change").clicked() {
            *action = Some(EditAction::Undo);
        }
    }

    /// The strip for an edit in progress, and the panel of files waiting to be added.
    pub(super) fn edit_panels(&mut self, ui: &mut Ui, action: &mut Option<EditAction>) {
        let entries_len = self.level().entries.len();
        let name_of = |browser: &Self, index: usize| browser.level().entries.get(index).map(|e| e.name.clone()).unwrap_or_default();
        let enter = ui.input(|i| i.key_pressed(Key::Enter));
        let escape = ui.input(|i| i.key_pressed(Key::Escape));
        let mut editing = self.editing.take();
        match &mut editing {
            Some(Editing::Rename { index, name }) if *index < entries_len => {
                let old = name_of(self, *index);
                ui.horizontal(|ui| {
                    ui.label(format!("Rename {old} to"));
                    let field = ui.add_sized([240.0, 22.0], egui::TextEdit::singleline(name));
                    field.request_focus();
                    if ui.button("Rename").clicked() || (field.lost_focus() && enter) {
                        *action = Some(EditAction::Finish);
                    }
                    if ui.button("Cancel").clicked() || escape {
                        *action = Some(EditAction::CancelEdit);
                    }
                });
            }
            Some(Editing::Delete { index }) if *index < entries_len => {
                let old = name_of(self, *index);
                ui.horizontal(|ui| {
                    ui.label(RichText::new(format!("Take {old} off the image?")).strong());
                    if ui.button("Delete").clicked() || enter {
                        *action = Some(EditAction::Finish);
                    }
                    if ui.button("Cancel").clicked() || escape {
                        *action = Some(EditAction::CancelEdit);
                    }
                });
            }
            Some(Editing::NewFolder { name }) => {
                ui.horizontal(|ui| {
                    ui.label("New folder called");
                    let field = ui.add_sized([240.0, 22.0], egui::TextEdit::singleline(name));
                    field.request_focus();
                    if ui.button("Make it").clicked() || (field.lost_focus() && enter) {
                        *action = Some(EditAction::Finish);
                    }
                    if ui.button("Cancel").clicked() || escape {
                        *action = Some(EditAction::CancelEdit);
                    }
                });
            }
            _ => editing = None,
        }
        self.editing = editing;
        if self.pending.is_empty() {
            return;
        }
        let kinds = self.abilities().map(|a| a.kinds.clone()).unwrap_or_default();
        egui::Frame::group(ui.style()).show(ui, |ui| {
            ui.label(RichText::new(format!("Adding {} file(s) to the image", self.pending.len())).strong());
            egui::Grid::new("image-add-grid").num_columns(5).spacing([10.0, 4.0]).show(ui, |ui| {
                for item in &mut self.pending {
                    ui.label(RichText::new(&item.from).color(theme::text_dim()));
                    ui.add_sized([180.0, 22.0], egui::TextEdit::singleline(&mut item.addition.name)).on_hover_text("The name it has on the image; it's made to fit the image's rules");
                    if kinds.is_empty() {
                        ui.label("");
                    } else {
                        let about = kinds.iter().find(|k| k.name == item.addition.kind).map_or("", |k| k.about);
                        egui::ComboBox::from_id_salt(("image-add-kind", item.from.as_str())).selected_text(item.addition.kind.as_str()).show_ui(ui, |ui| {
                            for kind in &kinds {
                                ui.selectable_value(&mut item.addition.kind, kind.name.to_string(), kind.name).on_hover_text(kind.about);
                            }
                        }).response.on_hover_text(about);
                    }
                    let kind = kinds.iter().find(|k| k.name == item.addition.kind);
                    ui.horizontal(|ui| {
                        if kind.is_some_and(|k| k.address) {
                            ui.label("at");
                            ui.add_sized([70.0, 22.0], egui::TextEdit::singleline(&mut item.address).hint_text("32768"));
                        }
                        if kind.is_some_and(|k| k.autostart) {
                            ui.label("runs from line");
                            ui.add_sized([60.0, 22.0], egui::TextEdit::singleline(&mut item.autostart).hint_text("none"));
                        }
                    });
                    ui.horizontal(|ui| {
                        if item.clash {
                            ui.checkbox(&mut item.addition.replace, "Replace it");
                        }
                        if let Some(problem) = &item.problem {
                            ui.label(RichText::new(problem).color(theme::rainbow_mistake()).size(11.5));
                        }
                    });
                    ui.end_row();
                }
            });
            ui.horizontal(|ui| {
                if ui.button("Add").clicked() {
                    *action = Some(EditAction::ConfirmAdd);
                }
                if ui.button("Cancel").clicked() {
                    *action = Some(EditAction::CancelAdd);
                }
            });
        });
    }
}

/// The New disk image box: what kind, its name, and where to save it.
pub(crate) struct NewImageBox {
    pub(crate) kind: Blank,
    pub(crate) label: String,
}

impl Limoniter {
    pub(crate) fn open_new_image_box(&mut self) {
        self.dialog = Dialog::NewImage(Box::new(NewImageBox { kind: Blank::D64, label: "NEW DISK".into() }));
    }

    pub(crate) fn new_image_dialog(&mut self, ctx: &Context) -> bool {
        let Dialog::NewImage(state) = &mut self.dialog else {
            return false;
        };
        let mut close = ctx.input(|i| i.key_pressed(Key::Escape));
        let mut go = false;
        egui::Window::new("New disk or tape image").collapsible(false).resizable(false).anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0]).show(ctx, |ui| {
            ui.label(RichText::new("A blank image, formatted as its machine would format it, to add files to.").color(theme::text_dim()));
            ui.add_space(6.0);
            for blank in Blank::ALL {
                ui.radio_value(&mut state.kind, blank, blank.name());
            }
            ui.add_space(6.0);
            let named = !matches!(state.kind, Blank::SpectrumTap | Blank::SpectrumTzx | Blank::Plus3 | Blank::Atari90 | Blank::Atari130 | Blank::CocoSingle);
            ui.add_enabled_ui(named, |ui| {
                ui.horizontal(|ui| {
                    ui.label("Disk name");
                    ui.add(egui::TextEdit::singleline(&mut state.label).desired_width(200.0));
                });
            });
            ui.horizontal(|ui| {
                go = ui.button("Save as\u{2026}").clicked();
                close |= ui.button("Cancel").clicked();
            });
        });
        if go {
            let (kind, label) = (state.kind, state.label.clone());
            let name = format!("{}.{}", if label.trim().is_empty() { "new" } else { label.trim() }, kind.ending());
            let Some(path) = rfd::FileDialog::new().set_title("Save the new image").set_file_name(&name).add_filter(kind.name(), &[kind.ending()]).save_file() else {
                return true;
            };
            self.dialog = Dialog::None;
            match crate::codec::write_file(&path, &kind.make(&label)) {
                Ok(()) => {
                    if !self.open_image(&path) {
                        self.open_path(&path);
                    }
                }
                Err(e) => self.message("New disk or tape image", format!("Couldn't save {}.\n\n{e}", path.display())),
            }
        } else if close {
            self.dialog = Dialog::None;
            self.pending_focus = true;
        }
        true
    }
}

#[cfg(test)]
mod tests {
    use crate::app::tests::{app_with, draw};
    use super::*;

    fn temp(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("limoniter-edit-{}-{name}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn files_added_to_an_image_in_its_tab_are_saved_and_can_be_undone() {
        let dir = temp("add");
        let image = dir.join("work.d64");
        std::fs::write(&image, Blank::D64.make("WORK")).unwrap();
        let program = dir.join("hello.prg");
        std::fs::write(&program, [1, 8, 0, 0]).unwrap();
        let notes = dir.join("notes.seq");
        std::fs::write(&notes, b"TEXT\r").unwrap();
        let (mut app, ctx) = app_with("");
        assert!(app.open_path(&image));
        draw(&mut app, &ctx, Vec::new());
        let browser = app.docs[app.active].browser.as_mut().unwrap();
        assert!(browser.abilities().is_some(), "a D64 can be changed");
        browser.act(EditAction::AddPaths(vec![program.clone(), notes.clone()]));
        assert_eq!(browser.pending.len(), 2);
        assert_eq!(browser.pending[1].addition.kind, "SEQ");
        browser.act(EditAction::ConfirmAdd);
        assert!(browser.pending.is_empty(), "{:?}", browser.pending.iter().map(|p| p.problem.clone()).collect::<Vec<_>>());
        let names: Vec<String> = browser.level().entries.iter().map(|e| e.name.clone()).collect();
        assert_eq!(names, ["HELLO", "NOTES"]);
        // The image on disk has them, not just the tab.
        let saved = media::open_file(&image).unwrap();
        assert_eq!(saved.list(None).unwrap().len(), 2);
        // Adding again finds the name taken, and offers to replace it.
        browser.act(EditAction::AddPaths(vec![program.clone()]));
        browser.act(EditAction::ConfirmAdd);
        assert!(browser.pending[0].clash);
        browser.pending[0].addition.replace = true;
        browser.act(EditAction::ConfirmAdd);
        assert!(browser.pending.is_empty());
        browser.act(EditAction::StartRename(1));
        if let Some(Editing::Rename { name, .. }) = &mut browser.editing {
            name.clear();
            name.push_str("README");
        }
        browser.act(EditAction::Finish);
        browser.act(EditAction::StartDelete(0));
        browser.act(EditAction::Finish);
        let names: Vec<String> = browser.level().entries.iter().map(|e| e.name.clone()).collect();
        assert_eq!(names, ["README"]);
        browser.act(EditAction::Undo);
        browser.act(EditAction::Undo);
        let names: Vec<String> = browser.level().entries.iter().map(|e| e.name.clone()).collect();
        assert_eq!(names, ["HELLO", "NOTES"]);
        assert_eq!(media::open_file(&image).unwrap().list(None).unwrap().len(), 2);
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn a_tape_reorders_and_a_fat_disk_makes_folders_from_its_tab() {
        let dir = temp("tape");
        let tape = dir.join("game.tap");
        std::fs::write(&tape, Blank::SpectrumTap.make("")).unwrap();
        let listing = dir.join("loader.bas");
        std::fs::write(&listing, "10 LOAD \"\" CODE\n20 RANDOMIZE USR 32768\n").unwrap();
        let code = dir.join("game.bin");
        std::fs::write(&code, [0xC9]).unwrap();
        let (mut app, ctx) = app_with("");
        assert!(app.open_path(&tape), "an empty tape opens to add to");
        draw(&mut app, &ctx, Vec::new());
        let browser = app.docs[app.active].browser.as_mut().unwrap();
        assert!(browser.abilities().is_some_and(|a| a.order));
        browser.act(EditAction::AddPaths(vec![code.clone(), listing.clone()]));
        assert_eq!((browser.pending[1].addition.kind.as_str(), browser.pending[1].autostart.as_str()), ("Program", "10"));
        browser.act(EditAction::ConfirmAdd);
        browser.act(EditAction::Move(1, -1));
        let names: Vec<String> = browser.level().entries.iter().map(|e| e.name.clone()).collect();
        assert_eq!(names, ["loader.bas", "game.code"]);
        assert_eq!(browser.selected, Some(0), "the moved file stays chosen");

        let disk = dir.join("pc.img");
        std::fs::write(&disk, Blank::PcFloppy1440.make("WORK")).unwrap();
        assert!(app.open_path(&disk));
        draw(&mut app, &ctx, Vec::new());
        let browser = app.docs[app.active].browser.as_mut().unwrap();
        browser.act(EditAction::StartFolder);
        if let Some(Editing::NewFolder { name }) = &mut browser.editing {
            name.clear();
            name.push_str("Letters");
        }
        browser.act(EditAction::Finish);
        assert!(browser.open(0).is_none(), "the new folder opens in place");
        browser.act(EditAction::AddPaths(vec![listing.clone()]));
        browser.act(EditAction::ConfirmAdd);
        let saved = media::open_file(&disk).unwrap();
        let letters = saved.list(None).unwrap().into_iter().find(|e| e.name == "Letters").unwrap();
        assert_eq!(saved.list(Some(&letters)).unwrap()[0].name, "loader.bas");
        std::fs::remove_dir_all(dir).ok();
    }
}
