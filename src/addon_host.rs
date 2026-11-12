//! The window's side of add-ons: running their commands and events, the panels they show, and the box where
//! they're turned on and off.

use super::script_host::After;
use super::*;
use crate::addons::{Event, State};

/// An add-on function waiting its turn, for a document.
pub(super) struct Queued {
    pub doc_id: u64,
    pub name: String,
    pub path: PathBuf,
    pub function: String,
    pub after: Option<After>,
}

/// Text an add-on has shown in a panel of its own.
pub(super) struct AddOnPanel {
    pub title: String,
    pub text: String,
}

const MOST_PANEL_BYTES: usize = 8 * 1024 * 1024;

impl Limoniter {
    /// Queues every add-on's function for something that's happened to a document.
    pub(super) fn queue_hooks(&mut self, event: Event, doc_id: u64) -> bool {
        self.addons.refresh_if_stale();
        let hooks = self.addons.hooks(event);
        let any = !hooks.is_empty();
        for (name, path, function) in hooks {
            self.addon_queue.push_back(Queued { doc_id, name, path, function, after: None });
        }
        any
    }

    /// Save, with the add-ons' on_save functions run first. A script saving a file doesn't set them off.
    pub(super) fn save_with_hooks(&mut self, index: usize) {
        let doc = &self.docs[index];
        let id = doc.id;
        if self.script.is_some() || doc.hex_mode || doc.browser.is_some() || doc.text.read_only || !self.queue_hooks(Event::Save, id) {
            self.save(index);
            return;
        }
        if let Some(last) = self.addon_queue.back_mut() {
            last.after = Some(After::Save(id));
        }
    }

    /// Starts the next queued add-on function, once nothing else is running.
    pub(super) fn start_queued_addon(&mut self, ui: &Ui) {
        if self.script.is_some() || !matches!(self.dialog, Dialog::None) || self.pending.is_some() {
            return;
        }
        let Some(queued) = self.addon_queue.pop_front() else {
            return;
        };
        // It works on the document it was for; if that's closed, it's skipped, but what was to follow still happens.
        let Some(index) = self.docs.iter().position(|doc| doc.id == queued.doc_id) else {
            return;
        };
        self.active = index;
        self.play_function(&queued.name, &queued.path, &queued.function, queued.after, ui);
    }

    /// Runs a command an add-on adds, from the menus or the command box.
    pub(super) fn run_addon_command(&mut self, number: usize, ui: &Ui) {
        let Some(listed) = self.addons.commands().into_iter().nth(number) else {
            return;
        };
        self.play_function(&format!("{}: {}", listed.addon, listed.title), &listed.path, &listed.function, None, ui);
    }

    pub(super) fn show_addon_panel(&mut self, title: String, text: &str, add: bool) {
        let panel = match self.addon_panels.iter_mut().position(|panel| panel.title == title) {
            Some(index) => &mut self.addon_panels[index],
            None => {
                self.addon_panels.push(AddOnPanel { title, text: String::new() });
                self.addon_panels.last_mut().unwrap()
            }
        };
        if !add {
            panel.text.clear();
        }
        panel.text += text;
        if panel.text.len() > MOST_PANEL_BYTES {
            let mut cut = panel.text.len() - MOST_PANEL_BYTES;
            while !panel.text.is_char_boundary(cut) {
                cut += 1;
            }
            panel.text.drain(..cut);
        }
    }

    /// The add-ons' own panels, each a window that stays until it's closed.
    pub(super) fn addon_panels_ui(&mut self, ctx: &Context) {
        let mut closed = Vec::new();
        for (index, panel) in self.addon_panels.iter().enumerate() {
            let mut open = true;
            egui::Window::new(&panel.title).id(egui::Id::unique(("addon-panel", &panel.title))).open(&mut open).default_size([460.0, 320.0]).resizable(true).show(ctx, |ui| {
                ui.horizontal(|ui| {
                    if ui.small_button("Copy all").clicked() {
                        ui.ctx().copy_text(panel.text.clone());
                    }
                });
                ui.separator();
                egui::ScrollArea::both().auto_shrink([false, false]).show(ui, |ui| {
                    ui.add(egui::Label::new(RichText::new(&panel.text).monospace()).wrap_mode(egui::TextWrapMode::Extend).selectable(true));
                });
            });
            if !open {
                closed.push(index);
            }
        }
        for index in closed.into_iter().rev() {
            self.addon_panels.remove(index);
        }
    }

