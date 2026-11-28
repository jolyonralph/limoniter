//! The window for colour tags of your own (View, Colour tag, Edit tags), the keys they can be given, and how they're
//! put to use: kept in the settings or with the open project, and turned into one another when one is deleted.

use super::*;
use crate::own_tags::{self, BUILT_IN, Def};

/// One line of the window: a tag.
#[derive(Clone, Debug)]
pub(super) struct Row {
    pub def: Def,
    /// Kept with the project rather than the settings.
    pub project: bool,
    /// Made in this window and not kept yet, so where it's kept can still be chosen.
    pub fresh: bool,
}

pub(super) struct Draft {
    pub rows: Vec<Row>,
    pub has_project: bool,
    /// The row being deleted, and what its places become: another tag's number, or None to take them away.
    pub deleting: Option<(usize, Option<u8>)>,
    /// Tags deleted here, and what their places become, for when OK is pressed.
    pub removed: Vec<(u8, Option<u8>)>,
    pub error: String,
    /// The row whose name is to be typed in first.
    pub focus: Option<usize>,
}

pub(super) enum Result {
    Open,
    Cancel,
    Ok,
}

impl Draft {
    /// The tags as they are now, for the window to change.
    pub fn new(has_project: bool, new_one: bool) -> Self {
        let user = own_tags::user();
        let mut rows: Vec<Row> = (0..BUILT_IN).map(|id| Row { def: user.iter().find(|d| d.id == id).cloned().unwrap_or(Def { id, name: String::new(), colour: None, key: String::new() }), project: false, fresh: false }).collect();
        rows.extend(own_tags::project().into_iter().map(|def| Row { def, project: true, fresh: false }));
        rows.extend(user.into_iter().filter(|d| d.id >= BUILT_IN).map(|def| Row { def, project: false, fresh: false }));
        let mut draft = Self { rows, has_project, deleting: None, removed: Vec::new(), error: String::new(), focus: None };
        if new_one {
            draft.add();
        }
        draft
    }

    /// A new tag at the end, in a colour that isn't one in use yet if there's one to spare.
    pub fn add(&mut self) {
        let Some(id) = self.free_id(false, usize::MAX) else {
            self.error = crate::i18n::tr("There's no room for another tag.").into();
            return;
        };
        const COLOURS: [[u8; 3]; 6] = [[0, 150, 136], [233, 30, 99], [121, 85, 72], [96, 125, 139], [205, 220, 57], [0, 188, 212]];
        let colour = COLOURS[self.rows.len() % COLOURS.len()];
        self.rows.push(Row { def: Def { id, name: String::new(), colour: Some(colour), key: String::new() }, project: false, fresh: true });
        self.focus = Some(self.rows.len() - 1);
    }

    /// The first number free for a tag kept in the settings or with the project, not counting the row `except`.
    fn free_id(&self, project: bool, except: usize) -> Option<u8> {
        let taken: Vec<Def> = self.rows.iter().enumerate().filter(|(at, r)| *at != except && r.project == project && r.def.id >= BUILT_IN).map(|(_, r)| r.def.clone()).collect();
        own_tags::free_id(&taken, project)
    }

    /// Moves a new tag to be kept in the settings or with the project, which changes its number.
    pub fn keep_in(&mut self, index: usize, project: bool) {
        if self.rows[index].project != project
            && let Some(id) = self.free_id(project, index)
        {
            self.rows[index].project = project;
            self.rows[index].def.id = id;
        }
    }

    /// What's wrong with the tags as they stand, in words, or nothing.
    pub fn problem(&self, taken: &[String]) -> Option<String> {
        let mut names: Vec<String> = Vec::new();
        let mut keys: Vec<String> = Vec::new();
        for row in &self.rows {
            let name = row.def.name.trim();
            if row.def.id >= BUILT_IN && name.is_empty() {
                return Some(crate::i18n::tr("Every tag of your own needs a name.").into());
            }
            let shown = if name.is_empty() { crate::i18n::tr(crate::theme::TAG_NAMES[row.def.id as usize]).to_string() } else { name.to_string() };
            if names.contains(&shown.to_lowercase()) {
                return Some(crate::i18n::tf!("Two tags are called \u{201C}{}\u{201D}. Each needs a name of its own.", shown));
            }
            names.push(shown.to_lowercase());
            let key = row.def.key.trim();
            if row.def.id >= BUILT_IN && !key.is_empty() {
                if own_tags::parse_key(key).is_none() {
                    return Some(crate::i18n::tf!("\u{201C}{}\u{201D} isn't a key Limoniter can use for {}. Write it like Ctrl+Alt+T: Ctrl or Alt, then a key.", key, shown));
                }
                let key = own_tags::normal_key(key);
                if taken.iter().any(|t| t.eq_ignore_ascii_case(&key)) || own_tags::BUILT_IN_KEYS.iter().any(|t| t.eq_ignore_ascii_case(&key)) {
                    return Some(crate::i18n::tf!("{} is already used for something else. Choose another key for {}.", key, shown));
                }
                if keys.contains(&key.to_lowercase()) {
                    return Some(crate::i18n::tf!("{} is given to two tags. Each needs a key of its own.", key));
                }
                keys.push(key.to_lowercase());
            }
        }
        None
    }
}

