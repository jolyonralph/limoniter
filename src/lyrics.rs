//! Song lyrics, for Suno and others: the section tags such as `[Verse 2: whispered]`, the style prompt kept in a
//! block at the top of the file, the lengths Suno allows, what is copied out, and the syllables and rhyme scheme
//! shown in the margin. No window here (see `lyrics_host`); the syllables and rhymes come from `words`.
//!
//! A song file looks like this:
//!
//! ```text
//! ---
//! style: warm indie folk, female vocals, acoustic guitar
//! ---
//! [Verse 1]
//! The first line
//! ```
//!
//! The block between the two `---` lines is Markdown's own front matter, so the style stays in the file when it is
//! copied, mailed or opened elsewhere. It is never copied out as lyrics.

use crate::words;
use std::collections::HashMap;
use std::sync::Mutex;

/// The name of the colouring (see `data/syntaxes/song-lyrics.sublime-syntax`).
pub const LANGUAGE: &str = "Song Lyrics";

/// The tags offered as `[` is typed: the ones Suno's own guidance uses for a song's parts, and the ones for the
/// voice or the playing. Numbers and a note after a colon (`[Verse 2: whispered]`) are the writer's own.
pub const TAGS: &[&str] = &[
    "Intro",
    "Verse",
    "Verse 1",
    "Verse 2",
    "Verse 3",
    "Pre-Chorus",
    "Chorus",
    "Post-Chorus",
    "Final Chorus",
    "Hook",
    "Refrain",
    "Bridge",
    "Interlude",
    "Instrumental",
    "Instrumental Break",
    "Break",
    "Breakdown",
    "Build",
    "Drop",
    "Solo",
    "Guitar Solo",
    "Rap",
    "Spoken Word",
    "Outro",
    "Fade Out",
    "End",
    "Female Vocal",
    "Male Vocal",
    "Whispered",
    "Harmonies",
    "Choir",
    "Ad-lib",
    "Silence",
];

/// What Suno allows, as it was last known: 5,000 characters of lyrics and 1,000 of style (the newer models; the
/// older ones took only 3,000 and 200). They are settings, as the numbers change with the models.
pub const DEFAULT_LYRICS_LIMIT: usize = 5000;
pub const DEFAULT_STYLE_LIMIT: usize = 1000;

// ---------------------------------------------------------------------------------------------------------------
// Tags and sections
// ---------------------------------------------------------------------------------------------------------------

/// The words inside the brackets, if the whole line is one tag: `[Verse 2: whispered]` gives "Verse 2: whispered".
pub fn tag_of(line: &str) -> Option<&str> {
    let line = line.trim();
    let inner = line.strip_prefix('[')?.strip_suffix(']')?;
    (!inner.trim().is_empty() && !inner.contains(['[', ']'])).then_some(inner.trim())
}

/// One part of the song, from its tag to the line before the next.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Section {
    /// The tag's line, or the first line of lyrics before any tag.
    pub line: usize,
    /// What's in the brackets; empty for lyrics before the first tag.
    pub title: String,
    /// The last line that has anything on it.
    pub last: usize,
    /// Each line that is sung: not blank, not a tag.
    pub sung: Vec<usize>,
}

/// Where the front matter ends (the line of its closing `---`), if the file starts with some.
pub fn front_matter_end<S: AsRef<str>>(lines: &[S]) -> Option<usize> {
    if lines.first()?.as_ref().trim() != "---" {
        return None;
    }
    (1..lines.len().min(60)).find(|&i| lines[i].as_ref().trim() == "---")
}

/// The sections of a song, in order. The front matter isn't part of any.
pub fn sections<S: AsRef<str>>(lines: &[S]) -> Vec<Section> {
    let first = front_matter_end(lines).map_or(0, |end| end + 1);
    let mut out: Vec<Section> = Vec::new();
    for (i, line) in lines.iter().enumerate().skip(first) {
        let line = line.as_ref();
        if let Some(title) = tag_of(line) {
            out.push(Section { line: i, title: title.to_string(), last: i, sung: Vec::new() });
        } else if !line.trim().is_empty() {
            if out.is_empty() {
                out.push(Section { line: i, title: String::new(), last: i, sung: Vec::new() });
            }
            if let Some(section) = out.last_mut() {
                section.sung.push(i);
                section.last = i;
            }
        }
    }
    out
}

