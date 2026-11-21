//! Colouring files, to tell apart several of the same kind at a glance: a tint chosen for a file (a stripe on its
//! tab, a faint tint behind its text, a mark in Go to file and the Explorer), or a whole document theme of its own;
//! and rules that colour every file in a folder, or every file whose name fits a pattern.
//!
//! A project keeps the colours of its files by their places in it, so they stay when the folder is moved or shared;
//! a file outside any project has its colour kept in Limoniter's settings, by its whole path. The little that's
//! drawn here (a swatch, the list of tints to choose from) is shared by the tabs, the Explorer and the project's
//! settings.

use crate::theme::{self, DocumentTheme};
use eframe::egui::{self, Color32, Rect, Sense, Ui, vec2};
use serde_json::{Map, Value, json};
use std::collections::BTreeMap;

/// The tints a file can be given: few, and far enough apart to tell from one another at a glance.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Tint {
    Blue,
    Green,
    Amber,
    Red,
    Rose,
    Violet,
}

impl Tint {
    pub const ALL: [Tint; 6] = [Tint::Blue, Tint::Green, Tint::Amber, Tint::Red, Tint::Rose, Tint::Violet];

    /// Its name as it's shown, to be translated.
    pub fn label(self) -> &'static str {
        match self {
            Tint::Blue => "Blue",
            Tint::Green => "Green",
            Tint::Amber => "Amber",
            Tint::Red => "Red",
            Tint::Rose => "Rose",
            Tint::Violet => "Violet",
        }
    }

    /// As it's written in a project or the settings, which never changes with the language.
    pub fn id(self) -> &'static str {
        match self {
            Tint::Blue => "blue",
            Tint::Green => "green",
            Tint::Amber => "amber",
            Tint::Red => "red",
            Tint::Rose => "rose",
            Tint::Violet => "violet",
        }
    }

    pub fn from_id(id: &str) -> Option<Tint> {
        let id = id.trim().to_lowercase();
        Tint::ALL.into_iter().find(|tint| tint.id() == id)
    }

    /// The colour itself, at full strength: a middling one of each, which reads against light and dark alike.
    pub fn colour(self) -> Color32 {
        match self {
            Tint::Blue => Color32::from_rgb(58, 128, 240),
            Tint::Green => Color32::from_rgb(52, 176, 92),
            Tint::Amber => Color32::from_rgb(232, 158, 24),
            Tint::Red => Color32::from_rgb(222, 58, 52),
            Tint::Rose => Color32::from_rgb(228, 88, 168),
            Tint::Violet => Color32::from_rgb(146, 92, 232),
        }
    }

    /// The colour for a stripe or a mark drawn on the window: a little deeper on a light window, so it stands out.
    pub fn mark(self) -> Color32 {
        if theme::is_light() { theme::mix(self.colour(), Color32::BLACK, 0.12) } else { self.colour() }
    }
}

/// How much of the tint goes into a theme's background, at most: enough to be seen, not enough to get in the way.
const DARK_STRENGTH: f32 = 0.13;
const LIGHT_STRENGTH: f32 = 0.10;
/// The share of a theme's own contrast between its text and its background that a tint must leave.
pub const KEEP_CONTRAST: f32 = 0.9;

/// How bright a colour looks, from 0 (black) to 1 (white), as the Web's guidelines on contrast work it out.
fn luminance(colour: Color32) -> f32 {
    let channel = |c: u8| {
        let c = f32::from(c) / 255.0;
        if c <= 0.039_28 { c / 12.92 } else { ((c + 0.055) / 1.055).powf(2.4) }
    };
    0.2126 * channel(colour.r()) + 0.7152 * channel(colour.g()) + 0.0722 * channel(colour.b())
}

/// How far apart two colours are to read, from 1 (the same) to 21 (black on white). Text wants at least 4.5.
pub fn contrast(a: Color32, b: Color32) -> f32 {
    let (a, b) = (luminance(a), luminance(b));
    (a.max(b) + 0.05) / (a.min(b) + 0.05)
}

