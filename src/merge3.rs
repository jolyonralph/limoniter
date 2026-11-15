//! Three-way merge of lines: what both sides did to a common original, put together. Where only one side changed
//! something, or both changed it the same way, the change is taken; where they changed the same lines differently,
//! that's a conflict for a person to settle. The same chunks can be read back from a file that already has
//! conflict markers in it, in the plain or the diff3 style. Nothing here draws anything or touches a file.
//!
//! Lines are compared without their line endings, so a file whose endings were changed on one side still merges
//! line by line. The merged text takes its endings from mine (then theirs, then the original), and ends with a
//! line break if whichever side changed that wants one.

use similar::{Algorithm, DiffOp, capture_diff_slices_deadline};
use std::time::{Duration, Instant};

/// How long each side's comparison with the original may take. After that it's still right, but coarser.
const MOST_TIME: Duration = Duration::from_secs(5);

/// What happened to a stretch of lines.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    /// The same on all three.
    Unchanged,
    /// Only mine changed it.
    Mine,
    /// Only theirs changed it.
    Theirs,
    /// Both changed it, to the same lines.
    Both,
    /// Both changed it, differently.
    Conflict,
}

/// One stretch of the merge, with its lines on each of the three.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Chunk {
    pub kind: Kind,
    pub base: Vec<String>,
    pub mine: Vec<String>,
    pub theirs: Vec<String>,
}

impl Chunk {
    fn same(lines: Vec<String>) -> Self {
        Chunk { kind: Kind::Unchanged, base: lines.clone(), mine: lines.clone(), theirs: lines }
    }

    /// The lines the merge takes here, or None for a conflict.
    pub fn resolved(&self) -> Option<&[String]> {
        match self.kind {
            Kind::Unchanged | Kind::Mine | Kind::Both => Some(&self.mine),
            Kind::Theirs => Some(&self.theirs),
            Kind::Conflict => None,
        }
    }
}

/// A text as lines, and how it ends them.
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct Text {
    pub lines: Vec<String>,
    /// Most of its line breaks are CR LF.
    pub crlf: bool,
    /// Its last line has a line break after it.
    pub final_newline: bool,
    /// It has any line breaks at all, so its endings say something.
    pub has_breaks: bool,
}

/// Splits a text into lines, without their endings.
pub fn split(text: &str) -> Text {
    if text.is_empty() {
        return Text::default();
    }
    let breaks = text.matches('\n').count();
    let crlf = text.matches("\r\n").count();
    let mut lines: Vec<String> = text.split('\n').map(|line| line.strip_suffix('\r').unwrap_or(line).to_string()).collect();
    let final_newline = text.ends_with('\n');
    if final_newline {
        lines.pop();
    }
    Text { lines, crlf: crlf * 2 > breaks, final_newline, has_breaks: breaks > 0 }
}

/// Puts lines back together into a text.
pub fn join(lines: &[String], crlf: bool, final_newline: bool) -> String {
    let eol = if crlf { "\r\n" } else { "\n" };
    let mut text = lines.join(eol);
    if final_newline && !lines.is_empty() {
        text.push_str(eol);
    }
    text
}

/// The stretches of lines two lists have in common, as (start in a, start in b, length).
fn matching(a: &[String], b: &[String]) -> Vec<(usize, usize, usize)> {
    let deadline = Some(Instant::now() + MOST_TIME);
    capture_diff_slices_deadline(Algorithm::Myers, a, b, deadline)
        .into_iter()
        .filter_map(|op| match op {
            DiffOp::Equal { old_index, new_index, len } => Some((old_index, new_index, len)),
            _ => None,
        })
        .collect()
}

