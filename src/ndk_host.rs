//! The Amiga NDK box (Tools, Amiga NDK, or from Settings): what's in use for Amiga code hints, and, when the
//! person asks, downloading Hyperion's NDK from Aminet, using one already on the computer, or removing it.

use super::{Dialog, Limoniter, theme};
use crate::amiga_ndk;
use eframe::egui::{self, Button, Context, Key};
use std::sync::mpsc::{Receiver, channel};

pub(super) struct NdkBox {
    status: String,
    working: Option<Receiver<Result<usize, String>>>,
}

/// What's in use, in a sentence.
fn in_use() -> String {
    match (amiga_ndk::current(), amiga_ndk::chosen_folder()) {
        (Some(index), _) => format!("In use: {} functions and {} autodoc pages, from {}.", index.functions.len(), index.docs.len(), index.folder.display()),
        (None, Some(folder)) => format!("There's an NDK in {}, but it couldn't be read.", folder.display()),
        (None, None) => "No NDK yet, so Amiga code has no hints.".into(),
    }
}

impl Limoniter {
    pub(super) fn open_ndk_box(&mut self) {
        self.dialog = Dialog::AmigaNdk(Box::new(NdkBox { status: String::new(), working: None }));
    }

    pub(super) fn ndk_dialog(&mut self, ctx: &Context) -> bool {
        let Dialog::AmigaNdk(state) = &mut self.dialog else {
            return false;
        };
        if let Some(rx) = &state.working {
            match rx.try_recv() {
                Ok(Ok(files)) => {
                    let count = amiga_ndk::set_folder(amiga_ndk::chosen_folder().as_deref()).ok().flatten().unwrap_or(0);
                    state.status = format!("Downloaded and unpacked: {files} files, {count} functions.");
                    state.working = None;
                }
                Ok(Err(problem)) => {
                    state.status = problem;
                    state.working = None;
                }
                Err(_) => ctx.request_repaint_after(std::time::Duration::from_millis(100)),
            }
        }
        let mut close = ctx.input(|i| i.key_pressed(Key::Escape));
        let (mut download, mut choose, mut remove) = (false, false, false);
        let busy = state.working.is_some();
        egui::Window::new(crate::i18n::tr("Amiga NDK")).collapsible(false).resizable(false).anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0]).show(ctx, |ui| {
            ui.set_max_width(520.0);
            ui.label(crate::i18n::tr("With Hyperion's Native Developer Kit for AmigaOS 3.2, resting the pointer on a system function in 68000 or C source shows its prototype, library, offset (_LVO), the registers its arguments go in, and its autodoc; typing _LVO offers the functions. Its headers' structure fields and flags (LN_NAME, MEMF_CHIP) are shown and offered too, and Limoniter's own 68000 assembler finds its headers and _LVO offsets."));
            ui.add_space(4.0);
            ui.label(crate::i18n::tr("The NDK is Hyperion's and free to download, but it isn't Limoniter's to include, so Limoniter only fetches it from Aminet when you ask (about 8 MB, 31 MB unpacked, kept beside the settings)."));
            ui.add_space(6.0);
            ui.strong(in_use());
            if busy {
                ui.horizontal(|ui| {
                    ui.spinner();
                    ui.label(crate::i18n::tr("Downloading from Aminet and unpacking\u{2026}"));
                });
            } else if !state.status.is_empty() {
                ui.colored_label(theme::change_unsaved(), &state.status);
            }
            ui.add_space(4.0);
            ui.horizontal(|ui| {
                download = ui.add_enabled(!busy, Button::new(crate::i18n::tr("Download from Aminet"))).on_hover_text(amiga_ndk::ADDRESS).clicked();
                choose = ui.add_enabled(!busy, Button::new(crate::i18n::tr("Use a folder\u{2026}"))).on_hover_text(crate::i18n::tr("An NDK already on this computer: the folder holding its SFD and Autodocs folders")).clicked();
                remove = ui.add_enabled(!busy && amiga_ndk::chosen_folder().is_some(), Button::new(crate::i18n::tr("Remove"))).on_hover_text(crate::i18n::tr("Stops using it, and deletes the downloaded copy (a folder you chose is left alone)")).clicked();
                close |= ui.button(crate::i18n::tr("Close")).clicked();
            });
        });
        if download {
            match amiga_ndk::download_folder() {
                Some(folder) => {
                    let (tx, rx) = channel();
                    std::thread::spawn(move || {
                        let _ = tx.send(amiga_ndk::download(&folder));
                    });
                    // A folder chosen by hand gives way to the download.
                    let _ = amiga_ndk::choose(None);
                    state.status.clear();
                    state.working = Some(rx);
                }
                None => state.status = "There's nowhere to keep it: Limoniter's settings folder couldn't be found.".into(),
            }
        } else if choose {
            if let Some(folder) = rfd::FileDialog::new().set_title(crate::i18n::tr("The NDK's folder")).pick_folder() {
                state.status = match amiga_ndk::choose(Some(&folder)) {
                    Ok(_) => String::new(),
                    Err(problem) => problem,
                };
            }
        } else if remove {
            if let Some(folder) = amiga_ndk::download_folder() {
                let _ = std::fs::remove_dir_all(folder);
            }
            let _ = amiga_ndk::choose(None);
            state.status = "Removed.".into();
        } else if close && !busy {
            self.dialog = Dialog::None;
            self.pending_focus = true;
        }
        true
    }
}
