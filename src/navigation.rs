//! Back and forward through where the cursor has been, across files.

/// How many lines the cursor must jump to count as having gone somewhere else.
const FAR_LINES: usize = 5;
const MAX_PLACES: usize = 100;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Place {
    pub doc: u64,
    pub line: usize,
    pub col: usize,
}

impl Place {
    fn is_far_from(self, other: Place) -> bool {
        self.doc != other.doc || self.line.abs_diff(other.line) >= FAR_LINES
    }
}

#[derive(Default)]
pub struct JumpHistory {
    back: Vec<Place>,
    forward: Vec<Place>,
    last: Option<Place>,
}

impl JumpHistory {
    /// Told where the cursor is now. A big move remembers where it came from, and small ones just update it.
    pub fn note(&mut self, now: Place) {
        if let Some(last) = self.last
            && now.is_far_from(last)
        {
            self.back.push(last);
            if self.back.len() > MAX_PLACES {
                self.back.remove(0);
            }
            self.forward.clear();
        }
        self.last = Some(now);
    }

    pub fn can_go_back(&self) -> bool {
        !self.back.is_empty()
    }

    pub fn can_go_forward(&self) -> bool {
        !self.forward.is_empty()
    }

    pub fn go_back(&mut self, now: Place) -> Option<Place> {
        let target = self.back.pop()?;
        self.forward.push(now);
        self.last = Some(target);
        Some(target)
    }

    pub fn go_forward(&mut self, now: Place) -> Option<Place> {
        let target = self.forward.pop()?;
        self.back.push(now);
        self.last = Some(target);
        Some(target)
    }

    /// Forgets the places in a closed document.
    pub fn forget_doc(&mut self, doc: u64) {
        self.back.retain(|place| place.doc != doc);
        self.forward.retain(|place| place.doc != doc);
        if self.last.is_some_and(|place| place.doc == doc) {
            self.last = None;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(doc: u64, line: usize) -> Place {
        Place { doc, line, col: 0 }
    }

    #[test]
    fn only_real_jumps_are_remembered() {
        let mut h = JumpHistory::default();
        h.note(at(1, 10));
        h.note(at(1, 11));
        h.note(at(1, 12));
        assert!(!h.can_go_back(), "arrowing along isn't a jump");
        h.note(at(1, 500));
        assert!(h.can_go_back());
        assert_eq!(h.go_back(at(1, 500)), Some(at(1, 12)));
        assert_eq!(h.go_forward(at(1, 12)), Some(at(1, 500)));
    }

    #[test]
    fn going_back_then_jumping_drops_the_forward_list() {
        let mut h = JumpHistory::default();
        for line in [0, 100, 200] {
            h.note(at(1, line));
        }
        assert_eq!(h.go_back(at(1, 200)), Some(at(1, 100)));
        assert!(h.can_go_forward());
        h.note(at(1, 100));
        assert!(h.can_go_forward(), "arriving where it was sent isn't a new jump");
        h.note(at(1, 900));
        assert!(!h.can_go_forward());
    }

    #[test]
    fn switching_files_counts_and_closing_one_forgets_it() {
        let mut h = JumpHistory::default();
        h.note(at(1, 5));
        h.note(at(2, 5));
        assert_eq!(h.back, [at(1, 5)]);
        h.forget_doc(1);
        assert!(!h.can_go_back());
    }

    #[test]
    fn the_list_has_a_limit() {
        let mut h = JumpHistory::default();
        for line in 0..500 {
            h.note(at(1, line * 10));
        }
        assert_eq!(h.back.len(), MAX_PLACES);
    }
}