/// How strongly a background can be tinted and still leave its text nearly as easy to read as it was.
fn strength(back: Color32, text: Color32, hue: Color32, most: f32) -> f32 {
    let before = contrast(back, text);
    let mut share = most;
    while share > 0.0 && contrast(theme::mix(back, hue, share), text) < before * KEEP_CONTRAST {
        share -= 0.01;
    }
    share.max(0.0)
}

/// A document theme with a tint behind its text: only the backgrounds change (the text's, the margins', the
/// cursor's line), each made from the theme's own, so it stays a light theme or a dark one and the syntax colours
/// are just as they were. A see-through colour is left as it is, as whatever's under it carries the tint already.
pub fn tinted(theme: &DocumentTheme, tint: Tint) -> DocumentTheme {
    let hue = tint.colour();
    let share = strength(theme.editor_back, theme.editor_text, hue, if theme.light { LIGHT_STRENGTH } else { DARK_STRENGTH });
    let mut out = theme.clone();
    for (back, text) in [(&mut out.editor_back, theme.editor_text), (&mut out.gutter_back, theme.gutter_text), (&mut out.ruler_back, theme.ruler_text), (&mut out.map_back, theme.map_text), (&mut out.active_line, theme.editor_text)] {
        if back.a() == 255 {
            *back = theme::mix(*back, hue, share.min(strength(*back, text, hue, share)));
        }
    }
    out
}

/// What a file looks like: its tint, and a theme of its own, each if it has one.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Look {
    pub tint: Option<Tint>,
    pub theme: Option<String>,
}

impl Look {
    pub fn is_empty(&self) -> bool {
        self.tint.is_none() && self.theme.is_none()
    }
}

/// A rule: every file that fits the pattern gets the tint.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Rule {
    /// A folder, written with a / at its end ("admin/"); a place in the project ("config/production.php"); or a
    /// name, which fits a file of that name in any folder ("*.inc"). * stands for any letters but a /, ** for any
    /// at all, and ? for one letter.
    pub pattern: String,
    pub tint: Tint,
}

/// Whether a name fits a pattern with *, ** and ?, in either case.
fn glob(pattern: &[u8], name: &[u8]) -> bool {
    match pattern.split_first() {
        None => name.is_empty(),
        Some((b'*', rest)) if rest.first() == Some(&b'*') => {
            let rest = &rest[1..];
            // "**/" can stand for no folders at all, so **/admin/ fits admin at the top too.
            (rest.first() == Some(&b'/') && glob(&rest[1..], name)) || (0..=name.len()).any(|at| glob(rest, &name[at..]))
        }
        Some((b'*', rest)) => {
            let reach = name.iter().position(|&c| c == b'/').unwrap_or(name.len());
            (0..=reach).any(|at| glob(rest, &name[at..]))
        }
        Some((b'?', rest)) => name.first().is_some_and(|&c| c != b'/') && glob(rest, &name[1..]),
        Some((&p, rest)) => name.first().is_some_and(|c| c.eq_ignore_ascii_case(&p)) && glob(rest, &name[1..]),
    }
}

/// A pattern as it's kept: with / between folders, and nothing before its first folder.
pub fn tidy_pattern(pattern: &str) -> String {
    let pattern = pattern.trim().replace('\\', "/");
    let pattern = pattern.trim_start_matches("./").trim_start_matches('/');
    pattern.to_string()
}

impl Rule {
    pub fn new(pattern: &str, tint: Tint) -> Self {
        Rule { pattern: tidy_pattern(pattern), tint }
    }

    /// Whether the rule colours a file (or, for a folder rule, a folder), by its place in the project written with
    /// /. A file outside the project's folder is never coloured by a rule.
    pub fn fits(&self, place: &str, is_folder: bool) -> bool {
        let pattern = tidy_pattern(&self.pattern);
        if pattern.is_empty() || place.starts_with("../") || place.starts_with('/') || place.contains(':') {
            return false;
        }
        if let Some(folder) = pattern.strip_suffix('/') {
            // Any of the folders the file is in, from the top down; a folder itself, too.
            let parts: Vec<&str> = place.split('/').collect();
            let last = if is_folder { parts.len() } else { parts.len() - 1 };
            return (1..=last).any(|count| glob(folder.as_bytes(), parts[..count].join("/").as_bytes()));
        }
        if is_folder {
            return false;
        }
        if pattern.contains('/') {
            glob(pattern.as_bytes(), place.as_bytes())
        } else {
            glob(pattern.as_bytes(), place.rsplit('/').next().unwrap_or(place).as_bytes())
        }
    }
}

