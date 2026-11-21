//! The window's side of a project from a ZIP or a web address (see `project_import`): File, Open a ZIP as a
//! project asks for the ZIP and the folder to unpack it in; Open a project from a web address asks for the address
//! and the folder, then fetches it in the background. Either way the new folder opens as a project.

use super::*;
use std::sync::mpsc::{Receiver, channel};

pub(super) struct WebProjectBox {
    address: String,
    status: String,
    fetching: Option<Receiver<Result<(PathBuf, String), String>>>,
}

impl Limoniter {
    /// A ZIP chosen, unpacked into a folder of its own in the folder chosen, and opened as a project.
    pub(super) fn open_zip_project(&mut self) {
        let Some(zip) = rfd::FileDialog::new().set_title(crate::i18n::tr("Open a ZIP as a project")).add_filter(crate::i18n::tr("ZIP archive"), &["zip"]).pick_file() else {
            return;
        };
        let mut picker = rfd::FileDialog::new().set_title(crate::i18n::tr("Where to unpack it (a folder of its own is made in it)"));
        if let Some(beside) = zip.parent() {
            picker = picker.set_directory(beside);
        }
        let Some(parent) = picker.pick_folder() else {
            return;
        };
        let name = zip.file_stem().map_or_else(|| "project".to_string(), |s| crate::media::tidy_name(&s.to_string_lossy()));
        match std::fs::read(&zip).map_err(|e| e.to_string()).and_then(|bytes| crate::project_import::unpack(bytes, &parent, &name)) {
            Ok(folder) => {
                self.open_folder(&folder);
                self.print_status = crate::i18n::tf!("{} is unpacked into {}, and open as a project.", zip.display(), folder.display());
            }
            Err(problem) => self.message("Open a ZIP as a project", problem),
        }
    }

    pub(super) fn open_web_project_box(&mut self) {
        self.dialog = Dialog::WebProject(Box::new(WebProjectBox { address: String::new(), status: String::new(), fetching: None }));
    }

    /// The box asking for the address, while it's open; what's fetched opens as a project when it's done.
    pub(super) fn web_project_dialog(&mut self, ctx: &Context) -> bool {
        let Dialog::WebProject(state) = &mut self.dialog else {
            return false;
        };
        if let Some(rx) = &state.fetching {
            match rx.try_recv() {
                Ok(Ok((folder, said))) => {
                    self.dialog = Dialog::None;
                    self.open_folder(&folder);
                    self.print_status = crate::i18n::tf!("{said} It's in {}, open as a project.", folder.display());
                    return true;
                }
                Ok(Err(problem)) => {
                    state.status = problem;
                    state.fetching = None;
                }
                Err(_) => ctx.request_repaint_after(std::time::Duration::from_millis(100)),
            }
        }
        let mut close = ctx.input(|i| i.key_pressed(Key::Escape));
        let mut go = false;
        crate::fit::Dialog::new(crate::i18n::tr("Open a project from a web address")).collapsible(false).resizable(false).anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0]).show(ctx, |ui| {
            ui.label(crate::i18n::tr("A ZIP's address, or a Git repository's (GitHub, GitLab, Codeberg or any other):"));
            let field = ui.add(egui::TextEdit::singleline(&mut state.address).desired_width(460.0).hint_text(crate::i18n::tr("https://github.com/owner/project")));
            if state.status.is_empty() && state.fetching.is_none() {
                field.request_focus();
            }
            go = field.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter));
            ui.label(RichText::new(crate::i18n::tr("A repository is cloned with Git; without Git, one on GitHub, GitLab or Codeberg is downloaded as a ZIP instead.")).color(theme::text_faint()).size(12.0));
            if state.fetching.is_some() {
                ui.horizontal(|ui| {
                    ui.spinner();
                    ui.label(crate::i18n::tr("Fetching\u{2026}"));
                });
            } else if !state.status.is_empty() {
                ui.colored_label(theme::change_unsaved(), &state.status);
            }
            ui.horizontal(|ui| {
                go |= ui.add_enabled(state.fetching.is_none(), Button::new(crate::i18n::tr("Choose where, and open"))).clicked();
                close |= ui.button(crate::i18n::tr("Cancel")).clicked();
            });
        });
        if go && state.fetching.is_none() {
            let address = state.address.trim().to_string();
            if crate::project_import::source_of(&address).is_none() {
                state.status = crate::i18n::tr("Give a web address starting with http:// or https://.").into();
            } else if let Some(parent) = rfd::FileDialog::new().set_title(crate::i18n::tr("Where to put it (a folder of its own is made in it)")).pick_folder() {
                let (tx, rx) = channel();
                let ctx = ctx.clone();
                std::thread::spawn(move || {
                    let _ = tx.send(crate::project_import::fetch(&address, &parent));
                    ctx.request_repaint();
                });
                state.status.clear();
                state.fetching = Some(rx);
            }
        } else if close && state.fetching.is_none() {
            self.dialog = Dialog::None;
            self.pending_focus = true;
        }
        true
    }
}
