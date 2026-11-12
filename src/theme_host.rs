//! The window's side of themes: which window theme is used, which document theme each document gets (by rules
//! about its file ending, language, character set or view, or chosen for it alone), and the box where themes are
//! made, changed and given rules.

use super::*;
use crate::theme::{DocumentTheme, WindowTheme};
use eframe::egui::{Color32, Rect, Sense};
use std::rc::Rc;

/// What a rule looks at.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum When {
    /// File endings, such as "seq, prg".
    Ending,
    /// A language, by the name it has under Format, Colour as.
    Language,
    /// Character sets, by the start of their names, such as "petscii, screen-c64".
    CharacterSet,
    Hex,
    Table,
}

impl When {
    pub const ALL: [When; 5] = [When::Ending, When::Language, When::CharacterSet, When::Hex, When::Table];

    pub fn label(self) -> &'static str {
        match self {
            When::Ending => "File ending is",
            When::Language => "Language is",
            When::CharacterSet => "Character set is",
            When::Hex => "Shown in hex",
            When::Table => "Shown as a table",
        }
    }

    fn key(self) -> &'static str {
        match self {
            When::Ending => "ending",
            When::Language => "language",
            When::CharacterSet => "charset",
            When::Hex => "hex",
            When::Table => "table",
        }
    }

    fn from_key(key: &str) -> Option<When> {
        When::ALL.into_iter().find(|w| w.key() == key)
    }

    /// Whether it needs something written after it.
    pub fn takes_value(self) -> bool {
        matches!(self, When::Ending | When::Language | When::CharacterSet)
    }
}

/// A rule: documents like this get this theme. The first rule that fits wins.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct Rule {
    pub when: When,
    pub value: String,
    pub theme: String,
}

impl Rule {
    fn new(when: When, value: &str, theme: &str) -> Self {
        Rule { when, value: value.into(), theme: theme.into() }
    }

    /// Whether a document fits the rule.
    pub fn fits(&self, doc: &Document) -> bool {
        let items = || self.value.split(',').map(|v| v.trim().trim_start_matches('.').to_lowercase()).filter(|v| !v.is_empty());
        match self.when {
            When::Ending => {
                let ending = doc.path.as_deref().or(Some(Path::new(&doc.untitled))).and_then(Path::extension).and_then(|e| e.to_str()).map(str::to_lowercase).unwrap_or_default();
                !ending.is_empty() && items().any(|v| v == ending)
            }
            When::Language => {
                let language = doc.language_name().to_lowercase();
                items().any(|v| v == language)
            }
            When::CharacterSet => {
                let id = doc.text.format.charset.id.to_lowercase();
                items().any(|v| id.starts_with(&v))
            }
            When::Hex => doc.hex_mode,
            When::Table => doc.table.is_some() && !doc.hex_mode,
        }
    }
}

/// The rules a first run starts with: each older machine's text in that machine's colours.
pub(super) fn suggested_rules() -> Vec<Rule> {
    vec![
        Rule::new(When::CharacterSet, "petscii, screen-c64, screen-vic20, screen-pet", "Commodore 64"),
        Rule::new(When::CharacterSet, "atascii", "Atari 8-bit"),
        Rule::new(When::CharacterSet, "zx", "ZX Spectrum"),
        Rule::new(When::CharacterSet, "teletext, bbc-mode7", "BBC Micro and teletext"),
        Rule::new(When::CharacterSet, "amiga-text", "Amiga Workbench"),
    ]
}

/// Every theme there is, which are in use, and the rules.
pub(super) struct Themes {
    pub windows: Vec<Rc<WindowTheme>>,
    pub documents: Vec<Rc<DocumentTheme>>,
    /// The window theme, and the document theme for documents no rule picks, by name.
    pub window: String,
    pub document: String,
    pub rules: Vec<Rule>,
}

