//! The window's side of song lyrics (see `lyrics`): the style box above the text, the lengths Suno allows shown as
//! you write, the copy-outs on Tools, Song lyrics, and the syllables and rhyme scheme in the margin.

use super::*;
use crate::lyrics;
use std::sync::{Mutex, RwLock};

/// Something Tools, Song lyrics does.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    /// The lyrics with their tags, as Suno's lyrics box wants them.
    CopyLyrics,
    /// The words alone, with no tags.
    CopyPlain,
    /// The style prompt, for Suno's style box.
    CopyStyle,
}

/// The most characters each of Suno's boxes takes. They change with its models, so they are settings, kept
/// beside the others.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Limits {
    pub lyrics: usize,
    pub style: usize,
}

impl Default for Limits {
    fn default() -> Self {
        Limits { lyrics: lyrics::DEFAULT_LYRICS_LIMIT, style: lyrics::DEFAULT_STYLE_LIMIT }
    }
}

impl Limits {
    fn from_text(text: &str) -> Self {
        let mut o = Limits::default();
        for (key, value) in text.lines().filter_map(|l| l.split_once('=')) {
            let number = value.trim().parse::<usize>().ok().filter(|n| (1..=1_000_000).contains(n));
            match key.trim() {
                "lyrics_limit" => o.lyrics = number.unwrap_or(o.lyrics),
                "style_limit" => o.style = number.unwrap_or(o.style),
                _ => {}
            }
        }
        o
    }

    fn to_text(self) -> String {
        format!("lyrics_limit={}\nstyle_limit={}\n", self.lyrics, self.style)
    }
}

fn settings_file() -> Option<std::path::PathBuf> {
    crate::settings::data_folder().map(|dir| dir.join("lyrics.txt"))
}

static LIMITS: RwLock<Option<Limits>> = RwLock::new(None);

/// Suno's limits, read from the settings folder the first time they're asked for (never in tests).
pub fn limits() -> Limits {
    if let Some(known) = LIMITS.read().ok().and_then(|o| *o) {
        return known;
    }
    let read = if cfg!(test) { None } else { settings_file().and_then(|f| std::fs::read_to_string(f).ok()) };
    let limits = read.map(|t| Limits::from_text(&t)).unwrap_or_default();
    if let Ok(mut kept) = LIMITS.write() {
        *kept = Some(limits);
    }
    limits
}

fn set_limits(limits: Limits) {
    if let Ok(mut kept) = LIMITS.write() {
        *kept = Some(limits);
    }
    if !cfg!(test)
        && let Some(file) = settings_file()
    {
        let _ = std::fs::create_dir_all(file.parent().expect("in a folder"));
        let _ = std::fs::write(file, limits.to_text());
    }
}

/// The style being typed in the box (and the file it's for), until it's put in the file when the box is left, so
/// that typing in it isn't a step of undo for every letter.
static DRAFT: Mutex<Option<(u64, String)>> = Mutex::new(None);

/// A song's lines as they read, for the copy-outs and the margin.
fn readable_lines(b: &TextBuffer) -> Vec<String> {
    b.all_lines().iter().map(|line| b.format.readable_text(line)).collect()
}

/// The most lines of a song that are read on every frame; a longer file is no song.
const MOST_LINES: usize = 20_000;

/// What is drawn in the margin of the lines `first..=last` of a song: the syllables and the rhyme scheme.
pub fn marks(b: &TextBuffer, first: usize, last: usize) -> super::words_host::Marks {
    let mut lines = std::collections::HashMap::new();
    if b.line_count() == 0 || b.line_count() > MOST_LINES || b.is_disk_backed() {
        return super::words_host::Marks { lines };
    }
    let all = readable_lines(b);
    for (line, m) in lyrics::margins(&all, first, last) {
        let entry: &mut super::words_host::LineMarks = lines.entry(line).or_default();
        entry.syllables = (m.syllables > 0).then_some((m.syllables, m.guessed));
        entry.rhyme = m.rhyme;
    }
    super::words_host::Marks { lines }
}

