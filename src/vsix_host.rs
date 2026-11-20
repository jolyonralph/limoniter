//! The window's side of VS Code extensions: the box where they're found on Open VSX or picked from a file,
//! installed, listed with what each gave, turned off and on, and removed; and keeping the themes, grammars and
//! the rest up to date as they change.

use super::*;
use crate::vsix::{self, Found, Installed};
use std::sync::mpsc::{Receiver, channel};

/// What the background work comes back with.
enum Job {
    Found(Result<Vec<Found>, String>),
    Installed(Result<Box<Installed>, String>),
}

#[derive(Default)]
pub(super) struct ExtensionsBox {
    pub query: String,
    pub found: Vec<Found>,
    /// What's happening or happened last, and whether it went wrong.
    pub status: String,
    pub problem: bool,
    job: Option<Receiver<Job>>,
    pub focus: bool,
    /// The extension asked to be removed, waiting for a yes.
    removing: Option<String>,
}

impl ExtensionsBox {
    fn busy(&self) -> bool {
        self.job.is_some()
    }

    fn start(&mut self, status: String, work: impl FnOnce() -> Job + Send + 'static, ctx: &Context) {
        let (send, receive) = channel();
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            let _ = send.send(work());
            ctx.request_repaint();
        });
        self.job = Some(receive);
        self.status = status;
        self.problem = false;
    }
}

/// Whether what's typed is an extension's id, publisher.name, rather than words to search for.
fn is_id(text: &str) -> bool {
    let text = text.trim();
    text.split_once('.').is_some_and(|(a, b)| !a.is_empty() && !b.is_empty() && !text.contains(' '))
}

impl Limoniter {
    pub(super) fn open_extensions(&mut self) {
        vsix::reload();
        self.dialog = Dialog::Extensions(Box::new(ExtensionsBox { focus: true, ..ExtensionsBox::default() }));
    }

    /// Takes in what's changed when extensions are installed, removed, or turned off or on: their themes now, and
    /// their grammars once they've been loaded again.
    pub(super) fn extensions_changed(&mut self, ctx: &Context) {
        self.themes.take_extensions();
        self.applied_window = None;
        highlight::reload(ctx.clone());
        self.lsp.reconsider();
        self.save_settings_if_changed();
    }

    /// Each frame: once the grammars have been loaded again, every document finds its language afresh (one chosen
    /// by hand keeps it, if it's still there); and themes follow the extensions.
    pub(super) fn follow_extensions(&mut self, _ctx: &Context) {
        if self.extensions_seen.0 != vsix::generation() {
            self.extensions_seen.0 = vsix::generation();
            self.themes.take_extensions();
        }
        let Some(engine) = highlight::engine() else { return };
        let before = self.extensions_seen.1;
        if before.is_some_and(|old| std::ptr::eq(old, engine)) {
            return;
        }
        self.extensions_seen.1 = Some(engine);
        let Some(old) = before else { return };
        for doc in &mut self.docs {
            if let Language::Named(syntax) = doc.language {
                let name = old.name_of(syntax).to_string();
                doc.language = engine.names().iter().find(|(n, _)| *n == name).map_or(Language::Auto, |(_, s)| Language::Named(*s));
            }
            doc.resolved_syntax = None;
            doc.text_state.colours = highlight::LineColours::default();
        }
    }

    fn take_job(&mut self, ctx: &Context) {
        let Dialog::Extensions(state) = &mut self.dialog else { return };
        let Some(job) = state.job.as_ref().and_then(|r| r.try_recv().ok()) else { return };
        state.job = None;
        match job {
            Job::Found(Ok(found)) => {
                state.status = if found.is_empty() { crate::i18n::tr("Nothing on Open VSX matches that.").to_string() } else { String::new() };
                state.found = found;
            }
            Job::Found(Err(problem)) | Job::Installed(Err(problem)) => {
                state.status = problem;
                state.problem = true;
            }
            Job::Installed(Ok(installed)) => {
                state.status = crate::i18n::tf!("{} is installed.", installed.manifest.name);
                // A new extension is on; one installed again keeps being on or off as it was.
                vsix::reload();
                self.extensions_changed(ctx);
            }
        }
    }