impl Themes {
    pub(super) fn from_settings(settings: &str, light: bool) -> Self {
        let mut windows: Vec<Rc<WindowTheme>> = theme::built_in_windows().into_iter().map(Rc::new).collect();
        let mut documents: Vec<Rc<DocumentTheme>> = theme::built_in_documents().into_iter().map(Rc::new).collect();
        for text in list_setting(settings, "custom_theme") {
            let text = text.replace('\u{1}', "\n");
            if let Ok(theme) = WindowTheme::from_text(&text) {
                if !windows.iter().any(|t| t.name == theme.name) {
                    windows.push(Rc::new(theme));
                }
            } else if let Ok(theme) = DocumentTheme::from_text(&text)
                && !documents.iter().any(|t| t.name == theme.name)
            {
                documents.push(Rc::new(theme));
            }
        }
        // Before there were themes there was light or dark, which carries over.
        let usual = if light { "Limoniter Light" } else { "Limoniter Dark" };
        let window = list_setting(settings, "window_theme").into_iter().next().filter(|n| windows.iter().any(|t| &t.name == n)).unwrap_or_else(|| usual.into());
        let document = list_setting(settings, "document_theme").into_iter().next().filter(|n| documents.iter().any(|t| &t.name == n)).unwrap_or_else(|| usual.into());
        let saved: Vec<Rule> = list_setting(settings, "theme_rule")
            .iter()
            .filter_map(|line| {
                let mut parts = line.split('\u{1}');
                Some(Rule { when: When::from_key(parts.next()?)?, value: parts.next()?.to_string(), theme: parts.next()?.to_string() })
            })
            .collect();
        // Rules that have been looked at are kept as they are, even none at all.
        let rules = if settings.lines().any(|l| l.starts_with("theme_rules_set=")) { saved } else { suggested_rules() };
        Themes { windows, documents, window, document, rules }
    }

    pub(super) fn to_settings(&self) -> String {
        let mut out = format!("window_theme={}\ndocument_theme={}\ntheme_rules_set=yes\n", self.window, self.document);
        for rule in &self.rules {
            out += &format!("theme_rule={}\u{1}{}\u{1}{}\n", rule.when.key(), rule.value.replace(['\n', '\u{1}'], " "), rule.theme);
        }
        for theme in self.windows.iter().filter(|t| !self.is_built_in_window(&t.name)) {
            out += &format!("custom_theme={}\n", theme.to_text().trim_end().replace('\n', "\u{1}"));
        }
        for theme in self.documents.iter().filter(|t| !self.is_built_in_document(&t.name)) {
            out += &format!("custom_theme={}\n", theme.to_text().trim_end().replace('\n', "\u{1}"));
        }
        out
    }

    pub(super) fn is_built_in_window(&self, name: &str) -> bool {
        theme::built_in_windows().iter().any(|t| t.name == name)
    }

    pub(super) fn is_built_in_document(&self, name: &str) -> bool {
        theme::built_in_documents().iter().any(|t| t.name == name)
    }

    pub(super) fn window_theme(&self) -> Rc<WindowTheme> {
        self.windows.iter().find(|t| t.name == self.window).cloned().unwrap_or_else(|| self.windows[0].clone())
    }

    pub(super) fn document_named(&self, name: &str) -> Option<Rc<DocumentTheme>> {
        self.documents.iter().find(|t| t.name == name).cloned()
    }

    /// The theme a document is drawn in: its own if it was given one, then the first rule that fits, then the usual.
    pub(super) fn document_for(&self, doc: &Document) -> Rc<DocumentTheme> {
        doc.theme_override
            .as_deref()
            .and_then(|name| self.document_named(name))
            .or_else(|| self.rules.iter().find(|rule| rule.fits(doc)).and_then(|rule| self.document_named(&rule.theme)))
            .or_else(|| self.document_named(&self.document))
            .unwrap_or_else(|| self.documents[0].clone())
    }

