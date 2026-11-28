//! The window's side of the Words tools (see `words`, `words_verse` and `words_prose`): the Words window with its
//! rhymes, thesaurus, metre and fixed-form checks, and the marks in the text (syllable counts down the right,
//! and underlines for repeated words, filler words and adverbs). The marks are off until asked for.

use super::*;
use crate::words::{self, RhymeKind};
use crate::words_prose as prose;
use crate::words_verse as verse;
use std::collections::HashMap;
use std::sync::{Mutex, RwLock};

/// Something the Words menu does.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    Rhymes,
    Thesaurus,
    Metre,
    Haiku,
    Limerick,
}

/// A switch in the Words menu.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Switch {
    /// Syllable counts beside each line of this file.
    SyllablesHere,
    /// Syllable counts in every text and Markdown file.
    SyllablesText,
    Repeats,
    Filler,
    Adverbs,
}

/// What's switched on, kept beside the settings.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Options {
    pub syllables_in_text: bool,
    pub repeats: bool,
    pub filler: bool,
    pub adverbs: bool,
    /// How many words apart a word may come round again before it's marked.
    pub distance: usize,
    /// Marks for verse as u and – rather than ˘ and ¯.
    pub plain_marks: bool,
}

impl Default for Options {
    fn default() -> Self {
        Options { syllables_in_text: false, repeats: false, filler: false, adverbs: false, distance: prose::DEFAULT_DISTANCE, plain_marks: false }
    }
}

impl Options {
    fn from_text(text: &str) -> Self {
        let mut o = Options::default();
        for (key, value) in text.lines().filter_map(|l| l.split_once('=')) {
            let flag = value.trim().parse::<bool>().ok();
            match key.trim() {
                "syllables_in_text" => o.syllables_in_text = flag.unwrap_or(o.syllables_in_text),
                "repeats" => o.repeats = flag.unwrap_or(o.repeats),
                "filler" => o.filler = flag.unwrap_or(o.filler),
                "adverbs" => o.adverbs = flag.unwrap_or(o.adverbs),
                "plain_marks" => o.plain_marks = flag.unwrap_or(o.plain_marks),
                "distance" => o.distance = value.trim().parse().ok().filter(|d| (2..=500).contains(d)).unwrap_or(o.distance),
                _ => {}
            }
        }
        o
    }

    fn to_text(self) -> String {
        format!("syllables_in_text={}\nrepeats={}\nfiller={}\nadverbs={}\ndistance={}\nplain_marks={}\n", self.syllables_in_text, self.repeats, self.filler, self.adverbs, self.distance, self.plain_marks)
    }

    fn any_marks(self) -> bool {
        self.repeats || self.filler || self.adverbs
    }
}

fn settings_file() -> Option<std::path::PathBuf> {
    crate::settings::data_folder().map(|dir| dir.join("words.txt"))
}

static OPTIONS: RwLock<Option<Options>> = RwLock::new(None);

/// The Words options, read from the settings folder the first time they're asked for (never in tests).
pub fn options() -> Options {
    if let Some(known) = OPTIONS.read().ok().and_then(|o| *o) {
        return known;
    }
    let read = if cfg!(test) { None } else { settings_file().and_then(|f| std::fs::read_to_string(f).ok()) };
    let options = read.map(|t| Options::from_text(&t)).unwrap_or_default();
    if let Ok(mut kept) = OPTIONS.write() {
        *kept = Some(options);
    }
    options
}

fn remember(options: Options) {
    if let Ok(mut kept) = OPTIONS.write() {
        *kept = Some(options);
    }
}

fn set_options(options: Options) {
    remember(options);
    if !cfg!(test)
        && let Some(file) = settings_file()
    {
        let _ = std::fs::create_dir_all(file.parent().expect("in a folder"));
        let _ = std::fs::write(file, options.to_text());
    }
}

/// Whether a switch is on, for a file's text.
pub fn switch_on(switch: Switch, text: &TextBuffer) -> bool {
    let o = options();
    match switch {
        Switch::SyllablesHere => text.syllable_counts.unwrap_or(o.syllables_in_text),
        Switch::SyllablesText => o.syllables_in_text,
        Switch::Repeats => o.repeats,
        Switch::Filler => o.filler,
        Switch::Adverbs => o.adverbs,
    }
}

/// Turns a switch over. The one for this file stays this file's own, whatever the default for text then is.
pub fn toggle(switch: Switch, text: &mut TextBuffer) {
    let mut o = options();
    match switch {
        Switch::SyllablesHere => text.syllable_counts = Some(!text.syllable_counts.unwrap_or(o.syllables_in_text)),
        Switch::SyllablesText => o.syllables_in_text = !o.syllables_in_text,
        Switch::Repeats => o.repeats = !o.repeats,
        Switch::Filler => o.filler = !o.filler,
        Switch::Adverbs => o.adverbs = !o.adverbs,
    }
    set_options(o);
}

