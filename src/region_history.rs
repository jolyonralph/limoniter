//! The history of one part of a file, such as a function: which of the kept states changed it, wherever it was at
//! the time, and how to take back one of those changes without touching anything done elsewhere since.
//!
//! Each step from one kept state to the next is compared line by line. A part is followed back through the steps by
//! where its lines were, so lines added or taken away above it don't lose it. A block moved within one step is
//! followed to where it came from, and so is one cut in one step and put back somewhere else in a later one.

use similar::{Algorithm, DiffOp, capture_diff_slices_deadline};
use std::ops::Range;
use std::sync::Arc;
use std::time::{Duration, Instant};

/// How far back a part that was put in is looked for, in case it was cut from somewhere first.
const STEPS_TO_FIND_A_CUT: usize = 40;
/// Moves are only looked for between this many changes in one step, which is plenty for an edit someone made.
const MOST_CHANGES_FOR_MOVES: usize = 64;
const MOST_MOVED_LINES: usize = 20_000;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Op {
    /// The same lines before and after.
    Equal { old: usize, new: usize, len: usize },
    /// Lines that went, and lines that came in their place. Either can be none.
    Change { old: usize, old_len: usize, new: usize, new_len: usize },
}

/// Lines that were taken away from one place and put in another, in the same step.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Move {
    pub old: usize,
    pub new: usize,
    pub len: usize,
}

/// What one step did to the lines.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Step {
    pub ops: Vec<Op>,
    pub moves: Vec<Move>,
}

/// Where some lines went, and whether the step changed or moved any of them.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Mapped {
    pub range: Range<usize>,
    pub touched: bool,
    pub moved: bool,
}

pub fn diff(before: &[Arc<str>], after: &[Arc<str>]) -> Step {
    let found = capture_diff_slices_deadline(Algorithm::Myers, before, after, Some(Instant::now() + Duration::from_millis(400)));
    let mut ops: Vec<Op> = Vec::with_capacity(found.len());
    for op in found {
        let (old, old_len, new, new_len) = match op {
            DiffOp::Equal { old_index, new_index, len } => {
                ops.push(Op::Equal { old: old_index, new: new_index, len });
                continue;
            }
            DiffOp::Delete { old_index, old_len, new_index } => (old_index, old_len, new_index, 0),
            DiffOp::Insert { old_index, new_index, new_len } => (old_index, 0, new_index, new_len),
            DiffOp::Replace { old_index, old_len, new_index, new_len } => (old_index, old_len, new_index, new_len),
        };
        // A deletion next to an insertion is one change.
        if let Some(Op::Change { old: o, old_len: ol, new: n, new_len: nl }) = ops.last_mut()
            && *o + *ol == old
            && *n + *nl == new
        {
            *ol += old_len;
            *nl += new_len;
            continue;
        }
        ops.push(Op::Change { old, old_len, new, new_len });
    }
    let moves = find_moves(&ops, before, after);
    Step { ops, moves }
}

/// Blocks that went from one change and turned up whole in another.
fn find_moves(ops: &[Op], before: &[Arc<str>], after: &[Arc<str>]) -> Vec<Move> {
    let changes: Vec<(usize, usize, usize, usize)> = ops
        .iter()
        .filter_map(|op| match *op {
            Op::Change { old, old_len, new, new_len } => Some((old, old_len, new, new_len)),
            Op::Equal { .. } => None,
        })
        .collect();
    let mut moves = Vec::new();
    if changes.len() < 2 || changes.len() > MOST_CHANGES_FOR_MOVES {
        return moves;
    }
    for (index, &(old, old_len, _, _)) in changes.iter().enumerate() {
        let block = &before[old..old + old_len];
        if old_len == 0 || old_len > MOST_MOVED_LINES || block.iter().all(|line| line.trim().is_empty()) {
            continue;
        }
        for (other, &(_, _, new, new_len)) in changes.iter().enumerate() {
            if other == index || new_len < old_len {
                continue;
            }
            let landing = &after[new..new + new_len];
            if let Some(at) = landing.windows(old_len).position(|window| window == block) {
                moves.push(Move { old, new: new + at, len: old_len });
                break;
            }
        }
    }
    moves
}

