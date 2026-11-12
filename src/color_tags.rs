//! Colour tags: marking sections of a file in a colour, which shows as a block down the left of the editor and as a
//! band right across the document map, so they can be found and gone to at a glance. And Help, opened as a tab.
//!
//! Tags live with the lines, so they move as lines come and go above them, and grow and shrink with what's typed
//! inside them. For a saved file they're kept beside the settings, and come back when it's opened again.

use super::*;
use std::collections::BTreeMap;
use std::ops::Range;

/// How far a tag's first line is looked for when a file has changed since its tags were kept.
const LOOK_FOR_MOVED: usize = 500;
const HELP_NAME: &str = "How Limoniter works.md";
const FEATURES_NAME: &str = "What Limoniter can do.md";
/// The list of what Limoniter can do, kept with the source as FEATURES.md and built into the program.
const FEATURES: &str = include_str!("../FEATURES.md");

/// The help as a Markdown page: each section's first line is its heading, and each line after it a point of its own.
pub(super) fn help_markdown() -> String {
    let mut page = String::from("# How Limoniter works\n\nPress F1 to find any command by what it does. This page is an ordinary Markdown file: View, Preview shows its text.\n");
    for section in HELP.split("\n\n") {
        let mut lines = section.lines().map(str::trim).filter(|line| !line.is_empty());
        let Some(title) = lines.next() else {
            continue;
        };
        page += &format!("\n## {title}\n\n");
        for line in lines {
            page += &format!("- {line}\n");
        }
    }
    page
}

/// A tag as kept in a file: first line, last line, colour, and the first line's text, to check it's still there.
fn tags_to_text(tags: &[(usize, usize, u8)], line: &dyn Fn(usize) -> Arc<str>) -> String {
    tags.iter().map(|(first, last, colour)| format!("{first}\t{last}\t{colour}\t{}\n", line(*first))).collect()
}

/// The tags in a kept file that still fit the lines, each moved to where its first line is now.
pub(super) fn tags_from_text(text: &str, count: usize, line: &dyn Fn(usize) -> Arc<str>) -> BTreeMap<usize, (usize, u8)> {
    let mut tags = BTreeMap::new();
    for entry in text.lines() {
        let mut parts = entry.splitn(4, '\t');
        let (Some(first), Some(last), Some(colour), Some(words)) = (parts.next(), parts.next(), parts.next(), parts.next()) else {
            continue;
        };
        let (Ok(first), Ok(last), Ok(colour)) = (first.parse::<usize>(), last.parse::<usize>(), colour.parse::<u8>()) else {
            continue;
        };
        if last < first || colour as usize >= theme::TAG_NAMES.len() {
            continue;
        }
        // Where it was, or the nearest line that reads the same.
        let found = (0..=LOOK_FOR_MOVED).find_map(|away| {
            [first.checked_sub(away), Some(first + away)].into_iter().flatten().find(|at| *at < count && &*line(*at) == words)
        });
        if let Some(at) = found {
            let last = (last + at).saturating_sub(first).min(count.saturating_sub(1));
            if !tags.range(..=last).next_back().is_some_and(|(_, (end, _))| *end >= at) {
                tags.insert(at, (last, colour));
            }
        }
    }
    tags
}

impl Limoniter {
    fn tags_root(&self) -> Option<PathBuf> {
        self.history_root.as_ref().and_then(|root| root.parent()).map(|home| home.join("tags"))
    }

    fn tags_file(&self, path: &Path) -> Option<PathBuf> {
        self.tags_root().map(|root| crate::history_file::file_for(&root, path).with_extension("tags"))
    }

    /// Brings back the tags kept for a file, as it's opened.
    pub(super) fn recall_tags(&mut self, doc: &mut Document, path: &Path) {
        let Some(file) = self.tags_file(path) else {
            return;
        };
        let Ok(text) = std::fs::read_to_string(file) else {
            return;
        };
        let count = doc.text.line_count();
        let tags = tags_from_text(&text, count, &|at| doc.text.line(at));
        doc.text.set_tags(tags);
        self.tags_kept.insert(doc.id, doc.text.tags());
    }

