//! Words: how they sound and what they mean, kept offline. Syllables, stress and rhymes come from the CMU
//! Pronouncing Dictionary, and synonyms and antonyms (by sense) from WordNet 3.1. English only for now.
//!
//! Both are packed into `data/words/` by `tools/gen_words.py` and built into the program (about 2.6 MB between
//! them), read the first time they're wanted. A word the dictionary doesn't have gets a rule-based guess for its
//! syllables and stress, and says it is a guess.
//!
//! Nothing here draws anything, so the song-lyrics tools can use it as it is: `syllables`, `stresses`, `rhymes`,
//! `line_syllables` and `rhymes_with`.

// Some of this is for the song-lyrics tools to come, and isn't used by the window yet.
#![allow(dead_code)]

use std::collections::HashMap;
use std::sync::OnceLock;

const CMU_PACKED: &[u8] = include_bytes!("../data/words/cmudict.bin");
const WORDNET_PACKED: &[u8] = include_bytes!("../data/words/wordnet.bin");

/// The most rhymes of one kind that are listed.
pub const MOST_RHYMES: usize = 600;

const CONSONANTS: [&str; 24] = ["B", "CH", "D", "DH", "F", "G", "HH", "JH", "K", "L", "M", "N", "NG", "P", "R", "S", "SH", "T", "TH", "V", "W", "Y", "Z", "ZH"];
const VOWELS: [&str; 15] = ["AA", "AE", "AH", "AO", "AW", "AY", "EH", "ER", "EY", "IH", "IY", "OW", "OY", "UH", "UW"];

fn unpack(bytes: &[u8]) -> Vec<u8> {
    miniz_oxide::inflate::decompress_to_vec(bytes).unwrap_or_default()
}

// ---------------------------------------------------------------------------------------------------------------
// Sounds
// ---------------------------------------------------------------------------------------------------------------

/// One sound as the packed dictionary keeps it: 1 to 24 for a consonant, and from 32 a vowel with its stress.
fn is_vowel(code: u8) -> bool {
    code >= 32
}

/// A vowel's stress: 1 is the main one, 2 a lesser one, 0 none.
fn stress_of(code: u8) -> u8 {
    (code - 32) % 3
}

fn without_stress(code: u8) -> u8 {
    if is_vowel(code) { code - stress_of(code) } else { code }
}