/// The colours of a set of files: each file's own, by its place (in a project) or its whole path (in the
/// settings), and the rules.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Colours {
    pub files: BTreeMap<String, Look>,
    pub rules: Vec<Rule>,
}

/// How a path is written as a key here: with / between folders, whatever the computer.
pub fn key_of(path: &std::path::Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}

impl Colours {
    pub fn is_empty(&self) -> bool {
        self.files.is_empty() && self.rules.is_empty()
    }

    /// What a file looks like: its own tint, or else the first rule's that fits it; and its own theme.
    pub fn look(&self, key: &str) -> Look {
        let mut look = self.files.get(key).cloned().unwrap_or_default();
        if look.tint.is_none() {
            look.tint = self.rules.iter().find(|rule| rule.fits(key, false)).map(|rule| rule.tint);
        }
        look
    }

    /// The tint of a folder's own row: the first folder rule that fits it.
    pub fn folder_tint(&self, key: &str) -> Option<Tint> {
        self.rules.iter().find(|rule| rule.pattern.ends_with('/') && rule.fits(key, true)).map(|rule| rule.tint)
    }

    fn change(&mut self, key: &str, change: impl FnOnce(&mut Look)) {
        let look = self.files.entry(key.to_string()).or_default();
        change(look);
        if look.is_empty() {
            self.files.remove(key);
        }
    }

    pub fn set_tint(&mut self, key: &str, tint: Option<Tint>) {
        self.change(key, |look| look.tint = tint);
    }

    pub fn set_theme(&mut self, key: &str, theme: Option<String>) {
        self.change(key, |look| look.theme = theme);
    }

    /// Colours a folder's files: a rule for the folder, in place of any it had; or with None, takes its rule away.
    pub fn colour_folder(&mut self, folder: &str, tint: Option<Tint>) {
        let pattern = format!("{}/", tidy_pattern(folder).trim_end_matches('/'));
        self.rules.retain(|rule| tidy_pattern(&rule.pattern) != pattern);
        if let Some(tint) = tint {
            // First, so it wins over a broader rule set before.
            self.rules.insert(0, Rule::new(&pattern, tint));
        }
    }

    /// The tint a folder's own rule gives, if it has one.
    pub fn folder_rule(&self, folder: &str) -> Option<Tint> {
        let pattern = format!("{}/", tidy_pattern(folder).trim_end_matches('/'));
        self.rules.iter().find(|rule| tidy_pattern(&rule.pattern) == pattern).map(|rule| rule.tint)
    }

    /// A file or folder renamed or moved: its colours, and those of everything in it, go with it.
    pub fn renamed(&mut self, from: &str, to: &str) {
        let moved: Vec<String> = self.files.keys().filter(|key| *key == from || key.strip_prefix(from).is_some_and(|rest| rest.starts_with('/'))).cloned().collect();
        for key in moved {
            if let Some(look) = self.files.remove(&key) {
                self.files.insert(format!("{to}{}", &key[from.len()..]), look);
            }
        }
    }

    /// As a project file keeps it: each file's tint and theme by its place, and the rules in order.
    pub fn to_json(&self) -> Value {
        let files: Map<String, Value> = self
            .files
            .iter()
            .map(|(key, look)| {
                let mut one = Map::new();
                if let Some(tint) = look.tint {
                    one.insert("tint".into(), json!(tint.id()));
                }
                if let Some(theme) = &look.theme {
                    one.insert("theme".into(), json!(theme));
                }
                (key.clone(), Value::Object(one))
            })
            .collect();
        let mut value = Map::new();
        if !files.is_empty() {
            value.insert("files".into(), Value::Object(files));
        }
        if !self.rules.is_empty() {
            value.insert("rules".into(), self.rules.iter().map(|rule| json!({ "pattern": rule.pattern, "tint": rule.tint.id() })).collect());
        }
        Value::Object(value)
    }