impl Step {
    /// The same step, the other way round.
    pub fn reversed(&self) -> Step {
        let ops = self
            .ops
            .iter()
            .map(|op| match *op {
                Op::Equal { old, new, len } => Op::Equal { old: new, new: old, len },
                Op::Change { old, old_len, new, new_len } => Op::Change { old: new, old_len: new_len, new: old, new_len: old_len },
            })
            .collect();
        let moves = self.moves.iter().map(|m| Move { old: m.new, new: m.old, len: m.len }).collect();
        Step { ops, moves }
    }

    fn moved(&self, line: usize) -> Option<usize> {
        self.moves.iter().find(|m| m.old <= line && line < m.old + m.len).map(|m| m.new + line - m.old)
    }

    /// Where some lines are after the step. Lines that were changed bring the whole of what replaced them.
    pub fn map_range(&self, r: Range<usize>) -> Mapped {
        let (mut lo, mut hi) = (usize::MAX, 0);
        let (mut touched, mut moved) = (false, false);
        let mut take = |a: usize, b: usize| {
            lo = lo.min(a);
            hi = hi.max(b);
        };
        for op in &self.ops {
            match *op {
                Op::Equal { old, new, len } => {
                    let (a, b) = (r.start.max(old), r.end.min(old + len));
                    if a < b {
                        take(new + a - old, new + b - old);
                    }
                }
                Op::Change { old, old_len: 0, new, new_len } => {
                    if r.start < old && old < r.end {
                        take(new, new + new_len);
                        touched = true;
                    }
                }
                Op::Change { old, old_len, new, new_len } => {
                    let (a, b) = (r.start.max(old), r.end.min(old + old_len));
                    if a >= b {
                        continue;
                    }
                    touched = true;
                    let mut rest = false;
                    for line in a..b {
                        match self.moved(line) {
                            Some(to) => {
                                take(to, to + 1);
                                moved = true;
                            }
                            None => rest = true,
                        }
                    }
                    if rest {
                        take(new, new + new_len);
                    }
                }
            }
        }
        if lo == usize::MAX {
            let point = self.map_point(r.start);
            return Mapped { range: point..point, touched, moved };
        }
        Mapped { range: lo..hi, touched, moved }
    }

    /// Where a place between lines is after the step, as near as it can be said.
    pub fn map_point(&self, p: usize) -> usize {
        let mut end = 0;
        for op in &self.ops {
            match *op {
                Op::Equal { old, new, len } => {
                    if old <= p && p <= old + len {
                        return new + p - old;
                    }
                    end = new + len;
                }
                Op::Change { old, old_len, new, new_len } => {
                    if old <= p && p <= old + old_len {
                        return if p == old + old_len && old_len > 0 { new + new_len } else { new };
                    }
                    end = new + new_len;
                }
            }
        }
        end
    }

    /// Where some lines are after the step, only if the step left every one of them as it was. Lines between two
    /// others count as left alone only if nothing was put between them.
    pub fn map_exact(&self, r: Range<usize>) -> Option<Range<usize>> {
        if r.is_empty() {
            return self.map_point_exact(r.start).map(|p| p..p);
        }
        for op in &self.ops {
            if let Op::Equal { old, new, len } = *op
                && old <= r.start
                && r.end <= old + len
            {
                return Some(new + r.start - old..new + r.end - old);
            }
        }
        self.moves.iter().find(|m| m.old <= r.start && r.end <= m.old + m.len).map(|m| m.new + r.start - m.old..m.new + r.end - m.old)
    }

    /// Where a place between lines is after the step, unless lines around it were changed.
    pub fn map_point_exact(&self, p: usize) -> Option<usize> {
        for op in &self.ops {
            match *op {
                Op::Equal { old, new, len } if old <= p && p <= old + len => return Some(new + p - old),
                Op::Change { old, old_len, .. } if old < p && p < old + old_len => return None,
                _ => {}
            }
        }
        Some(self.map_point(p))
    }