impl Limoniter {
    /// Tells each document whether it's a song, so that its sections fold, its tags are offered and its margin is
    /// drawn.
    pub(super) fn lyrics_tick(&mut self) {
        for doc in &mut self.docs {
            let song = !doc.hex_mode && !doc.placeholder && doc.language_name() == lyrics::LANGUAGE;
            if doc.text.song != song {
                doc.text.song = song;
                // Folds of another sort stay where they were; those of a song are by sections.
                doc.text.expand_all();
            }
        }
    }

    /// Puts the style being typed into the file, if there is one for the front document that differs.
    pub(super) fn lyrics_commit_style(&mut self) {
        let Some((id, draft)) = DRAFT.lock().ok().and_then(|mut d| d.take()) else { return };
        let Some(doc) = self.docs.iter_mut().find(|d| d.id == id) else { return };
        set_style(&mut doc.text, &draft);
    }

    /// For a picture of a song: a tag begun at the end, with the tags offered.
    pub(super) fn lyrics_screenshot_step(&mut self) {
        self.lyrics_tick();
        let doc = &mut self.docs[self.active];
        let last = doc.text.line_count().saturating_sub(1);
        doc.text.move_caret(last, 0, false);
        doc.text.type_text("[Ch");
        crate::views::offer_completions(&doc.text, &mut doc.text_state, false);
    }

    /// Tools, Song lyrics.
    pub(super) fn lyrics_command(&mut self, action: Action, ctx: &egui::Context) {
        self.lyrics_commit_style();
        let doc = &mut self.docs[self.active];
        if doc.hex_mode {
            return;
        }
        let lines = readable_lines(&doc.text);
        let limit = limits();
        let (text, over) = match action {
            Action::CopyLyrics => (lyrics::lyrics_for_suno(&lines), limit.lyrics),
            Action::CopyPlain => (lyrics::lyrics_plain(&lines), limit.lyrics),
            Action::CopyStyle => (lyrics::style_for_suno(&lines), limit.style),
        };
        let count = text.chars().count();
        let extra = count.saturating_sub(over);
        doc.text.notice = Some(match (action, extra > 0) {
            (Action::CopyLyrics, false) => crate::i18n::tf!("Copied the lyrics ({count} characters)."),
            (Action::CopyLyrics, true) => crate::i18n::tf!("Copied the lyrics ({count} characters), which is {extra} more than Suno's lyrics box takes."),
            (Action::CopyPlain, false) => crate::i18n::tf!("Copied the words without their tags ({count} characters)."),
            (Action::CopyPlain, true) => crate::i18n::tf!("Copied the words without their tags ({count} characters), which is {extra} more than Suno's lyrics box takes."),
            (Action::CopyStyle, false) => crate::i18n::tf!("Copied the style ({count} characters)."),
            (Action::CopyStyle, true) => crate::i18n::tf!("Copied the style ({count} characters), which is {extra} more than Suno's style box takes."),
        });
        ctx.copy_text(text);
    }