    /// A name for a copy that no other theme has.
    fn free_name(taken: impl Fn(&str) -> bool, base: &str) -> String {
        let base = base.trim_end_matches(" (copy)").trim();
        let mut name = format!("{base} (copy)");
        let mut n = 2;
        while taken(&name) {
            name = format!("{base} (copy {n})");
            n += 1;
        }
        name
    }
}

/// Which page of the themes box is showing.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub(super) enum ThemePage {
    #[default]
    Documents,
    Window,
    Rules,
}

/// The themes box.
#[derive(Default)]
pub(super) struct ThemeBox {
    pub page: ThemePage,
    /// The theme being looked at on each page, by name.
    pub document: String,
    pub window: String,
    /// Three colours to make a theme from, and a problem to show.
    pub seeds: Option<(Color32, Color32, Color32)>,
    pub problem: String,
}

impl Limoniter {
    pub(super) fn open_theme_box(&mut self, page: ThemePage) {
        let document = self.themes.document_for(&self.docs[self.active]).name.clone();
        self.dialog = Dialog::Themes(Box::new(ThemeBox { page, document, window: self.themes.window.clone(), ..ThemeBox::default() }));
    }

    /// Sets the window and document themes for this frame: the window's when it changes, and the document's for the
    /// document in front.
    pub(super) fn apply_themes(&mut self, ctx: &Context) {
        let window = self.themes.window_theme();
        if self.applied_window.as_ref() != Some(&window) {
            theme::set_window(window.clone());
            theme::apply(ctx);
            self.prefs.light = window.light;
            self.applied_window = Some(window);
        }
        theme::set_document(self.themes.document_for(&self.docs[self.active]));
    }

    /// The themes box: document themes, window themes, and the rules that choose a document's theme.
    pub(super) fn theme_dialog(&mut self, ctx: &Context) -> bool {
        let Dialog::Themes(state) = &mut self.dialog else {
            return false;
        };
        let mut close = ctx.input(|i| i.key_pressed(Key::Escape));
        let mut changed = false;
        let themes = &mut self.themes;
        let in_front = themes.document_for(&self.docs[self.active]).name.clone();
        let most = (ctx.content_rect().height() - 80.0).max(300.0);
        egui::Window::new("Themes").collapsible(false).resizable(true).default_width(860.0).max_height(most).anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0]).show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.selectable_value(&mut state.page, ThemePage::Documents, "Document themes");
                ui.selectable_value(&mut state.page, ThemePage::Window, "Window themes");
                ui.selectable_value(&mut state.page, ThemePage::Rules, "Which documents get which theme");
            });
            ui.separator();
            egui::ScrollArea::vertical().id_salt("theme-page").max_height(most - 140.0).auto_shrink([false, true]).show(ui, |ui| match state.page {
                ThemePage::Documents => changed |= document_page(ui, themes, state, &in_front),
                ThemePage::Window => changed |= window_page(ui, themes, state),
                ThemePage::Rules => changed |= rules_page(ui, themes),
            });
            ui.separator();
            ui.horizontal(|ui| {
                if ui.button("Close").clicked() {
                    close = true;
                }
                if !state.problem.is_empty() {
                    ui.colored_label(theme::rainbow_mistake(), &state.problem);
                }
            });
        });
        if changed {
            self.save_settings_if_changed();
        }
        if close {
            self.close_dialog();
        }
        true
    }
}

/// The list of themes on the left of a page, with which is chosen.
fn theme_list(ui: &mut Ui, names: &[(String, bool)], chosen: &mut String, in_use: &[&str]) {
    egui::ScrollArea::vertical().id_salt("theme-list").max_height(420.0).auto_shrink([false, true]).show(ui, |ui| {
        for (name, built_in) in names {
            let mut label = RichText::new(name.as_str());
            if in_use.contains(&name.as_str()) {
                label = label.strong();
            }
            let response = ui.selectable_label(chosen == name, label);
            let response = if *built_in { response } else { response.on_hover_text("Your own theme") };
            if response.clicked() {
                chosen.clone_from(name);
            }
        }
    });
}