    /// Writes down a saved file's tags whenever they change, so they're there next time. While a file has unsaved
    /// changes its tags wait, as they're placed by its lines as they'll be saved.
    pub(super) fn keep_tags(&mut self) {
        // A screenshot run changes nothing, as with the settings.
        if self.tags_root().is_none() || (self.screenshot.is_some() && !cfg!(test)) {
            return;
        }
        for index in 0..self.docs.len() {
            let doc = &self.docs[index];
            let Some(path) = doc.path.clone() else {
                continue;
            };
            if doc.placeholder || doc.is_modified() || doc.is_busy() || doc.hex_mode || doc.narrow.is_some() {
                continue;
            }
            let now = doc.text.tags();
            let kept = self.tags_kept.get(&doc.id);
            if kept == Some(&now) || (kept.is_none() && now.is_empty()) {
                continue;
            }
            let id = doc.id;
            if let Some(file) = self.tags_file(&path) {
                if now.is_empty() {
                    let _ = std::fs::remove_file(&file);
                } else {
                    let text = tags_to_text(&now, &|at| self.docs[index].text.line(at));
                    if let Some(folder) = file.parent() {
                        let _ = std::fs::create_dir_all(folder);
                    }
                    let _ = codec::write_file(&file, text.as_bytes());
                }
            }
            self.tags_kept.insert(id, now);
        }
    }

    /// The lines a tag goes on: those selected, or the function or paragraph the cursor is in.
    fn lines_to_tag(&self) -> Range<usize> {
        region_panel::chosen_lines(&self.docs[self.active].text)
    }

    pub(super) fn tag_here(&mut self, colour: u8) {
        let range = self.lines_to_tag();
        if range.is_empty() {
            return;
        }
        let text = &mut self.doc().text;
        text.tag_lines(range.start, range.end - 1, colour);
        // Where it starts, so the block in the margin can be seen.
        if !text.has_selection() {
            text.reveal = true;
        }
    }

    pub(super) fn untag_here(&mut self) {
        let text = &self.docs[self.active].text;
        let range = if text.has_selection() { self.lines_to_tag() } else { text.caret_line..text.caret_line + 1 };
        if range.is_empty() || !self.doc().text.untag_lines(range.start, range.end - 1) {
            self.message("Colour tags", "There's no tag here to take away. Put the cursor in a tagged section first.".into());
        }
    }

    pub(super) fn go_to_tag(&mut self, next: bool) {
        if !self.doc().text.go_to_tag(next) {
            self.message("Colour tags", "This file has no colour tags yet. Select some lines, or put the cursor in a function or paragraph, and choose a colour from Edit, Colour tag, or by right-clicking.".into());
        }
    }

    /// Whether there's a tag where taking one away would act.
    pub(super) fn tag_here_to_remove(&self) -> bool {
        let text = &self.docs[self.active].text;
        let range = if text.has_selection() { self.lines_to_tag() } else { text.caret_line..text.caret_line + 1 };
        text.tags().iter().any(|(first, last, _)| *first < range.end && range.start <= *last)
    }

    /// How things work, as a tab of its own that can be read, searched and kept open beside the work.
    pub(super) fn open_help(&mut self) {
        self.open_page(HELP_NAME, help_markdown());
    }

    /// The list of everything Limoniter can do, opened as a page.
    pub(super) fn open_features(&mut self) {
        self.open_page(FEATURES_NAME, FEATURES.replace("\r\n", "\n"));
    }

    /// A built-in Markdown page, opened as a tab showing the page it makes, or gone to if it's open already.
    fn open_page(&mut self, name: &str, markdown: String) {
        if let Some(index) = self.docs.iter().position(|doc| !doc.placeholder && doc.path.is_none() && doc.untitled == name) {
            self.active = index;
            self.pending_focus = true;
            return;
        }
        self.new_document();
        // It isn't one of the numbered new files, so the next of those keeps its number.
        self.untitled_count -= 1;
        let doc = self.doc();
        doc.untitled = name.into();
        doc.text.set_content(markdown.lines().map(str::to_string).collect());
        doc.text.mark_saved();
        doc.text.move_caret(0, 0, false);
        // It opens as the page it makes, rather than as its text.
        doc.preview_view = settings::PreviewView::Full;
        doc.resolved_syntax = None;
        self.pending_focus = true;
    }

    #[cfg(test)]
    fn run_command_for_test(&mut self, command: Command, ctx: &Context) {
        let mut output = ctx.run_ui(egui::RawInput::default(), |ui| self.run(command, ui));
        output.textures_delta.clear();
    }

    pub(super) fn tags_screenshot_step(&mut self) {
        let text = &mut self.doc().text;
        for (first, last, colour) in [(2, 8, 0u8), (11, 14, 3), (17, 23, 4), (40, 60, 1), (120, 150, 5), (220, 240, 2)] {
            text.tag_lines(first, last, colour);
        }
        text.move_caret(12, 0, false);
    }
}