// ---------------------------------------------------------------------------------------------------------------
// Marks in the text
// ---------------------------------------------------------------------------------------------------------------

/// What's marked on one line.
#[derive(Clone, Debug, Default)]
pub struct LineMarks {
    pub spans: Vec<prose::Mark>,
    /// Syllables in the line, and whether some of its words were guessed.
    pub syllables: Option<(usize, bool)>,
    /// A song's rhyme scheme: the line's letter, and whether it only rhymes nearly (see `lyrics_host`).
    pub rhyme: Option<(char, bool)>,
}

#[derive(Clone, Debug, Default)]
pub struct Marks {
    pub lines: HashMap<usize, LineMarks>,
}

/// The longest line whose words are looked at.
const MOST_WORDS_LINE: usize = 2000;

/// The marks for the lines `first..=last`, as they'd be drawn. Syllable counts go beside text and Markdown (or
/// any file the person turned them on for); the underlines only on prose, never code.
pub fn marks_for(b: &TextBuffer, is_code: bool, first: usize, last: usize) -> Marks {
    // A song's margin is its own: syllables and the rhyme scheme of each section.
    if b.song {
        return super::lyrics_host::marks(b, first, last);
    }
    let o = options();
    let count_syllables = b.syllable_counts.unwrap_or(o.syllables_in_text && !is_code);
    let prose_marks = o.any_marks() && !is_code;
    if (!count_syllables && !prose_marks) || b.line_count() == 0 {
        return Marks::default();
    }
    let count = b.line_count();
    let last = last.min(count - 1);
    let read = |i: usize| -> String {
        let line = b.line(i);
        if line.len() > MOST_WORDS_LINE { String::new() } else { b.format.readable_text(&line) }
    };
    let mut lines: HashMap<usize, LineMarks> = HashMap::new();
    if prose_marks {
        let wanted = prose::Wanted { repeats: o.repeats, filler: o.filler, adverbs: o.adverbs, distance: o.distance };
        for (line, spans) in prose::marks(&read, count, first, last, wanted) {
            lines.entry(line).or_default().spans = spans;
        }
    }
    if count_syllables {
        for line in first..=last {
            let counted = words::line_syllables(&read(line));
            if counted.syllables > 0 {
                lines.entry(line).or_default().syllables = Some((counted.syllables, !counted.guessed.is_empty()));
            }
        }
    }
    Marks { lines }
}

/// What a mark says, for the hover.
pub fn reason(mark: &prose::Mark) -> String {
    match mark.kind {
        prose::Kind::Repeat => {
            let apart = mark.apart;
            crate::i18n::tf!("This word is used again {apart} words away. Using it so soon can sound clumsy.")
        }
        prose::Kind::Filler => crate::i18n::tr("A filler word: it adds little. Try the sentence without it.").to_string(),
        prose::Kind::Adverb => crate::i18n::tr("An adverb. A stronger verb or noun can often do its work.").to_string(),
    }
}

/// The colour a kind of mark is drawn in.
pub fn colour(kind: prose::Kind) -> egui::Color32 {
    theme::tag_colour(match kind {
        prose::Kind::Repeat => 1,
        prose::Kind::Filler => 4,
        prose::Kind::Adverb => 5,
    })
}

// ---------------------------------------------------------------------------------------------------------------
// The window
// ---------------------------------------------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Tab {
    Rhymes,
    Thesaurus,
    Metre,
    Forms,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Form {
    Haiku,
    Limerick,
}

/// A word in the text that a click on a suggestion replaces.
#[derive(Clone, Debug)]
struct Anchor {
    doc_id: u64,
    line: usize,
    start: usize,
    end: usize,
    word: String,
}

struct Window {
    open: bool,
    tab: Tab,
    form: Form,
    anchor: Option<Anchor>,
    /// What's typed in the word box.
    query: String,
    rhyme_kind: RhymeKind,
    rhymes: Option<(String, RhymeKind, words::Rhymes)>,
    senses: Option<(String, Vec<words::Sense>)>,
    /// The lines of the passage for the metre and the forms.
    passage_lines: Vec<String>,
    passage: Option<verse::Passage>,
    note: String,
    /// Wait a frame before taking the keyboard: the word box has the cursor when the window opens.
    focus_word: bool,
}

impl Window {
    const fn new() -> Self {
        Window { open: false, tab: Tab::Rhymes, form: Form::Haiku, anchor: None, query: String::new(), rhyme_kind: RhymeKind::Perfect, rhymes: None, senses: None, passage_lines: Vec::new(), passage: None, note: String::new(), focus_word: false }
    }
}

static WINDOW: Mutex<Window> = Mutex::new(Window::new());

