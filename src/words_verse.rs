//! Verse: where a line's stresses fall, which metre a passage keeps ("iambic pentameter"), and checks of the
//! fixed forms: the haiku (5-7-5) and the limerick (AABBA, with its usual syllables and beats).
//!
//! The stresses are the dictionary's, with the give that verse has always taken: a short word that isn't much
//! of a word ("the", "of", "to") can sit on a beat at a small cost, and a short word that matters can fall
//! between them at a smaller one. A line that's a syllable out is still fitted, and the syllable it's out by is
//! said, so what's reported is where a line leaves its metre and not only that it does.

// `marks_of` and the like are for the song-lyrics tools to come.
#![allow(dead_code)]

use crate::words::{Stress, line_syllables, rhymes_with, say, words_in};

/// How a syllable stands in a line of verse.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    /// The main stress of a word of two syllables or more.
    Heavy,
    /// A lesser stress.
    Medium,
    /// No stress, in a long word.
    Light,
    /// A one-syllable word that matters: "night", "run".
    Content,
    /// A one-syllable word of the kind that carries little: "the", "of", "to", "I".
    Function,
}

impl Kind {
    /// Whether it is heard as a beat if read plainly.
    pub fn is_stressed(self) -> bool {
        matches!(self, Kind::Heavy | Kind::Content)
    }

    /// What it costs to put this syllable where the metre wants a beat (`beat`) or wants none.
    fn cost(self, beat: bool) -> f32 {
        match (self, beat) {
            (Kind::Heavy, true) | (Kind::Light, false) | (Kind::Content, true) | (Kind::Function, false) => 0.0,
            (Kind::Heavy, false) => 1.0,
            (Kind::Light, true) => 1.0,
            (Kind::Medium, _) => 0.35,
            (Kind::Content, false) => 0.4,
            (Kind::Function, true) => 0.5,
        }
    }
}

/// One-syllable words that give way to a beat, and take one when the line asks.
const SMALL_WORDS: &[&str] = &[
    "a", "an", "the", "of", "to", "in", "on", "at", "by", "for", "from", "with", "and", "or", "but", "nor", "as", "if", "so", "than", "that", "this", "these", "those", "i", "me", "my", "we", "us", "our", "you", "your", "he", "him", "his", "she", "her", "it", "its", "they", "them", "their", "is", "am", "are", "was", "were", "be", "been", "do", "does", "did", "has", "have", "had", "will", "would", "shall", "should", "can", "could", "may", "might", "must", "not", "no", "up", "out", "off", "into", "upon", "thee", "thou", "thy", "thine", "who", "whom", "whose", "which", "what", "when", "where", "while", "there", "then", "yet", "o", "oh", "ere", "'tis", "'twas", "'t", "'s", "nor", "per", "via", "got", "get", "let", "all", "any", "each", "one", "some", "such", "too", "how", "why", "than", "till", "'til", "e'er", "ne'er", "o'er",
];

/// A word of a line, split into the syllables it has.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Scanned {
    pub word: String,
    /// Where it is in the line, by character.
    pub start: usize,
    pub end: usize,
    pub kinds: Vec<Kind>,
    pub guessed: bool,
}

/// A line's words, each with how its syllables stand.
pub fn scan_line(line: &str) -> Vec<Scanned> {
    words_in(line)
        .into_iter()
        .map(|(start, end, word)| {
            let spoken = say(&word);
            let one = spoken.stresses.len() == 1;
            let small = SMALL_WORDS.contains(&word.to_lowercase().replace('\u{2019}', "'").as_str());
            let kinds = spoken
                .stresses
                .iter()
                .map(|s| match (s, one, small) {
                    (_, true, true) => Kind::Function,
                    (_, true, false) => Kind::Content,
                    (Stress::Primary, ..) => Kind::Heavy,
                    (Stress::Secondary, ..) => Kind::Medium,
                    (Stress::None, ..) => Kind::Light,
                })
                .collect();
            Scanned { word, start, end, kinds, guessed: spoken.guessed }
        })
        .collect()
}

/// The kinds of a line's syllables in order.
pub fn syllable_kinds(line: &str) -> Vec<Kind> {
    scan_line(line).into_iter().flat_map(|w| w.kinds).collect()
}

