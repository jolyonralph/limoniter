//! The Settings window, and the choices it adds that have no other home. Every entry here really changes
//! something; nothing is listed for show.

use crate::associations;
use crate::reformat::{Align, ParagraphSetup};
use crate::text_buffer::{EditPrefs, MAX_TAB_SIZE, MIN_TAB_SIZE};
use crate::theme;
use crate::views::ViewOptions;
use eframe::egui::{self, Button, Context, RichText, Ui, vec2};
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

/// The folder that settings, recovery copies, character sets and the rest are kept in: in %APPDATA% on Windows,
/// Application Support on macOS, and the configuration folder (~/.config) on Linux.
pub fn data_folder() -> Option<PathBuf> {
    if cfg!(windows) {
        return Some(data_folder_in(&PathBuf::from(std::env::var_os("APPDATA")?)));
    }
    let home = std::env::var_os("HOME").map(PathBuf::from);
    if cfg!(target_os = "macos") {
        return Some(data_folder_in(&home?.join("Library").join("Application Support")));
    }
    let config = std::env::var_os("XDG_CONFIG_HOME").map(PathBuf::from).filter(|p| p.is_absolute()).or_else(|| home.map(|home| home.join(".config")))?;
    Some(config.join("limoniter"))
}

pub fn data_folder_in(appdata: &Path) -> PathBuf {
    appdata.join("Limoniter")
}

/// The rows hex mode can show per line.
pub const HEX_ROWS: [usize; 3] = [8, 16, 32];

/// How a file's text and its preview share the space. Each tab has its own, and Markdown files start with the one chosen in Settings.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum PreviewView {
    #[default]
    Text,
    Split,
    Full,
}

impl PreviewView {
    pub const ALL: [Self; 3] = [Self::Text, Self::Split, Self::Full];

    /// As the settings file has it.
    fn id(self) -> &'static str {
        match self {
            Self::Text => "markup",
            Self::Split => "split",
            Self::Full => "full",
        }
    }

    /// As the Settings window offers it for Markdown files.
    pub fn markdown_label(self) -> &'static str {
        match self {
            Self::Text => "Markdown markup",
            Self::Split => "Live preview, split screen",
            Self::Full => "Live preview, full screen",
        }
    }

}

/// Choices remembered in the settings file.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Prefs {
    pub light: bool,
    pub edit: EditPrefs,
    pub find_from_selection: bool,
    pub find_in_file_folder: bool,
    pub watch_files: bool,
    pub hex_row: usize,
    pub print_header: bool,
    pub print_line_numbers: bool,
    pub paragraph: ParagraphSetup,
    pub markdown_view: PreviewView,
    pub keep_undo_history: bool,
    /// Files opened from Explorer each get a window of their own, rather than a tab in the one already open.
    pub new_window_per_file: bool,
    /// Spelling mistakes are underlined.
    pub spelling: bool,
}

impl Default for Prefs {
    fn default() -> Self {
        Self { light: true, edit: EditPrefs::default(), find_from_selection: true, find_in_file_folder: false, watch_files: true, hex_row: 16, print_header: true, print_line_numbers: false, paragraph: ParagraphSetup::default(), markdown_view: PreviewView::Text, keep_undo_history: true, new_window_per_file: false, spelling: false }
    }
}

impl Prefs {
    /// One "name=value" per line, for the settings file.
    pub fn to_settings(self) -> String {
        format!(
            "theme={}\nspaces_for_tab={}\nauto_indent={}\nlock_insert={}\nhome_to_start={}\nwhole_line_copy={}\nnulls_open_in_hex={}\ncopy_before_paste={}\nkeep_undo_history={}\nmarkdown_view={}\nfind_from_selection={}\nfind_in_file_folder={}\nwatch_files={}\nhex_row={}\nprint_header={}\nprint_line_numbers={}\nparagraph_width={}\nparagraph_left={}\nparagraph_first={}\nparagraph_align={}\nnew_window_per_file={}\nspelling={}\n",
            if self.light { "light" } else { "dark" },
            self.edit.spaces_for_tab,
            self.edit.auto_indent,
            self.edit.lock_insert,
            self.edit.home_to_start,
            self.edit.whole_line_copy,
            self.edit.nulls_open_in_hex,
            self.edit.copy_before_paste,
            self.keep_undo_history,
            self.markdown_view.id(),
            self.find_from_selection,
            self.find_in_file_folder,
            self.watch_files,
            self.hex_row,
            self.print_header,
            self.print_line_numbers,
            self.paragraph.width,
            self.paragraph.left,
            self.paragraph.first,
            self.paragraph.align.name(),
            self.new_window_per_file,
            self.spelling
        )
    }