/// A colour button for each colour of a theme, in its groups. True if any changed.
fn colour_grid(ui: &mut Ui, colours: &[(&str, &str, &str)], mut get: impl FnMut(&str) -> Color32, mut set: impl FnMut(&str, Color32), editable: bool) -> bool {
    let mut changed = false;
    let mut groups: Vec<&str> = Vec::new();
    for (_, _, group) in colours {
        if !groups.contains(group) {
            groups.push(group);
        }
    }
    for group in groups {
        egui::CollapsingHeader::new(group).default_open(group == "Text" || group == "Window" || group == "Words").show(ui, |ui| {
            egui::Grid::new(("theme-colours", group)).num_columns(2).spacing([12.0, 4.0]).show(ui, |ui| {
                for (key, label, g) in colours {
                    if *g != group {
                        continue;
                    }
                    let mut colour = get(key);
                    ui.add_enabled_ui(editable, |ui| {
                        if egui::color_picker::color_edit_button_srgba(ui, &mut colour, egui::color_picker::Alpha::OnlyBlend).changed() {
                            set(key, colour);
                            changed = true;
                        }
                    });
                    ui.label(*label);
                    ui.end_row();
                }
            });
        });
    }
    changed
}

/// A few lines drawn in a document theme, to see it without opening a file.
fn document_sample(ui: &mut Ui, t: &DocumentTheme) {
    let font = egui::FontId::monospace(13.0);
    let row = 19.0;
    let lines: [(&str, &str); 6] = [("1", "10 REM A SAMPLE"), ("2", "20 PRINT \"HELLO\";"), ("3", "30 FOR I=1 TO 10"), ("4", "40 PRINT I*2"), ("5", "50 NEXT I"), ("6", "60 GOTO 20")];
    let (rect, _) = ui.allocate_exact_size(egui::vec2(ui.available_width().min(420.0), row * lines.len() as f32 + 12.0), Sense::hover());
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, 6.0, t.editor_back);
    let gutter = 34.0;
    for (i, (number, text)) in lines.iter().enumerate() {
        let y = rect.min.y + 6.0 + i as f32 * row;
        let line_rect = Rect::from_min_size(egui::pos2(rect.min.x + gutter, y), egui::vec2(rect.width() - gutter, row));
        if i == 2 {
            painter.rect_filled(line_rect, 0.0, t.active_line);
        }
        painter.text(egui::pos2(rect.min.x + gutter - 8.0, y + row / 2.0), egui::Align2::RIGHT_CENTER, number, font.clone(), if i == 2 { t.editor_text } else { t.gutter_text });
        let left = rect.min.x + gutter + 6.0;
        let char_width = ui.fonts_mut(|f| f.glyph_width(&font, '0'));
        if i == 1 {
            // "HELLO" selected.
            let start = left + 9.0 * char_width;
            painter.rect_filled(Rect::from_min_size(egui::pos2(start, y + 1.0), egui::vec2(7.0 * char_width, row - 2.0)), 2.0, t.selection);
        }
        painter.text(egui::pos2(left, y + row / 2.0), egui::Align2::LEFT_CENTER, *text, font.clone(), t.editor_text);
        if i == 2 {
            let x = left + 16.0 * char_width;
            painter.vline(x, (y + 2.0)..=(y + row - 2.0), egui::Stroke::new(2.0, t.caret));
        }
        if i == 4 {
            // A word marked as a mistake.
            let start = left + 3.0 * char_width;
            painter.hline(start..=(start + 4.0 * char_width), y + row - 3.0, egui::Stroke::new(1.0, t.mistake));
        }
    }
    painter.vline(rect.min.x + gutter, rect.y_range(), egui::Stroke::new(1.0, t.strong_line));
}