/// The feet verse is built of.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Foot {
    Iamb,
    Trochee,
    Anapaest,
    Dactyl,
}

impl Foot {
    pub const ALL: [Foot; 4] = [Foot::Iamb, Foot::Trochee, Foot::Anapaest, Foot::Dactyl];

    /// Where its beats fall: an iamb is a light syllable then a beat.
    fn slots(self) -> &'static [bool] {
        match self {
            Foot::Iamb => &[false, true],
            Foot::Trochee => &[true, false],
            Foot::Anapaest => &[false, false, true],
            Foot::Dactyl => &[true, false, false],
        }
    }

    fn index(self) -> usize {
        Foot::ALL.iter().position(|f| *f == self).unwrap_or(0)
    }
}

/// The names of metres, by foot and by how many feet: "iambic pentameter". Whole names, so each can be translated.
const NAMES: [[&str; 8]; 4] = [
    ["iambic monometer", "iambic dimeter", "iambic trimeter", "iambic tetrameter", "iambic pentameter", "iambic hexameter", "iambic heptameter", "iambic octameter"],
    ["trochaic monometer", "trochaic dimeter", "trochaic trimeter", "trochaic tetrameter", "trochaic pentameter", "trochaic hexameter", "trochaic heptameter", "trochaic octameter"],
    ["anapaestic monometer", "anapaestic dimeter", "anapaestic trimeter", "anapaestic tetrameter", "anapaestic pentameter", "anapaestic hexameter", "anapaestic heptameter", "anapaestic octameter"],
    ["dactylic monometer", "dactylic dimeter", "dactylic trimeter", "dactylic tetrameter", "dactylic pentameter", "dactylic hexameter", "dactylic heptameter", "dactylic octameter"],
];

/// A foot and how many of them to a line.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Metre {
    pub foot: Foot,
    pub feet: usize,
}

impl Metre {
    /// Its name in English: "iambic pentameter".
    pub fn name(self) -> &'static str {
        NAMES[self.foot.index()][self.feet.clamp(1, 8) - 1]
    }

    fn pattern(self) -> Vec<bool> {
        self.foot.slots().iter().copied().cycle().take(self.foot.slots().len() * self.feet).collect()
    }
}

/// How the last syllable of a line stands to its metre.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Ending {
    /// As the metre has it.
    Full,
    /// The last (light) syllable is missing: "catalectic", as in "Tyger Tyger, burning bright".
    Short,
    /// One light syllable more: a feminine ending, as in "To be, or not to be, that is the question".
    Extra,
}

/// How a line fits a metre.
#[derive(Clone, Debug, PartialEq)]
pub struct Fit {
    pub metre: Metre,
    /// What the fit costs: nothing for a line that is the metre exactly, 1 for every syllable that is wrong.
    pub cost: f32,
    pub ending: Ending,
    /// The line starts on its first beat, leaving out the metre's first light syllable.
    pub headless: bool,
    /// Syllables (by their place in the line) that leave the metre: a beat missing or in the wrong place, or one
    /// too many.
    pub off: Vec<usize>,
    /// How many syllables the line has against the metre's.
    pub syllables: usize,
    pub expected: usize,
}

const GAP: f32 = 1.0;
const HEADLESS: f32 = 0.3;
const SHORT_OR_EXTRA: f32 = 0.1;

