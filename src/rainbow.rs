//! Rainbow parentheses: works out how deeply each bracket is nested, so each level can have a colour.

use crate::text_buffer::{TextBuffer, next_visual};

/// Past this depth brackets still get a colour, but nesting stops being remembered.
const MAX_DEPTH: usize = 4096;
/// In-memory documents up to this size are scanned from their first line.
const MAX_FULL_SCAN_LINES: usize = 200_000;
/// How far above the view a larger file is scanned, since it can't all be read each time.
const LOOK_BEHIND_LINES: usize = 2000;
const CHECKPOINT_LINES: usize = 256;

fn opener_of(closer: char) -> Option<char> {
    match closer {
        ')' => Some('('),
        ']' => Some('['),
        '}' => Some('{'),
        _ => None,
    }
}

/// Takes one character's part in the nesting, and reports it if it's a bracket.
/// A closing bracket with nothing to match it has no depth, and is drawn as a mistake.
fn scan_one(stack: &mut Vec<u8>, c: char) -> Option<(char, Option<usize>)> {
    if matches!(c, '(' | '[' | '{') {
        let depth = stack.len();
        if depth < MAX_DEPTH {
            stack.push(c as u8);
        }
        return Some((c, Some(depth)));
    }
    let opener = opener_of(c)?;
    // Anything left open inside this pair is passed over, so one slip doesn't recolour the rest.
    match stack.iter().rposition(|open| *open == opener as u8) {
        Some(position) => {
            stack.truncate(position);
            Some((c, Some(position)))
        }
        None => Some((c, None)),
    }
}

/// Goes through a line, calling back with each bracket's visual column, character and depth.
pub fn scan_line(stack: &mut Vec<u8>, line: &str, mut found: impl FnMut(usize, char, Option<usize>)) {
    let mut column = 0;
    for c in line.chars() {
        if let Some((bracket, depth)) = scan_one(stack, c) {
            found(column, bracket, depth);
        }
        column = next_visual(column, c);
    }
}

/// The same, by character position, leaving out the sorted stretches given (strings and comments).
pub fn scan_chars(stack: &mut Vec<u8>, line: &str, skip: &[(usize, usize)], mut found: impl FnMut(usize, char, Option<usize>)) {
    let mut stretches = skip.iter().peekable();
    for (index, c) in line.chars().enumerate() {
        while stretches.peek().is_some_and(|(_, end)| *end <= index) {
            stretches.next();
        }
        if stretches.peek().is_some_and(|(start, _)| *start <= index) {
            continue;
        }
        if let Some((bracket, depth)) = scan_one(stack, c) {
            found(index, bracket, depth);
        }
    }
}

/// Remembers the nesting at every so many lines, until the text changes.
#[derive(Default)]
pub struct RainbowCache {
    version: Option<u64>,
    checkpoints: Vec<Vec<u8>>,
}

impl RainbowCache {
    /// The brackets still open at the start of a line.
    pub fn stack_at(&mut self, buffer: &TextBuffer, line: usize) -> Vec<u8> {
        let (start, mut stack) = if buffer.is_disk_backed() || buffer.line_count() > MAX_FULL_SCAN_LINES {
            (line.saturating_sub(LOOK_BEHIND_LINES), Vec::new())
        } else {
            self.refresh(buffer);
            let index = (line / CHECKPOINT_LINES).min(self.checkpoints.len().saturating_sub(1));
            (index * CHECKPOINT_LINES, self.checkpoints.get(index).cloned().unwrap_or_default())
        };
        for i in start..line.min(buffer.line_count()) {
            scan_line(&mut stack, &buffer.line(i), |_, _, _| {});
        }
        stack
    }

    fn refresh(&mut self, buffer: &TextBuffer) {
        if self.version == Some(buffer.version()) {
            return;
        }
        self.version = Some(buffer.version());
        self.checkpoints.clear();
        let mut stack = Vec::new();
        for i in 0..buffer.line_count() {
            if i % CHECKPOINT_LINES == 0 {
                self.checkpoints.push(stack.clone());
            }
            scan_line(&mut stack, &buffer.line(i), |_, _, _| {});
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn depths(line: &str) -> Vec<(usize, char, Option<usize>)> {
        let mut found = Vec::new();
        scan_line(&mut Vec::new(), line, |column, c, depth| found.push((column, c, depth)));
        found
    }

    #[test]
    fn matching_pairs_share_a_depth() {
        assert_eq!(
            depths("f(a[1], {b})"),
            [(1, '(', Some(0)), (3, '[', Some(1)), (5, ']', Some(1)), (8, '{', Some(1)), (10, '}', Some(1)), (11, ')', Some(0))]
        );
    }

    #[test]
    fn a_closer_with_no_opener_is_a_mistake() {
        assert_eq!(depths(") ]"), [(0, ')', None), (2, ']', None)]);
    }

    #[test]
    fn something_left_open_doesnt_spoil_the_pair_around_it() {
        assert_eq!(depths("([)"), [(0, '(', Some(0)), (1, '[', Some(1)), (2, ')', Some(0))]);
    }

    #[test]
    fn stretches_can_be_left_out() {
        let mut found = Vec::new();
        let mut stack = Vec::new();
        scan_chars(&mut stack, "(a\"(\"b)", &[(2, 5)], |index, c, depth| found.push((index, c, depth)));
        assert_eq!(found, [(0, '(', Some(0)), (6, ')', Some(0))]);
        assert!(stack.is_empty());
    }

    #[test]
    fn columns_allow_for_tabs() {
        assert_eq!(depths("\t("), [(8, '(', Some(0))]);
    }

    #[test]
    fn nesting_carries_across_lines_and_past_checkpoints() {
        let mut b = TextBuffer::default();
        let mut lines = vec!["(".to_string()];
        lines.extend((0..1000).map(|_| "plain".to_string()));
        lines.push("[".to_string());
        lines.extend((0..10).map(|_| "plain".to_string()));
        b.set_content(lines);
        let mut cache = RainbowCache::default();
        assert_eq!(cache.stack_at(&b, 0), Vec::<u8>::new());
        assert_eq!(cache.stack_at(&b, 1), [b'(']);
        assert_eq!(cache.stack_at(&b, 800), [b'(']);
        assert_eq!(cache.stack_at(&b, 1005), [b'(', b'[']);
        b.move_caret(0, 0, false);
        b.delete_forward();
        assert_eq!(cache.stack_at(&b, 800), Vec::<u8>::new(), "an edit is noticed");
    }
}