    /// The style box and the lengths, above the text of a song.
    pub(super) fn lyrics_strip(&mut self, ui: &mut Ui, index: usize) {
        let doc = &self.docs[index];
        if !doc.text.song || doc.text.line_count() > MOST_LINES || doc.text.is_disk_backed() {
            return;
        }
        let id = doc.id;
        let lines = readable_lines(&doc.text);
        let in_file = lyrics::style_of(&lines);
        let mut draft = DRAFT.lock().ok().and_then(|d| d.as_ref().filter(|(of, _)| *of == id).map(|(_, text)| text.clone())).unwrap_or_else(|| in_file.clone());
        let limit = limits();
        let (lyrics_length, _) = lyrics::lengths(&lines);
        let style_length = draft.split_whitespace().collect::<Vec<_>>().join(" ").chars().count();
        let (mut copy, mut new_limits) = (None, limit);
        let (mut edited, mut typing) = (None, false);
        egui::Frame::new().fill(theme::editor_back()).inner_margin(egui::Margin { left: 8, right: 8, top: 3, bottom: 3 }).show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label(RichText::new(crate::i18n::tr("Style")).size(12.5).color(theme::text_dim()));
                let edit = egui::TextEdit::multiline(&mut draft)
                    .id_salt(("song-style", id))
                    .desired_rows(2)
                    .desired_width(f32::INFINITY)
                    .hint_text(crate::i18n::tr("Genre, mood, instruments, voice: what the song should sound like"));
                let response = ui.add(edit);
                typing = response.has_focus();
                if response.changed() {
                    edited = Some(draft.clone());
                }
                if response.lost_focus() {
                    edited = Some(draft.clone());
                    // Left: from now on the file's own is what's shown.
                    if let Ok(mut kept) = DRAFT.lock() {
                        *kept = None;
                    }
                }
            });
            ui.horizontal(|ui| {
                let count = |used: usize, most: usize, name: String| {
                    let text = crate::i18n::tf!("{name}: {used} of {most}");
                    let colour = if used > most { theme::problem_mark() } else { theme::text_dim() };
                    RichText::new(text).size(12.0).color(colour)
                };
                ui.label(count(lyrics_length, limit.lyrics, crate::i18n::tr("Lyrics").to_string()));
                ui.label(count(style_length, limit.style, crate::i18n::tr("Style").to_string()));
                if ui.small_button(crate::i18n::tr("Copy lyrics")).on_hover_text(crate::i18n::tr("Copy the lyrics, with their tags, for Suno's lyrics box")).clicked() {
                    copy = Some(Action::CopyLyrics);
                }
                if ui.small_button(crate::i18n::tr("Copy style")).on_hover_text(crate::i18n::tr("Copy the style for Suno's style box")).clicked() {
                    copy = Some(Action::CopyStyle);
                }
                crate::fit::menu_button(ui, RichText::new(crate::i18n::tr("Limits")).size(12.0), |ui| {
                    ui.label(crate::i18n::tr("The most characters Suno's boxes take. These change with its models, so they can be changed here."));
                    ui.horizontal(|ui| {
                        ui.label(crate::i18n::tr("Lyrics"));
                        ui.add(egui::DragValue::new(&mut new_limits.lyrics).range(1..=100_000).speed(10.0));
                    });
                    ui.horizontal(|ui| {
                        ui.label(crate::i18n::tr("Style"));
                        ui.add(egui::DragValue::new(&mut new_limits.style).range(1..=100_000).speed(10.0));
                    });
                });
            });
        });
        if new_limits != limit {
            set_limits(new_limits);
        }
        if let Some(text) = edited {
            // Kept while the box has the keyboard; put in the file when it's left.
            if typing {
                if let Ok(mut kept) = DRAFT.lock() {
                    *kept = Some((id, text));
                }
            } else {
                set_style(&mut self.docs[index].text, &text);
            }
        }
        if let Some(action) = copy {
            self.lyrics_command(action, ui.ctx());
        }
    }
}

/// Puts a style into a song's front matter, keeping the caret where it was in the words.
pub fn set_style(text: &mut TextBuffer, style: &str) {
    let lines = readable_lines(text);
    if lyrics::style_of(&lines).split_whitespace().eq(style.split_whitespace()) && lyrics::front_matter_end(&lines).is_some() {
        return;
    }
    if style.trim().is_empty() && lyrics::front_matter_end(&lines).is_none() {
        return;
    }
    let (top, bottom, rows) = lyrics::with_style(&lines, style);
    let (line, col) = (text.caret_line, text.caret_col);
    let delta = rows.len() as isize - (bottom - top + 1) as isize;
    if text.replace_lines(top, bottom, rows) {
        let line = if line > bottom { (line as isize + delta).max(0) as usize } else { line };
        text.move_caret(line.min(text.line_count().saturating_sub(1)), col, false);
    }
}

#[cfg(test)]
mod tests {
    use super::super::tests::app_with;
    use super::*;

    /// An app with one document of this text.
    fn app_of(text: &str) -> (Limoniter, egui::Context) {
        let (mut app, ctx) = app_with("");
        app.docs[0].text.set_content(text.lines().map(str::to_string).chain(std::iter::once(String::new())).collect());
        (app, ctx)
    }

    fn song_app() -> (Limoniter, egui::Context) {
        let (mut app, ctx) = app_of("---\nstyle: folk\n---\n[Verse 1]\nThe light is in the night\nI hold you tight\n\n[Chorus]\nSing it again");
        crate::highlight::load_now();
        let name = lyrics::LANGUAGE;
        let syntax = crate::highlight::engine().and_then(|e| e.names().iter().find(|(n, _)| *n == name).map(|(_, s)| *s)).expect("the grammar is there");
        app.docs[0].set_language(Language::Named(syntax));
        app.docs[0].update_syntax();
        app.lyrics_tick();
        (app, ctx)
    }