/// Every key the menus show, so a tag isn't given one that's taken.
pub(super) fn keys_taken() -> Vec<String> {
    fn walk(entries: &[menus::Entry], found: &mut Vec<String>) {
        for entry in entries {
            match entry {
                menus::Entry::Keyed(_, _, key) => found.push(key.to_string()),
                menus::Entry::Sub(_, inner) => walk(inner, found),
                _ => {}
            }
        }
    }
    let mut found = Vec::new();
    for (_, entries) in menus::MENUS {
        walk(entries, &mut found);
    }
    found
}

pub(super) fn window(ctx: &Context, draft: &mut Draft, counts: &dyn Fn(u8) -> usize, taken: &[String]) -> Result {
    let mut result = Result::Open;
    let mut cancelled = ctx.input(|i| i.key_pressed(Key::Escape));
    let mut delete = None;
    let mut swap = None;
    let mut add = false;
    crate::fit::Dialog::new(crate::i18n::tr("Edit tags")).collapsible(false).resizable(true).default_width(640.0).anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0]).show_with_buttons(ctx, |ui, part| match part {
        crate::fit::Part::Body => {
            ui.label(crate::i18n::tr("A tag marks some lines in a colour, and a name says why. The six that come with Limoniter can be renamed. Your own tags can have a key too, such as Ctrl+Alt+K."));
            ui.add_space(6.0);
            let count = draft.rows.len();
            egui::Grid::new("own-tags-grid").num_columns(6).spacing([8.0, 6.0]).show(ui, |ui| {
                for label in ["Colour", "Name", "Key", "Kept", "Order", ""] {
                    ui.label(RichText::new(crate::i18n::tr(label)).color(theme::text_faint()));
                }
                ui.end_row();
                let mut first_own = true;
                for index in 0..count {
                    let row = &mut draft.rows[index];
                    let built_in = row.def.id < BUILT_IN;
                    if !built_in && first_own {
                        first_own = false;
                    }
                    match &mut row.def.colour {
                        Some(colour) if !built_in => {
                            ui.color_edit_button_srgb(colour).on_hover_text(crate::i18n::tr("The colour of this tag"));
                        }
                        _ => {
                            ui.label(RichText::new(crate::i18n::tr("\u{25A0}")).color(own_tags::colour(row.def.id)).size(20.0));
                        }
                    }
                    let hint = if built_in { crate::i18n::tr(theme::TAG_NAMES[row.def.id as usize]).to_string() } else { crate::i18n::tr("Name").to_string() };
                    let response = ui.add_sized([170.0, ui.spacing().interact_size.y], egui::TextEdit::singleline(&mut row.def.name).hint_text(hint));
                    if draft.focus == Some(index) {
                        response.request_focus();
                        draft.focus = None;
                    }
                    let row = &mut draft.rows[index];
                    if built_in {
                        ui.label(RichText::new(own_tags::BUILT_IN_KEYS[row.def.id as usize]).color(theme::text_faint()));
                    } else {
                        ui.add_sized([130.0, ui.spacing().interact_size.y], egui::TextEdit::singleline(&mut row.def.key).hint_text(crate::i18n::tr("none")));
                    }
                    if built_in {
                        ui.label("");
                    } else if row.fresh && draft.has_project {
                        let mut project = row.project;
                        egui::ComboBox::from_id_salt(("own-tag-kept", index)).selected_text(if project { crate::i18n::tr("This project") } else { crate::i18n::tr("This computer") }).show_ui(ui, |ui| {
                            ui.selectable_value(&mut project, false, crate::i18n::tr("This computer"));
                            ui.selectable_value(&mut project, true, crate::i18n::tr("This project"));
                        });
                        if project != row.project {
                            draft.keep_in(index, project);
                        }
                    } else {
                        ui.label(RichText::new(if row.project { crate::i18n::tr("This project") } else { crate::i18n::tr("This computer") }).color(theme::text_faint()));
                    }
                    ui.horizontal(|ui| {
                        if !built_in {
                            if ui.add_enabled(index > BUILT_IN as usize, egui::Button::new("\u{25B2}")).on_hover_text(crate::i18n::tr("Move up")).clicked() {
                                swap = Some((index, index - 1));
                            }
                            if ui.add_enabled(index + 1 < count, egui::Button::new("\u{25BC}")).on_hover_text(crate::i18n::tr("Move down")).clicked() {
                                swap = Some((index, index + 1));
                            }
                        }
                    });
                    if !built_in && ui.button(crate::i18n::tr("Delete")).clicked() {
                        delete = Some(index);
                    }
                    ui.end_row();
                }
            });
            ui.add_space(4.0);
            if ui.button(crate::i18n::tr("New tag")).clicked() {
                add = true;
            }
            if let Some((index, to)) = draft.deleting {
                let id = draft.rows[index].def.id;
                let places = counts(id);
                let name = if draft.rows[index].def.name.trim().is_empty() { own_tags::name(id) } else { draft.rows[index].def.name.trim().to_string() };
                ui.add_space(8.0);
                ui.separator();
                ui.label(RichText::new(if places == 1 { crate::i18n::tf!("One place in your open files has the tag \u{201C}{}\u{201D}.", name) } else { crate::i18n::tf!("{} places in your open files have the tag \u{201C}{}\u{201D}.", places, name) }).strong());
                let mut to = to;
                ui.horizontal(|ui| {
                    ui.label(crate::i18n::tr("Turn them into"));
                    let shown = to.and_then(|id| draft.rows.iter().find(|r| r.def.id == id)).map_or_else(|| crate::i18n::tr("no tag: take them away").to_string(), |r| if r.def.name.trim().is_empty() { own_tags::name(r.def.id) } else { r.def.name.trim().to_string() });
                    egui::ComboBox::from_id_salt("own-tag-convert").selected_text(shown).show_ui(ui, |ui| {
                        ui.selectable_value(&mut to, None, crate::i18n::tr("no tag: take them away"));
                        for other in draft.rows.iter().filter(|r| r.def.id != id) {
                            let other_name = if other.def.name.trim().is_empty() { own_tags::name(other.def.id) } else { other.def.name.trim().to_string() };
                            ui.selectable_value(&mut to, Some(other.def.id), other_name);
                        }
                    });
                });
                draft.deleting = Some((index, to));
                ui.horizontal(|ui| {
                    if ui.button(crate::i18n::tr("Delete the tag")).clicked() {
                        draft.removed.push((id, to));
                        draft.rows.remove(index);
                        draft.deleting = None;
                    }
                    if ui.button(crate::i18n::tr("Keep it")).clicked() {
                        draft.deleting = None;
                    }
                });
            }
            if !draft.error.is_empty() {
                ui.add_space(4.0);
                ui.colored_label(theme::rainbow_mistake(), draft.error.clone());
            }
        }
        crate::fit::Part::Buttons => {
            ui.horizontal(|ui| {
                if ui.button(crate::i18n::tr("OK")).clicked() {
                    result = Result::Ok;
                }
                if ui.button(crate::i18n::tr("Cancel")).clicked() {
                    cancelled = true;
                }
            });
        }
    });
    if add {
        draft.add();
    }
    if let Some((a, b)) = swap {
        // Only your own tags move, and only past others kept in the same place.
        if b >= BUILT_IN as usize && draft.rows[a].project == draft.rows[b].project {
            draft.rows.swap(a, b);
        }
    }
    if let Some(index) = delete {
        if counts(draft.rows[index].def.id) == 0 {
            let id = draft.rows[index].def.id;
            draft.removed.push((id, None));
            draft.rows.remove(index);
        } else {
            draft.deleting = Some((index, None));
        }
    }
    if matches!(result, Result::Ok) {
        match draft.problem(taken) {
            Some(problem) => {
                draft.error = problem;
                result = Result::Open;
            }
            None => draft.error.clear(),
        }
    }
    if cancelled { Result::Cancel } else { result }
}

