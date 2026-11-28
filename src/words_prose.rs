//! Prose: words that come round again too soon, filler words, and adverbs, found line by line for the editor to
//! underline. Plain English only, and a nudge, not a rule: a good writer keeps some of each.

use crate::words::words_in;
use std::collections::HashMap;

/// What a mark says about a word.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Kind {
    /// The same word used again a few words on.
    Repeat,
    /// A word that adds little: "very", "really", "just".
    Filler,
    /// An adverb in -ly.
    Adverb,
}

/// A word marked in a line.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Mark {
    /// Where the word is, by character.
    pub from: usize,
    pub to: usize,
    pub kind: Kind,
    /// For a repeat, how many words away the other use is.
    pub apart: usize,
}

/// How many words apart a word may be used again before it's marked.
pub const DEFAULT_DISTANCE: usize = 20;

/// Words that may be used again and again without it being a fault.
const COMMON: &[&str] = &[
    "that", "with", "have", "this", "from", "they", "were", "been", "said", "there", "their", "would", "could", "should", "about", "which", "when", "what", "will", "then", "than", "them", "into", "over", "your", "some", "more", "also", "like", "were", "here", "just", "very", "such", "only", "other", "these", "those", "where", "while", "after", "before", "again", "still", "much", "many", "most", "even", "does", "done", "made", "make", "back", "down", "away", "each", "every", "being", "because", "though", "through", "whom", "whose", "must", "might", "shall", "upon", "within", "without", "between", "under", "along", "around", "against", "until", "since", "once", "both", "same", "next", "last", "well", "going", "come", "came", "goes", "went", "know", "take", "took", "see", "saw", "get", "got", "put", "let", "say", "says",
];

/// Words that add little, which the writer is usually better without.
const FILLER: &[&str] = &[
    "very", "really", "just", "quite", "rather", "somewhat", "actually", "basically", "literally", "totally", "completely", "absolutely", "definitely", "certainly", "simply", "essentially", "virtually", "practically", "honestly", "truly", "seriously", "extremely", "fairly", "surely", "clearly", "obviously", "apparently", "perhaps", "maybe", "somehow", "kinda", "sorta", "anyway", "indeed", "mostly", "generally", "particularly", "especially", "highly", "pretty",
];

/// Words that end in -ly and aren't adverbs, or aren't ones worth marking.
const NOT_ADVERBS: &[&str] = &[
    "family", "only", "early", "fly", "july", "apply", "reply", "supply", "rely", "ally", "belly", "bully", "jelly", "holy", "ugly", "lovely", "friendly", "lonely", "silly", "lively", "likely", "unlikely", "deadly", "costly", "elderly", "orderly", "daily", "weekly", "monthly", "yearly", "hourly", "nightly", "anomaly", "assembly", "butterfly", "comply", "imply", "multiply", "homily", "melancholy", "monopoly", "rally", "tally", "sally", "ally", "curly", "surly", "burly", "oily", "wobbly", "gnarly", "jolly", "bubbly", "grizzly", "chilly", "hilly", "silly", "frilly", "smelly", "wily", "shapely", "stately", "timely", "manly", "womanly", "scholarly", "cowardly", "heavenly", "motherly", "fatherly", "brotherly", "sisterly", "earthly", "worldly", "ghostly", "ghastly", "lowly", "gravelly", "sickly", "underlying",
];

fn is_adverb(word: &str) -> bool {
    word.len() >= 5 && word.ends_with("ly") && !NOT_ADVERBS.contains(&word) && !FILLER.contains(&word)
}

/// A word with an -s taken off, so that "road" and "roads" are the same word for finding repeats.
fn stem(word: &str) -> &str {
    if word.len() > 4 && word.ends_with('s') && !word.ends_with("ss") { &word[..word.len() - 1] } else { word }
}

/// Which kinds to look for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Wanted {
    pub repeats: bool,
    pub filler: bool,
    pub adverbs: bool,
    pub distance: usize,
}

/// A line's words with their places, lower-cased.
fn tokens(line: &str) -> Vec<(usize, usize, String)> {
    words_in(line).into_iter().map(|(a, z, w)| (a, z, w.replace('\u{2019}', "'").to_lowercase())).collect()
}