    /// The extensions box.
    pub(super) fn extensions_dialog(&mut self, ctx: &Context) -> bool {
        if !matches!(self.dialog, Dialog::Extensions(_)) {
            return false;
        }
        self.take_job(ctx);
        let Dialog::Extensions(state) = &mut self.dialog else { return false };
        let loaded = vsix::current();
        let folder = vsix::folder();
        let mut close = ctx.input(|i| i.key_pressed(Key::Escape));
        let (mut search, mut from_file, mut install_id, mut switch, mut remove, mut icons, mut open_folder) = (false, false, None, None, None, None, false);
        egui::Window::new(crate::i18n::tr("VS Code extensions")).collapsible(false).resizable(true).default_size([720.0, 560.0]).anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0]).show(ctx, |ui| {
            ui.label(RichText::new(crate::i18n::tr("Colour themes, grammars, snippets, file icons and language settings from VS Code extensions, and the language servers they start. An extension's own program (JavaScript) isn't run here: one that's nothing but its program can't be installed, and one that has a program says what's left behind.")).color(theme::text_dim()));
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                let field = ui.add_enabled(!state.busy(), egui::TextEdit::singleline(&mut state.query).hint_text(crate::i18n::tr("Find on Open VSX, or type publisher.name")).desired_width(360.0));
                if state.focus {
                    field.request_focus();
                    state.focus = false;
                }
                let entered = field.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter));
                let typed = !state.query.trim().is_empty();
                search = ui.add_enabled(typed && !state.busy(), Button::new(crate::i18n::tr("Search"))).clicked() || (entered && typed);
                if is_id(&state.query) && ui.add_enabled(!state.busy(), Button::new(crate::i18n::tr("Install this one"))).on_hover_text(crate::i18n::tr("Installs the extension with this id from Open VSX")).clicked() {
                    install_id = Some(state.query.trim().to_string());
                }
                from_file = ui.add_enabled(!state.busy(), Button::new(crate::i18n::tr("Install from a file\u{2026}"))).clicked();
            });
            if !state.status.is_empty() {
                ui.add_space(4.0);
                let colour = if state.problem { theme::rainbow_mistake() } else { theme::text_dim() };
                ui.horizontal_wrapped(|ui| {
                    if state.busy() {
                        ui.spinner();
                    }
                    ui.label(RichText::new(&state.status).color(colour));
                });
            }
            if !state.found.is_empty() {
                ui.add_space(6.0);
                ui.label(RichText::new(crate::i18n::tr("On Open VSX")).strong());
                egui::ScrollArea::vertical().id_salt("open-vsx-found").max_height(170.0).auto_shrink([false, true]).show(ui, |ui| {
                    for found in &state.found {
                        let installed = loaded.extensions.iter().any(|e| e.manifest.id == found.id.to_lowercase());
                        ui.horizontal(|ui| {
                            ui.label(RichText::new(&found.name).strong());
                            ui.label(RichText::new(format!("{}  {}", found.id, found.version)).color(theme::text_faint()).size(11.5));
                            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                let label = if installed { crate::i18n::tr("Install again") } else { crate::i18n::tr("Install") };
                                if ui.add_enabled(!state.busy(), Button::new(label)).clicked() {
                                    install_id = Some(found.id.clone());
                                }
                                ui.label(RichText::new(crate::i18n::tf!("{} downloads", found.downloads)).color(theme::text_faint()).size(11.5));
                            });
                        });
                        if !found.description.is_empty() {
                            ui.label(RichText::new(&found.description).size(12.0));
                        }
                        ui.add_space(3.0);
                    }
                });
            }
            ui.add_space(6.0);
            ui.separator();
            ui.label(RichText::new(crate::i18n::tr("Installed")).strong());
            egui::ScrollArea::vertical().id_salt("extensions-installed").max_height(260.0).auto_shrink([false, true]).show(ui, |ui| {
                if loaded.extensions.is_empty() {
                    ui.label(RichText::new(crate::i18n::tr("No extensions yet. Find one on Open VSX above, or install a .vsix file you have.")).color(theme::text_faint()));
                }
                for extension in &loaded.extensions {
                    let m = &extension.manifest;
                    ui.horizontal(|ui| {
                        let mut on = extension.on;
                        if ui.checkbox(&mut on, RichText::new(&m.name).strong()).on_hover_text(crate::i18n::tr("Turned off, it gives nothing until it's turned on again")).changed() {
                            switch = Some((m.id.clone(), on));
                        }
                        ui.label(RichText::new(format!("{}  {}", m.id, m.version)).color(theme::text_faint()).size(11.5));
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if state.removing.as_deref() == Some(m.id.as_str()) {
                                if ui.small_button(crate::i18n::tr("Keep it")).clicked() {
                                    state.removing = None;
                                }
                                if ui.small_button(RichText::new(crate::i18n::tr("Yes, remove it")).color(theme::rainbow_mistake())).clicked() {
                                    remove = Some(extension.clone());
                                }
                            } else if ui.small_button(crate::i18n::tr("Remove")).on_hover_text(extension.folder.display().to_string()).clicked() {
                                state.removing = Some(m.id.clone());
                            }
                        });
                    });
                    if !m.description.is_empty() {
                        ui.label(RichText::new(&m.description).size(12.0).color(theme::text_dim()));
                    }
                    let quiet = if extension.on { theme::text() } else { theme::text_disabled() };
                    for line in &extension.gave {
                        ui.label(RichText::new(format!("\u{2022} {line}")).size(12.0).color(quiet));
                    }
                    for line in extension.problems.iter().chain(vsix::grammar_problems(&m.id).iter()) {
                        ui.label(RichText::new(line).size(12.0).color(theme::rainbow_mistake()));
                    }
                    ui.add_space(4.0);
                }
            });
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                ui.label(crate::i18n::tr("File icons:"));
                let badges = crate::i18n::tr("Limoniter's own badges").to_string();
                let shown = loaded.icon_choice.as_ref().and_then(|key| loaded.icon_themes.iter().find(|(k, _)| k == key)).map_or(badges.clone(), |(_, label)| label.clone());
                egui::ComboBox::from_id_salt("file-icon-theme").selected_text(shown).width(240.0).show_ui(ui, |ui| {
                    if ui.selectable_label(loaded.icon_choice.is_none(), badges.as_str()).clicked() {
                        icons = Some(None);
                    }
                    for (key, label) in &loaded.icon_themes {
                        if ui.selectable_label(loaded.icon_choice.as_deref() == Some(key.as_str()), label.as_str()).clicked() {
                            icons = Some(Some(key.clone()));
                        }
                    }
                });
            });
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                if folder.is_some() && ui.button(crate::i18n::tr("Open the extensions folder")).clicked() {
                    open_folder = true;
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.button(crate::i18n::tr("Close")).clicked() {
                        close = true;
                    }
                });
            });
        });
        let Some(folder) = folder else {
            if close {
                self.close_dialog();
            }
            return true;
        };
        if search {
            let query = state.query.trim().to_string();
            state.found.clear();
            state.start(crate::i18n::tf!("Looking on Open VSX for \u{201C}{query}\u{201D}\u{2026}"), move || Job::Found(vsix::search(&query)), ctx);
        }
        if let Some(id) = install_id {
            let target = folder.clone();
            state.start(crate::i18n::tf!("Downloading {id} from Open VSX\u{2026}"), move || Job::Installed(vsix::download(&id).and_then(|bytes| vsix::install(bytes, &target)).map(Box::new)), ctx);
        }
        if from_file
            && let Some(path) = rfd::FileDialog::new().set_title(crate::i18n::tr("Install a VS Code extension")).add_filter(crate::i18n::tr("VS Code extensions"), &["vsix"]).pick_file()
        {
            let target = folder.clone();
            state.start(crate::i18n::tf!("Installing {}\u{2026}", path.display()), move || Job::Installed(std::fs::read(&path).map_err(|e| crate::i18n::tf!("{} couldn't be read: {e}", path.display())).and_then(|bytes| vsix::install(bytes, &target)).map(Box::new)), ctx);
        }
        if let Some((id, on)) = switch {
            vsix::set_on(&id, on);
            self.extensions_changed(ctx);
        }
        if let Some(key) = icons {
            vsix::set_icon_theme(key.as_deref());
            self.save_settings_if_changed();
        }
        if let Some(extension) = remove {
            if let Dialog::Extensions(state) = &mut self.dialog {
                state.removing = None;
                match vsix::remove(&extension) {
                    Ok(()) => state.status = crate::i18n::tf!("{} is removed.", extension.manifest.name),
                    Err(problem) => {
                        state.status = problem;
                        state.problem = true;
                    }
                }
            }
            vsix::reload();
            self.extensions_changed(ctx);
        }
        if open_folder {
            let _ = std::fs::create_dir_all(&folder);
            if let Err(problem) = crate::platform::open_folder(&folder) {
                self.message("VS Code extensions", problem);
            }
        }
        if close {
            self.close_dialog();
        }
        true
    }

    /// Screenshot steps: the box, with words in its search box; and a file icon theme chosen by its key.
    pub(super) fn extensions_screenshot_step(&mut self, name: &str, value: &str) {
        match name {
            "extensions" => {
                self.open_extensions();
                if let Dialog::Extensions(state) = &mut self.dialog {
                    state.query = value.to_string();
                    state.focus = false;
                }
            }
            "icon-theme" => vsix::set_icon_theme(Some(value).filter(|v| !v.is_empty())),
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_id_is_told_from_words_to_search_for() {
        assert!(is_id("dracula-theme.theme-dracula"));
        assert!(!is_id("dracula theme"));
        assert!(!is_id("dracula."));
        assert!(!is_id("x86 asm.vsix files"));
    }
}