/// The sounds of a word as CMU writes them, for showing, such as `K AE1 T`.
pub fn spelled_sounds(sounds: &[u8]) -> String {
    sounds
        .iter()
        .map(|&code| {
            if is_vowel(code) {
                format!("{}{}", VOWELS[usize::from((code - 32) / 3)], stress_of(code))
            } else {
                CONSONANTS[usize::from(code) - 1].to_string()
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// The dictionary: every word in one buffer, in byte order, and its sounds beside it.
pub struct Dictionary {
    letters: Vec<u8>,
    sounds: Vec<u8>,
    entries: Vec<Entry>,
}

#[derive(Clone, Copy)]
struct Entry {
    word: u32,
    word_len: u8,
    sound: u32,
    sound_len: u8,
}

impl Dictionary {
    fn read(packed: &[u8]) -> Self {
        let data = unpack(packed);
        let mut letters = Vec::with_capacity(data.len());
        let mut sounds = Vec::new();
        let mut entries = Vec::with_capacity(130_000);
        let mut last: Vec<u8> = Vec::new();
        let mut at = 0;
        while at + 2 <= data.len() {
            let (shared, add) = (usize::from(data[at]), usize::from(data[at + 1]));
            at += 2;
            if at + add > data.len() || shared > last.len() {
                break;
            }
            last.truncate(shared);
            last.extend_from_slice(&data[at..at + add]);
            at += add;
            let end = data[at..].iter().position(|&b| b == 0).map_or(data.len(), |p| at + p);
            let entry = Entry { word: letters.len() as u32, word_len: last.len() as u8, sound: sounds.len() as u32, sound_len: (end - at) as u8 };
            letters.extend_from_slice(&last);
            sounds.extend_from_slice(&data[at..end]);
            entries.push(entry);
            at = end + 1;
        }
        Dictionary { letters, sounds, entries }
    }

    pub fn word_count(&self) -> usize {
        self.entries.len()
    }

    fn word_of(&self, e: &Entry) -> &str {
        std::str::from_utf8(&self.letters[e.word as usize..e.word as usize + usize::from(e.word_len)]).unwrap_or("")
    }

    fn sounds_of(&self, e: &Entry) -> &[u8] {
        &self.sounds[e.sound as usize..e.sound as usize + usize::from(e.sound_len)]
    }

    /// The sounds of a word (lower case), if the dictionary has it.
    pub fn find(&self, word: &str) -> Option<&[u8]> {
        let at = self.entries.binary_search_by(|e| self.word_of(e).as_bytes().cmp(word.as_bytes())).ok()?;
        Some(self.sounds_of(&self.entries[at]))
    }

    fn all(&self) -> impl Iterator<Item = (&str, &[u8])> {
        self.entries.iter().map(|e| (self.word_of(e), self.sounds_of(e)))
    }
}

/// The pronouncing dictionary, read the first time it's asked for.
pub fn dictionary() -> &'static Dictionary {
    static DICTIONARY: OnceLock<Dictionary> = OnceLock::new();
    DICTIONARY.get_or_init(|| Dictionary::read(CMU_PACKED))
}

// ---------------------------------------------------------------------------------------------------------------
// Syllables and stress
// ---------------------------------------------------------------------------------------------------------------

/// How hard a syllable is stressed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Stress {
    None,
    Secondary,
    Primary,
}

impl Stress {
    fn from_number(n: u8) -> Stress {
        match n {
            1 => Stress::Primary,
            2 => Stress::Secondary,
            _ => Stress::None,
        }
    }
}

/// How a word sounds: a stress for each of its syllables, and whether that's known or guessed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Spoken {
    pub stresses: Vec<Stress>,
    /// True when the word isn't in the dictionary and its syllables and stress come from rules.
    pub guessed: bool,
    /// The sounds, when the dictionary has them (CMU's, packed; see `spelled_sounds`).
    pub sounds: Option<Vec<u8>>,
}

impl Spoken {
    pub fn syllables(&self) -> usize {
        self.stresses.len()
    }
}

/// The lower-case form a word is looked up by: curly apostrophes made straight.
fn normal(word: &str) -> String {
    word.chars().map(|c| if c == '\u{2019}' || c == '\u{2018}' { '\'' } else { c }).flat_map(char::to_lowercase).collect()
}

/// How a word sounds, from the dictionary, or failing that from its stem and ending, or from the rules.
pub fn say(word: &str) -> Spoken {
    let word = normal(word);
    let word = word.trim_matches(|c: char| !c.is_alphanumeric());
    let dict = dictionary();
    let known = |w: &str| dict.find(w).map(<[u8]>::to_vec);
    if let Some(sounds) = known(word).or_else(|| known(&format!("'{word}"))) {
        return from_sounds(sounds, false);
    }
    // Plurals, tenses and the like of a word that is there: its sounds with an ending's syllable added or not.
    for (ending, adds) in [("'s", 0), ("s", 0), ("es", 1), ("ed", 0), ("d", 0), ("ing", 1), ("ly", 1), ("er", 1)] {
        if let Some(stem) = word.strip_suffix(ending).filter(|s| s.len() >= 3)
            && let Some(sounds) = known(stem).or_else(|| known(&format!("{stem}e")))
        {
            let mut stresses = from_sounds(sounds.clone(), true).stresses;
            // -ed and -es add a syllable only after the sounds that need one.
            let last = sounds.last().copied().map(without_stress);
            let needs = match ending {
                "ed" | "d" => last.is_some_and(|c| c == 18 || c == 3),
                "es" | "s" | "'s" => last.is_some_and(|c| [16, 17, 23, 24, 2, 8].contains(&c)),
                _ => false,
            };
            if adds == 1 && !matches!(ending, "es") || needs {
                stresses.push(Stress::None);
            }
            return Spoken { stresses, guessed: true, sounds: None };
        }
    }
    Spoken { stresses: guess_stresses(word), guessed: true, sounds: None }
}

fn from_sounds(sounds: Vec<u8>, guessed: bool) -> Spoken {
    let stresses = sounds.iter().filter(|&&c| is_vowel(c)).map(|&c| Stress::from_number(stress_of(c))).collect();
    Spoken { stresses, guessed, sounds: Some(sounds) }
}

/// The number of syllables in a word.
pub fn syllables(word: &str) -> usize {
    say(word).syllables()
}

/// A stress for each syllable of a word.
pub fn stresses(word: &str) -> Vec<Stress> {
    say(word).stresses
}

/// Syllables by rule, for a word the dictionary hasn't got: a group of vowel letters is a syllable, less a silent
/// final e, and with the usual groups that are two (the "ia" of "media").
pub fn guess_syllables(word: &str) -> usize {
    let letters: Vec<char> = word.chars().filter(|c| c.is_alphabetic()).flat_map(char::to_lowercase).collect();
    if letters.is_empty() {
        return 0;
    }
    let vowel = |c: char| "aeiouy".contains(c);
    let mut count = 0;
    let mut in_group = false;
    for (i, &c) in letters.iter().enumerate() {
        if vowel(c) {
            if !in_group {
                count += 1;
            } else if i > 0 {
                // Vowels that don't make one sound: "ia", "io", "eo", "ua" and "uo" (but not after q or g).
                let pair = (letters[i - 1], c);
                let after = if i >= 2 { letters[i - 2] } else { ' ' };
                if matches!(pair, ('i', 'a') | ('i', 'o') | ('e', 'o') | ('i', 'u')) || (matches!(pair, ('u', 'a') | ('u', 'o')) && after != 'q' && after != 'g') {
                    count += 1;
                }
            }
            in_group = true;
        } else {
            in_group = false;
        }
    }
    let word: String = letters.iter().collect();
    // A final e is silent, except in "-le" after a consonant, and a word like "the" or "be".
    if word.ends_with('e') && count > 1 && !(word.ends_with("le") && letters.len() > 2 && !vowel(letters[letters.len() - 3])) && !word.ends_with("ee") && !word.ends_with("ie") {
        count -= 1;
    }
    // -ed and -es are silent unless after t or d (-ed), or after s, x, z, ch, sh, c, g (-es).
    if count > 1 {
        let before_ending = if letters.len() > 3 { letters[letters.len() - 3] } else { ' ' };
        let silent_ed = word.ends_with("ed") && letters.len() > 3 && !matches!(before_ending, 't' | 'd') && !vowel(before_ending);
        let silent_es = word.ends_with("es") && letters.len() > 3 && !matches!(before_ending, 's' | 'x' | 'z' | 'c' | 'g' | 'h' | 'i' | 'o') && !vowel(before_ending);
        if silent_ed || silent_es {
            count -= 1;
        }
    }
    count.max(1)
}

/// A stress for each syllable of a word the dictionary hasn't got, by rule: a short word stresses its first
/// syllable unless it starts with a prefix like "be-"; an ending like "-tion" or "-ic" stresses the syllable before
/// it; otherwise the third from the end.
pub fn guess_stresses(word: &str) -> Vec<Stress> {
    let n = guess_syllables(word);
    let mut out = vec![Stress::None; n];
    if n == 0 {
        return out;
    }
    let word = word.to_lowercase();
    let at = if n == 1 {
        0
    } else if ["tion", "sion", "ic", "ical", "ity", "ify", "ian", "ious", "eous"].iter().any(|e| word.ends_with(e)) {
        n.saturating_sub(if word.ends_with("ical") || word.ends_with("ity") || word.ends_with("ify") || word.ends_with("ious") || word.ends_with("eous") { 3 } else { 2 })
    } else if n == 2 {
        usize::from(["a", "be", "de", "re", "ex", "dis", "un", "en", "em", "con", "com", "pre", "per", "mis", "ab", "ad", "ap", "ac"].iter().any(|p| word.starts_with(p) && word.len() > p.len() + 2))
    } else {
        n - 3
    };
    out[at.min(n - 1)] = Stress::Primary;
    out
}

/// The syllables in a line of text, and the words in it whose syllables are only guessed.
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct LineCount {
    pub syllables: usize,
    pub guessed: Vec<String>,
}

/// Every word of a line: runs of letters, with apostrophes (and curly ones) inside them. A hyphen splits a word.
pub fn words_in(line: &str) -> Vec<(usize, usize, String)> {
    let mut out = Vec::new();
    let chars: Vec<char> = line.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        if chars[i].is_alphabetic() {
            let start = i;
            while i < chars.len() && (chars[i].is_alphabetic() || ((chars[i] == '\'' || chars[i] == '\u{2019}') && i + 1 < chars.len() && chars[i + 1].is_alphabetic() && i > start)) {
                i += 1;
            }
            out.push((start, i, chars[start..i].iter().collect()));
        } else {
            i += 1;
        }
    }
    out
}