/// What a fold of the tag's line hides: down to the last line with anything on it before the next tag, looking at
/// most `most` lines on. `get` gives a line by its number, of `count` lines.
pub fn section_fold_end(count: usize, line: usize, most: usize, get: impl Fn(usize) -> std::sync::Arc<str>) -> Option<usize> {
    tag_of(&get(line))?;
    let mut last = None;
    for i in line + 1..count.min(line + 1 + most) {
        let next = get(i);
        if tag_of(&next).is_some() {
            break;
        }
        if !next.trim().is_empty() {
            last = Some(i);
        }
    }
    last
}

/// The tags to offer for what's been typed after a `[` just before the caret: where the name starts, and the
/// tags that start with it, written as they'd be put in (with the `]` unless one follows already).
pub fn tag_completions(line: &str, col: usize) -> Option<(usize, Vec<String>)> {
    let before: Vec<char> = line.chars().take(col).collect();
    let open = before.iter().rposition(|&c| c == '[')?;
    if before[open..].contains(&']') {
        return None;
    }
    let typed: String = before[open + 1..].iter().collect();
    if typed.contains(':') {
        return None;
    }
    let closed = line.chars().nth(col) == Some(']');
    let wanted = typed.to_lowercase();
    let items: Vec<String> = TAGS
        .iter()
        .filter(|tag| tag.to_lowercase().starts_with(&wanted) && tag.to_lowercase() != wanted)
        .map(|tag| if closed { (*tag).to_string() } else { format!("{tag}]") })
        .collect();
    (!items.is_empty()).then_some((open + 1, items))
}

// ---------------------------------------------------------------------------------------------------------------
// The style prompt
// ---------------------------------------------------------------------------------------------------------------

/// The song's style prompt, from the `style:` line of the front matter (a continued, indented line is part of it).
pub fn style_of<S: AsRef<str>>(lines: &[S]) -> String {
    let Some(end) = front_matter_end(lines) else {
        return String::new();
    };
    let mut style = String::new();
    let mut inside = false;
    for line in &lines[1..end] {
        let line = line.as_ref();
        if let Some(rest) = line.strip_prefix("style:") {
            inside = true;
            style.push_str(rest.trim());
        } else if inside && line.starts_with([' ', '\t']) {
            style.push(' ');
            style.push_str(line.trim());
        } else {
            inside = false;
        }
    }
    let style = style.trim();
    let quoted = style.len() >= 2 && ((style.starts_with('"') && style.ends_with('"')) || (style.starts_with('\'') && style.ends_with('\'')));
    if quoted { style[1..style.len() - 1].to_string() } else { style.to_string() }
}

/// The lines to put in place of the front matter so that the style is `style` and any other keys stay: the first
/// and last line replaced (the front matter's, or just the first line if there is none) and the rows to put there.
pub fn with_style<S: AsRef<str>>(lines: &[S], style: &str) -> (usize, usize, Vec<String>) {
    let flat = style.split_whitespace().collect::<Vec<_>>().join(" ");
    let mut rows = vec!["---".to_string()];
    let end = front_matter_end(lines);
    if let Some(end) = end {
        let mut inside = false;
        for line in &lines[1..end] {
            let line = line.as_ref();
            if line.starts_with("style:") {
                inside = true;
            } else if !(inside && line.starts_with([' ', '\t'])) {
                inside = false;
                rows.push(line.to_string());
            }
        }
    }
    rows.insert(1, format!("style: {flat}").trim_end().to_string());
    rows.push("---".to_string());
    match end {
        Some(end) => (0, end, rows),
        None => {
            // No front matter yet: the new block goes in front of the first line, which is put back after it.
            rows.push(lines.first().map_or(String::new(), |l| l.as_ref().to_string()));
            (0, 0, rows)
        }
    }
}

// ---------------------------------------------------------------------------------------------------------------
// Copying out
// ---------------------------------------------------------------------------------------------------------------

fn body<S: AsRef<str>>(lines: &[S]) -> impl Iterator<Item = &str> {
    let first = front_matter_end(lines).map_or(0, |end| end + 1);
    lines[first.min(lines.len())..].iter().map(AsRef::as_ref)
}

/// The lyrics as Suno's lyrics box wants them: the tags kept, the style block left out, no spaces at the ends of
/// lines, no blank lines at the top or bottom, and no run of blank lines (Suno reads one as a gap).
pub fn lyrics_for_suno<S: AsRef<str>>(lines: &[S]) -> String {
    let mut out: Vec<String> = Vec::new();
    for line in body(lines) {
        let line = line.trim_end();
        if line.trim().is_empty() && out.last().is_none_or(|last| last.is_empty()) {
            continue;
        }
        out.push(line.to_string());
    }
    while out.last().is_some_and(String::is_empty) {
        out.pop();
    }
    out.join("\n")
}