    /// Reads the settings file; anything missing or unreadable keeps its default.
    pub fn from_settings(text: &str) -> Self {
        let mut prefs = Self::default();
        let (mut view_chosen, mut preview_was_open) = (false, false);
        for line in text.lines() {
            let Some((name, value)) = line.split_once('=') else {
                continue;
            };
            let value = value.trim();
            let flag = value.parse::<bool>().ok();
            let target = match name.trim() {
                "theme" => {
                    // Anything but the two it knows leaves the default alone.
                    match value {
                        "light" => prefs.light = true,
                        "dark" => prefs.light = false,
                        _ => {}
                    }
                    continue;
                }
                "paragraph_width" | "paragraph_left" | "paragraph_first" => {
                    if let Ok(number) = value.parse::<usize>() {
                        match name.trim() {
                            "paragraph_width" => prefs.paragraph.width = number.clamp(crate::reformat::LEAST_WIDTH, crate::reformat::MOST_WIDTH),
                            "paragraph_left" => prefs.paragraph.left = number.min(crate::reformat::MOST_WIDTH),
                            _ => prefs.paragraph.first = number.min(crate::reformat::MOST_WIDTH),
                        }
                    }
                    continue;
                }
                "markdown_view" => {
                    if let Some(view) = PreviewView::ALL.into_iter().find(|view| view.id() == value) {
                        prefs.markdown_view = view;
                        view_chosen = true;
                    }
                    continue;
                }
                // Before there was a choice, one switch said whether the preview was open.
                "preview_open" => {
                    preview_was_open = flag == Some(true);
                    continue;
                }
                "paragraph_align" => {
                    prefs.paragraph.align = Align::from_name(value).unwrap_or(prefs.paragraph.align);
                    continue;
                }
                "hex_row" => {
                    if let Some(row) = value.parse().ok().filter(|row| HEX_ROWS.contains(row)) {
                        prefs.hex_row = row;
                    }
                    continue;
                }
                "spaces_for_tab" => &mut prefs.edit.spaces_for_tab,
                "auto_indent" => &mut prefs.edit.auto_indent,
                "lock_insert" => &mut prefs.edit.lock_insert,
                "home_to_start" => &mut prefs.edit.home_to_start,
                "whole_line_copy" => &mut prefs.edit.whole_line_copy,
                "nulls_open_in_hex" => &mut prefs.edit.nulls_open_in_hex,
                "copy_before_paste" => &mut prefs.edit.copy_before_paste,
                "keep_undo_history" => &mut prefs.keep_undo_history,
                "new_window_per_file" => &mut prefs.new_window_per_file,
                "spelling" => &mut prefs.spelling,
                "find_from_selection" => &mut prefs.find_from_selection,
                "find_in_file_folder" => &mut prefs.find_in_file_folder,
                "watch_files" => &mut prefs.watch_files,
                "print_header" => &mut prefs.print_header,
                "print_line_numbers" => &mut prefs.print_line_numbers,
                _ => continue,
            };
            if let Some(flag) = flag {
                *target = flag;
            }
        }
        if preview_was_open && !view_chosen {
            prefs.markdown_view = PreviewView::Split;
        }
        prefs
    }
}

/// What the Settings window can change, and what it needs to show.
pub struct Target<'a> {
    pub view: &'a mut ViewOptions,
    pub prefs: &'a mut Prefs,
    pub file_tabs: &'a mut bool,
    pub status_bar: &'a mut bool,
    pub font: Option<&'a Path>,
    pub new_file_preset: &'a str,
    pub recent_files: usize,
}

/// What the window asks the app to do, beyond changing a value.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Outcome {
    Close,
    ChooseFont,
    ChoosePreset,
    ClearRecent,
    SetFileTypes,
    RemoveFileTypes,
    OpenWindowsPage,
}