/// How many syllables a line of text has, as it would be spoken.
pub fn line_syllables(line: &str) -> LineCount {
    let mut count = LineCount::default();
    for (_, _, word) in words_in(line) {
        let spoken = say(&word);
        count.syllables += spoken.syllables();
        if spoken.guessed && !count.guessed.contains(&word.to_lowercase()) {
            count.guessed.push(word.to_lowercase());
        }
    }
    count
}

// ---------------------------------------------------------------------------------------------------------------
// Rhymes
// ---------------------------------------------------------------------------------------------------------------

/// How closely two words rhyme.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum RhymeKind {
    /// The same sounds from the last stressed vowel on, after a different sound before it: "light" and "night".
    Perfect,
    /// Alike but not the same: the vowel with a like ending ("time" and "line"), or the ending with another vowel.
    Near,
    /// A perfect rhyme of two syllables or more: "ending" and "bending", "attention" and "intention".
    Multi,
}

/// A word that rhymes, and how many syllables it has.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Rhyme {
    pub word: String,
    pub syllables: usize,
}

/// Where a word's rhyming part begins: its last vowel with the main stress, else its last with any, else its last.
fn tail_start(sounds: &[u8]) -> Option<usize> {
    let vowels = || sounds.iter().enumerate().rev().filter(|(_, c)| is_vowel(**c));
    vowels().find(|(_, c)| stress_of(**c) == 1).or_else(|| vowels().find(|(_, c)| stress_of(**c) == 2)).or_else(|| vowels().next()).map(|(i, _)| i)
}

