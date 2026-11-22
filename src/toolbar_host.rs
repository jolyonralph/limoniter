//! A toolbar under the menus: a row of buttons for commands the person chooses, each with the picture the ribbon
//! gives that command, or its name where the ribbon has none. Right-click it, or use Settings, Appearance, Toolbar
//! buttons, to choose the commands from the full list and put them in order.
//!
//! A button is kept by its place in the menus, in English (File > New), so it survives a change of language and a
//! command that moves between releases simply goes missing rather than turning into another one.

use super::*;
use crate::ribbon_icons::{self, Icon};
use std::sync::OnceLock;

/// The buttons a toolbar starts with.
const USUAL: [Command; 9] = [Command::New, Command::Open, Command::Save, Command::Undo, Command::Redo, Command::Find, Command::Replace, Command::RunOnMachine, Command::BuildTask];

/// Every command as the menus list it, in English, for as long as the program runs.
fn catalogue() -> &'static [menus::Found] {
    static ALL: OnceLock<Vec<menus::Found>> = OnceLock::new();
    ALL.get_or_init(menus::every_command_in_english)
}

/// The key a command is kept under: its place in the menus, in English.
fn key_of(command: Command) -> Option<String> {
    catalogue().iter().find(|found| found.command == command).map(|found| found.path.clone())
}

fn usual_keys() -> Vec<String> {
    USUAL.iter().filter_map(|command| key_of(*command)).collect()
}

/// The picture the ribbon gives a command, if it has one.
fn icon_of(command: Command) -> Option<Icon> {
    static ICONS: OnceLock<Vec<(Command, Icon)>> = OnceLock::new();
    ICONS
        .get_or_init(|| {
            let mut all = Vec::new();
            for tab in ribbon_bar::Tab::FIXED {
                for group in tab.groups() {
                    for control in group.controls {
                        match control.what {
                            ribbon_bar::What::Run(command) => all.push((command, control.icon)),
                            ribbon_bar::What::Tick(flag) => all.push((Command::Toggle(flag), control.icon)),
                            ribbon_bar::What::Drop(_) => {}
                        }
                    }
                }
            }
            all
        })
        .iter()
        .find(|(each, _)| *each == command)
        .map(|(_, icon)| *icon)
}

/// The words after the last ">" of a place in the menus: the command's own name.
fn own_name(path: &str) -> &str {
    path.rsplit('\u{203A}').next().unwrap_or(path).trim()
}

/// Whether the toolbar is showing, and its buttons, by their keys.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct Toolbar {
    pub on: bool,
    pub keys: Vec<String>,
}

impl Default for Toolbar {
    fn default() -> Self {
        Self { on: false, keys: usual_keys() }
    }
}

/// One button, ready to draw.
struct Button {
    command: Command,
    name: String,
    shortcut: &'static str,
    icon: Option<Icon>,
}

impl Toolbar {
    pub fn from_settings(settings: &str) -> Self {
        let mut bar = Self::default();
        for (key, value) in settings.lines().filter_map(|line| line.split_once('=')) {
            if key.trim() == "toolbar" {
                bar.on = value.trim() == "true";
            }
        }
        // Any button line, even an empty one, means the person chose the buttons.
        let chosen: Vec<&str> = settings.lines().filter_map(|line| line.strip_prefix("toolbar_button=")).collect();
        if !chosen.is_empty() {
            bar.keys = chosen.into_iter().map(str::trim).filter(|key| !key.is_empty()).map(str::to_string).collect();
        }
        bar
    }

    pub fn to_settings(&self) -> String {
        let mut text = format!("toolbar={}\n", self.on);
        if self.keys != usual_keys() {
            if self.keys.is_empty() {
                text.push_str("toolbar_button=\n");
            }
            for key in &self.keys {
                text.push_str(&format!("toolbar_button={key}\n"));
            }
        }
        text
    }

    /// The buttons that can still be found: a key for a command that's gone is passed over.
    fn buttons(&self) -> Vec<Button> {
        self.keys
            .iter()
            .filter_map(|key| catalogue().iter().find(|found| &found.path == key))
            .map(|found| Button { command: found.command, name: own_name(&found.path).to_string(), shortcut: found.shortcut, icon: icon_of(found.command) })
            .collect()
    }
}

