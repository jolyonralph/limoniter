//! A review of a change someone proposes to a file: what Claude Code's `openDiff` shows, and what an agent
//! working in Limoniter's own panel will show for each edit it asks permission for. The window opens a tab with
//! the file as it is on the left and the proposed text, which can be changed, on the right; the person accepts
//! or rejects it, and whoever asked is told on a channel.

use std::sync::mpsc::{Receiver, Sender, channel};

/// What the person decided.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Decision {
    /// Accepted, with the text on the right as it was then (the person may have changed it). Limoniter doesn't
    /// write the file: whoever asked does, with this text.
    Accepted(String),
    Rejected,
    /// The tab was closed (or the program asking went away) without a decision. To be taken as a refusal.
    Closed,
}

/// The way back from a review tab to whoever asked. If it's dropped undecided, which closing the tab does, they're
/// told the tab was closed.
pub struct Review {
    reply: Option<Sender<Decision>>,
    /// What the tab is called, as the one who asked knows it.
    pub label: String,
}

impl Review {
    pub fn new(label: &str) -> (Review, Receiver<Decision>) {
        let (reply, decisions) = channel();
        (Review { reply: Some(reply), label: label.to_string() }, decisions)
    }

    /// Tells the decision. Only the first counts.
    pub fn decide(&mut self, decision: Decision) {
        if let Some(reply) = self.reply.take() {
            let _ = reply.send(decision);
        }
    }

    #[cfg(test)]
    pub fn decided(&self) -> bool {
        self.reply.is_none()
    }
}

impl Drop for Review {
    fn drop(&mut self) {
        self.decide(Decision::Closed);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_first_decision_is_the_one_told() {
        let (mut review, decisions) = Review::new("Edit a.rs");
        review.decide(Decision::Accepted("text".into()));
        review.decide(Decision::Rejected);
        drop(review);
        assert_eq!(decisions.try_iter().collect::<Vec<_>>(), vec![Decision::Accepted("text".into())]);
    }

    #[test]
    fn closing_without_deciding_says_closed() {
        let (review, decisions) = Review::new("x");
        assert!(!review.decided());
        drop(review);
        assert_eq!(decisions.try_recv(), Ok(Decision::Closed));
    }
}