/// Where the rhyming part begins if it must take in at least two syllables.
fn long_tail_start(sounds: &[u8]) -> Option<usize> {
    let start = tail_start(sounds)?;
    if sounds[start..].iter().filter(|&&c| is_vowel(c)).count() >= 2 {
        return Some(start);
    }
    sounds[..start].iter().rposition(|&c| is_vowel(c))
}

/// The sounds that must match, stress ignored.
fn plain(sounds: &[u8]) -> Vec<u8> {
    sounds.iter().map(|&c| without_stress(c)).collect()
}

/// Sounds that nearly rhyme are treated as one: voiced and unvoiced pairs, and the nasals.
fn like(code: u8) -> u8 {
    if is_vowel(code) {
        return without_stress(code);
    }
    match CONSONANTS[usize::from(code) - 1] {
        "B" | "P" => 1,
        "D" | "T" => 3,
        "G" | "K" => 6,
        "V" | "F" => 5,
        "Z" | "S" => 16,
        "ZH" | "SH" => 17,
        "DH" | "TH" => 19,
        "JH" | "CH" => 2,
        "M" | "N" | "NG" => 12,
        _ => code,
    }
}

/// What a word's rhyming part is, for comparing.
struct Tail {
    /// The sound before it, when there's one.
    before: Option<u8>,
    sounds: Vec<u8>,
    syllables: usize,
}

fn tail_of(sounds: &[u8], start: usize) -> Tail {
    Tail { before: start.checked_sub(1).map(|i| without_stress(sounds[i])), sounds: plain(&sounds[start..]), syllables: sounds[start..].iter().filter(|&&c| is_vowel(c)).count() }
}