/// The best way of laying a line of syllables along a metre, by dynamic programming: each syllable meets a slot
/// of the metre, or is one too many, or its slot goes unfilled.
pub fn fit_kinds(kinds: &[Kind], metre: Metre) -> Fit {
    let slots = metre.pattern();
    let (n, m) = (kinds.len(), slots.len());
    let inf = f32::INFINITY;
    // best[i][j]: the cost of fitting the first i syllables to the first j slots.
    let mut best = vec![vec![inf; m + 1]; n + 1];
    // 0 start, 1 match, 2 syllable too many, 3 slot unfilled
    let mut how = vec![vec![0u8; m + 1]; n + 1];
    best[0][0] = 0.0;
    if m > 1 && !slots[0] {
        best[0][1] = HEADLESS;
    }
    for i in 0..=n {
        for j in 0..=m {
            if i > 0 && j > 0 && best[i - 1][j - 1] + kinds[i - 1].cost(slots[j - 1]) < best[i][j] {
                best[i][j] = best[i - 1][j - 1] + kinds[i - 1].cost(slots[j - 1]);
                how[i][j] = 1;
            }
            if i > 0 && best[i - 1][j] + GAP < best[i][j] {
                best[i][j] = best[i - 1][j] + GAP;
                how[i][j] = 2;
            }
            if j > 0 && best[i][j - 1] + GAP < best[i][j] {
                best[i][j] = best[i][j - 1] + GAP;
                how[i][j] = 3;
            }
        }
    }
    // The ways a line may end: as the metre has it, a light slot short (catalectic) or a light syllable over.
    let mut end = (n, m, Ending::Full, best[n][m]);
    if m > 0 && !slots[m - 1] && best[n][m - 1] + SHORT_OR_EXTRA < end.3 {
        end = (n, m - 1, Ending::Short, best[n][m - 1] + SHORT_OR_EXTRA);
    }
    if n > 0 && !kinds[n - 1].is_stressed() && best[n - 1][m] + SHORT_OR_EXTRA < end.3 {
        end = (n - 1, m, Ending::Extra, best[n - 1][m] + SHORT_OR_EXTRA);
    }
    let (mut i, mut j, ending, cost) = end;
    let mut off = Vec::new();
    if ending == Ending::Extra {
        // The extra light syllable isn't "off": it's the ending.
    }
    while i > 0 || j > 0 {
        match how[i][j] {
            1 => {
                if kinds[i - 1].cost(slots[j - 1]) >= 0.4 {
                    off.push(i - 1);
                }
                i -= 1;
                j -= 1;
            }
            2 => {
                off.push(i - 1);
                i -= 1;
            }
            3 => j -= 1,
            _ => break,
        }
    }
    off.reverse();
    Fit { metre, cost, ending, headless: m > 1 && !slots[0] && how_starts_late(&how, n, m, ending), off, syllables: n, expected: m }
}

/// Whether the best way through began by leaving out the first slot.
fn how_starts_late(how: &[Vec<u8>], n: usize, m: usize, ending: Ending) -> bool {
    let (mut i, mut j) = match ending {
        Ending::Full => (n, m),
        Ending::Short => (n, m - 1),
        Ending::Extra => (n - 1, m),
    };
    while i > 0 || j > 0 {
        match how[i][j] {
            1 => {
                i -= 1;
                j -= 1;
            }
            2 => i -= 1,
            3 => j -= 1,
            _ => return i == 0 && j == 1,
        }
    }
    false
}

/// The metre a line fits best, by trying every foot and from one to eight of them.
pub fn best_fit(kinds: &[Kind]) -> Option<Fit> {
    let n = kinds.len();
    if n < 2 {
        return None;
    }
    let mut best: Option<Fit> = None;
    for foot in Foot::ALL {
        for feet in 1..=8 {
            let m = foot.slots().len() * feet;
            if m + 3 < n || n + 3 < m {
                continue;
            }
            let fit = fit_kinds(kinds, Metre { foot, feet });
            if best.as_ref().is_none_or(|b| fit.cost + 1e-4 < b.cost) {
                best = Some(fit);
            }
        }
    }
    best
}

/// A line of a passage and how it stands.
#[derive(Clone, Debug, PartialEq)]
pub struct ScannedLine {
    /// Its place among the lines asked about.
    pub index: usize,
    pub words: Vec<Scanned>,
    pub syllables: usize,
    /// How it fits the passage's metre (or its own best, if there's no one metre).
    pub fit: Option<Fit>,
}

/// What a passage's metre is, line by line.
#[derive(Clone, Debug, PartialEq)]
pub struct Passage {
    pub lines: Vec<ScannedLine>,
    /// The metre most of the passage keeps, when it keeps one clearly enough to be named.
    pub metre: Option<Metre>,
    /// Lines with no words (blank lines between verses) are left out, and so aren't in `lines`.
    pub guessed: Vec<String>,
}

/// The most that can be wrong, about a syllable in six, before a passage is said to have no clear metre.
const MOST_WRONG: f32 = 0.16;
/// The most a line may be out, in syllables, from the metre before the passage is said not to keep it.
const MOST_OUT: usize = 2;

