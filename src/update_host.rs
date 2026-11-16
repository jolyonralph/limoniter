//! Asking for a newer Limoniter in the background: once a day by itself when the setting is on, or at once from
//! Help, Check for a newer Limoniter.

use super::{Dialog, Limoniter};
use crate::update_check::{self, Outcome, UpdatedBy};
use std::sync::mpsc::{Receiver, channel};

/// A check under way, and whether the person asked for it.
pub(super) struct UpdateCheck {
    result: Receiver<Result<Outcome, String>>,
    asked: bool,
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

    /// From the menu: checks now, and says what it found.
    pub(super) fn check_updates_now(&mut self) {
        match UpdatedBy::this_copy() {
            UpdatedBy::Store => self.message("Check for a newer Limoniter", format!("This is Limoniter {}, from the Microsoft Store, which brings each new version to it by itself.", update_check::running())),
            UpdatedBy::Flathub => self.message("Check for a newer Limoniter", format!("This is Limoniter {}, from Flathub. Your software centre, or flatpak update, brings each new version.", update_check::running())),
            UpdatedBy::DownloadPage => self.start_update_check(true),
        }
    }

    /// Each frame: starts the day's check when the window first opens, and hears back from one under way.
    pub(super) fn poll_update_check(&mut self) {
        if !self.update_started {
            self.update_started = true;
            if self.prefs.update_checks && self.screenshot.is_none() && self.script.is_none() && !cfg!(test) && UpdatedBy::this_copy() == UpdatedBy::DownloadPage {
                self.start_update_check(false);
            }
        }
        if matches!(self.dialog, Dialog::None)
            && let Some((version, notes)) = self.update_waiting.take()
        {
            self.dialog = Dialog::Update { version, notes };
        }
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
                // By itself, it waits until nothing else is open.
                if asked || matches!(self.dialog, Dialog::None) {
                    self.dialog = Dialog::Update { version, notes };
                } else {
                    self.update_waiting = Some((version, notes));
                }
            }
            Ok(Outcome::UpToDate) if asked => self.message("Check for a newer Limoniter", format!("Limoniter {} is the latest.", update_check::running())),
            Err(problem) if asked => self.message("Check for a newer Limoniter", problem),
            _ => {}
        }
    }
}
