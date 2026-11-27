//! The window's side of a review: a tab showing a file as it is beside what someone proposes it should be, with
//! Accept and Reject over it. It's the compare tab (two files side by side, both editable) with the left side
//! locked and a way back to whoever asked. Claude Code's `openDiff` uses it, and so will the agent panel's edits.

use super::*;
use crate::review::{Decision, Review};
use std::sync::mpsc::Receiver;

/// A document's whole text, as the CLI would want to write it: UTF-8, its line endings as they are.
pub(super) fn text_of(doc: &Document) -> String {
    let bytes = doc.text.encode(&crate::codec::TextFormat::default(), Lossy::Replace).unwrap_or_default();
    String::from_utf8_lossy(&bytes).into_owned()
}

impl Limoniter {
    /// Opens a review of `proposed` as the new text of the file at `path` (which needn't exist yet: its left side
    /// is then empty). `label` names the tab. Returns the tab's number and the channel the decision comes on:
    /// Accepted with the text on the right, which the person may have changed; Rejected; or Closed.
    /// Nothing is written: that's for whoever asked to do.
    pub fn open_review(&mut self, path: &Path, proposed: &str, label: &str) -> Result<(u64, Receiver<Decision>), String> {
        let path = std::path::absolute(path).unwrap_or_else(|_| path.to_path_buf());
        let name = short_name(&path);
        let mut left = if path.is_file() {
            if std::fs::metadata(&path).is_ok_and(|meta| meta.len() > DISK_THRESHOLD) {
                return Err(crate::i18n::tf!("{} is too large to review here.", path.display()));
            }
            Document::open(&path, self.encoding_prefs.get(&path))?
        } else {
            Document::untitled(name.clone(), &PRESETS[0])
        };
        if left.hex_mode {
            return Err(crate::i18n::tf!("{} isn't a text file, so a change to it can't be reviewed as text.", path.display()));
        }
        left.set_read_only(true);
        let mut right = Document::new(name);
        right.load_bytes(proposed.as_bytes().to_vec(), &path, Some(crate::codec::TextFormat::default()), None);
        let (review, decisions) = Review::new(label);
        let names = [path.display().to_string(), crate::i18n::tf!("{} (proposed)", path.display())];
        let mut files = FileCompare { sources: [Source2::Own(Box::new(left)), Source2::Own(Box::new(right))], names, diff: DiffView::new(), review: Some(review) };
        if let Some(pair) = two_docs(&mut self.docs, &mut files.sources) {
            files.diff.refresh([&*pair[0], &*pair[1]]);
            if files.diff.difference_count() > 0 {
                files.diff.step(pair, true);
            }
            // The edit is the part to look at, and it's the right side that's changed.
            files.diff.active = 1;
        }
        self.add_compare_tab(label.to_string(), CompareTab::Files(Box::new(files)));
        Ok((self.docs[self.active].id, decisions))
    }

    /// Closes a review's tab, without asking about its changes. Whoever asked is told it was closed, unless
    /// it was decided already.
    pub fn close_review(&mut self, tab: u64) {
        if let Some(index) = self.docs.iter().position(|doc| doc.id == tab && doc.review_of().is_some()) {
            self.remove_tab(index);
        }
    }

    /// The numbers of the review tabs that are open.
    #[cfg_attr(not(test), allow(dead_code))] // For the agent panel, which closes its own.
    pub fn review_tabs(&self) -> Vec<u64> {
        self.docs.iter().filter(|doc| doc.review_of().is_some()).map(|doc| doc.id).collect()
    }

    /// Carries out Accept or Reject on a review tab: tells whoever asked, and closes the tab.
    pub(super) fn decide_review(&mut self, tab: u64, accept: bool) {
        let decided = self.with_compare_tab(tab, |app, tab| {
            let CompareTab::Files(files) = tab else { return false };
            let text = if accept { two_docs(&mut app.docs, &mut files.sources).map(|pair| text_of(pair[1])) } else { None };
            let Some(review) = files.review.as_mut() else { return false };
            match text {
                Some(text) => review.decide(Decision::Accepted(text)),
                None => review.decide(Decision::Rejected),
            }
            true
        });
        if decided == Some(true)
            && let Some(index) = self.docs.iter().position(|doc| doc.id == tab)
        {
            self.remove_tab(index);
        }
    }
}

impl Document {
    /// The review this tab is, if it is one.
    pub(super) fn review_of(&self) -> Option<&Review> {
        match self.compare.as_deref() {
            Some(CompareTab::Files(files)) => files.review.as_ref(),
            _ => None,
        }
    }
}

#[cfg(test)]
#[path = "review_host_tests.rs"]
mod review_host_tests;