/// Scans a passage of verse: the metre it mostly keeps and how each line stands to it.
pub fn scan_passage(lines: &[&str]) -> Passage {
    let mut scanned = Vec::new();
    let mut guessed: Vec<String> = Vec::new();
    for (index, line) in lines.iter().enumerate() {
        let words = scan_line(line);
        if words.is_empty() {
            continue;
        }
        for w in words.iter().filter(|w| w.guessed) {
            if !guessed.contains(&w.word.to_lowercase()) {
                guessed.push(w.word.to_lowercase());
            }
        }
        let kinds: Vec<Kind> = words.iter().flat_map(|w| w.kinds.clone()).collect();
        scanned.push((index, words, kinds));
    }
    let total: usize = scanned.iter().map(|(_, _, k)| k.len()).sum();
    // The metre whose cost over every line is least.
    let mut chosen: Option<(Metre, f32)> = None;
    if total > 0 {
        for foot in Foot::ALL {
            for feet in 1..=8 {
                let metre = Metre { foot, feet };
                let fits: Vec<Fit> = scanned.iter().map(|(_, _, k)| fit_kinds(k, metre)).collect();
                // Prose can fit a metre by giving way a little everywhere, but not by lines of another length.
                if fits.iter().any(|f| f.syllables.abs_diff(f.expected) > MOST_OUT) {
                    continue;
                }
                let cost: f32 = fits.iter().map(|f| f.cost).sum();
                if chosen.is_none_or(|(_, c)| cost + 1e-4 < c) {
                    chosen = Some((metre, cost));
                }
            }
        }
    }
    let metre = chosen.filter(|(_, cost)| total >= 4 && *cost <= MOST_WRONG * total as f32).map(|(m, _)| m);
    let lines = scanned
        .into_iter()
        .map(|(index, words, kinds)| {
            let fit = match metre {
                Some(m) => Some(fit_kinds(&kinds, m)),
                None => best_fit(&kinds),
            };
            ScannedLine { index, syllables: kinds.len(), words, fit }
        })
        .collect();
    Passage { lines, metre, guessed }
}

/// A line's marks: `true` for a syllable heard as a beat, in order, for showing as ¯ and ˘ (or – and u).
pub fn marks_of(words: &[Scanned]) -> Vec<bool> {
    words.iter().flat_map(|w| w.kinds.iter().map(|k| k.is_stressed())).collect()
}

/// Something a fixed-form check has to say.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Remark {
    /// The poem has the wrong number of lines.
    Lines { found: usize, wanted: usize },
    /// A line has too many or too few syllables. A form with a range of them gives `wanted` as its middle.
    Syllables { line: usize, found: usize, low: usize, high: usize },
    /// Two lines that should rhyme don't.
    Rhyme { first: usize, second: usize, first_word: String, second_word: String },
    /// A line has the wrong number of beats.
    Beats { line: usize, found: usize, low: usize, high: usize },
    /// Words whose syllables are only guessed, so a count may be out.
    Guessed(Vec<String>),
}

/// The result of checking a form.
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct Check {
    /// Syllables of each line, as counted.
    pub counts: Vec<usize>,
    pub remarks: Vec<Remark>,
}

impl Check {
    /// Whether it's right, apart from words that are only guessed at.
    pub fn is_ok(&self) -> bool {
        self.remarks.iter().all(|r| matches!(r, Remark::Guessed(_)))
    }
}

fn used_lines<'a>(lines: &[&'a str]) -> Vec<&'a str> {
    lines.iter().copied().filter(|l| !l.trim().is_empty()).collect()
}

fn guessed_in(lines: &[&str]) -> Vec<String> {
    let mut guessed: Vec<String> = Vec::new();
    for line in lines {
        for g in line_syllables(line).guessed {
            if !guessed.contains(&g) {
                guessed.push(g);
            }
        }
    }
    guessed
}

