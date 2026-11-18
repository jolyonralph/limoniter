//! The Search panel, as VS Code has one beside the Explorer: what to find (and, opened up, what to put in its
//! place), searched for as it's typed in the open folder, the project, or else the folder of the file in front.
//! What's found is a tree, a file and under it each line with a match, the match marked; with something to put in
//! its place, each match shows struck through with the new text after it, so the change can be seen before it's
//! made. A file can be put out of the way, so Replace all leaves it; Replace all, or in one file, asks first and
//! then changes open files in their tabs (unsaved, to undo) and the rest on disk, as Replace in Files does.

use super::*;
use crate::find_files::{self, Hit, SearchJob};
use regex::Regex;
use std::collections::{HashMap, HashSet};
use std::time::Instant;

/// The most rows drawn for the results; past that, what's found is still counted.
const MOST_ROWS: usize = 20_000;

#[derive(Default)]
pub(super) struct SearchPanel {
    pub open: bool,
    pub query: String,
    pub replace: String,
    pub show_replace: bool,
    pub match_case: bool,
    pub whole_word: bool,
    pub regex: bool,
    /// Names to search, and to leave out: patterns such as *.rs;*.s.
    pub include: String,
    pub exclude: String,
    show_patterns: bool,
    pub job: Option<Arc<SearchJob>>,
    /// What the job was asked: the options, the patterns, and the regular expression, for showing replacements.
    asked: Option<(SearchOptions, String, String)>,
    pattern: Option<Regex>,
    /// When the words last changed, to search a moment after typing stops.
    typed_at: Option<Instant>,
    collapsed: HashSet<PathBuf>,
    /// Files put out of the way.
    pub dismissed: HashSet<PathBuf>,
    pub focus: bool,
    /// Searching again once a replace has finished, to show what's left.
    again_after_replace: bool,
    pub problem: String,
}

/// One row of the tree: a file (by its place in the list), or one of its matches.
#[derive(Clone, Copy)]
enum Row {
    File(usize),
    Match(usize, usize),
}

/// What's asked for in the panel, done after it's drawn.
enum Act {
    Open(Hit),
    Toggle(PathBuf),
    Dismiss(PathBuf),
    ReplaceIn(Vec<PathBuf>, usize),
}

impl SearchPanel {
    /// The options for what's typed.
    fn options(&self) -> SearchOptions {
        SearchOptions { pattern: self.query.clone(), replacement: self.replace.clone(), match_case: self.match_case, whole_word: self.whole_word, regex: self.regex, ..SearchOptions::default() }
    }

    /// The text a match becomes: with a regular expression, its groups put in ($1 and the like).
    fn replacement(&self, hit: &Hit) -> String {
        match (&self.pattern, self.regex) {
            (Some(re), true) => re
                .captures_at(&hit.text, hit.start)
                .filter(|c| c.get(0).is_some_and(|m| m.start() == hit.start))
                .map(|caps| {
                    let mut out = String::new();
                    caps.expand(&self.replace, &mut out);
                    out
                })
                .unwrap_or_else(|| self.replace.clone()),
            _ => self.replace.clone(),
        }
    }
}

/// The files found and their matches, in the order they were found, leaving out those put away and those whose
/// names are to be left out.
fn grouped(hits: &[Hit], dismissed: &HashSet<PathBuf>, exclude: &str) -> Vec<(PathBuf, Vec<Hit>)> {
    let mut files: Vec<(PathBuf, Vec<Hit>)> = Vec::new();
    let mut places: HashMap<PathBuf, usize> = HashMap::new();
    for hit in hits {
        if dismissed.contains(&hit.path) {
            continue;
        }
        let at = *places.entry(hit.path.clone()).or_insert_with(|| {
            files.push((hit.path.clone(), Vec::new()));
            files.len() - 1
        });
        files[at].1.push(hit.clone());
    }
    if !exclude.trim().is_empty() {
        files.retain(|(path, _)| !path.file_name().is_some_and(|n| find_files::name_matches(&n.to_string_lossy(), exclude)));
    }
    files
}