    #[test]
    fn a_lyrics_file_is_known_by_its_ending_and_by_new_as() {
        crate::highlight::load_now();
        let engine = crate::highlight::engine().expect("loaded");
        for file in ["song.lyrics", "Song.SONG"] {
            let found = engine.detect(Some(Path::new(file)), "").map(|s| engine.name_of(s).to_string());
            assert_eq!(found.as_deref(), Some(lyrics::LANGUAGE), "{file}");
        }
        let (mut app, _ctx) = app_with("");
        let at = app.new_kinds.iter().position(|k| k.name == "Song lyrics (Suno and others)").expect("New as has song lyrics");
        app.new_of_kind(at);
        app.lyrics_tick();
        let doc = &app.docs[app.active];
        assert!(doc.name().ends_with(".lyrics"), "{}", doc.name());
        assert_eq!(doc.language_name(), lyrics::LANGUAGE);
        assert!(doc.text.song);
        assert!(doc.text.line(0).starts_with("---"));
        assert_eq!(lyrics::sections(&readable_lines(&doc.text)).first().map(|s| s.title.clone()).as_deref(), Some("Intro"));
        assert_eq!(super::super::new_kinds::NewKind::usual().iter().find(|k| k.name == "Song lyrics (Suno and others)").map(super::super::new_kinds::NewKind::group), Some("Text"));
    }

    #[test]
    fn tags_are_coloured_by_their_kind_and_listed_as_the_outline() {
        crate::highlight::load_now();
        let engine = crate::highlight::engine().expect("loaded");
        let syntax = engine.names().iter().find(|(n, _)| *n == lyrics::LANGUAGE).map(|(_, s)| *s).expect("grammar");
        let lines: Vec<std::sync::Arc<str>> = ["---", "style: folk", "---", "[Verse 1]", "la la", "[Chorus]", "la", "[Pre-Chorus: rising]", "[Bridge]", "[Outro]", "[Female Vocal]", "[Instrumental]"].iter().map(|s| std::sync::Arc::from(*s)).collect();
        let found = engine.list_functions(syntax, &lines, &std::sync::atomic::AtomicBool::new(false));
        let names: Vec<&str> = found.iter().map(|(_, n)| n.as_str()).collect();
        assert_eq!(names, ["Verse 1", "Chorus", "Pre-Chorus", "Bridge", "Outro", "Female Vocal", "Instrumental"]);
        assert_eq!(found.iter().map(|(l, _)| *l).collect::<Vec<_>>(), [3, 5, 7, 8, 9, 10, 11]);
        // Each kind has a colour of its own.
        let mut buffer = TextBuffer::default();
        buffer.set_content(lines.iter().map(|l| l.to_string()).collect());
        let mut colours = crate::highlight::LineColours::default();
        colours.set_syntax(Some(syntax));
        let bottom = buffer.line_count();
        while !colours.prepare(engine, &mut buffer, 0, bottom, Duration::from_millis(50)) {}
        let colour = |line: usize, at: usize| {
            let mut position = 0;
            for (length, colour) in &colours.line(line).expect("coloured").spans {
                position += length;
                if at < position {
                    return *colour;
                }
            }
            panic!("past the end");
        };
        let (verse, chorus) = (colour(3, 2), colour(5, 2));
        let kinds: std::collections::HashSet<_> = [3, 5, 7, 8, 9, 10, 11].iter().map(|line| colour(*line, 2)).collect();
        assert_ne!(verse, chorus);
        assert!(kinds.len() >= 5, "section kinds are told apart by colour: {kinds:?}");
        assert_ne!(verse, colour(4, 1), "a tag isn't coloured as the words under it");
    }

    #[test]
    fn sections_fold_like_blocks_of_code() {
        let (mut app, _ctx) = song_app();
        let text = &mut app.docs[0].text;
        assert!(text.can_fold(3) && text.can_fold(7), "a tag line can fold");
        assert!(!text.can_fold(4), "a sung line can't");
        assert_eq!(text.fold_end(3), Some(5));
        assert!(text.toggle_fold(3));
        assert!(text.is_folded(3));
        assert_eq!(text.folds().get(&3), Some(&5), "the blank line after stays showing");
        assert!(text.toggle_fold(3));
        text.collapse_all();
        assert_eq!(text.folds().len(), 2, "both sections");
        text.expand_all();
        // Not a song: folds go by indent again.
        text.song = false;
        assert!(!text.can_fold(3));
    }