/// The words alone, for a place that takes no tags: the same, with every line that is only a tag taken out.
pub fn lyrics_plain<S: AsRef<str>>(lines: &[S]) -> String {
    let kept: Vec<&str> = body(lines).filter(|line| tag_of(line).is_none()).collect();
    lyrics_for_suno(&kept)
}

/// The style as Suno's style box wants it: one line, the words spaced singly.
pub fn style_for_suno<S: AsRef<str>>(lines: &[S]) -> String {
    style_of(lines).split_whitespace().collect::<Vec<_>>().join(" ")
}

/// How many characters each box would hold, counted as they would be copied out (a line break is one).
pub fn lengths<S: AsRef<str>>(lines: &[S]) -> (usize, usize) {
    (lyrics_for_suno(lines).chars().count(), style_for_suno(lines).chars().count())
}

// ---------------------------------------------------------------------------------------------------------------
// The margin: syllables and rhymes
// ---------------------------------------------------------------------------------------------------------------

/// What's shown beside a sung line.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Margin {
    pub syllables: usize,
    /// Whether some word's syllables were guessed.
    pub guessed: bool,
    /// The line's place in its section's rhyme scheme, and whether it only rhymes nearly.
    pub rhyme: Option<(char, bool)>,
}

/// The last word of a line, outside any brackets, for rhyming.
fn end_word(line: &str) -> Option<String> {
    let mut plain = String::new();
    let mut depth = 0u32;
    for c in line.chars() {
        match c {
            '[' | '(' => depth += 1,
            ']' | ')' => depth = depth.saturating_sub(1),
            _ if depth == 0 => plain.push(c),
            _ => {}
        }
    }
    // A line that is only backing vocals ("(ooh)") rhymes by what's in the brackets.
    let from = if words::words_in(&plain).is_empty() { line.to_string() } else { plain };
    words::words_in(&from).pop().map(|(_, _, word)| word)
}

/// The letters of a rhyme scheme for the end words of a section's lines: a line takes the letter of the earliest
/// line it rhymes with (perfectly, or nearly, which is marked), and a new letter if it rhymes with none.
pub fn scheme(end_words: &[Option<String>]) -> Vec<Option<(char, bool)>> {
    let mut letters: Vec<Option<(char, bool)>> = Vec::new();
    let mut next = 0u8;
    for (i, word) in end_words.iter().enumerate() {
        let Some(word) = word else {
            letters.push(None);
            continue;
        };
        let mut found: Option<(char, bool)> = None;
        for earlier in 0..i {
            let (Some(other), Some((letter, _))) = (&end_words[earlier], letters[earlier]) else { continue };
            let degree = if other.eq_ignore_ascii_case(word) { Some(true) } else { words::rhyme_degree(other, word) };
            match degree {
                Some(true) => {
                    found = Some((letter, false));
                    break;
                }
                Some(false) if found.is_none() => found = Some((letter, true)),
                _ => {}
            }
        }
        letters.push(Some(found.unwrap_or_else(|| {
            let letter = (b'A' + next % 26) as char;
            next += 1;
            (letter, false)
        })));
    }
    letters
}

/// A section's scheme, remembered by its end words: working it out reads the dictionary, and the margin asks for
/// it on every frame.
fn scheme_of_section(end_words: &[Option<String>]) -> Vec<Option<(char, bool)>> {
    static KEPT: Mutex<Option<HashMap<Vec<Option<String>>, Vec<Option<(char, bool)>>>>> = Mutex::new(None);
    if let Ok(kept) = KEPT.lock()
        && let Some(found) = kept.as_ref().and_then(|k| k.get(end_words))
    {
        return found.clone();
    }
    let made = scheme(end_words);
    if let Ok(mut kept) = KEPT.lock() {
        let map = kept.get_or_insert_with(HashMap::new);
        if map.len() > 400 {
            map.clear();
        }
        map.insert(end_words.to_vec(), made.clone());
    }
    made
}