/// The word at (or touching) the cursor, or the selected one, and where it is.
fn word_at_cursor(text: &TextBuffer) -> Option<(usize, usize, usize, String)> {
    let (line, column) = text.caret_char_position();
    let whole = text.format.readable_text(&text.line(line));
    if text.has_selection() && text.caret_line == text.anchor_line {
        let (a, z) = (column.min(text.anchor_char_position().1), column.max(text.anchor_char_position().1));
        let picked: String = whole.chars().skip(a).take(z - a).collect();
        let found = words::words_in(&picked);
        if found.len() == 1 && found[0].0 == 0 && found[0].1 == picked.chars().count() {
            return Some((line, a, z, picked));
        }
    }
    words::words_in(&whole).into_iter().find(|(a, z, _)| column >= *a && column <= *z).map(|(a, z, w)| (line, a, z, w))
}

/// The lines to scan for metre: those the selection touches, or else the paragraph the cursor is in.
fn passage_at_cursor(text: &TextBuffer) -> Vec<String> {
    let read = |i: usize| text.format.readable_text(&text.line(i));
    let count = text.line_count();
    let (mut first, mut last) = (text.caret_line.min(text.anchor_line), text.caret_line.max(text.anchor_line));
    if !text.has_selection() {
        while first > 0 && !read(first - 1).trim().is_empty() {
            first -= 1;
        }
        while last + 1 < count && !read(last + 1).trim().is_empty() {
            last += 1;
        }
    }
    (first..=last.min(count.saturating_sub(1))).take(200).map(read).collect()
}

impl Window {
    fn look_up(&mut self) {
        let word = self.query.trim().to_string();
        self.rhymes = None;
        self.senses = None;
        self.note.clear();
        if word.is_empty() {
            return;
        }
        match self.tab {
            Tab::Rhymes => self.rhymes = Some((word.clone(), self.rhyme_kind, words::rhymes(&word, self.rhyme_kind))),
            Tab::Thesaurus => self.senses = Some((word.clone(), words::senses(&word))),
            _ => {}
        }
    }

    fn scan(&mut self) {
        let lines: Vec<&str> = self.passage_lines.iter().map(String::as_str).collect();
        self.passage = Some(verse::scan_passage(&lines));
    }
}

impl Limoniter {
    pub(super) fn words_command(&mut self, action: Action) {
        let index = self.active;
        let doc = &self.docs[index];
        if doc.hex_mode {
            return;
        }
        let found = word_at_cursor(&doc.text);
        let passage = passage_at_cursor(&doc.text);
        let id = doc.id;
        let Ok(mut w) = WINDOW.lock() else { return };
        w.open = true;
        w.tab = match action {
            Action::Rhymes => Tab::Rhymes,
            Action::Thesaurus => Tab::Thesaurus,
            Action::Metre => Tab::Metre,
            Action::Haiku | Action::Limerick => Tab::Forms,
        };
        match action {
            Action::Haiku => w.form = Form::Haiku,
            Action::Limerick => w.form = Form::Limerick,
            _ => {}
        }
        if matches!(action, Action::Rhymes | Action::Thesaurus) {
            w.anchor = found.clone().map(|(line, start, end, word)| Anchor { doc_id: id, line, start, end, word });
            w.query = found.map(|(_, _, _, word)| word).unwrap_or_default();
            w.look_up();
            w.focus_word = w.query.is_empty();
        } else {
            w.passage_lines = passage;
            w.scan();
        }
    }

    /// For pictures of the Words tools: the cursor in the first word of the text, or on its first four lines.
    pub(super) fn words_screenshot_step(&mut self, step: &str) {
        let text = &mut self.docs[self.active].text;
        match step {
            "words-rhymes" | "words-thesaurus" => {
                // The word the cursor is in is the one on the first line's second word, if there is one.
                let line = text.format.readable_text(&text.line(0));
                if let Some((a, z, _)) = words::words_in(&line).get(1) {
                    text.select_chars((0, *a), (0, *z));
                }
                self.words_command(if step == "words-rhymes" { Action::Rhymes } else { Action::Thesaurus });
            }
            "words-verse" => {
                let last = text.line_count().min(4).saturating_sub(1);
                text.select_chars((0, 0), (last, text.line(last).chars().count()));
                text.syllable_counts = Some(true);
                self.words_command(Action::Metre);
            }
            _ => {
                // Without keeping them: a picture mustn't change the settings of whoever takes it.
                remember(Options { repeats: true, filler: true, adverbs: true, ..options() });
                text.syllable_counts = Some(true);
            }
        }
    }

