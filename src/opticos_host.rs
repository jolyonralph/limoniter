//! Asking for a newer OpticOS ROM in the background: once a day by itself when the setting is on, or at once from
//! Help, Check for a newer OpticOS ROM.

use super::{Dialog, Limoniter};
use crate::opticos_rom::{self, Outcome};
use std::sync::mpsc::{Receiver, channel};

/// A check under way, and whether the person asked for it.
pub(super) struct OpticosCheck {
    result: Receiver<Result<Outcome, String>>,
    asked: bool,
}

impl Limoniter {
    fn start_opticos_check(&mut self, asked: bool) {
        if self.opticos_check.is_some() {
            return;
        }
        let (send, result) = channel();
        std::thread::spawn(move || {
            let _ = send.send(opticos_rom::check(asked));
        });
        self.opticos_check = Some(OpticosCheck { result, asked });
    }

    /// From the menu: checks now, and says what it found.
    pub(super) fn check_opticos_now(&mut self) {
        self.start_opticos_check(true);
    }

    /// Each frame: starts the day's check when the window first opens, and hears back from one under way.
    pub(super) fn poll_opticos_check(&mut self) {
        if !self.opticos_started {
            self.opticos_started = true;
            if self.prefs.opticos_updates && self.screenshot.is_none() && self.script.is_none() {
                self.start_opticos_check(false);
            }
        }
        let Some(check) = &self.opticos_check else {
            return;
        };
        let Ok(result) = check.result.try_recv() else {
            return;
        };
        let asked = check.asked;
        self.opticos_check = None;
        let words = match result {
            Ok(outcome) => match (&outcome, asked) {
                // By itself, it only speaks up when something's changed.
                (Outcome::Installed(_) | Outcome::Unusable(..), _) | (_, true) => outcome.message(),
                _ => None,
            },
            Err(problem) => asked.then_some(problem),
        };
        // Not over something the person is in the middle of: a check that finds a new ROM still installs it.
        if let Some(words) = words
            && (asked || matches!(self.dialog, Dialog::None))
        {
            self.message("OpticOS", words);
        }
    }
}