/// What the margin shows for each sung line of the sections that touch `first..=last`.
pub fn margins<S: AsRef<str>>(lines: &[S], first: usize, last: usize) -> HashMap<usize, Margin> {
    let mut out = HashMap::new();
    for section in sections(lines) {
        if section.last < first || section.line > last {
            continue;
        }
        let ends: Vec<Option<String>> = section.sung.iter().map(|&i| end_word(lines[i].as_ref())).collect();
        let letters = scheme_of_section(&ends);
        for (n, &i) in section.sung.iter().enumerate() {
            if i < first || i > last {
                continue;
            }
            let counted = words::line_syllables(&without_notes(lines[i].as_ref()));
            out.insert(i, Margin { syllables: counted.syllables, guessed: !counted.guessed.is_empty(), rhyme: letters[n] });
        }
    }
    out
}

/// A line without its tags inside it, which aren't sung (`[Whispered] Hello` is two syllables).
fn without_notes(line: &str) -> String {
    let mut out = String::new();
    let mut depth = 0u32;
    for c in line.chars() {
        match c {
            '[' => depth += 1,
            ']' => depth = depth.saturating_sub(1),
            _ if depth == 0 => out.push(c),
            _ => {}
        }
    }
    out
}

/// What a song starts with.
pub const STARTER: &str = "---\nstyle: warm indie folk, female vocals, acoustic guitar, hopeful, mid-tempo\n---\n[Intro]\n\n[Verse 1]\nWrite the first line of the verse here\nAnother line that rhymes with it\n\n[Chorus]\nThe line everyone will sing\nThe line that brings it home\n\n[Verse 2: softly]\n\n[Bridge]\n\n[Outro]\n";

#[cfg(test)]
mod tests {
    use super::*;

/// The same scheme as the letters alone, such as "ABAB", with a ~ after a letter that rhymes only nearly.
fn scheme_text(letters: &[Option<(char, bool)>]) -> String {
    letters.iter().flatten().map(|(letter, near)| if *near { format!("{letter}~") } else { letter.to_string() }).collect()
}