/// What's being chosen on the File types page, and what Windows has now.
#[derive(Default)]
pub struct FileTypesState {
    pub ticked: BTreeSet<String>,
    pub others: String,
    pub registered: Vec<String>,
    pub status: String,
    pub program: String,
}

impl FileTypesState {
    /// Starts from what's registered with Windows now.
    pub fn from_registered(registered: Vec<String>, program: String) -> Self {
        let (ticked, others) = associations::split_registered(&registered);
        Self { ticked, others, registered, status: String::new(), program }
    }

    /// Carries out one of the page's buttons. The Windows calls are passed in, so that testing this changes nothing in Windows.
    pub fn carry_out(&mut self, outcome: Outcome, register: impl FnOnce(&[String]) -> Result<(), String>, remove: impl FnOnce() -> Result<(), String>, open_page: impl FnOnce() -> Result<(), String>) {
        match outcome {
            Outcome::SetFileTypes => match associations::parse_types(&self.ticked, &self.others) {
                Err(problem) => self.status = problem,
                Ok(types) if types.is_empty() => self.status = "Tick at least one type first.".to_string(),
                Ok(types) => match register(&types) {
                    Err(problem) => self.status = problem,
                    Ok(()) => {
                        let count = types.len();
                        self.registered = types;
                        self.status = match open_page() {
                            Ok(()) => format!("Limoniter can now open {count} type{}. To finish, choose Limoniter for each one on the Windows page that just opened.", if count == 1 { "" } else { "s" }),
                            Err(problem) => problem,
                        };
                    }
                },
            },
            Outcome::RemoveFileTypes => match remove() {
                Err(problem) => self.status = problem,
                Ok(()) => {
                    self.registered.clear();
                    self.ticked.clear();
                    self.others.clear();
                    self.status = "Limoniter is no longer registered with Windows. A type it was the default for will have Windows ask what to use.".to_string();
                }
            },
            Outcome::OpenWindowsPage => {
                if let Err(problem) = open_page() {
                    self.status = problem;
                }
            }
            _ => {}
        }
    }
}

