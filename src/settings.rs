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
/// Application Support on macOS, and the configuration folder (~/.config) on Linux. A portable copy keeps them in
/// a folder beside itself instead.
pub fn data_folder() -> Option<PathBuf> {
    if let Some(folder) = portable_folder() {
        return Some(folder.to_path_buf());
    }
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

/// The file that, beside the program, makes it portable.
pub const PORTABLE_FILE: &str = "portable";
/// The folder a portable copy keeps everything in, beside the program.
pub const PORTABLE_FOLDER: &str = "Limoniter data";

/// Where a portable copy keeps everything: if there's a file named `portable` beside the program (as in the
/// portable download for Windows), it's the `Limoniter data` folder beside it, so settings, recent files, layouts,
/// downloaded emulators and the rest go with it on a USB stick and leave nothing on the computer. Worked out once.
pub fn portable_folder() -> Option<&'static Path> {
    static FOLDER: std::sync::OnceLock<Option<PathBuf>> = std::sync::OnceLock::new();
    FOLDER
        .get_or_init(|| {
            if cfg!(test) {
                return None;
            }
            portable_folder_beside(std::env::current_exe().ok()?.parent()?)
        })
        .as_deref()
}

/// The portable folder for a program in `program_folder`, if it's a portable copy.
pub fn portable_folder_beside(program_folder: &Path) -> Option<PathBuf> {
    program_folder.join(PORTABLE_FILE).is_file().then(|| program_folder.join(PORTABLE_FOLDER))
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
    /// Every few seconds as well, not only when the window comes back into focus.
    pub watch_often: bool,
    /// A changed file with no unsaved changes is reloaded without asking, staying at its end if it was there.
    pub reload_quietly: bool,
    pub hex_row: usize,
    pub print_header: bool,
    pub print_line_numbers: bool,
    pub paragraph: ParagraphSetup,
    pub markdown_view: PreviewView,
    pub keep_undo_history: bool,
    /// Files opened from Explorer each get a window of their own, rather than a tab in the one already open.
    pub new_window_per_file: bool,
    /// CSV and TSV files open straight into table view.
    pub open_tables: bool,
    /// Spelling mistakes are underlined.
    pub spelling: bool,
    /// What spell checking reads in a file of code: its comments and strings, those and its names, or nothing.
    pub code_spelling: crate::code_spelling::CodeSpelling,
    /// Files with unsaved changes are saved by themselves, this many seconds after they were first changed.
    pub autosave: bool,
    pub autosave_seconds: u32,
    /// What starts an autosave, when it's on: the delay, another tab being chosen, or the window losing the focus.
    pub autosave_when: AutosaveWhen,
    /// Files the language server reports errors in are left unsaved by autosave.
    pub autosave_skip_errors: bool,
    /// Quitting with unsaved or never-saved files doesn't ask: they come back as they were next time.
    pub hot_exit: bool,
    /// The project open when Limoniter last closed opens again when it starts.
    pub reopen_project: bool,
    /// The first save of a file in a session keeps the file as it was beside it, ending .bak.
    pub backup_on_save: bool,
    /// How Spectrum BASIC listings write block graphics and UDGs.
    pub zx_notation: crate::zx_graphics::Notation,
    /// The 680x0 that 68000 source is assembled for, unless MACHINE in it says another.
    pub m68k_processor: crate::asm::m68k::Processor,
    /// The maths coprocessor (FPU) beside it, unless MACHINE 68881 in the source chooses one.
    pub m68k_fpu: crate::asm::m68k::Fpu,
    /// The 68851 MMU beside it (for a 68020), unless MACHINE 68851 in the source chooses it.
    pub m68k_mmu: crate::asm::m68k::Mmu,
    /// Once a day, a newer OpticOS ROM is looked for on GitHub and installed.
    pub opticos_updates: bool,
    /// Once a day, GitHub is asked whether there's a newer Limoniter.
    pub update_checks: bool,
    /// A newer Limoniter the daily check finds is downloaded by itself, and offered once it's ready.
    pub update_download: bool,
    /// The language the words are in, by its code (en, de, es, fr, it).
    pub language: &'static str,
}

/// How long autosave waits, in seconds.
pub const AUTOSAVE_TIMES: [u32; 9] = [1, 2, 5, 15, 30, 60, 120, 300, 600];

/// What makes autosave save a file, as VS Code's `files.autoSave` has it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum AutosaveWhen {
    /// Once it has had unsaved changes for the delay.
    #[default]
    AfterDelay,
    /// When another tab or panel is chosen, or the window loses the focus.
    OnFocusChange,
    /// When the window loses the focus.
    OnWindowChange,
}

/// A project's own say about autosave, which beats the settings'.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AutosaveChoice {
    Off,
    When(AutosaveWhen),
}

impl AutosaveChoice {
    pub fn id(self) -> &'static str {
        match self {
            Self::Off => "off",
            Self::When(when) => when.id(),
        }
    }

    pub fn from_id(id: &str) -> Option<Self> {
        if id == "off" { Some(Self::Off) } else { AutosaveWhen::from_id(id).map(Self::When) }
    }
}

impl AutosaveWhen {
    pub const ALL: [Self; 3] = [Self::AfterDelay, Self::OnFocusChange, Self::OnWindowChange];

    pub fn id(self) -> &'static str {
        match self {
            Self::AfterDelay => "delay",
            Self::OnFocusChange => "focus",
            Self::OnWindowChange => "window",
        }
    }

    pub fn from_id(id: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|when| when.id() == id)
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::AfterDelay => "After a delay",
            Self::OnFocusChange => "When another tab or panel is chosen",
            Self::OnWindowChange => "When the window loses the focus",
        }
    }
}