    /// Reads what [`to_json`](Self::to_json) wrote, leaving out anything it can't make sense of.
    pub fn from_json(value: Option<&Value>) -> Self {
        let Some(value) = value else { return Colours::default() };
        let files = value.get("files").and_then(Value::as_object).map_or_else(BTreeMap::new, |files| {
            files
                .iter()
                .filter_map(|(key, one)| {
                    let look = Look { tint: one.get("tint").and_then(Value::as_str).and_then(Tint::from_id), theme: one.get("theme").and_then(Value::as_str).filter(|t| !t.is_empty()).map(str::to_string) };
                    (!look.is_empty()).then(|| (key.clone(), look))
                })
                .collect()
        });
        let rules = value.get("rules").and_then(Value::as_array).map_or_else(Vec::new, |rules| {
            rules.iter().filter_map(|rule| Some(Rule::new(rule.get("pattern")?.as_str()?, Tint::from_id(rule.get("tint")?.as_str()?)?))).filter(|rule| !rule.pattern.is_empty()).collect()
        });
        Colours { files, rules }
    }

    /// As Limoniter's settings keep the colours of files outside any project: one line a file, its whole path,
    /// tint and theme apart by a character no name has.
    pub fn to_settings(&self) -> String {
        self.files
            .iter()
            .map(|(key, look)| format!("file_colour={}\u{1}{}\u{1}{}\n", key.replace(['\n', '\u{1}'], " "), look.tint.map_or("", Tint::id), look.theme.as_deref().unwrap_or_default().replace(['\n', '\u{1}'], " ")))
            .collect()
    }

    pub fn from_settings(lines: &[String]) -> Self {
        let mut colours = Colours::default();
        for line in lines {
            let mut parts = line.split('\u{1}');
            let (Some(key), tint, theme) = (parts.next().filter(|k| !k.is_empty()), parts.next().and_then(Tint::from_id), parts.next().filter(|t| !t.is_empty()).map(str::to_string)) else { continue };
            let look = Look { tint, theme };
            if !look.is_empty() {
                colours.files.insert(key.to_string(), look);
            }
        }
        colours
    }
}

/// A small square of a tint, as files are marked with (a dot is for files edited lately).
pub fn swatch(ui: &Ui, rect: Rect, tint: Tint) {
    ui.painter().rect_filled(rect, 2.0, tint.mark());
}

/// The tints to choose from in a menu, each with its swatch, and None to take the colour away. Gives what's chosen:
/// Some(None) for none.
pub fn tint_menu(ui: &mut Ui, current: Option<Tint>) -> Option<Option<Tint>> {
    let mut chosen = None;
    for tint in std::iter::once(None).chain(Tint::ALL.map(Some)) {
        ui.horizontal(|ui| {
            let (rect, _) = ui.allocate_exact_size(vec2(12.0, 12.0), Sense::hover());
            match tint {
                Some(tint) => swatch(ui, rect, tint),
                None => {
                    ui.painter().rect_stroke(rect.shrink(0.5), 2.0, egui::Stroke::new(1.0, theme::text_faint()), egui::StrokeKind::Inside);
                }
            }
            if ui.radio(current == tint, crate::i18n::tr(tint.map_or("None", Tint::label))).clicked() {
                chosen = Some(tint);
                ui.close();
            }
        });
    }
    chosen
}