#[derive(Default)]
pub struct WindowState {
    pub section: usize,
    pub find: String,
    pub focus: bool,
    pub file_types: FileTypesState,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Special {
    Theme,
    Font,
    TextSize,
    TabSize,
    HexRow,
    Preset,
    ClearRecent,
    MarkdownView,
    FileTypes,
}

#[derive(Clone, Copy)]
enum Row {
    Heading(&'static str),
    /// A check box: its label, a line saying what it does, and the value it changes.
    Flag(&'static str, &'static str, for<'a, 'b> fn(&'a mut Target<'b>) -> &'a mut bool),
    Special(&'static str, Special),
}

impl Row {
    fn matches(&self, find: &str) -> bool {
        let lower = |text: &str| text.to_lowercase().contains(find);
        match self {
            Row::Heading(_) => false,
            Row::Flag(label, tip, _) => lower(label) || lower(tip),
            Row::Special(label, _) => lower(label),
        }
    }
}

const SECTIONS: [(&str, &[Row]); 6] = [
    (
        "Appearance",
        &[
            Row::Special("Color theme", Special::Theme),
            Row::Special("Editor font", Special::Font),
            Row::Special("Text size", Special::TextSize),
            Row::Heading("In the editor"),
            Row::Flag("Ruler", "The column ruler above the text", |t| &mut t.view.ruler),
            Row::Flag("Line numbers", "Numbers down the left of the text", |t| &mut t.view.line_numbers),
            Row::Flag("Bookmark gutter", "The strip beside the line numbers that shows bookmarks", |t| &mut t.view.bookmark_gutter),
            Row::Flag("Document map", "The small picture of the whole file on the right", |t| &mut t.view.document_map),
            Row::Flag("Highlight the active line", "Marks the row the cursor is on", |t| &mut t.view.active_line),
            Row::Flag("Highlight the active column", "Marks the column the cursor is in", |t| &mut t.view.active_col),
            Row::Flag("Fold markers", "Shows a box beside lines that have something indented under them, to fold it", |t| &mut t.view.fold_lines),
            Row::Flag("Line change indicator", "Marks changed lines beside their numbers: orange until saved, then green", |t| &mut t.view.line_change),
            Row::Flag("Highlight all copies of the selection", "Marks every other copy of the selected text on screen", |t| &mut t.view.highlight_selected),
            Row::Flag("Rainbow parentheses", "Gives each level of nested brackets its own color", |t| &mut t.view.rainbow_parentheses),
            Row::Flag("Show spaces and tabs", "Draws a mark for each one", |t| &mut t.view.whitespace),
            Row::Flag("Show line endings", "Draws a mark at the end of each line", |t| &mut t.view.line_endings),
            Row::Flag("Show control characters as tokens", "Shows them by name, rather than as one small cell", |t| &mut t.view.source_view),
            Row::Heading("In the window"),
            Row::Flag("File tabs", "The row of open files above the text", |t| t.file_tabs),
            Row::Flag("Status bar", "The row of details along the bottom", |t| t.status_bar),
        ],
    ),
    (
        "Editing",
        &[
            Row::Special("Tab size", Special::TabSize),
            Row::Flag("Tab key inserts spaces", "Spaces up to the next tab stop, instead of a tab character", |t| &mut t.prefs.edit.spaces_for_tab),
            Row::Flag("Word wrap", "Long lines continue on the next row. Saved files aren't changed", |t| &mut t.view.word_wrap),
            Row::Flag("Keep the indent on new lines", "Enter starts the new line with the same indent as the last", |t| &mut t.prefs.edit.auto_indent),
            Row::Flag("Home always goes to the start of the line", "Rather than to the first text, and then the start", |t| &mut t.prefs.edit.home_to_start),
            Row::Flag("Ignore the Insert key", "So overwriting can't be switched on by accident", |t| &mut t.prefs.edit.lock_insert),
            Row::Flag("Copy or cut the whole line if nothing is selected", "Instead of doing nothing", |t| &mut t.prefs.edit.whole_line_copy),
            Row::Flag("Copy before paste", "Pasting over selected text swaps them: what was selected goes on the clipboard, ready to paste somewhere else", |t| &mut t.prefs.edit.copy_before_paste),
            Row::Flag("Keep undo history after a file is closed", "Kept in a folder beside the settings, and that includes text you've since deleted. Turn this off if that matters for what you edit", |t| &mut t.prefs.keep_undo_history),
        ],
    ),
    (
        "Hex mode",
        &[
            Row::Flag("Open files containing zero bytes in hex mode", "When off, they open as text", |t| &mut t.prefs.edit.nulls_open_in_hex),
            Row::Special("Bytes per row", Special::HexRow),
        ],
    ),
    (
        "Searching",
        &[
            Row::Flag("Start a search with the selected text", "Opening Find puts one selected line in the find box", |t| &mut t.prefs.find_from_selection),
            Row::Flag("Find in Files starts in this file's folder", "Rather than the folder used last", |t| &mut t.prefs.find_in_file_folder),
        ],
    ),
    (
        "Files",
        &[
            Row::Flag("Check for changes by other programs", "Each time the window comes back into focus", |t| &mut t.prefs.watch_files),
            Row::Flag("Open each file in a new window", "When a file is opened from Explorer. When off, it opens as a tab in the Limoniter window already open", |t| &mut t.prefs.new_window_per_file),
            Row::Special("Format for new files", Special::Preset),
            Row::Special("Recently opened files", Special::ClearRecent),
            Row::Special("Open Markdown files as", Special::MarkdownView),
            Row::Heading("Printing"),
            Row::Flag("Print the file name and page number", "Across the top of each page", |t| &mut t.prefs.print_header),
            Row::Flag("Print line numbers", "Down the left of the text", |t| &mut t.prefs.print_line_numbers),
        ],
    ),
    ("File types", &[Row::Special("Default editor for file types in Windows", Special::FileTypes)]),
];

fn draw_file_types(ui: &mut Ui, label: &str, types: &mut FileTypesState, outcome: &mut Option<Outcome>) {
    ui.label(label);
    ui.label(RichText::new("Windows only lets you change a default yourself. Choose types here, then pick Limoniter for each one on the Windows page that opens.").color(theme::text_dim()));
    ui.add_space(6.0);
    for (group, names) in associations::COMMON_TYPES {
        ui.label(RichText::new(group).color(theme::text_faint()));
        ui.horizontal_wrapped(|ui| {
            for name in names {
                let mut on = types.ticked.contains(*name);
                if ui.checkbox(&mut on, *name).changed() {
                    if on {
                        types.ticked.insert((*name).to_string());
                    } else {
                        types.ticked.remove(*name);
                    }
                }
            }
        });
    }
    ui.add_space(4.0);
    ui.label("Other types, separated by commas:");
    ui.add(egui::TextEdit::singleline(&mut types.others).desired_width(300.0));
    ui.add_space(6.0);
    ui.horizontal(|ui| {
        if ui.button("Set as default...").on_hover_text("Registers Limoniter for the ticked types, then opens the Windows page to choose it").clicked() {
            *outcome = Some(Outcome::SetFileTypes);
        }
        if ui.button("Open Windows settings").clicked() {
            *outcome = Some(Outcome::OpenWindowsPage);
        }
        if ui.add_enabled(!types.registered.is_empty(), Button::new("Remove from Windows")).on_hover_text("Takes away everything Limoniter registered").clicked() {
            *outcome = Some(Outcome::RemoveFileTypes);
        }
    });
    ui.add_space(4.0);
    if types.registered.is_empty() {
        ui.label(RichText::new("Not registered with Windows yet.").color(theme::text_faint()));
    } else {
        ui.label(RichText::new(format!("Registered with Windows: {}", types.registered.join(", "))).color(theme::text_faint()));
    }
    if !types.program.is_empty() {
        ui.label(RichText::new(format!("This copy of the program: {}. If it moves, press Set as default again.", types.program)).color(theme::text_faint()));
    }
    if !types.status.is_empty() {
        ui.add_space(4.0);
        ui.label(types.status.as_str());
    }
}

fn draw_special(ui: &mut Ui, label: &str, kind: Special, target: &mut Target<'_>, outcome: &mut Option<Outcome>) {
    ui.horizontal(|ui| {
        ui.label(label);
        ui.add_space(8.0);
        match kind {
            Special::Theme => {
                ui.radio_value(&mut target.prefs.light, false, "Dark");
                ui.radio_value(&mut target.prefs.light, true, "Light");
            }
            Special::Font => {
                let name = target.font.and_then(Path::file_stem).map_or_else(|| "Consolas".to_string(), |stem| stem.to_string_lossy().into_owned());
                ui.label(RichText::new(name).color(theme::text_dim()));
                if ui.button("Choose...").clicked() {
                    *outcome = Some(Outcome::ChooseFont);
                }
            }
            Special::TextSize => {
                ui.add(egui::Slider::new(&mut target.view.font_size, 8.0..=40.0).step_by(1.0));
            }
            Special::TabSize => {
                ui.add(egui::DragValue::new(&mut target.view.tab_size).range(MIN_TAB_SIZE..=MAX_TAB_SIZE));
            }
            Special::HexRow => {
                for row in HEX_ROWS {
                    ui.radio_value(&mut target.prefs.hex_row, row, row.to_string());
                }
            }
            Special::Preset => {
                ui.label(RichText::new(target.new_file_preset).color(theme::text_dim()));
                if ui.button("Choose...").clicked() {
                    *outcome = Some(Outcome::ChoosePreset);
                }
            }
            Special::ClearRecent => {
                ui.label(RichText::new(format!("{} listed", target.recent_files)).color(theme::text_dim()));
                if ui.add_enabled(target.recent_files > 0, Button::new("Clear the list")).clicked() {
                    *outcome = Some(Outcome::ClearRecent);
                }
            }
            Special::MarkdownView => {
                egui::ComboBox::from_id_salt("markdown-view").width(200.0).selected_text(target.prefs.markdown_view.markdown_label()).show_ui(ui, |ui| {
                    for view in PreviewView::ALL {
                        ui.selectable_value(&mut target.prefs.markdown_view, view, view.markdown_label());
                    }
                });
            }
            // It's a whole page, so draw_row gives it room of its own.
            Special::FileTypes => {}
        }
    });
}

fn draw_row(ui: &mut Ui, row: Row, target: &mut Target<'_>, types: &mut FileTypesState, outcome: &mut Option<Outcome>) {
    match row {
        Row::Special(label, Special::FileTypes) => draw_file_types(ui, label, types, outcome),
        Row::Heading(text) => {
            ui.add_space(8.0);
            ui.label(RichText::new(text).color(theme::text_dim()));
        }
        Row::Flag(label, tip, value) => {
            ui.checkbox(value(target), label).on_hover_text(tip);
        }
        Row::Special(label, kind) => draw_special(ui, label, kind, target, outcome),
    }
}

/// Draws the window. Changes apply straight away, so there's only Close.
pub fn window(ctx: &Context, state: &mut WindowState, target: &mut Target<'_>) -> Option<Outcome> {
    let mut outcome = ctx.input(|i| i.key_pressed(egui::Key::Escape)).then_some(Outcome::Close);
    egui::Window::new("Settings").collapsible(false).resizable(false).fixed_size(vec2(680.0, 470.0)).anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0]).show(ctx, |ui| {
        ui.horizontal_top(|ui| {
            ui.vertical(|ui| {
                ui.set_width(150.0);
                let field = ui.add(egui::TextEdit::singleline(&mut state.find).hint_text("Find a setting").desired_width(150.0));
                if state.focus {
                    field.request_focus();
                    state.focus = false;
                }
                ui.add_space(6.0);
                for (place, (name, _)) in SECTIONS.iter().enumerate() {
                    let on = state.find.is_empty() && state.section == place;
                    if ui.add(Button::selectable(on, *name).frame_when_inactive(false).min_size(vec2(150.0, 0.0))).clicked() {
                        state.section = place;
                        state.find.clear();
                    }
                }
            });
            ui.separator();
            ui.vertical(|ui| {
                ui.set_width(490.0);
                ui.set_height(430.0);
                egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
                    let find = state.find.trim().to_lowercase();
                    if find.is_empty() {
                        for row in SECTIONS[state.section].1 {
                            draw_row(ui, *row, target, &mut state.file_types, &mut outcome);
                        }
                        return;
                    }
                    let mut any = false;
                    for (name, rows) in SECTIONS {
                        let found: Vec<Row> = rows.iter().copied().filter(|row| row.matches(&find)).collect();
                        if !found.is_empty() {
                            any = true;
                            ui.add_space(4.0);
                            ui.label(RichText::new(name).color(theme::text_dim()));
                            for row in found {
                                draw_row(ui, row, target, &mut state.file_types, &mut outcome);
                            }
                        }
                    }
                    if !any {
                        ui.label(RichText::new("No setting matches that.").color(theme::text_dim()));
                    }
                });
            });
        });
        ui.separator();
        ui.horizontal(|ui| {
            ui.label(RichText::new("Changes apply straight away.").color(theme::text_faint()));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.button("Close").clicked() {
                    outcome = Some(Outcome::Close);
                }
            });
        });
    });
    outcome
}