    /// Draws the Words window while it's open, and makes the change if a word in it is clicked.
    pub(super) fn words_window(&mut self, ctx: &egui::Context) {
        let Ok(mut w) = WINDOW.lock() else { return };
        if !w.open {
            return;
        }
        let mut open = true;
        let mut chosen: Option<String> = None;
        let plain = options().plain_marks;
        let mut plain_now = plain;
        let can_replace = w.anchor.as_ref().is_some_and(|a| self.docs.get(self.active).is_some_and(|d| d.id == a.doc_id && !d.text.read_only));
        let state = &mut *w;
        crate::fit::Dialog::new(crate::i18n::tr("Words")).open(&mut open).collapsible(false).resizable(true).default_size(egui::vec2(520.0, 520.0)).anchor(egui::Align2::RIGHT_TOP, [-40.0, 90.0]).show(ctx, |ui| {
            let before = state.tab;
            ui.horizontal(|ui| {
                for (tab, label) in [(Tab::Rhymes, "Rhymes"), (Tab::Thesaurus, "Thesaurus"), (Tab::Metre, "Metre"), (Tab::Forms, "Haiku and limerick")] {
                    ui.selectable_value(&mut state.tab, tab, crate::i18n::tr(label));
                }
            });
            ui.separator();
            if state.tab != before {
                match state.tab {
                    Tab::Rhymes | Tab::Thesaurus => state.look_up(),
                    Tab::Metre | Tab::Forms => {
                        if state.passage.is_none() {
                            state.scan();
                        }
                    }
                }
            }
            match state.tab {
                Tab::Rhymes | Tab::Thesaurus => {
                    ui.horizontal(|ui| {
                        ui.label(crate::i18n::tr("Word:"));
                        let edit = ui.add(egui::TextEdit::singleline(&mut state.query).desired_width(180.0));
                        if state.focus_word {
                            edit.request_focus();
                            state.focus_word = false;
                        }
                        let go = ui.button(crate::i18n::tr("Look up")).clicked();
                        if go || (edit.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter))) {
                            state.look_up();
                        }
                    });
                    if !state.note.is_empty() {
                        ui.label(egui::RichText::new(state.note.as_str()).color(theme::rainbow_mistake()));
                    }
                    if !can_replace {
                        ui.label(egui::RichText::new(crate::i18n::tr("The word in the text has gone, so clicking a word here won't change anything.")).color(theme::text_faint()));
                    }
                    if state.tab == Tab::Rhymes {
                        rhymes_page(ui, state, &mut chosen);
                    } else {
                        thesaurus_page(ui, state, &mut chosen);
                    }
                }
                Tab::Metre => {
                    ui.horizontal(|ui| {
                        if ui.button(crate::i18n::tr("Scan again")).on_hover_text(crate::i18n::tr("Read the lines of the selection (or the paragraph) again")).clicked() {
                            state.scan();
                        }
                        ui.checkbox(&mut plain_now, crate::i18n::tr("Marks as u and \u{2013}"));
                    });
                    metre_page(ui, state, plain_now);
                }
                Tab::Forms => {
                    ui.horizontal(|ui| {
                        ui.selectable_value(&mut state.form, Form::Haiku, crate::i18n::tr("Haiku"));
                        ui.selectable_value(&mut state.form, Form::Limerick, crate::i18n::tr("Limerick"));
                        if ui.button(crate::i18n::tr("Check again")).on_hover_text(crate::i18n::tr("Read the lines of the selection (or the paragraph) again")).clicked() {
                            state.scan();
                        }
                    });
                    forms_page(ui, state);
                }
            }
        });
        let (anchor, tab) = (w.anchor.clone(), w.tab);
        if !open {
            w.open = false;
        }
        drop(w);
        if plain_now != plain {
            set_options(Options { plain_marks: plain_now, ..options() });
        }
        if let (Some(word), Some(anchor)) = (chosen, anchor) {
            self.replace_anchored(&anchor, &word, tab);
        }
    }

    /// Puts a word from the window in place of the one it was looked up for, if that's still there.
    fn replace_anchored(&mut self, anchor: &Anchor, with: &str, tab: Tab) {
        let Some(index) = self.docs.iter().position(|d| d.id == anchor.doc_id) else { return };
        let text = &mut self.docs[index].text;
        if text.read_only || anchor.line >= text.line_count() {
            return;
        }
        let line = text.format.readable_text(&text.line(anchor.line));
        let there: String = line.chars().skip(anchor.start).take(anchor.end - anchor.start).collect();
        let Ok(mut w) = WINDOW.lock() else { return };
        if there != anchor.word {
            w.note = crate::i18n::tr("The word has moved or changed since it was looked up. Choose it again in the text.").to_string();
            w.anchor = None;
            return;
        }
        let new = words::match_case(&anchor.word, with);
        text.select_chars((anchor.line, anchor.start), (anchor.line, anchor.end));
        text.type_text(&new);
        let end = anchor.start + new.chars().count();
        w.anchor = Some(Anchor { end, word: new.clone(), ..anchor.clone() });
        w.query = new;
        w.note.clear();
        w.look_up();
        let _ = tab;
    }
}

/// A row of words, each a button; the one clicked.
fn word_buttons(ui: &mut Ui, list: &[String]) -> Option<String> {
    let mut clicked = None;
    ui.horizontal_wrapped(|ui| {
        for word in list {
            if ui.add(egui::Button::new(word.as_str()).frame(false)).on_hover_text(crate::i18n::tr("Put this in the text in place of the word")).clicked() {
                clicked = Some(word.clone());
            }
        }
    });
    clicked
}