/// The marks in the lines `first..=last` of a text, given its lines: those wanted by `wanted`. Words used again are
/// looked for among the words before and after, over as many lines before and after as it takes to reach the
/// distance, so a line's marks don't depend on which lines happen to be on the screen.
pub fn marks(lines: &dyn Fn(usize) -> String, count: usize, first: usize, last: usize, wanted: Wanted) -> HashMap<usize, Vec<Mark>> {
    let mut found: HashMap<usize, Vec<Mark>> = HashMap::new();
    if count == 0 || first >= count {
        return found;
    }
    let last = last.min(count - 1);
    let (from, to) = if wanted.repeats {
        // Back and forward until a distance's worth of words, or the most lines it's worth reading.
        let mut from = first;
        let mut words = 0;
        while from > 0 && words < wanted.distance && first - from < 60 {
            from -= 1;
            words += words_in(&lines(from)).len();
        }
        let mut to = last;
        let mut words = 0;
        while to + 1 < count && words < wanted.distance && to - last < 60 {
            to += 1;
            words += words_in(&lines(to)).len();
        }
        (from, to)
    } else {
        (first, last)
    };
    // Every word from `from` to `to`, in order, with its line.
    let mut all: Vec<(usize, usize, usize, String)> = Vec::new();
    for line in from..=to {
        for (a, z, w) in tokens(&lines(line)) {
            all.push((line, a, z, w));
        }
    }
    for (index, (line, a, z, word)) in all.iter().enumerate() {
        if *line < first || *line > last {
            continue;
        }
        let mut push = |kind: Kind, apart: usize| found.entry(*line).or_default().push(Mark { from: *a, to: *z, kind, apart });
        if wanted.filler && FILLER.contains(&word.as_str()) {
            push(Kind::Filler, 0);
        } else if wanted.adverbs && is_adverb(word) {
            push(Kind::Adverb, 0);
        }
        if wanted.repeats && word.chars().count() >= 4 && !COMMON.contains(&word.as_str()) {
            let key = stem(word);
            let near = (index.saturating_sub(wanted.distance)..all.len().min(index + wanted.distance + 1)).filter(|&other| other != index).filter(|&other| stem(&all[other].3) == key).min_by_key(|&other| other.abs_diff(index));
            if let Some(other) = near {
                push(Kind::Repeat, other.abs_diff(index));
            }
        }
    }
    found
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(text: &str, wanted: Wanted) -> Vec<(usize, String, Kind)> {
        let lines: Vec<String> = text.lines().map(str::to_string).collect();
        let get = |i: usize| lines.get(i).cloned().unwrap_or_default();
        let mut out = Vec::new();
        let found = marks(&get, lines.len(), 0, lines.len().max(1) - 1, wanted);
        let mut keys: Vec<_> = found.keys().copied().collect();
        keys.sort();
        for line in keys {
            for m in &found[&line] {
                out.push((line, lines[line].chars().skip(m.from).take(m.to - m.from).collect::<String>(), m.kind));
            }
        }
        out
    }

    const ALL: Wanted = Wanted { repeats: true, filler: true, adverbs: true, distance: DEFAULT_DISTANCE };

    #[test]
    fn repeats_close_together_are_found_in_both_places() {
        let found = run("The garden was quiet and the garden was dark.", ALL);
        let repeats: Vec<_> = found.iter().filter(|(_, _, k)| *k == Kind::Repeat).map(|(_, w, _)| w.as_str()).collect();
        assert_eq!(repeats, ["garden", "garden"]);
    }

    #[test]
    fn repeats_far_apart_and_common_words_are_not() {
        let far = "garden one two three four five six seven eight nine ten eleven twelve thirteen fourteen fifteen sixteen seventeen eighteen nineteen twenty twentyone garden";
        assert!(run(far, ALL).iter().all(|(_, _, k)| *k != Kind::Repeat));
        let common = "They said that this would be that way, with that thing.";
        assert!(run(common, ALL).iter().all(|(_, _, k)| *k != Kind::Repeat));
        // A short word isn't counted either.
        assert!(run("the cat sat on the mat and the cat", ALL).iter().all(|(_, w, k)| *k != Kind::Repeat || w == "cat"));
    }

    #[test]
    fn repeats_are_found_across_lines_whatever_is_asked_for() {
        let text = "She walked along the river.\nHe followed her along the river bank.";
        let lines: Vec<String> = text.lines().map(str::to_string).collect();
        let get = |i: usize| lines[i].clone();
        // Only the second line is asked about, and its repeat of the first line's word is still found.
        let found = marks(&get, 2, 1, 1, ALL);
        assert!(found[&1].iter().any(|m| m.kind == Kind::Repeat && lines[1].chars().skip(m.from).take(m.to - m.from).collect::<String>() == "river"));
        assert!(!found.contains_key(&0));
    }

    #[test]
    fn plurals_count_as_the_same_word() {
        let found = run("The road was long and the roads were longer.", ALL);
        assert!(found.iter().filter(|(_, _, k)| *k == Kind::Repeat).count() >= 2);
    }

    #[test]
    fn filler_and_adverbs_are_found() {
        let found = run("It was really quite a very slowly changing family affair, and he quickly left.", ALL);
        let get = |kind| found.iter().filter(|(_, _, k)| *k == kind).map(|(_, w, _)| w.clone()).collect::<Vec<_>>();
        assert_eq!(get(Kind::Filler), ["really", "quite", "very"]);
        assert_eq!(get(Kind::Adverb), ["slowly", "quickly"]);
    }

    #[test]
    fn each_kind_is_only_found_if_wanted() {
        let none = Wanted { repeats: false, filler: false, adverbs: false, distance: 20 };
        assert!(run("It was really slowly done and done again, really.", none).is_empty());
        let only_filler = Wanted { filler: true, ..none };
        let found = run("It was really slowly done.", only_filler);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].2, Kind::Filler);
    }
}