#[cfg(test)]
mod tests {
    use super::*;

    use eframe::egui::RawInput;

    fn all_rows() -> impl Iterator<Item = Row> {
        SECTIONS.iter().flat_map(|(_, rows)| rows.iter().copied())
    }

    #[test]
    fn changed_choices_survive_the_settings_file() {
        let prefs = Prefs {
            light: true,
            edit: EditPrefs { spaces_for_tab: true, auto_indent: false, lock_insert: true, home_to_start: true, whole_line_copy: true, nulls_open_in_hex: false, copy_before_paste: true },
            find_from_selection: false,
            find_in_file_folder: true,
            watch_files: false,
            hex_row: 32,
            print_header: false,
            print_line_numbers: true,
            paragraph: ParagraphSetup { width: 60, left: 2, first: 4, align: Align::Justify },
            markdown_view: PreviewView::Full,
            keep_undo_history: false,
            new_window_per_file: true,
            spelling: true,
        };
        assert_eq!(Prefs::from_settings(&prefs.to_settings()), prefs);
        assert_eq!(Prefs::from_settings(&Prefs::default().to_settings()), Prefs::default());
    }

    #[test]
    fn an_open_preview_from_before_there_was_a_choice_carries_on() {
        assert_eq!(Prefs::from_settings("preview_open=true\n").markdown_view, PreviewView::Split);
        assert_eq!(Prefs::from_settings("preview_open=false\n").markdown_view, PreviewView::Text);
        assert_eq!(Prefs::from_settings("preview_open=true\nmarkdown_view=markup\n").markdown_view, PreviewView::Text, "a choice beats the old switch");
        assert_eq!(Prefs::from_settings("markdown_view=sideways\n").markdown_view, PreviewView::Text);
    }