fn rhymes_page(ui: &mut Ui, state: &mut Window, chosen: &mut Option<String>) {
    let before = state.rhyme_kind;
    ui.horizontal(|ui| {
        for (kind, label, tip) in [
            (RhymeKind::Perfect, "Perfect", "The same sounds from the last stressed vowel: light and night"),
            (RhymeKind::Near, "Near", "Alike, but not the same: time and line, or cat and cut"),
            (RhymeKind::Multi, "Several syllables", "Two syllables or more match: ending and bending"),
        ] {
            ui.selectable_value(&mut state.rhyme_kind, kind, crate::i18n::tr(label)).on_hover_text(crate::i18n::tr(tip));
        }
    });
    if state.rhyme_kind != before || state.rhymes.is_none() {
        state.look_up();
    }
    let Some((word, _, found)) = &state.rhymes else {
        ui.label(crate::i18n::tr("Type a word, or put the cursor in one in the text."));
        return;
    };
    ui.add_space(4.0);
    ui.label(egui::RichText::new(crate::i18n::tf!("Rhymes for \"{word}\"")).strong());
    if found.guessed {
        ui.label(egui::RichText::new(crate::i18n::tr("This word isn't in the dictionary, so its sound is guessed from how it's spelt.")).color(theme::text_faint()));
    }
    if found.list.is_empty() {
        ui.label(crate::i18n::tr("No rhymes found."));
        return;
    }
    egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
        let mut at = 0;
        while at < found.list.len() {
            let syllables = found.list[at].syllables;
            let group: Vec<String> = found.list[at..].iter().take_while(|r| r.syllables == syllables).map(|r| r.word.clone()).collect();
            at += group.len();
            ui.add_space(4.0);
            ui.label(egui::RichText::new(if syllables == 1 { crate::i18n::tr("1 syllable").to_string() } else { crate::i18n::tf!("{syllables} syllables") }).color(theme::text_faint()));
            if let Some(word) = word_buttons(ui, &group) {
                *chosen = Some(word);
            }
        }
    });
}

fn thesaurus_page(ui: &mut Ui, state: &mut Window, chosen: &mut Option<String>) {
    if state.senses.is_none() {
        state.look_up();
    }
    let Some((word, senses)) = &state.senses else {
        ui.label(crate::i18n::tr("Type a word, or put the cursor in one in the text."));
        return;
    };
    ui.add_space(4.0);
    ui.label(egui::RichText::new(crate::i18n::tf!("Words with a meaning like \"{word}\"")).strong());
    if senses.is_empty() {
        ui.label(crate::i18n::tr("Nothing found for this word."));
        return;
    }
    egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
        for (number, sense) in senses.iter().enumerate() {
            ui.add_space(6.0);
            ui.label(egui::RichText::new(format!("{}. {} \u{2013} {}", number + 1, crate::i18n::tr(sense.part.label()), sense.definition)).color(theme::text_dim()));
            if !sense.synonyms.is_empty()
                && let Some(w) = word_buttons(ui, &sense.synonyms)
            {
                *chosen = Some(w);
            }
            if !sense.antonyms.is_empty() {
                ui.label(egui::RichText::new(crate::i18n::tr("Opposites:")).color(theme::text_faint()));
                if let Some(w) = word_buttons(ui, &sense.antonyms) {
                    *chosen = Some(w);
                }
            }
        }
    });
}