/// Merges the lines of mine and theirs, both changed from base, into chunks. Runs of unchanged lines are one chunk.
pub fn merge(base: &[String], mine: &[String], theirs: &[String]) -> Vec<Chunk> {
    // Where all three agree: the original lines that both sides kept, and where they are on each side.
    let with_mine = matching(base, mine);
    let with_theirs = matching(base, theirs);
    let mut steady = Vec::new();
    let (mut i, mut j) = (0, 0);
    while i < with_mine.len() && j < with_theirs.len() {
        let (a_base, a_at, a_len) = with_mine[i];
        let (b_base, b_at, b_len) = with_theirs[j];
        let start = a_base.max(b_base);
        let end = (a_base + a_len).min(b_base + b_len);
        if start < end {
            steady.push((start, a_at + (start - a_base), b_at + (start - b_base), end - start));
        }
        if a_base + a_len < b_base + b_len {
            i += 1;
        } else {
            j += 1;
        }
    }
    steady.push((base.len(), mine.len(), theirs.len(), 0));

    let mut chunks: Vec<Chunk> = Vec::new();
    let mut push = |chunk: Chunk| {
        if chunk.kind == Kind::Unchanged
            && let Some(last) = chunks.last_mut()
            && last.kind == Kind::Unchanged
        {
            last.base.extend(chunk.base);
            last.mine.extend(chunk.mine);
            last.theirs.extend(chunk.theirs);
            return;
        }
        chunks.push(chunk);
    };
    let (mut at_base, mut at_mine, mut at_theirs) = (0, 0, 0);
    for (base_to, mine_to, theirs_to, len) in steady {
        let (b, m, t) = (&base[at_base..base_to], &mine[at_mine..mine_to], &theirs[at_theirs..theirs_to]);
        if !(b.is_empty() && m.is_empty() && t.is_empty()) {
            let kind = if m == t {
                if m == b { Kind::Unchanged } else { Kind::Both }
            } else if m == b {
                Kind::Theirs
            } else if t == b {
                Kind::Mine
            } else {
                Kind::Conflict
            };
            push(Chunk { kind, base: b.to_vec(), mine: m.to_vec(), theirs: t.to_vec() });
        }
        if len > 0 {
            push(Chunk::same(mine[mine_to..mine_to + len].to_vec()));
        }
        (at_base, at_mine, at_theirs) = (base_to + len, mine_to + len, theirs_to + len);
    }
    chunks
}

/// A whole merge of three texts: its chunks, and how the merged text ends its lines.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Merge {
    pub chunks: Vec<Chunk>,
    pub crlf: bool,
    pub final_newline: bool,
}

impl Merge {
    /// Merges three texts.
    pub fn of(base: &str, mine: &str, theirs: &str) -> Merge {
        let (base, mine, theirs) = (split(base), split(mine), split(theirs));
        let chunks = merge(&base.lines, &mine.lines, &theirs.lines);
        Merge { chunks, crlf: endings_from(&base, &mine, &theirs), final_newline: final_newline_from(&base, &mine, &theirs) }
    }

    #[cfg(test)]
    pub fn conflicts(&self) -> usize {
        self.chunks.iter().filter(|chunk| chunk.kind == Kind::Conflict).count()
    }

    /// The merged lines, with markers around each conflict.
    pub fn lines(&self, labels: &Labels) -> Vec<String> {
        with_markers(&self.chunks, labels)
    }

    /// The merged text, with markers around each conflict.
    pub fn text(&self, labels: &Labels) -> String {
        join(&self.lines(labels), self.crlf, self.final_newline)
    }

    /// The merged text if there are no conflicts.
    #[cfg(test)]
    pub fn clean_text(&self) -> Option<String> {
        (self.conflicts() == 0).then(|| self.text(&Labels::default()))
    }
}

/// Mine's endings, unless it has no line breaks to go by.
fn endings_from(base: &Text, mine: &Text, theirs: &Text) -> bool {
    [mine, theirs, base].into_iter().find(|text| text.has_breaks).is_some_and(|text| text.crlf)
}

/// Whether the merged text ends with a line break: what changed it from the original, if either side did.
fn final_newline_from(base: &Text, mine: &Text, theirs: &Text) -> bool {
    // A text with no lines has nothing to end, so it says nothing either way.
    let says = |text: &Text| (!text.lines.is_empty()).then_some(text.final_newline);
    let (b, m, t) = (says(base), says(mine), says(theirs));
    match (m, t) {
        (Some(m), _) if b != Some(m) => m,
        (_, Some(t)) if b != Some(t) => t,
        _ => b.or(m).or(t).unwrap_or(false),
    }
}