impl Limoniter {
    pub(super) fn open_tag_editor(&mut self, new_one: bool) {
        self.dialog = Dialog::TagEditor(Box::new(Draft::new(self.project.is_some(), new_one)));
    }

    /// Lets the project's tags (and so their names and colours) be those of the project that's open.
    pub(super) fn sync_project_tags(&mut self) {
        own_tags::set_project(self.project.as_ref().map_or(&[][..], |project| &project.prefs.tags));
    }

    /// For a picture: a few tags of your own, some text tagged with one, and the window open with a deletion asked about.
    pub(super) fn tag_editor_screenshot_step(&mut self) {
        let mine = |id, name: &str, colour, key: &str| Def { id, name: name.into(), colour: Some(colour), key: key.into() };
        own_tags::set(&[Def { id: 1, name: "Slow".into(), colour: None, key: String::new() }, mine(6, "To check", [0, 150, 136], "Ctrl+Alt+K"), mine(7, "Ask Sam", [233, 30, 99], "")], &[]);
        self.doc().text.tag_lines(3, 6, 6);
        self.doc().text.tag_lines(9, 12, 7);
        let mut draft = Draft::new(false, false);
        draft.deleting = Some((7, Some(2)));
        self.dialog = Dialog::TagEditor(Box::new(draft));
    }