    fn song() -> Vec<&'static str> {
        vec!["---", "title: Night", "style: warm folk,  female vocals", "---", "[Verse 1]", "The light is in the night  ", "I hold you tight", "", "", "[Chorus]", "[Whispered] Hello again", "(ooh) oh", "", "[Outro: fading]"]
    }

    #[test]
    fn tags_are_whole_lines_in_brackets_with_a_name_a_number_and_a_note() {
        assert_eq!(tag_of("  [Verse 2: whispered] "), Some("Verse 2: whispered"));
        assert_eq!(tag_of("[Hello] there"), None);
        assert_eq!(tag_of("[]"), None);
        assert!(TAGS.contains(&"Pre-Chorus") && TAGS.contains(&"Instrumental") && TAGS.contains(&"End"));
    }

    #[test]
    fn sections_hold_the_sung_lines_and_leave_out_the_front_matter() {
        let s = sections(&song());
        assert_eq!(s.iter().map(|s| s.title.as_str()).collect::<Vec<_>>(), ["Verse 1", "Chorus", "Outro: fading"]);
        assert_eq!(s[0].sung, [5, 6]);
        assert_eq!(s[1].sung, [10, 11]);
        assert!(s[2].sung.is_empty());
        assert_eq!(s[0].last, 6);
        assert_eq!(front_matter_end(&song()), Some(3));
    }

    #[test]
    fn a_section_folds_to_its_last_line_with_something_on_it() {
        let lines = song();
        let fold = |line| section_fold_end(lines.len(), line, 1000, |i| std::sync::Arc::from(lines[i]));
        assert_eq!(fold(4), Some(6), "the blank lines after stay out");
        assert_eq!(fold(9), Some(11));
        assert_eq!(fold(13), None, "nothing under it");
        assert_eq!(fold(5), None, "not a tag");
    }

    #[test]
    fn tags_are_offered_after_a_bracket() {
        let (start, items) = tag_completions("[Chor", 5).unwrap();
        assert_eq!(start, 1);
        assert_eq!(items, ["Chorus]"]);
        let (_, many) = tag_completions("[", 1).unwrap();
        assert!(many.len() > 20 && many.contains(&"Verse 1]".to_string()));
        let (_, closed) = tag_completions("[Pre]", 4).unwrap();
        assert_eq!(closed, ["Pre-Chorus"], "a bracket already after the cursor isn't put in twice");
        assert!(tag_completions("[Verse 2: whi", 13).is_none(), "the note is the writer's own");
        assert!(tag_completions("[Verse] la", 10).is_none());
        assert!(tag_completions("no bracket", 4).is_none());
    }

    #[test]
    fn the_style_is_read_from_the_front_matter_and_written_back_keeping_other_keys() {
        let lines = song();
        assert_eq!(style_of(&lines), "warm folk,  female vocals");
        assert_eq!(style_for_suno(&lines), "warm folk, female vocals");
        let (top, bottom, rows) = with_style(&lines, "dark synth\nwave");
        assert_eq!((top, bottom), (0, 3));
        assert_eq!(rows, ["---", "style: dark synth wave", "title: Night", "---"]);
        // And read back from what was written.
        let mut changed: Vec<String> = lines.iter().map(|s| s.to_string()).collect();
        changed.splice(top..=bottom, rows);
        assert_eq!(style_of(&changed), "dark synth wave");
        assert_eq!(changed[2], "title: Night");
        assert_eq!(changed.len(), lines.len(), "nothing else moved");
        // A song with no front matter gets some, and its first line is kept.
        let plain = ["[Verse]", "la"];
        let (top, bottom, rows) = with_style(&plain, "jazz");
        assert_eq!((top, bottom), (0, 0));
        assert_eq!(rows, ["---", "style: jazz", "---", "[Verse]"]);
        assert_eq!(style_of(&["---", "style: \"quoted, style\"", "---"]), "quoted, style");
        assert_eq!(style_of(&["---", "style: first", "  second", "---"]), "first second");
        assert_eq!(style_of(&["[Verse]"]), "");
    }

    #[test]
    fn lyrics_are_copied_out_with_their_tags_and_without_the_style_or_stray_spaces() {
        let lines = song();
        assert_eq!(lyrics_for_suno(&lines), "[Verse 1]\nThe light is in the night\nI hold you tight\n\n[Chorus]\n[Whispered] Hello again\n(ooh) oh\n\n[Outro: fading]");
        assert_eq!(lyrics_plain(&lines), "The light is in the night\nI hold you tight\n\n[Whispered] Hello again\n(ooh) oh");
        assert_eq!(lyrics_for_suno(&["", "[Verse]", "la", "", ""]), "[Verse]\nla");
    }

    #[test]
    fn lengths_count_what_would_be_copied() {
        let lines = song();
        let (lyrics, style) = lengths(&lines);
        assert_eq!(lyrics, lyrics_for_suno(&lines).chars().count());
        assert_eq!(style, "warm folk, female vocals".chars().count());
        assert_eq!(lengths(&["é"]).0, 1, "characters, not bytes");
    }

    #[test]
    fn a_known_verse_has_its_syllables_and_its_rhyme_scheme() {
        let verse = ["[Verse]", "The light is in the night", "I hold you close and tight", "We walk along the shore", "Then wait for something more"];
        let m = margins(&verse, 0, 4);
        assert_eq!(m[&1].syllables, 6);
        assert_eq!(m[&2].syllables, 6);
        assert_eq!(m[&3].syllables, 6);
        assert_eq!(m[&1].rhyme, Some(('A', false)));
        assert_eq!(m[&2].rhyme, Some(('A', false)));
        assert_eq!(m[&3].rhyme, Some(('B', false)));
        assert_eq!(m[&4].rhyme, Some(('B', false)));
        assert!(!m.contains_key(&0), "a tag isn't sung");
        let ends: Vec<Option<String>> = ["day", "night", "way", "light"].iter().map(|w| Some(w.to_string())).collect();
        assert_eq!(scheme_text(&scheme(&ends)), "ABAB");
    }

    #[test]
    fn near_rhymes_are_marked_as_near() {
        let ends: Vec<Option<String>> = ["time", "line", "sea"].iter().map(|w| Some(w.to_string())).collect();
        let s = scheme(&ends);
        assert_eq!(s[0], Some(('A', false)));
        assert_eq!(s[1], Some(('A', true)), "time and line");
        assert_eq!(s[2], Some(('B', false)));
        assert_eq!(scheme_text(&s), "AA~B");
        // A repeated word is the same sound again.
        let same: Vec<Option<String>> = ["home", "Home"].iter().map(|w| Some(w.to_string())).collect();
        assert_eq!(scheme(&same)[1], Some(('A', false)));
    }

    #[test]
    fn tags_inside_a_line_are_not_counted_or_rhymed() {
        let m = margins(&["[Chorus]", "[Whispered] Hello again"], 0, 1);
        assert_eq!(m[&1].syllables, 4);
        assert_eq!(end_word("(ooh) oh"), Some("oh".to_string()));
        assert_eq!(end_word("see you (oh yeah)"), Some("you".to_string()));
        assert_eq!(end_word("(ooh)"), Some("ooh".to_string()));
    }
}