impl Limoniter {
    /// Opens the Search panel (Ctrl+Shift+F), with the selection, if it's on one line, to find.
    pub(super) fn open_search_panel(&mut self) {
        let doc = &self.docs[self.active];
        if !self.no_file() && !doc.hex_mode && doc.text.has_selection() && doc.text.caret_line == doc.text.anchor_line {
            let selected = doc.text.selection_text();
            if !selected.trim().is_empty() && selected.chars().count() <= 200 {
                self.search.query = selected;
            }
        }
        self.search.open = true;
        self.search.focus = true;
    }

    /// Where the panel searches, and how it says so: the open folder's files, the project's, or the file in
    /// front's folder and those in it.
    fn search_scope(&self) -> Option<(String, Result<Vec<PathBuf>, PathBuf>)> {
        if let Some(project) = self.project.as_ref().filter(|p| !p.files.is_empty()) {
            let name = project.root.as_deref().and_then(Path::file_name).map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| project.name.clone());
            return Some((name, Ok(project.files.clone())));
        }
        let folder = self.docs.get(self.active).and_then(|d| d.path.as_deref()).and_then(Path::parent)?;
        Some((folder.display().to_string(), Err(folder.to_path_buf())))
    }

    /// Starts searching for what's typed, stopping any search before.
    fn search_panel_start(&mut self) {
        if let Some(job) = self.search.job.take() {
            job.cancel.store(true, Ordering::Relaxed);
        }
        self.search.problem.clear();
        self.search.dismissed.clear();
        let options = self.search.options();
        self.search.asked = Some((options.clone(), self.search.include.clone(), self.search.exclude.clone()));
        if self.search.query.is_empty() {
            self.search.pattern = None;
            return;
        }
        self.search.pattern = TextBuffer::build_regex(&options).ok();
        let include = self.search.include.clone();
        let named = |path: &Path| path.file_name().is_some_and(|n| find_files::name_matches(&n.to_string_lossy(), &include));
        let started = match self.search_scope() {
            Some((_, Ok(files))) => find_files::start_list(files.into_iter().filter(|p| named(p)).collect(), &options),
            Some((_, Err(folder))) => find_files::start(folder, include, true, &options),
            None => Err(crate::i18n::tr("Open a folder (File, Open folder) or a file to search beside.").to_string()),
        };
        match started {
            Ok(job) => self.search.job = Some(job),
            Err(problem) => self.search.problem = problem,
        }
    }

    /// Searches now and waits for it, for a screenshot.
    pub(super) fn search_panel_now(&mut self) {
        self.search_panel_start();
        if let Some(job) = &self.search.job {
            let began = Instant::now();
            while !job.finished.load(Ordering::SeqCst) && began.elapsed() < Duration::from_secs(10) {
                std::thread::sleep(Duration::from_millis(20));
            }
        }
    }

    /// The Search panel, at the left.
    pub(super) fn search_panel(&mut self, ui: &mut Ui) {
        if !self.search.open {
            return;
        }
        // A moment after typing stops (or at once, for Enter), what's typed is searched for.
        let wanted = (self.search.options(), self.search.include.clone(), self.search.exclude.clone());
        let changed = self.search.asked.as_ref().is_none_or(|(o, i, e)| (o.pattern.as_str(), o.match_case, o.whole_word, o.regex, i.as_str(), e.as_str()) != (wanted.0.pattern.as_str(), wanted.0.match_case, wanted.0.whole_word, wanted.0.regex, wanted.1.as_str(), wanted.2.as_str()));
        if changed && self.search.typed_at.is_none_or(|t| t.elapsed() >= Duration::from_millis(300)) {
            self.search.typed_at = None;
            self.search_panel_start();
        } else if changed {
            ui.ctx().request_repaint_after(Duration::from_millis(100));
        }
        if !changed && self.search.asked.as_ref().is_some_and(|(o, ..)| o.replacement != self.search.replace) {
            // Only what it's replaced with: nothing to search again.
            if let Some(asked) = self.search.asked.as_mut() {
                asked.0.replacement = self.search.replace.clone();
            }
        }
        if self.search.again_after_replace && self.replace_job.is_none() && !matches!(self.dialog, Dialog::ConfirmReplaceInFiles { .. }) {
            self.search.again_after_replace = false;
            self.search_panel_start();
        }
        let job = self.search.job.clone();
        let finished = job.as_ref().is_none_or(|j| j.finished.load(Ordering::SeqCst));
        if !finished {
            ui.ctx().request_repaint_after(Duration::from_millis(150));
        }
        let hits: Vec<Hit> = job.as_ref().map(|j| j.hits.lock().unwrap().clone()).unwrap_or_default();
        let files = grouped(&hits, &self.search.dismissed, &self.search.exclude);
        let shown_matches: usize = files.iter().map(|(_, h)| h.len()).sum();
        let scope = self.search_scope();
        let root = self.project.as_ref().and_then(|p| p.root.clone()).or_else(|| scope.as_ref().and_then(|(_, s)| s.as_ref().err().cloned())).unwrap_or_default();
        let mut rows: Vec<Row> = Vec::new();
        for (n, (path, found)) in files.iter().enumerate() {
            rows.push(Row::File(n));
            if !self.search.collapsed.contains(path) {
                rows.extend((0..found.len()).map(|m| Row::Match(n, m)));
            }
            if rows.len() >= MOST_ROWS {
                break;
            }
        }
        let mut acts: Vec<Act> = Vec::new();
        let mut close = false;
        let replacing = self.search.show_replace;
        Panel::left("search-panel").default_size(300.0).size_range(200.0..=900.0).resizable(true).frame(egui::Frame::new().fill(theme::window()).inner_margin(egui::Margin::symmetric(6, 4))).show(ui, |ui| {
            let s = &mut self.search;
            ui.horizontal(|ui| {
                ui.label(RichText::new(crate::i18n::tr("Search")).color(theme::text()).size(12.5));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    close = crate::access::called(ui.add(egui::Button::new(RichText::new(crate::i18n::tr("\u{D7}")).color(theme::text_dim())).frame(false)), "Close").on_hover_text(crate::i18n::tr("Close")).clicked();
                    if crate::access::called(ui.add(egui::Button::new(RichText::new("\u{2026}").color(theme::text_dim())).frame(false)), "Files to search").on_hover_text(crate::i18n::tr("Files to search, and to leave out")).clicked() {
                        s.show_patterns = !s.show_patterns;
                    }
                });
            });
            ui.horizontal(|ui| {
                let arrow = if s.show_replace { "\u{25BE}" } else { "\u{25B8}" };
                if crate::access::called(ui.add(egui::Button::new(arrow).frame(false)), "Replace").on_hover_text(crate::i18n::tr("Replace")).clicked() {
                    s.show_replace = !s.show_replace;
                }
                ui.vertical(|ui| {
                    // Room left for the three switches (or Replace all) beside each box, so the panel never grows.
                    let width = (ui.available_width() - 118.0).max(60.0);
                    ui.horizontal(|ui| {
                        let field = ui.add(egui::TextEdit::singleline(&mut s.query).hint_text(crate::i18n::tr("Search")).desired_width(width));
                        if s.focus {
                            field.request_focus();
                            s.focus = false;
                        }
                        if field.changed() {
                            s.typed_at = Some(Instant::now());
                        }
                        if field.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter)) {
                            s.typed_at = None;
                        }
                        let toggle = |ui: &mut Ui, on: &mut bool, text: &str, tip: &str| {
                            if ui.selectable_label(*on, RichText::new(text).monospace().size(12.0)).on_hover_text(crate::i18n::tr(tip)).clicked() {
                                *on = !*on;
                            }
                        };
                        toggle(ui, &mut s.match_case, "Aa", "Match case");
                        toggle(ui, &mut s.whole_word, "ab", "Whole words only");
                        toggle(ui, &mut s.regex, ".*", "Regular expression");
                    });
                    if s.show_replace {
                        ui.horizontal(|ui| {
                            ui.add(egui::TextEdit::singleline(&mut s.replace).hint_text(crate::i18n::tr("Replace")).desired_width(width));
                            let all: Vec<PathBuf> = files.iter().map(|(p, _)| p.clone()).collect();
                            let can = finished && !all.is_empty() && !s.query.is_empty();
                            if ui.add_enabled(can, egui::Button::new(crate::i18n::tr("Replace all"))).on_hover_text(crate::i18n::tr("In every file listed; asks first")).clicked() {
                                acts.push(Act::ReplaceIn(all, shown_matches));
                            }
                        });
                    }
                });
            });
            if s.show_patterns {
                ui.add(egui::TextEdit::singleline(&mut s.include).hint_text(crate::i18n::tr("Files to search, such as *.rs;*.s")).desired_width(f32::INFINITY));
                ui.add(egui::TextEdit::singleline(&mut s.exclude).hint_text(crate::i18n::tr("Files to leave out, such as *.min.js")).desired_width(f32::INFINITY));
            }
            // What's been searched, and how much was found.
            let summary = if !s.problem.is_empty() {
                s.problem.clone()
            } else if s.query.is_empty() {
                match &scope {
                    Some((name, _)) => crate::i18n::tf!("Type to search {name}."),
                    None => crate::i18n::tr("Open a folder (File, Open folder) or a file to search beside.").to_string(),
                }
            } else {
                let matches = if shown_matches == 1 { crate::i18n::tr("1 match").to_string() } else { crate::i18n::tf!("{shown_matches} matches") };
                let found = if files.len() == 1 { crate::i18n::tf!("{matches} in 1 file") } else { crate::i18n::tf!("{matches} in {} files", files.len()) };
                let truncated = job.as_ref().is_some_and(|j| j.truncated.load(Ordering::Relaxed));
                match (finished, truncated) {
                    (false, _) => crate::i18n::tf!("{found}, still searching\u{2026}"),
                    (true, true) => crate::i18n::tf!("{found} (stopped at {})", find_files::MAX_HITS),
                    (true, false) => found,
                }
            };
            ui.label(RichText::new(summary).color(theme::text_dim()).size(12.0));
            ui.separator();
            let font = egui::FontId::monospace(12.5);
            let row_height = ui.text_style_height(&egui::TextStyle::Body).max(18.0) + 3.0;
            egui::ScrollArea::vertical().id_salt("search-results").auto_shrink([false, false]).show_rows(ui, row_height, rows.len(), |ui, range| {
                for row in &rows[range] {
                    let (rect, response) = ui.allocate_exact_size(egui::vec2(ui.available_width(), row_height), egui::Sense::click());
                    let hovered = response.hovered();
                    if hovered {
                        ui.painter().rect_filled(rect, 3.0, theme::hover());
                    }
                    match *row {
                        Row::File(n) => {
                            let (path, found) = &files[n];
                            let open = !s.collapsed.contains(path);
                            let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
                            let folder = path.parent().map(|f| if root.as_os_str().is_empty() { f.display().to_string() } else { crate::explorer::relative(&root, f) }).unwrap_or_default();
                            let mut job = egui::text::LayoutJob::default();
                            job.append(if open { "\u{25BE} " } else { "\u{25B8} " }, 0.0, egui::TextFormat { font_id: egui::FontId::proportional(12.5), color: theme::text_dim(), ..Default::default() });
                            job.append(&name, 0.0, egui::TextFormat { font_id: egui::FontId::proportional(13.0), color: theme::text(), ..Default::default() });
                            job.append(&format!("  {folder}"), 0.0, egui::TextFormat { font_id: egui::FontId::proportional(11.5), color: theme::text_faint(), ..Default::default() });
                            let galley = ui.painter().layout_job(job);
                            ui.painter().with_clip_rect(rect.shrink2(egui::vec2(0.0, 0.0)).intersect(egui::Rect::from_min_max(rect.min, egui::pos2(rect.max.x - 56.0, rect.max.y)))).galley(egui::pos2(rect.min.x + 2.0, rect.center().y - galley.size().y / 2.0), galley, theme::text());
                            // How many, and (under the pointer) replace or put away.
                            let count = egui::Rect::from_min_size(egui::pos2(rect.max.x - 30.0, rect.min.y + 2.0), egui::vec2(28.0, row_height - 4.0));
                            ui.painter().rect_filled(count, 8.0, theme::selected());
                            ui.painter().text(count.center(), egui::Align2::CENTER_CENTER, found.len().to_string(), egui::FontId::proportional(11.0), theme::text_dim());
                            let mut on_button = false;
                            if hovered || ui.rect_contains_pointer(rect) {
                                let gone = ui.put(egui::Rect::from_min_size(egui::pos2(rect.max.x - 52.0, rect.min.y), egui::vec2(20.0, row_height)), egui::Button::new(RichText::new("\u{D7}").color(theme::text_dim())).frame(false));
                                on_button |= gone.hovered();
                                if crate::access::called(gone, "Leave this file out").on_hover_text(crate::i18n::tr("Leave this file out of the results, and of Replace all")).clicked() {
                                    acts.push(Act::Dismiss(path.clone()));
                                }
                                if replacing && finished {
                                    let swap = ui.put(egui::Rect::from_min_size(egui::pos2(rect.max.x - 74.0, rect.min.y), egui::vec2(20.0, row_height)), egui::Button::new(RichText::new("\u{21C4}").color(theme::text_dim())).frame(false));
                                    on_button |= swap.hovered();
                                    if crate::access::called(swap, "Replace in this file").on_hover_text(crate::i18n::tr("Replace in this file; asks first")).clicked() {
                                        acts.push(Act::ReplaceIn(vec![path.clone()], found.len()));
                                    }
                                }
                            }
                            crate::access::button(&response, &format!("{name}, {}", found.len()));
                            if response.clicked() && !on_button {
                                acts.push(Act::Toggle(path.clone()));
                            }
                        }
                        Row::Match(n, m) => {
                            let hit = &files[n].1[m];
                            let text = &hit.text;
                            let (start, end) = (hit.start.min(text.len()), hit.end.min(text.len()));
                            let (start, end) = if text.is_char_boundary(start) && text.is_char_boundary(end) { (start, end) } else { (0, 0) };
                            // A little before the match, its leading space left off.
                            let before = text[..start].trim_start();
                            let before: String = if before.chars().count() > 24 { format!("\u{2026}{}", before.chars().rev().take(23).collect::<Vec<_>>().into_iter().rev().collect::<String>()) } else { before.to_string() };
                            let plain = egui::TextFormat { font_id: font.clone(), color: theme::text_dim(), ..Default::default() };
                            let mut job = egui::text::LayoutJob::default();
                            job.append(&before, 0.0, plain.clone());
                            if replacing {
                                job.append(&text[start..end], 0.0, egui::TextFormat { font_id: font.clone(), color: theme::text(), background: theme::compare_removed(), strikethrough: egui::Stroke::new(1.0, theme::text()), ..Default::default() });
                                job.append(&s.replacement(hit), 0.0, egui::TextFormat { font_id: font.clone(), color: theme::text(), background: theme::compare_added(), ..Default::default() });
                            } else {
                                job.append(&text[start..end], 0.0, egui::TextFormat { font_id: font.clone(), color: theme::text(), background: theme::highlight_all(), ..Default::default() });
                            }
                            job.append(&text[end..], 0.0, plain);
                            let galley = ui.painter().layout_job(job);
                            ui.painter().with_clip_rect(rect).galley(egui::pos2(rect.min.x + 18.0, rect.center().y - galley.size().y / 2.0), galley, theme::text());
                            let response = response.on_hover_text(crate::i18n::tf!("Line {}", hit.line + 1));
                            crate::access::button(&response, &format!("{}: {}", hit.line + 1, text.trim()));
                            if response.clicked() {
                                acts.push(Act::Open(hit.clone()));
                            }
                        }
                    }
                }
            });
        });
        for act in acts {
            match act {
                Act::Open(hit) => self.open_hit(&hit),
                Act::Toggle(path) => {
                    if !self.search.collapsed.remove(&path) {
                        self.search.collapsed.insert(path);
                    }
                }
                Act::Dismiss(path) => {
                    self.search.dismissed.insert(path);
                }
                Act::ReplaceIn(files, matches) => {
                    self.replace_plan = Some(ReplacePlan { options: self.search.options(), backup: self.find.keep_backups });
                    self.dialog = Dialog::ConfirmReplaceInFiles { files, matches };
                    self.search.again_after_replace = true;
                }
            }
        }
        if close {
            if let Some(job) = self.search.job.take() {
                job.cancel.store(true, Ordering::Relaxed);
            }
            self.search.open = false;
            self.search.asked = None;
            self.pending_focus = true;
        }
    }
}

#[cfg(test)]
#[path = "search_panel_tests.rs"]
mod tests;