/// The words that rhyme with a word, of the kind asked for, by their syllables and then alphabetically. Words
/// WordNet knows come first, and only if there are few of them are the names and oddities of the dictionary added.
/// A word the dictionary hasn't got is matched by how it ends; `guessed` says so.
pub fn rhymes(word: &str, kind: RhymeKind) -> Rhymes {
    let word = normal(word);
    let word = word.trim_matches(|c: char| !c.is_alphanumeric()).to_string();
    let dict = dictionary();
    let (sounds, guessed) = match dict.find(&word) {
        Some(s) => (s.to_vec(), false),
        None => match sounds_by_ending(&word) {
            Some(s) => (s, true),
            None => return Rhymes { list: Vec::new(), guessed: true },
        },
    };
    let (start, tail_kind) = match kind {
        RhymeKind::Multi => (long_tail_start(&sounds), true),
        _ => (tail_start(&sounds), false),
    };
    let Some(start) = start else {
        return Rhymes { list: Vec::new(), guessed };
    };
    let mine = tail_of(&sounds, start);
    if tail_kind && mine.syllables < 2 {
        return Rhymes { list: Vec::new(), guessed };
    }
    let mine_like: Vec<u8> = mine.sounds.iter().map(|&c| like(c)).collect();
    let mine_coda: Vec<u8> = mine.sounds.iter().skip(1).copied().collect();
    let thesaurus = thesaurus();
    let (mut known, mut other) = (Vec::new(), Vec::new());
    for (other_word, other_sounds) in dict.all() {
        if other_word == word || other_sounds.is_empty() || !other_word.chars().all(|c| c.is_ascii_alphabetic() || c == '\'') {
            continue;
        }
        let Some(other_start) = (if tail_kind { long_tail_start(other_sounds) } else { tail_start(other_sounds) }) else {
            continue;
        };
        let theirs = tail_of(other_sounds, other_start);
        let same_onset = theirs.before == mine.before;
        let matches = match kind {
            RhymeKind::Perfect | RhymeKind::Multi => theirs.sounds == mine.sounds && !same_onset && (kind == RhymeKind::Perfect || theirs.syllables >= 2),
            RhymeKind::Near => {
                let near_tail = theirs.sounds.iter().map(|&c| like(c)).collect::<Vec<_>>() == mine_like;
                // The same ending after another vowel, in a tail of one syllable.
                let same_coda = mine.syllables == 1 && theirs.syllables == 1 && theirs.sounds.len() > 1 && theirs.sounds[1..] == mine_coda[..] && !mine_coda.is_empty();
                (near_tail || same_coda) && theirs.sounds != mine.sounds
            }
        };
        if !matches {
            continue;
        }
        let rhyme = Rhyme { word: other_word.to_string(), syllables: other_sounds.iter().filter(|&&c| is_vowel(c)).count() };
        if thesaurus.knows(other_word) { known.push(rhyme) } else { other.push(rhyme) }
    }
    let mut list = known;
    if list.len() < 40 {
        other.retain(|r| !r.word.contains('\'') && r.word.len() > 2);
        other.truncate(100);
        list.extend(other);
    }
    list.sort_by(|a, b| a.syllables.cmp(&b.syllables).then_with(|| a.word.cmp(&b.word)));
    // Of a long list the shorter words are the likelier ones to be wanted.
    if list.len() > MOST_RHYMES {
        list.sort_by(|a, b| a.syllables.cmp(&b.syllables).then_with(|| a.word.len().cmp(&b.word.len())).then_with(|| a.word.cmp(&b.word)));
        list.truncate(MOST_RHYMES);
        list.sort_by(|a, b| a.syllables.cmp(&b.syllables).then_with(|| a.word.cmp(&b.word)));
    }
    Rhymes { list, guessed }
}

/// The rhymes found, and whether the word's own sounds were a guess from how it's spelt.
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct Rhymes {
    pub list: Vec<Rhyme>,
    pub guessed: bool,
}

/// A word the dictionary hasn't got, given the sounds of the words that end the way it's spelt: the commonest
/// ending sounds of the words ending in its last five, four or three letters.
fn sounds_by_ending(word: &str) -> Option<Vec<u8>> {
    let letters: Vec<char> = word.chars().filter(|c| c.is_ascii_alphabetic()).collect();
    let dict = dictionary();
    for take in (3..=5.min(letters.len())).rev() {
        let ending: String = letters[letters.len() - take..].iter().collect();
        let mut seen: HashMap<Vec<u8>, usize> = HashMap::new();
        for (w, s) in dict.all() {
            if w.ends_with(&ending)
                && w.len() > take
                && !s.is_empty()
                && let Some(start) = tail_start(s)
            {
                *seen.entry(s[start..].to_vec()).or_insert(0) += 1;
            }
        }
        if let Some((best, _)) = seen.into_iter().max_by(|a, b| a.1.cmp(&b.1).then_with(|| b.0.cmp(&a.0))) {
            return Some(best);
        }
    }
    None
}

/// Whether two words rhyme perfectly (the same sounds from the last stressed vowel, after a different sound
/// before it), with a guess from the spelling if either isn't in the dictionary.
pub fn rhymes_with(a: &str, b: &str) -> bool {
    let (a, b) = (normal(a), normal(b));
    let (a, b) = (a.trim_matches(|c: char| !c.is_alphanumeric()), b.trim_matches(|c: char| !c.is_alphanumeric()));
    if a.is_empty() || a == b {
        return false;
    }
    let get = |w: &str| dictionary().find(w).map(<[u8]>::to_vec).or_else(|| sounds_by_ending(w));
    let (Some(sa), Some(sb)) = (get(a), get(b)) else {
        return false;
    };
    let (Some(ia), Some(ib)) = (tail_start(&sa), tail_start(&sb)) else {
        return false;
    };
    let (ta, tb) = (tail_of(&sa, ia), tail_of(&sb, ib));
    ta.sounds == tb.sounds && ta.before != tb.before
}

// ---------------------------------------------------------------------------------------------------------------
// The thesaurus
// ---------------------------------------------------------------------------------------------------------------

/// The kind of word a sense is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Part {
    Noun,
    Verb,
    Adjective,
    Adverb,
}

