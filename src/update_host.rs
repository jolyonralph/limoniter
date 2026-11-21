//! Asking for a newer Limoniter in the background: once a day by itself when the setting is on, or at once from
//! Help, Check for a newer Limoniter. A copy that can update itself (see self_update.rs) offers to install it, or
//! downloads it in the background and says when it's ready to restart into; others offer the download page.

use super::{Dialog, Limoniter};
use crate::self_update::{self, Install, Progress};
use crate::theme;
use crate::update_check::{self, Outcome, UpdatedBy};
use eframe::egui::{self, Context, Key, RichText, ViewportCommand};
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::sync::mpsc::{Receiver, channel};

/// A check under way, and whether the person asked for it.
pub(super) struct UpdateCheck {
    result: Receiver<Result<Outcome, String>>,
    asked: bool,
}

/// A newer version, offered in its window, and how far its update has got.
pub(super) struct Offer {
    pub version: String,
    pub notes: String,
    pub stage: Stage,
    /// How this copy updates itself, or why it can't, worked out when it's offered.
    pub how: Result<Install, String>,
}

pub(super) enum Stage {
    /// Install now, download it in the background, or the download page.
    Offered,
    /// Being downloaded: the window shows how far (the download itself is in `update_download`).
    Downloading,
    /// Downloaded and checked, ready to restart into.
    Ready(PathBuf),
    /// It couldn't be downloaded or checked.
    Failed(String),
}

/// A download under way, which goes on while its window is hidden.
pub(super) struct Download {
    version: String,
    notes: String,
    install: Install,
    progress: Arc<Progress>,
    result: Receiver<Result<PathBuf, String>>,
    /// Install now was chosen: it restarts into the new version as soon as it's checked, unless there's unsaved
    /// work by then.
    restart: bool,
}

/// How this copy can be updated, or why it can't update itself: the Store's and Flathub's are updated by them,
/// and the rest as self_update works out.
fn how_to_update() -> Result<Install, String> {
    match UpdatedBy::this_copy() {
        UpdatedBy::Store | UpdatedBy::Flathub => Err(String::new()),
        UpdatedBy::DownloadPage if self_update::key_is_placeholder() => Err(crate::i18n::tr("This build of Limoniter has no key to check updates with yet, so it doesn't update itself: download the new version from the page.").into()),
        UpdatedBy::DownloadPage => self_update::this_install(),
    }
}

/// What a size is, in megabytes to one place.
fn megabytes(bytes: u64) -> String {
    format!("{:.1}", bytes as f64 / (1024.0 * 1024.0))
}

impl Limoniter {
    fn start_update_check(&mut self, asked: bool) {
        if self.update_check.is_some() {
            return;
        }
        let (send, result) = channel();
        std::thread::spawn(move || {
            let _ = send.send(update_check::check(asked));
        });
        self.update_check = Some(UpdateCheck { result, asked });
    }

    /// From the menu: checks now, and says what it found. A version already downloaded is offered at once.
    pub(super) fn check_updates_now(&mut self) {
        if let Some(download) = &self.update_download {
            let version = download.version.clone();
            let notes = download.notes.clone();
            let how = Ok(download.install.clone());
            self.dialog = Dialog::Update(Box::new(Offer { version, notes, stage: Stage::Downloading, how }));
            return;
        }
        match UpdatedBy::this_copy() {
            UpdatedBy::Store => self.message("Check for a newer Limoniter", crate::i18n::tf!("This is Limoniter {}, from the Microsoft Store, which brings each new version to it by itself.", update_check::running())),
            UpdatedBy::Flathub => self.message("Check for a newer Limoniter", crate::i18n::tf!("This is Limoniter {}, from Flathub. Your software centre, or flatpak update, brings each new version.", update_check::running())),
            UpdatedBy::DownloadPage => self.start_update_check(true),
        }
    }

    /// Shows a newer version's offer, or keeps it until nothing else is open, if it came by itself.
    fn offer(&mut self, offer: Offer, asked: bool) {
        if asked || matches!(self.dialog, Dialog::None) {
            self.dialog = Dialog::Update(Box::new(offer));
        } else {
            self.update_waiting = Some(offer);
        }
    }