    /// The box listing every add-on, to turn each on or off, and to set the shared folder.
    pub(super) fn addons_dialog(&mut self, ctx: &Context) -> bool {
        let Dialog::AddOns { shared } = &mut self.dialog else {
            return false;
        };
        self.addons.refresh_if_stale();
        let mut close = ctx.input(|i| i.key_pressed(Key::Escape));
        let mut switch = None;
        let mut edit = None;
        let mut new_one = false;
        let mut use_shared = None;
        let mut open_folder = None;
        egui::Window::new("Add-ons").collapsible(false).resizable(true).default_size([620.0, 480.0]).anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0]).show(ctx, |ui| {
            ui.label("Add-ons add commands to Tools \u{203A} Add-ons and the command box, and can run when a file is opened or saved. They can do what a script can, and no more: nothing outside the open files, and no network. A new add-on is off until you turn it on here, and one whose file changes is turned off until you look at it again.");
            ui.add_space(6.0);
            egui::ScrollArea::vertical().max_height(300.0).show(ui, |ui| {
                if self.addons.list.is_empty() {
                    ui.label(RichText::new("There are no add-ons yet. New add-on makes one to start from.").color(theme::text_faint()));
                }
                for addon in &self.addons.list {
                    let state = self.addons.state(addon);
                    ui.horizontal(|ui| {
                        let mut on = state == State::On;
                        let response = ui.add_enabled(addon.problem.is_none(), egui::Checkbox::new(&mut on, RichText::new(&addon.name).strong()));
                        if response.changed() {
                            switch = Some((addon.path.clone(), on));
                        }
                        if addon.shared {
                            ui.label(RichText::new("shared").color(theme::text_faint()).size(11.5));
                        }
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if ui.small_button("Edit").on_hover_text(addon.path.display().to_string()).clicked() {
                                edit = Some(addon.path.clone());
                            }
                        });
                    });
                    if !addon.about.is_empty() {
                        ui.label(&addon.about);
                    }
                    let commands: Vec<String> = addon.commands.iter().map(|c| format!("\u{201C}{}\u{201D}", c.title)).collect();
                    let mut runs = Vec::new();
                    if addon.on_open.is_some() {
                        runs.push("when a file is opened");
                    }
                    if addon.on_save.is_some() {
                        runs.push("before a file is saved");
                    }
                    let mut says = Vec::new();
                    if !commands.is_empty() {
                        says.push(format!("Adds {}.", commands.join(", ")));
                    }
                    if !runs.is_empty() {
                        says.push(format!("Runs {}.", runs.join(" and ")));
                    }
                    if !says.is_empty() {
                        ui.label(RichText::new(says.join(" ")).color(theme::text_faint()).size(12.0));
                    }
                    if state == State::Changed {
                        ui.colored_label(theme::rainbow_mistake(), "Its file has changed since it was turned on, so it's off until you turn it on again.");
                    }
                    if let Some(problem) = &addon.problem {
                        ui.colored_label(theme::rainbow_mistake(), problem);
                    }
                    ui.separator();
                }
            });
            ui.add_space(4.0);
            ui.horizontal(|ui| {
                ui.label("Shared add-ons folder:");
                ui.add(egui::TextEdit::singleline(shared).hint_text("None").desired_width(300.0));
                if ui.button("Browse\u{2026}").clicked()
                    && let Some(folder) = rfd::FileDialog::new().set_title("Shared add-ons folder").pick_folder()
                {
                    *shared = folder.display().to_string();
                }
                if ui.button("Use").clicked() {
                    use_shared = Some(shared.trim().to_string());
                }
            });
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                if ui.button("New add-on\u{2026}").clicked() {
                    new_one = true;
                }
                if let Some(folder) = self.addons.own_folder()
                    && ui.button("Open the add-ons folder").clicked()
                {
                    open_folder = Some(folder.to_path_buf());
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.button("Close").clicked() {
                        close = true;
                    }
                });
            });
        });
        if let Some((path, on)) = switch {
            self.addons.set_on(&path, on);
            self.save_settings_if_changed();
        }
        if let Some(folder) = use_shared {
            self.addons.set_shared((!folder.is_empty()).then(|| PathBuf::from(folder)));
            self.save_settings_if_changed();
        }
        if let Some(folder) = open_folder {
            let _ = std::fs::create_dir_all(&folder);
            if let Err(problem) = crate::platform::open_folder(&folder) {
                self.message("Add-ons", problem);
            }
        }
        if let Some(path) = edit {
            self.close_dialog();
            self.open_path(&path);
            return true;
        }
        if new_one {
            self.close_dialog();
            self.dialog = Dialog::Script(script_host::ScriptBox::new_addon());
            return true;
        }
        if close {
            self.close_dialog();
        }
        true
    }

    /// Makes a new add-on from the example and opens it to be written.
    pub(super) fn create_addon(&mut self, name: &str) -> Result<(), String> {
        let path = self.addons.create(name)?;
        self.open_path(&path);
        Ok(())
    }
}