/// What the markers around a conflict say each side is.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Labels {
    pub base: String,
    pub mine: String,
    pub theirs: String,
}

impl Default for Labels {
    fn default() -> Self {
        Labels { base: "base".into(), mine: "mine".into(), theirs: "theirs".into() }
    }
}

fn marker(sign: char, label: &str) -> String {
    let marks: String = std::iter::repeat_n(sign, 7).collect();
    if label.is_empty() { marks } else { format!("{marks} {label}") }
}

/// The lines of a merge, each conflict written with markers: mine, the original (if it had any lines), theirs.
pub fn with_markers(chunks: &[Chunk], labels: &Labels) -> Vec<String> {
    let mut out = Vec::new();
    for chunk in chunks {
        match chunk.resolved() {
            Some(lines) => out.extend(lines.iter().cloned()),
            None => out.extend(conflict_lines(chunk, labels)),
        }
    }
    out
}

/// One conflict, as it's written with markers.
pub fn conflict_lines(chunk: &Chunk, labels: &Labels) -> Vec<String> {
    let mut out = vec![marker('<', &labels.mine)];
    out.extend(chunk.mine.iter().cloned());
    if !chunk.base.is_empty() {
        out.push(marker('|', &labels.base));
        out.extend(chunk.base.iter().cloned());
    }
    out.push("=======".into());
    out.extend(chunk.theirs.iter().cloned());
    out.push(marker('>', &labels.theirs));
    out
}

// ---------- Conflict markers already in a file ----------

/// Where one conflict's markers are, by line: <<<<<<<, ||||||| (if the original is shown), =======, >>>>>>>.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Marked {
    pub start: usize,
    pub base: Option<usize>,
    pub middle: usize,
    pub end: usize,
}

fn is_marker(line: &str, sign: char) -> bool {
    let line = line.strip_suffix('\r').unwrap_or(line);
    let rest = line.trim_start_matches(sign);
    line.len() - rest.len() == 7 && (rest.is_empty() || rest.starts_with(' '))
}

/// The conflicts marked in some lines. A marker out of place, or one left open, is just a line.
pub fn marked<S: AsRef<str>>(lines: &[S]) -> Vec<Marked> {
    let mut found = Vec::new();
    let mut open: Option<(usize, Option<usize>, Option<usize>)> = None;
    for (i, line) in lines.iter().enumerate() {
        let line = line.as_ref();
        if is_marker(line, '<') {
            open = Some((i, None, None));
        } else if let Some((start, base, middle)) = &mut open {
            if is_marker(line, '|') && base.is_none() && middle.is_none() {
                *base = Some(i);
            } else if line.strip_suffix('\r').unwrap_or(line) == "=======" && middle.is_none() {
                *middle = Some(i);
            } else if is_marker(line, '>')
                && let Some(middle) = *middle
            {
                found.push(Marked { start: *start, base: *base, middle, end: i });
                open = None;
            }
        }
    }
    found
}

/// How many conflicts are marked in some lines.
pub fn count_marked<S: AsRef<str>>(lines: &[S]) -> usize {
    marked(lines).len()
}

/// Reads a file's conflict markers into chunks: what's outside them is unchanged, and each marked conflict is one.
/// None if there are no conflicts marked.
pub fn parse_markers<S: AsRef<str>>(lines: &[S]) -> Option<Vec<Chunk>> {
    let found = marked(lines);
    if found.is_empty() {
        return None;
    }
    let text = |from: usize, to: usize| -> Vec<String> { lines[from..to].iter().map(|line| line.as_ref().strip_suffix('\r').unwrap_or(line.as_ref()).to_string()).collect() };
    let mut chunks = Vec::new();
    let mut at = 0;
    for conflict in found {
        if conflict.start > at {
            chunks.push(Chunk::same(text(at, conflict.start)));
        }
        let mine_end = conflict.base.unwrap_or(conflict.middle);
        let base = conflict.base.map_or_else(Vec::new, |base| text(base + 1, conflict.middle));
        chunks.push(Chunk { kind: Kind::Conflict, base, mine: text(conflict.start + 1, mine_end), theirs: text(conflict.middle + 1, conflict.end) });
        at = conflict.end + 1;
    }
    if at < lines.len() {
        chunks.push(Chunk::same(text(at, lines.len())));
    }
    Some(chunks)
}