impl Default for Prefs {
    fn default() -> Self {
        Self { light: true, edit: EditPrefs::default(), find_from_selection: true, find_in_file_folder: false, watch_files: true, watch_often: false, reload_quietly: false, hex_row: 16, print_header: true, print_line_numbers: false, paragraph: ParagraphSetup::default(), markdown_view: PreviewView::Text, keep_undo_history: true, new_window_per_file: false, spelling: false, code_spelling: crate::code_spelling::CodeSpelling::Comments, open_tables: false, autosave: false, autosave_seconds: 60, autosave_when: AutosaveWhen::AfterDelay, autosave_skip_errors: false, hot_exit: true, reopen_project: false, backup_on_save: false, zx_notation: crate::zx_graphics::Notation::Limoniter, m68k_processor: crate::asm::m68k::Processor::M68000, m68k_fpu: crate::asm::m68k::Fpu::None, m68k_mmu: crate::asm::m68k::Mmu::None, opticos_updates: true, update_checks: true, update_download: false, language: "en" }
    }
}

impl Prefs {
    /// One "name=value" per line, for the settings file.
    pub fn to_settings(self) -> String {
        format!(
            "theme={}\nspaces_for_tab={}\nauto_indent={}\nlock_insert={}\nhome_to_start={}\nwhole_line_copy={}\nnulls_open_in_hex={}\ncopy_before_paste={}\nkeep_undo_history={}\nmarkdown_view={}\nfind_from_selection={}\nfind_in_file_folder={}\nwatch_files={}\nwatch_often={}\nreload_quietly={}\nhex_row={}\nprint_header={}\nprint_line_numbers={}\nparagraph_width={}\nparagraph_left={}\nparagraph_first={}\nparagraph_align={}\nnew_window_per_file={}\nspelling={}\nopen_tables={}\nautosave={}\nautosave_seconds={}\nbackup_on_save={}\nzx_notation={}\nm68k_processor={}\nm68k_fpu={}\nm68k_mmu={}\nopticos_updates={}\nupdate_checks={}\nupdate_download={}\nlanguage={}\nautosave_when={}\nautosave_skip_errors={}\nhot_exit={}\ncode_spelling={}\nreopen_project={}\n",
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
            self.watch_often,
            self.reload_quietly,
            self.hex_row,
            self.print_header,
            self.print_line_numbers,
            self.paragraph.width,
            self.paragraph.left,
            self.paragraph.first,
            self.paragraph.align.name(),
            self.new_window_per_file,
            self.spelling,
            self.open_tables,
            self.autosave,
            self.autosave_seconds,
            self.backup_on_save,
            self.zx_notation.id(),
            self.m68k_processor.id(),
            self.m68k_fpu.id(),
            self.m68k_mmu.id(),
            self.opticos_updates,
            self.update_checks,
            self.update_download,
            self.language,
            self.autosave_when.id(),
            self.autosave_skip_errors,
            self.hot_exit,
            self.code_spelling.id(),
            self.reopen_project
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
                "zx_notation" => {
                    prefs.zx_notation = crate::zx_graphics::Notation::from_id(value).unwrap_or_default();
                    continue;
                }
                "m68k_processor" => {
                    prefs.m68k_processor = value.parse().ok().and_then(crate::asm::m68k::Processor::from_id).unwrap_or_default();
                    continue;
                }
                "m68k_fpu" => {
                    prefs.m68k_fpu = value.parse().ok().and_then(crate::asm::m68k::Fpu::from_id).unwrap_or_default();
                    continue;
                }
                "m68k_mmu" => {
                    prefs.m68k_mmu = value.parse().ok().and_then(crate::asm::m68k::Mmu::from_id).unwrap_or_default();
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
                "autosave_seconds" => {
                    if let Some(seconds) = value.parse().ok().filter(|seconds| AUTOSAVE_TIMES.contains(seconds)) {
                        prefs.autosave_seconds = seconds;
                    }
                    continue;
                }
                "code_spelling" => {
                    prefs.code_spelling = crate::code_spelling::CodeSpelling::from_id(value).unwrap_or_default();
                    continue;
                }
                "autosave_when" => {
                    prefs.autosave_when = AutosaveWhen::from_id(value).unwrap_or_default();
                    continue;
                }
                "language" => {
                    if let Some(language) = crate::i18n::LANGUAGES.iter().find(|l| l.code == value) {
                        prefs.language = language.code;
                    }
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
                "open_tables" => &mut prefs.open_tables,
                "autosave" => &mut prefs.autosave,
                "autosave_skip_errors" => &mut prefs.autosave_skip_errors,
                "hot_exit" => &mut prefs.hot_exit,
                "reopen_project" => &mut prefs.reopen_project,
                "backup_on_save" => &mut prefs.backup_on_save,
                "opticos_updates" => &mut prefs.opticos_updates,
                "update_checks" => &mut prefs.update_checks,
                "update_download" => &mut prefs.update_download,
                "find_from_selection" => &mut prefs.find_from_selection,
                "find_in_file_folder" => &mut prefs.find_in_file_folder,
                "watch_files" => &mut prefs.watch_files,
                "watch_often" => &mut prefs.watch_often,
                "reload_quietly" => &mut prefs.reload_quietly,
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
    /// Whether language servers are used.
    pub language_servers: &'a mut bool,
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
    Themes,
    Assemblers,
    AmigaNdk,
    AmigaTools,
    RetroHelper,
    NewKinds,
    LanguageServers,
    CompareRules,
    ToolbarButtons,
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
                Ok(types) if types.is_empty() => self.status = crate::i18n::tr("Tick at least one type first.").to_string(),
                Ok(types) => match register(&types) {
                    Err(problem) => self.status = problem,
                    Ok(()) => {
                        let count = types.len();
                        self.registered = types;
                        self.status = match open_page() {
                            Ok(()) => if count == 1 { crate::i18n::tr("Limoniter can now open 1 type. To finish, choose Limoniter for it on the Windows page that just opened.").to_string() } else { crate::i18n::tf!("Limoniter can now open {count} types. To finish, choose Limoniter for each one on the Windows page that just opened.") },
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
                    self.status = crate::i18n::tr("Limoniter is no longer registered with Windows. A type it was the default for will have Windows ask what to use.").to_string();
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
    Language,
    Theme,
    Font,
    TextSize,
    LineSpacing,
    TabSize,
    MarkerColumns,
    HexRow,
    AutosaveTime,
    AutosaveWhen,
    CodeSpelling,
    Preset,
    ClearRecent,
    MarkdownView,
    ZxNotation,
    M68kProcessor,
    M68kFpu,
    M68kMmu,
    Assemblers,
    AmigaNdk,
    AmigaTools,
    RetroHelper,
    NewKinds,
    LanguageServers,
    OwnGrammars,
    FileTypes,
    CompareRules,
    ToolbarButtons,
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
        // Found by its words in English or in the language chosen.
        let lower = |text: &str| text.to_lowercase().contains(find) || crate::i18n::tr(text).to_lowercase().contains(find);
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
            Row::Special("Language", Special::Language),
            Row::Special("Color theme", Special::Theme),
            Row::Special("Editor font", Special::Font),
            Row::Special("Text size", Special::TextSize),
            Row::Special("Line spacing", Special::LineSpacing),
            Row::Heading("In the editor"),
            Row::Flag("Ruler", "The column ruler above the text", |t| &mut t.view.ruler),
            Row::Flag("Column markers", "Lines down the text after chosen columns, such as 72 or 80", |t| &mut t.view.column_markers),
            Row::Special("Column marker columns", Special::MarkerColumns),
            Row::Flag("Line numbers", "Numbers down the left of the text", |t| &mut t.view.line_numbers),
            Row::Flag("Bookmark gutter", "The strip beside the line numbers that shows bookmarks", |t| &mut t.view.bookmark_gutter),
            Row::Flag("Document map", "The small picture of the whole file on the right", |t| &mut t.view.document_map),
            Row::Flag("Breadcrumbs", "Above the text: the file's folders, and the function or label the cursor is in, each a menu to go to another", |t| &mut t.view.breadcrumbs),
            Row::Flag("Sticky scroll", "The function or label scrolled past stays at the top of the text", |t| &mut t.view.sticky_scroll),
            Row::Flag("Indentation guides", "A faint line down each level of indentation", |t| &mut t.view.indent_guides),
            Row::Flag("Indentation guides coloured by depth", "Each level of the guides in its own colour, as the rainbow brackets are", |t| &mut t.view.indent_rainbow),
            Row::Flag("Problems at the ends of their lines", "Each line's problem written faintly after it, as well as marked beside it", |t| &mut t.view.problems_at_line_end),
            Row::Flag("Colours shown where they're written", "Written as #rrggbb, rgb(), hsl(), or the Amiga's $0RGB beside COLOR registers, with a picker at the cursor to change one", |t| &mut t.view.colour_swatches),
            Row::Flag("Colour each column of a CSV file", "In a CSV, TSV or other delimited file, each column's text in a colour of its own, so the columns are easy to follow across the rows", |t| &mut t.view.rainbow_csv),
            Row::Flag("Comment kinds in colour", "TODO, FIXME, NOTE and the like, and comments beginning with ! or ?, in colours of their own", |t| &mut t.view.comment_kinds),
            Row::Flag("Trim trailing spaces as a file's saved", "As .editorconfig's trim_trailing_whitespace does; a file's .editorconfig has the last word", |t| &mut t.view.trim_on_save),
            Row::Flag("Only on the lines changed", "Trailing spaces are trimmed on the lines changed since the file was opened, so the rest of its history isn't touched", |t| &mut t.view.trim_only_changed),
            Row::Flag("Folds take in their closing bracket", "A folded block shows as { … } on one line, the line with its closing bracket folded in with it", |t| &mut t.view.fold_closer),
            Row::Flag("Show the keys for commands chosen from a menu", "Now and then, a note of the shortcut for a command just chosen from a menu or the command box, to learn the keys by", |t| &mut t.view.learn_keys),
            Row::Flag("Panels hide themselves", "The side panels come out when the pointer reaches the window's edge, and go again when it leaves them", |t| &mut t.view.panels_hide),
            Row::Flag("Focus follows the mouse", "The pane under the pointer takes the keyboard without a click: either half of a split file, or either of two files side by side", |t| &mut t.view.focus_follows_mouse),
            Row::Flag("Parameter hints below the line", "A call's parameters, as a language server gives them, shown under the line being typed rather than over it", |t| &mut t.view.hints_below),
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
            Row::Special("Toolbar buttons", Special::ToolbarButtons),
            Row::Flag("Ribbon", "Tabs of large buttons with pictures in place of the menus, as Word and UltraEdit have. Ctrl+F1 folds it to its tabs", |t| &mut t.view.ribbon),
            Row::Flag("Show the start page", "Once every tab is closed: new files, recent work and the guide, rather than an empty window", |t| &mut t.view.start_page),
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
            Row::Special("Spell check code", Special::CodeSpelling),
            Row::Special("Spectrum BASIC listings write graphics as", Special::ZxNotation),
            Row::Special("Assemblers", Special::Assemblers),
            Row::Special("68000 source is for", Special::M68kProcessor),
            Row::Special("With the maths coprocessor (FPU)", Special::M68kFpu),
            Row::Special("With the memory management unit (MMU)", Special::M68kMmu),
            Row::Special("Amiga code hints", Special::AmigaNdk),
            Row::Special("Amiga crunchers and music", Special::AmigaTools),
            Row::Special("Helper for emulators in Limoniter's window", Special::RetroHelper),
            Row::Flag("Check for a newer Limoniter", "Once a day, Limoniter asks GitHub whether a newer version is out, and says so once for each. The Store's Limoniter and the Flatpak are updated by the Store and Flathub instead. Help, Check for a newer Limoniter asks at once", |t| &mut t.prefs.update_checks),
            Row::Flag("Download a newer Limoniter by itself", "When the daily check finds one, it's downloaded in the background and checked against its signature, and Limoniter says when it's ready to restart into. Off, it asks first. Nothing is installed without asking, and never while anything is unsaved", |t| &mut t.prefs.update_download),
            Row::Flag("Keep the OpticOS ROM up to date", "Once a day, Limoniter asks github.com/brixham99/OpticOS for a newer release and runs its ROM in the built-in Spectrum. A ROM whose OpticBASIC keywords differ waits for a newer Limoniter. Help, Check for a newer OpticOS ROM asks at once", |t| &mut t.prefs.opticos_updates),
            Row::Special("Language servers", Special::LanguageServers),
            Row::Special("Your own highlighting grammars", Special::OwnGrammars),
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
            Row::Flag("Check every few seconds too", "For files that change while you watch, such as logs: every two seconds, as well as when the window comes back into focus", |t| &mut t.prefs.watch_often),
            Row::Flag("Reload changed files without asking", "When a file has no unsaved changes of yours. If you were at its end, you stay at the end, to follow a log as it grows", |t| &mut t.prefs.reload_quietly),
            Row::Heading("Saving"),
            Row::Flag("Save files by themselves", "A file with unsaved changes is saved once it has had them for the time below. Untitled and read-only files, files open from a disk image, archive or server, and files of 64 MB and more are left for you to save", |t| &mut t.prefs.autosave),
            Row::Special("Save when", Special::AutosaveWhen),
            Row::Special("Save after", Special::AutosaveTime),
            Row::Flag("Not while the language server reports errors", "A file the language server says has errors in it is left for you to save, so half-written code isn't saved over working code", |t| &mut t.prefs.autosave_skip_errors),
            Row::Flag("Keep unsaved files when quitting", "Quitting with unsaved or never-saved files doesn't ask: they come back as they were the next time Limoniter starts. Turn this off to be asked about each one", |t| &mut t.prefs.hot_exit),
            Row::Flag("Keep a backup when saving", "The first time a file is saved in a session, the file as it was is kept beside it, with .bak added to its name", |t| &mut t.prefs.backup_on_save),
            Row::Heading("Opening"),
            Row::Flag("Open the last project when Limoniter starts", "The project that was open when Limoniter last closed opens again, with its files. Not when Limoniter is started to open a file. Also on the start page", |t| &mut t.prefs.reopen_project),
            Row::Flag("Open CSV and TSV files as tables", "Files ending .csv, .tsv, .tab or .psv go straight into table view", |t| &mut t.prefs.open_tables),
            Row::Flag("Open each file in a new window", "When a file is opened from Explorer. When off, it opens as a tab in the Limoniter window already open", |t| &mut t.prefs.new_window_per_file),
            Row::Special("Format for new files", Special::Preset),
            Row::Special("Kinds of new file (File, New as)", Special::NewKinds),
            Row::Special("Recently opened files", Special::ClearRecent),
            Row::Special("Open Markdown files as", Special::MarkdownView),
            Row::Heading("Comparing"),
            Row::Special("How files are compared", Special::CompareRules),
            Row::Heading("Printing"),
            Row::Flag("Print the file name and page number", "Across the top of each page", |t| &mut t.prefs.print_header),
            Row::Flag("Print line numbers", "Down the left of the text", |t| &mut t.prefs.print_line_numbers),
        ],
    ),
    ("File types", &[Row::Special("Default editor for file types in Windows", Special::FileTypes)]),
];

fn draw_file_types(ui: &mut Ui, label: &str, types: &mut FileTypesState, outcome: &mut Option<Outcome>) {
    ui.label(crate::i18n::tr(label));
    // From the Store, the package says which types Limoniter opens; Windows' own page makes it the default.
    if crate::platform::from_store() {
        ui.label(RichText::new(crate::i18n::tr("Limoniter is offered for text, code, data, BASIC, tape, disk and ANSI files. To make it the one they open in, choose it for each type in Windows' Default apps.")).color(theme::text_dim()));
        if ui.button(crate::i18n::tr("Open Default apps")).clicked() {
            let _ = crate::platform::open_web_page("ms-settings:defaultapps");
        }
        return;
    }
    ui.label(RichText::new(crate::i18n::tr("Windows only lets you change a default yourself. Choose types here, then pick Limoniter for each one on the Windows page that opens.")).color(theme::text_dim()));
    ui.add_space(6.0);
    for (group, names) in associations::COMMON_TYPES {
        ui.label(RichText::new(group).color(theme::text_faint()));
        ui.horizontal_wrapped(|ui| {
            for name in names {
                let mut on = types.ticked.contains(*name);
                if ui.checkbox(&mut on, crate::i18n::tr(name)).changed() {
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
    ui.label(crate::i18n::tr("Other types, separated by commas:"));
    ui.add(egui::TextEdit::singleline(&mut types.others).desired_width(300.0));
    ui.add_space(6.0);
    ui.horizontal_wrapped(|ui| {
        if ui.button(crate::i18n::tr("Set as default...")).on_hover_text(crate::i18n::tr("Registers Limoniter for the ticked types, then opens the Windows page to choose it")).clicked() {
            *outcome = Some(Outcome::SetFileTypes);
        }
        if ui.button(crate::i18n::tr("Open Windows settings")).clicked() {
            *outcome = Some(Outcome::OpenWindowsPage);
        }
        if ui.add_enabled(!types.registered.is_empty(), Button::new(crate::i18n::tr("Remove from Windows"))).on_hover_text(crate::i18n::tr("Takes away everything Limoniter registered")).clicked() {
            *outcome = Some(Outcome::RemoveFileTypes);
        }
    });
    ui.add_space(4.0);
    if types.registered.is_empty() {
        ui.label(RichText::new(crate::i18n::tr("Not registered with Windows yet.")).color(theme::text_faint()));
    } else {
        ui.label(RichText::new(crate::i18n::tf!("Registered with Windows: {}", types.registered.join(", "))).color(theme::text_faint()));
    }
    if !types.program.is_empty() {
        ui.label(RichText::new(crate::i18n::tf!("This copy of the program: {}. If it moves, press Set as default again.", types.program)).color(theme::text_faint()));
    }
    if !types.status.is_empty() {
        ui.add_space(4.0);
        ui.label(types.status.as_str());
    }
}

fn draw_special(ui: &mut Ui, label: &str, kind: Special, target: &mut Target<'_>, outcome: &mut Option<Outcome>) {
    // Wrapped, so a row of many choices (autosave's delays) goes onto a second line rather than past the box's edge.
    ui.horizontal_wrapped(|ui| {
        // The language's row has a globe beside it, found whatever language it's in.
        if kind == Special::Language {
            ui.label(format!("\u{1F310} {}", crate::i18n::tr(label)));
        } else {
            ui.label(crate::i18n::tr(label));
        }
        ui.add_space(8.0);
        match kind {
            Special::Language => {
                // Each language by its own name, so the way back can always be read.
                let mut chosen = target.prefs.language;
                let shown = crate::i18n::LANGUAGES.iter().find(|l| l.code == chosen).map_or("English", |l| l.name);
                egui::ComboBox::from_id_salt("settings-language").selected_text(shown).show_ui(ui, |ui| {
                    for language in &crate::i18n::LANGUAGES {
                        ui.selectable_value(&mut chosen, language.code, language.name);
                    }
                });
                if chosen != target.prefs.language {
                    target.prefs.language = chosen;
                    crate::i18n::set(chosen);
                }
            }
            Special::Theme => {
                if ui.button(crate::i18n::tr("Themes\u{2026}")).on_hover_text(crate::i18n::tr("Window and document themes, and which documents get which. Also under View, Theme.")).clicked() {
                    *outcome = Some(Outcome::Themes);
                }
            }
            Special::Font => {
                let name = target.font.and_then(Path::file_stem).map_or_else(|| "Consolas".to_string(), |stem| stem.to_string_lossy().into_owned());
                ui.label(RichText::new(name).color(theme::text_dim()));
                if ui.button(crate::i18n::tr("Choose...")).clicked() {
                    *outcome = Some(Outcome::ChooseFont);
                }
            }
            Special::TextSize => {
                ui.add(egui::Slider::new(&mut target.view.font_size, 8.0..=40.0).step_by(1.0));
            }
            Special::LineSpacing => {
                ui.add(egui::Slider::new(&mut target.view.line_spacing, crate::views::LINE_SPACINGS).step_by(0.05).custom_formatter(|n, _| format!("{n:.2}\u{D7}")));
            }
            Special::TabSize => {
                ui.add(egui::DragValue::new(&mut target.view.tab_size).range(MIN_TAB_SIZE..=MAX_TAB_SIZE));
            }
            Special::MarkerColumns => {
                // What's typed is kept as it's typed, and taken whenever it reads as columns.
                let id = egui::Id::unique("settings marker columns");
                let mut typed = ui.data(|d| d.get_temp::<String>(id)).unwrap_or_else(|| target.view.marker_columns.to_string());
                let valid = crate::views::MarkerColumns::parse(&typed);
                let edit = egui::TextEdit::singleline(&mut typed).desired_width(140.0).hint_text(crate::i18n::tr("72, 80"));
                let response = ui.add_enabled(target.view.column_markers, edit).on_hover_text(crate::i18n::tr("Up to eight columns, between commas: 7, 72 and 80 for COBOL, 72 for commit messages"));
                if response.changed()
                    && let Some(columns) = crate::views::MarkerColumns::parse(&typed)
                {
                    target.view.marker_columns = columns;
                }
                if valid.is_none() {
                    ui.label(RichText::new(crate::i18n::tr("Numbers from 1 to 9999, up to eight")).color(theme::text_faint()));
                }
                if response.has_focus() {
                    ui.data_mut(|d| d.insert_temp(id, typed));
                } else {
                    ui.data_mut(|d| d.remove::<String>(id));
                }
            }
            Special::AutosaveTime => {
                ui.add_enabled_ui(target.prefs.autosave && target.prefs.autosave_when == AutosaveWhen::AfterDelay, |ui| {
                    for seconds in AUTOSAVE_TIMES {
                        let minutes = seconds / 60;
                        let label = if seconds == 1 { crate::i18n::tr("1 second").to_string() } else if seconds < 60 { crate::i18n::tf!("{seconds} seconds") } else if seconds == 60 { crate::i18n::tr("1 minute").to_string() } else { crate::i18n::tf!("{minutes} minutes") };
                        ui.radio_value(&mut target.prefs.autosave_seconds, seconds, label);
                    }
                });
            }
            Special::AutosaveWhen => {
                ui.add_enabled_ui(target.prefs.autosave, |ui| {
                    for when in AutosaveWhen::ALL {
                        ui.radio_value(&mut target.prefs.autosave_when, when, crate::i18n::tr(when.label()));
                    }
                });
            }
            Special::CodeSpelling => {
                egui::ComboBox::from_id_salt("code-spelling").width(240.0).selected_text(crate::i18n::tr(target.prefs.code_spelling.label())).show_ui(ui, |ui| {
                    for mode in crate::code_spelling::CodeSpelling::ALL {
                        ui.selectable_value(&mut target.prefs.code_spelling, mode, crate::i18n::tr(mode.label()));
                    }
                });
                ui.label(RichText::new(crate::i18n::tr("When Underline mistakes is on")).color(theme::text_faint()));
            }
            Special::HexRow => {
                for row in HEX_ROWS {
                    ui.radio_value(&mut target.prefs.hex_row, row, row.to_string());
                }
            }
            Special::Preset => {
                ui.label(RichText::new(target.new_file_preset).color(theme::text_dim()));
                if ui.button(crate::i18n::tr("Choose...")).clicked() {
                    *outcome = Some(Outcome::ChoosePreset);
                }
            }
            Special::ClearRecent => {
                ui.label(RichText::new(crate::i18n::tf!("{} listed", target.recent_files)).color(theme::text_dim()));
                if ui.add_enabled(target.recent_files > 0, Button::new(crate::i18n::tr("Clear the list"))).clicked() {
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
            Special::LanguageServers => {
                ui.checkbox(target.language_servers, crate::i18n::tr("Use them")).on_hover_text(crate::i18n::tr("Completion, hover, go to definition, uses, renaming and problems for C, C++, Rust, Python, JavaScript, Go and others, from the server for the file's language (rust-analyzer, clangd, pylsp...), started in the project's folder. They're programs of their own, installed separately; Limoniter only starts those listed"));
                if ui.button(crate::i18n::tr("Which\u{2026}")).clicked() {
                    *outcome = Some(Outcome::LanguageServers);
                }
            }
            Special::NewKinds => {
                if ui.button(crate::i18n::tr("Change the list\u{2026}")).on_hover_text(crate::i18n::tr("Each kind is a file ending, an encoding, line endings, colouring, and text to start with")).clicked() {
                    *outcome = Some(Outcome::NewKinds);
                }
            }
            Special::OwnGrammars => {
                ui.vertical(|ui| {
                    let folder = crate::highlight::grammars_folder();
                    let button = Button::new(crate::i18n::tr("Open the folder"));
                    if ui.add_enabled(folder.is_some(), button).on_hover_text(crate::i18n::tr("Put Sublime Text grammars (.sublime-syntax files) here, and Limoniter reads them when it next starts. For a file ending both know, yours is used")).clicked()
                        && let Some(folder) = &folder
                    {
                        let _ = std::fs::create_dir_all(folder);
                        let _ = crate::platform::open_folder(folder);
                    }
                    if let Some(engine) = crate::highlight::engine() {
                        for (file, result) in engine.own_grammars() {
                            let (words, colour) = match result {
                                Ok(language) => (format!("{file}: {language}"), theme::text_dim()),
                                Err(problem) => (format!("{file} wasn't read: {problem}"), theme::problem_mark()),
                            };
                            ui.label(RichText::new(words).color(colour).size(12.0));
                        }
                    }
                });
            }
            Special::Assemblers => {
                if ui.button(crate::i18n::tr("Choose\u{2026}")).on_hover_text(crate::i18n::tr("Limoniter's own assembler, or one you've installed, for Z80, 6502, 6809 and 68000 source. Also under File, Run on, Assemblers")).clicked() {
                    *outcome = Some(Outcome::Assemblers);
                }
            }
            Special::AmigaNdk => {
                if ui.button(crate::i18n::tr("Amiga NDK\u{2026}")).on_hover_text(crate::i18n::tr("System functions' prototypes, offsets, registers and autodocs for 68000 and C source, from Hyperion's NDK for AmigaOS 3.2, downloaded from Aminet when you ask")).clicked() {
                    *outcome = Some(Outcome::AmigaNdk);
                }
            }
            Special::AmigaTools => {
                if ui.button(crate::i18n::tr("Amiga crunchers and music\u{2026}")).on_hover_text(crate::i18n::tr("Shrinkler and Cranker for programs (and crunching what F9 builds), LSPConvert and P61Con for music modules: where they are, and their options")).clicked() {
                    *outcome = Some(Outcome::AmigaTools);
                }
            }
            Special::RetroHelper => {
                if ui.button(crate::i18n::tr("Choose helper\u{2026}")).on_hover_text(crate::i18n::tr("limoniter-retro, which runs libretro cores for machines in Limoniter's window. Without one chosen, it's looked for beside Limoniter and where Get an emulator puts it")).clicked() {
                    *outcome = Some(Outcome::RetroHelper);
                }
            }
            Special::M68kProcessor => {
                let before = target.prefs.m68k_processor;
                egui::ComboBox::from_id_salt("m68k-processor").width(260.0).selected_text(target.prefs.m68k_processor.name()).show_ui(ui, |ui| {
                    for processor in crate::asm::m68k::Processor::ALL {
                        ui.selectable_value(&mut target.prefs.m68k_processor, processor, processor.name());
                    }
                })
                .response
                .on_hover_text(crate::i18n::tr("The processor Limoniter's own assembler takes 68000 source to be for, and its cycle counts are for: the 68008 is the Sinclair QL's, the 68020 the A1200's, the 68030 the A4000/030's and the Falcon's. MACHINE 68020 (or MC68020, CPU32, OPT P=68020) in the source chooses for that file"));
                if target.prefs.m68k_processor != before {
                    crate::asm::m68k::choose(target.prefs.m68k_processor);
                }
            }
            Special::M68kFpu => {
                let before = target.prefs.m68k_fpu;
                let shown = |fpu: crate::asm::m68k::Fpu| if fpu == crate::asm::m68k::Fpu::None { crate::i18n::tr("None").to_string() } else { fpu.name().to_string() };
                egui::ComboBox::from_id_salt("m68k-fpu").width(260.0).selected_text(shown(target.prefs.m68k_fpu)).show_ui(ui, |ui| {
                    for fpu in crate::asm::m68k::Fpu::ALL {
                        ui.selectable_value(&mut target.prefs.m68k_fpu, fpu, shown(fpu));
                    }
                })
                .response
                .on_hover_text(crate::i18n::tr("The maths coprocessor Limoniter's own assembler takes 68000 source to have beside its processor, for FMOVE, FADD and the rest: a 68881 or 68882 as on an A2630, an A3000 or a Falcon with one fitted. The 68040 and 68060 have their own, so need none here. MACHINE 68881 (or FPU 1, OPT P=68020/68881) in the source chooses one for that file"));
                if target.prefs.m68k_fpu != before {
                    crate::asm::m68k::choose_fpu(target.prefs.m68k_fpu);
                }
            }
            Special::M68kMmu => {
                let before = target.prefs.m68k_mmu;
                let shown = |mmu: crate::asm::m68k::Mmu| if mmu == crate::asm::m68k::Mmu::None { crate::i18n::tr("None").to_string() } else { mmu.name().to_string() };
                egui::ComboBox::from_id_salt("m68k-mmu").width(260.0).selected_text(shown(target.prefs.m68k_mmu)).show_ui(ui, |ui| {
                    for mmu in crate::asm::m68k::Mmu::ALL {
                        ui.selectable_value(&mut target.prefs.m68k_mmu, mmu, shown(mmu));
                    }
                })
                .response
                .on_hover_text(crate::i18n::tr("The memory management unit Limoniter's own assembler takes a 68020 to have beside it, for PMOVE, PFLUSH, PTEST and the rest: the 68851, as on an A2620. The 68030, 68040 and 68060 have their own, so need none here. MACHINE 68851 (or PMMU, OPT P=68020/68851) in the source chooses it for that file"));
                if target.prefs.m68k_mmu != before {
                    crate::asm::m68k::choose_mmu(target.prefs.m68k_mmu);
                }
            }
            Special::ZxNotation => {
                egui::ComboBox::from_id_salt("zx-notation").width(260.0).selected_text(crate::i18n::tr(target.prefs.zx_notation.label())).show_ui(ui, |ui| {
                    for notation in crate::zx_graphics::Notation::ALL {
                        ui.selectable_value(&mut target.prefs.zx_notation, notation, crate::i18n::tr(notation.label()));
                    }
                });
            }
            Special::CompareRules => {
                if ui.button(crate::i18n::tr("Rules for comparing files\u{2026}")).on_hover_text(crate::i18n::tr("By name: text leaving out spaces, case, line endings or lines such as timestamps; bytes; pictures; size and time only; or files always left out, such as *.o and target/. Each folder comparison starts with these, and can have its own")).clicked() {
                    *outcome = Some(Outcome::CompareRules);
                }
            }
            Special::ToolbarButtons => {
                if ui.button(crate::i18n::tr("Toolbar buttons\u{2026}")).on_hover_text(crate::i18n::tr("Choose which commands have a button on the toolbar under the menus (View, Toolbar), and put them in order")).clicked() {
                    *outcome = Some(Outcome::ToolbarButtons);
                }
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
            ui.label(RichText::new(crate::i18n::tr(text)).color(theme::text_dim()));
        }
        Row::Flag(label, tip, value) => {
            ui.checkbox(value(target), crate::i18n::tr(label)).on_hover_text(crate::i18n::tr(tip));
        }
        Row::Special(label, kind) => draw_special(ui, label, kind, target, outcome),
    }
}

/// Draws the window. Changes apply straight away, so there's only Close.
pub fn window(ctx: &Context, state: &mut WindowState, target: &mut Target<'_>) -> Option<Outcome> {
    let mut outcome = ctx.input(|i| i.key_pressed(egui::Key::Escape)).then_some(Outcome::Close);
    crate::fit::Dialog::new(crate::i18n::tr("Settings")).collapsible(false).resizable(false).fixed_size(vec2(680.0, 470.0)).anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0]).show_with_buttons(ctx, |ui, part| match part {
        crate::fit::Part::Body => {
            ui.horizontal_top(|ui| {
                ui.vertical(|ui| {
                    ui.set_width(150.0);
                    let field = ui.add(egui::TextEdit::singleline(&mut state.find).hint_text(crate::i18n::tr("Find a setting")).desired_width(150.0));
                    if state.focus {
                        field.request_focus();
                        state.focus = false;
                    }
                    ui.add_space(6.0);
                    for (place, (name, _)) in SECTIONS.iter().enumerate() {
                        let on = state.find.is_empty() && state.section == place;
                        if ui.add(Button::selectable(on, crate::i18n::tr(name)).frame_when_inactive(false).min_size(vec2(150.0, 0.0))).clicked() {
                            state.section = place;
                            state.find.clear();
                        }
                    }
                });
                ui.separator();
                ui.vertical(|ui| {
                    ui.set_width(490.0);
                    // No taller than the screen has room for, so the list scrolls on its own rather than the whole box.
                    ui.set_height(430.0_f32.min(crate::fit::height_left(ui)));
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
                                ui.label(RichText::new(crate::i18n::tr(name)).color(theme::text_dim()));
                                for row in found {
                                    draw_row(ui, row, target, &mut state.file_types, &mut outcome);
                                }
                            }
                        }
                        if !any {
                            ui.label(RichText::new(crate::i18n::tr("No setting matches that.")).color(theme::text_dim()));
                        }
                    });
                });
            });
        }
        crate::fit::Part::Buttons => {
            ui.separator();
            ui.horizontal(|ui| {
                ui.label(RichText::new(crate::i18n::tr("Changes apply straight away.")).color(theme::text_faint()));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.button(crate::i18n::tr("Close")).clicked() {
                        outcome = Some(Outcome::Close);
                    }
                });
            });
        }
    });
    outcome
}

#[cfg(test)]
mod tests {
    use super::*;

    use eframe::egui::RawInput;

    #[test]
    fn a_portable_file_beside_the_program_keeps_everything_in_a_folder_beside_it() {
        let program = std::env::temp_dir().join(format!("limoniter-portable-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&program);
        std::fs::create_dir_all(&program).unwrap();
        assert_eq!(portable_folder_beside(&program), None, "an ordinary copy keeps them in the usual place");
        // A folder of that name isn't the file.
        std::fs::create_dir_all(program.join(PORTABLE_FILE)).unwrap();
        assert_eq!(portable_folder_beside(&program), None);
        std::fs::remove_dir(program.join(PORTABLE_FILE)).unwrap();
        // The file, whatever it says (the download's says what it's for).
        std::fs::write(program.join(PORTABLE_FILE), "This file makes Limoniter portable.").unwrap();
        let folder = portable_folder_beside(&program).unwrap();
        assert_eq!(folder, program.join("Limoniter data"));
        assert!(folder.starts_with(&program));
        // The test program itself has no such file, and isn't taken for portable.
        assert_eq!(portable_folder(), None);
        let _ = std::fs::remove_dir_all(&program);
    }

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
            watch_often: true,
            reload_quietly: true,
            hex_row: 32,
            print_header: false,
            print_line_numbers: true,
            paragraph: ParagraphSetup { width: 60, left: 2, first: 4, align: Align::Justify },
            markdown_view: PreviewView::Full,
            keep_undo_history: false,
            new_window_per_file: true,
            spelling: true,
            code_spelling: crate::code_spelling::CodeSpelling::Names,
            open_tables: true,
            autosave: true,
            autosave_seconds: 300,
            autosave_when: AutosaveWhen::OnWindowChange,
            autosave_skip_errors: true,
            hot_exit: false,
            reopen_project: true,
            backup_on_save: true,
            zx_notation: crate::zx_graphics::Notation::Bas2tap,
            m68k_processor: crate::asm::m68k::Processor::M68008,
            m68k_fpu: crate::asm::m68k::Fpu::M68882,
            m68k_mmu: crate::asm::m68k::Mmu::M68851,
            opticos_updates: false,
            update_checks: false,
            update_download: true,
            language: "fr",
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
            let mut servers = false;
        let mut target = Target { language_servers: &mut servers, view: &mut view, prefs: &mut prefs, file_tabs: &mut tabs, status_bar: &mut status, font: None, new_file_preset: "", recent_files: 0 };
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
        state.carry_out(Outcome::SetFileTypes, |_| Err(crate::i18n::tr("no").to_string()), never, never);
        assert_eq!(state.status, "no");
        assert!(state.registered.is_empty());
        let mut state = page(&["txt"], "");
        state.registered = vec!["txt".to_string()];
        state.carry_out(Outcome::RemoveFileTypes, never_register, || Err(crate::i18n::tr("still no").to_string()), never);
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
            let mut servers = false;
        let mut target = Target { language_servers: &mut servers, view: &mut view, prefs: &mut prefs, file_tabs: &mut tabs, status_bar: &mut status, font: None, new_file_preset: "Windows", recent_files: 3 };
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
        let mut servers = false;
        let mut target = Target { language_servers: &mut servers, view: &mut view, prefs: &mut prefs, file_tabs: &mut tabs, status_bar: &mut status, font: None, new_file_preset: "", recent_files: 0 };
        let mut state = WindowState::default();
        let key = egui::Event::Key { key: egui::Key::Escape, physical_key: None, pressed: true, repeat: false, modifiers: egui::Modifiers::NONE };
        let raw = RawInput { screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, vec2(1200.0, 800.0))), events: vec![key], ..RawInput::default() };
        let mut outcome = None;
        let mut output = ctx.run_ui(raw, |ui| outcome = window(ui.ctx(), &mut state, &mut target));
        output.textures_delta.clear();
        assert_eq!(outcome, Some(Outcome::Close));
    }
}