    #[test]
    fn missing_or_nonsense_values_keep_the_defaults() {
        assert_eq!(Prefs::from_settings(""), Prefs::default());
        assert_eq!(Prefs::from_settings("theme=purple\nhex_row=7\nhex_row=x\nauto_indent=maybe\nnonsense\nunknown=true"), Prefs::default());
    }

    #[test]
    fn the_defaults_keep_how_the_app_has_always_behaved_and_start_light() {
        let prefs = Prefs::default();
        assert!(prefs.light);
        assert!(!Prefs::from_settings("theme=dark\n").light, "a dark choice already made is kept");
        assert!(prefs.edit.auto_indent && prefs.edit.nulls_open_in_hex && prefs.watch_files);
        assert!(!prefs.edit.spaces_for_tab && !prefs.edit.lock_insert && !prefs.edit.home_to_start && !prefs.edit.whole_line_copy && !prefs.edit.copy_before_paste);
        assert_eq!(prefs.markdown_view, PreviewView::Text);
        assert_eq!(prefs.hex_row, 16);
    }

    fn check_values() -> (ViewOptions, Prefs, bool, bool) {
        (ViewOptions::default(), Prefs::default(), true, true)
    }

    #[test]
    fn every_check_box_changes_its_own_value_and_no_other() {
        let snapshot = |view: &ViewOptions, prefs: &Prefs, tabs: bool, status: bool| format!("{}{}{tabs}{status}", view.to_settings(), prefs.to_settings());
        for row in all_rows() {
            let Row::Flag(label, _, value) = row else {
                continue;
            };
            let (mut view, mut prefs, mut tabs, mut status) = check_values();
            let before = snapshot(&view, &prefs, tabs, status);
            let mut target = Target { view: &mut view, prefs: &mut prefs, file_tabs: &mut tabs, status_bar: &mut status, font: None, new_file_preset: "", recent_files: 0 };
            let flag = value(&mut target);
            *flag = !*flag;
            let after = snapshot(&view, &prefs, tabs, status);
            assert_ne!(before, after, "{label} changes something that's saved");
            let changed = before.lines().zip(after.lines()).filter(|(a, b)| a != b).count();
            assert!(changed <= 1 || label == "Status bar" || label == "File tabs", "{label} changed {changed} values");
        }
    }