    /// How many places in the open files have a tag.
    #[cfg(test)]
    pub(super) fn places_with_tag(&self, id: u8) -> usize {
        self.docs.iter().filter(|doc| !doc.placeholder).map(|doc| doc.text.tags_of(id)).sum()
    }

    /// Puts the edited tags in force: kept in the settings and with the project, and the places of any tag deleted
    /// turned into another tag or taken away.
    pub(super) fn apply_tag_draft(&mut self, draft: Draft) {
        for (id, to) in &draft.removed {
            for doc in self.docs.iter_mut().filter(|doc| !doc.placeholder) {
                doc.text.retag(*id, *to);
            }
        }
        let mut user = Vec::new();
        let mut project = Vec::new();
        for row in draft.rows {
            let mut def = row.def;
            def.name = def.name.trim().to_string();
            def.key = if def.key.trim().is_empty() { String::new() } else { own_tags::normal_key(&def.key) };
            if def.id < BUILT_IN {
                // One of the six is only kept if it's been renamed.
                def.colour = None;
                def.key.clear();
                if def.name.is_empty() {
                    continue;
                }
            }
            if row.project && self.project.is_some() { project.push(def) } else { user.push(def) }
        }
        // The six come first, as the settings file has always had them.
        user.sort_by_key(|def| def.id >= BUILT_IN);
        own_tags::set_user(&user);
        if let Some(project_now) = &mut self.project
            && project_now.prefs.tags != project
        {
            project_now.prefs.tags = project.clone();
            own_tags::set_project(&project);
            self.save_project();
        }
        self.pending_focus = true;
    }

    /// Tags whatever a key of a tag of your own is pressed for.
    pub(super) fn tag_keys(&mut self, ui: &Ui) {
        let ctx = ui.ctx().clone();
        for (id, key) in own_tags::keys() {
            if let Some((modifiers, key)) = own_tags::parse_key(&key)
                && ctx.input_mut(|i| i.consume_shortcut(&KeyboardShortcut::new(modifiers, key)))
            {
                self.run(Command::Tag(id), ui);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_draft_starts_with_the_six_then_the_projects_then_yours() {
        own_tags::set(&[Def { id: 6, name: "Slow".into(), colour: Some([1, 2, 3]), key: String::new() }], &[Def { id: 128, name: "Release".into(), colour: Some([4, 5, 6]), key: String::new() }]);
        let draft = Draft::new(true, true);
        let names: Vec<&str> = draft.rows.iter().map(|r| r.def.name.as_str()).collect();
        assert_eq!(names, ["", "", "", "", "", "", "Release", "Slow", ""]);
        // A new one is kept in the settings until it's moved to the project.
        assert!(!draft.rows.last().unwrap().project);
        assert_eq!(draft.rows.last().unwrap().def.id, 7);
        let mut draft = draft;
        let last = draft.rows.len() - 1;
        draft.keep_in(last, true);
        assert_eq!(draft.rows[last].def.id, 129);
        own_tags::set(&[], &[]);
    }

    #[test]
    fn names_and_keys_are_checked() {
        own_tags::set(&[], &[]);
        let mut draft = Draft::new(false, true);
        assert!(draft.problem(&[]).is_some(), "a tag with no name");
        draft.rows[6].def.name = "To check".into();
        assert!(draft.problem(&[]).is_none());
        draft.rows[6].def.name = "red".into();
        assert!(draft.problem(&[]).unwrap().contains("red"), "the same as one of the six, ignoring capitals");
        draft.rows[6].def.name = "To check".into();
        draft.rows[6].def.key = "T".into();
        assert!(draft.problem(&[]).is_some(), "a key with no Ctrl or Alt");
        draft.rows[6].def.key = "ctrl+alt+k".into();
        assert!(draft.problem(&[]).is_none());
        assert!(draft.problem(&["Ctrl+Alt+K".to_string()]).unwrap().contains("Ctrl+Alt+K"), "a key a menu has");
        draft.rows[6].def.key = "Ctrl+Shift+1".into();
        assert!(draft.problem(&[]).is_some(), "a key one of the six has");
    }

    #[test]
    fn menu_keys_are_found() {
        let taken = keys_taken();
        assert!(taken.iter().any(|k| k == "Ctrl+S") && taken.iter().any(|k| k == "F4"));
    }
}