/// What a marked conflict is settled with.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Take {
    Mine,
    Theirs,
    /// Mine, then theirs.
    MineFirst,
    /// Theirs, then mine.
    TheirsFirst,
    /// The original lines, as neither side has them.
    Base,
}

/// The lines a marked conflict is replaced with.
pub fn take<S: AsRef<str>>(lines: &[S], conflict: &Marked, take: Take) -> Vec<String> {
    let part = |from: usize, to: usize| lines[from..to].iter().map(|line| line.as_ref().to_string()).collect::<Vec<_>>();
    let mine = part(conflict.start + 1, conflict.base.unwrap_or(conflict.middle));
    let theirs = part(conflict.middle + 1, conflict.end);
    let base = conflict.base.map_or_else(Vec::new, |base| part(base + 1, conflict.middle));
    match take {
        Take::Mine => mine,
        Take::Theirs => theirs,
        Take::MineFirst => mine.into_iter().chain(theirs).collect(),
        Take::TheirsFirst => theirs.into_iter().chain(mine).collect(),
        Take::Base => base,
    }
}

/// The three texts a list of chunks was made from, as lines: the original, mine and theirs.
pub fn sides(chunks: &[Chunk]) -> [Vec<String>; 3] {
    let mut out: [Vec<String>; 3] = Default::default();
    for chunk in chunks {
        out[0].extend(chunk.base.iter().cloned());
        out[1].extend(chunk.mine.iter().cloned());
        out[2].extend(chunk.theirs.iter().cloned());
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lines(text: &str) -> Vec<String> {
        split(text).lines
    }

    fn kinds(chunks: &[Chunk]) -> Vec<Kind> {
        chunks.iter().map(|chunk| chunk.kind).collect()
    }

    fn merged(base: &str, mine: &str, theirs: &str) -> Merge {
        Merge::of(base, mine, theirs)
    }

    #[test]
    fn splitting_and_joining_keep_the_text() {
        for text in ["", "a", "a\n", "a\nb", "a\nb\n", "a\r\nb\r\n", "\n", "\n\n", "a\n\nb\n"] {
            let split = split(text);
            assert_eq!(join(&split.lines, split.crlf, split.final_newline), text, "{text:?}");
        }
        assert_eq!(split("a\r\nb\r\n").lines, ["a", "b"]);
        assert!(split("a\r\nb\r\n").crlf && !split("a\nb\n").crlf);
        assert!(!split("").final_newline && split("x\n").final_newline);
    }

    #[test]
    fn nothing_changed_is_one_unchanged_chunk() {
        let m = merged("a\nb\nc\n", "a\nb\nc\n", "a\nb\nc\n");
        assert_eq!(kinds(&m.chunks), [Kind::Unchanged]);
        assert_eq!(m.clean_text().unwrap(), "a\nb\nc\n");
    }

    #[test]
    fn changes_on_one_side_each_are_taken() {
        let m = merged("1\n2\n3\n4\n5\n", "1\nTWO\n3\n4\n5\n", "1\n2\n3\n4\nFIVE\n");
        assert_eq!(kinds(&m.chunks), [Kind::Unchanged, Kind::Mine, Kind::Unchanged, Kind::Theirs]);
        assert_eq!(m.clean_text().unwrap(), "1\nTWO\n3\n4\nFIVE\n");
    }

    #[test]
    fn the_same_change_on_both_sides_is_taken_once() {
        let m = merged("a\nb\nc\n", "a\nB\nc\n", "a\nB\nc\n");
        assert_eq!(kinds(&m.chunks), [Kind::Unchanged, Kind::Both, Kind::Unchanged]);
        assert_eq!(m.clean_text().unwrap(), "a\nB\nc\n");
    }

    #[test]
    fn different_changes_to_the_same_lines_conflict() {
        let m = merged("a\nb\nc\n", "a\nmine\nc\n", "a\ntheirs\nc\n");
        assert_eq!(kinds(&m.chunks), [Kind::Unchanged, Kind::Conflict, Kind::Unchanged]);
        assert_eq!(m.conflicts(), 1);
        assert!(m.clean_text().is_none());
        assert_eq!(m.text(&Labels::default()), "a\n<<<<<<< mine\nmine\n||||||| base\nb\n=======\ntheirs\n>>>>>>> theirs\nc\n");
        let conflict = &m.chunks[1];
        assert_eq!((conflict.base.as_slice(), conflict.mine.as_slice(), conflict.theirs.as_slice()), (&["b".to_string()][..], &["mine".to_string()][..], &["theirs".to_string()][..]));
    }

    #[test]
    fn changes_to_lines_side_by_side_conflict_as_git_has_them() {
        // Next to each other, with no line both kept between them: git calls this a conflict too.
        let m = merged("1\n2\n3\n4\n", "1\nTWO\n3\n4\n", "1\n2\nTHREE\n4\n");
        assert_eq!(kinds(&m.chunks), [Kind::Unchanged, Kind::Conflict, Kind::Unchanged]);
        assert_eq!(m.chunks[1].mine, ["TWO", "3"]);
        assert_eq!(m.chunks[1].theirs, ["2", "THREE"]);
        // With a line kept between, they're separate and both taken.
        let m = merged("1\n2\n3\n4\n", "1\nTWO\n3\n4\n", "1\n2\n3\nFOUR\n");
        assert_eq!(m.clean_text().unwrap(), "1\nTWO\n3\nFOUR\n");
    }

    #[test]
    fn inserts_at_the_start_and_end() {
        let m = merged("a\nb\n", "top\na\nb\n", "a\nb\nbottom\n");
        assert_eq!(kinds(&m.chunks), [Kind::Mine, Kind::Unchanged, Kind::Theirs]);
        assert_eq!(m.clean_text().unwrap(), "top\na\nb\nbottom\n");
        // Both putting something different at the very start is a conflict with nothing in the original.
        let m = merged("a\n", "x\na\n", "y\na\n");
        assert_eq!(kinds(&m.chunks), [Kind::Conflict, Kind::Unchanged]);
        assert!(m.chunks[0].base.is_empty());
        assert_eq!(m.text(&Labels::default()), "<<<<<<< mine\nx\n=======\ny\n>>>>>>> theirs\na\n");
        // And at the end.
        let m = merged("a\n", "a\nx\n", "a\ny\n");
        assert_eq!(kinds(&m.chunks), [Kind::Unchanged, Kind::Conflict]);
    }

    #[test]
    fn deleting_on_one_side_and_keeping_on_the_other() {
        let m = merged("a\nb\nc\n", "a\nc\n", "a\nb\nc\nd\n");
        assert_eq!(m.clean_text().unwrap(), "a\nc\nd\n");
        // Deleted on one side and changed on the other is a conflict.
        let m = merged("a\nb\nc\n", "a\nc\n", "a\nB\nc\n");
        assert_eq!(m.conflicts(), 1);
        assert!(m.chunks[1].mine.is_empty());
    }

    #[test]
    fn empty_files() {
        assert!(merged("", "", "").chunks.is_empty());
        assert_eq!(merged("", "", "").clean_text().unwrap(), "");
        let m = merged("", "new\n", "");
        assert_eq!(kinds(&m.chunks), [Kind::Mine]);
        assert_eq!(m.clean_text().unwrap(), "new\n");
        let m = merged("", "one\n", "two\n");
        assert_eq!(kinds(&m.chunks), [Kind::Conflict]);
        let m = merged("old\n", "", "old\n");
        assert_eq!(m.clean_text().unwrap(), "", "mine emptied it, theirs left it alone");
        let m = merged("x\n", "", "");
        assert_eq!(kinds(&m.chunks), [Kind::Both]);
        assert_eq!(m.clean_text().unwrap(), "");
    }

    #[test]
    fn a_line_break_added_or_taken_away_at_the_end_is_merged_too() {
        // Mine takes away the last line break, theirs changes a line: both happen.
        assert_eq!(merged("a\nb\n", "a\nb", "A\nb\n").clean_text().unwrap(), "A\nb");
        // Theirs adds one.
        assert_eq!(merged("a\nb", "A\nb", "a\nb\n").clean_text().unwrap(), "A\nb\n");
        // Nobody changes it.
        assert_eq!(merged("a\nb", "A\nb", "a\nb").clean_text().unwrap(), "A\nb");
        // Lines added after a last line with no break: the new text ends as the side that added them does.
        assert_eq!(merged("a", "a\nb\n", "a").clean_text().unwrap(), "a\nb\n");
    }

    #[test]
    fn crlf_files_merge_and_keep_their_endings() {
        let m = merged("1\r\n2\r\n3\r\n", "ONE\r\n2\r\n3\r\n", "1\r\n2\r\nTHREE\r\n");
        assert_eq!(m.clean_text().unwrap(), "ONE\r\n2\r\nTHREE\r\n");
        // Line endings changed on one side aren't a change to every line.
        let m = merged("1\n2\n3\n", "1\r\n2\r\n3\r\n", "1\n2\nTHREE\n");
        assert_eq!(m.conflicts(), 0);
        assert_eq!(m.clean_text().unwrap(), "1\r\n2\r\nTHREE\r\n", "mine's endings are used");
        let m = merged("1\n", "1\n", "1\r\n2\r\n");
        assert_eq!(m.clean_text().unwrap(), "1\n2\n");
        let m = merged("x", "x", "x\r\ny\r\n");
        assert_eq!(m.clean_text().unwrap(), "x\r\ny\r\n", "mine has no line breaks to go by, so theirs are used");
    }

    #[test]
    fn several_conflicts_and_clean_changes_together() {
        let base = "a\nb\nc\nd\ne\nf\ng\nh\ni\n";
        let mine = "a\nB1\nc\nd\nE1\nf\ng\nh\ni\nj\n";
        let theirs = "a\nB2\nc\nd\ne\nf\nG\nh\ni\n";
        let m = merged(base, mine, theirs);
        assert_eq!(m.conflicts(), 1);
        assert_eq!(kinds(&m.chunks), [Kind::Unchanged, Kind::Conflict, Kind::Unchanged, Kind::Mine, Kind::Unchanged, Kind::Theirs, Kind::Unchanged, Kind::Mine]);
        let text = m.lines(&Labels::default());
        assert_eq!(count_marked(&text), 1);
        assert_eq!(join(&text, false, true), "a\n<<<<<<< mine\nB1\n||||||| base\nb\n=======\nB2\n>>>>>>> theirs\nc\nd\nE1\nf\nG\nh\ni\nj\n");
    }

    #[test]
    fn nothing_is_lost_from_any_side() {
        let samples = ["", "a\n", "a\nb\nc\n", "x\na\ny\nb\n", "a\na\na\n", "b\nc\nd\n", "a\nb\n\nc\n", "1\n2\n3\n4\n5\n6\n"];
        for base in samples {
            for mine in samples {
                for theirs in samples {
                    let chunks = merge(&lines(base), &lines(mine), &lines(theirs));
                    let [b, m, t] = sides(&chunks);
                    assert_eq!((b, m, t), (lines(base), lines(mine), lines(theirs)), "{base:?} {mine:?} {theirs:?}");
                    for chunk in &chunks {
                        let expected = match chunk.kind {
                            Kind::Unchanged => chunk.base == chunk.mine && chunk.mine == chunk.theirs,
                            Kind::Mine => chunk.theirs == chunk.base && chunk.mine != chunk.base,
                            Kind::Theirs => chunk.mine == chunk.base && chunk.theirs != chunk.base,
                            Kind::Both => chunk.mine == chunk.theirs && chunk.mine != chunk.base,
                            Kind::Conflict => chunk.mine != chunk.theirs && chunk.mine != chunk.base && chunk.theirs != chunk.base,
                        };
                        assert!(expected, "{chunk:?}");
                    }
                    // A side that changed nothing means the merge is just the other side.
                    if base == theirs {
                        assert_eq!(join(&with_markers(&chunks, &Labels::default()), false, true), join(&lines(mine), false, true));
                    }
                }
            }
        }
    }

    #[test]
    fn markers_in_the_diff3_style_are_read_back_into_the_same_chunks() {
        let m = merged("a\nb\nc\nd\n", "a\nmine\nc\nd\nmore\n", "a\ntheirs\nc\nD\n");
        let text = m.lines(&Labels { base: "BASE".into(), mine: "HEAD".into(), theirs: "feature".into() });
        let parsed = parse_markers(&text).unwrap();
        let conflict = parsed.iter().find(|c| c.kind == Kind::Conflict).unwrap();
        assert_eq!(conflict.base, ["b"]);
        assert_eq!(conflict.mine, ["mine"]);
        assert_eq!(conflict.theirs, ["theirs"]);
        assert_eq!(with_markers(&parsed, &Labels { base: "BASE".into(), mine: "HEAD".into(), theirs: "feature".into() }), text);
    }

    #[test]
    fn markers_in_the_plain_style_and_odd_ones() {
        let text = ["top", "<<<<<<< HEAD", "mine 1", "mine 2", "=======", "theirs", ">>>>>>> branch", "middle", "<<<<<<<", "=======", "only theirs", ">>>>>>>", "end"];
        let chunks = parse_markers(&text).unwrap();
        assert_eq!(kinds(&chunks), [Kind::Unchanged, Kind::Conflict, Kind::Unchanged, Kind::Conflict, Kind::Unchanged]);
        assert_eq!(chunks[1].mine, ["mine 1", "mine 2"]);
        assert!(chunks[1].base.is_empty());
        assert!(chunks[3].mine.is_empty());
        assert_eq!(chunks[3].theirs, ["only theirs"]);
        assert_eq!(chunks[4].mine, ["end"]);
        // Markers with CR LF line ends, as read from a Windows file a line at a time.
        let crlf = ["<<<<<<< ours\r", "a\r", "||||||| base\r", "o\r", "=======\r", "b\r", ">>>>>>> theirs\r"];
        let chunks = parse_markers(&crlf).unwrap();
        assert_eq!((chunks[0].mine.clone(), chunks[0].base.clone(), chunks[0].theirs.clone()), (vec!["a".to_string()], vec!["o".to_string()], vec!["b".to_string()]));
        // Not markers: too many signs, something straight after them, or never closed.
        assert!(parse_markers(&["<<<<<<<< x", "=======", ">>>>>>>"]).is_none());
        assert!(parse_markers(&["<<<<<<<x", "=======", ">>>>>>>"]).is_none());
        assert!(parse_markers(&["<<<<<<< a", "=======", "no end"]).is_none());
        assert!(parse_markers(&["a", "b"]).is_none());
        // A start with no end before the next start: the first is just lines.
        let chunks = parse_markers(&["<<<<<<< a", "x", "<<<<<<< b", "y", "=======", "z", ">>>>>>> c"]).unwrap();
        assert_eq!(kinds(&chunks), [Kind::Unchanged, Kind::Conflict]);
        assert_eq!(chunks[0].mine, ["<<<<<<< a", "x"]);
    }

    #[test]
    fn a_marked_conflict_can_be_settled_each_way() {
        let text = ["k", "<<<<<<< m", "M", "||||||| b", "B", "=======", "T", ">>>>>>> t", "k"];
        let found = marked(&text);
        assert_eq!(found, [Marked { start: 1, base: Some(3), middle: 5, end: 7 }]);
        assert_eq!(take(&text, &found[0], Take::Mine), ["M"]);
        assert_eq!(take(&text, &found[0], Take::Theirs), ["T"]);
        assert_eq!(take(&text, &found[0], Take::MineFirst), ["M", "T"]);
        assert_eq!(take(&text, &found[0], Take::TheirsFirst), ["T", "M"]);
        assert_eq!(take(&text, &found[0], Take::Base), ["B"]);
    }
}