    #[test]
    fn labels_are_all_different_and_can_be_found() {
        let mut seen = std::collections::HashSet::new();
        for row in all_rows() {
            match row {
                Row::Heading(_) => assert!(!row.matches("in")),
                Row::Flag(label, tip, _) => {
                    assert!(seen.insert(label), "{label} twice");
                    assert!(row.matches(&label.to_lowercase()));
                    assert!(!tip.is_empty());
                }
                Row::Special(label, _) => {
                    assert!(seen.insert(label), "{label} twice");
                    assert!(row.matches(&label.to_lowercase()));
                }
            }
        }
    }

    #[test]
    fn searching_finds_a_setting_by_what_it_does() {
        let find = |text: &str| all_rows().filter(|row| row.matches(text)).count();
        assert!(find("theme") >= 1);
        assert!(find("overwriting") >= 1, "from its description");
        assert_eq!(find("no such setting"), 0);
        assert_eq!(find("default editor"), 1);
    }

    fn page(ticked: &[&str], others: &str) -> FileTypesState {
        FileTypesState { ticked: ticked.iter().map(|name| name.to_string()).collect(), others: others.to_string(), ..FileTypesState::default() }
    }

    fn never_register(_: &[String]) -> Result<(), String> {
        panic!("this shouldn't have reached Windows")
    }

    fn never() -> Result<(), String> {
        panic!("this shouldn't have reached Windows")
    }

    #[test]
    fn setting_the_default_registers_exactly_whats_chosen_then_opens_the_windows_page() {
        let mut state = page(&["txt", "rs"], ".log");
        let opened = std::cell::Cell::new(false);
        state.carry_out(
            Outcome::SetFileTypes,
            |types| {
                assert_eq!(types, ["log", "rs", "txt"]);
                assert!(!opened.get(), "registered before the page opens, so it's listed there");
                Ok(())
            },
            never,
            || {
                opened.set(true);
                Ok(())
            },
        );
        assert!(opened.get());
        assert_eq!(state.registered, ["log", "rs", "txt"]);
        assert!(state.status.contains("3 types") && state.status.contains("choose Limoniter"), "{}", state.status);
    }