fn document_page(ui: &mut Ui, themes: &mut Themes, state: &mut ThemeBox, in_front: &str) -> bool {
    let mut changed = false;
    let names: Vec<(String, bool)> = themes.documents.iter().map(|t| (t.name.clone(), themes.is_built_in_document(&t.name))).collect();
    if !names.iter().any(|(n, _)| *n == state.document) {
        state.document = themes.document.clone();
    }
    ui.horizontal_top(|ui| {
        ui.vertical(|ui| {
            ui.set_width(200.0);
            let usual = themes.document.clone();
            theme_list(ui, &names, &mut state.document, &[usual.as_str(), in_front]);
            ui.label(RichText::new("Bold: the usual theme, and the one the file in front is shown in.").color(theme::text_faint()).small());
        });
        ui.separator();
        ui.vertical(|ui| {
            let Some(index) = themes.documents.iter().position(|t| t.name == state.document) else {
                return;
            };
            let built_in = themes.is_built_in_document(&state.document);
            let current = themes.documents[index].clone();
            ui.horizontal(|ui| {
                if ui.add_enabled(themes.document != current.name, egui::Button::new("Use for documents")).on_hover_text("The theme for documents no rule picks another for").clicked() {
                    themes.document.clone_from(&current.name);
                    changed = true;
                }
                if ui.button("Make a copy to change").clicked() {
                    let mut copy = (*current).clone();
                    copy.name = Themes::free_name(|n| themes.documents.iter().any(|t| t.name == n), &current.name);
                    state.document.clone_from(&copy.name);
                    themes.documents.push(Rc::new(copy));
                    changed = true;
                }
                if !built_in && ui.button("Delete").clicked() {
                    themes.documents.remove(index);
                    if themes.document == current.name {
                        themes.document = "Limoniter Dark".into();
                    }
                    changed = true;
                }
                if ui.button("Save to a file\u{2026}").clicked()
                    && let Some(path) = rfd::FileDialog::new().set_title("Save the theme").set_file_name(format!("{}.limoniter-theme", current.name)).save_file()
                    && let Err(e) = std::fs::write(&path, current.to_text())
                {
                    state.problem = format!("Couldn't save it: {e}");
                }
                if ui.button("Open a theme file\u{2026}").clicked()
                    && let Some(path) = rfd::FileDialog::new().set_title("Open a theme").add_filter("Limoniter theme", &["limoniter-theme"]).pick_file()
                {
                    match std::fs::read_to_string(&path).map_err(|e| e.to_string()).and_then(|text| DocumentTheme::from_text(&text)) {
                        Ok(mut theme) => {
                            if themes.documents.iter().any(|t| t.name == theme.name) {
                                theme.name = Themes::free_name(|n| themes.documents.iter().any(|t| t.name == n), &theme.name);
                            }
                            state.document.clone_from(&theme.name);
                            themes.documents.push(Rc::new(theme));
                            state.problem.clear();
                            changed = true;
                        }
                        Err(problem) => state.problem = problem,
                    }
                }
            });
            if built_in {
                ui.label(RichText::new("Themes that come with Limoniter can't be changed. Make a copy to change it.").color(theme::text_faint()));
            }
            let Some(index) = themes.documents.iter().position(|t| t.name == state.document) else {
                return;
            };
            let theme = Rc::make_mut(&mut themes.documents[index]);
            let old_name = theme.name.clone();
            ui.add_enabled_ui(!built_in, |ui| {
                ui.horizontal(|ui| {
                    ui.label("Name:");
                    let mut name = theme.name.clone();
                    if ui.add(egui::TextEdit::singleline(&mut name).desired_width(200.0)).lost_focus() && !name.trim().is_empty() && name.trim() != old_name {
                        theme.name = name.trim().to_string();
                    }
                    changed |= ui.checkbox(&mut theme.light, "Light").on_hover_text("Whether it's a light theme or a dark one").changed();
                    ui.label("Syntax colours:");
                    egui::ComboBox::from_id_salt("theme-syntax").selected_text(theme.syntax.as_str()).width(180.0).show_ui(ui, |ui| {
                        for scheme in crate::highlight::SCHEMES {
                            if ui.selectable_label(theme.syntax == scheme.as_name(), scheme.as_name()).clicked() {
                                theme.syntax = scheme.as_name().to_string();
                                changed = true;
                            }
                        }
                    });
                });
                ui.horizontal(|ui| {
                    let (mut back, mut text, mut accent) = state.seeds.unwrap_or((theme.editor_back, theme.editor_text, theme.caret));
                    ui.label("Start again from three colours:");
                    egui::color_picker::color_edit_button_srgba(ui, &mut back, egui::color_picker::Alpha::Opaque).on_hover_text("Background");
                    egui::color_picker::color_edit_button_srgba(ui, &mut text, egui::color_picker::Alpha::Opaque).on_hover_text("Text");
                    egui::color_picker::color_edit_button_srgba(ui, &mut accent, egui::color_picker::Alpha::Opaque).on_hover_text("Accent, for the cursor and selection");
                    state.seeds = Some((back, text, accent));
                    if ui.button("Make the rest from these").clicked() {
                        let made = theme::document_from(&theme.name, theme.light, back, text, accent, &theme.syntax);
                        *theme = made;
                        changed = true;
                    }
                });
            });
            // A renamed theme keeps its place as the usual one and in the rules.
            if theme.name != old_name {
                let new_name = theme.name.clone();
                if themes.documents.iter().filter(|t| t.name == new_name).count() > 1 {
                    Rc::make_mut(&mut themes.documents[index]).name.clone_from(&old_name);
                    state.problem = format!("There's already a theme called {new_name}.");
                } else {
                    if themes.document == old_name {
                        themes.document.clone_from(&new_name);
                    }
                    for rule in &mut themes.rules {
                        if rule.theme == old_name {
                            rule.theme.clone_from(&new_name);
                        }
                    }
                    state.document.clone_from(&new_name);
                    changed = true;
                }
            }
            let Some(index) = themes.documents.iter().position(|t| t.name == state.document) else {
                return;
            };
            ui.add_space(4.0);
            document_sample(ui, &themes.documents[index]);
            ui.add_space(4.0);
            {
                let theme = Rc::make_mut(&mut themes.documents[index]);
                let snapshot = theme.clone();
                changed |= colour_grid(
                    ui,
                    DocumentTheme::COLOURS,
                    |key| snapshot.colour(key).unwrap_or_default(),
                    |key, colour| {
                        if let Some(slot) = theme.colour_mut(key) {
                            *slot = colour;
                        }
                    },
                    !built_in,
                );
            }
        });
    });
    changed
}