#[cfg(test)]
mod tests {

    /// The feature list is kept up to date with what Limoniter reads: every picture format and the newer
    /// additions must be in it.
    #[test]
    fn the_feature_list_names_what_limoniter_reads() {
        let lower = FEATURES.to_lowercase();
        for name in crate::pictures::Format::ALL.map(crate::pictures::Format::name) {
            let key = name.split_whitespace().find(|w| w.len() >= 3 && !matches!(*w, "picture" | "image" | "screen" | "file" | "Kodak")).unwrap_or(name);
            assert!(lower.contains(&key.to_lowercase()), "FEATURES.md doesn't mention {name}");
        }
        for must in ["cp/m", "mbasic", "rp9", "amiga installer", "colour cycling"] {
            assert!(lower.contains(must), "FEATURES.md doesn't mention {must}");
        }
    }

    use super::*;

    fn app_with_text(text: &[&str]) -> (Limoniter, Context) {
        let mut app = Limoniter::build(String::new(), None, Some(PathBuf::from("never-written.png")));
        app.new_document();
        app.doc().text.set_content(text.iter().map(|line| line.to_string()).collect());
        (app, Context::default())
    }

    fn frame(app: &mut Limoniter, ctx: &Context, events: Vec<egui::Event>) -> egui::FullOutput {
        let raw = egui::RawInput { screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1600.0, 900.0))), events, ..Default::default() };
        let mut output = ctx.run_ui(raw, |ui| app.draw_frame(ui));
        output.textures_delta.clear();
        output
    }

    fn key(key: Key, modifiers: Modifiers) -> egui::Event {
        egui::Event::Key { key, physical_key: None, pressed: true, repeat: false, modifiers: if modifiers.ctrl { modifiers | Modifiers::COMMAND } else { modifiers } }
    }

    const CODE: [&str; 9] = ["fn one() {", "    a();", "}", "", "fn two() {", "    b();", "}", "", "end"];

    #[test]
    fn a_tag_goes_on_the_function_and_follows_its_lines() {
        let (mut app, ctx) = app_with_text(&CODE);
        app.doc().text.move_caret(5, 2, false);
        frame(&mut app, &ctx, vec![key(Key::Num5, Modifiers::CTRL | Modifiers::SHIFT)]);
        assert_eq!(app.docs[app.active].text.tags(), [(4, 6, 4)], "Ctrl+Shift+5 tags fn two blue");
        let text = &mut app.doc().text;
        text.splice_lines(0, 0, vec![Arc::from("// one"), Arc::from("// two")]);
        assert_eq!(text.tags(), [(6, 8, 4)], "lines added above move it down");
        text.splice_lines(7, 7, vec![Arc::from("    c();")]);
        assert_eq!(text.tags(), [(6, 9, 4)], "lines added inside it make it longer");
        text.move_caret(9, 0, false);
        text.new_line();
        assert_eq!(text.tags(), [(6, 10, 4)], "and so does Enter on its last line");
        text.splice_lines(6, 11, Vec::new());
        assert!(text.tags().is_empty(), "taking all its lines away takes the tag away");
    }

    #[test]
    fn tags_are_gone_to_with_f4_and_taken_away_with_ctrl_shift_0() {
        let (mut app, ctx) = app_with_text(&CODE);
        app.doc().text.tag_lines(0, 2, 0);
        app.doc().text.tag_lines(4, 6, 3);
        app.doc().text.move_caret(8, 0, false);
        frame(&mut app, &ctx, vec![key(Key::F4, Modifiers::NONE)]);
        assert_eq!(app.docs[app.active].text.caret_line, 0, "round to the first");
        frame(&mut app, &ctx, vec![key(Key::F4, Modifiers::NONE)]);
        assert_eq!(app.docs[app.active].text.caret_line, 4);
        frame(&mut app, &ctx, vec![key(Key::F4, Modifiers::SHIFT)]);
        assert_eq!(app.docs[app.active].text.caret_line, 0);
        assert!(app.can(Command::Untag));
        frame(&mut app, &ctx, vec![key(Key::Num0, Modifiers::CTRL | Modifiers::SHIFT)]);
        assert_eq!(app.docs[app.active].text.tags(), [(4, 6, 3)]);
        assert!(!app.can(Command::Untag), "nothing left to take away here");
        // A new colour on the same lines replaces the old one, rather than piling up.
        app.doc().text.move_caret(5, 0, false);
        app.run_command_for_test(Command::Tag(1), &ctx);
        assert_eq!(app.docs[app.active].text.tags(), [(4, 6, 1)]);
    }

    #[test]
    fn tags_are_drawn_in_the_margin_and_across_the_map() {
        let (mut app, ctx) = app_with_text(&CODE);
        frame(&mut app, &ctx, Vec::new());
        let plain = frame(&mut app, &ctx, Vec::new()).shapes.len();
        app.doc().text.tag_lines(4, 6, 2);
        let tagged = frame(&mut app, &ctx, Vec::new()).shapes.len();
        assert!(tagged >= plain + 3, "a wash, a block and a band: {plain} then {tagged}");
    }

    #[test]
    fn a_saved_files_tags_are_kept_for_next_time() {
        let home = std::env::temp_dir().join(format!("limoniter-tags-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&home);
        std::fs::create_dir_all(&home).unwrap();
        let file = home.join("code.rs");
        std::fs::write(&file, CODE.join("\r\n")).unwrap();
        let (mut app, ctx) = app_with_text(&[]);
        app.history_root = Some(home.join("history"));
        assert!(app.open_path(&file));
        app.doc().text.tag_lines(4, 6, 5);
        frame(&mut app, &ctx, Vec::new());
        let mut again = Limoniter::build(String::new(), None, Some(PathBuf::from("never-written.png")));
        again.history_root = Some(home.join("history"));
        assert!(again.open_path(&file));
        assert_eq!(again.docs[again.active].text.tags(), [(4, 6, 5)]);
        drop((app, again));
        let _ = std::fs::remove_dir_all(home);
    }

    #[test]
    fn how_things_work_opens_as_a_tab_of_its_own() {
        let (mut app, ctx) = app_with_text(&["my work"]);
        app.run_command_for_test(Command::Help, &ctx);
        let help = &app.docs[app.active];
        assert_eq!(help.name(), HELP_NAME);
        assert!(help.text.line_count() > 50);
        assert_eq!(&*help.text.line(0), "# How Limoniter works");
        assert!(help.text.all_lines().iter().any(|line| &**line == "## Colour tags"), "each section is a heading");
        assert_eq!(help.preview_view, settings::PreviewView::Full, "it opens as the page, not the text");
        frame(&mut app, &ctx, Vec::new());
        assert!(app.full_preview_shown, "and the page is showing");
        assert!(app.preview_blocks.iter().any(|block| block.kind == preview::BlockKind::Heading(2)));
        assert!(!app.docs[app.active].is_modified(), "closing it doesn't ask about saving");
        assert!(matches!(app.dialog, Dialog::None), "no box");
        let tabs = app.docs.len();
        app.active = 0;
        app.run_command_for_test(Command::Help, &ctx);
        assert_eq!(app.docs.len(), tabs, "a second time goes back to it");
        assert_eq!(app.docs[app.active].name(), HELP_NAME);
        app.new_document();
        assert_eq!(app.docs[app.active].name(), "Edit2", "numbered new files carry on as before");
    }

    fn lines(words: &[&str]) -> Vec<Arc<str>> {
        words.iter().map(|w| Arc::from(*w)).collect()
    }

    #[test]
    fn kept_tags_come_back_where_their_lines_are_now() {
        let before = lines(&["a", "fn one() {", "x", "}", "b"]);
        let kept = tags_to_text(&[(1, 3, 2)], &|at| before[at].clone());
        assert_eq!(kept, "1\t3\t2\tfn one() {\n");
        assert_eq!(tags_from_text(&kept, before.len(), &|at| before[at].clone()), BTreeMap::from([(1, (3, 2))]));
        // Two lines were added above it outside Limoniter.
        let after = lines(&["new", "new", "a", "fn one() {", "x", "}", "b"]);
        assert_eq!(tags_from_text(&kept, after.len(), &|at| after[at].clone()), BTreeMap::from([(3, (5, 2))]));
        // Its first line has gone altogether.
        let gone = lines(&["a", "b"]);
        assert!(tags_from_text(&kept, gone.len(), &|at| gone[at].clone()).is_empty());
        assert!(tags_from_text("rubbish\n1\t0\t2\tx\n1\t1\t99\tx\n", 3, &|_| Arc::from("x")).is_empty(), "nonsense is left out");
    }
}