fn metre_page(ui: &mut Ui, state: &Window, plain: bool) {
    let Some(passage) = &state.passage else { return };
    if passage.lines.is_empty() {
        ui.label(crate::i18n::tr("Select some lines of verse, or put the cursor in a verse."));
        return;
    }
    ui.add_space(4.0);
    let heading = match passage.metre {
        Some(metre) => {
            let name = crate::i18n::tr(metre.name());
            crate::i18n::tf!("Metre: {name}")
        }
        None => crate::i18n::tr("No one metre fits these lines well, so each is shown with the one it fits best.").to_string(),
    };
    ui.label(egui::RichText::new(heading).strong());
    if !passage.guessed.is_empty() {
        let list = passage.guessed.join(", ");
        ui.label(egui::RichText::new(crate::i18n::tf!("Guessed, as not in the dictionary: {list}")).color(theme::text_faint()));
    }
    let (light, beat) = if plain { ('u', '\u{2013}') } else { ('\u{02D8}', '\u{00AF}') };
    egui::ScrollArea::both().auto_shrink([false, false]).show(ui, |ui| {
        for line in &passage.lines {
            ui.add_space(6.0);
            let mut words_row = String::new();
            let mut marks_row = String::new();
            let mut off_words = Vec::new();
            let mut syllable = 0;
            let off: &[usize] = line.fit.as_ref().map_or(&[], |f| f.off.as_slice());
            for w in &line.words {
                let marks: Vec<String> = w
                    .kinds
                    .iter()
                    .map(|k| {
                        let here = if k.is_stressed() { beat } else { light };
                        if off.contains(&syllable) && !off_words.contains(&w.word) {
                            off_words.push(w.word.clone());
                        }
                        syllable += 1;
                        here.to_string()
                    })
                    .collect();
                let marks = marks.join(" ");
                let width = marks.chars().count().max(w.word.chars().count());
                words_row.push_str(&format!("{:<width$} ", w.word));
                marks_row.push_str(&format!("{marks:<width$} "));
            }
            ui.label(egui::RichText::new(words_row).monospace());
            ui.label(egui::RichText::new(marks_row).monospace().color(theme::accent()));
            let n = line.syllables;
            let mut said = vec![format!("{}.", if n == 1 { crate::i18n::tr("1 syllable").to_string() } else { crate::i18n::tf!("{n} syllables") })];
            if let Some(fit) = &line.fit {
                match fit.ending {
                    verse::Ending::Extra => said.push(crate::i18n::tr("It ends on an extra unstressed syllable (a feminine ending).").to_string()),
                    verse::Ending::Short => said.push(crate::i18n::tr("Its last unstressed syllable is missing (catalectic).").to_string()),
                    verse::Ending::Full => {}
                }
                if passage.metre.is_some() && fit.syllables != fit.expected && fit.ending == verse::Ending::Full {
                    let expected = fit.expected;
                    said.push(format!("{}.", crate::i18n::tf!("The metre has {expected}")));
                }
                if !off_words.is_empty() {
                    let list = off_words.join(", ");
                    said.push(format!("{}.", crate::i18n::tf!("Off the beat at: {list}")));
                }
            }
            let said = said.join(" ");
            ui.label(egui::RichText::new(said).color(theme::text_faint()));
        }
    });
}

fn forms_page(ui: &mut Ui, state: &Window) {
    if state.passage_lines.iter().all(|l| l.trim().is_empty()) {
        ui.label(crate::i18n::tr("Select the lines of the poem, or put the cursor in it."));
        return;
    }
    let lines: Vec<&str> = state.passage_lines.iter().map(String::as_str).collect();
    let (check, haiku) = match state.form {
        Form::Haiku => (verse::check_haiku(&lines), true),
        Form::Limerick => (verse::check_limerick(&lines), false),
    };
    ui.add_space(4.0);
    let used: Vec<&&str> = lines.iter().filter(|l| !l.trim().is_empty()).collect();
    egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
        for (line, count) in used.iter().zip(&check.counts) {
            ui.label(egui::RichText::new(format!("{count:>2}  {}", line.trim())).monospace());
        }
        ui.add_space(8.0);
        if check.is_ok() {
            ui.label(egui::RichText::new(if haiku { crate::i18n::tr("This is a haiku: 5, 7 and 5 syllables.") } else { crate::i18n::tr("This is a limerick: it rhymes AABBA and has the usual syllables and beats.") }).strong());
        } else {
            ui.label(egui::RichText::new(if haiku { crate::i18n::tr("This isn't quite a haiku:") } else { crate::i18n::tr("This isn't quite a limerick:") }).strong());
        }
        for remark in &check.remarks {
            ui.label(remark_text(remark, haiku));
        }
    });
}