/// A haiku: three lines of five, seven and five syllables.
pub fn check_haiku(lines: &[&str]) -> Check {
    let lines = used_lines(lines);
    let mut check = Check { counts: lines.iter().map(|l| line_syllables(l).syllables).collect(), ..Check::default() };
    if lines.len() != 3 {
        check.remarks.push(Remark::Lines { found: lines.len(), wanted: 3 });
    }
    for (i, (found, wanted)) in check.counts.iter().zip([5, 7, 5]).enumerate() {
        if *found != wanted && i < 3 {
            check.remarks.push(Remark::Syllables { line: i + 1, found: *found, low: wanted, high: wanted });
        }
    }
    let guessed = guessed_in(&lines);
    if !guessed.is_empty() {
        check.remarks.push(Remark::Guessed(guessed));
    }
    check
}

/// The last word of a line, without what's after it.
fn last_word(line: &str) -> String {
    words_in(line).pop().map_or_else(String::new, |(_, _, w)| w)
}

/// The beats a line has when read plainly.
pub fn beats(line: &str) -> usize {
    syllable_kinds(line).iter().filter(|k| k.is_stressed()).count()
}

/// A limerick: five lines, the first, second and fifth rhyming with each other (about eight or nine syllables, three
/// beats) and the third and fourth with each other (five or six syllables, two beats).
pub fn check_limerick(lines: &[&str]) -> Check {
    let lines = used_lines(lines);
    let mut check = Check { counts: lines.iter().map(|l| line_syllables(l).syllables).collect(), ..Check::default() };
    if lines.len() != 5 {
        check.remarks.push(Remark::Lines { found: lines.len(), wanted: 5 });
    }
    let long = |i: usize| i != 2 && i != 3;
    for (i, line) in lines.iter().enumerate().take(5) {
        let found = check.counts[i];
        let (low, high) = if long(i) { (7, 10) } else { (5, 7) };
        if found < low || found > high {
            check.remarks.push(Remark::Syllables { line: i + 1, found, low, high });
        }
        let b = beats(line);
        let (low, high) = if long(i) { (3, 4) } else { (2, 3) };
        if b < low || b > high {
            check.remarks.push(Remark::Beats { line: i + 1, found: b, low, high });
        }
    }
    if lines.len() == 5 {
        for (a, b) in [(0, 1), (0, 4), (2, 3)] {
            // If one of three that should rhyme doesn't, the first pair has said so; the third is told too.
            let (wa, wb) = (last_word(lines[a]), last_word(lines[b]));
            if !rhymes_with(&wa, &wb) {
                check.remarks.push(Remark::Rhyme { first: a + 1, second: b + 1, first_word: wa, second_word: wb });
            }
        }
    }
    let guessed = guessed_in(&lines);
    if !guessed.is_empty() {
        check.remarks.push(Remark::Guessed(guessed));
    }
    check
}

#[cfg(test)]
mod tests {
    use super::*;

    fn passage(lines: &[&str]) -> Passage {
        scan_passage(lines)
    }

    #[test]
    fn a_sonnet_line_is_iambic_pentameter() {
        let p = passage(&["Shall I compare thee to a summer's day?"]);
        assert_eq!(p.metre, Some(Metre { foot: Foot::Iamb, feet: 5 }));
        assert_eq!(p.metre.unwrap().name(), "iambic pentameter");
        let fit = p.lines[0].fit.as_ref().unwrap();
        assert_eq!(fit.ending, Ending::Full);
        assert_eq!(p.lines[0].syllables, 10);
        assert!(fit.cost < 1.5, "{}", fit.cost);
    }

    #[test]
    fn a_whole_passage_keeps_one_metre_and_a_feminine_ending_is_allowed() {
        let p = passage(&["To be, or not to be, that is the question:", "Whether 'tis nobler in the mind to suffer", "The slings and arrows of outrageous fortune"]);
        assert_eq!(p.metre, Some(Metre { foot: Foot::Iamb, feet: 5 }), "{:?}", p.lines.iter().map(|l| l.fit.as_ref().map(|f| f.cost)).collect::<Vec<_>>());
        assert_eq!(p.lines[0].fit.as_ref().unwrap().ending, Ending::Extra);
    }

    #[test]
    fn trochees_are_told_from_iambs() {
        let p = passage(&["Tyger Tyger, burning bright", "In the forests of the night"]);
        assert_eq!(p.metre, Some(Metre { foot: Foot::Trochee, feet: 4 }));
        assert_eq!(p.lines[0].fit.as_ref().unwrap().ending, Ending::Short);
        assert_eq!(Metre { foot: Foot::Trochee, feet: 4 }.name(), "trochaic tetrameter");
    }