    #[test]
    fn the_style_is_kept_in_the_file_and_read_back() {
        let (mut app, _ctx) = song_app();
        let text = &mut app.docs[0].text;
        text.move_caret(8, 3, false);
        set_style(text, "dark  synthwave,\nfemale vocals");
        assert_eq!(text.line(1).as_ref(), "style: dark synthwave, female vocals");
        assert_eq!(lyrics::style_of(&readable_lines(text)), "dark synthwave, female vocals");
        assert_eq!((text.caret_line, text.caret_col), (8, 3), "the cursor stays in the words");
        assert_eq!(text.line_count(), 10);
        // It survives a save and a reopen as plain text.
        let saved = readable_lines(text).join("\n");
        let again: Vec<&str> = saved.lines().collect();
        assert_eq!(lyrics::style_of(&again), "dark synthwave, female vocals");
        // A song with none gets a block of its own at the top.
        let (mut bare, _c) = app_of("[Verse]\nla la");
        set_style(&mut bare.docs[0].text, "jazz trio");
        assert_eq!(readable_lines(&bare.docs[0].text), ["---", "style: jazz trio", "---", "[Verse]", "la la", ""]);
        // A box typed in and then left alone changes nothing.
        let before = bare.docs[0].text.version();
        set_style(&mut bare.docs[0].text, "jazz trio");
        assert_eq!(bare.docs[0].text.version(), before);
    }

    #[test]
    fn typing_a_bracket_offers_the_tags_in_a_song_only() {
        let (mut app, _ctx) = song_app();
        let doc = &mut app.docs[0];
        doc.text.move_caret(9, 0, false);
        doc.text.type_text("[Ch");
        crate::views::offer_completions(&doc.text, &mut doc.text_state, false);
        let offered = doc.text_state.completion.as_ref().map(|c| (c.start, c.items.clone()));
        assert_eq!(offered, Some((1, vec!["Chorus]".to_string(), "Choir]".to_string()])));
        doc.text.song = false;
        crate::views::offer_completions(&doc.text, &mut doc.text_state, false);
        assert!(doc.text_state.completion.is_none(), "not in other files");
    }

    #[test]
    fn the_copy_outs_go_to_the_clipboard_with_a_note() {
        let (mut app, ctx) = song_app();
        app.lyrics_command(Action::CopyLyrics, &ctx);
        let copied = |ctx: &egui::Context| ctx.output(|o| o.commands.iter().rev().find_map(|c| if let egui::OutputCommand::CopyText(t) = c { Some(t.clone()) } else { None }));
        assert_eq!(copied(&ctx).as_deref(), Some("[Verse 1]\nThe light is in the night\nI hold you tight\n\n[Chorus]\nSing it again"));
        app.lyrics_command(Action::CopyPlain, &ctx);
        assert_eq!(copied(&ctx).as_deref(), Some("The light is in the night\nI hold you tight\n\nSing it again"));
        app.lyrics_command(Action::CopyStyle, &ctx);
        assert_eq!(copied(&ctx).as_deref(), Some("folk"));
        assert!(app.docs[0].text.notice.as_deref().is_some_and(|n| n.contains("style")));
    }

    #[test]
    fn the_margin_shows_syllables_and_rhymes_of_a_song_and_the_limits_can_be_changed() {
        let (app, _ctx) = song_app();
        let m = marks(&app.docs[0].text, 0, 20).lines;
        assert_eq!(m[&4].syllables, Some((6, false)));
        assert_eq!(m[&4].rhyme.map(|r| r.0), Some('A'));
        assert_eq!(m[&5].rhyme, Some(('A', false)), "night and tight");
        assert!(!m.contains_key(&3), "no marks by a tag");
        assert_eq!(Limits::from_text("lyrics_limit=3000\nstyle_limit=abc\n"), Limits { lyrics: 3000, style: lyrics::DEFAULT_STYLE_LIMIT });
        assert_eq!(Limits::from_text(&Limits { lyrics: 12, style: 34 }.to_text()), Limits { lyrics: 12, style: 34 });
    }
}