/// What a remark says, in plain words.
pub fn remark_text(remark: &verse::Remark, haiku: bool) -> String {
    match remark {
        verse::Remark::Lines { found, wanted } => {
            if haiku { crate::i18n::tf!("A haiku has {wanted} lines, and this has {found}.") } else { crate::i18n::tf!("A limerick has {wanted} lines, and this has {found}.") }
        }
        verse::Remark::Syllables { line, found, low, high } => {
            if haiku {
                crate::i18n::tf!("Line {line} has {found} syllables; a haiku's has {low}.")
            } else {
                crate::i18n::tf!("Line {line} has {found} syllables; a limerick's line like it has {low} to {high}.")
            }
        }
        verse::Remark::Rhyme { first, second, first_word, second_word } => crate::i18n::tf!("Lines {first} and {second} should rhyme, but \"{first_word}\" and \"{second_word}\" don't."),
        verse::Remark::Beats { line, found, low, high } => crate::i18n::tf!("Line {line} has {found} beats; a limerick's line like it usually has {low} to {high}."),
        verse::Remark::Guessed(list) => {
            let list = list.join(", ");
            crate::i18n::tf!("Not in the dictionary, so their syllables are guessed: {list}")
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn options_are_kept_as_text_and_read_back() {
        let o = Options { syllables_in_text: true, repeats: true, filler: false, adverbs: true, distance: 33, plain_marks: true };
        assert_eq!(Options::from_text(&o.to_text()), o);
        assert_eq!(Options::from_text("distance=1\nrepeats=maybe\n"), Options::default());
        assert!(!Options::default().any_marks());
    }

    /// The options and the window are shared by every test, so those that change them take turns.
    static TURNS: Mutex<()> = Mutex::new(());

    fn app_with_text(lines: &[&str]) -> (Limoniter, egui::Context) {
        let mut app = Limoniter::build(String::new(), None, Some(PathBuf::from("never-written.png")));
        app.new_document();
        app.doc().text.set_content(lines.iter().map(|line| line.to_string()).collect());
        (app, egui::Context::default())
    }

    fn frame(app: &mut Limoniter, ctx: &egui::Context) -> egui::FullOutput {
        let input = egui::RawInput { screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1600.0, 900.0))), ..Default::default() };
        let mut output = ctx.run_ui(input, |ui| app.draw_frame(ui));
        output.textures_delta.clear();
        output
    }

    fn give(app: &mut Limoniter, ctx: &egui::Context, command: Command) {
        let input = egui::RawInput { screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1600.0, 900.0))), ..Default::default() };
        let mut output = ctx.run_ui(input, |ui| app.run(command, ui));
        output.textures_delta.clear();
    }

    fn reset() {
        remember(Options::default());
        if let Ok(mut w) = WINDOW.lock() {
            *w = Window::new();
        }
    }

    #[test]
    fn the_thesaurus_opens_on_the_word_at_the_cursor_and_a_click_replaces_it_keeping_its_capitals() {
        let _turn = TURNS.lock().unwrap_or_else(|e| e.into_inner());
        reset();
        let (mut app, ctx) = app_with_text(&["She was Happy today."]);
        app.doc().text.select_chars((0, 8), (0, 8));
        give(&mut app, &ctx, Command::Words(Action::Thesaurus));
        let (anchor, senses) = {
            let w = WINDOW.lock().unwrap();
            assert!(w.open && w.tab == Tab::Thesaurus);
            (w.anchor.clone().expect("a word at the cursor"), w.senses.clone().expect("looked up"))
        };
        assert_eq!((anchor.line, anchor.start, anchor.end, anchor.word.as_str()), (0, 8, 13, "Happy"));
        assert!(!senses.1.is_empty());
        frame(&mut app, &ctx);
        app.replace_anchored(&anchor, "glad", Tab::Thesaurus);
        assert_eq!(&*app.doc().text.line(0), "She was Glad today.");
        // The window follows the new word, so another can be tried straight away.
        let next = WINDOW.lock().unwrap().anchor.clone().unwrap();
        assert_eq!((next.start, next.end, next.word.as_str()), (8, 12, "Glad"));
        app.replace_anchored(&next, "content", Tab::Thesaurus);
        assert_eq!(&*app.doc().text.line(0), "She was Content today.");
        // If the text has changed under it, nothing is replaced, and it says so.
        app.doc().text.set_content(vec!["Something else entirely".into()]);
        let stale = WINDOW.lock().unwrap().anchor.clone().unwrap();
        app.replace_anchored(&stale, "glad", Tab::Thesaurus);
        assert_eq!(&*app.doc().text.line(0), "Something else entirely");
        assert!(!WINDOW.lock().unwrap().note.is_empty());
        reset();
    }

    #[test]
    fn rhymes_open_for_the_word_at_the_cursor_and_the_window_draws() {
        let _turn = TURNS.lock().unwrap_or_else(|e| e.into_inner());
        reset();
        let (mut app, ctx) = app_with_text(&["The moon is bright tonight"]);
        app.doc().text.select_chars((0, 15), (0, 15));
        give(&mut app, &ctx, Command::Words(Action::Rhymes));
        {
            let w = WINDOW.lock().unwrap();
            let (word, kind, found) = w.rhymes.as_ref().expect("looked up");
            assert_eq!((word.as_str(), *kind), ("bright", RhymeKind::Perfect));
            assert!(found.list.iter().any(|r| r.word == "night"));
        }
        frame(&mut app, &ctx);
        frame(&mut app, &ctx);
        // Replacing with a rhyme.
        let anchor = WINDOW.lock().unwrap().anchor.clone().unwrap();
        app.replace_anchored(&anchor, "light", Tab::Rhymes);
        assert_eq!(&*app.doc().text.line(0), "The moon is light tonight");
        reset();
    }

    #[test]
    fn the_metre_and_forms_tabs_read_the_selection_or_the_paragraph() {
        let _turn = TURNS.lock().unwrap_or_else(|e| e.into_inner());
        reset();
        let (mut app, ctx) = app_with_text(&["Not this line", "", "An old silent pond", "A frog jumps into the pond", "Splash! Silence again", "", "Nor this"]);
        app.doc().text.select_chars((3, 2), (3, 2));
        give(&mut app, &ctx, Command::Words(Action::Haiku));
        {
            let w = WINDOW.lock().unwrap();
            assert_eq!((w.tab, w.form), (Tab::Forms, Form::Haiku));
            assert_eq!(w.passage_lines, ["An old silent pond", "A frog jumps into the pond", "Splash! Silence again"]);
        }
        frame(&mut app, &ctx);
        app.doc().text.select_chars((2, 0), (3, 4));
        give(&mut app, &ctx, Command::Words(Action::Metre));
        let w = WINDOW.lock().unwrap();
        assert_eq!(w.tab, Tab::Metre);
        assert_eq!(w.passage_lines.len(), 2, "the lines the selection touches");
        assert_eq!(w.passage.as_ref().unwrap().lines.len(), 2);
        drop(w);
        frame(&mut app, &ctx);
        reset();
    }

    #[test]
    fn syllable_counts_are_off_until_asked_for_and_can_be_for_one_file_or_for_text() {
        let _turn = TURNS.lock().unwrap_or_else(|e| e.into_inner());
        reset();
        let (mut app, ctx) = app_with_text(&["An old silent pond", "", "A frog jumps into the pond"]);
        assert!(marks_for(&app.doc().text, false, 0, 2).lines.is_empty(), "off by default");
        give(&mut app, &ctx, Command::Toggle(menus::Flag::Words(Switch::SyllablesHere)));
        assert!(app.flag_on(menus::Flag::Words(Switch::SyllablesHere)));
        let marks = marks_for(&app.doc().text, false, 0, 2);
        assert_eq!(marks.lines[&0].syllables, Some((5, false)));
        assert_eq!(marks.lines[&2].syllables, Some((7, false)));
        assert!(!marks.lines.contains_key(&1), "a blank line has none");
        let before = frame(&mut app, &ctx).shapes.len();
        give(&mut app, &ctx, Command::Toggle(menus::Flag::Words(Switch::SyllablesHere)));
        assert!(marks_for(&app.doc().text, false, 0, 2).lines.is_empty());
        assert!(frame(&mut app, &ctx).shapes.len() < before, "the counts are drawn");
        // For every text file, but not for code.
        give(&mut app, &ctx, Command::Toggle(menus::Flag::Words(Switch::SyllablesText)));
        app.doc().text.syllable_counts = None;
        assert!(!marks_for(&app.doc().text, false, 0, 2).lines.is_empty());
        assert!(marks_for(&app.doc().text, true, 0, 2).lines.is_empty());
        reset();
    }

    #[test]
    fn repeats_filler_and_adverbs_are_marked_in_prose_only_and_each_can_be_switched() {
        let _turn = TURNS.lock().unwrap_or_else(|e| e.into_inner());
        reset();
        let (mut app, ctx) = app_with_text(&["It was a really quiet garden and the garden was quickly dark."]);
        assert!(marks_for(&app.doc().text, false, 0, 0).lines.is_empty());
        for switch in [Switch::Repeats, Switch::Filler, Switch::Adverbs] {
            give(&mut app, &ctx, Command::Toggle(menus::Flag::Words(switch)));
            assert!(app.flag_on(menus::Flag::Words(switch)));
        }
        let kinds: Vec<prose::Kind> = marks_for(&app.doc().text, false, 0, 0).lines[&0].spans.iter().map(|m| m.kind).collect();
        assert_eq!(kinds.iter().filter(|k| **k == prose::Kind::Repeat).count(), 2);
        assert!(kinds.contains(&prose::Kind::Filler) && kinds.contains(&prose::Kind::Adverb));
        assert!(marks_for(&app.doc().text, true, 0, 0).lines.is_empty(), "never in code");
        let marked = frame(&mut app, &ctx).shapes.len();
        give(&mut app, &ctx, Command::Toggle(menus::Flag::Words(Switch::Filler)));
        let kinds: Vec<prose::Kind> = marks_for(&app.doc().text, false, 0, 0).lines[&0].spans.iter().map(|m| m.kind).collect();
        assert!(!kinds.contains(&prose::Kind::Filler) && kinds.contains(&prose::Kind::Adverb));
        let _ = marked;
        let hover = reason(&marks_for(&app.doc().text, false, 0, 0).lines[&0].spans[0]);
        assert!(!hover.is_empty());
        reset();
    }

    #[test]
    fn remarks_say_what_is_wrong_in_plain_words() {
        let check = verse::check_haiku(&["An old silent pond", "A frog jumps into the cold pond", "Splash! Silence again"]);
        let said: Vec<String> = check.remarks.iter().map(|r| remark_text(r, true)).collect();
        assert_eq!(said, ["Line 2 has 8 syllables; a haiku's has 7."]);
        let check = verse::check_limerick(&["There was an old man from Peru", "Who dreamed he was eating his shoe", "He awoke in the night", "With a terrible fright", "And found that the weather was cold"]);
        assert!(check.remarks.iter().any(|r| remark_text(r, false).starts_with("Lines 1 and 5 should rhyme")));
    }
}