fn window_page(ui: &mut Ui, themes: &mut Themes, state: &mut ThemeBox) -> bool {
    let mut changed = false;
    let names: Vec<(String, bool)> = themes.windows.iter().map(|t| (t.name.clone(), themes.is_built_in_window(&t.name))).collect();
    if !names.iter().any(|(n, _)| *n == state.window) {
        state.window = themes.window.clone();
    }
    ui.horizontal_top(|ui| {
        ui.vertical(|ui| {
            ui.set_width(200.0);
            let using = themes.window.clone();
            theme_list(ui, &names, &mut state.window, &[using.as_str()]);
            ui.label(RichText::new("Bold: the theme in use.").color(theme::text_faint()).small());
        });
        ui.separator();
        ui.vertical(|ui| {
            let Some(index) = themes.windows.iter().position(|t| t.name == state.window) else {
                return;
            };
            let built_in = themes.is_built_in_window(&state.window);
            let current = themes.windows[index].clone();
            ui.horizontal(|ui| {
                if ui.add_enabled(themes.window != current.name, egui::Button::new("Use for the window")).clicked() {
                    themes.window.clone_from(&current.name);
                    changed = true;
                }
                if ui.button("Make a copy to change").clicked() {
                    let mut copy = (*current).clone();
                    copy.name = Themes::free_name(|n| themes.windows.iter().any(|t| t.name == n), &current.name);
                    state.window.clone_from(&copy.name);
                    themes.windows.push(Rc::new(copy));
                    changed = true;
                }
                if !built_in && ui.button("Delete").clicked() {
                    themes.windows.remove(index);
                    if themes.window == current.name {
                        themes.window = "Limoniter Dark".into();
                    }
                    changed = true;
                }
                if ui.button("Save to a file\u{2026}").clicked()
                    && let Some(path) = rfd::FileDialog::new().set_title("Save the theme").set_file_name(format!("{}.limoniter-theme", current.name)).save_file()
                    && let Err(e) = std::fs::write(&path, current.to_text())
                {
                    state.problem = format!("Couldn't save it: {e}");
                }
                if ui.button("Open a theme file\u{2026}").clicked()
                    && let Some(path) = rfd::FileDialog::new().set_title("Open a theme").add_filter("Limoniter theme", &["limoniter-theme"]).pick_file()
                {
                    match std::fs::read_to_string(&path).map_err(|e| e.to_string()).and_then(|text| WindowTheme::from_text(&text)) {
                        Ok(mut theme) => {
                            if themes.windows.iter().any(|t| t.name == theme.name) {
                                theme.name = Themes::free_name(|n| themes.windows.iter().any(|t| t.name == n), &theme.name);
                            }
                            state.window.clone_from(&theme.name);
                            themes.windows.push(Rc::new(theme));
                            state.problem.clear();
                            changed = true;
                        }
                        Err(problem) => state.problem = problem,
                    }
                }
            });
            if built_in {
                ui.label(RichText::new("Themes that come with Limoniter can't be changed. Make a copy to change it.").color(theme::text_faint()));
            } else {
                ui.label(RichText::new("Changes to the theme in use show at once.").color(theme::text_faint()));
            }
            let Some(index) = themes.windows.iter().position(|t| t.name == state.window) else {
                return;
            };
            let theme = Rc::make_mut(&mut themes.windows[index]);
            let old_name = theme.name.clone();
            ui.add_enabled_ui(!built_in, |ui| {
                ui.horizontal(|ui| {
                    ui.label("Name:");
                    let mut name = theme.name.clone();
                    if ui.add(egui::TextEdit::singleline(&mut name).desired_width(200.0)).lost_focus() && !name.trim().is_empty() && name.trim() != old_name {
                        theme.name = name.trim().to_string();
                    }
                    changed |= ui.checkbox(&mut theme.light, "Light").changed();
                });
                ui.horizontal(|ui| {
                    let (mut back, mut text, mut accent) = state.seeds.unwrap_or((theme.window, theme.text, theme.accent));
                    ui.label("Start again from three colours:");
                    egui::color_picker::color_edit_button_srgba(ui, &mut back, egui::color_picker::Alpha::Opaque).on_hover_text("Window");
                    egui::color_picker::color_edit_button_srgba(ui, &mut text, egui::color_picker::Alpha::Opaque).on_hover_text("Text");
                    egui::color_picker::color_edit_button_srgba(ui, &mut accent, egui::color_picker::Alpha::Opaque).on_hover_text("Accent");
                    state.seeds = Some((back, text, accent));
                    if ui.button("Make the rest from these").clicked() {
                        *theme = theme::window_from(&theme.name, theme.light, back, text, accent);
                        changed = true;
                    }
                });
            });
            if theme.name != old_name {
                let new_name = theme.name.clone();
                if themes.windows.iter().filter(|t| t.name == new_name).count() > 1 {
                    Rc::make_mut(&mut themes.windows[index]).name.clone_from(&old_name);
                    state.problem = format!("There's already a theme called {new_name}.");
                } else {
                    if themes.window == old_name {
                        themes.window.clone_from(&new_name);
                    }
                    state.window.clone_from(&new_name);
                    changed = true;
                }
            }
            let Some(index) = themes.windows.iter().position(|t| t.name == state.window) else {
                return;
            };
            {
                let theme = Rc::make_mut(&mut themes.windows[index]);
                let snapshot = theme.clone();
                changed |= colour_grid(
                    ui,
                    WindowTheme::COLOURS,
                    |key| snapshot.colour(key).unwrap_or_default(),
                    |key, colour| {
                        if let Some(slot) = theme.colour_mut(key) {
                            *slot = colour;
                        }
                    },
                    !built_in,
                );
            }
        });
    });
    changed
}