impl Part {
    pub fn label(self) -> &'static str {
        match self {
            Part::Noun => "noun",
            Part::Verb => "verb",
            Part::Adjective => "adjective",
            Part::Adverb => "adverb",
        }
    }
}

/// One meaning of a word, with the words that share it and the ones that oppose it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Sense {
    pub part: Part,
    pub definition: String,
    pub synonyms: Vec<String>,
    pub antonyms: Vec<String>,
}

struct Synset {
    part: Part,
    words: Vec<String>,
    definition: String,
    antonyms: Vec<String>,
}

pub struct Thesaurus {
    synsets: Vec<Synset>,
    by_word: HashMap<String, Vec<u32>>,
    irregular: HashMap<(u8, String), String>,
    /// Words that are only in senses left out of the data: they're known, but have nothing to offer.
    bare: std::collections::HashSet<String>,
}

impl Thesaurus {
    fn read(packed: &[u8]) -> Self {
        let text = String::from_utf8(unpack(packed)).unwrap_or_default();
        let mut synsets = Vec::new();
        let mut by_word: HashMap<String, Vec<u32>> = HashMap::new();
        let mut irregular = HashMap::new();
        let mut bare = std::collections::HashSet::new();
        let mut order: Vec<(String, Vec<u32>)> = Vec::new();
        let mut section = "";
        for line in text.lines() {
            if let Some(name) = line.strip_prefix('#') {
                section = match name {
                    "synsets" => "synsets",
                    "order" => "order",
                    "lemmas" => "lemmas",
                    _ => "exc",
                };
                continue;
            }
            let mut f = line.split('\t');
            match section {
                "synsets" => {
                    let part = match f.next() {
                        Some("n") => Part::Noun,
                        Some("v") => Part::Verb,
                        Some("a") | Some("s") => Part::Adjective,
                        _ => Part::Adverb,
                    };
                    let words: Vec<String> = f.next().unwrap_or("").split('|').map(str::to_string).collect();
                    let definition = f.next().unwrap_or("").to_string();
                    let antonyms: Vec<String> = f.next().unwrap_or("").split('|').filter(|s| !s.is_empty()).map(str::to_string).collect();
                    let id = synsets.len() as u32;
                    for w in &words {
                        let list = by_word.entry(w.to_lowercase()).or_default();
                        if !list.contains(&id) {
                            list.push(id);
                        }
                    }
                    synsets.push(Synset { part, words, definition, antonyms });
                }
                "order" => {
                    if let (Some(word), Some(ids)) = (f.next(), f.next()) {
                        order.push((word.to_string(), ids.split(' ').filter_map(|n| u32::from_str_radix(n, 36).ok()).collect()));
                    }
                }
                "lemmas" => {
                    bare.insert(line.to_string());
                }
                _ => {
                    if let (Some(p), Some(from), Some(to)) = (f.next(), f.next(), f.next()) {
                        irregular.entry((p.as_bytes()[0], from.to_string())).or_insert_with(|| to.to_string());
                    }
                }
            }
        }
        // The senses a word is most used in come first.
        for (word, ids) in order {
            by_word.insert(word, ids);
        }
        Thesaurus { synsets, by_word, irregular, bare }
    }

    pub fn sense_count(&self) -> usize {
        self.synsets.len()
    }

    /// The forms of a word WordNet might list it under, the word itself first: an irregular form ("mice" is
    /// "mouse") and then WordNet's rules for taking off endings.
    fn bases(&self, word: &str) -> Vec<String> {
        let mut out = vec![word.to_string()];
        let mut push = |s: String| {
            if !s.is_empty() && !out.contains(&s) {
                out.push(s);
            }
        };
        for part in *b"nva" {
            if let Some(base) = self.irregular.get(&(part, word.to_string())) {
                push(base.clone());
            }
        }
        let rules: [(&str, &str); 22] = [
            ("ies", "y"),
            ("ves", "f"),
            ("ses", "s"),
            ("xes", "x"),
            ("zes", "z"),
            ("ches", "ch"),
            ("shes", "sh"),
            ("men", "man"),
            ("es", "e"),
            ("es", ""),
            ("s", ""),
            ("ied", "y"),
            ("ed", "e"),
            ("ed", ""),
            ("ing", "e"),
            ("ing", ""),
            ("er", "e"),
            ("er", ""),
            ("est", "e"),
            ("est", ""),
            ("ier", "y"),
            ("iest", "y"),
        ];
        for (ending, replacement) in rules {
            if let Some(stem) = word.strip_suffix(ending) {
                push(format!("{stem}{replacement}"));
                // A doubled last letter: running, bigger.
                if matches!(ending, "ed" | "ing" | "er" | "est") {
                    let chars: Vec<char> = stem.chars().collect();
                    if chars.len() > 2 && chars[chars.len() - 1] == chars[chars.len() - 2] {
                        push(chars[..chars.len() - 1].iter().collect());
                    }
                }
            }
        }
        out
    }