    /// Each frame: starts the day's check when the window first opens, and hears back from one under way and from
    /// a download.
    pub(super) fn poll_update_check(&mut self, ctx: &Context) {
        if !self.update_started {
            self.update_started = true;
            self.after_update();
            if self.prefs.update_checks && self.screenshot.is_none() && self.script.is_none() && !cfg!(test) && UpdatedBy::this_copy() == UpdatedBy::DownloadPage {
                self.start_update_check(false);
            }
        }
        if matches!(self.dialog, Dialog::None)
            && let Some(offer) = self.update_waiting.take()
        {
            self.dialog = Dialog::Update(Box::new(offer));
        }
        self.poll_download(ctx);
        let Some(check) = &self.update_check else {
            return;
        };
        let Ok(result) = check.result.try_recv() else {
            return;
        };
        let asked = check.asked;
        self.update_check = None;
        match result {
            Ok(Outcome::Newer { version, notes }) => {
                // With the setting on, a copy that can update itself downloads it first, and says when it's ready.
                if !asked
                    && self.prefs.update_download
                    && let Ok(install) = how_to_update()
                {
                    self.start_download(version, notes, install, false);
                } else {
                    self.offer(Offer { version, notes, stage: Stage::Offered, how: how_to_update() }, asked);
                }
            }
            Ok(Outcome::UpToDate) if asked => self.message("Check for a newer Limoniter", crate::i18n::tf!("Limoniter {} is the latest.", update_check::running())),
            Err(problem) if asked => self.message("Check for a newer Limoniter", problem),
            _ => {}
        }
    }

    fn start_download(&mut self, version: String, notes: String, install: Install, restart: bool) {
        if self.update_download.is_some() {
            return;
        }
        let Some(folder) = self_update::updates_folder() else {
            let offer = Offer { version, notes, stage: Stage::Failed(crate::i18n::tr("There's no folder to keep Limoniter's files in.").into()), how: Ok(install) };
            return self.offer(offer, true);
        };
        let progress = Arc::new(Progress::default());
        let (send, result) = channel();
        let (thread_progress, thread_install, thread_version) = (progress.clone(), install.clone(), version.clone());
        std::thread::spawn(move || {
            let folder = folder.join(&thread_version);
            let _ = send.send(self_update::download(&thread_install, &thread_version, &folder, self_update::PUBLIC_KEY, &self_update::fetch_from_web, &thread_progress));
        });
        self.update_download = Some(Download { version, notes, install, progress, result, restart });
    }

    fn poll_download(&mut self, ctx: &Context) {
        let Some(download) = &self.update_download else {
            return;
        };
        let Ok(result) = download.result.try_recv() else {
            return;
        };
        let Some(download) = self.update_download.take() else {
            return;
        };
        let stopped = download.progress.cancel.load(Ordering::Relaxed);
        let showing = matches!(&self.dialog, Dialog::Update(offer) if offer.version == download.version);
        if showing {
            self.dialog = Dialog::None;
        }
        match result {
            // Install now: straight into it, unless there's unsaved work by now, when it waits, ready.
            Ok(file) if download.restart && self.unsaved_names().is_empty() => {
                if let Err(problem) = self.restart_into(ctx, &download.install, &file) {
                    self.offer(Offer { version: download.version, notes: download.notes, stage: Stage::Failed(problem), how: Ok(download.install) }, true);
                }
            }
            Ok(file) => self.offer(Offer { version: download.version, notes: download.notes, stage: Stage::Ready(file), how: Ok(download.install) }, showing),
            Err(_) if stopped => {}
            Err(problem) => self.offer(Offer { version: download.version, notes: download.notes, stage: Stage::Failed(problem), how: Ok(download.install) }, showing),
        }
    }

    /// The names of the files with unsaved changes, which an update mustn't lose.
    fn unsaved_names(&self) -> Vec<String> {
        self.docs.iter().filter(|d| d.is_modified()).map(|d| d.name()).collect()
    }

    /// The files open, to open again in the new version.
    fn open_files(&self) -> Vec<PathBuf> {
        self.docs.iter().filter_map(|d| d.path.clone()).collect()
    }

    /// Puts the downloaded version in place and restarts into it, unless there's unsaved work.
    fn restart_into(&mut self, ctx: &Context, install: &Install, file: &std::path::Path) -> Result<(), String> {
        let unsaved = self.unsaved_names();
        if !unsaved.is_empty() {
            return Err(crate::i18n::tf!("Save or close {} first: Limoniter restarts to update, and nothing unsaved is ever lost to it.", unsaved.join(", ")));
        }
        let updates = self_update::updates_folder().ok_or_else(|| crate::i18n::tr("There's no folder to keep Limoniter's files in.").to_string())?;
        let version = file.parent().and_then(|f| f.file_name()).map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        let relaunch = self_update::install(install, file, &version, update_check::running(), &updates, &self.open_files())?;
        relaunch.start()?;
        self.allow_close = true;
        ctx.send_viewport_cmd(ViewportCommand::Close);
        Ok(())
    }