fn rules_page(ui: &mut Ui, themes: &mut Themes) -> bool {
    let mut changed = false;
    let names: Vec<String> = themes.documents.iter().map(|t| t.name.clone()).collect();
    ui.horizontal(|ui| {
        ui.label("Documents no rule fits use:");
        let mut usual = themes.document.clone();
        egui::ComboBox::from_id_salt("usual-document-theme").selected_text(usual.as_str()).width(220.0).show_ui(ui, |ui| {
            for name in &names {
                ui.selectable_value(&mut usual, name.clone(), name.as_str());
            }
        });
        if usual != themes.document {
            themes.document = usual;
            changed = true;
        }
    });
    ui.label(RichText::new("The first rule that fits a document chooses its theme. Endings, languages and character sets can be listed with commas; a character set matches by the start of its name, so \u{201C}teletext\u{201D} matches every teletext set.").color(theme::text_faint()));
    ui.add_space(4.0);
    let mut remove = None;
    let mut swap = None;
    let count = themes.rules.len();
    {
        egui::Grid::new("theme-rules-grid").num_columns(4).spacing([8.0, 6.0]).show(ui, |ui| {
            for (n, rule) in themes.rules.iter_mut().enumerate() {
                egui::ComboBox::from_id_salt(("rule-when", n)).selected_text(rule.when.label()).width(150.0).show_ui(ui, |ui| {
                    for when in When::ALL {
                        if ui.selectable_label(rule.when == when, when.label()).clicked() {
                            rule.when = when;
                            changed = true;
                        }
                    }
                });
                if rule.when.takes_value() {
                    let hint = match rule.when {
                        When::Ending => "seq, prg",
                        When::Language => "COBOL",
                        _ => "petscii, atascii",
                    };
                    changed |= ui.add_sized([240.0, 22.0], egui::TextEdit::singleline(&mut rule.value).hint_text(hint)).changed();
                } else {
                    ui.label("");
                }
                egui::ComboBox::from_id_salt(("rule-theme", n)).selected_text(rule.theme.as_str()).width(200.0).show_ui(ui, |ui| {
                    for name in &names {
                        if ui.selectable_label(rule.theme == *name, name.as_str()).clicked() {
                            rule.theme.clone_from(name);
                            changed = true;
                        }
                    }
                });
                ui.horizontal(|ui| {
                    if ui.add_enabled(n > 0, egui::Button::new("\u{2191}").small()).on_hover_text("Ask this rule first").clicked() {
                        swap = Some(n - 1);
                    }
                    if ui.add_enabled(n + 1 < count, egui::Button::new("\u{2193}").small()).on_hover_text("Ask this rule later").clicked() {
                        swap = Some(n);
                    }
                    if ui.small_button("Remove").clicked() {
                        remove = Some(n);
                    }
                });
                ui.end_row();
            }
        });
    }
    if let Some(n) = swap {
        themes.rules.swap(n, n + 1);
        changed = true;
    }
    if let Some(n) = remove {
        themes.rules.remove(n);
        changed = true;
    }
    ui.horizontal(|ui| {
        if ui.button("Add a rule").clicked() {
            themes.rules.push(Rule::new(When::Ending, "", &themes.document.clone()));
            changed = true;
        }
        if ui.button("Put back the suggested rules").on_hover_text("Commodore, Atari, Spectrum, BBC and Amiga text each in its machine's colours").clicked() {
            for rule in suggested_rules() {
                if !themes.rules.contains(&rule) {
                    themes.rules.push(rule);
                }
            }
            changed = true;
        }
    });
    changed
}

#[cfg(test)]
#[path = "theme_host_tests.rs"]
mod tests;