/// A row of swatches to pick one tint from, as in the rules of the project's settings. True when it changed.
pub fn tint_row(ui: &mut Ui, tint: &mut Tint) -> bool {
    let mut changed = false;
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 3.0;
        for choice in Tint::ALL {
            let (rect, response) = ui.allocate_exact_size(vec2(16.0, 16.0), Sense::click());
            swatch(ui, rect.shrink(2.0), choice);
            if *tint == choice {
                ui.painter().rect_stroke(rect, 3.0, egui::Stroke::new(1.5, theme::text()), egui::StrokeKind::Inside);
            }
            let name = crate::i18n::tr(choice.label());
            crate::access::chosen(&response, egui::Role::RadioButton, *tint == choice, name);
            if response.on_hover_text(name).clicked() && *tint != choice {
                *tint = choice;
                changed = true;
            }
        }
    });
    changed
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rules_colour_by_folder_by_place_and_by_name() {
        let admin = Rule::new("admin/", Tint::Rose);
        assert!(admin.fits("admin/index.php", false) && admin.fits("admin/users/edit.php", false));
        assert!(!admin.fits("administrator.php", false) && !admin.fits("site/admin.php", false), "a folder rule is only for what's in that folder");
        assert!(admin.fits("admin", true), "the folder's own row is coloured too");
        let nested = Rule::new("**/admin/", Tint::Rose);
        assert!(nested.fits("site/admin/index.php", false) && nested.fits("admin/index.php", false));
        let production = Rule::new("config/production.php", Tint::Red);
        assert!(production.fits("config/production.php", false));
        assert!(!production.fits("config/staging.php", false) && !production.fits("old/config/production.php", false));
        let includes = Rule::new("*.INC", Tint::Amber);
        assert!(includes.fits("lib/db.inc", false) && includes.fits("x.inc", false) && !includes.fits("lib/db.php", false), "a name alone fits in any folder, either case");
        let one_level = Rule::new("src/*.php", Tint::Blue);
        assert!(one_level.fits("src/a.php", false) && !one_level.fits("src/deep/a.php", false), "* doesn't go into folders");
        assert!(Rule::new("src/**.php", Tint::Blue).fits("src/deep/a.php", false));
        assert!(Rule::new("v?.php", Tint::Blue).fits("v2.php", false));
        assert!(!Rule::new("*.php", Tint::Blue).fits("../shared/a.php", false), "outside the project, no rule colours a file");
        assert!(Rule::new(".\\admin\\", Tint::Rose).fits("admin/a.php", false), "written the Windows way, it's tidied");
    }

    #[test]
    fn a_file_s_own_colour_comes_before_the_rules_and_the_first_rule_wins() {
        let mut colours = Colours { rules: vec![Rule::new("admin/", Tint::Rose), Rule::new("*.php", Tint::Green)], ..Colours::default() };
        assert_eq!(colours.look("admin/a.php").tint, Some(Tint::Rose));
        assert_eq!(colours.look("index.php").tint, Some(Tint::Green));
        assert_eq!(colours.look("notes.txt"), Look::default());
        colours.set_tint("admin/a.php", Some(Tint::Blue));
        assert_eq!(colours.look("admin/a.php").tint, Some(Tint::Blue));
        colours.set_theme("admin/a.php", Some("Dracula".into()));
        colours.set_tint("admin/a.php", None);
        assert_eq!(colours.look("admin/a.php"), Look { tint: Some(Tint::Rose), theme: Some("Dracula".into()) }, "the rule's tint, its own theme");
        colours.set_theme("admin/a.php", None);
        assert!(colours.files.is_empty(), "nothing left, nothing kept");
        colours.colour_folder("lib", Some(Tint::Violet));
        colours.colour_folder("lib/", Some(Tint::Amber));
        assert_eq!(colours.rules.iter().filter(|r| r.pattern == "lib/").count(), 1, "a folder has one rule of its own");
        assert_eq!((colours.folder_rule("lib"), colours.folder_tint("lib"), colours.look("lib/x.c").tint), (Some(Tint::Amber), Some(Tint::Amber), Some(Tint::Amber)));
        colours.colour_folder("lib", None);
        assert_eq!(colours.folder_rule("lib"), None);
    }

    #[test]
    fn colours_go_with_a_renamed_file_or_folder() {
        let mut colours = Colours::default();
        colours.set_tint("src/a.php", Some(Tint::Blue));
        colours.set_tint("src/b/c.php", Some(Tint::Red));
        colours.set_tint("srcs.php", Some(Tint::Green));
        colours.renamed("src", "code");
        let keys: Vec<&str> = colours.files.keys().map(String::as_str).collect();
        assert_eq!(keys, ["code/a.php", "code/b/c.php", "srcs.php"]);
    }

    #[test]
    fn colours_are_kept_as_json_and_in_the_settings() {
        let mut colours = Colours { rules: vec![Rule::new("admin/", Tint::Rose), Rule::new("config/production.php", Tint::Red)], ..Colours::default() };
        colours.set_tint("src/a.php", Some(Tint::Blue));
        colours.set_theme("src/b.php", Some("Nord".into()));
        assert_eq!(Colours::from_json(Some(&colours.to_json())), colours);
        assert_eq!(Colours::from_json(None), Colours::default());
        let broken = json!({ "files": { "x": { "tint": "mauve" }, "y": "blue" }, "rules": [{ "pattern": "a/", "tint": "nothing" }, { "pattern": "", "tint": "blue" }] });
        assert!(Colours::from_json(Some(&broken)).is_empty(), "what can't be read is left out");
        let mut lone = Colours::default();
        lone.set_tint("/home/me/notes.php", Some(Tint::Amber));
        lone.set_theme("C:/work/a b.php", Some("Monokai".into()));
        let lines: Vec<String> = lone.to_settings().lines().map(|l| l.trim_start_matches("file_colour=").to_string()).collect();
        assert_eq!(Colours::from_settings(&lines), lone);
    }

    #[test]
    fn a_tint_leaves_the_text_readable_in_light_and_dark_themes() {
        let mut themes = theme::built_in_documents();
        themes.push(theme::limoniter_light_document());
        themes.push(theme::limoniter_dark_document());
        for theme in &themes {
            let before = contrast(theme.editor_back, theme.editor_text);
            for tint in Tint::ALL {
                let after = tinted(theme, tint);
                let now = contrast(after.editor_back, after.editor_text);
                assert!(now >= before * KEEP_CONTRAST - 0.01, "{} in {}: contrast {now:.2}, was {before:.2}", tint.id(), theme.name);
                assert!(now >= 4.5 || before < 5.0, "{} in {}: contrast {now:.2} is too little to read", tint.id(), theme.name);
                assert_eq!(after.light, theme.light, "a light theme stays light");
                assert_eq!(after.syntax, theme.syntax, "the syntax colours are the theme's own");
                // Only the backgrounds change.
                let backgrounds = ["editor_back", "gutter_back", "ruler_back", "map_back", "active_line"];
                for (key, _, _) in DocumentTheme::COLOURS {
                    if !backgrounds.contains(key) {
                        assert_eq!(after.colour(key), theme.colour(key), "{key} in {} with {}", theme.name, tint.id());
                    }
                }
            }
        }
        // In Limoniter's own themes the tint is there to see, and each one differs from the others.
        for theme in [theme::limoniter_light_document(), theme::limoniter_dark_document()] {
            let backs: Vec<Color32> = Tint::ALL.iter().map(|&tint| tinted(&theme, tint).editor_back).collect();
            for (at, back) in backs.iter().enumerate() {
                let apart = |a: Color32, b: Color32| [a.r().abs_diff(b.r()), a.g().abs_diff(b.g()), a.b().abs_diff(b.b())].into_iter().max().unwrap_or(0);
                assert!(apart(*back, theme.editor_back) >= 6, "{} hardly shows in {}", Tint::ALL[at].id(), theme.name);
                assert!(backs.iter().enumerate().all(|(other, b)| other == at || apart(*back, *b) >= 3), "{} looks like another tint in {}", Tint::ALL[at].id(), theme.name);
                assert!(contrast(*back, theme.editor_text) >= 4.5);
            }
        }
    }

    #[test]
    fn contrast_is_worked_out_as_the_guidelines_say() {
        assert!((contrast(Color32::BLACK, Color32::WHITE) - 21.0).abs() < 0.01);
        assert!((contrast(Color32::WHITE, Color32::WHITE) - 1.0).abs() < 0.001);
        assert!((contrast(Color32::from_rgb(118, 118, 118), Color32::WHITE) - 4.54).abs() < 0.02);
    }
}