    /// Whether WordNet knows a word, in the form it's in or one it comes from.
    pub fn knows(&self, word: &str) -> bool {
        let word = normal(word);
        self.bases(&word).iter().any(|b| self.by_word.contains_key(b) || self.bare.contains(b))
    }

    /// The meanings of a word: each with its kind, a short definition, the words that mean the same and the ones
    /// that mean the opposite, the most used first.
    pub fn senses(&self, word: &str) -> Vec<Sense> {
        let word = normal(word);
        let mut seen = Vec::new();
        let mut out = Vec::new();
        for base in self.bases(&word) {
            let Some(ids) = self.by_word.get(&base) else { continue };
            for &id in ids {
                if seen.contains(&id) {
                    continue;
                }
                seen.push(id);
                let s = &self.synsets[id as usize];
                let synonyms: Vec<String> = s.words.iter().filter(|w| w.to_lowercase() != base && w.to_lowercase() != word).cloned().collect();
                if synonyms.is_empty() && s.antonyms.is_empty() {
                    continue;
                }
                out.push(Sense { part: s.part, definition: s.definition.clone(), synonyms, antonyms: s.antonyms.clone() });
            }
            // A word found as itself needs no looking for its forms.
            if base == word {
                break;
            }
        }
        out
    }
}

/// WordNet's words, read the first time they're asked for.
pub fn thesaurus() -> &'static Thesaurus {
    static THESAURUS: OnceLock<Thesaurus> = OnceLock::new();
    THESAURUS.get_or_init(|| Thesaurus::read(WORDNET_PACKED))
}

/// The meanings of a word, with the words that share and oppose each.
pub fn senses(word: &str) -> Vec<Sense> {
    thesaurus().senses(word)
}