    #[test]
    fn anapaests_and_dactyls_are_found() {
        let p = passage(&["Twas the night before Christmas, when all through the house", "Not a creature was stirring, not even a mouse"]);
        assert_eq!(p.metre.map(|m| m.foot), Some(Foot::Anapaest), "{:?}", p.metre);
        let d = passage(&["Just for a handful of silver he left us", "Just for a riband to stick in his coat"]);
        assert_eq!(d.metre.map(|m| m.foot), Some(Foot::Dactyl), "{:?}", d.metre);
    }

    #[test]
    fn prose_has_no_one_metre() {
        for lines in [
            ["Please remember to bring the receipts from the shop on Thursday", "Nobody has seen the quarterly accounts yet"],
            ["The committee will meet on Tuesday to discuss the new proposals", "Several members have raised concerns about the budget"],
            ["It was a really quiet evening, and the garden was quickly turning dark.", "She walked slowly along the garden path, very carefully avoiding the wet stones."],
        ] {
            assert_eq!(passage(&lines).metre, None, "{lines:?}");
        }
        let p = passage(&["Please remember to bring the receipts from the shop on Thursday", "Nobody has seen the quarterly accounts yet"]);
        assert!(p.lines.iter().all(|l| l.fit.is_some()));
    }

    #[test]
    fn a_line_that_leaves_the_metre_says_where() {
        // A syllable too many.
        let p = passage(&["Shall I compare thee to a summer's day", "Thou art more lovely and more temperate", "Rough winds do shake the darling buds of May and June"]);
        let last = p.lines[2].fit.as_ref().unwrap();
        assert!(!last.off.is_empty(), "{last:?}");
        assert_eq!(last.syllables, 12);
        assert!(p.lines[0].fit.as_ref().unwrap().off.is_empty() || p.lines[0].fit.as_ref().unwrap().cost < 1.5);
    }

    #[test]
    fn syllable_marks_are_beats_and_gaps() {
        let words = scan_line("The water");
        assert_eq!(marks_of(&words), [false, true, false]);
    }

    #[test]
    fn haiku_pass_and_fail() {
        let good = check_haiku(&["An old silent pond", "A frog jumps into the pond", "Splash! Silence again"]);
        assert_eq!(good.counts, [5, 7, 5], "{good:?}");
        assert!(good.is_ok());
        let bad = check_haiku(&["An old silent pond", "A frog jumps into the cold pond", "Splash! Silence again"]);
        assert!(!bad.is_ok());
        assert!(bad.remarks.contains(&Remark::Syllables { line: 2, found: 8, low: 7, high: 7 }));
        let two = check_haiku(&["An old silent pond", "A frog jumps into the pond"]);
        assert!(two.remarks.contains(&Remark::Lines { found: 2, wanted: 3 }));
        let blanks = check_haiku(&["An old silent pond", "", "A frog jumps into the pond", "Splash! Silence again"]);
        assert!(blanks.is_ok(), "blank lines don't count");
    }

    #[test]
    fn limerick_pass_and_fail() {
        let good = check_limerick(&["There was an old man from Peru", "Who dreamed he was eating his shoe", "He awoke in the night", "With a terrible fright", "And found it was perfectly true"]);
        assert!(good.is_ok(), "{good:?}");
        let bad = check_limerick(&["There was an old man from Peru", "Who dreamed he was eating his shoe", "He awoke in the night", "With a terrible fright", "And found that the weather was cold"]);
        assert!(bad.remarks.iter().any(|r| matches!(r, Remark::Rhyme { first: 1, second: 5, .. })), "{bad:?}");
        let short = check_limerick(&["There was an old man from Peru", "Who dreamed he was eating his shoe"]);
        assert!(short.remarks.contains(&Remark::Lines { found: 2, wanted: 5 }));
        let long = check_limerick(&["There was an old man from Peru", "Who dreamed he was eating his shoe", "He awoke in the middle of the night with a start", "With a terrible fright", "And found it was perfectly true"]);
        assert!(long.remarks.iter().any(|r| matches!(r, Remark::Syllables { line: 3, .. })), "{long:?}");
    }
}