    /// The window for a newer version, at each stage of its update. True while it's showing.
    pub(super) fn update_dialog(&mut self, ctx: &Context) -> bool {
        let Dialog::Update(offer) = &self.dialog else {
            return false;
        };
        let unsaved = self.unsaved_names();
        let progress = self.update_download.as_ref().filter(|d| d.version == offer.version).map(|d| (d.progress.done.load(Ordering::Relaxed), d.progress.total.load(Ordering::Relaxed)));
        let (mut install_now, mut background, mut page, mut close, mut restart, mut stop) = (false, false, false, false, false, false);
        let enter = ctx.input(|i| i.key_pressed(Key::Enter));
        crate::fit::Dialog::new(crate::i18n::tr("A newer Limoniter")).collapsible(false).resizable(false).anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0]).show(ctx, |ui| {
            ui.set_max_width(540.0);
            let version = &offer.version;
            match &offer.stage {
                Stage::Offered => {
                    ui.label(crate::i18n::tf!("Limoniter {version} is out. This is {}.", update_check::running()));
                    if !offer.notes.is_empty() {
                        ui.add_space(6.0);
                        egui::ScrollArea::vertical().max_height(260.0).max_width(520.0).show(ui, |ui| ui.label(RichText::new(offer.notes.as_str()).color(theme::text_dim())));
                    }
                    ui.add_space(6.0);
                    match &offer.how {
                        Ok(_) => {
                            ui.label(RichText::new(crate::i18n::tr("It's downloaded from GitHub and checked against the release's signature before anything changes. The version you have now is kept, so you can go back to it from the Help menu.")).size(12.5).color(theme::text_dim()));
                            ui.add_space(6.0);
                            ui.horizontal(|ui| {
                                install_now = ui.button(crate::i18n::tr("Install now")).on_hover_text(crate::i18n::tr("Downloads it, then restarts into it with your files open again")).clicked() || enter;
                                background = ui.button(crate::i18n::tr("Download in the background")).on_hover_text(crate::i18n::tr("Carries on working, and says when it's ready to restart into")).clicked();
                                page = ui.button(crate::i18n::tr("Open the download page")).clicked();
                                close |= ui.button(crate::i18n::tr("Not now")).clicked();
                            });
                        }
                        Err(why) => {
                            if !why.is_empty() {
                                ui.label(RichText::new(why.as_str()).size(12.5).color(theme::text_dim()));
                                ui.add_space(6.0);
                            }
                            ui.horizontal(|ui| {
                                page = ui.button(crate::i18n::tr("Open the download page")).clicked() || enter;
                                close |= ui.button(crate::i18n::tr("Not now")).clicked();
                            });
                        }
                    }
                }
                Stage::Downloading => {
                    ui.label(crate::i18n::tf!("Downloading Limoniter {version}\u{2026}"));
                    let (done, total) = progress.unwrap_or((0, 0));
                    let fraction = if total > 0 { done as f32 / total as f32 } else { 0.0 };
                    let words = if total > 0 { crate::i18n::tf!("{} of {} MB", megabytes(done), megabytes(total)) } else { crate::i18n::tf!("{} MB", megabytes(done)) };
                    ui.add(egui::ProgressBar::new(fraction).desired_width(420.0).text(words));
                    ui.add_space(6.0);
                    ui.horizontal(|ui| {
                        close |= ui.button(crate::i18n::tr("Hide")).on_hover_text(crate::i18n::tr("The download carries on, and Limoniter says when it's ready")).clicked();
                        stop = ui.button(crate::i18n::tr("Stop the download")).clicked();
                    });
                    ctx.request_repaint_after(std::time::Duration::from_millis(250));
                }
                Stage::Ready(_) => {
                    ui.label(crate::i18n::tf!("Limoniter {version} has been downloaded and checked, and is ready to restart into."));
                    ui.label(RichText::new(crate::i18n::tr("The files you have open come back after it restarts. The version you have now is kept, so you can go back to it from the Help menu.")).size(12.5).color(theme::text_dim()));
                    if !unsaved.is_empty() {
                        ui.add_space(4.0);
                        ui.label(RichText::new(crate::i18n::tf!("Save or close these first: {}", unsaved.join(", "))).color(theme::ruler_mark()));
                    }
                    ui.add_space(6.0);
                    ui.horizontal(|ui| {
                        restart = ui.add_enabled(unsaved.is_empty(), egui::Button::new(crate::i18n::tr("Restart now"))).clicked() || (enter && unsaved.is_empty());
                        close |= ui.button(crate::i18n::tr("Later")).on_hover_text(crate::i18n::tr("It stays ready: Help, Check for a newer Limoniter offers it again")).clicked();
                    });
                }
                Stage::Failed(problem) => {
                    ui.label(crate::i18n::tf!("Limoniter {version} couldn't be installed: {problem}"));
                    ui.add_space(6.0);
                    ui.horizontal(|ui| {
                        page = ui.button(crate::i18n::tr("Open the download page")).clicked() || enter;
                        close |= ui.button(crate::i18n::tr("Close")).clicked();
                    });
                }
            }
        });
        close |= ctx.input(|i| i.key_pressed(Key::Escape));
        let Dialog::Update(offer) = std::mem::replace(&mut self.dialog, Dialog::None) else {
            return true;
        };
        let offer = *offer;
        if page {
            if let Err(problem) = crate::platform::open_web_page(update_check::DOWNLOAD_PAGE) {
                self.message("A newer Limoniter", problem);
            }
        } else if install_now || background {
            if let Ok(install) = &offer.how {
                self.start_download(offer.version.clone(), offer.notes.clone(), install.clone(), install_now);
            }
            if install_now {
                self.dialog = Dialog::Update(Box::new(Offer { stage: Stage::Downloading, ..offer }));
            }
        } else if stop {
            if let Some(download) = &self.update_download {
                download.progress.cancel.store(true, Ordering::Relaxed);
            }
        } else if restart {
            if let (Stage::Ready(file), Ok(install)) = (&offer.stage, &offer.how)
                && let Err(problem) = self.restart_into(ctx, install, file)
            {
                self.message("A newer Limoniter", problem);
            }
        } else if !close {
            self.dialog = Dialog::Update(Box::new(offer));
        }
        true
    }

    /// For a picture: the newer-version window at a stage ("offered", "downloading", "ready" or "failed"), for an
    /// AppImage, with a file left unsaved for "ready".
    pub(super) fn update_screenshot_step(&mut self, value: &str) {
        let install = Install::AppImage { file: "/home/me/Limoniter.AppImage".into() };
        let file = PathBuf::from("/home/me/.config/limoniter/updates/0.15.0/Limoniter-0.15.0-x86_64.AppImage");
        let stage = match value {
            "downloading" => {
                let (_send, result) = channel();
                let progress = Arc::new(Progress::default());
                progress.total.store(41 * 1024 * 1024, Ordering::Relaxed);
                progress.done.store(17 * 1024 * 1024, Ordering::Relaxed);
                self.update_download = Some(Download { version: "0.15.0".into(), notes: String::new(), install: install.clone(), progress, result, restart: true });
                Stage::Downloading
            }
            "ready" => {
                self.doc().text.type_text(" ");
                Stage::Ready(file)
            }
            "failed" => Stage::Failed(crate::i18n::tr("The update's signature doesn't match it, so it wasn't installed.").into()),
            _ => Stage::Offered,
        };
        let notes = "Limoniter 0.15.0 updates itself, and comes as a portable download for Windows.\n\n- The 68000 assembler warns where vasm does.".to_string();
        self.dialog = Dialog::Update(Box::new(Offer { version: "0.15.0".into(), notes, stage, how: Ok(install) }));
    }

    /// Whether there's a previous version kept to go back to.
    pub(super) fn can_go_back(&self) -> bool {
        self_update::updates_folder().is_some_and(|updates| self_update::kept_version(&updates).is_some())
    }

    /// Help, Go back to the previous version: after asking, and never with unsaved work.
    pub(super) fn go_back_version(&mut self, ctx: &Context) {
        let title = "Go back to the previous version";
        let Some(updates) = self_update::updates_folder() else { return };
        let Some(version) = self_update::kept_version(&updates) else {
            return self.message(title, crate::i18n::tr("No previous version of Limoniter has been kept.").into());
        };
        let unsaved = self.unsaved_names();
        if !unsaved.is_empty() {
            return self.message(title, crate::i18n::tf!("Save or close {} first: Limoniter restarts to go back, and nothing unsaved is ever lost to it.", unsaved.join(", ")));
        }
        let install = match self_update::this_install() {
            Ok(install) => install,
            Err(why) => return self.message(title, why),
        };
        if !super::ask_yes_no(title, &crate::i18n::tf!("Go back to Limoniter {version}? This version, {}, is kept in its place, so you can come back to it the same way.", update_check::running())) {
            return;
        }
        let gone = self_update::roll_back(&install, &updates, update_check::running(), &self.open_files()).and_then(|relaunch| relaunch.start());
        match gone {
            Ok(()) => {
                self.allow_close = true;
                ctx.send_viewport_cmd(ViewportCommand::Close);
            }
            Err(problem) => self.message(title, problem),
        }
    }

    /// When this was just started by an update (or by going back): says so, or that it didn't finish, and clears
    /// away the download.
    fn after_update(&mut self) {
        let Some((from, to)) = self_update::updated().cloned() else {
            return;
        };
        // What the copy that finished it on Windows couldn't do, if anything, written beside its plan.
        let problems = self_update::updates_folder().and_then(|updates| {
            let places = std::iter::once(updates.clone()).chain(std::fs::read_dir(&updates).into_iter().flatten().flatten().map(|entry| entry.path()));
            let found = places.filter_map(|place| std::fs::read_to_string(place.join("finish-update.problems.txt")).ok()).next();
            self_update::tidy(&updates);
            found
        });
        let words = match (update_check::running() == to, problems) {
            (true, _) if update_check::newer(&to, &from) => crate::i18n::tf!("Limoniter is now {to}, updated from {from}. Help, Go back to the previous version returns to {from}."),
            (true, _) => crate::i18n::tf!("Limoniter is back to {to}. Help, Go back to the previous version returns to {from}."),
            (false, Some(problem)) => crate::i18n::tf!("The update to {to} didn't finish, so this is still Limoniter {}: {problem}", update_check::running()),
            (false, None) => crate::i18n::tf!("The update to {to} didn't finish, so this is still Limoniter {}.", update_check::running()),
        };
        self.message("A newer Limoniter", words);
    }
}