/// Moves the button at `at` one place up or down. Returns where it is now.
fn shift(keys: &mut [String], at: usize, up: bool) -> usize {
    let to = if up { at.saturating_sub(1) } else { (at + 1).min(keys.len().saturating_sub(1)) };
    if at < keys.len() && to != at {
        keys.swap(at, to);
        return to;
    }
    at
}

/// The box that chooses the buttons.
pub(super) struct ToolbarBox {
    keys: Vec<String>,
    /// Every command: its key, and its place in the menus as it reads in the language chosen.
    all: Vec<(String, String)>,
    filter: String,
    picked: Option<usize>,
}

impl Limoniter {
    pub(super) fn open_toolbar_box(&mut self) {
        let shown = menus::every_command();
        let all = catalogue().iter().zip(shown).map(|(english, shown)| (english.path.clone(), shown.path)).collect();
        self.dialog = Dialog::Toolbar(Box::new(ToolbarBox { keys: self.toolbar.keys.clone(), all, filter: String::new(), picked: None }));
    }

    /// The row of buttons, under the menus. Returns the command chosen.
    pub(super) fn toolbar_bar(&mut self, ui: &mut Ui) -> Option<Command> {
        if !self.toolbar.on {
            return None;
        }
        let buttons = self.toolbar.buttons();
        let mut chosen = None;
        let bar = Panel::top("toolbar").show_separator_line(true).frame(egui::Frame::new().fill(theme::window()).inner_margin(egui::Margin { left: 8, right: 8, top: 3, bottom: 3 })).show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 2.0;
                if buttons.is_empty() {
                    ui.label(RichText::new(crate::i18n::tr("The toolbar has no buttons. Right-click it to choose some.")).color(theme::text_faint()).size(12.5));
                }
                for button in &buttons {
                    let enabled = self.can(button.command);
                    let ticked = if let Command::Toggle(flag) = button.command { Some(self.flag_on(flag)) } else { None };
                    let response = button_widget(ui, button, enabled, ticked);
                    if enabled && response.clicked() {
                        chosen = Some(button.command);
                    }
                    crate::fit::context_menu(&response, |ui| bar_menu(ui, &mut chosen));
                }
            });
        });
        // A right-click between the buttons has the same menu.
        crate::fit::context_menu(&bar.response.interact(egui::Sense::click()), |ui| bar_menu(ui, &mut chosen));
        chosen
    }

    pub(super) fn toolbar_dialog(&mut self, ctx: &Context) -> bool {
        let Dialog::Toolbar(state) = &mut self.dialog else {
            return false;
        };
        let (mut accept, mut cancel, mut usual) = (false, ctx.input(|i| i.key_pressed(Key::Escape)), false);
        crate::fit::Dialog::new(crate::i18n::tr("Toolbar buttons")).collapsible(false).resizable(true).default_size([640.0, 440.0]).anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0]).show(ctx, |ui| {
            ui.label(crate::i18n::tr("Click a command to give it a button. Buttons have the ribbon's picture for the command, or its name where the ribbon has none."));
            ui.add_space(4.0);
            let room = (ui.available_height() - 40.0).max(120.0);
            ui.columns(2, |columns| {
                columns[0].label(RichText::new(crate::i18n::tr("Commands")).strong());
                columns[0].add(egui::TextEdit::singleline(&mut state.filter).hint_text(crate::i18n::tr("Type to find a command")).desired_width(f32::INFINITY));
                let words: Vec<String> = state.filter.split_whitespace().map(str::to_lowercase).collect();
                egui::ScrollArea::vertical().id_salt("toolbar-all").max_height(room - 40.0).auto_shrink([false, false]).show(&mut columns[0], |ui| {
                    for (key, shown) in &state.all {
                        let lower = shown.to_lowercase();
                        if !words.iter().all(|word| lower.contains(word.as_str())) {
                            continue;
                        }
                        let there = state.keys.contains(key);
                        if ui.add_enabled(!there, egui::Button::new(shown).frame(false).wrap_mode(egui::TextWrapMode::Truncate).min_size(egui::vec2(ui.available_width(), 0.0))).clicked() {
                            state.keys.push(key.clone());
                            state.picked = Some(state.keys.len() - 1);
                        }
                    }
                });
                columns[1].label(RichText::new(crate::i18n::tr("Buttons, in order")).strong());
                let shown_of = |key: &str| state.all.iter().find(|(each, _)| each == key).map_or_else(|| key.to_string(), |(_, shown)| own_name(shown).to_string());
                egui::ScrollArea::vertical().id_salt("toolbar-chosen").max_height(room - 40.0).auto_shrink([false, false]).show(&mut columns[1], |ui| {
                    for (at, key) in state.keys.iter().enumerate() {
                        if ui.add(egui::Button::selectable(state.picked == Some(at), shown_of(key)).frame_when_inactive(false).wrap_mode(egui::TextWrapMode::Truncate).min_size(egui::vec2(ui.available_width(), 0.0))).clicked() {
                            state.picked = Some(at);
                        }
                    }
                });
                columns[1].horizontal(|ui| {
                    let at = state.picked.filter(|at| *at < state.keys.len());
                    if ui.add_enabled(at.is_some_and(|at| at > 0), egui::Button::new(crate::i18n::tr("Move up"))).clicked() {
                        state.picked = at.map(|at| shift(&mut state.keys, at, true));
                    }
                    if ui.add_enabled(at.is_some_and(|at| at + 1 < state.keys.len()), egui::Button::new(crate::i18n::tr("Move down"))).clicked() {
                        state.picked = at.map(|at| shift(&mut state.keys, at, false));
                    }
                    if ui.add_enabled(at.is_some(), egui::Button::new(crate::i18n::tr("Remove"))).clicked()
                        && let Some(at) = at
                    {
                        state.keys.remove(at);
                        state.picked = None;
                    }
                });
            });
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                accept = ui.button(crate::i18n::tr("OK")).clicked();
                cancel |= ui.button(crate::i18n::tr("Cancel")).clicked();
                usual = ui.button(crate::i18n::tr("Back to the usual buttons")).clicked();
            });
        });
        if usual {
            state.keys = usual_keys();
            state.picked = None;
        } else if accept {
            self.toolbar.keys = std::mem::take(&mut state.keys);
            self.close_dialog();
        } else if cancel {
            self.close_dialog();
        }
        true
    }
}