    /// The changes that reach into some lines after the step: each as the lines it took away and those it left.
    pub fn changes_in(&self, after: &Range<usize>) -> Vec<(Range<usize>, Range<usize>)> {
        self.ops
            .iter()
            .filter_map(|op| match *op {
                Op::Change { old, old_len, new, new_len } => {
                    let reaches = if new_len == 0 { after.start < new && new < after.end } else { new < after.end && after.start < new + new_len };
                    reaches.then_some((old..old + old_len, new..new + new_len))
                }
                Op::Equal { .. } => None,
            })
            .collect()
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    /// Some of its lines were changed.
    Changed,
    /// It was moved, in one go.
    Moved,
    /// It was put in, and wasn't found anywhere before.
    Written,
    /// It was put in, having been cut from somewhere else earlier.
    MovedHere,
    /// It was cut from here, to be put in somewhere else later.
    CutFromHere,
}

/// One step that did something to the part.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Touch {
    /// Which step: from state `step` to state `step + 1`.
    pub step: usize,
    /// Where the part was before the step, and after it. None where it wasn't in the file.
    pub before: Option<Range<usize>>,
    pub after: Option<Range<usize>>,
    pub kind: Kind,
}

/// Every step that did something to these lines of the last state, newest first. `steps[i]` takes state `i` to
/// state `i + 1`, and `lines_of` gives a state's lines, which are only asked for when a cut is being looked for.
pub fn touches(steps: &[Arc<Step>], lines_of: &dyn Fn(usize) -> Option<Vec<Arc<str>>>, region: Range<usize>) -> Vec<Touch> {
    let mut out: Vec<Touch> = Vec::new();
    let mut here = Some(region);
    // What was put in, being looked for in what was cut before, and for how many more steps.
    let mut looking: Option<(Vec<Arc<str>>, usize)> = None;
    for step in (0..steps.len()).rev() {
        if let Some(r) = here.clone() {
            let back = steps[step].reversed().map_range(r.clone());
            if !back.touched {
                here = Some(back.range);
                continue;
            }
            if back.range.is_empty() && !r.is_empty() {
                out.push(Touch { step, before: Some(back.range), after: Some(r.clone()), kind: Kind::Written });
                let content = lines_of(step + 1).and_then(|lines| lines.get(r).map(<[Arc<str>]>::to_vec));
                looking = content.filter(|lines| lines.iter().any(|line| !line.trim().is_empty())).map(|lines| (lines, STEPS_TO_FIND_A_CUT));
                here = None;
                continue;
            }
            let kind = if back.moved && back.range.len() == r.len() { Kind::Moved } else { Kind::Changed };
            out.push(Touch { step, before: Some(back.range.clone()), after: Some(r), kind });
            here = Some(back.range);
        } else if let Some((content, left)) = &mut looking {
            if let Some(found) = cut_in(&steps[step], &lines_of(step), content) {
                if let Some(written) = out.last_mut() {
                    written.kind = Kind::MovedHere;
                }
                out.push(Touch { step, before: Some(found.clone()), after: None, kind: Kind::CutFromHere });
                here = Some(found);
                looking = None;
                continue;
            }
            *left -= 1;
            if *left == 0 {
                break;
            }
        } else {
            break;
        }
    }
    out
}

/// Where these lines were taken away in a step, if they were.
fn cut_in(step: &Step, before: &Option<Vec<Arc<str>>>, content: &[Arc<str>]) -> Option<Range<usize>> {
    let before = before.as_ref()?;
    step.ops.iter().find_map(|op| match *op {
        Op::Change { old, old_len, .. } if old_len >= content.len() => {
            let taken = before.get(old..old + old_len)?;
            taken.windows(content.len()).position(|window| window == content).map(|at| old + at..old + at + content.len())
        }
        _ => None,
    })
}

/// Follows lines from one state on through later steps, only if none of them changed it.
pub fn follow_exactly(steps: &[Arc<Step>], mut r: Range<usize>) -> Option<Range<usize>> {
    for step in steps {
        r = step.map_exact(r)?;
    }
    Some(r)
}