#[cfg(test)]
mod tests {
    use super::super::Command;
    use super::super::tests::{app_with, draw};
    use super::*;
    use std::path::Path;

    /// What the window shows: drawn twice, since a new window is only measured the first time.
    fn window_words(app: &mut Limoniter, ctx: &Context) -> String {
        draw(app, ctx, Vec::new());
        words(&draw(app, ctx, Vec::new()))
    }

    /// Every piece of text a frame drew.
    fn words(output: &egui::FullOutput) -> String {
        fn gather(shape: &egui::Shape, into: &mut String) {
            match shape {
                egui::Shape::Text(text) => {
                    into.push_str(text.galley.text());
                    into.push('\n');
                }
                egui::Shape::Vec(shapes) => shapes.iter().for_each(|s| gather(s, into)),
                _ => {}
            }
        }
        let mut into = String::new();
        output.shapes.iter().for_each(|clipped| gather(&clipped.shape, &mut into));
        into
    }

    fn appimage() -> Install {
        Install::AppImage { file: "/nowhere/Limoniter.AppImage".into() }
    }

    fn offered(stage: Stage, how: Result<Install, String>) -> Dialog {
        Dialog::Update(Box::new(Offer { version: "9.9.9".into(), notes: "New: everything.".into(), stage, how }))
    }