    #[test]
    fn nothing_reaches_windows_with_nothing_chosen_or_a_bad_type() {
        let mut state = page(&[], "");
        state.carry_out(Outcome::SetFileTypes, never_register, never, never);
        assert!(state.status.contains("at least one"));
        let mut state = page(&["txt"], "exe");
        state.carry_out(Outcome::SetFileTypes, never_register, never, never);
        assert!(state.status.contains("run by Windows"));
        assert!(state.registered.is_empty());
    }

    #[test]
    fn a_refusal_from_windows_is_shown_and_nothing_is_claimed() {
        let mut state = page(&["txt"], "");
        state.carry_out(Outcome::SetFileTypes, |_| Err("no".to_string()), never, never);
        assert_eq!(state.status, "no");
        assert!(state.registered.is_empty());
        let mut state = page(&["txt"], "");
        state.registered = vec!["txt".to_string()];
        state.carry_out(Outcome::RemoveFileTypes, never_register, || Err("still no".to_string()), never);
        assert_eq!(state.status, "still no");
        assert_eq!(state.registered, ["txt"], "it's still registered");
    }

    #[test]
    fn removing_clears_the_page() {
        let mut state = page(&["txt"], "log");
        state.registered = vec!["txt".to_string(), "log".to_string()];
        state.carry_out(Outcome::RemoveFileTypes, never_register, || Ok(()), never);
        assert!(state.registered.is_empty() && state.ticked.is_empty() && state.others.is_empty());
        assert!(state.status.contains("no longer"));
    }

    #[test]
    fn the_page_starts_from_whats_registered() {
        let state = FileTypesState::from_registered(vec!["log".to_string(), "weird".to_string()], "x.exe".to_string());
        assert!(state.ticked.contains("log"));
        assert_eq!(state.others, "weird");
        assert_eq!(state.registered, ["log", "weird"]);
    }

    #[test]
    fn the_window_draws_every_section_and_search_in_both_themes() {
        for light in [false, true] {
            theme::set_light(light);
            let ctx = Context::default();
            theme::apply(&ctx);
            let (mut view, mut prefs, mut tabs, mut status) = check_values();
            let mut target = Target { view: &mut view, prefs: &mut prefs, file_tabs: &mut tabs, status_bar: &mut status, font: None, new_file_preset: "Windows", recent_files: 3 };
            let states: Vec<WindowState> = (0..SECTIONS.len())
                .map(|section| WindowState { section, ..WindowState::default() })
                .chain(["size", "zzz"].map(|find| WindowState { find: find.to_string(), ..WindowState::default() }))
                .collect();
            for mut state in states {
                for _ in 0..2 {
                    let raw = RawInput { screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, vec2(1200.0, 800.0))), ..RawInput::default() };
                    let mut output = ctx.run_ui(raw, |ui| {
                        assert_eq!(window(ui.ctx(), &mut state, &mut target), None);
                    });
                    output.textures_delta.clear();
                }
            }
        }
        theme::set_light(false);
    }

    #[test]
    fn escape_closes_the_window() {
        let ctx = Context::default();
        let (mut view, mut prefs, mut tabs, mut status) = check_values();
        let mut target = Target { view: &mut view, prefs: &mut prefs, file_tabs: &mut tabs, status_bar: &mut status, font: None, new_file_preset: "", recent_files: 0 };
        let mut state = WindowState::default();
        let key = egui::Event::Key { key: egui::Key::Escape, physical_key: None, pressed: true, repeat: false, modifiers: egui::Modifiers::NONE };
        let raw = RawInput { screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, vec2(1200.0, 800.0))), events: vec![key], ..RawInput::default() };
        let mut outcome = None;
        let mut output = ctx.run_ui(raw, |ui| outcome = window(ui.ctx(), &mut state, &mut target));
        output.textures_delta.clear();
        assert_eq!(outcome, Some(Outcome::Close));
    }
}