/// What a right-click on the toolbar offers.
fn bar_menu(ui: &mut Ui, chosen: &mut Option<Command>) {
    if ui.button(crate::i18n::tr("Toolbar buttons\u{2026}")).clicked() {
        *chosen = Some(Command::ToolbarButtons);
        ui.close();
    }
    if ui.button(crate::i18n::tr("Hide the toolbar")).clicked() {
        *chosen = Some(Command::Toggle(menus::Flag::Toolbar));
        ui.close();
    }
}

/// One button: a picture where there is one, else the name.
fn button_widget(ui: &mut Ui, button: &Button, enabled: bool, ticked: Option<bool>) -> egui::Response {
    let name = crate::i18n::tr(&button.name);
    let font = egui::FontId::proportional(13.0);
    let size = match button.icon {
        Some(_) => egui::vec2(30.0, 26.0),
        None => egui::vec2(ui.painter().layout_no_wrap(name.to_string(), font.clone(), theme::text()).size().x + 16.0, 26.0),
    };
    let (rect, response) = ui.allocate_exact_size(size, if enabled { egui::Sense::click() } else { egui::Sense::hover() });
    let painter = ui.painter();
    if ticked == Some(true) {
        painter.rect(rect, 4.0, theme::selected(), egui::Stroke::new(1.0, theme::accent().gamma_multiply(0.6)), egui::StrokeKind::Inside);
    } else if enabled && response.hovered() {
        painter.rect_filled(rect, 4.0, theme::hover());
    }
    if response.has_focus() {
        painter.rect_stroke(rect, 4.0, egui::Stroke::new(1.5, theme::accent()), egui::StrokeKind::Inside);
    }
    let ink = if enabled { theme::text() } else { theme::text_disabled() };
    let accent = if enabled { theme::accent() } else { theme::text_disabled() };
    match button.icon {
        Some(icon) => ribbon_icons::paint(painter, egui::Rect::from_center_size(rect.center(), egui::vec2(16.0, 16.0)), icon, ink, accent, theme::window()),
        None => {
            painter.text(rect.center(), egui::Align2::CENTER_CENTER, name.to_string(), font, ink);
        }
    }
    let tip = if button.shortcut.is_empty() { name.to_string() } else { format!("{name} ({})", menus::shown_shortcut(button.shortcut)) };
    crate::access::button(&response, &tip);
    response.on_hover_text(tip)
}

#[cfg(test)]
mod tests {
    use super::super::tests::{app_with, command, draw};
    use super::*;