    /// A download under way, as the thread would give it, and the end to send it.
    fn downloading(restart: bool) -> (Download, std::sync::mpsc::Sender<Result<PathBuf, String>>) {
        let (send, result) = channel();
        let progress = Arc::new(Progress::default());
        progress.total.store(40 * 1024 * 1024, Ordering::Relaxed);
        progress.done.store(10 * 1024 * 1024, Ordering::Relaxed);
        (Download { version: "9.9.9".into(), notes: "New: everything.".into(), install: appimage(), progress, result, restart }, send)
    }

    fn stage(app: &Limoniter) -> Option<&Stage> {
        match &app.dialog {
            Dialog::Update(offer) => Some(&offer.stage),
            _ => None,
        }
    }

    #[test]
    fn a_copy_that_can_update_itself_is_offered_install_now_and_others_the_download_page() {
        let (mut app, ctx) = app_with("");
        app.dialog = offered(Stage::Offered, Ok(appimage()));
        let shown = window_words(&mut app, &ctx);
        for expected in ["Limoniter 9.9.9 is out", "New: everything.", "Install now", "Download in the background", "Open the download page", "Not now", "checked against the release's signature"] {
            assert!(shown.contains(expected), "{expected} in {shown}");
        }
        app.dialog = offered(Stage::Offered, Err("This is the portable Limoniter, which doesn't change itself.".into()));
        let shown = window_words(&mut app, &ctx);
        assert!(shown.contains("portable Limoniter") && shown.contains("Open the download page") && !shown.contains("Install now"), "{shown}");
        // While the key is the placeholder, every copy is only told, and says why.
        if self_update::key_is_placeholder() {
            assert!(how_to_update().unwrap_err().contains("no key"));
        }
    }