/// What taking back just one step's changes to a part means in the text as it is now: each place to change, as
/// lines now and what goes there instead, last first so that each can be done in turn. Err if a later step changed
/// the same lines again, so that this one can't be taken back alone.
pub fn undo_one(steps: &[Arc<Step>], touch: &Touch, before_lines: &[Arc<str>]) -> Result<Vec<(Range<usize>, Vec<Arc<str>>)>, String> {
    let Some(after) = &touch.after else {
        return Err("This is where the part was cut from. Taking back where it was put in does the same.".into());
    };
    let mut edits = Vec::new();
    for (old, new) in steps[touch.step].changes_in(after) {
        let now = follow_exactly(&steps[touch.step + 1..], new).ok_or_else(|| "Some of the lines this changed have been changed again since. Take back the later change first, or put the part back as it was before this.".to_string())?;
        let lines = before_lines.get(old).ok_or("That state is no longer kept.")?.to_vec();
        edits.push((now, lines));
    }
    if edits.is_empty() {
        return Err("This change left nothing to take back here.".into());
    }
    edits.sort_by_key(|(range, _)| std::cmp::Reverse(range.start));
    Ok(edits)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lines(text: &str) -> Vec<Arc<str>> {
        text.split('\n').map(Arc::from).collect()
    }

    /// The states, and the steps between them.
    fn history(states: &[&str]) -> (Vec<Vec<Arc<str>>>, Vec<Arc<Step>>) {
        let states: Vec<Vec<Arc<str>>> = states.iter().map(|s| lines(s)).collect();
        let steps = states.windows(2).map(|pair| Arc::new(diff(&pair[0], &pair[1]))).collect();
        (states, steps)
    }

    fn apply(text: &[Arc<str>], edits: Vec<(Range<usize>, Vec<Arc<str>>)>) -> Vec<Arc<str>> {
        let mut text = text.to_vec();
        for (range, with) in edits {
            text.splice(range, with);
        }
        text
    }

    const ONE: &str = "fn one() {\n    a();\n}";
    const TWO: &str = "fn two() {\n    b();\n}";

    #[test]
    fn a_part_is_followed_past_lines_added_and_taken_away_above_it() {
        let (states, steps) = history(&[
            &format!("{ONE}\n\n{TWO}"),
            &format!("{ONE}\n\n{TWO}").replace("b();", "b(1);"),
            &format!("// a comment\n// and another\n{ONE}\n\n{TWO}").replace("b();", "b(1);"),
            &format!("// a comment\n// and another\n{ONE}\n\n{TWO}").replace("b();", "b(1);").replace("a();", "a(2);"),
        ]);
        let last = states.len() - 1;
        let two = 6..9;
        assert_eq!(&*states[last][two.start], "fn two() {");
        let found = touches(&steps, &|i| states.get(i).cloned(), two);
        assert_eq!(found.len(), 1, "only the step that changed fn two, not the comments or fn one: {found:?}");
        assert_eq!(found[0].step, 0);
        assert_eq!(found[0].kind, Kind::Changed);
        assert_eq!(found[0].before, Some(4..7), "where it was before the comments went in");
        let one = touches(&steps, &|i| states.get(i).cloned(), 2..5);
        assert_eq!(one.iter().map(|t| t.step).collect::<Vec<_>>(), [2]);
    }

    #[test]
    fn one_change_is_taken_back_and_later_changes_elsewhere_stay() {
        let (states, steps) = history(&[
            &format!("{ONE}\n\n{TWO}"),
            &format!("{ONE}\n\n{TWO}").replace("a();", "a(1);"),
            &format!("{ONE}\n\n{TWO}").replace("a();", "a(1);").replace("b();", "b(2);"),
            &format!("// new\n{ONE}\n\n{TWO}").replace("a();", "a(1);").replace("b();", "b(2);"),
        ]);
        let now = states.last().unwrap();
        let found = touches(&steps, &|i| states.get(i).cloned(), 1..4);
        assert_eq!(found.len(), 1);
        let edits = undo_one(&steps, &found[0], &states[found[0].step]).unwrap();
        let fixed = apply(now, edits);
        assert_eq!(fixed, lines(&format!("// new\n{ONE}\n\n{TWO}").replace("b();", "b(2);")), "a(1) is taken back; b(2) and the comment stay");
    }

    #[test]
    fn a_change_made_again_later_cant_be_taken_back_alone() {
        let (states, steps) = history(&[ONE, &ONE.replace("a();", "a(1);"), &ONE.replace("a();", "a(1, 2);")]);
        let found = touches(&steps, &|i| states.get(i).cloned(), 0..3);
        assert_eq!(found.len(), 2);
        let oldest = found.last().unwrap();
        assert!(undo_one(&steps, oldest, &states[oldest.step]).unwrap_err().contains("changed again"));
        let newest = &found[0];
        assert_eq!(apply(states.last().unwrap(), undo_one(&steps, newest, &states[newest.step]).unwrap()), lines(&ONE.replace("a();", "a(1);")));
    }

    #[test]
    fn a_block_moved_in_one_step_is_followed_to_where_it_was() {
        let (states, steps) = history(&[&format!("{ONE}\n{TWO}\nend"), &format!("{TWO}\nend\n{ONE}"), &format!("{TWO}\nend\n{ONE}").replace("a();", "a(9);")]);
        let found = touches(&steps, &|i| states.get(i).cloned(), 4..7);
        assert_eq!(found.iter().map(|t| t.kind).collect::<Vec<_>>(), [Kind::Changed, Kind::Moved]);
        assert_eq!(found[1].before, Some(0..3));
    }

    #[test]
    fn a_part_cut_and_pasted_later_is_followed_back_through_the_cut() {
        let (states, steps) = history(&[
            &format!("{ONE}\n{TWO}\nend"),
            &format!("{ONE}\n{TWO}\nend").replace("a();", "a(1);"),
            &format!("{TWO}\nend"),
            &format!("{TWO}\nend\nmore"),
            &format!("{TWO}\nend\nmore\n{}", ONE.replace("a();", "a(1);")),
        ]);
        let found = touches(&steps, &|i| states.get(i).cloned(), 5..8);
        let kinds: Vec<Kind> = found.iter().map(|t| t.kind).collect();
        assert_eq!(kinds, [Kind::MovedHere, Kind::CutFromHere, Kind::Changed], "{found:?}");
        assert_eq!(found[2].step, 0, "the change made before it was moved");
        assert_eq!(found[1].before, Some(0..3));
    }

    #[test]
    fn a_part_written_fresh_has_its_writing_as_its_first_change() {
        let (states, steps) = history(&["top", &format!("top\n{ONE}"), &format!("top\n{ONE}").replace("a();", "a(1);")]);
        let found = touches(&steps, &|i| states.get(i).cloned(), 1..4);
        assert_eq!(found.iter().map(|t| t.kind).collect::<Vec<_>>(), [Kind::Changed, Kind::Written]);
        // Taking back the writing alone can't be done, as the lines changed again; the newest can.
        assert!(undo_one(&steps, &found[1], &states[found[1].step]).is_err());
    }

    #[test]
    fn mapping_ranges_and_points_through_a_step() {
        let step = diff(&lines("a\nb\nc\nd"), &lines("a\nx\nc\nd\ne"));
        assert_eq!(step.map_range(2..4), Mapped { range: 2..4, touched: false, moved: false });
        assert_eq!(step.map_range(0..3).range, 0..3);
        assert!(step.map_range(1..2).touched);
        assert_eq!(step.map_exact(2..4), Some(2..4));
        assert_eq!(step.map_exact(0..2), None);
        assert_eq!(step.map_point_exact(4), Some(4));
        let deleted = diff(&lines("a\nb\nc\nd"), &lines("a\nd"));
        assert_eq!(deleted.map_range(1..3).range, 1..1, "lines that went leave a place");
        assert_eq!(deleted.map_point_exact(2), None);
    }
}