    #[test]
    fn the_usual_buttons_are_found_in_the_menus() {
        assert_eq!(usual_keys().len(), USUAL.len(), "every usual command has a place in the menus");
        let bar = Toolbar::default();
        assert!(!bar.on);
        let commands: Vec<Command> = bar.buttons().iter().map(|button| button.command).collect();
        assert!(commands == USUAL);
        // The first ones have the ribbon's pictures.
        assert!(bar.buttons().iter().take(5).all(|button| button.icon.is_some()));
    }

    #[test]
    fn the_buttons_chosen_are_kept_and_read_back() {
        let bar = Toolbar { on: true, keys: vec![key_of(Command::Save).unwrap(), key_of(Command::New).unwrap()] };
        let again = Toolbar::from_settings(&bar.to_settings());
        assert_eq!(again, bar);
        // The usual buttons aren't written out, so a later release's usual set reaches whoever never changed them.
        let usual = Toolbar { on: true, keys: usual_keys() };
        assert_eq!(usual.to_settings(), "toolbar=true\n");
        assert_eq!(Toolbar::from_settings("toolbar=true\n"), usual);
        // Taking every button away is a choice too.
        let none = Toolbar { on: false, keys: Vec::new() };
        assert_eq!(Toolbar::from_settings(&none.to_settings()), none);
    }

    #[test]
    fn a_button_for_a_command_that_has_gone_is_passed_over() {
        let bar = Toolbar { on: true, keys: vec!["File \u{203A} Nothing like it".into(), key_of(Command::Find).unwrap()] };
        let buttons = bar.buttons();
        assert_eq!(buttons.len(), 1);
        assert!(buttons[0].command == Command::Find);
    }

    #[test]
    fn buttons_move_up_and_down_and_stop_at_the_ends() {
        let mut keys: Vec<String> = ["a", "b", "c"].iter().map(|k| k.to_string()).collect();
        assert_eq!(shift(&mut keys, 2, true), 1);
        assert_eq!(keys, ["a", "c", "b"]);
        assert_eq!(shift(&mut keys, 0, true), 0);
        assert_eq!(shift(&mut keys, 2, false), 2);
        assert_eq!(shift(&mut keys, 0, false), 1);
        assert_eq!(keys, ["c", "a", "b"]);
    }

    #[test]
    fn names_are_the_last_part_of_the_place() {
        assert_eq!(own_name("File \u{203A} New"), "New");
        assert_eq!(own_name("Tools \u{203A} Tasks \u{203A} Build"), "Build");
        assert_eq!(own_name("Plain"), "Plain");
    }

    #[test]
    fn the_toolbar_draws_runs_a_button_and_is_hidden_from_its_menu() {
        let (mut app, ctx) = app_with("");
        draw(&mut app, &ctx, Vec::new());
        command(&mut app, &ctx, Command::Toggle(menus::Flag::Toolbar));
        assert!(app.toolbar.on);
        draw(&mut app, &ctx, Vec::new());
        // The box opens, takes a button away and gives another, and keeps the result on OK.
        command(&mut app, &ctx, Command::ToolbarButtons);
        assert!(matches!(app.dialog, Dialog::Toolbar(_)));
        draw(&mut app, &ctx, Vec::new());
        if let Dialog::Toolbar(state) = &mut app.dialog {
            assert_eq!(state.keys, usual_keys());
            state.keys.remove(0);
            state.keys.push(key_of(Command::Print).unwrap());
        }
        app.toolbar.keys = match &app.dialog {
            Dialog::Toolbar(state) => state.keys.clone(),
            _ => unreachable!(),
        };
        assert!(app.toolbar.buttons().last().is_some_and(|button| button.command == Command::Print));
        command(&mut app, &ctx, Command::Toggle(menus::Flag::Toolbar));
        assert!(!app.toolbar.on);
    }

    #[test]
    fn the_toolbar_is_kept_in_the_settings_the_app_writes() {
        let (mut app, _ctx) = app_with("");
        app.toolbar.on = true;
        app.toolbar.keys = vec![key_of(Command::Undo).unwrap()];
        app.side_tabs = side_tabs::SideTabs { on: true, right: true };
        let text = app.current_settings();
        assert_eq!(Toolbar::from_settings(&text), app.toolbar);
        assert_eq!(side_tabs::SideTabs::from_settings(&text), app.side_tabs);
    }
}