    #[test]
    fn a_download_shows_how_far_it_has_got_then_is_ready_to_restart_into() {
        let (mut app, ctx) = app_with("");
        let (download, send) = downloading(false);
        app.update_download = Some(download);
        app.dialog = offered(Stage::Downloading, Ok(appimage()));
        let shown = window_words(&mut app, &ctx);
        assert!(shown.contains("Downloading Limoniter 9.9.9") && shown.contains("10.0 of 40.0 MB") && shown.contains("Stop the download"), "{shown}");
        // Asking again while it downloads shows it, rather than checking again.
        app.dialog = Dialog::None;
        app.check_updates_now();
        assert!(matches!(stage(&app), Some(Stage::Downloading)));
        assert!(app.update_check.is_none());
        send.send(Ok(PathBuf::from("/nowhere/updates/9.9.9/Limoniter-9.9.9-x86_64.AppImage"))).unwrap();
        let shown = window_words(&mut app, &ctx);
        assert!(app.update_download.is_none());
        assert!(matches!(stage(&app), Some(Stage::Ready(_))));
        assert!(shown.contains("ready to restart into") && shown.contains("Restart now") && shown.contains("Later"), "{shown}");
    }

    #[test]
    fn nothing_restarts_while_there_is_unsaved_work() {
        let (mut app, ctx) = app_with("");
        app.doc().text.type_text("not saved yet");
        // Install now, finished: it waits, ready, naming what's unsaved, rather than restarting.
        let (download, send) = downloading(true);
        app.update_download = Some(download);
        send.send(Ok(PathBuf::from("/nowhere/updates/9.9.9/Limoniter-9.9.9-x86_64.AppImage"))).unwrap();
        draw(&mut app, &ctx, Vec::new());
        assert!(matches!(stage(&app), Some(Stage::Ready(_))));
        let shown = window_words(&mut app, &ctx);
        assert!(shown.contains("Save or close these first"), "{shown}");
        let refused = app.restart_into(&ctx, &appimage(), Path::new("/nowhere/updates/9.9.9/Limoniter-9.9.9-x86_64.AppImage")).unwrap_err();
        assert!(refused.contains("Save or close"), "{refused}");
        assert!(matches!(stage(&app), Some(Stage::Ready(_))), "still ready, for once it's saved");
    }

    #[test]
    fn a_failed_download_says_why_and_a_stopped_one_says_nothing() {
        let (mut app, ctx) = app_with("");
        let (download, send) = downloading(false);
        app.update_download = Some(download);
        send.send(Err("The update's signature doesn't match it, so it wasn't installed.".into())).unwrap();
        let shown = window_words(&mut app, &ctx);
        assert!(matches!(stage(&app), Some(Stage::Failed(_))));
        assert!(shown.contains("couldn't be installed: The update's signature doesn't match it") && shown.contains("Open the download page"), "{shown}");
        app.dialog = Dialog::None;
        let (download, send) = downloading(false);
        download.progress.cancel.store(true, Ordering::Relaxed);
        app.update_download = Some(download);
        send.send(Err("The download was stopped.".into())).unwrap();
        draw(&mut app, &ctx, Vec::new());
        assert!(app.update_download.is_none() && matches!(app.dialog, Dialog::None));
    }

    #[test]
    fn with_no_previous_version_kept_there_is_nothing_to_go_back_to() {
        let (app, _ctx) = app_with("");
        assert!(!app.can_go_back());
        assert!(!app.can(Command::GoBackVersion));
    }
}