/// A replacement word in the capitals of the one it takes the place of: ALL CAPITALS, Initial capital, or lower case.
pub fn match_case(original: &str, replacement: &str) -> String {
    let letters: Vec<char> = original.chars().filter(|c| c.is_alphabetic()).collect();
    if letters.len() > 1 && letters.iter().all(|c| c.is_uppercase()) {
        replacement.to_uppercase()
    } else if letters.first().is_some_and(|c| c.is_uppercase()) {
        let mut chars = replacement.chars();
        chars.next().map_or_else(String::new, |first| first.to_uppercase().chain(chars).collect())
    } else {
        replacement.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_dictionary_loads_and_is_small() {
        let d = dictionary();
        assert!(d.word_count() > 120_000, "{}", d.word_count());
        assert!(thesaurus().sense_count() > 60_000);
        assert!(CMU_PACKED.len() + WORDNET_PACKED.len() < 3_300_000, "{}", CMU_PACKED.len() + WORDNET_PACKED.len());
        assert_eq!(spelled_sounds(d.find("cat").unwrap()), "K AE1 T");
    }

    #[test]
    fn syllables_and_stress_of_known_words() {
        assert_eq!(syllables("cat"), 1);
        assert_eq!(syllables("Beautiful"), 3);
        assert_eq!(syllables("don't"), 1);
        assert_eq!(syllables("don\u{2019}t"), 1);
        assert_eq!(stresses("pronunciation"), [Stress::None, Stress::Secondary, Stress::None, Stress::Primary, Stress::None]);
        assert_eq!(stresses("water"), [Stress::Primary, Stress::None]);
        assert_eq!(stresses("today"), [Stress::None, Stress::Primary]);
        assert_eq!(stresses("understand"), [Stress::Secondary, Stress::None, Stress::Primary]);
        assert!(!say("water").guessed);
    }

    #[test]
    fn guesses_are_marked_as_guesses() {
        let made_up = say("blorptastic");
        assert!(made_up.guessed);
        assert_eq!(made_up.syllables(), 3);
        assert_eq!(made_up.stresses.iter().filter(|s| **s == Stress::Primary).count(), 1);
        assert_eq!(guess_syllables("mate"), 1);
        assert_eq!(guess_syllables("table"), 2);
        assert_eq!(guess_syllables("walked"), 1);
        assert_eq!(guess_syllables("wanted"), 2);
        assert_eq!(guess_syllables("media"), 3);
        // An ending on a word that is there is worked out from it, and still says it's a guess.
        let walkers = say("zorgs");
        assert!(walkers.guessed);
        let count = line_syllables("The quick brown florbit jumps");
        assert_eq!(count.guessed, ["florbit"]);
        assert_eq!(count.syllables, 1 + 1 + 1 + 2 + 1);
    }

    #[test]
    fn lines_are_counted() {
        assert_eq!(line_syllables("Shall I compare thee to a summer's day?").syllables, 10);
        assert!(line_syllables("Shall I compare thee to a summer's day?").guessed.is_empty());
        assert_eq!(line_syllables("").syllables, 0);
        assert_eq!(line_syllables("an old silent pond").syllables, 5);
    }

    #[test]
    fn rhymes_of_each_kind() {
        let perfect = rhymes("light", RhymeKind::Perfect);
        assert!(!perfect.guessed);
        let words: Vec<&str> = perfect.list.iter().map(|r| r.word.as_str()).collect();
        assert!(words.contains(&"night") && words.contains(&"bright") && words.contains(&"tonight"), "{words:?}");
        assert!(!words.contains(&"light") && !words.contains(&"lights"), "same word or ending is no rhyme");
        assert!(!words.contains(&"flight") || words.contains(&"fight"));
        let near: Vec<String> = rhymes("time", RhymeKind::Near).list.into_iter().map(|r| r.word).collect();
        assert!(near.contains(&"line".to_string()) && near.contains(&"mine".to_string()), "{near:?}");
        assert!(!near.contains(&"lime".to_string()), "a perfect rhyme isn't a near one");
        let multi: Vec<String> = rhymes("ending", RhymeKind::Multi).list.into_iter().map(|r| r.word).collect();
        assert!(multi.contains(&"bending".to_string()) && multi.contains(&"mending".to_string()), "{multi:?}");
        assert!(rhymes("light", RhymeKind::Multi).list.is_empty() || rhymes("light", RhymeKind::Multi).list.iter().all(|r| r.syllables >= 2));
        assert!(rhymes("attention", RhymeKind::Multi).list.iter().any(|r| r.word == "mention"));
        // Sorted by syllables.
        assert!(perfect.list.windows(2).all(|w| w[0].syllables <= w[1].syllables));
    }

    #[test]
    fn a_word_not_in_the_dictionary_rhymes_by_its_ending() {
        let r = rhymes("flightish", RhymeKind::Perfect);
        assert!(r.guessed);
        let r = rhymes("gronight", RhymeKind::Perfect);
        assert!(r.guessed);
        assert!(r.list.iter().any(|x| x.word == "tonight"));
    }

    #[test]
    fn two_words_rhyming() {
        assert!(rhymes_with("cat", "hat"));
        assert!(rhymes_with("Peru", "shoe"));
        assert!(!rhymes_with("cat", "cat"));
        assert!(!rhymes_with("cat", "dog"));
        assert!(!rhymes_with("tight", "tite") || rhymes_with("tight", "tite"));
    }

    #[test]
    fn the_thesaurus_gives_senses_with_synonyms_and_antonyms() {
        let senses = senses("happy");
        assert!(!senses.is_empty());
        let all: Vec<&String> = senses.iter().flat_map(|s| &s.synonyms).collect();
        assert!(all.iter().any(|w| w.as_str() == "glad" || w.as_str() == "felicitous" || w.as_str() == "content"), "{all:?}");
        let opposite: Vec<&String> = senses.iter().flat_map(|s| &s.antonyms).collect();
        assert!(opposite.iter().any(|w| w.as_str() == "unhappy"), "{opposite:?}");
        assert!(senses.iter().all(|s| !s.synonyms.iter().any(|w| w == "happy")));
        // Forms of a word are found by the word they come from, and an irregular one too.
        assert!(!self::senses("running").is_empty());
        assert!(!self::senses("mice").is_empty());
        assert!(self::senses("zzzqqq").is_empty());
        let big = self::senses("big");
        assert!(big.iter().any(|s| s.antonyms.iter().any(|w| w == "small" || w == "little")), "satellites take their cluster's opposite");
        assert!(thesaurus().knows("walked"));
        assert!(!thesaurus().knows("qzxv"));
    }

    #[test]
    fn replacements_take_the_capitals_of_the_word() {
        assert_eq!(match_case("Happy", "glad"), "Glad");
        assert_eq!(match_case("HAPPY", "glad"), "GLAD");
        assert_eq!(match_case("happy", "glad"), "glad");
        assert_eq!(match_case("I", "me"), "Me");
    }
}
