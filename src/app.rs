//! The window: ribbon, tabs, status bar, panels, dialogs and file handling.

use crate::associations;
use crate::char_table;
use crate::charset::{self, Charset};
use crate::columns;
use crate::compare;
use crate::formatters;
use crate::project::{self, Project};
use crate::reformat::{self, Align};
use crate::basic;
use crate::preview;
use crate::codec::{self, Certainty, Display, Eol, Lossy, NewlinePolicy, PRESETS, Preset, TextFormat};
use crate::console;
use crate::encoding_prefs::EncodingPrefs;
use crate::encoding_ui::{self, LossyChoice, LossyPurpose, LossyReport, MenuAction, Outcome, Picker, PresetPicker, Purpose};
use crate::find_files::{self, Hit, ReplaceJob, ReplaceReport, SearchJob};
use crate::hex_buffer::{self, HexBuffer};
use crate::highlight::{self, Language};
use crate::line_store::{LineStore, Source};
use crate::navigation::{JumpHistory, Place};
use crate::panels::{self, Action, DocView, Panels, TabInfo};
use crate::print;
use crate::recovery;
use crate::settings::{self, Prefs};
use crate::tabs;
use crate::text_buffer::{self, CaseChange, Justify, ReplaceCursor, SearchCursor, SearchOptions, SearchStep, TextBuffer};
use crate::theme;
use crate::scripting;
use crate::text_objects;
use crate::tools;
use crate::views::{self, ClipState, ContextCommand, Pane, ViewOptions, ViewState};
use eframe::egui::{self, Button, Context, Key, KeyboardShortcut, Modifiers, Panel, RichText, Ui, ViewportCommand};
use rfd::{MessageButtons, MessageDialog, MessageDialogResult, MessageLevel};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::Duration;

/// Text files at least this big stay on disk rather than being read into memory.
const DISK_THRESHOLD: u64 = 64 * 1024 * 1024;
/// The title of Convert to encoding, which a large file's conversion is told apart by when it finishes.
const CONVERT_TITLE: &str = "Convert to encoding";
/// How far into a file it's read to see which processor some assembler is for.
const ASM_SAMPLE_LINES: usize = 300;
/// How long a search or Replace All may run in one frame, so the window stays responsive.
const WORK_SLICE: Duration = Duration::from_millis(30);

#[derive(Clone, Copy, PartialEq, Eq)]
enum Command {
    New,
    Open,
    Save,
    SaveAs,
    CloseTab,
    Undo,
    Redo,
    Cut,
    Copy,
    Paste,
    SelectAll,
    Upper,
    Lower,
    SentenceCase,
    TitleCase,
    Find,
    Replace,
    FindInFiles,
    FindNext,
    FindPrevious,
    GoTo,
    ToggleHex,
    ToggleColumn,
    ColumnFill,
    ColumnCut,
    ColumnDelete,
    JustifyLeft,
    JustifyCenter,
    JustifyRight,
    CsvToFixed,
    FixedToCsv,
    Settings,
    Print,
    LastChange,
    NextChange,
    SplitHorizontal,
    SplitVertical,
    DuplicateWindow,
    FilterSelection,
    CollapseAll,
    ExpandAll,
    HideLines,
    ShowLines,
    MakeSelections,
    Compare,
    TogglePreview,
    SetPreview(settings::PreviewView),
    Reindent,
    WrapToLineEnds,
    LineEndsToWrap,
    ParagraphReformat,
    ParagraphSetup,
    AlignText(Align),
    Format(formatters::Kind),
    ProjectOpen,
    ProjectNew,
    ProjectClose,
    ProjectSettings,
    ProjectAddActive,
    ProjectAddFiles,
    ProjectAddFolder,
    ProjectBackup,
    ProjectRestore,
    ColumnNumber,
    ColumnSum,
    MoveLineUp,
    MoveLineDown,
    Duplicate,
    Join,
    SortAscending,
    SortDescending,
    WordCount,
    TrimLeading,
    TrimTrailing,
    TabsToSpaces,
    SpacesToTabs,
    Indent,
    Outdent,
    GoToBrace,
    SelectToBrace,
    FormatJson,
    CompactJson,
    ComputeHash,
    GenerateGuid,
    ConvertNumber,
    CopyPath,
    ToggleBookmark,
    NextBookmark,
    PreviousBookmark,
    ZoomIn,
    ZoomOut,
    AlwaysOnTop,
    FullScreen,
    PreviousTab,
    NextTab,
    NewInstance,
    Help,
    ReopenAs,
    ConvertTo,
    SaveCopyAs,
    ChoosePreset,
    InsertByte,
    RecoverEdits,
    Comment,
    Uncomment,
    NavBack,
    NavForward,
    ToggleWrap,
    ToggleReadOnly,
    ChooseFont,
    ShowFileLists,
    InsertDateTime,
    InsertDate,
    InsertTime,
    InsertIsoDate,
    InsertFileName,
    InsertFilePath,
    InsertPageBreak,
    InsertColor,
    InsertFile,
    ViewRegular,
    ViewHex,
    ViewEbcdic,
    HexInsertDelete,
    HexCopySelected,
    Toggle(menus::Flag),
    HidePanels,
    CommandPalette,
    About,
    RecordMacro,
    PlayMacro,
    PlayMacroTimes,
    LoadMacro,
    ShowMacros,
    NewScript,
    LoadScript,
    ManageAddOns,
    NewAddOn,
    OpenRemote,
    SaveRemote,
    SendTo(usize),
    RunOnMachine,
    RunOnBuiltIn,
    RunOn(usize),
    StopRunning,
    ChooseAssembler,
    AssembleAndSave,
    AssembleBinary,
    /// A new file of a kind from the New as list, by its place in it.
    NewAs(usize),
    EditNewKinds,
    /// For the label at the cursor in assembler source.
    GoToDefinition,
    FindUses,
    RenameLabel,
    Disassemble,
    NewImage,
    PixelEditor,
    ScreenDesigner,
    ToggleTerminal,
    /// A task, by its place in the list of those here.
    RunTask(usize),
    BuildTask,
    TestTask,
    EditTasks,
    MachineMemory,
    MachineScreen,
    MachineBasicOut,
    MachineBasicIn,
    SourceControl,
    GitWizard,
    GitCompareHead,
    GitHistory,
    GitStageChange,
    GitUndoChange,
    Themes,
    Website,
    ToggleTable,
    SendAgain,
    ManageDevices,
    /// A command an add-on adds, by its place in the list of every add-on's commands.
    AddOn(usize),
    ShowScripts,
    TextObject(text_objects::Action, text_objects::Object),
    Narrow,
    Widen,
    ShowHistory,
    SwapClipboard,
    /// Back through the files in the order they were used, as Ctrl+Tab does, and the other way.
    UsedFileBack,
    UsedFileForward,
    SaveSelection,
    /// Lists a tokenised BASIC program as text in a new document.
    OpenBasicListing,
    /// GRAPH mode on or off, for typing the Spectrum's block graphics and UDGs.
    GraphMode,
    /// Takes the pictures a Spectrum listing's UDGs are drawn with from a file.
    UdgPictures,
    /// Saves the picture a file holds, such as an Amiga IFF or a Spectrum screen, as a PNG.
    SavePictureAsPng,
    /// Opens the list of everything Limoniter can do.
    Features,
    /// Opens the comparison with other editors.
    /// Tokenises the text as BASIC and saves it as a program file.
    SaveSpectrumTape,
    SaveNextProgram,
    SaveCommodoreProgram,
    SaveBbcProgram,
    SplitFile,
    RenameFile,
    ShowInExplorer,
    OpenWithDefault,
    Snippets,
    RunTool,
    StopTool,
    NextMistake,
    /// The history of the selected lines, or of the function the cursor is in.
    PartHistory,
    /// Colour-tags the selected lines, or the function or paragraph the cursor is in.
    Tag(u8),
    Untag,
    NextTag,
    PreviousTag,
    /// Asks Claude Code or Codex to work on the selection, or the function the cursor is in.
    Assist(assistant::Task),
}

thread_local! {
    /// Whether the window is showing no file, for the buttons to grey themselves.
    static NO_FILE: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

impl Command {
    /// Whether it still means something with no file open.
    fn works_without_file(self) -> bool {
        matches!(
            self,
            Command::New
                | Command::Open
                | Command::Help
                | Command::NewInstance
                | Command::RecoverEdits
                | Command::FindInFiles
                | Command::ZoomIn
                | Command::ZoomOut
                | Command::AlwaysOnTop
                | Command::FullScreen
                | Command::ToggleWrap
                | Command::ChooseFont
                | Command::ChoosePreset
                | Command::ShowFileLists
                | Command::StopRunning
                | Command::ChooseAssembler
                | Command::AssembleAndSave
                | Command::AssembleBinary
                | Command::NewAs(_)
                | Command::PixelEditor
                | Command::ScreenDesigner
                | Command::ToggleTerminal
                | Command::EditNewKinds
                | Command::MachineMemory
                | Command::MachineScreen
                | Command::MachineBasicOut
                | Command::SourceControl
                | Command::GitWizard
                | Command::Settings
                | Command::ProjectOpen
                | Command::ProjectNew
                | Command::ProjectClose
                | Command::ProjectSettings
                | Command::ProjectAddFiles
                | Command::ProjectAddFolder
                | Command::ProjectBackup
                | Command::ProjectRestore
                | Command::RecordMacro
                | Command::PlayMacro
                | Command::PlayMacroTimes
                | Command::LoadMacro
                | Command::ShowMacros
                | Command::NewScript
                | Command::LoadScript
                | Command::ManageAddOns
                | Command::NewAddOn
                | Command::OpenRemote
                | Command::ManageDevices
                | Command::Themes
                | Command::Website
                | Command::AddOn(_)
                | Command::ShowScripts
                | Command::ShowHistory
                | Command::Toggle(_)
                | Command::HidePanels
                | Command::CommandPalette
                | Command::About
                | Command::Snippets
                | Command::RunTool
                | Command::StopTool
        )
    }

    /// Whether it would change the document, and so has to wait while the document is locked.
    fn edits(self) -> bool {
        matches!(
            self,
            Command::Undo
                | Command::Redo
                | Command::Cut
                | Command::Paste
                | Command::Upper
                | Command::Lower
                | Command::SentenceCase
                | Command::TitleCase
                | Command::Reindent
                | Command::WrapToLineEnds
                | Command::LineEndsToWrap
                | Command::ParagraphReformat
                | Command::AlignText(_)
                | Command::Format(_)
                | Command::ColumnFill
                | Command::ColumnCut
                | Command::ColumnDelete
                | Command::JustifyLeft
                | Command::JustifyCenter
                | Command::JustifyRight
                | Command::CsvToFixed
                | Command::FixedToCsv
                | Command::ColumnNumber
                | Command::MoveLineUp
                | Command::MoveLineDown
                | Command::Duplicate
                | Command::Join
                | Command::SortAscending
                | Command::SortDescending
                | Command::TrimLeading
                | Command::TrimTrailing
                | Command::TabsToSpaces
                | Command::SpacesToTabs
                | Command::Indent
                | Command::Outdent
                | Command::FormatJson
                | Command::CompactJson
                | Command::GenerateGuid
                | Command::ConvertTo
                | Command::InsertByte
                | Command::Comment
                | Command::Uncomment
                | Command::InsertDateTime
                | Command::InsertDate
                | Command::InsertTime
                | Command::InsertIsoDate
                | Command::InsertFileName
                | Command::InsertFilePath
                | Command::InsertPageBreak
                | Command::InsertColor
                | Command::InsertFile
                | Command::HexInsertDelete
                | Command::SwapClipboard
                | Command::TextObject(text_objects::Action::ReplaceInside | text_objects::Action::DeleteInside | text_objects::Action::DeleteAround, _)
        )
    }

    /// Whether it needs the whole file rather than the part that's narrowed to, and so waits until that's back.
    fn needs_whole_file(self) -> bool {
        matches!(
            self,
            Command::ToggleHex
                | Command::ViewHex
                | Command::ViewEbcdic
                | Command::HexInsertDelete
                | Command::HexCopySelected
                | Command::ReopenAs
                | Command::ConvertTo
                | Command::SaveCopyAs
                | Command::SplitHorizontal
                | Command::SplitVertical
                | Command::DuplicateWindow
                | Command::FilterSelection
                | Command::SplitFile
        )
    }
}

/// Monospaced fonts Windows may have. Only those actually there are offered.
const FONT_CHOICES: [(&str, &str); 10] = [
    ("Consolas", "consola.ttf"),
    ("Cascadia Mono", "CascadiaMono.ttf"),
    ("Cascadia Code", "CascadiaCode.ttf"),
    ("Courier New", "cour.ttf"),
    ("Lucida Console", "lucon.ttf"),
    ("DejaVu Sans Mono", "DejaVuSansMono.ttf"),
    ("Fira Code", "FiraCode-Regular.ttf"),
    ("JetBrains Mono", "JetBrainsMono-Regular.ttf"),
    ("Source Code Pro", "SourceCodePro-Regular.ttf"),
    ("Inconsolata", "Inconsolata-Regular.ttf"),
];

/// The folders the system keeps fonts in, for everyone and for this person.
fn font_folders() -> Vec<PathBuf> {
    let home = std::env::var_os("HOME").map(PathBuf::from);
    let mut folders = Vec::new();
    if cfg!(windows) {
        folders.push(PathBuf::from(r"C:\Windows\Fonts"));
        if let Some(local) = std::env::var_os("LOCALAPPDATA") {
            folders.push(PathBuf::from(local).join("Microsoft").join("Windows").join("Fonts"));
        }
    } else if cfg!(target_os = "macos") {
        folders.extend([PathBuf::from("/Library/Fonts"), PathBuf::from("/System/Library/Fonts")]);
        folders.extend(home.map(|home| home.join("Library").join("Fonts")));
    } else {
        folders.extend([PathBuf::from("/usr/share/fonts"), PathBuf::from("/usr/local/share/fonts")]);
        if let Some(home) = home {
            folders.extend([home.join(".local").join("share").join("fonts"), home.join(".fonts")]);
        }
    }
    folders
}

/// A font file in a folder, or in the folders inside it, as Linux sorts them by maker.
fn find_font(folder: &Path, file: &str, depth: usize) -> Option<PathBuf> {
    let here = folder.join(file);
    if here.is_file() {
        return Some(here);
    }
    if depth == 0 || cfg!(windows) {
        return None;
    }
    std::fs::read_dir(folder).ok()?.flatten().map(|entry| entry.path()).filter(|path| path.is_dir()).find_map(|inner| find_font(&inner, file, depth - 1))
}

fn installed_fonts() -> Vec<(&'static str, PathBuf)> {
    FONT_CHOICES.iter().filter_map(|(name, file)| font_folders().into_iter().find_map(|folder| find_font(&folder, file, 3)).map(|path| (*name, path))).collect()
}

/// Whether every character in a font is the same width, which the editor's columns depend on.
/// A file that isn't really a font fails here, in a throwaway context, rather than in the window.
fn is_fixed_pitch(bytes: Vec<u8>) -> bool {
    std::panic::catch_unwind(move || {
        let ctx = Context::default();
        let mut fonts = egui::FontDefinitions::empty();
        fonts.font_data.insert("probe".into(), Arc::new(egui::FontData::from_owned(bytes)));
        for family in [egui::FontFamily::Monospace, egui::FontFamily::Proportional] {
            fonts.families.insert(family, vec!["probe".into()]);
        }
        ctx.set_fonts(fonts);
        let mut widths = [0.0; 5];
        let raw = egui::RawInput { screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(100.0, 100.0))), ..Default::default() };
        let mut output = ctx.run_ui(raw, |ui| {
            let font = egui::FontId::monospace(14.0);
            widths = ui.ctx().fonts_mut(|f| ['i', 'M', '.', 'W', ' '].map(|c| f.glyph_width(&font, c)));
        });
        output.textures_delta.clear();
        widths[0] > 0.0 && widths.iter().all(|width| (width - widths[0]).abs() < 0.01)
    })
    .unwrap_or(false)
}

/// Every value given for a name in the settings, in order, exactly as written.
fn list_setting(settings: &str, name: &str) -> Vec<String> {
    settings.lines().filter_map(|line| line.split_once('=')).filter(|(key, _)| *key == name).map(|(_, value)| value.to_string()).collect()
}

/// Puts a search at the top of a list, without repeats.
fn note_search(list: &mut Vec<String>, text: &str) {
    if text.is_empty() {
        return;
    }
    list.retain(|known| known != text);
    list.insert(0, text.to_string());
    list.truncate(MAX_SEARCHES);
}

const MAX_SEARCHES: usize = 30;
const MOST_FIND_FROM_SELECTION: usize = 1000;

/// Two panes on one document. They share the cursor, and scroll on their own.
struct Split {
    /// Side by side, rather than one above the other.
    vertical: bool,
    /// How much of the window the first pane has.
    share: f32,
    second: ViewState,
    /// Which pane has the keyboard.
    active: usize,
}

/// Only showing lines that contain some text. Editing is off for as long as it lasts,
/// because a selection would reach into lines that can't be seen.
struct Filter {
    needle: String,
    lines: Vec<usize>,
    was_read_only: bool,
}

/// The lines that contain some text, in order.
fn lines_containing<S: AsRef<str>>(lines: &[S], needle: &str) -> Vec<usize> {
    lines.iter().enumerate().filter(|(_, line)| line.as_ref().contains(needle)).map(|(number, _)| number).collect()
}

/// A second window on a document, while another of its windows is the one in front.
struct HiddenWindow {
    number: usize,
    state: ViewState,
    place: (usize, usize, usize, usize),
}

/// One open file: a text view and a hex view of the same content.
struct Document {
    text: TextBuffer,
    hex: HexBuffer,
    path: Option<PathBuf>,
    untitled: String,
    hex_mode: bool,
    /// Shown as a table of delimited data, and how that's read.
    table: Option<Box<table_view::TableState>>,
    /// Delimited data was found when the file was opened, and table view is offered for it.
    table_offer: Option<crate::table::Dialect>,
    table_checked: bool,
    /// Every line is being changed in the background, written to a new file it will carry on from.
    rewriting: bool,
    /// A document theme chosen for this document alone, by name, for as long as it's open.
    theme_override: Option<String>,
    /// Unsaved changes carried across a switch between text and hex.
    carried: bool,
    text_state: ViewState,
    hex_state: ViewState,
    id: u64,
    /// How this tab shares its space between the text and a preview.
    preview_view: settings::PreviewView,
    /// The picture format its bytes were in when it was loaded, for the preview and Save as PNG.
    picture: Option<crate::pictures::Format>,
    /// Set while only part of the file is showing. `text` then holds just that part.
    narrow: Option<edit_tools::Narrow>,
    language: Language,
    /// The grammar in use, once it's been worked out; None here means not yet.
    resolved_syntax: Option<Option<usize>>,
    /// The file's size and modified time when it was last read or saved.
    disk_stamp: Option<(u64, Option<std::time::SystemTime>)>,
    /// The file's background scan, while it is running.
    scanning: Option<Arc<Source>>,
    saving: Option<SaveJob>,
    /// Where it goes back to, if it came from a server.
    remote: Option<remote_host::RemoteLink>,
    /// Just saved, and to be sent back to its server.
    upload_wanted: bool,
    /// A BASIC listing, with the machine it goes back to as a program and the program's name: sending it sends
    /// the program, tokenised.
    listing_of: Option<(basic::Machine, String)>,
    /// Opened from a Spectrum tape's list of files: a program from it goes back as a tape.
    from_tape: bool,
    /// Where it loads, when the image it came from says (a tape's CODE block).
    load_address: Option<u16>,
    /// Following a running machine: its memory or its screen, looked at again every second.
    live: Option<Box<live_host::LiveView>>,
    /// When autosave first saw it with unsaved changes.
    unsaved_since: Option<std::time::Instant>,
    /// Why autosave is leaving it for the person to save, if it is.
    autosave_skipped: Option<&'static str>,
    /// The file a backup was kept of this session, so it's only kept the first time.
    backed_up: Option<PathBuf>,
    /// This session's own save replaced the file the view reads from.
    saved_over_source: bool,
    /// The encoding was chosen, so a background scan's own finding doesn't replace it.
    format_chosen: bool,
    /// What the last recovery copy was taken of, and the earliest the next may be taken.
    recovery_key: Option<RecoveryKey>,
    recovery_next: std::time::Instant,
    /// Stands in when no file is open, so there's always something to point at. Never shown or edited.
    placeholder: bool,
    split: Option<Split>,
    filter: Option<Filter>,
    /// The stretches hidden by folds, and what that was worked out from.
    fold_view: Vec<views::Hidden>,
    fold_view_of: Option<(u64, std::collections::BTreeMap<usize, usize>)>,
    /// This document's other windows, and the number of the one in front.
    windows: Vec<HiddenWindow>,
    window_number: usize,
    /// For a disk, tape or CD image or an archive: the browser showing in the text's place.
    browser: Option<Box<image_browser::ImageBrowser>>,
}

/// Anything that changes what a recovery copy would hold.
type RecoveryKey = (u64, bool, Eol, usize, bool, Display);

/// One copy found from an earlier run, as offered to the person.
struct OfferItem {
    file: PathBuf,
    selected: bool,
    title: String,
    detail: String,
    note: String,
    content: Option<(recovery::Meta, recovery::Body)>,
}

struct RecoveryOffer {
    orphans: Vec<recovery::Orphan>,
    items: Vec<OfferItem>,
}

/// A save of a file kept on disk, running in the background.
struct SaveJob {
    path: PathBuf,
    written: Arc<AtomicU64>,
    total: u64,
    result: Arc<Mutex<Option<Result<(), String>>>>,
}

impl Drop for Document {
    fn drop(&mut self) {
        if let Some(source) = &self.scanning {
            source.stop();
        }
    }
}

/// A search or Replace All on a big file, done a slice at a time.
enum Pending {
    Find(SearchCursor),
    ReplaceAll(ReplaceCursor),
    HexFind(crate::hex_buffer::HexSearch),
    HexReplaceAll(crate::byte_store::ReplaceAll),
}

/// How much of a file left on disk a search in hex looks through between frames.
const HEX_SLICE: usize = 32 * 1024 * 1024;

impl Document {
    fn new(untitled: String) -> Self {
        static NEXT_ID: AtomicU64 = AtomicU64::new(1);
        Self {
            id: NEXT_ID.fetch_add(1, Ordering::Relaxed),
            preview_view: settings::PreviewView::Text,
            picture: None,
            narrow: None,
            language: Language::Auto,
            resolved_syntax: None,
            disk_stamp: None,
            text: TextBuffer::default(),
            hex: HexBuffer::default(),
            path: None,
            untitled,
            hex_mode: false,
            table: None,
            table_offer: None,
            table_checked: false,
            rewriting: false,
            theme_override: None,
            carried: false,
            text_state: ViewState::default(),
            hex_state: ViewState::default(),
            scanning: None,
            saving: None,
            remote: None,
            upload_wanted: false,
            listing_of: None,
            from_tape: false,
            load_address: None,
            live: None,
            unsaved_since: None,
            autosave_skipped: None,
            backed_up: None,
            saved_over_source: false,
            format_chosen: false,
            recovery_key: None,
            recovery_next: std::time::Instant::now(),
            placeholder: false,
            split: None,
            filter: None,
            fold_view: Vec::new(),
            fold_view_of: None,
            windows: Vec::new(),
            window_number: 1,
            browser: None,
        }
    }

    /// Brings the list of lines showing up to date. A cursor that has landed on a hidden line, as Find and
    /// Go To can do, opens whatever's hiding it.
    fn refresh_fold_view(&mut self) {
        if !self.text.has_folds() {
            self.fold_view_of = None;
            return;
        }
        let (caret, anchor) = (self.text.caret_line, self.text.anchor_line);
        self.text.reveal_line(caret);
        self.text.reveal_line(anchor);
        if !self.text.has_folds() {
            self.fold_view_of = None;
            return;
        }
        let version = self.text.version();
        let current = self.fold_view_of.as_ref().is_some_and(|(known, folds)| *known == version && folds == self.text.folds());
        if !current {
            self.fold_view = views::hidden_by_folds(self.text.folds());
            self.fold_view_of = Some((version, self.text.folds().clone()));
        }
    }

    /// The numbers of this document's windows, in order.
    fn window_numbers(&self) -> Vec<usize> {
        let mut numbers: Vec<usize> = self.windows.iter().map(|window| window.number).collect();
        numbers.push(self.window_number);
        numbers.sort_unstable();
        numbers
    }

    /// Brings one of this document's windows to the front, with the cursor and scroll position it had.
    fn switch_window(&mut self, number: usize) {
        let Some(window) = self.windows.iter_mut().find(|window| window.number == number) else {
            return;
        };
        let text = &mut self.text;
        let place = (text.caret_line, text.caret_col, text.anchor_line, text.anchor_col);
        let last = text.line_count() - 1;
        (text.caret_line, text.caret_col, text.anchor_line, text.anchor_col) = window.place;
        text.caret_line = text.caret_line.min(last);
        text.anchor_line = text.anchor_line.min(last);
        text.reveal = false;
        window.place = place;
        window.number = self.window_number;
        self.window_number = number;
        std::mem::swap(&mut window.state, &mut self.text_state);
        // The colors belong to the document, whichever window is in front.
        std::mem::swap(&mut window.state.colours, &mut self.text_state.colours);
    }

    /// Opens another window on this document, looking at the same place, and brings it to the front.
    fn duplicate_window(&mut self) {
        let number = self.window_numbers().iter().max().map_or(1, |most| most + 1);
        let text = &self.text;
        let place = (text.caret_line, text.caret_col, text.anchor_line, text.anchor_col);
        self.windows.push(HiddenWindow { number, state: self.text_state.same_place(), place });
        self.switch_window(number);
    }

    /// Closes one window. There's always another to take its place, because the last one closes the document.
    fn close_window(&mut self, number: usize) {
        if number == self.window_number
            && let Some(other) = self.windows.first().map(|window| window.number)
        {
            self.switch_window(other);
        }
        self.windows.retain(|window| window.number != number);
    }

    fn set_read_only(&mut self, on: bool) {
        self.text.read_only = on;
        self.hex.read_only = on;
    }

    fn recovery_key_now(&self) -> RecoveryKey {
        let version = if self.hex_mode { self.hex.version() } else { self.text.version() };
        (version, self.hex_mode, self.text.insert_eol(), self.text.format.state, self.text.format.has_bom, self.text.format.display)
    }

    /// Everything needed to bring these edits back. Cheap: the lines themselves are shared.
    fn snapshot(&self) -> recovery::Snapshot {
        let caret = if self.hex_mode {
            (self.hex.caret, 0)
        } else {
            let (line, column) = self.text.caret_char_position();
            (line + self.text.line_offset, column)
        };
        let mut meta = recovery::Meta::new(self.name(), self.path.clone(), &self.text.format, caret);
        meta.file_stamp = self.disk_stamp;
        // While narrowed, a copy still holds the whole file, because that's what would need bringing back.
        let body = if !self.hex_mode {
            recovery::Body::Text(self.whole_store().image())
        } else if let Some(source) = self.hex.source() {
            // A file too large to copy is left where it is; the copy holds only what was changed.
            let (pieces, added) = self.hex.image();
            let source = crate::line_store::SourceRef { path: source.path.clone(), len: source.len, modified: source.modified };
            recovery::Body::HexOnDisk { source, pieces, added }
        } else {
            recovery::Body::Hex(self.hex.bytes())
        };
        recovery::Snapshot { doc_id: self.id, meta, body }
    }

    /// Rebuilds a document from a recovery copy. It comes back as unsaved changes, and the file on disk is left alone.
    fn from_recovered(meta: recovery::Meta, body: recovery::Body) -> Result<Self, String> {
        let format = meta.format()?;
        let mut doc = Self::new(meta.name.clone());
        doc.path.clone_from(&meta.path);
        doc.disk_stamp = meta.path.as_deref().and_then(file_stamp);
        doc.text.format = format;
        charset_widths(&doc.text.format);
        doc.format_chosen = true;
        doc.carried = true;
        match body {
            recovery::Body::Hex(bytes) => {
                doc.hex.load(bytes);
                doc.hex.move_caret(meta.caret.0, false);
                doc.hex_mode = true;
            }
            recovery::Body::HexOnDisk { source, pieces, added } => {
                let file = crate::byte_store::FileBytes::open(&source.path).ok().filter(|_| same_file_as(&source));
                let put_back = file.is_some_and(|file| doc.hex.load_image(file, pieces.clone(), added.clone()));
                if !put_back {
                    // The file has changed since, so only the bytes that were added can be kept, run together.
                    let kept: Vec<u8> = pieces.iter().filter(|p| p.added).flat_map(|p| added[p.offset as usize..p.offset as usize + p.len].iter().copied()).collect();
                    doc.path = None;
                    doc.disk_stamp = None;
                    doc.untitled = format!("{} (added bytes)", meta.name);
                    doc.hex.load(kept);
                    doc.hex.notice = Some(format!("{} has changed since these edits were made, so only the bytes that were typed or pasted in could be kept.", meta.name));
                }
                doc.hex.move_caret(meta.caret.0, false);
                doc.hex_mode = true;
            }
            recovery::Body::Text(image) => {
                let source = match &image.source {
                    None => None,
                    Some(wanted) if same_file_as(wanted) => Some(Source::open_as(&wanted.path, Some(doc.text.format.charset)).map_err(|e| e.to_string())?),
                    Some(_) => return Ok(Self::edited_lines_only(&meta, image, doc)),
                };
                doc.scanning.clone_from(&source);
                let store = crate::line_store::LineStore::from_image(image, source).ok_or("It needs the original file, which can't be read.")?;
                doc.text.adopt_recovered(store, meta.caret);
            }
        }
        Ok(doc)
    }

    /// A very large file's copy only holds what was edited. If the file itself has since changed, the
    /// untouched lines are gone, so what's left is each edited stretch, labelled with where it was.
    fn edited_lines_only(meta: &recovery::Meta, image: crate::line_store::StoreImage, mut doc: Self) -> Self {
        let mut lines = vec![format!("The file {} has changed since these edits were made, so only the edited lines could be kept.", meta.name)];
        let mut position = 0;
        for piece in image.pieces {
            match piece {
                crate::line_store::ImagePiece::Disk { count, .. } => position += count,
                crate::line_store::ImagePiece::Memory(edited) => {
                    lines.push(format!("--- from line {} ---", position + 1));
                    position += edited.len();
                    lines.extend(edited.iter().map(|line| line.to_string()));
                }
            }
        }
        doc.path = None;
        doc.disk_stamp = None;
        doc.untitled = format!("{} (edited lines)", meta.name);
        doc.text.set_content(lines);
        doc
    }

    /// A new, empty document with a preset's settings.
    fn untitled(name: String, preset: &'static Preset) -> Self {
        let mut doc = Self::new(name);
        doc.text.format = preset.format();
        charset_widths(&doc.text.format);
        doc.text.set_content(vec![String::new()]);
        doc.text.set_insert_eol(preset.eol);
        doc
    }

    /// Opens a file, with an encoding chosen for it if there is one, and otherwise working it out.
    fn open(path: &Path, chosen: Option<TextFormat>) -> Result<Self, String> {
        let size = std::fs::metadata(path).map_err(|e| e.to_string())?.len();
        let mut doc = Self::new(String::new());
        doc.path = Some(path.to_path_buf());
        doc.disk_stamp = file_stamp(path);
        let stays_on_disk = match &chosen {
            Some(format) => size >= DISK_THRESHOLD && readable_a_block_at_a_time(format),
            None => size >= DISK_THRESHOLD && is_plain_text_start(path).map_err(|e| e.to_string())?,
        };
        if stays_on_disk {
            let source = Source::open_as(path, chosen.as_ref().map(|format| format.charset)).map_err(|e| e.to_string())?;
            if let Some(mut format) = chosen {
                format.has_bom = source.has_bom();
                charset_widths(&format);
                doc.text.format = format;
                doc.format_chosen = true;
            }
            doc.text.set_source(source.clone());
            doc.scanning = Some(source);
            return Ok(doc);
        }
        // A large binary file, or anything too large to read into memory, is shown in hex where it lies.
        let binary = size >= DISK_THRESHOLD && chosen.is_none() && text_buffer::edit_prefs().nulls_open_in_hex && head_looks_binary(path).map_err(|e| e.to_string())?;
        if binary || size > codec::MAX_FILE_BYTES {
            doc.hex.load_file(crate::byte_store::FileBytes::open(path).map_err(|e| e.to_string())?);
            doc.hex_mode = true;
            if let Some(format) = chosen {
                charset_widths(&format);
                doc.text.format = format;
                doc.format_chosen = true;
            }
            if !binary {
                doc.hex.notice = Some(format!("{} is too large to read as text in its encoding, so it's shown in hex. Text files in UTF-8 or a byte set can be any size.", doc.name()));
            }
            return Ok(doc);
        }
        let bytes = std::fs::read(path).map_err(|e| e.to_string())?;
        doc.load_bytes(bytes, path, chosen, None);
        Ok(doc)
    }

    /// A file taken out of a disk image or archive: read only, named for where it came from, and read as a file
    /// on disk would be, its ending saying what it is. Text with no ending to go by is read in the image's own
    /// preset, if it has one, such as the Amiga's; a program is still binary, and opens in hex.
    fn from_image_file(name: String, bytes: Vec<u8>, image_preset: Option<&'static str>) -> Self {
        let mut doc = Self::new(name.clone());
        let hint = image_preset.and_then(codec::preset_by_id).filter(|_| !codec::looks_binary(&bytes));
        doc.load_bytes(bytes, Path::new(&name), None, hint);
        doc.set_read_only(true);
        doc
    }

    /// Reads a file's bytes as text or, when they look binary and nothing says otherwise, as hex. `hint` is a
    /// preset to suggest when the file's ending doesn't name one.
    fn load_bytes(&mut self, bytes: Vec<u8>, path: &Path, chosen: Option<TextFormat>, hint: Option<&'static codec::Preset>) {
        let doc = self;
        doc.picture = crate::pictures::detect(&path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default(), &bytes);
        let found = codec::detect(&bytes);
        // A retro format named by its file ending is read as that, rather than guessed at or opened as binary.
        let by_ending = if chosen.is_none() && !found.has_bom { codec::preset_for_extension(path).or(hint) } else { None };
        let chosen = chosen.or_else(|| by_ending.map(|preset| TextFormat { certainty: Certainty::Suggested, ..preset.format() }));
        let format = chosen.clone().unwrap_or_else(|| {
            let mut format = TextFormat { charset: found.charset, has_bom: found.has_bom, certainty: found.certainty, ..TextFormat::default() };
            if found.charset.ebcdic {
                format.preset_eol = Eol::Nl;
                if let Some(rows) = codec::ebcdic_rows(&bytes) {
                    format.newline = rows;
                    format.preset_eol = Eol::None;
                }
            }
            format
        });
        doc.text.format = format;
        charset_widths(&doc.text.format);
        doc.format_chosen = chosen.is_some() && by_ending.is_none();
        if chosen.is_none() && text_buffer::edit_prefs().nulls_open_in_hex && codec::looks_binary(&bytes) {
            doc.hex.load(bytes);
            doc.hex_mode = true;
        } else {
            let decoded = codec::decode(&bytes, &mut doc.text.format);
            doc.text.set_decoded(decoded);
        }
    }

    fn name(&self) -> String {
        match &self.path {
            Some(p) => p.file_name().map_or_else(|| p.display().to_string(), |n| n.to_string_lossy().into_owned()),
            None => self.untitled.clone(),
        }
    }

    fn is_modified(&self) -> bool {
        if let Some(narrow) = &self.narrow {
            return self.carried || narrow.outer.is_modified() || self.text.version() != narrow.synced;
        }
        self.carried || if self.hex_mode { self.hex.is_modified() } else { self.text.is_modified() }
    }

    /// The wording for the status bar.
    fn status_file_parts(&self) -> (Option<String>, String, bool) {
        let meta = self.path.as_ref().and_then(|p| std::fs::metadata(p).ok());
        let modified = meta.as_ref().map(|m| format!("Last saved {}", panels::format_time(m.modified())));
        let bytes = meta.as_ref().map_or(0, std::fs::Metadata::len);
        let read_only = meta.as_ref().is_some_and(|m| m.permissions().readonly());
        let size = if self.hex_mode {
            short_size(self.hex.len() as u64)
        } else {
            // This is about the file, so while narrowed it counts all of the file's lines.
            let lines = self.narrow.as_ref().map_or(self.text.line_count(), |narrow| narrow.outer.line_count() + self.text.line_count() - (narrow.bottom - narrow.top + 1));
            format!("{} line{} \u{B7} {}", lines, if lines == 1 { "" } else { "s" }, short_size(bytes))
        };
        (modified, size, read_only)
    }

    /// Keeps the colouring on the language chosen, or the one detected from the file.
    fn update_syntax(&mut self) {
        let Some(engine) = highlight::engine() else {
            return;
        };
        if self.hex_mode || self.scanning.is_some() {
            return;
        }
        let named = self.path_or_name();
        let syntax = *self.resolved_syntax.get_or_insert_with(|| match self.language {
            Language::Off => None,
            Language::Named(syntax) => Some(syntax),
            Language::Auto => {
                let sample: Vec<Arc<str>> = (0..self.text.line_count().min(ASM_SAMPLE_LINES)).map(|i| self.text.line(i)).collect();
                engine.detect_with_sample(Some(&named), &sample)
            }
        });
        self.text_state.colours.set_syntax(syntax);
    }

    /// Where the file is, or for one not saved yet, its name, which is all there is to go by.
    fn path_or_name(&self) -> PathBuf {
        self.path.clone().unwrap_or_else(|| PathBuf::from(&self.untitled))
    }

    fn language_name(&self) -> String {
        match (highlight::engine(), self.resolved_syntax.flatten()) {
            (Some(engine), Some(syntax)) if !self.hex_mode => engine.name_of(syntax).to_string(),
            _ => "No highlighting".to_string(),
        }
    }

    fn set_language(&mut self, language: Language) {
        self.language = language;
        self.resolved_syntax = None;
    }

    /// Whether the document is tied up with its scan or a save, and so can't be edited yet.
    fn is_busy(&self) -> bool {
        self.scanning.is_some() || self.saving.is_some() || self.rewriting
    }

    fn is_blank(&self) -> bool {
        self.placeholder || self.path.is_none() && !self.is_modified() && !self.hex_mode && self.text.line_count() == 1 && self.text.line(0).is_empty()
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Default)]
enum FindTab {
    #[default]
    Find,
    Replace,
    InFiles,
    ReplaceInFiles,
}

#[derive(Clone, Copy, PartialEq, Eq, Default)]
enum FileScope {
    #[default]
    Folder,
    OpenFiles,
    Favorites,
    Project,
}

/// What a Replace in Files preview is ready to do, should the person go ahead.
struct ReplacePlan {
    options: SearchOptions,
    backup: bool,
}

#[derive(Default)]
struct FindState {
    open: bool,
    tab: FindTab,
    focus_box: bool,
    find: String,
    replace: String,
    match_case: bool,
    whole_word: bool,
    regex: bool,
    hex_bytes: bool,
    status: String,
    scope: FileScope,
    /// Earlier searches, newest first, and the ones kept on purpose. Both are kept in the settings file.
    history: Vec<String>,
    favourites: Vec<String>,
    file_types: String,
    folder: String,
    recursive: bool,
    /// Files kept as a place to search, in the settings file.
    favorite_files: Vec<PathBuf>,
    /// Replace in Files keeps a copy of each file it changes on disk.
    keep_backups: bool,
}

/// The About box. Returns whether OK was clicked, and whether the list of what Limoniter can do was asked for.
fn about_page(ui: &mut Ui, notices: Option<&Path>) -> (bool, bool) {
    let (mut ok, mut features) = (false, false);
    ui.set_width(560.0);
    ui.add_space(4.0);
    ui.horizontal_top(|ui| {
        if let Some(logo) = about_logo(ui.ctx()) {
            ui.add(egui::Image::new(&logo).fit_to_exact_size(egui::vec2(124.0, 124.0)));
            ui.add_space(14.0);
        }
        ui.vertical(|ui| {
            ui.add_space(10.0);
            ui.label(RichText::new("Limoniter").size(36.0).strong().color(theme::accent()));
            ui.label(RichText::new(format!("Version {}", env!("CARGO_PKG_VERSION"))).size(12.5).color(theme::text_dim()));
            ui.add_space(8.0);
            ui.add(egui::Label::new(RichText::new("A text and hex editor for files old and new, and an explorer for the disks, tapes and archives of classic computers.").size(15.0).color(theme::strong_text())).wrap());
        });
    });
    ui.add_space(12.0);
    ui.separator();
    ui.add_space(6.0);
    ui.add(
        egui::Label::new(
            RichText::new(
                "Edit text and bytes of any size, in the character sets of machines from mainframes to home computers. Open the disk images, tapes and \
                 archives of the Amiga, Commodore 64, ZX Spectrum, Acorn, Atari ST, Macintosh, CP/M machines and more: browse their files, list their \
                 BASIC programs, convert their word processor documents and see their pictures, then save any of it in today's formats. Nothing is \
                 ever written back to an image.",
            )
            .size(13.5)
            .color(theme::text()),
        )
        .wrap(),
    );
    ui.add_space(12.0);
    // What it knows, counted from the program itself so it stays true.
    let languages = crate::highlight::engine().map_or(0, |engine| engine.names().len());
    let tiles = [
        (crate::charset::all().len().to_string(), "character sets"),
        (if languages > 0 { languages.to_string() } else { "\u{2026}".into() }, "languages coloured"),
        (crate::media::IMAGE_ENDINGS.len().to_string(), "disk & archive formats"),
        (crate::pictures::Format::ALL.len().to_string(), "legacy picture formats"),
    ];
    ui.columns(tiles.len(), |columns| {
        for (column, (number, what)) in columns.iter_mut().zip(&tiles) {
            egui::Frame::new().fill(theme::hover()).stroke(egui::Stroke::new(1.0, theme::border())).corner_radius(8.0).inner_margin(egui::Margin::symmetric(8, 10)).show(column, |ui| {
                ui.set_width(ui.available_width());
                ui.vertical_centered(|ui| {
                    ui.label(RichText::new(number).size(24.0).strong().color(theme::accent()));
                    ui.label(RichText::new(*what).size(11.5).color(theme::text_dim()));
                });
            });
        }
    });
    ui.add_space(14.0);
    ui.horizontal(|ui| {
        ui.label(RichText::new("Created by").size(14.0).color(theme::text_dim()));
        ui.label(RichText::new("Jolyon Ralph").size(14.0).strong().color(theme::strong_text()));
    });
    ui.hyperlink_to(RichText::new("github.com/jolyonralph/limoniter").size(13.0), "https://github.com/jolyonralph/limoniter");
    ui.add_space(10.0);
    let mut licence = "Limoniter is free software under the Apache License 2.0.".to_string();
    if let Some(path) = notices {
        licence += &format!(" The work it's built on, and their licences, are listed in {}", path.display());
    }
    ui.add(egui::Label::new(RichText::new(licence).size(11.5).color(theme::text_faint())).wrap());
    ui.add_space(10.0);
    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
        ok = ui.button(RichText::new("   OK   ")).clicked();
        features = ui.button("What Limoniter can do\u{2026}").on_hover_text("The list of everything Limoniter can do, as a page of its own").clicked();
    });
    (ok, features)
}

/// Limoniter's logo, for the About box: decoded and handed to the graphics card once, then kept.
fn about_logo(ctx: &Context) -> Option<egui::TextureHandle> {
    let id = egui::Id::unique("about-logo");
    if let Some(logo) = ctx.data(|d| d.get_temp::<egui::TextureHandle>(id)) {
        return Some(logo);
    }
    let mut reader = png::Decoder::new(std::io::Cursor::new(&include_bytes!("../data/logo.png")[..])).read_info().ok()?;
    let mut rgba = vec![0; reader.output_buffer_size()?];
    let frame = reader.next_frame(&mut rgba).ok()?;
    rgba.truncate(frame.buffer_size());
    if frame.color_type != png::ColorType::Rgba {
        return None;
    }
    let image = egui::ColorImage::from_rgba_unmultiplied([frame.width as usize, frame.height as usize], &rgba);
    let logo = ctx.load_texture("about-logo", image, egui::TextureOptions::LINEAR);
    ctx.data_mut(|d| d.insert_temp(id, logo.clone()));
    Some(logo)
}

enum Dialog {
    None,
    AddOns { shared: String },
    Remote(Box<remote_host::RemoteBox>),
    Themes(Box<theme_host::ThemeBox>),
    Send(Box<deploy_host::SendBox>),
    Devices(Box<deploy_host::DevicesBox>),
    Run(Box<run_host::RunBox>),
    GitHistory(Box<git_host::HistoryBox>),
    Palette(menus::PaletteState),
    Script(script_host::ScriptBox),
    GoTo { value: String, error: String },
    Settings(settings::WindowState),
    ConfirmReplaceInFiles { files: Vec<PathBuf>, matches: usize },
    /// Choosing what to compare this file with: each other open file, by where it is in the list.
    ComparePick(Vec<(usize, String)>),
    WrapColumn { value: String, error: String, focus: bool },
    ParagraphSetup { width: String, left: String, first: String, align: Align, error: String },
    ProjectSettings { name: String, notes: String },
    RestorePick { backups: Vec<project::Backup>, chosen: usize },
    Compare(Box<compare::Comparison>),
    Fill { value: String, skip_last: bool },
    /// Before saving a BASIC listing as a program: which Commodore machine, or which line a Spectrum program runs from.
    SaveBasic { machine: basic::Machine, run_from: String, error: String },
    ColumnRemove { cut: bool, count: String, error: String, focus: bool },
    CsvToFixed { rows: (usize, usize), delimiter: char, error: String },
    FixedToCsv { rows: (usize, usize), delimiter: char, starts: String, error: String, focus: bool },
    Number { first: String, step: String, error: String },
    Message { title: String, body: String },
    /// The About box: what Limoniter is, who made it, and where the list of what it can do is. The notices file,
    /// when it's beside the program.
    About { notices: Option<PathBuf> },
    Picker(Picker),
    Presets(PresetPicker),
    /// Characters that won't fit, and the settings being aimed for.
    Lossy(LossyReport, TextFormat),
    InsertByte { value: String, error: String },
    HexInsertDelete { count: String, delete: bool, fill: String, error: String, focus: bool },
    /// A table entry from a set that isn't the document's own.
    InsertFrom { set: &'static Charset, table: usize, byte: u8 },
    Recovery(RecoveryOffer),
    Fonts { error: String },
    Color { rgb: [u8; 3], form: usize },
    Rename { name: String, error: String, focus: bool },
    SplitFile { every: String, by_text: bool, text: String, error: String },
    Snippets(file_tools::SnippetBox),
    RunTool { line: String, save_first: bool, error: String, focus: bool },
    Assembler(Box<asm_host::AssemblerBox>),
    NewKinds(Box<new_kinds::NewKindsBox>),
    RenameLabel(Box<intel_host::RenameBox>),
    Disassemble(Box<disasm_host::DisasmBox>),
    NewImage(Box<image_browser::NewImageBox>),
    LanguageServers(Vec<crate::lsp::ServerSetting>),
    Tasks(Vec<crate::tasks::Task>),
    Spelling { found: crate::spelling::Found, chosen: usize },
    AssistReview(assistant::Review),
}

pub struct Limoniter {
    docs: Vec<Document>,
    active: usize,
    untitled_count: usize,
    /// What File, New as offers.
    new_kinds: Vec<new_kinds::NewKind>,
    /// The document and version waiting to be assembled quietly, and since when.
    intel_wait: Option<(u64, u64, std::time::Instant)>,
    /// The character and sprite editor, while it's open.
    pixel_editor: Option<Box<crate::pixel_editor::Editor>>,
    /// The Spectrum screen designer, while it's open.
    screen_designer: Option<Box<crate::screen_designer::Designer>>,
    /// Language servers: whether they're used, which, and those running.
    lsp: lsp_host::LspState,
    /// The terminal panel's shell, while it's open.
    terminal: Option<Box<crate::terminal::Terminal>>,
    /// A task running, and the last test task's results.
    task_run: Option<tasks_host::TaskRun>,
    test_panel: Option<tasks_host::TestPanel>,
    clip: ClipState,
    find: FindState,
    job: Option<Arc<SearchJob>>,
    pending: Option<Pending>,
    pending_share: f32,
    /// Word Count or Save a copy, going through the whole of a file too large to hold in memory.
    big_job: Option<crate::big_jobs::BigJob>,
    /// Files written by changes too large to make in memory, which documents' lines and undo may be read from;
    /// taken away when the window closes.
    /// This run's folder for the files large changes are written to, made when first needed.
    large_edits: Option<crate::large_edits::Folder>,
    show_results: bool,
    dialog: Dialog,
    /// A menu laid open without being clicked, for a picture or a test, and whether its sub-menus are opened out too.
    shown_menu: Option<(usize, bool)>,
    /// Chosen in the command box, to be done in the next frame.
    pending_command: Option<Command>,
    /// Drawing a menu laid open, rather than one opened by a click.
    drawing_open: bool,
    view: ViewOptions,
    show_file_tabs: bool,
    panels: Panels,
    saved_settings: String,
    show_status: bool,
    always_on_top: bool,
    full_screen: bool,
    pending_focus: bool,
    window_title: String,
    allow_close: bool,
    screenshot: Option<ScreenshotRun>,
    encoding_prefs: EncodingPrefs,
    new_file_preset: &'static Preset,
    /// Copies of unsaved edits, written as they happen. None in a screenshot run, or if it couldn't start.
    recovery: Option<recovery::Session>,
    recovery_failed: Option<String>,
    jumps: JumpHistory,
    /// The font file chosen; None uses Consolas.
    font_file: Option<PathBuf>,
    /// A file picked from a menu, to open once the ribbon's finished drawing.
    open_request: Option<PathBuf>,
    /// The last answer to leaving an empty last line alone, for the next fill in this run.
    fill_skip_last: bool,
    prefs: Prefs,
    /// Which theme the window and its frame were last colored for.
    /// The window theme as last applied, to apply it again only when it changes.
    applied_window: Option<std::rc::Rc<theme::WindowTheme>>,
    title_bar_theme: Option<std::rc::Rc<theme::WindowTheme>>,
    /// A print job on its way to the printer, and what's to say about the last one.
    print_job: Option<print::Outcome>,
    print_status: String,
    /// What the results panel's list is of.
    results_title: &'static str,
    replace_plan: Option<ReplacePlan>,
    replace_job: Option<Arc<ReplaceJob>>,
    /// The whole text area was showing a preview last frame, so there's no text there for keys to act on.
    full_preview_shown: bool,
    /// What's being previewed, by document and version, and how it came out.
    preview_of: Option<(u64, u64, preview::Kind)>,
    preview_blocks: Vec<preview::Block>,
    /// A retro screen preview, drawn from the file's exact bytes.
    preview_screen: Option<crate::screen::Screen>,
    /// The picture the preview shows, and it on the graphics card.
    preview_picture: Option<(crate::pictures::Picture, egui::TextureHandle)>,
    /// When the picture's colours started cycling, while they are.
    preview_cycling: Option<f64>,
    /// Why a preview made from bytes has nothing to show, kept until the file changes.
    preview_note: Option<String>,
    project: Option<Project>,
    recent_projects: Vec<PathBuf>,
    /// Backing up or restoring on another thread, and whether it's a restore.
    project_job: Option<(project::Outcome, bool)>,
    /// A macro or script that's running. While it is, nothing else can touch the files.
    script: Option<Box<script_host::ScriptRun>>,
    /// What the person answered, for a script waiting on a box.
    script_answer: Option<scripting::Answer>,
    /// What a search or Replace All on a big file came to, for a script waiting on it.
    pending_count: usize,
    pending_play: Option<(Option<PathBuf>, scripting::Repeat)>,
    current_macro: Option<script_host::CurrentMacro>,
    library: scripting::Library,
    addons: crate::addons::AddOns,
    /// Add-on functions waiting to run, as on_open after a file opens.
    addon_queue: std::collections::VecDeque<addon_host::Queued>,
    addon_panels: Vec<addon_host::AddOnPanel>,
    /// Remote files: jobs under way, sites, trusted server keys, recent files, and sign-ins typed this session.
    remote_jobs: Vec<(remote_host::Purpose, crate::remote::Job)>,
    remote_sites: Vec<remote_host::SavedSite>,
    known_hosts: crate::remote::KnownHosts,
    remote_recent: Vec<String>,
    remote_secrets: std::collections::HashMap<String, crate::remote::Secret>,
    /// Where copies of remote files are kept; None in a test, which uses a temporary folder.
    remote_root: Option<PathBuf>,
    /// The themes there are, which are used, and the rules that pick a document's.
    themes: theme_host::Themes,
    /// Devices files are sent to, and sends under way.
    sends: deploy_host::Sends,
    running: run_host::Running,
    git: git_host::GitState,
    /// The Amiga's pens for ANSI previews, and the table that changes them.
    amiga_palette: crate::ansi_art::AmigaPalette,
    palette_editor: crate::amiga_palette_ui::PaletteEditor,
    /// A send waiting for the add-ons' on_send functions.
    send_pending: Option<deploy_host::PendingSend>,
    script_output: Vec<String>,
    /// Where files' histories are kept between runs. None means they aren't, as in a test.
    history_root: Option<PathBuf>,
    history_job: Option<std::thread::JoinHandle<()>>,
    /// Listening for files opened from Explorer, and any that arrived while a script was running.
    listening: Option<crate::instance::Listening>,
    handed_over: Vec<PathBuf>,
    /// Documents by when they were last in front, latest first, and a Ctrl+Tab switch that's still going.
    used_order: Vec<u64>,
    cycle: Option<(Vec<u64>, usize)>,
    /// Keeps the list of files showing, for a picture of it, as if Ctrl were held.
    hold_cycle: bool,
    snippets: Vec<file_tools::Snippet>,
    run_job: Option<file_tools::RunJob>,
    run_history: Vec<String>,
    split_job: Option<file_tools::SplitJob>,
    speller: Option<crate::spelling::Speller>,
    spell_problem: Option<String>,
    /// Waiting for the spell checker to find the next mistake.
    spell_asked: bool,
    /// For work on other threads to ask for the window to be drawn again.
    waker: Option<Context>,
    /// A part of a file whose history is being shown.
    region: Option<region_panel::RegionState>,
    /// Each open file's colour tags as last written down, by document.
    tags_kept: std::collections::HashMap<u64, Vec<(usize, usize, u8)>>,
    /// Which coding assistant, and the commands for each, and one at work.
    assistant: assistant::Setup,
    assist_job: Option<assistant::AssistJob>,
    /// The Assistant panel, and the history it shows.
    assist_panel: assistant::AssistPanel,
    assist_log: assistant_log::Log,
}

/// A run started with --screenshot: draw for a while, save the window as a picture, and exit.
struct ScreenshotRun {
    path: PathBuf,
    frames: u32,
    requested: bool,
    saved: Arc<AtomicBool>,
    /// What's still to do before the picture, one step at a time, each once the one before has finished working.
    steps: std::collections::VecDeque<String>,
    /// Frames since anything was last done or worked on.
    quiet: u32,
}

const SCREENSHOT_FRAMES: u32 = 120;
const SCREENSHOT_STEPS_AT: u32 = 5;

fn save_png(path: &Path, image: &egui::ColorImage) -> Result<(), String> {
    if let Some(dir) = path.parent().filter(|d| !d.as_os_str().is_empty()) {
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    let file = std::fs::File::create(path).map_err(|e| e.to_string())?;
    let mut encoder = png::Encoder::new(std::io::BufWriter::new(file), image.size[0] as u32, image.size[1] as u32);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    let mut writer = encoder.write_header().map_err(|e| e.to_string())?;
    writer.write_image_data(image.as_raw()).map_err(|e| e.to_string())?;
    writer.finish().map_err(|e| e.to_string())
}

impl Limoniter {
    pub fn new(cc: &eframe::CreationContext<'_>, files: Vec<PathBuf>, screenshot: Option<PathBuf>) -> Self {
        let settings = read_settings_file();
        theme::set_window(theme_host::Themes::from_settings(&settings, Prefs::from_settings(&settings).light).window_theme());
        theme::apply(&cc.egui_ctx);
        theme::colour_title_bar(cc);
        #[cfg(target_os = "macos")]
        crate::mac_open::set_window(cc.egui_ctx.clone());
        highlight::start_loading(cc.egui_ctx.clone());
        let font_file = list_setting(&settings, "font_file").pop().map(PathBuf::from).filter(|path| path.exists());
        install_fonts(&cc.egui_ctx, font_file.as_deref());
        let mut app = Self::build(settings, font_file, screenshot);
        for file in &files {
            app.open_path(file);
        }
        if app.docs.is_empty() {
            app.new_document();
        }
        if app.screenshot.is_none() {
            app.start_recovery();
        }
        app.start_listening(&cc.egui_ctx);
        app
    }

    /// The app itself, with nothing open yet, whatever window it's going to be in.
    fn build(settings: String, font_file: Option<PathBuf>, screenshot: Option<PathBuf>) -> Self {
        crate::ansi_art::set_amiga_palette(crate::ansi_art::AmigaPalette::from_settings(&settings));
        let mut view = ViewOptions::from_settings(&settings);
        // The column ruler was one of the things that made it look like the program it began as. It goes once, with
        // the new design, and stays wherever it's chosen after that.
        if !settings.lines().any(|line| line.trim() == "design=2") {
            view.ruler = false;
        }
        let prefs = Prefs::from_settings(&settings);
        text_buffer::set_tab_size(view.tab_size);
        text_buffer::set_source_view(view.source_view);
        let mut panels = Panels::default();
        panels.read_settings(&settings);
        panels.recent = list_setting(&settings, "recent_file").into_iter().map(PathBuf::from).collect();
        let new_file_preset = preset_from_settings(&settings);
        let recent_projects = list_setting(&settings, "recent_project").into_iter().map(PathBuf::from).collect();
        // Beside the settings, unless this is a test or a screenshot run with nowhere of its own to write.
        let script_home = if cfg!(test) || (screenshot.is_some() && std::env::var_os("LIMONITER_SETTINGS").is_none()) { None } else { settings_path().and_then(|path| path.parent().map(Path::to_path_buf)) };
        let assist_log = assistant_log::Log::open(script_home.as_deref());
        let history_root = script_home.as_ref().map(|home| home.join("history"));
        let (remote_sites, known_hosts, remote_recent) = remote_host::read_remote_settings(&settings);
        let remote_root = script_home.as_ref().map(|home| home.join("remote-copies"));
        let mut addons = crate::addons::AddOns::new(script_home.clone());
        addons.read_settings(&list_setting(&settings, "addon_on"), list_setting(&settings, "addons_shared").first().map(String::as_str));
        addons.refresh();
        let mut library = scripting::Library::new(script_home);
        library.read_settings(&list_setting(&settings, "script_key"));
        let current_macro = list_setting(&settings, "current_macro").into_iter().next().map(PathBuf::from).filter(|path| path.is_file()).map(|path| script_host::CurrentMacro { name: path.file_stem().map_or_else(String::new, |name| name.to_string_lossy().into_owned()), path: Some(path), source: String::new() });
        Self {
            script: None,
            script_answer: None,
            pending_count: 0,
            pending_play: None,
            current_macro,
            library,
            addons,
            addon_queue: std::collections::VecDeque::new(),
            addon_panels: Vec::new(),
            remote_jobs: Vec::new(),
            remote_sites,
            known_hosts,
            remote_recent,
            remote_secrets: std::collections::HashMap::new(),
            remote_root,
            sends: deploy_host::Sends::from_settings(&settings),
            running: run_host::Running::from_settings(&settings),
            git: git_host::GitState::from_settings(&settings),
            amiga_palette: crate::ansi_art::AmigaPalette::from_settings(&settings),
            palette_editor: Default::default(),
            themes: theme_host::Themes::from_settings(&settings, Prefs::from_settings(&settings).light),
            send_pending: None,
            script_output: Vec::new(),
            history_root,
            history_job: None,
            listening: None,
            handed_over: Vec::new(),
            used_order: Vec::new(),
            cycle: None,
            hold_cycle: false,
            snippets: file_tools::snippets_from_settings(&settings),
            run_job: None,
            run_history: list_setting(&settings, "run_command"),
            split_job: None,
            speller: None,
            spell_problem: None,
            spell_asked: false,
            waker: None,
            region: None,
            tags_kept: std::collections::HashMap::new(),
            assistant: assistant::Setup::from_settings(&settings),
            assist_job: None,
            assist_panel: assistant::AssistPanel::default(),
            assist_log,
            docs: Vec::new(),
            active: 0,
            untitled_count: 0,
            new_kinds: new_kinds::kinds_from_settings(&settings),
            intel_wait: None,
            pixel_editor: None,
            screen_designer: None,
            lsp: lsp_host::LspState::from_settings(&settings),
            terminal: None,
            task_run: None,
            test_panel: None,
            clip: ClipState::default(),
            find: FindState { recursive: true, file_types: "*.*".into(), history: list_setting(&settings, "find_history"), favourites: list_setting(&settings, "find_favourite"), favorite_files: list_setting(&settings, "favorite_file").into_iter().map(PathBuf::from).collect(), keep_backups: true, ..FindState::default() },
            job: None,
            pending: None,
            pending_share: 0.0,
            big_job: None,
            large_edits: None,
            show_results: false,
            dialog: Dialog::None,
            shown_menu: None,
            pending_command: None,
            drawing_open: false,
            view,
            show_file_tabs: setting_flag(&settings, "file_tabs", true),
            show_status: setting_flag(&settings, "status_bar", true),
            panels,
            saved_settings: settings,
            always_on_top: false,
            full_screen: false,
            pending_focus: true,
            window_title: String::new(),
            allow_close: screenshot.is_some(),
            encoding_prefs: EncodingPrefs::load(encoding_prefs_path()),
            new_file_preset,
            recovery: None,
            recovery_failed: None,
            jumps: JumpHistory::default(),
            font_file,
            open_request: None,
            fill_skip_last: true,
            prefs,
            applied_window: None,
            title_bar_theme: None,
            print_job: None,
            print_status: String::new(),
            results_title: "Find in Files",
            replace_plan: None,
            replace_job: None,
            full_preview_shown: false,
            preview_of: None,
            preview_blocks: Vec::new(),
            preview_screen: None,
            preview_picture: None,
            preview_cycling: None,
            preview_note: None,
            project: None,
            recent_projects,
            project_job: None,
            screenshot: screenshot.map(|path| ScreenshotRun { path, frames: 0, requested: false, saved: Arc::new(AtomicBool::new(false)), steps: std::collections::VecDeque::new(), quiet: 0 }),
        }
    }

    // ---------- Crash recovery ----------

    /// Starts keeping copies of edits, and offers back anything an earlier run left behind.
    fn start_recovery(&mut self) {
        let Some(root) = recovery_root() else {
            self.recovery_failed = Some("There's nowhere to keep them".to_string());
            return;
        };
        match recovery::Session::start(&root) {
            Ok(session) => self.recovery = Some(session),
            Err(problem) => self.recovery_failed = Some(problem.to_string()),
        }
        // Copies of large files a crashed run left behind, which can be as large as the files, unless recovery
        // still reads from them.
        let large = self.large_edits_root();
        let recovery_root = root.clone();
        std::thread::spawn(move || crate::large_edits::sweep(&large, &crate::recovery::sources_named(&recovery_root)));
        self.offer_recovered(&root, false);
    }

    fn offer_recovered(&mut self, root: &Path, asked: bool) {
        let (orphans, found) = recovery::find_orphans(root);
        if found.is_empty() {
            recovery::tidy(orphans);
            if asked {
                self.message("Recover edits", "There are no unsaved edits waiting to be recovered.".into());
            }
            return;
        }
        let items = found.into_iter().map(offer_item).collect();
        self.dialog = Dialog::Recovery(RecoveryOffer { orphans, items });
        self.pending_focus = false;
    }

    /// Hands over a copy of anything that's changed. The writing happens on another thread.
    fn recovery_tick(&mut self, ctx: &Context) {
        if let Some(session) = self.recovery.as_mut()
            && let Some(wait) = offer_changes(session, &mut self.docs)
        {
            ctx.request_repaint_after(wait);
        }
    }

    /// The window is failing: writes the very latest of everything.
    fn emergency_save(&mut self) {
        if let Some(session) = self.recovery.as_mut() {
            save_everything_now(session, &self.docs);
        }
    }

    /// Carries out the person's choice about copies from an earlier run.
    fn recovery_dialog(&mut self, ctx: &Context) -> bool {
        let Dialog::Recovery(offer) = &mut self.dialog else {
            return false;
        };
        let (mut recover, mut later, mut discard) = (false, false, false);
        egui::Window::new("Recover unsaved edits").collapsible(false).resizable(false).anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0]).show(ctx, |ui| {
            ui.set_width(620.0);
            ui.label("Limoniter didn't close normally last time. These unsaved edits were kept.");
            ui.label(RichText::new("Recovering opens them as unsaved changes. The files on disk aren't touched until you save.").size(12.5).color(theme::text_dim()));
            ui.add_space(6.0);
            egui::ScrollArea::vertical().id_salt("recovery-list").max_height(360.0).auto_shrink([false, true]).show(ui, |ui| {
                for item in &mut offer.items {
                    ui.horizontal_top(|ui| {
                        ui.add_enabled(item.content.is_some(), egui::Checkbox::without_text(&mut item.selected));
                        ui.vertical(|ui| {
                            ui.label(RichText::new(&item.title).strong());
                            ui.label(RichText::new(&item.detail).size(12.0).color(theme::text_dim()));
                            if !item.note.is_empty() {
                                ui.label(RichText::new(&item.note).size(12.0).color(theme::ruler_mark()));
                            }
                        });
                    });
                    ui.add_space(4.0);
                }
            });
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                recover = ui.button("Recover selected").clicked();
                later = ui.button("Decide later").on_hover_text("Keeps them. They're offered again next time, or from File, Recover unsaved edits").clicked();
                discard = ui.button("Discard selected...").clicked();
            });
        });
        if later || ctx.input(|i| i.key_pressed(Key::Escape)) {
            self.finish_offer();
        } else if discard {
            let count = offer.items.iter().filter(|item| item.selected || item.content.is_none()).count();
            if ask_yes_no("Recover unsaved edits", &format!("Permanently discard {count} recovery cop{}?", if count == 1 { "y" } else { "ies" })) {
                let chosen: Vec<PathBuf> = offer.items.iter().filter(|item| item.selected || item.content.is_none()).map(|item| item.file.clone()).collect();
                self.finish_offer();
                for file in chosen {
                    let _ = recovery::delete_copy(&file);
                }
                self.tidy_after_offer();
            }
        } else if recover {
            self.recover_selected();
        }
        true
    }

    fn tidy_after_offer(&mut self) {
        if let Some(root) = recovery_root() {
            let (orphans, _) = recovery::find_orphans(&root);
            recovery::tidy(orphans);
        }
    }

    /// Closes the offer, leaving the copies where they are.
    fn finish_offer(&mut self) {
        if let Dialog::Recovery(offer) = std::mem::replace(&mut self.dialog, Dialog::None) {
            recovery::tidy(offer.orphans);
        }
        self.pending_focus = true;
    }

    fn recover_selected(&mut self) {
        let Dialog::Recovery(offer) = std::mem::replace(&mut self.dialog, Dialog::None) else {
            return;
        };
        let mut first = None;
        for item in offer.items {
            let (file, selected, title) = (item.file, item.selected, item.title);
            let Some((meta, body)) = item.content.filter(|_| selected) else {
                continue;
            };
            match Document::from_recovered(meta, body) {
                Ok(mut doc) => {
                    doc.preview_view = self.opening_view(doc.path.as_deref(), doc.picture.is_some());
                    // The old copy only goes once a new one is safely on the disk.
                    let kept = self.recovery.as_mut().is_some_and(|session| session.write_now(&doc.snapshot()).is_ok());
                    if self.docs.len() == 1 && self.docs[0].is_blank() {
                        self.docs.clear();
                    }
                    let same_tab = doc.path.as_deref().and_then(|path| self.docs.iter().position(|open| !open.is_modified() && open.path.as_deref().is_some_and(|p| same_path(p, path))));
                    let place = if let Some(index) = same_tab {
                        self.docs[index] = doc;
                        index
                    } else {
                        self.docs.push(doc);
                        self.docs.len() - 1
                    };
                    first = first.or(Some(place));
                    if kept {
                        let _ = recovery::delete_copy(&file);
                    }
                }
                Err(problem) => warn(&format!("Couldn't recover {title}.\n\n{problem}\n\nThe copy has been kept at {}.", file.display())),
            }
        }
        recovery::tidy(offer.orphans);
        if self.docs.is_empty() {
            self.new_document();
        }
        if let Some(index) = first {
            self.active = index;
        }
        self.pending_focus = true;
    }

    fn doc(&mut self) -> &mut Document {
        &mut self.docs[self.active]
    }

    /// Whether the window is showing no file at all.
    fn no_file(&self) -> bool {
        self.docs.len() == 1 && self.docs[0].placeholder
    }

    fn new_document(&mut self) {
        if self.no_file() {
            self.docs.clear();
        }
        self.untitled_count += 1;
        self.docs.push(Document::untitled(format!("Edit{}", self.untitled_count), self.new_file_preset));
        self.active = self.docs.len() - 1;
        self.pending_focus = true;
    }

    fn open_dialog(&mut self) {
        if let Some(files) = with_open_filters(rfd::FileDialog::new().set_title("Open")).pick_files() {
            for file in files {
                self.open_path(&file);
            }
        }
    }

    /// How a file starts out: Markdown as chosen in Settings, and anything else as text.
    /// How a file opens: a picture beside its bytes, Markdown as the settings say, anything else as text.
    fn opening_view(&self, path: Option<&Path>, picture: bool) -> settings::PreviewView {
        if picture {
            settings::PreviewView::Split
        } else if preview::kind_for(path, "").is_some() {
            self.prefs.markdown_view
        } else {
            settings::PreviewView::Text
        }
    }

    /// The preview a document gets: a picture if its bytes are one, otherwise what its name and reading say.
    fn preview_kind(doc: &Document) -> Option<preview::Kind> {
        if doc.picture.is_some() {
            return Some(preview::Kind::Picture);
        }
        preview::kind_for_file(Some(&doc.path_or_name()), &doc.language_name(), &doc.text.format)
    }

    fn open_path(&mut self, path: &Path) -> bool {
        let full = std::path::absolute(path).unwrap_or_else(|_| path.to_path_buf());
        if let Some(i) = self.docs.iter().position(|d| d.path.as_deref().is_some_and(|p| same_path(p, &full))) {
            self.active = i;
            self.pending_focus = true;
            panels::note_recent(&mut self.panels.recent, &full);
            return true;
        }
        // Disk, tape and CD images and archives open in a browser, unless it turns out they can't be read as one.
        if self.encoding_prefs.get(&full).is_none() && self.open_image(&full) {
            self.pending_focus = true;
            return true;
        }
        match Document::open(&full, self.encoding_prefs.get(&full)) {
            Ok(mut doc) => {
                panels::note_recent(&mut self.panels.recent, &full);
                // A file Windows has marked read-only opens locked; the switch on the Edit tab unlocks it.
                if std::fs::metadata(&full).is_ok_and(|meta| meta.permissions().readonly()) {
                    doc.set_read_only(true);
                }
                doc.preview_view = self.opening_view(Some(&full), doc.picture.is_some());
                self.recall_history(&mut doc, &full);
                self.recall_tags(&mut doc, &full);
                // A blank, untouched only tab gives way to the file.
                if self.docs.len() == 1 && self.docs[0].is_blank() {
                    self.docs.clear();
                }
                let id = doc.id;
                let text = !doc.hex_mode;
                self.docs.push(doc);
                self.active = self.docs.len() - 1;
                self.pending_focus = true;
                if text {
                    self.queue_hooks(crate::addons::Event::Open, id);
                }
                true
            }
            Err(e) => {
                warn(&format!("Couldn't open {}.\n\n{e}", full.display()));
                false
            }
        }
    }

    /// The bytes to save or show in hex. Anything that won't fit is listed for the person,
    /// and nothing is written or changed until they've dealt with it.
    fn bytes_for(&mut self, index: usize) -> Option<Vec<u8>> {
        let doc = &mut self.docs[index];
        if doc.hex_mode {
            return Some(doc.hex.bytes().to_vec());
        }
        if doc.text.format.ensure_final_newline {
            doc.text.ensure_final_newline();
        }
        let format = doc.text.format.clone();
        match doc.text.encode(&format, Lossy::Refuse) {
            Ok(bytes) => Some(bytes),
            Err(problems) => {
                self.active = index;
                let report = LossyReport { purpose: LossyPurpose::Save, target_name: format.encoding_name(), problems };
                self.dialog = Dialog::Lossy(report, format);
                None
            }
        }
    }

    fn save(&mut self, index: usize) -> bool {
        if self.docs[index].browser.is_some() {
            self.message("Save", "An image or archive is only read here, so nothing is saved over it. Save a copy takes a file out of it.".into());
            return false;
        }
        match self.docs[index].path.clone() {
            Some(path) => self.write_to(index, &path),
            None => self.save_as(index),
        }
    }

    fn save_as(&mut self, index: usize) -> bool {
        if self.docs[index].browser.is_some() {
            self.message("Save as", "An image or archive is only read here. Save a copy, in the browser, takes a file out of it.".into());
            return false;
        }
        let name = self.docs[index].name();
        match rfd::FileDialog::new().set_title("Save As").set_file_name(name).save_file() {
            Some(path) => self.write_to(index, &path),
            None => false,
        }
    }

    fn write_to(&mut self, index: usize, path: &Path) -> bool {
        if self.docs[index].narrow.is_some() {
            return self.write_narrowed(index, path);
        }
        if self.docs[index].saving.is_some() {
            return false;
        }
        if !self.docs[index].hex_mode && self.docs[index].text.is_disk_backed() {
            return self.save_disk_backed(index, path);
        }
        if self.docs[index].hex_mode && self.docs[index].hex.is_on_disk() {
            return self.save_disk_hex(index, path);
        }
        if !self.docs[index].hex_mode {
            let uneven = self.docs[index].text.uneven_rows();
            if let Some(first) = uneven.first() {
                let question = format!("Row {} and {} other row(s) aren't the width this format needs, which would shift everything after them.\n\nSave anyway?", first + 1, uneven.len() - 1);
                if !ask_yes_no("Row widths", &question) {
                    return false;
                }
            }
        }
        let Some(bytes) = self.bytes_for(index) else {
            return false;
        };
        self.keep_backup(index, path);
        if let Err(e) = codec::write_file(path, &bytes) {
            warn(&format!("Couldn't save {}.\n\n{e}", path.display()));
            return false;
        }
        panels::note_recent(&mut self.panels.recent, path);
        let doc = &mut self.docs[index];
        note_saved_to(doc, path);
        doc.path = Some(path.to_path_buf());
        doc.resolved_syntax = None;
        doc.disk_stamp = file_stamp(path);
        doc.carried = false;
        if doc.hex_mode {
            doc.hex.mark_saved();
        } else {
            doc.text.mark_saved();
        }
        self.keep_history(index, &bytes);
        true
    }

    /// The first time a file is saved over in a session, keeps it as it was beside it, with .bak added, if asked.
    fn keep_backup(&mut self, index: usize, path: &Path) {
        let doc = &mut self.docs[index];
        if !self.prefs.backup_on_save || doc.backed_up.as_deref().is_some_and(|done| same_path(done, path)) || !path.is_file() {
            return;
        }
        let mut backup = path.as_os_str().to_owned();
        backup.push(".bak");
        match std::fs::copy(path, &backup) {
            Ok(_) => doc.backed_up = Some(path.to_path_buf()),
            Err(e) => warn(&format!("Couldn't keep a backup of {} before saving.\n\n{e}\n\nIt will still be saved.", path.display())),
        }
    }

    /// Saves files that have had unsaved changes for the time asked, if autosave is on. A file is left for the
    /// person to save when saving it would need a question answered, or would mean saving over something else.
    fn autosave(&mut self, ctx: &Context) {
        // A screenshot never saves over a file; tests run as one.
        if !self.prefs.autosave || (self.screenshot.is_some() && !cfg!(test)) || self.script.is_some() {
            for doc in &mut self.docs {
                doc.unsaved_since = None;
                doc.autosave_skipped = None;
            }
            return;
        }
        let wait = std::time::Duration::from_secs(u64::from(self.prefs.autosave_seconds));
        let now = std::time::Instant::now();
        for index in 0..self.docs.len() {
            let doc = &mut self.docs[index];
            if !doc.is_modified() || doc.is_busy() {
                doc.unsaved_since = None;
                if !doc.is_modified() {
                    doc.autosave_skipped = None;
                }
                continue;
            }
            if let Some(never) = autosave_never(doc) {
                doc.autosave_skipped = Some(never);
                doc.unsaved_since = None;
                continue;
            }
            let since = *doc.unsaved_since.get_or_insert(now);
            let left = wait.saturating_sub(now.duration_since(since));
            if !left.is_zero() {
                ctx.request_repaint_after(left);
                continue;
            }
            // Something that would need asking is looked at again after another wait.
            doc.autosave_skipped = autosave_not_now(doc);
            doc.unsaved_since = doc.autosave_skipped.map(|_| now);
            if doc.autosave_skipped.is_some() {
                ctx.request_repaint_after(wait);
                continue;
            }
            let path = doc.path.clone().expect("checked above");
            if self.write_to(index, &path) {
                self.print_status = format!("Saved {} by itself", self.docs[index].name());
            }
        }
    }

    /// Saves a file kept on disk in the background, so the window keeps working.
    /// Untouched stretches are copied byte for byte, and the original stays until the copy is complete.
    fn save_disk_backed(&mut self, index: usize, path: &Path) -> bool {
        let doc = &mut self.docs[index];
        if let Err(problems) = doc.text.store().check_encodable(&doc.text.format) {
            let format = doc.text.format.clone();
            self.active = index;
            let report = LossyReport { purpose: LossyPurpose::Save, target_name: format.encoding_name(), problems };
            self.dialog = Dialog::Lossy(report, format);
            return false;
        }
        if let Some(source) = doc.text.store().source()
            && same_path(source.path(), path)
            && source.changed_on_disk()
            && doc.path.as_deref().is_some_and(|p| same_path(p, source.path()))
            && !doc.saved_over_source
        {
            let question = format!("{} has been changed by another program since it was opened.\n\nOverwrite it?", path.display());
            if !ask_yes_no("File changed", &question) {
                return false;
            }
        }
        let store = doc.text.store().clone();
        let format = doc.text.format.clone();
        let written = Arc::new(AtomicU64::new(0));
        let result = Arc::new(Mutex::new(None));
        let total = store.source().map_or(0, |s| s.file_size());
        let (target, worker_written, worker_result) = (path.to_path_buf(), written.clone(), result.clone());
        std::thread::spawn(move || {
            // If this fails, the window is told, and the original file is still as it was.
            let attempt = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| store.write_file(&target, &format, &worker_written).map_err(|e| e.to_string())));
            *worker_result.lock().unwrap() = Some(attempt.unwrap_or_else(|_| Err("Saving failed unexpectedly.".to_string())));
        });
        doc.saving = Some(SaveJob { path: path.to_path_buf(), written, total, result });
        true
    }

    /// Saves a large file open in hex in the background. Untouched stretches are copied from the file as they are,
    /// and the original stays until the copy is complete.
    fn save_disk_hex(&mut self, index: usize, path: &Path) -> bool {
        let doc = &mut self.docs[index];
        let over_itself = doc.path.as_deref().is_some_and(|p| same_path(p, path));
        if over_itself && doc.disk_stamp.is_some() && file_stamp(path) != doc.disk_stamp {
            let question = format!("{} has been changed by another program since it was opened.\n\nOverwrite it?", path.display());
            if !ask_yes_no("File changed", &question) {
                return false;
            }
        }
        let store = doc.hex.snapshot();
        let written = Arc::new(AtomicU64::new(0));
        let result = Arc::new(Mutex::new(None));
        let total = store.len() as u64;
        let (target, worker_written, worker_result) = (path.to_path_buf(), written.clone(), result.clone());
        std::thread::spawn(move || {
            let attempt = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| store.write_file(&target, &worker_written).map_err(|e| e.to_string())));
            *worker_result.lock().unwrap() = Some(attempt.unwrap_or_else(|_| Err("Saving failed unexpectedly.".to_string())));
        });
        doc.saving = Some(SaveJob { path: path.to_path_buf(), written, total, result });
        true
    }

    /// One level of indent, as the Tab key would make it.
    fn indent_unit(&self) -> String {
        if self.prefs.edit.spaces_for_tab { " ".repeat(self.view.tab_size) } else { "\t".to_string() }
    }

    /// These commands act on whole lines, so several selections have to be ended first. Says so if they aren't.
    fn one_selection(&mut self, title: &str) -> bool {
        if self.docs[self.active].text.has_multi() {
            self.message(title, "Press Esc to go back to one selection first.".into());
            return false;
        }
        true
    }

    /// Changes the selected lines, or the whole file.
    fn change_lines(&mut self, change: impl FnOnce(&[String], usize) -> Vec<String>) {
        if !self.one_selection("Format") {
            return;
        }
        let width = self.prefs.paragraph.width;
        let text = &mut self.doc().text;
        if let Some((top, bottom)) = text.rows_for_convert() {
            let rows = text.rows_text(top, bottom);
            text.replace_lines(top, bottom, change(&rows, width));
        }
    }

    /// Changes the selected lines, or the cursor's paragraph.
    fn change_paragraph(&mut self, change: impl FnOnce(&[String]) -> Vec<String>) {
        if !self.one_selection("Paragraph") {
            return;
        }
        let text = &mut self.doc().text;
        if let Some((top, bottom)) = text.rows_for_paragraph() {
            let rows = text.rows_text(top, bottom);
            text.replace_lines(top, bottom, change(&rows));
        } else if text.notice.is_none() {
            self.message("Paragraph", "Put the cursor in a paragraph, or select some lines.".into());
        }
    }

    fn reindent_lines(&mut self) {
        if !self.one_selection("Reindent") {
            return;
        }
        let language = self.docs[self.active].language_name();
        let marks = match reindent_marks(&language) {
            Ok(marks) => marks,
            Err(problem) => {
                self.message("Reindent", problem);
                return;
            }
        };
        let unit = self.indent_unit();
        if self.docs[self.active].text.is_disk_backed() {
            // Too large to hold: every line goes through, from the top, as the depth of brackets is followed.
            let (top, bottom) = self.docs[self.active].text.convert_bounds();
            let mut indenter = reformat::Reindenter::new(&unit, marks);
            let format = self.docs[self.active].text.format.clone();
            self.start_rewrite(self.active, "Reindent", format, None, Lossy::Refuse, Box::new(move |number, line| indenter.line(line, (top..=bottom).contains(&number))));
            return;
        }
        let text = &mut self.doc().text;
        if let Some((top, bottom)) = text.rows_for_convert() {
            let rows = reformat::reindent(&text.all_lines(), top, bottom, &unit, &marks);
            text.replace_lines(top, bottom, rows);
        }
    }

    /// Runs a formatter over the selection, or the whole file. It only goes in if it changed nothing but the layout.
    fn run_formatter(&mut self, kind: formatters::Kind) {
        if !self.one_selection(kind.title()) {
            return;
        }
        let doc = &self.docs[self.active];
        if doc.text.is_block() {
            self.message(kind.title(), "Turn off Column Mode first, then try again.".into());
            return;
        }
        let whole = !doc.text.has_selection();
        let Some(text) = doc.text.selection_or_all() else {
            self.message(kind.title(), "This file is too large to reformat as a whole. Select the part you want instead.".into());
            return;
        };
        match formatters::apply(kind, &text, &self.indent_unit()) {
            Ok(formatted) if formatted == text.replace("\r\n", "\n") => {}
            Ok(formatted) => {
                let buffer = &mut self.doc().text;
                buffer.replace_selection_or_all(&formatted);
                if whole {
                    buffer.move_caret(0, 0, false);
                }
            }
            Err(problem) => self.message(kind.title(), problem),
        }
    }

    // ---------- Projects ----------

    fn note_recent_project(&mut self, path: &Path) {
        const MOST_RECENT: usize = 15;
        self.recent_projects.retain(|known| !same_path(known, path));
        self.recent_projects.insert(0, path.to_path_buf());
        self.recent_projects.truncate(MOST_RECENT);
    }

    /// Keeps the project's file up to date. Not being able to is worth saying.
    fn save_project(&mut self) {
        if let Some(problem) = self.project.as_ref().and_then(|project| project.save().err().map(|e| format!("Couldn't save {}.\n\n{e}", project.path.display()))) {
            self.message("Project", problem);
        }
    }

    fn new_project(&mut self, mut path: PathBuf) {
        if path.extension().is_none() {
            path.set_extension(project::EXTENSION);
        }
        self.close_project();
        self.note_recent_project(&path);
        self.project = Some(Project::new(&path));
        self.panels.project = true;
        self.save_project();
    }

    fn open_project(&mut self, path: &Path) {
        match Project::load(path) {
            Ok(opened) => {
                self.close_project();
                self.note_recent_project(path);
                let files: Vec<PathBuf> = opened.open_files.iter().filter(|file| file.exists()).cloned().collect();
                self.project = Some(opened);
                self.panels.project = true;
                for file in files {
                    self.open_path(&file);
                }
            }
            Err(problem) => {
                self.recent_projects.retain(|known| !same_path(known, path));
                self.message("Open project", format!("Couldn't open {}.\n\n{problem}", path.display()));
            }
        }
    }

    /// Closes the project and remembers which files were open. They stay open.
    fn close_project(&mut self) {
        let files: Vec<PathBuf> = self.docs.iter().filter_map(|doc| doc.path.clone()).collect();
        if let Some(project) = &mut self.project {
            project.open_files = files;
            self.save_project();
            self.project = None;
        }
    }

    fn add_to_project(&mut self, paths: impl IntoIterator<Item = PathBuf>) {
        if let Some(project) = &mut self.project {
            project.add(paths);
            self.save_project();
        }
    }

    fn backup_project(&mut self) {
        if self.project_job.is_some() {
            self.message("Backup project", "A backup or restore is still running.".into());
            return;
        }
        let Some(project) = self.project.clone() else {
            return;
        };
        let stamp = chrono::Local::now().format("%Y-%m-%d-%H%M%S").to_string();
        self.project_job = Some((project::in_background(move || project::backup(&project, &stamp)), false));
    }

    /// Takes a backup of how things are now, then puts the chosen one back.
    fn restore_project(&mut self, chosen: project::Backup) {
        if self.project_job.is_some() {
            self.message("Restore project", "A backup or restore is still running.".into());
            return;
        }
        let Some(current) = self.project.clone() else {
            return;
        };
        let keep: Vec<PathBuf> = self.docs.iter().filter(|doc| doc.is_modified()).filter_map(|doc| doc.path.clone()).collect();
        let stamp = chrono::Local::now().format("%Y-%m-%d-%H%M%S-before-restore").to_string();
        let work = move || {
            project::backup(&current, &stamp)?;
            Ok(project::restore(&chosen, &keep))
        };
        self.project_job = Some((project::in_background(work), true));
    }

    /// Whatever a restore has just replaced, the windows on it read again. Unsaved work was left out of the restore.
    fn after_restore(&mut self) {
        for index in 0..self.docs.len() {
            let doc = &self.docs[index];
            let Some(path) = doc.path.clone() else {
                continue;
            };
            if doc.is_modified() || doc.is_busy() || file_stamp(&path) == doc.disk_stamp {
                continue;
            }
            if let Ok(mut fresh) = self.reopened(index, &path) {
                fresh.preview_view = self.docs[index].preview_view;
                self.docs[index] = fresh;
            }
        }
        if let Some(path) = self.project.as_ref().map(|project| project.path.clone())
            && let Ok(restored) = Project::load(&path)
        {
            self.project = Some(restored);
        }
    }

    /// Asks what to compare this file with.
    fn begin_compare(&mut self) {
        let doc = &self.docs[self.active];
        if doc.hex_mode {
            self.message("Compare", "Compare works on text, so it can't take a file open in hex mode.".into());
            return;
        }
        if doc.is_busy() {
            self.message("Compare", "Please wait for this file to finish being read or written first.".into());
            return;
        }
        let others = self.docs.iter().enumerate().filter(|(index, other)| *index != self.active && !other.placeholder).map(|(index, other)| (index, other.name())).collect();
        self.dialog = Dialog::ComparePick(others);
    }

    /// The lines to compare with, from another open file with its unsaved edits.
    fn lines_of_open_file(&self, index: usize) -> Result<(String, crate::big_jobs::Side), String> {
        let other = &self.docs[index];
        if other.hex_mode {
            return Err(format!("{} is open in hex mode, so it can't be compared.", other.name()));
        }
        if other.is_busy() {
            return Err(format!("{} is still being read or written. Please try again when it's finished.", other.name()));
        }
        Ok((other.name(), crate::big_jobs::Side::Lines(other.text.store().clone())))
    }

    /// The lines of a file on disk to compare with: read in if it's small, or looked through where it is if not.
    fn lines_of_file_on_disk(path: &Path) -> Result<(String, crate::big_jobs::Side), String> {
        let name = path.display().to_string();
        if std::fs::metadata(path).map_err(|e| e.to_string())?.len() > DISK_THRESHOLD {
            let source = Source::open_as(path, None).map_err(|e| e.to_string())?;
            return Ok((name, crate::big_jobs::Side::Scanning(source)));
        }
        let text = tools::text_of_file(path)?;
        Ok((name, crate::big_jobs::Side::Lines(LineStore::from_vec(text.split('\n').map(Arc::from).collect()))))
    }

    /// Compares this file, with its unsaved edits, with those lines. Small files are compared there and then; if
    /// either side is too large to hold, the comparison is worked out in the background and shows when it's done.
    fn show_comparison(&mut self, other: Result<(String, crate::big_jobs::Side), String>) {
        let (name, side) = match other {
            Ok(other) => other,
            Err(problem) => return self.message("Compare", problem),
        };
        let doc = &self.docs[self.active];
        let left = doc.text.store().clone();
        match side {
            crate::big_jobs::Side::Lines(right) if !left.is_disk_backed() && !right.is_disk_backed() => {
                let mut comparison = compare::Comparison::new(doc.name(), name, left, right);
                comparison.step(true);
                self.dialog = Dialog::Compare(Box::new(comparison));
            }
            side => {
                if self.big_job.is_some() {
                    return self.message("Compare", "Please wait for the file being worked through to finish.".into());
                }
                self.big_job = Some(crate::big_jobs::compare(doc.name(), name, left, side, doc.id));
            }
        }
    }

    /// Asks what to print with Windows' own window, then sends it on its way.
    fn print_document(&mut self) {
        const MOST_HEX_PRINTED: usize = crate::hex_buffer::MOST_VIEW_COPIED;
        let doc = &self.docs[self.active];
        let (rows, selection) = if doc.hex_mode {
            let shown = |value: u8| views::hex_pane_char(&doc.hex, &doc.text.format, value);
            let (start, end) = (doc.hex.caret.min(doc.hex.anchor), doc.hex.caret.max(doc.hex.anchor));
            let selection = (start != end && end - start <= MOST_HEX_PRINTED).then(|| doc.hex.view_rows(start, end, shown));
            if doc.hex.len() > MOST_HEX_PRINTED {
                self.message("Print", "This file is too large to print in hex. It can be up to 16 MB.".into());
                return;
            }
            (print::Rows::Ready(doc.hex.view_rows(0, doc.hex.len(), shown)), selection.map(print::Rows::Ready))
        } else {
            let selection = doc.text.has_selection().then(|| {
                let text = doc.text.selection_text();
                print::Rows::Ready(print::printable_rows(&text.split("\r\n").collect::<Vec<_>>(), &doc.text.format))
            });
            // The lines are made ready as they're printed, so even a file too large to hold can be.
            (print::Rows::Lines { store: doc.text.store().clone(), format: doc.text.format.clone() }, selection)
        };
        let job = print::Job { title: doc.path.as_ref().map_or_else(|| doc.name(), |path| path.display().to_string()), rows, selection, header: self.prefs.print_header, line_numbers: self.prefs.print_line_numbers };
        match print::ask_and_print(job) {
            Ok(Some(outcome)) => {
                self.print_job = Some(outcome);
                self.print_status = "Printing...".to_string();
            }
            Ok(None) => {}
            Err(problem) => self.message("Print", problem),
        }
    }

    /// Finishes background scans and saves when they are done.
    fn check_background_work(&mut self, ctx: &Context) {
        self.send_saved_remotes();
        let mut busy = self.check_big_job() | self.check_remote_jobs() | self.check_sends() | self.check_sessions();
        busy |= self.git_tick(ctx);
        if self.lsp_tick() {
            ctx.request_repaint_after(Duration::from_millis(50));
        }
        self.assembly_tick();
        if self.intel_wait.is_some() {
            ctx.request_repaint_after(Duration::from_millis(350));
        }
        if self.follow_live_views() {
            ctx.request_repaint_after(Duration::from_millis(250));
        }
        self.autosave(ctx);
        self.open_handed_over(ctx);
        #[cfg(target_os = "macos")]
        for path in crate::mac_open::take() {
            self.open_path(&path);
        }
        self.keep_tags();
        let running = self.check_run_tool();
        busy |= running;
        self.task_tick(running);
        busy |= self.check_split();
        busy |= self.check_assistant();
        self.spelling_tick();
        if let Some((job, restoring)) = self.project_job.clone() {
            let finished = job.lock().unwrap().take();
            if let Some(result) = finished {
                self.project_job = None;
                if restoring && result.is_ok() {
                    self.after_restore();
                }
                let title = if restoring { "Restore project" } else { "Backup project" };
                self.message(title, project_summary(&result, restoring));
            } else {
                busy = true;
            }
        }
        if let Some(job) = self.print_job.clone() {
            let finished = job.lock().unwrap().take();
            match finished {
                Some(Ok(pages)) => {
                    self.print_job = None;
                    self.print_status = format!("Sent {pages} page{} to the printer", if pages == 1 { "" } else { "s" });
                }
                Some(Err(problem)) => {
                    self.print_job = None;
                    self.print_status.clear();
                    self.message("Print", problem);
                }
                None => busy = true,
            }
        }
        if let Some(job) = self.replace_job.clone() {
            if job.finished.load(Ordering::SeqCst) {
                let summary = replace_summary(&job.report.lock().unwrap());
                self.replace_job = None;
                self.find.status.clear();
                // The list was of what was there before.
                self.job = None;
                self.show_results = false;
                self.message("Replace in Files", summary);
            } else {
                self.find.status = format!("Replacing: {} of {} files...", job.done.load(Ordering::Relaxed), job.total.load(Ordering::Relaxed));
                busy = true;
            }
        }
        for doc in &mut self.docs {
            if let Some(source) = doc.scanning.clone() {
                doc.text.refresh_from_source();
                if source.is_finished() {
                    if let Some(problem) = source.failure() {
                        warn(&format!("Couldn't finish reading {}.\n\n{problem}\n\nOnly the part read so far is shown. Saving is not safe until the file is reopened.", source.path().display()));
                    }
                    if !doc.format_chosen {
                        doc.text.format = source.format();
                    }
                    doc.text.refresh_from_source();
                    doc.text.mark_all_changed();
                    doc.scanning = None;
                } else {
                    busy = true;
                }
            }
            let finished = doc.saving.as_ref().and_then(|job| job.result.lock().unwrap().take());
            match finished {
                Some(Ok(())) if doc.hex_mode => {
                    let job = doc.saving.take().unwrap();
                    finish_hex_save(doc, job.path);
                }
                Some(Ok(())) => {
                    let job = doc.saving.take().unwrap();
                    doc.saved_over_source = doc.text.store().source().is_some_and(|s| same_path(s.path(), &job.path));
                    // From here the lines are read from the file as saved, so a recovery copy can refer to it.
                    if doc.saved_over_source
                        && let Ok(source) = Source::open_as(&job.path, Some(doc.text.format.charset))
                    {
                        doc.text.rebase_on_source(source.clone());
                        doc.scanning = Some(source);
                        doc.format_chosen = true;
                        doc.saved_over_source = false;
                        busy = true;
                    }
                    note_saved_to(doc, &job.path);
                    doc.disk_stamp = file_stamp(&job.path);
                    doc.resolved_syntax = None;
                    doc.path = Some(job.path);
                    doc.carried = false;
                    doc.text.mark_saved();
                }
                Some(Err(problem)) => {
                    let job = doc.saving.take().unwrap();
                    warn(&format!("Couldn't save {}.\n\n{problem}\n\nThe original file has not been changed.", job.path.display()));
                }
                None => busy |= doc.saving.is_some(),
            }
        }
        if busy {
            ctx.request_repaint_after(Duration::from_millis(100));
        }
    }

    /// Shows what a Word Count or Save a copy of a huge file came to, once it's done. True while it's running.
    /// Changes every line of a document too large to hold in memory, in the background. The new version is written
    /// to a file of its own that the document then carries on from, as one step for Undo.
    fn start_rewrite(&mut self, index: usize, title: &'static str, to: TextFormat, eol: Option<codec::Eol>, lossy: Lossy, change: crate::big_jobs::LineChange) -> bool {
        if self.big_job.is_some() {
            self.message(title, "Please wait for the file being worked through to finish.".into());
            return false;
        }
        let doc = &mut self.docs[index];
        if doc.text.read_only {
            self.tell_read_only();
            return false;
        }
        let path = self.large_edit_path(index);
        let doc = &mut self.docs[index];
        doc.rewriting = true;
        self.big_job = Some(crate::big_jobs::rewrite(title, doc.text.store().clone(), doc.text.format.clone(), to, eol, lossy, change, path, doc.id));
        true
    }

    /// Where a file too large to hold is rewritten to, in the data folder or else the temporary one. Each is
    /// deleted when Limoniter closes.
    fn large_edit_path(&mut self, index: usize) -> PathBuf {
        let doc = &self.docs[index];
        let stem = doc.path.as_deref().and_then(Path::file_stem).map_or_else(|| "untitled".into(), |s| s.to_string_lossy().into_owned());
        let ending = doc.path.as_deref().and_then(Path::extension).map_or_else(String::new, |e| format!(".{}", e.to_string_lossy()));
        let root = self.large_edits_root();
        if self.large_edits.is_none() {
            self.large_edits = crate::large_edits::Folder::start(&root).ok();
        }
        match &mut self.large_edits {
            Some(folder) => folder.new_path(&stem, &ending),
            // Without a folder of its own, a loose file, which the next run tidies away.
            None => root.join(format!("{stem}-{}-{}{ending}", std::process::id(), std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_nanos()))),
        }
    }

    /// Where the files large changes are written to are kept: in the data folder, or else the temporary one.
    fn large_edits_root(&self) -> PathBuf {
        self.remote_root.as_ref().and_then(|r| r.parent()).map_or_else(|| std::env::temp_dir().join("limoniter-large-edits"), |data| data.join("large-edits"))
    }

    /// Rewrites the records of a table too large to hold, changing each and putting them in order, in the
    /// background; the file carries on from the result as one step that Undo takes back.
    pub(super) fn start_record_rewrite(&mut self, title: &'static str, records: Arc<Vec<crate::table::Record>>, delimiter: char, arrange: crate::big_jobs::Arrange, change: crate::big_jobs::RecordChange) {
        if self.big_job.is_some() {
            return self.message(title, "Please wait for the file being worked through to finish.".into());
        }
        let index = self.active;
        if self.docs[index].text.read_only {
            return self.tell_read_only();
        }
        let path = self.large_edit_path(index);
        let doc = &mut self.docs[index];
        doc.rewriting = true;
        self.big_job = Some(crate::big_jobs::rewrite_records(title, doc.text.store().clone(), doc.text.format.clone(), delimiter, records, arrange, change, path, doc.id));
    }

    fn check_big_job(&mut self) -> bool {
        let Some(job) = &self.big_job else {
            return false;
        };
        let Some(outcome) = job.take() else {
            return true;
        };
        let title = job.title;
        let doc_id = job.doc_id;
        self.big_job = None;
        if let Some(doc) = self.docs.iter_mut().find(|d| d.id == doc_id) {
            doc.rewriting = false;
        }
        match outcome {
            crate::big_jobs::Outcome::Lines { needle, lines } => {
                if let Some(index) = self.docs.iter().position(|d| d.id == doc_id) {
                    self.active = index;
                    let doc = &mut self.docs[index];
                    let was_read_only = doc.text.read_only;
                    doc.set_read_only(true);
                    doc.filter = Some(Filter { needle, lines, was_read_only });
                    doc.text.reveal = true;
                    self.pending_focus = true;
                }
                return true;
            }
            crate::big_jobs::Outcome::Compared(mut comparison) => {
                comparison.step(true);
                self.dialog = Dialog::Compare(comparison);
                return true;
            }
            crate::big_jobs::Outcome::Rewritten { path, format } => {
                let Some(doc) = self.docs.iter_mut().find(|d| d.id == doc_id) else {
                    return false;
                };
                match Source::open_as(&path, Some(format.charset)) {
                    Ok(source) => {
                        let mut format = format;
                        format.has_bom = source.has_bom();
                        charset_widths(&format);
                        doc.text.replace_with_source(source.clone(), format);
                        doc.scanning = Some(source);
                        doc.resolved_syntax = None;
                    }
                    Err(e) => warn(&format!("{title} couldn't read back what it wrote.\n\n{e}")),
                }
                return true;
            }
            crate::big_jobs::Outcome::Done(text) => self.message(title, text),
            crate::big_jobs::Outcome::Failed(problem) => warn(&problem),
            crate::big_jobs::Outcome::Problems(problems, target) => {
                if let Some(index) = self.docs.iter().position(|d| d.id == doc_id) {
                    self.active = index;
                    let purpose = if title == CONVERT_TITLE { LossyPurpose::Convert } else { LossyPurpose::Export };
                    let report = LossyReport { purpose, target_name: target.charset.name.clone(), problems };
                    self.dialog = Dialog::Lossy(report, target);
                }
            }
        }
        false
    }

    /// Waits for a background save, for when the app is about to close.
    fn wait_for_save(&mut self, index: usize) -> bool {
        while self.docs[index].saving.is_some() {
            std::thread::sleep(Duration::from_millis(50));
            let finished = self.docs[index].saving.as_ref().and_then(|job| job.result.lock().unwrap().take());
            match finished {
                Some(Ok(())) if self.docs[index].hex_mode => {
                    let doc = &mut self.docs[index];
                    let job = doc.saving.take().unwrap();
                    finish_hex_save(doc, job.path);
                }
                Some(Ok(())) => {
                    let doc = &mut self.docs[index];
                    let job = doc.saving.take().unwrap();
                    doc.disk_stamp = file_stamp(&job.path);
                    doc.resolved_syntax = None;
                    doc.path = Some(job.path);
                    doc.carried = false;
                    doc.text.mark_saved();
                }
                Some(Err(problem)) => {
                    let job = self.docs[index].saving.take().unwrap();
                    warn(&format!("Couldn't save {}.\n\n{problem}\n\nThe original file has not been changed.", job.path.display()));
                    return false;
                }
                None => {}
            }
        }
        true
    }

    /// Asks about unsaved changes; returns false if the person cancels.
    fn confirm_close(&mut self, index: usize) -> bool {
        if !self.docs[index].is_modified() {
            return true;
        }
        self.active = index;
        let answer = MessageDialog::new()
            .set_title("Limoniter")
            .set_level(MessageLevel::Warning)
            .set_description(format!("Save changes to {}?", self.docs[index].name()))
            .set_buttons(MessageButtons::YesNoCancel)
            .show();
        match answer {
            MessageDialogResult::Yes => self.save(index) && self.wait_for_save(index),
            MessageDialogResult::No => true,
            _ => false,
        }
    }

    fn close_tab(&mut self, index: usize) {
        // With other windows on the document, only this one goes.
        if !self.docs[index].windows.is_empty() {
            let number = self.docs[index].window_number;
            self.docs[index].close_window(number);
            self.pending_focus = true;
            return;
        }
        if !self.confirm_close(index) {
            return;
        }
        let closed = self.docs.remove(index);
        self.jumps.forget_doc(closed.id);
        drop(closed);
        if self.docs.is_empty() {
            // Nothing takes its place: the window just shows no file until one is opened or made.
            let mut none = Document::new(String::new());
            none.placeholder = true;
            self.docs.push(none);
            self.active = 0;
            self.pending = None;
            self.find.status.clear();
        } else if self.active >= self.docs.len() {
            self.active = self.docs.len() - 1;
        } else if self.active > index {
            self.active -= 1;
        }
        self.pending_focus = true;
    }

    fn toggle_hex(&mut self) {
        let index = self.active;
        let large = if self.docs[index].hex_mode { self.docs[index].hex.is_on_disk() } else { self.docs[index].text.is_disk_backed() };
        if large {
            self.toggle_large_hex(index);
            return;
        }
        let modified = self.docs[index].is_modified();
        // The cursor stays at the same place in the file across the switch, as in the original.
        if self.docs[index].hex_mode {
            let doc = &mut self.docs[index];
            let bytes = doc.hex.bytes().to_vec();
            let decoded = codec::decode(&bytes, &mut doc.text.format);
            let (line, character) = codec::position_of_offset(&bytes, doc.hex.caret, &doc.text.format);
            doc.text.set_decoded(decoded);
            doc.text.move_caret_to_char(line, character);
            doc.hex_mode = false;
            doc.hex.ebcdic = false;
        } else {
            let Some(bytes) = self.bytes_for(index) else {
                return;
            };
            let doc = &mut self.docs[index];
            let (line, character) = doc.text.caret_char_position();
            let lines = doc.text.all_lines();
            let text = &doc.text;
            let offset = codec::offset_of_position(&lines, &|i| text.terminator(i), line, character, &text.format);
            doc.hex.load(bytes);
            doc.hex.move_caret(offset, false);
            doc.hex_mode = true;
        }
        self.docs[index].carried = modified;
        self.pending_focus = true;
    }

    /// A file kept on disk switches by reading it afresh the other way, so its edits have to be saved first.
    fn toggle_large_hex(&mut self, index: usize) {
        let doc = &self.docs[index];
        let Some(path) = doc.path.clone() else {
            return;
        };
        if doc.is_modified() || doc.narrow.is_some() {
            let what = if doc.narrow.is_some() { "Show the whole file again" } else { "Save your changes" };
            self.message("Hex Mode", format!("{what} first: a file this large switches between text and hex by reading it again from disk."));
            return;
        }
        if doc.is_busy() {
            self.message("Please wait", "The file is still loading or saving. This will be available when it finishes.".into());
            return;
        }
        let doc = &mut self.docs[index];
        if doc.hex_mode {
            if !readable_a_block_at_a_time(&doc.text.format) {
                let name = doc.text.format.encoding_name();
                self.message("Hex Mode", format!("A file this large can only be shown as text in UTF-8 or a byte set, not {name}. Choose one with Reopen with encoding."));
                return;
            }
            let offset = doc.hex.caret as u64;
            match Source::open_as(&path, Some(doc.text.format.charset)) {
                Ok(source) => {
                    doc.format_chosen = true;
                    doc.text.set_source(source.clone());
                    if let Some(line) = source.line_at_offset(offset) {
                        doc.text.move_caret_to_char(line, 0);
                    }
                    doc.scanning = Some(source);
                    doc.hex = HexBuffer::default();
                    doc.hex_mode = false;
                }
                Err(e) => warn(&format!("Couldn't read {}.\n\n{e}", path.display())),
            }
        } else {
            let (line, _) = doc.text.caret_char_position();
            let offset = doc.text.store().source().and_then(|source| source.line_start(line + doc.text.line_offset).ok()).unwrap_or(0);
            match crate::byte_store::FileBytes::open(&path) {
                Ok(file) => {
                    doc.hex.load_file(file);
                    doc.hex.move_caret(offset as usize, false);
                    doc.hex_mode = true;
                    // The lines aren't needed while it's shown in hex; they're read again on the way back.
                    if let Some(source) = doc.scanning.take() {
                        source.stop();
                    }
                    doc.text.set_content(vec![String::new()]);
                }
                Err(e) => warn(&format!("Couldn't read {}.\n\n{e}", path.display())),
            }
        }
        self.pending_focus = true;
    }

    /// Every tab in order, as a document and which of its windows.
    fn all_tabs(&self) -> Vec<(usize, usize)> {
        self.docs.iter().enumerate().flat_map(|(index, doc)| doc.window_numbers().into_iter().map(move |number| (index, number))).collect()
    }

    fn step_tab(&mut self, forward: bool) {
        let tabs = self.all_tabs();
        let here = tabs.iter().position(|tab| *tab == (self.active, self.docs[self.active].window_number)).unwrap_or(0);
        let (index, number) = tabs[if forward { (here + 1) % tabs.len() } else { (here + tabs.len() - 1) % tabs.len() }];
        self.active = index;
        self.docs[index].switch_window(number);
        self.pending_focus = true;
    }

    /// Splits the window, changes which way it's split, or if it's already split this way, ends the split.
    fn toggle_split(&mut self, vertical: bool) {
        let doc = self.doc();
        match &mut doc.split {
            Some(split) if split.vertical == vertical => doc.split = None,
            Some(split) => split.vertical = vertical,
            None => doc.split = Some(Split { vertical, share: 0.5, second: doc.text_state.same_place(), active: 0 }),
        }
        self.pending_focus = true;
    }

    /// Starts showing only the lines that contain the selected text, or ends that if it's going already.
    fn toggle_filter(&mut self) {
        let doc = self.doc();
        if let Some(filter) = doc.filter.take() {
            doc.set_read_only(filter.was_read_only);
            doc.text.reveal = true;
            self.pending_focus = true;
            return;
        }
        if doc.hex_mode {
            return;
        }
        if !doc.text.has_selection() || doc.text.caret_line != doc.text.anchor_line || doc.text.is_block() {
            self.message("Filter on selection", "Select some text on one line first. Only the lines containing it will show.".into());
            return;
        }
        let needle = doc.text.selection_text();
        if doc.text.is_disk_backed() {
            // Too large to hold: the lines are looked through in the background, and the filter shows when that's done.
            let (store, id) = (doc.text.store().clone(), doc.id);
            if self.big_job.is_some() {
                self.message("Filter on selection", "Please wait for the file being worked through to finish.".into());
                return;
            }
            self.big_job = Some(crate::big_jobs::find_lines(store, needle, id));
            return;
        }
        let lines = lines_containing(&doc.text.all_lines(), &needle);
        let was_read_only = doc.text.read_only;
        doc.set_read_only(true);
        doc.filter = Some(Filter { needle, lines, was_read_only });
        doc.text.reveal = true;
        self.pending_focus = true;
    }

    /// Regular mode for None; hex mode otherwise, with the character pane reading EBCDIC or not.
    fn set_view(&mut self, hex: Option<bool>) {
        if self.docs[self.active].hex_mode != hex.is_some() {
            self.toggle_hex();
        }
        let doc = self.doc();
        if let Some(ebcdic) = hex
            && doc.hex_mode
        {
            doc.hex.ebcdic = ebcdic;
        }
        self.pending_focus = true;
    }

    /// Copies the selection as it looks on screen.
    fn copy_hex_view(&mut self, ui: &Ui) {
        let doc = &self.docs[self.active];
        if !doc.hex_mode || !doc.hex.has_selection() {
            return;
        }
        if doc.hex.caret.abs_diff(doc.hex.anchor) > hex_buffer::MOST_VIEW_COPIED {
            self.message("Copy selected", "That's too much to lay out as text. It can be up to 16 MB at a time.".into());
            return;
        }
        ui.ctx().copy_text(doc.hex.view_text(|value| views::hex_pane_char(&doc.hex, &doc.text.format, value)));
    }

    // ---------- Searching ----------

    fn search_options(&self, backward: bool) -> SearchOptions {
        let hex = self.docs[self.active].hex_mode;
        let format = &self.docs[self.active].text.format;
        // Where every byte stays a byte in the text, a search has to look for those bytes.
        let by_bytes = !hex && format.charset.keep_bytes;
        let table = format.state;
        SearchOptions {
            pattern: if by_bytes { format.prepare_input(&self.find.find, table) } else { self.find.find.clone() },
            replacement: if by_bytes { format.prepare_input(&self.find.replace, table) } else { self.find.replace.clone() },
            match_case: self.find.match_case,
            whole_word: !hex && !by_bytes && self.find.whole_word,
            regex: !hex && !by_bytes && self.find.regex,
            hex_bytes: hex && self.find.hex_bytes,
            backward,
        }
    }

    fn run_find(&mut self, backward: bool) {
        if self.no_file() {
            self.find.status = "No file is open.".into();
            return;
        }
        self.record_find(backward);
        note_search(&mut self.find.history, &self.find.find.clone());
        let options = self.search_options(backward);
        if self.docs[self.active].is_busy() {
            self.find.status = "Please wait until the file has finished loading or saving.".into();
            return;
        }
        if self.docs[self.active].hex_mode && self.docs[self.active].hex.is_on_disk() {
            match self.docs[self.active].hex.begin_find(&options) {
                Ok(Some(search)) => {
                    self.pending = Some(Pending::HexFind(search));
                    self.pending_share = 0.0;
                    self.find.status = "Searching...".into();
                }
                Ok(None) => {}
                Err(e) => self.find.status = e,
            }
            return;
        }
        if !self.docs[self.active].hex_mode && self.docs[self.active].text.is_disk_backed() {
            match self.docs[self.active].text.begin_find(&options) {
                Ok(Some(cursor)) => {
                    self.pending = Some(Pending::Find(cursor));
                    self.pending_share = 0.0;
                    self.find.status = "Searching...".into();
                }
                Ok(None) => {}
                Err(e) => self.find.status = e,
            }
            return;
        }
        let not_found = search_not_found(&self.find.find);
        let doc = self.doc();
        let result = if doc.hex_mode { doc.hex.find(&options) } else { doc.text.find(&options) };
        self.find.status = match result {
            Ok(true) => String::new(),
            Ok(false) => not_found,
            Err(e) => e,
        };
    }

    fn run_replace(&mut self) {
        if self.no_file() {
            self.find.status = "No file is open.".into();
            return;
        }
        if self.docs[self.active].text.read_only {
            self.tell_read_only();
            return;
        }
        self.record_replace("replace_next");
        note_search(&mut self.find.history, &self.find.find.clone());
        let options = self.search_options(false);
        let not_found = search_not_found(&self.find.find);
        if self.docs[self.active].hex_mode && self.docs[self.active].hex.is_on_disk() {
            // The replacement is made straight away; finding the next one goes a stretch at a time.
            if let Err(e) = self.docs[self.active].hex.replace_selected(&options) {
                self.find.status = e;
                return;
            }
            self.run_find(false);
            return;
        }
        let doc = self.doc();
        let result = if doc.hex_mode { doc.hex.replace_current(&options) } else { doc.text.replace_current(&options) };
        self.find.status = match result {
            Ok(true) => String::new(),
            Ok(false) => not_found,
            Err(e) => e,
        };
    }

    /// Does a slice of any search or Replace All running on a big file.
    fn continue_pending(&mut self, ctx: &Context) {
        let Some(mut pending) = self.pending.take() else {
            return;
        };
        let doc = &mut self.docs[self.active];
        let text = &mut doc.text;
        match &mut pending {
            Pending::HexFind(search) => match doc.hex.find_step(search, HEX_SLICE) {
                SearchStep::Running(share) => self.pending_share = share,
                SearchStep::Found => {
                    self.pending_count = 1;
                    self.find.status.clear();
                    self.pending_focus = !self.find.open;
                    return;
                }
                SearchStep::NotFound => {
                    self.pending_count = 0;
                    self.find.status = search_not_found(&self.find.find);
                    return;
                }
            },
            Pending::HexReplaceAll(job) => match doc.hex.replace_step(job, HEX_SLICE) {
                Some(share) => self.pending_share = share,
                None => {
                    self.pending_count = job.count;
                    self.find.status = match job.count {
                        0 => search_not_found(&self.find.find),
                        n => format!("{n} items replaced"),
                    };
                    return;
                }
            },
            Pending::Find(cursor) => match text.find_step(cursor, WORK_SLICE) {
                SearchStep::Running(share) => self.pending_share = share,
                SearchStep::Found => {
                    self.pending_count = 1;
                    self.find.status.clear();
                    self.pending_focus = !self.find.open;
                    return;
                }
                SearchStep::NotFound => {
                    self.pending_count = 0;
                    self.find.status = search_not_found(&self.find.find);
                    return;
                }
            },
            Pending::ReplaceAll(cursor) => match text.replace_step(cursor, WORK_SLICE) {
                Some(share) => self.pending_share = share,
                None if cursor.too_large => {
                    self.pending_count = 0;
                    self.find.status.clear();
                    self.message("Replace All", "That would change more than 1 GB of text, which is too much to hold in memory until it's saved, so nothing has been changed.\n\nReplace in Files on the Find panel can do it, because it writes straight to disk. Save this file first, then close it, and choose it there.".into());
                    return;
                }
                None => {
                    self.pending_count = cursor.count;
                    self.find.status = match cursor.count {
                        0 => search_not_found(&self.find.find),
                        n => format!("{n} items replaced"),
                    };
                    return;
                }
            },
        }
        self.find.status = format!("Working... {:.0}%", self.pending_share * 100.0);
        self.pending = Some(pending);
        ctx.request_repaint();
    }

    fn run_replace_all(&mut self) {
        if self.no_file() {
            self.find.status = "No file is open.".into();
            return;
        }
        if self.docs[self.active].text.read_only {
            self.tell_read_only();
            return;
        }
        self.record_replace("replace_all");
        note_search(&mut self.find.history, &self.find.find.clone());
        let options = self.search_options(false);
        if self.docs[self.active].is_busy() {
            self.find.status = "Please wait until the file has finished loading or saving.".into();
            return;
        }
        if self.docs[self.active].hex_mode && self.docs[self.active].hex.is_on_disk() {
            match self.docs[self.active].hex.begin_replace_all(&options) {
                Ok(Some(job)) => {
                    self.pending = Some(Pending::HexReplaceAll(job));
                    self.pending_share = 0.0;
                }
                Ok(None) => {}
                Err(e) => self.find.status = e,
            }
            return;
        }
        if !self.docs[self.active].hex_mode && self.docs[self.active].text.is_disk_backed() {
            match self.docs[self.active].text.begin_replace_all(&options) {
                Ok(Some(cursor)) => {
                    self.pending = Some(Pending::ReplaceAll(cursor));
                    self.pending_share = 0.0;
                }
                Ok(None) => {}
                Err(e) => self.find.status = e,
            }
            return;
        }
        let not_found = search_not_found(&self.find.find);
        let doc = self.doc();
        let result = if doc.hex_mode { doc.hex.replace_all(&options) } else { doc.text.replace_all(&options) };
        self.find.status = match result {
            Ok(0) => not_found,
            Ok(n) => format!("{n} items replaced"),
            Err(e) => e,
        };
    }

    /// With `preview`, the results are what Replace in Files would change, and it's then ready to go ahead.
    fn start_find_in_files(&mut self, preview: bool) {
        note_search(&mut self.find.history, &self.find.find.clone());
        if let Some(job) = &self.job {
            job.cancel.store(true, Ordering::Relaxed);
        }
        self.replace_plan = None;
        let options = SearchOptions {
            pattern: self.find.find.clone(),
            replacement: self.find.replace.clone(),
            match_case: self.find.match_case,
            whole_word: self.find.whole_word,
            regex: self.find.regex,
            ..SearchOptions::default()
        };
        if options.pattern.is_empty() {
            self.find.status = "Type something to find first.".into();
            return;
        }
        let result = match self.find.scope {
            FileScope::Folder => {
                if self.find.folder.trim().is_empty() || !Path::new(self.find.folder.trim()).is_dir() {
                    self.find.status = "Choose a folder to search first.".into();
                    return;
                }
                find_files::start(PathBuf::from(self.find.folder.trim()), self.find.file_types.clone(), self.find.recursive, &options)
            }
            FileScope::OpenFiles => self.search_open_files(&options),
            FileScope::Project => match &self.project {
                Some(project) if !project.files.is_empty() => find_files::start_list(project.files.clone(), &options),
                Some(_) => {
                    self.find.status = "This project has no files yet.".into();
                    return;
                }
                None => {
                    self.find.status = "Open a project first.".into();
                    return;
                }
            },
            FileScope::Favorites => {
                if self.find.favorite_files.is_empty() {
                    self.find.status = "Add a favorite file first.".into();
                    return;
                }
                find_files::start_list(self.find.favorite_files.clone(), &options)
            }
        };
        match result {
            Ok(job) => {
                self.job = Some(job);
                self.show_results = true;
                self.find.status.clear();
                self.results_title = if preview { "Replace in Files preview" } else { "Find in Files" };
                if preview {
                    self.replace_plan = Some(ReplacePlan { options, backup: self.find.keep_backups });
                }
            }
            Err(e) => self.find.status = e,
        }
    }

    /// Lists each line of this file that has a match, in the results panel.
    fn list_matching_lines(&mut self) {
        if self.no_file() || self.docs[self.active].hex_mode {
            return;
        }
        note_search(&mut self.find.history, &self.find.find.clone());
        if self.find.find.is_empty() {
            self.find.status = "Type something to find first.".into();
            return;
        }
        if let Some(job) = &self.job {
            job.cancel.store(true, Ordering::Relaxed);
        }
        self.replace_plan = None;
        let options = self.search_options(false);
        let doc = &self.docs[self.active];
        let name = doc.path.clone().unwrap_or_else(|| PathBuf::from(&doc.untitled));
        let result = if doc.text.is_disk_backed() {
            // A huge file is searched where it lies, in the background; unsaved edits aren't included.
            find_files::start_list(vec![name], &options)
        } else {
            TextBuffer::build_regex(&options).map(|re| {
                let job = Arc::new(SearchJob::default());
                find_files::search_lines(&name, &doc.text.all_lines(), &re, &job);
                // One entry per line, however many matches it has.
                job.hits.lock().unwrap().dedup_by_key(|hit| hit.line);
                job.files_searched.store(1, Ordering::Relaxed);
                job.finished.store(true, Ordering::SeqCst);
                job
            })
        };
        match result {
            Ok(job) => {
                self.job = Some(job);
                self.show_results = true;
                self.find.status.clear();
                self.results_title = "Lines containing the find string";
            }
            Err(e) => self.find.status = e,
        }
    }

    /// Which open document a path from the results means, if any.
    fn doc_for_path(&self, path: &Path) -> Option<usize> {
        self.docs.iter().position(|d| match &d.path {
            Some(p) => same_path(p, path),
            None => !d.placeholder && Path::new(&d.untitled) == path,
        })
    }

    /// Goes ahead with a Replace in Files. Open files change in their windows, where they can be undone,
    /// and the rest are rewritten on another thread.
    fn start_replace_in_files(&mut self, files: Vec<PathBuf>) {
        let Some(plan) = self.replace_plan.take() else {
            return;
        };
        let mut report = ReplaceReport::default();
        let mut on_disk = Vec::new();
        let waiting = self.pending.is_some();
        for path in files {
            let Some(index) = self.doc_for_path(&path) else {
                on_disk.push(path);
                continue;
            };
            let doc = &mut self.docs[index];
            let reason = if doc.hex_mode {
                Some("it's open in hex mode")
            } else if doc.text.is_disk_backed() {
                Some("it's open, and too large to change in its window")
            } else if doc.text.read_only {
                Some("it's open and set to read-only")
            } else if doc.is_busy() || waiting {
                Some("it's open and busy")
            } else {
                None
            };
            if let Some(reason) = reason {
                report.skipped.push((path, reason.to_string()));
                continue;
            }
            match doc.text.replace_all(&plan.options) {
                Ok(0) => {}
                Ok(count) => {
                    report.open_files_changed += 1;
                    report.replaced += count;
                }
                Err(problem) => report.skipped.push((path, problem)),
            }
        }
        self.replace_job = Some(find_files::start_replace(on_disk, plan.options, plan.backup, report));
        self.find.status = "Replacing...".to_string();
    }

    fn search_open_files(&self, options: &SearchOptions) -> Result<Arc<SearchJob>, String> {
        let re = TextBuffer::build_regex(options)?;
        let job = Arc::new(SearchJob::default());
        let mut on_disk = Vec::new();
        for doc in self.docs.iter().filter(|d| !d.hex_mode && !d.placeholder) {
            let name = doc.path.clone().unwrap_or_else(|| PathBuf::from(&doc.untitled));
            if doc.text.is_disk_backed() {
                // A huge file is searched where it lies, in the background; unsaved edits aren't included.
                on_disk.push(name);
                continue;
            }
            find_files::search_lines(&name, &doc.text.all_lines(), &re, &job);
            job.files_searched.fetch_add(1, Ordering::Relaxed);
        }
        let worker = job.clone();
        std::thread::spawn(move || {
            let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                for path in on_disk {
                    if worker.cancel.load(Ordering::Relaxed) {
                        break;
                    }
                    find_files::search_file(&path, &re, &worker);
                }
            }));
            worker.finished.store(true, Ordering::SeqCst);
        });
        Ok(job)
    }

    fn open_hit(&mut self, hit: &Hit) {
        let index = self.docs.iter().position(|d| match &d.path {
            Some(p) => same_path(p, &hit.path),
            None => Path::new(&d.untitled) == hit.path,
        });
        let index = match index {
            Some(i) => i,
            None => {
                if !self.open_path(&hit.path) {
                    return;
                }
                self.active
            }
        };
        self.active = index;
        self.pending_focus = true;
        let doc = &mut self.docs[index];
        // A result's line is its line in the whole file.
        doc.widen();
        if doc.hex_mode || hit.line >= doc.text.line_count() {
            return;
        }
        // The file may have been edited since the search, so check the match is still there.
        let line = doc.text.line(hit.line);
        if line.get(hit.start..hit.end).is_some() {
            doc.text.select_match(hit.line, hit.start, hit.end);
        } else {
            doc.text.move_caret(hit.line, 0, false);
        }
    }

    // ---------- Commands ----------

    /// Pastes this text at the cursor, or over the selection.
    fn paste_here(&mut self, text: &str, ui: &Ui) {
        let doc = &mut self.docs[self.active];
        if doc.hex_mode {
            views::paste_into_hex(ui, &mut doc.hex, text);
        } else {
            views::paste_into(ui.ctx(), &mut doc.text, &mut self.clip, text);
        }
    }

    fn paste_from_clipboard(&mut self, ui: &Ui) {
        let text = arboard::Clipboard::new().and_then(|mut c| c.get_text());
        match text {
            Ok(text) => self.paste_here(&text, ui),
            Err(arboard::Error::ContentNotAvailable) => {}
            Err(e) => warn(&format!("Couldn't read the clipboard.\n\n{e}")),
        }
    }

    fn copy_selection(&mut self, ui: &Ui) -> bool {
        let index = self.active;
        let doc = &mut self.docs[index];
        if doc.hex_mode {
            views::copy_hex_selection(ui, &doc.hex);
            true
        } else {
            views::copy_text_selection(ui, &mut doc.text, &mut self.clip)
        }
    }


    fn message(&mut self, title: &str, body: String) {
        self.dialog = Dialog::Message { title: title.into(), body };
    }

    fn open_find(&mut self, tab: FindTab) {
        self.find.open = true;
        self.find.tab = tab;
        self.find.focus_box = true;
        self.find.status.clear();
        let doc = &self.docs[self.active];
        if self.prefs.find_from_selection && !doc.hex_mode && !doc.placeholder && doc.text.has_selection() && doc.text.caret_line == doc.text.anchor_line && !doc.text.is_block() {
            let text = doc.text.format.readable_text(&doc.text.selection_text());
            if text.len() <= MOST_FIND_FROM_SELECTION {
                self.find.find = text;
            }
        }
        if tab == FindTab::InFiles
            && self.prefs.find_in_file_folder
            && let Some(folder) = self.docs[self.active].path.as_deref().and_then(Path::parent)
        {
            self.find.folder = folder.display().to_string();
        }
    }

    fn run(&mut self, command: Command, ui: &Ui) {
        // While a script runs it's the only one giving commands. It takes itself out of here to give them.
        if self.script.is_some() {
            return;
        }
        // In a table, the clipboard commands work on the cell the cursor is in.
        if !self.no_file() && self.table_command(command, ui) {
            return;
        }
        let hex = self.docs[self.active].hex_mode;
        // Nothing may change a document while it is loading or saving, or while a Replace All runs.
        let changes_document = !matches!(
            command,
            Command::New
                | Command::Open
                | Command::CloseTab
                | Command::Copy
                | Command::SelectAll
                | Command::Find
                | Command::Replace
                | Command::FindInFiles
                | Command::GoTo
                | Command::CopyPath
                | Command::ToggleBookmark
                | Command::NextBookmark
                | Command::PreviousBookmark
                | Command::ZoomIn
                | Command::ZoomOut
                | Command::AlwaysOnTop
                | Command::FullScreen
                | Command::PreviousTab
                | Command::NextTab
                | Command::NewInstance
                | Command::Help
                | Command::WordCount
                | Command::ColumnSum
                | Command::GoToBrace
                | Command::SelectToBrace
                | Command::ComputeHash
                | Command::ConvertNumber
                | Command::FindNext
                | Command::FindPrevious
                | Command::NavBack
                | Command::NavForward
                | Command::ToggleWrap
                | Command::ToggleReadOnly
                | Command::ChooseFont
                | Command::ShowFileLists
                | Command::HexCopySelected
                | Command::Settings
                | Command::LastChange
                | Command::NextChange
                | Command::SplitHorizontal
                | Command::SplitVertical
                | Command::DuplicateWindow
                | Command::FilterSelection
                | Command::CollapseAll
                | Command::ExpandAll
                | Command::HideLines
                | Command::ShowLines
                | Command::MakeSelections
                | Command::Compare
                | Command::TogglePreview
                | Command::SetPreview(_)
                | Command::ParagraphSetup
                | Command::ProjectOpen
                | Command::ProjectNew
                | Command::ProjectClose
                | Command::ProjectSettings
                | Command::ProjectAddActive
                | Command::ProjectAddFiles
                | Command::ProjectAddFolder
                | Command::ProjectBackup
                | Command::ProjectRestore
                | Command::RecordMacro
                | Command::PlayMacro
                | Command::PlayMacroTimes
                | Command::LoadMacro
                | Command::ShowMacros
                | Command::NewScript
                | Command::LoadScript
                | Command::ManageAddOns
                | Command::NewAddOn
                | Command::OpenRemote
                | Command::ManageDevices
                | Command::Themes
                | Command::Website
                | Command::SendTo(_)
                | Command::RunOnMachine
                | Command::RunOnBuiltIn
                | Command::RunOn(_)
                | Command::StopRunning
                | Command::MachineMemory
                | Command::MachineScreen
                | Command::MachineBasicOut
                | Command::MachineBasicIn
                | Command::SourceControl
                | Command::GitWizard
                | Command::GitCompareHead
                | Command::GitHistory
                | Command::GitStageChange
                | Command::ToggleTable
                | Command::AddOn(_)
                | Command::ShowScripts
                | Command::ShowHistory
                | Command::Toggle(_)
                | Command::HidePanels
                | Command::CommandPalette
                | Command::About
                | Command::UsedFileBack
                | Command::UsedFileForward
                | Command::ShowInExplorer
                | Command::OpenWithDefault
                | Command::OpenBasicListing
                | Command::SavePictureAsPng
                | Command::Features
                | Command::SaveSpectrumTape
                | Command::SaveNextProgram
                | Command::SaveCommodoreProgram
                | Command::SaveBbcProgram
                | Command::Snippets
                | Command::RunTool
                | Command::StopTool
                | Command::PartHistory
                | Command::Tag(_)
                | Command::Untag
                | Command::NextTag
                | Command::PreviousTag
                | Command::Assist(_)
        );
        if self.no_file() && !command.works_without_file() {
            return;
        }
        if changes_document && (self.docs[self.active].is_busy() || self.pending.is_some()) {
            self.message("Please wait", "The file is still loading or saving. This will be available when it finishes.".into());
            return;
        }
        if command.edits() && self.docs[self.active].text.read_only {
            self.tell_read_only();
            return;
        }
        if command.needs_whole_file() && self.docs[self.active].narrow.is_some() {
            self.message("Narrowed", "This needs the whole file. Press Show the whole file first.".into());
            return;
        }
        self.record_command(command);
        if self.pending.is_some() && matches!(command, Command::CloseTab | Command::PreviousTab | Command::NextTab | Command::UsedFileBack | Command::UsedFileForward | Command::New | Command::Open) {
            self.pending = None;
            self.find.status.clear();
        }
        match command {
            Command::New => self.new_document(),
            Command::Open => self.open_dialog(),
            Command::Save => self.save_with_hooks(self.active),
            Command::SaveAs => {
                self.save_as(self.active);
            }
            Command::CloseTab => self.close_tab(self.active),
            Command::Undo => {
                let doc = self.doc();
                if hex { doc.hex.undo() } else { doc.text.undo() }
            }
            Command::Redo => {
                let doc = self.doc();
                if hex { doc.hex.redo() } else { doc.text.redo() }
            }
            Command::Cut if !hex => {
                let doc = &mut self.docs[self.active];
                views::cut_text(ui.ctx(), &mut doc.text, &mut self.clip);
            }
            Command::Cut => {
                if self.copy_selection(ui) {
                    self.doc().hex.delete_selection();
                }
            }
            Command::Copy if !hex => {
                let doc = &mut self.docs[self.active];
                views::copy_text(ui.ctx(), &mut doc.text, &mut self.clip);
            }
            Command::Copy => {
                self.copy_selection(ui);
            }
            Command::Print => self.print_document(),
            Command::Toggle(flag) => self.toggle_flag(flag, ui),
            Command::HidePanels => self.panels.hide_all(),
            Command::CommandPalette => self.dialog = Dialog::Palette(menus::PaletteState::new()),
            Command::About => self.about(),
            Command::ShowHistory => self.panels.history = !self.panels.history,
            Command::Narrow => self.narrow_command(),
            Command::Widen => self.widen_command(),
            Command::RecordMacro | Command::PlayMacro | Command::PlayMacroTimes | Command::LoadMacro | Command::ShowMacros | Command::NewScript | Command::LoadScript | Command::ShowScripts => self.script_command(command, ui),
            Command::ManageAddOns => {
                self.addons.refresh();
                let shared = self.addons.shared.as_ref().map_or_else(String::new, |folder| folder.display().to_string());
                self.dialog = Dialog::AddOns { shared };
            }
            Command::NewAddOn => self.dialog = Dialog::Script(script_host::ScriptBox::new_addon()),
            Command::OpenRemote => self.open_remote_box(None),
            Command::SaveRemote => {
                let id = self.docs[self.active].id;
                self.open_remote_box(Some(id));
            }
            Command::AddOn(number) => self.run_addon_command(number, ui),
            // An emulator in the list runs the file rather than being sent it.
            Command::SendTo(device) if matches!(self.sends.devices.get(device).map(|d| &d.place), Some(crate::deploy::Place::Emulator { .. })) => self.run_on_target(run_host::Target::Device(device)),
            Command::SendTo(device) => self.open_send_box(device),
            Command::RunOnMachine => self.run_on_machine(),
            Command::RunOnBuiltIn => self.run_on_target(run_host::Target::BuiltIn),
            Command::RunOn(device) => self.run_on_target(run_host::Target::Device(device)),
            Command::StopRunning => self.stop_all_sessions(),
            Command::ChooseAssembler => self.open_assembler_box(),
            Command::AssembleAndSave => self.assemble_and_save(),
            Command::AssembleBinary => self.assemble_binary(),
            Command::NewAs(index) => self.new_of_kind(index),
            Command::GoToDefinition => self.go_to_definition(),
            Command::FindUses => self.find_uses(),
            Command::RenameLabel => self.open_rename_box(),
            Command::Disassemble => self.open_disassemble_box(),
            Command::NewImage => self.open_new_image_box(),
            Command::ToggleTerminal => self.toggle_terminal(),
            Command::RunTask(index) => {
                if let Some(task) = self.tasks_here().get(index).cloned() {
                    self.run_task(task);
                }
            }
            Command::BuildTask => self.run_build_task(),
            Command::TestTask => self.run_test_task(),
            Command::EditTasks => self.edit_project_tasks(),
            Command::ScreenDesigner => {
                if self.screen_designer.is_none() {
                    let mut designer = crate::screen_designer::Designer::default();
                    // A Spectrum screen in front is opened in it.
                    if !self.no_file() && self.docs[self.active].path_or_name().extension().is_some_and(|e| e.eq_ignore_ascii_case("scr"))
                        && let Some(screen) = crate::screen_designer::Screen::from_scr(&self.document_bytes())
                    {
                        designer.screen = screen;
                    }
                    self.screen_designer = Some(Box::new(designer));
                }
            }
            Command::PixelEditor => {
                if self.pixel_editor.is_none() {
                    self.pixel_editor = Some(Box::new(crate::pixel_editor::Editor::new(crate::pixel_editor::Kind::Characters)));
                }
            }
            Command::EditNewKinds => self.open_new_kinds_box(),
            Command::MachineMemory => self.open_machine_memory(),
            Command::MachineScreen => self.open_machine_screen(),
            Command::MachineBasicOut => self.take_machine_basic(),
            Command::MachineBasicIn => self.put_machine_basic(),
            Command::SourceControl => {
                self.git.open = !self.git.open;
                self.git.refresh();
            }
            Command::GitWizard => self.open_git_wizard(),
            Command::GitCompareHead => self.compare_with_commit("HEAD"),
            Command::GitHistory => self.git_history(),
            Command::GitStageChange => self.stage_change_at_cursor(),
            Command::GitUndoChange => self.undo_change_at_cursor(),
            Command::ToggleTable => self.toggle_table(),
            Command::Themes => self.open_theme_box(theme_host::ThemePage::Documents),
            Command::Website => {
                if let Err(problem) = crate::platform::open_web_page("https://limoniter.com") {
                    warn(&problem);
                }
            }
            Command::SendAgain => self.send_again(),
            Command::ManageDevices => self.open_devices_box(),
            Command::ProjectOpen => {
                if let Some(path) = rfd::FileDialog::new().set_title("Open project").add_filter("Limoniter project", &[project::EXTENSION]).pick_file() {
                    self.open_project(&path);
                }
            }
            Command::ProjectNew => {
                if let Some(path) = rfd::FileDialog::new().set_title("New project").set_file_name(format!("project.{}", project::EXTENSION)).add_filter("Limoniter project", &[project::EXTENSION]).save_file() {
                    self.new_project(path);
                }
            }
            Command::ProjectClose => self.close_project(),
            Command::ProjectSettings => {
                if let Some(project) = &self.project {
                    self.dialog = Dialog::ProjectSettings { name: project.name.clone(), notes: project.notes.clone() };
                }
            }
            Command::ProjectAddActive => {
                if let Some(path) = self.docs[self.active].path.clone() {
                    self.add_to_project([path]);
                } else {
                    self.message("Add to project", "Save this file first, so it has somewhere to be found.".into());
                }
            }
            Command::ProjectAddFiles => {
                if self.project.is_some()
                    && let Some(paths) = with_open_filters(rfd::FileDialog::new().set_title("Add files to the project")).pick_files()
                {
                    self.add_to_project(paths);
                }
            }
            Command::ProjectAddFolder => {
                if self.project.is_some()
                    && let Some(folder) = rfd::FileDialog::new().set_title("Add a folder to the project").pick_folder()
                {
                    let (paths, cut_short) = project::files_in_folder(&folder);
                    self.add_to_project(paths);
                    if cut_short {
                        self.message("Add to project", "That folder has a great many files, so only the first 20,000 were added.".into());
                    }
                }
            }
            Command::ProjectBackup => self.backup_project(),
            Command::ProjectRestore => {
                if let Some(project) = &self.project {
                    let backups = project::list_backups(project);
                    if backups.is_empty() {
                        self.message("Restore project", "This project has no backups yet.".into());
                    } else {
                        self.dialog = Dialog::RestorePick { backups, chosen: 0 };
                    }
                }
            }
            Command::TogglePreview => {
                let doc = self.doc();
                doc.preview_view = if doc.preview_view == settings::PreviewView::Text { settings::PreviewView::Split } else { settings::PreviewView::Text };
                self.pending_focus = true;
            }
            Command::SetPreview(view) => {
                self.doc().preview_view = view;
                self.pending_focus = true;
            }
            Command::Compare => self.begin_compare(),
            Command::LastChange | Command::NextChange => {
                self.doc().text.go_to_change(command == Command::NextChange);
            }
            Command::Settings => {
                let file_types = settings::FileTypesState::from_registered(associations::registered(), associations::this_program_shown());
                self.dialog = Dialog::Settings(settings::WindowState { focus: true, file_types, ..settings::WindowState::default() });
            }
            Command::Paste => self.paste_from_clipboard(ui),
            Command::SelectAll => {
                let doc = self.doc();
                if hex { doc.hex.select_all() } else { doc.text.select_all() }
            }
            Command::Find => self.open_find(FindTab::Find),
            Command::Replace => self.open_find(FindTab::Replace),
            Command::FindInFiles => self.open_find(FindTab::InFiles),
            Command::FindNext | Command::FindPrevious => {
                if self.find.find.is_empty() {
                    self.open_find(FindTab::Find);
                } else {
                    self.run_find(command == Command::FindPrevious);
                }
            }
            Command::GoTo => self.dialog = Dialog::GoTo { value: String::new(), error: String::new() },
            Command::ToggleHex => self.toggle_hex(),
            // The panes and windows are views of the text, so hex mode has one of each.
            Command::SplitHorizontal | Command::SplitVertical | Command::DuplicateWindow if hex => {}
            Command::SplitHorizontal => self.toggle_split(false),
            Command::SplitVertical => self.toggle_split(true),
            Command::DuplicateWindow => {
                self.doc().duplicate_window();
                self.pending_focus = true;
            }
            Command::FilterSelection => self.toggle_filter(),
            Command::ViewRegular => self.set_view(None),
            Command::ViewHex => self.set_view(Some(false)),
            Command::ViewEbcdic => self.set_view(Some(true)),
            Command::HexInsertDelete => {
                let bytes = &self.docs[self.active].hex;
                let selected = bytes.caret.abs_diff(bytes.anchor);
                if hex {
                    self.dialog = Dialog::HexInsertDelete { count: selected.max(1).to_string(), delete: false, fill: "00".to_string(), error: String::new(), focus: true };
                }
            }
            Command::HexCopySelected => self.copy_hex_view(ui),
            Command::CopyPath => {
                if let Some(path) = &self.docs[self.active].path {
                    ui.ctx().copy_text(path.display().to_string());
                }
            }
            Command::ZoomIn => self.view.font_size = (self.view.font_size + 1.0).min(40.0),
            Command::ZoomOut => self.view.font_size = (self.view.font_size - 1.0).max(8.0),
            Command::AlwaysOnTop => {
                self.always_on_top = !self.always_on_top;
                let level = if self.always_on_top { egui::WindowLevel::AlwaysOnTop } else { egui::WindowLevel::Normal };
                ui.ctx().send_viewport_cmd(ViewportCommand::WindowLevel(level));
            }
            Command::FullScreen => {
                self.full_screen = !self.full_screen;
                ui.ctx().send_viewport_cmd(ViewportCommand::Fullscreen(self.full_screen));
            }
            Command::PreviousTab => self.step_tab(false),
            Command::NextTab => self.step_tab(true),
            Command::NewInstance => {
                let started = std::env::current_exe().and_then(|exe| std::process::Command::new(exe).spawn());
                if let Err(e) = started {
                    warn(&format!("Couldn't open a new window.\n\n{e}"));
                }
            }
            Command::Help => self.open_help(),
            Command::ReopenAs => self.begin_pick(Purpose::Reopen),
            Command::SaveCopyAs => self.begin_pick(Purpose::SaveCopy),
            Command::ChoosePreset => self.begin_presets(),
            Command::InsertByte => self.dialog = Dialog::InsertByte { value: String::new(), error: String::new() },
            Command::NavBack | Command::NavForward => self.go_through_history(command == Command::NavBack),
            Command::ToggleWrap => self.toggle_word_wrap(),
            Command::ToggleReadOnly if self.docs[self.active].filter.is_some() => self.tell_read_only(),
            Command::ToggleReadOnly => {
                let on = !self.docs[self.active].text.read_only;
                self.doc().set_read_only(on);
            }
            Command::ChooseFont => self.dialog = Dialog::Fonts { error: String::new() },
            Command::ShowFileLists => self.panels.file_lists = true,
            Command::InsertDate => self.type_at_cursor(&chrono::Local::now().format("%d/%m/%Y").to_string()),
            Command::InsertTime => self.type_at_cursor(&chrono::Local::now().format("%H:%M:%S").to_string()),
            Command::InsertDateTime => self.type_at_cursor(&chrono::Local::now().format("%d/%m/%Y %H:%M:%S").to_string()),
            Command::InsertIsoDate => self.type_at_cursor(&chrono::Local::now().format("%Y-%m-%d").to_string()),
            Command::InsertFileName => {
                let name = self.docs[self.active].name();
                self.type_at_cursor(&name);
            }
            Command::InsertFilePath => {
                let doc = &self.docs[self.active];
                let path = doc.path.as_ref().map_or_else(|| doc.name(), |path| path.display().to_string());
                self.type_at_cursor(&path);
            }
            Command::InsertPageBreak => self.type_at_cursor("\u{C}"),
            Command::InsertColor => self.dialog = Dialog::Color { rgb: [0, 122, 204], form: 0 },
            Command::InsertFile => {
                if let Some(path) = rfd::FileDialog::new().set_title("Insert file").pick_file() {
                    match tools::text_of_file(&path) {
                        Ok(text) => {
                            let doc = self.doc();
                            if doc.hex_mode {
                                doc.hex.paste_text(&text);
                            } else {
                                doc.text.paste_text(&text, false);
                            }
                        }
                        Err(problem) => self.message("Insert file", problem),
                    }
                }
            }
            Command::RecoverEdits => {
                if let Some(root) = recovery_root() {
                    self.offer_recovered(&root, true);
                }
            }
            Command::SwapClipboard => self.swap_with_clipboard(ui),
            Command::UsedFileBack | Command::UsedFileForward => self.cycle_used(command == Command::UsedFileBack),
            Command::SaveSelection => self.save_selection(),
            Command::OpenBasicListing => self.open_basic_listing(),
            Command::GraphMode => self.toggle_graph_mode(),
            Command::UdgPictures => self.choose_udg_pictures(),
            Command::SavePictureAsPng => self.save_picture_as_png(),
            Command::Features => self.open_features(),
            Command::SaveSpectrumTape => self.dialog = Dialog::SaveBasic { machine: basic::Machine::Spectrum, run_from: String::new(), error: String::new() },
            Command::SaveNextProgram => self.dialog = Dialog::SaveBasic { machine: basic::Machine::Next, run_from: String::new(), error: String::new() },
            Command::SaveCommodoreProgram => {
                let cbm = basic::CbmMachine::of(self.docs[self.active].text.format.charset);
                self.dialog = Dialog::SaveBasic { machine: basic::Machine::Commodore(cbm), run_from: String::new(), error: String::new() };
            }
            Command::SaveBbcProgram => self.save_tokenised(basic::Machine::Bbc, None),
            Command::RenameFile => self.begin_rename(),
            Command::ShowInExplorer => self.show_in_explorer(),
            Command::OpenWithDefault => self.open_with_usual_program(),
            Command::Snippets => self.dialog = Dialog::Snippets(file_tools::SnippetBox { focus: true, ..Default::default() }),
            Command::RunTool => self.begin_run_tool(),
            Command::StopTool => self.stop_tool(),
            _ if hex => {}
            Command::SplitFile => self.begin_split(),
            Command::PartHistory => self.choose_part(),
            Command::Tag(colour) => self.tag_here(colour),
            Command::Untag => self.untag_here(),
            Command::NextTag | Command::PreviousTag => self.go_to_tag(command == Command::NextTag),
            Command::Assist(task) => self.begin_assist(task),
            Command::NextMistake => self.next_mistake(),
            Command::ConvertTo => self.begin_pick(Purpose::Convert),
            Command::Comment | Command::Uncomment => {
                let doc = &mut self.docs[self.active];
                if let Some(marks) = tools::comment_marks(&doc.language_name()) {
                    if command == Command::Comment {
                        doc.text.comment_lines(&marks);
                    } else {
                        doc.text.uncomment_lines(&marks);
                    }
                }
            }
            Command::CollapseAll => self.doc().text.collapse_all(),
            Command::ExpandAll => self.doc().text.expand_all(),
            Command::HideLines => {
                if !self.doc().text.hide_selected_lines() {
                    self.message("Hide lines", "Select two or more lines first. The first of them stays showing, and stands for the rest.".into());
                }
            }
            Command::ShowLines => self.doc().text.show_selected_lines(),
            Command::MakeSelections => {
                if let Err(problem) = self.doc().text.make_selections() {
                    self.message("Make selections", problem);
                }
            }
            Command::Reindent => self.reindent_lines(),
            Command::TextObject(action, object) => self.text_object(action, object),
            Command::WrapToLineEnds => self.dialog = Dialog::WrapColumn { value: self.prefs.paragraph.width.to_string(), error: String::new(), focus: true },
            Command::LineEndsToWrap => self.change_lines(|rows, _| reformat::line_ends_to_wrap(rows)),
            Command::ParagraphReformat => {
                let setup = self.prefs.paragraph;
                self.change_paragraph(move |rows| reformat::reformat_paragraphs(rows, &setup));
            }
            Command::ParagraphSetup => {
                let setup = self.prefs.paragraph;
                self.dialog = Dialog::ParagraphSetup { width: setup.width.to_string(), left: setup.left.to_string(), first: setup.first.to_string(), align: setup.align, error: String::new() };
            }
            Command::AlignText(align) => {
                self.prefs.paragraph.align = align;
                let setup = self.prefs.paragraph;
                self.change_paragraph(move |rows| reformat::align_lines(rows, align, setup.width, setup.left));
            }
            Command::Format(kind) => self.run_formatter(kind),
            Command::Upper => self.doc().text.change_case(CaseChange::Upper),
            Command::Lower => self.doc().text.change_case(CaseChange::Lower),
            Command::SentenceCase => self.doc().text.change_case(CaseChange::Sentence),
            Command::TitleCase => self.doc().text.change_case(CaseChange::Title),
            Command::ToggleColumn if self.view.word_wrap => {
                self.message("Column mode", "Column mode can't be used with Word wrap on. Turn off Word wrap first.".into());
            }
            Command::ToggleColumn => {
                let text = &mut self.doc().text;
                let on = !text.column_mode();
                text.set_column_mode(on);
            }
            Command::ColumnFill => self.dialog = Dialog::Fill { value: String::new(), skip_last: self.fill_skip_last },
            Command::ColumnCut | Command::ColumnDelete => {
                self.dialog = Dialog::ColumnRemove { cut: command == Command::ColumnCut, count: "0".into(), error: String::new(), focus: true };
            }
            Command::JustifyLeft => self.doc().text.justify_columns(Justify::Left),
            Command::JustifyCenter => self.doc().text.justify_columns(Justify::Center),
            Command::JustifyRight => self.doc().text.justify_columns(Justify::Right),
            Command::CsvToFixed => {
                if let Some(rows) = self.doc().text.rows_for_convert() {
                    let delimiter = columns::guess_delimiter(&self.doc().text.rows_text(rows.0, rows.1));
                    self.dialog = Dialog::CsvToFixed { rows, delimiter, error: String::new() };
                }
            }
            Command::FixedToCsv => {
                if let Some(rows) = self.doc().text.rows_for_convert() {
                    let starts = columns::starts_text(&columns::detect_starts(&self.doc().text.rows_text(rows.0, rows.1)));
                    self.dialog = Dialog::FixedToCsv { rows, delimiter: ',', starts, error: String::new(), focus: true };
                }
            }
            Command::ColumnNumber => {
                self.dialog = Dialog::Number { first: "1".into(), step: "1".into(), error: String::new() };
            }
            Command::ColumnSum => self.show_column_sum(),
            Command::MoveLineUp => self.doc().text.move_lines(true),
            Command::MoveLineDown => self.doc().text.move_lines(false),
            Command::Duplicate => self.doc().text.duplicate_lines(),
            Command::Join => self.doc().text.join_lines(),
            Command::SortAscending => self.doc().text.sort_lines(false),
            Command::SortDescending => self.doc().text.sort_lines(true),
            Command::WordCount if self.docs[self.active].text.is_disk_backed() && !self.docs[self.active].text.has_selection() => {
                if self.big_job.is_some() {
                    self.message("Word Count", "Please wait for the file being worked through to finish.".into());
                } else {
                    // Counted in the background, a stretch of lines at a time; the result is shown when it's done.
                    let doc = &self.docs[self.active];
                    self.big_job = Some(crate::big_jobs::count(doc.text.store().clone(), doc.text.insert_eol().char_count(), doc.id));
                }
            }
            Command::WordCount if self.docs[self.active].text.selection_too_large_to_copy() => {
                self.message("Word Count", "That selection is too large to count. Select fewer lines.".into());
            }
            Command::WordCount => {
                let doc = &self.docs[self.active];
                let c = doc.text.counts(doc.text.insert_eol().char_count());
                self.message("Word Count", format!("Number of Words: {}\nNumber of Unwrapped Lines: {}\nNumber of characters (non-space): {}", c.words, c.lines, c.non_space));
            }
            Command::TrimLeading => self.doc().text.trim_leading(),
            Command::TrimTrailing => self.doc().text.trim_trailing(),
            Command::TabsToSpaces => self.doc().text.tabs_to_spaces(),
            Command::SpacesToTabs => self.doc().text.spaces_to_tabs(),
            Command::Indent => self.doc().text.tab_key(false),
            Command::Outdent => self.doc().text.tab_key(true),
            Command::ToggleBookmark => {
                let line = self.docs[self.active].text.caret_line;
                self.doc().text.toggle_bookmark(line);
            }
            Command::NextBookmark | Command::PreviousBookmark => {
                self.doc().text.go_to_bookmark(command == Command::NextBookmark);
            }
            Command::FormatJson | Command::CompactJson => self.format_json(command == Command::FormatJson),
            Command::ComputeHash => {
                match self.docs[self.active].text.selection_or_all() {
                    Some(text) => {
                        let scope = if self.docs[self.active].text.has_selection() { "The selection" } else { "This document" };
                        let hash = tools::sha256_text(&text);
                        ui.ctx().copy_text(hash.clone());
                        self.message("Compute Hash", format!("{scope}, as text:\n\nSHA-256: {hash}\n\nThe hash has been copied."));
                    }
                    None => self.message("Compute Hash", "This file is too large to hash as text. File properties can hash the saved file, whatever its size.".into()),
                }
            }
            Command::GenerateGuid => self.doc().text.type_text(&tools::new_guid()),
            Command::ConvertNumber => {
                let text = self.docs[self.active].text.selection_for_preview();
                let body = tools::convert_number(&text).unwrap_or_else(|| "Select a whole number first, such as 255, 0xFF or 0b1010.".to_string());
                self.message("Convert Number", body);
            }
            Command::GoToBrace | Command::SelectToBrace => {
                if !self.doc().text.go_to_brace(command == Command::SelectToBrace) {
                    self.message("Braces", "Put the cursor next to a bracket that has a match: ( ) [ ] { }".into());
                }
            }
        }
        if !matches!(self.dialog, Dialog::None) {
            self.pending_focus = false;
        } else if !self.find.open || !self.find.focus_box {
            self.pending_focus = true;
        }
    }

    fn tell_read_only(&mut self) {
        if self.docs[self.active].filter.is_some() {
            self.message("Filter on selection", "Editing is off while filtering, because a selection would reach into lines that can't be seen. Press Filter on selection again to end it.".into());
            return;
        }
        self.message("Read-only", "This file is set to read-only. Turn off Read-only on the Edit menu, or click Read-only in the status bar, to change it.".into());
    }

    /// Puts text at the cursor as one undoable action, in the document's own encoding.
    fn type_at_cursor(&mut self, text: &str) {
        let doc = self.doc();
        if doc.hex_mode {
            doc.hex.paste_text(text);
        } else {
            doc.text.type_text(text);
        }
    }

    /// Long lines continue on the next row. A column block makes no sense across rows, so it's refused, as in the original.
    fn toggle_word_wrap(&mut self) {
        self.view.word_wrap = !self.view.word_wrap;
        if self.view.word_wrap {
            for doc in &mut self.docs {
                doc.text.set_column_mode(false);
                doc.text.end_temporary_block();
            }
        }
        let doc = self.doc();
        doc.text.reveal = true;
    }

    /// Back or forward to where the cursor was. A place in a document since closed is passed over.
    fn go_through_history(&mut self, back: bool) {
        let doc = &self.docs[self.active];
        let mut now = Place { doc: doc.id, line: doc.text.caret_line, col: doc.text.caret_col };
        loop {
            let target = if back { self.jumps.go_back(now) } else { self.jumps.go_forward(now) };
            let Some(target) = target else {
                return;
            };
            if let Some(index) = self.docs.iter().position(|doc| doc.id == target.doc && !doc.hex_mode) {
                self.active = index;
                let text = &mut self.docs[index].text;
                text.move_caret(target.line, target.col, false);
                text.set_desired_col();
                self.pending_focus = true;
                return;
            }
            now = target;
        }
    }

    fn format_json(&mut self, pretty: bool) {
        let doc = &self.docs[self.active];
        if doc.text.is_block() {
            self.message("JSON", "Turn off Column Mode first, then try again.".into());
            return;
        }
        let Some(text) = doc.text.selection_or_all() else {
            self.message("JSON", "This file is too large to reformat as a whole. Select the part you want instead.".into());
            return;
        };
        match tools::format_json(&text, pretty) {
            Ok(formatted) => self.doc().text.replace_selection_or_all(&formatted),
            Err(problem) => self.message("JSON", problem),
        }
    }

    fn show_column_sum(&mut self) {
        let text = &self.docs[self.active].text;
        if !text.has_selection() || !text.is_block() {
            self.message("Sum Selection", "Select a column first: turn on Column Mode (Alt+C), or hold Alt and drag.".into());
            return;
        }
        let s = text.sum_column();
        let mut body = format!("Sum: {:.*}\nNumbers added: {}", s.decimals, s.sum, s.numbers);
        if s.numbers > 0 {
            body += &format!("\nAverage: {}", s.sum / s.numbers as f64);
        }
        if s.skipped > 0 {
            body += &format!("\nSkipped (not numbers): {}", s.skipped);
        }
        self.message("Sum Selection", body);
    }

    fn shortcuts(&mut self, ui: &Ui) {
        let ctx = ui.ctx().clone();
        // While a box is open the document behind it takes no commands from the keyboard.
        let in_editor = !self.full_preview_shown && self.script.is_none() && matches!(self.dialog, Dialog::None) && self.pending.is_none() && !self.docs[self.active].is_busy() && ctx.memory(|m| m.focused().is_none_or(views::is_editor_id));
        // Cmd on a Mac, Ctrl elsewhere.
        let ctrl = Modifiers::COMMAND;
        let ctrl_shift = Modifiers::COMMAND | Modifiers::SHIFT;
        let mac = cfg!(target_os = "macos");
        // Option and a letter types a character on a Mac, so those shortcuts take Control and Option there.
        let alt_letter = if mac { Modifiers::CTRL | Modifiers::ALT } else { Modifiers::ALT };
        // Option and an arrow moves by a word on a Mac, so back and forward are Cmd+[ and Cmd+] there, as browsers have.
        let (back, forward) = if mac { ((ctrl, Key::OpenBracket), (ctrl, Key::CloseBracket)) } else { ((Modifiers::ALT, Key::ArrowLeft), (Modifiers::ALT, Key::ArrowRight)) };
        // Longer combinations come first, because Ctrl+S also matches Ctrl+Shift+S.
        // These are taken whether or not they can act, because Ctrl+W also matches Ctrl+Shift+W, and closes the tab.
        let narrowing = if self.docs[self.active].narrow.is_some() { Command::Widen } else { Command::Narrow };
        let in_text_shifted = [
            (Key::Q, Command::TextObject(text_objects::Action::SelectInside, text_objects::Object::Quotes)),
            (Key::B, Command::TextObject(text_objects::Action::SelectInside, text_objects::Object::Brackets)),
            (Key::T, Command::TextObject(text_objects::Action::SelectInside, text_objects::Object::Tag)),
            (Key::W, Command::TextObject(text_objects::Action::SelectInside, text_objects::Object::Word)),
            (Key::L, Command::TextObject(text_objects::Action::SelectInside, text_objects::Object::Line)),
            (Key::G, Command::TextObject(text_objects::Action::SelectInside, text_objects::Object::Paragraph)),
            (Key::N, narrowing),
            (Key::H, Command::ShowHistory),
            (Key::V, Command::SwapClipboard),
            (Key::Num1, Command::Tag(0)),
            (Key::Num2, Command::Tag(1)),
            (Key::Num3, Command::Tag(2)),
            (Key::Num4, Command::Tag(3)),
            (Key::Num5, Command::Tag(4)),
            (Key::Num6, Command::Tag(5)),
            (Key::Num0, Command::Untag),
            (Key::Num9, Command::GraphMode),
            (Key::A, Command::Assist(assistant::Task::Fix)),
        ];
        for (key, command) in in_text_shifted {
            if ctx.input_mut(|i| i.consume_shortcut(&KeyboardShortcut::new(ctrl_shift, key))) && in_editor {
                self.run(command, ui);
            }
        }
        // Before Ctrl+H, which would take it otherwise.
        if ctx.input_mut(|i| i.consume_shortcut(&KeyboardShortcut::new(ctrl | Modifiers::ALT, Key::H))) && in_editor {
            self.run(Command::PartHistory, ui);
        }
        // Before Ctrl+S, likewise.
        if ctx.input_mut(|i| i.consume_shortcut(&KeyboardShortcut::new(ctrl | Modifiers::ALT, Key::S))) && in_editor && !self.no_file() {
            self.run(Command::SendAgain, ui);
        }
        let always = [
            // Control and Tab on every system, a Mac too.
            (Modifiers::CTRL | Modifiers::SHIFT, Key::Tab, Command::UsedFileForward),
            (Modifiers::CTRL, Key::Tab, Command::UsedFileBack),
            (ctrl, Key::J, Command::Snippets),
            (Modifiers::NONE, Key::F5, Command::RunTool),
            (Modifiers::NONE, Key::F9, Command::RunOnMachine),
            (Modifiers::CTRL, Key::Backtick, Command::ToggleTerminal),
            (ctrl_shift, Key::B, Command::BuildTask),
            (ctrl_shift, Key::T, Command::TestTask),
            (Modifiers::NONE, Key::F12, Command::GoToDefinition),
            (Modifiers::SHIFT, Key::F12, Command::FindUses),
            (ctrl, Key::F12, Command::RenameLabel),
            (Modifiers::NONE, Key::F6, Command::ToggleTable),
            (Modifiers::NONE, Key::F7, Command::NextMistake),
            (ctrl_shift, Key::R, Command::RecordMacro),
            (ctrl_shift, Key::P, Command::PlayMacro),
            (ctrl_shift, Key::S, Command::SaveAs),
            (ctrl_shift, Key::F, Command::FindInFiles),
            (ctrl_shift, Key::G, Command::SourceControl),
            (ctrl, Key::N, Command::New),
            (ctrl, Key::O, Command::Open),
            (ctrl, Key::P, Command::Print),
            (ctrl, Key::S, Command::Save),
            (ctrl, Key::W, Command::CloseTab),
            (ctrl, Key::F, Command::Find),
            (ctrl, Key::R, Command::Replace),
            (ctrl, Key::G, Command::GoTo),
            (ctrl, Key::H, Command::ToggleHex),
            (back.0, back.1, Command::NavBack),
            (forward.0, forward.1, Command::NavForward),
            (Modifiers::SHIFT, Key::F3, Command::FindPrevious),
            (Modifiers::NONE, Key::F3, Command::FindNext),
            (Modifiers::NONE, Key::F1, Command::CommandPalette),
        ];
        let in_text_only = [
            (ctrl, Key::Z, Command::Undo),
            (ctrl_shift, Key::Z, Command::Redo),
            (ctrl, Key::Y, Command::Redo),
            (ctrl, Key::A, Command::SelectAll),
            (ctrl, Key::D, Command::Duplicate),
            (ctrl, Key::F2, Command::ToggleBookmark),
            (Modifiers::SHIFT, Key::F2, Command::PreviousBookmark),
            (Modifiers::SHIFT, Key::F4, Command::PreviousTag),
            (Modifiers::NONE, Key::F4, Command::NextTag),
            (Modifiers::NONE, Key::F2, Command::NextBookmark),
            (alt_letter, Key::C, Command::ToggleColumn),
            (alt_letter, Key::U, Command::Upper),
            (alt_letter, Key::L, Command::Lower),
        ];
        for (modifiers, key, command) in always {
            if ctx.input_mut(|i| i.consume_shortcut(&KeyboardShortcut::new(modifiers, key))) {
                self.run(command, ui);
            }
        }
        if in_editor {
            for (modifiers, key, command) in in_text_only {
                if ctx.input_mut(|i| i.consume_shortcut(&KeyboardShortcut::new(modifiers, key))) {
                    self.run(command, ui);
                }
            }
            self.script_shortcuts(ui);
        }
    }

    // ---------- Ribbon ----------

    /// Applies a change between tokens and small cells to every open file, keeping cursors on the same characters.
    fn apply_source_view(&mut self) {
        if self.view.source_view == text_buffer::source_view() {
            return;
        }
        let places: Vec<_> = self
            .docs
            .iter()
            .map(|d| {
                charset_widths(&d.text.format);
                (d.text.caret_char_position(), d.text.anchor_char_position())
            })
            .collect();
        text_buffer::set_source_view(self.view.source_view);
        for (doc, (caret, anchor)) in self.docs.iter_mut().zip(places) {
            charset_widths(&doc.text.format);
            doc.text.after_tab_size_change(caret, anchor);
        }
    }

    /// Applies a new tab size to every open file, keeping cursors on the same characters.
    fn apply_tab_size(&mut self) {
        if self.view.tab_size == text_buffer::tab_size() {
            return;
        }
        let places: Vec<_> = self.docs.iter().map(|d| (d.text.caret_char_position(), d.text.anchor_char_position())).collect();
        text_buffer::set_tab_size(self.view.tab_size);
        for (doc, (caret, anchor)) in self.docs.iter_mut().zip(places) {
            doc.text.after_tab_size_change(caret, anchor);
        }
    }

    /// Everything worth remembering between runs, as the settings file holds it.
    fn current_settings(&self) -> String {
        format!(
            "{}{}{}design=2\nfile_tabs={}\nstatus_bar={}\nnew_file_preset={}\n{}{}{}{}{}{}{}{}{}{}{}",
            self.view.to_settings(),
            self.prefs.to_settings(),
            self.panels.to_settings(),
            self.show_file_tabs,
            self.show_status,
            self.new_file_preset.id,
            self.font_file.iter().map(|path| format!("font_file={}\n", path.display())).collect::<String>(),
            self.panels.recent.iter().map(|path| format!("recent_file={}\n", path.display())).collect::<String>(),
            self.find.history.iter().map(|text| format!("find_history={text}\n")).collect::<String>(),
            self.find.favourites.iter().map(|text| format!("find_favourite={text}\n")).collect::<String>(),
            self.find.favorite_files.iter().map(|path| format!("favorite_file={}\n", path.display())).collect::<String>(),
            self.recent_projects.iter().map(|path| format!("recent_project={}\n", path.display())).collect::<String>(),
            self.library.to_settings() + self.addons.to_settings().as_str() + self.remote_settings().as_str() + self.sends.to_settings().as_str() + self.running.to_settings().as_str() + self.git.to_settings().as_str() + self.amiga_palette.to_setting().as_str() + self.themes.to_settings().as_str(),
            self.current_macro.as_ref().and_then(|current| current.path.as_ref()).map_or_else(String::new, |path| format!("current_macro={}\n", path.display())),
            file_tools::snippets_to_settings(&self.snippets),
            self.assistant.to_settings(),
            self.run_history.iter().map(|line| format!("run_command={line}\n")).collect::<String>() + new_kinds::kinds_to_settings(&self.new_kinds).as_str() + self.lsp.to_settings().as_str()
        )
    }

    /// Not being able to save a preference shouldn't get in the way of editing, so a failure is ignored.
    /// A screenshot run changes nothing, so it leaves the settings alone.
    fn save_settings_if_changed(&mut self) {
        if self.screenshot.is_some() {
            return;
        }
        let settings = self.current_settings();
        if settings == self.saved_settings {
            return;
        }
        if let Some(path) = settings_path() {
            if let Some(dir) = path.parent() {
                let _ = std::fs::create_dir_all(dir);
            }
            let _ = std::fs::write(path, &settings);
        }
        self.saved_settings = settings;
    }

    fn side_panels(&mut self, ui: &mut Ui) {
        let tabs: Vec<TabInfo> = self
            .docs
            .iter()
            .filter(|doc| !doc.placeholder)
            .map(|doc| TabInfo { title: if doc.is_modified() { format!("{} *", doc.name()) } else { doc.name() }, path: doc.path.clone() })
            .collect();
        let active = self.active;
        let doc = &self.docs[active];
        let view = DocView { id: doc.id, text: &doc.text, hex: &doc.hex, format: &doc.text.format, path: doc.path.as_deref(), hex_mode: doc.hex_mode, name: doc.name(), language: doc.language_name(), no_file: doc.placeholder, syntax: doc.resolved_syntax.flatten(), image: doc.browser.as_ref().map(|b| b.view()) };
        let mut actions = Vec::new();
        let mut project_changed = false;
        self.panels.show(ui, &view, &tabs, active, &mut actions);
        let library_actions = self.panels.show_library(ui, &mut self.library);
        let history_rows = if self.panels.history && !doc.placeholder { doc.text.history_rows() } else { Vec::new() };
        let history_actions = self.panels.show_history(ui, &history_rows, doc.hex_mode);
        let project_actions = self.panels.show_project(ui, self.project.as_ref(), doc.path.as_deref());
        for action in project_actions {
            match action {
                project::PanelAction::Open(path) => actions.push(Action::OpenPath(path)),
                project::PanelAction::Remove(path) => {
                    if let Some(project) = &mut self.project {
                        project.remove(&path);
                    }
                    project_changed = true;
                }
            }
        }
        if project_changed {
            self.save_project();
        }
        for action in actions {
            self.run_panel_action(action, ui);
        }
        for action in library_actions {
            self.library_action(action, ui);
        }
        for action in history_actions {
            self.history_action(action, ui);
        }
        self.part_history_panel(ui);
    }

    /// Shows this file as it will look, beside the text, keeping up as it's edited. Returns true if it's to fill the
    /// text area instead. It only does that with something to show: otherwise the text stays, with the reason beside it.
    fn preview_panel(&mut self, ui: &mut Ui) -> bool {
        let doc = &self.docs[self.active];
        let view = doc.preview_view;
        let kind = Self::preview_kind(doc);
        let mut note = None;
        match kind {
            _ if doc.placeholder => note = Some("No file open".to_string()),
            None => note = Some("There's no preview for this kind of file yet. There are previews for Markdown, PC ANSI art, PETSCII, Commodore screens, teletext, ZX Spectrum text, tokenised BASIC, and pictures from the Amiga, Atari ST, Commodore 64, Mac, RISC OS, PC (PCX, GEM IMG), ZX Spectrum and Kodak Photo CD.".to_string()),
            Some(kind) if kind.reads_bytes() && !doc.text.is_disk_backed() => {
                // These are made from the exact bytes, so they work from hex mode as well as text.
                let version = if doc.hex_mode { doc.hex.version() | 1 << 63 } else { doc.text.version() };
                let key = (doc.id, version, kind);
                if self.preview_of != Some(key) {
                    // Only as much as could be shown is read, so a huge file isn't read in just to say it's too large.
                    let enough = crate::pictures::MOST_PICTURE_BYTES.max(preview::MOST_PREVIEW_BYTES) + 1;
                    let bytes = if doc.hex_mode { doc.hex.read(0, enough) } else { doc.text.encode(&doc.text.format, Lossy::Omit).unwrap_or_default() };
                    self.preview_screen = None;
                    self.preview_picture = None;
                    self.preview_cycling = None;
                    self.preview_blocks.clear();
                    self.preview_note = None;
                    // Pictures are decoded rather than laid out, so they can be larger: a Photo CD image is 4 to 6 MB.
                    let most = if kind == preview::Kind::Picture { crate::pictures::MOST_PICTURE_BYTES } else { preview::MOST_PREVIEW_BYTES };
                    if bytes.len() > most {
                        note = Some(format!("This file is too large to preview. It can be up to {} MB.", most >> 20));
                    } else {
                        match preview::render_bytes(kind, &bytes, &doc.text.format, Some(&doc.path_or_name())) {
                            preview::FromBytes::Screen(screen) => self.preview_screen = Some(screen),
                            preview::FromBytes::Blocks(blocks) => self.preview_blocks = blocks,
                            preview::FromBytes::Picture(picture) => {
                                let options = egui::TextureOptions { magnification: egui::TextureFilter::Nearest, minification: egui::TextureFilter::Linear, ..egui::TextureOptions::default() };
                                let texture = ui.ctx().load_texture("preview-picture", egui::ColorImage::from_rgba_unmultiplied([picture.width, picture.height], &picture.rgba), options);
                                self.preview_picture = Some((picture, texture));
                            }
                            preview::FromBytes::Nothing(why) => self.preview_note = Some(why),
                        }
                        self.preview_of = Some(key);
                    }
                }
                if note.is_none() {
                    note.clone_from(&self.preview_note);
                }
            }
            Some(_) if doc.hex_mode => note = Some("Switch to text mode to preview this file.".to_string()),
            Some(_) if doc.text.is_disk_backed() => note = Some("This file is too large to preview. It is being kept on disk.".to_string()),
            Some(kind) => {
                let key = (doc.id, doc.text.version(), kind);
                if self.preview_of != Some(key) {
                    let lines = doc.text.all_lines();
                    let size: usize = lines.iter().map(|line| line.len() + 1).sum();
                    if size > preview::MOST_PREVIEW_BYTES {
                        self.preview_blocks.clear();
                        note = Some("This file is too large to preview. It can be up to 4 MB.".to_string());
                    } else {
                        let readable = lines.iter().map(|line| doc.text.format.readable_text(line)).collect::<Vec<_>>().join("\n");
                        self.preview_blocks = preview::render(kind, &readable);
                        self.preview_of = Some(key);
                    }
                }
            }
        }
        if view == settings::PreviewView::Full && note.is_none() {
            return true;
        }
        let width = ui.available_width() * 0.4;
        // Dragging it can't squeeze the text out altogether.
        let most = (ui.available_width() - 200.0).max(160.0);
        let mut open = true;
        Panel::right("preview-panel").default_size(width).size_range(160.0..=most).resizable(true).frame(egui::Frame::new().fill(theme::field()).inner_margin(egui::Margin::symmetric(8, 4))).show(ui, |ui| {
            ui.horizontal(|ui| {
                let title = kind.map_or_else(|| "Preview".to_string(), |kind| format!("Preview: {}", kind.name()));
                ui.label(RichText::new(title).color(theme::text_dim()).size(12.5));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.add(Button::new(RichText::new("\u{D7}").color(theme::text_dim())).frame(false)).on_hover_text("Close").clicked() {
                        open = false;
                    }
                });
            });
            ui.separator();
            match &note {
                Some(text) => {
                    ui.label(RichText::new(text).color(theme::text_faint()));
                }
                None => self.draw_preview(ui, kind),
            }
        });
        if !open {
            self.doc().preview_view = settings::PreviewView::Text;
            self.pending_focus = true;
        }
        false
    }

    /// Draws whichever preview was made last: a retro screen or a laid-out document.
    fn draw_preview(&mut self, ui: &mut Ui, kind: Option<preview::Kind>) {
        if kind == Some(preview::Kind::Picture) {
            if let Some((picture, texture)) = &mut self.preview_picture
                && draw_picture(ui, picture, texture, &mut self.preview_cycling)
            {
                self.pending_command = Some(Command::SavePictureAsPng);
            }
            return;
        }
        // Amiga ANSI is drawn in the pens of a palette that can be changed, in a table below it.
        let doc = &self.docs[self.active];
        if kind == Some(preview::Kind::AnsiArt) && preview::is_amiga_console(&doc.text.format) {
            let mut changed = false;
            Panel::bottom("amiga-palette").frame(egui::Frame::new().inner_margin(egui::Margin::symmetric(0, 6))).show(ui, |ui| {
                changed = crate::amiga_palette_ui::draw(ui, &mut self.amiga_palette, &mut self.palette_editor);
            });
            if changed {
                crate::ansi_art::set_amiga_palette(self.amiga_palette);
                self.preview_of = None;
                self.save_settings_if_changed();
            }
        }
        match (&self.preview_screen, kind) {
            (Some(screen), Some(kind)) if kind.is_screen() => crate::screen::draw(ui, screen),
            _ => preview::draw(ui, &self.preview_blocks),
        }
    }

    /// The preview with the whole text area to itself, and the way back to the text.
    fn full_preview(&mut self, ui: &mut Ui) {
        self.doc().update_syntax();
        let mut chosen = None;
        egui::Frame::new().inner_margin(egui::Margin::symmetric(12, 4)).show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label(RichText::new("Preview, full screen").color(theme::text_dim()).size(12.5));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.button("Text only").on_hover_text("Back to the text, to edit it").clicked() {
                        chosen = Some(settings::PreviewView::Text);
                    }
                    if ui.button("Split screen").on_hover_text("The text and the preview side by side").clicked() {
                        chosen = Some(settings::PreviewView::Split);
                    }
                });
            });
            ui.separator();
            let kind = Self::preview_kind(&self.docs[self.active]);
            self.draw_preview(ui, kind);
        });
        if let Some(view) = chosen {
            self.doc().preview_view = view;
            self.pending_focus = true;
        }
    }

    /// A new choice in Settings shows at once on the Markdown files that are open, as well as later ones.
    fn apply_markdown_view(&mut self) {
        let view = self.prefs.markdown_view;
        for doc in &mut self.docs {
            if preview::kind_for(Some(&doc.path_or_name()), &doc.language_name()).is_some() {
                doc.preview_view = view;
            }
        }
    }

    fn run_panel_action(&mut self, action: Action, ui: &Ui) {
        self.pending_focus = true;
        if self.no_file() && !matches!(action, Action::OpenPath(_) | Action::OpenDialog | Action::CopyText(_)) {
            return;
        }
        match action {
            Action::OpenPath(path) => {
                self.open_path(&path);
            }
            Action::OpenDialog => self.open_dialog(),
            Action::SwitchTab(index) => {
                if index < self.docs.len() {
                    self.active = index;
                }
            }
            Action::CopyText(text) => ui.ctx().copy_text(text),
            Action::SelectBytes { start, end } => {
                let doc = self.doc();
                if doc.hex_mode {
                    // The end first, then back to the start, so the string's start is what comes into view.
                    doc.hex.move_caret(end, false);
                    doc.hex.move_caret(start, true);
                }
            }
            Action::ViewHex => self.set_view(Some(false)),
            Action::GoToLine(line) => {
                let doc = self.doc();
                if !doc.hex_mode {
                    doc.text.go_to(&(line + 1).to_string());
                }
            }
            Action::InsertEntry { .. } | Action::InsertCharacter(_) | Action::InsertBytes(_) | Action::AskInsertByte | Action::Paste(_) | Action::WrapTag(..) if self.docs[self.active].text.read_only => self.tell_read_only(),
            Action::InsertEntry { .. } | Action::InsertCharacter(_) | Action::InsertBytes(_) | Action::AskInsertByte | Action::Paste(_) | Action::WrapTag(..) if self.docs[self.active].is_busy() || self.pending.is_some() => {
                self.message("Please wait", "The file is still loading or saving. This will be available when it finishes.".into());
            }
            Action::InsertEntry { set, table, byte } => {
                let doc = self.doc();
                if doc.hex_mode {
                    doc.hex.insert_bytes(&[byte]);
                } else if doc.text.format.charset.id == set.id {
                    doc.text.insert_exact(&char_table::text_for_byte(set, byte));
                } else {
                    // Never mixed silently: the person says which they mean.
                    self.dialog = Dialog::InsertFrom { set, table, byte };
                }
            }
            Action::InsertCharacter(c) => {
                let doc = self.doc();
                if doc.hex_mode {
                    doc.hex.paste_text(&c.to_string());
                } else {
                    doc.text.type_text(&c.to_string());
                }
            }
            Action::WrapTag(before, after) => {
                let doc = self.doc();
                if !doc.hex_mode {
                    doc.text.wrap_selection(&before, &after);
                }
            }
            Action::InsertBytes(bytes) => self.insert_bytes(&bytes),
            Action::AskInsertByte => self.dialog = Dialog::InsertByte { value: String::new(), error: String::new() },
            Action::Paste(text) => self.paste_here(&text, ui),
        }
    }

    /// Exact bytes at the cursor, as one undoable action.
    fn insert_bytes(&mut self, bytes: &[u8]) {
        let doc = self.doc();
        if doc.hex_mode {
            doc.hex.insert_bytes(bytes);
        } else {
            let text = console::bytes_as_text(bytes, doc.text.format.charset);
            doc.text.insert_exact(&text);
        }
    }

    // ---------- Encodings ----------

    /// The start of the file, to preview what an encoding would make of it.
    fn sample_of(&self, index: usize) -> Vec<u8> {
        const SAMPLE: u64 = 4096;
        use std::io::Read;
        let doc = &self.docs[index];
        if doc.hex_mode && doc.hex.is_modified() {
            return doc.hex.read(0, SAMPLE as usize);
        }
        let mut sample = Vec::new();
        if let Some(path) = &doc.path
            && let Ok(file) = std::fs::File::open(path)
        {
            let _ = file.take(SAMPLE).read_to_end(&mut sample);
        }
        sample
    }

    fn begin_pick(&mut self, purpose: Purpose) {
        let index = self.active;
        let doc = &self.docs[index];
        if doc.is_busy() {
            self.message("Please wait", "The file is still loading or saving. This will be available when it finishes.".into());
            return;
        }
        let sample = if purpose == Purpose::Reopen { self.sample_of(index) } else { Vec::new() };
        self.dialog = Dialog::Picker(Picker::new(purpose, &self.docs[index].text.format, sample));
    }

    fn begin_presets(&mut self) {
        let selected = PRESETS.iter().position(|preset| Some(preset.id) == self.docs[self.active].text.format.preset).unwrap_or(0);
        self.dialog = Dialog::Presets(PresetPicker { selected, for_new_files: false });
    }

    /// Reads the file's bytes again in another way, after settling any unsaved changes. The file isn't changed.
    fn reopen(&mut self, index: usize, format: TextFormat) {
        if self.docs[index].is_modified() {
            let answer = MessageDialog::new()
                .set_title(Purpose::Reopen.title())
                .set_level(MessageLevel::Warning)
                .set_description(format!("{} has unsaved changes.\n\nSave them first? Choosing No discards them.", self.docs[index].name()))
                .set_buttons(MessageButtons::YesNoCancel)
                .show();
            match answer {
                MessageDialogResult::Yes => {
                    if !(self.save(index) && self.wait_for_save(index)) {
                        return;
                    }
                }
                MessageDialogResult::No => {}
                _ => return,
            }
        }
        let Some(path) = self.docs[index].path.clone() else {
            if self.docs[index].is_blank() {
                let eol = format.preset_eol;
                self.docs[index].text.format = format;
                charset_widths(&self.docs[index].text.format);
                self.docs[index].text.set_content(vec![String::new()]);
                self.docs[index].text.set_insert_eol(eol);
            } else {
                self.message(Purpose::Reopen.title(), "This document hasn't been saved, so there are no bytes to read again. Convert to encoding keeps the text and changes how it's stored.".into());
            }
            return;
        };
        let caret_line = self.docs[index].text.caret_line;
        let locked = self.docs[index].text.read_only;
        match Document::open(&path, Some(format)) {
            Ok(mut fresh) => {
                fresh.set_read_only(locked);
                if fresh.scanning.is_none() {
                    fresh.text.move_caret_to_char(caret_line, 0);
                }
                self.encoding_prefs.set(&path, &fresh.text.format);
                fresh.preview_view = self.docs[index].preview_view;
                self.docs[index] = fresh;
                self.pending_focus = true;
            }
            Err(problem) => warn(&format!("Couldn't open {}.\n\n{problem}", path.display())),
        }
    }

    /// Keeps the text and changes how it's stored, checking every character first.
    fn convert_to(&mut self, index: usize, target: TextFormat) {
        self.convert_with(index, target, Lossy::Refuse);
    }

    /// Converts, with what to do about characters the new encoding can't hold.
    fn convert_with(&mut self, index: usize, target: TextFormat, lossy: Lossy) {
        let doc = &mut self.docs[index];
        if doc.text.is_disk_backed() {
            // A file this large is read from disk a block at a time, which only works where a line ends in the
            // usual bytes: UTF-8 or a one-byte set, not UTF-16, UTF-32 or EBCDIC.
            if !readable_a_block_at_a_time(&target) {
                let name = target.charset.name.clone();
                return self.message(CONVERT_TITLE, format!("A file this large can only be converted in place to UTF-8 or a one-byte character set, since it's read from disk a part at a time.\n\nFile, Save a copy can write it in {name}: the copy is written straight to disk."));
            }
            // Too large to hold: every line is written out in the new encoding, and the document carries on from that.
            self.start_rewrite(index, CONVERT_TITLE, target, None, lossy, Box::new(|_, line| line.to_string()));
            return;
        }
        let lines = doc.text.all_lines();
        match codec::convert_lines(&lines, &doc.text.format, &target) {
            Ok(converted) => doc.text.apply_conversion(converted, target),
            Err(problems) => {
                let report = LossyReport { purpose: LossyPurpose::Convert, target_name: target.charset.name.clone(), problems };
                self.dialog = Dialog::Lossy(report, target);
            }
        }
    }

    /// Writes a copy in another encoding. The document keeps its own settings and stays unsaved or saved as it was.
    fn save_copy(&mut self, index: usize, target: TextFormat, lossy: Lossy) {
        if self.docs[index].text.is_disk_backed() && !self.docs[index].hex_mode {
            if self.big_job.is_some() {
                self.message("Save a copy", "Please wait for the file being worked through to finish.".into());
                return;
            }
            let doc = &self.docs[index];
            if let Some(path) = rfd::FileDialog::new().set_title("Save a copy").set_file_name(doc.name()).save_file() {
                self.big_job = Some(crate::big_jobs::save_converted(doc.text.store().clone(), doc.text.format.clone(), target, lossy, path, doc.id));
            }
            return;
        }
        if self.docs[index].hex.is_on_disk() {
            self.message("Save a copy", "This file is too large to copy into another encoding, because that would have to read it all into memory. Save As can copy it in the encoding it has.".into());
            return;
        }
        let doc = &self.docs[index];
        let bytes = if doc.hex_mode {
            // Hex mode holds the bytes; they're read as this document's encoding to carry the text over.
            let mut from = doc.text.format.clone();
            let decoded = codec::decode(&doc.hex.bytes(), &mut from);
            let eol_of = |i: usize| decoded.exceptions.get(&i).copied().unwrap_or(decoded.base);
            codec::export(&decoded.lines, &eol_of, &from, &target, lossy)
        } else {
            let lines = doc.text.all_lines();
            let text = &doc.text;
            codec::export(&lines, &|i| text.terminator(i), &text.format, &target, lossy)
        };
        let bytes = match bytes {
            Ok(bytes) => bytes,
            Err(problems) => {
                let report = LossyReport { purpose: LossyPurpose::Export, target_name: target.charset.name.clone(), problems };
                self.dialog = Dialog::Lossy(report, target);
                return;
            }
        };
        let name = doc.name();
        if let Some(path) = rfd::FileDialog::new().set_title("Save a copy").set_file_name(name).save_file()
            && let Err(problem) = codec::write_file(&path, &bytes)
        {
            warn(&format!("Couldn't save {}.\n\n{problem}", path.display()));
        }
    }

    fn run_menu_action(&mut self, action: MenuAction) {
        let index = self.active;
        if self.docs[index].narrow.is_some() && !matches!(action, MenuAction::Forget | MenuAction::ToggleSourceView | MenuAction::AskInsertByte | MenuAction::ChoosePreset) {
            self.message("Narrowed", "This changes how the whole file is read or saved. Press Show the whole file first.".into());
            return;
        }
        match action {
            MenuAction::Pick(Purpose::Convert) | MenuAction::SetBom(_) | MenuAction::ConvertEols(_) | MenuAction::AskInsertByte if self.docs[index].text.read_only => self.tell_read_only(),
            MenuAction::Pick(purpose) => self.begin_pick(purpose),
            MenuAction::ChoosePreset => self.begin_presets(),
            MenuAction::SetBom(wanted) => {
                let text = &mut self.docs[index].text;
                let format = TextFormat { has_bom: wanted, ..text.format.clone() };
                text.change_format(format);
            }
            MenuAction::Forget => {
                if let Some(path) = self.docs[index].path.clone() {
                    self.encoding_prefs.clear(&path);
                }
            }
            MenuAction::SetInsertEol(eol) => self.docs[index].text.set_insert_eol(eol),
            MenuAction::ConvertEols(eol) => {
                if self.docs[index].text.is_disk_backed() {
                    let format = self.docs[index].text.format.clone();
                    self.start_rewrite(index, "Line endings", format, Some(eol), Lossy::Refuse, Box::new(|_, line| line.to_string()));
                } else {
                    self.docs[index].text.convert_line_endings(eol);
                }
            }
            MenuAction::SetPolicy(policy) => {
                let format = TextFormat { newline: policy, certainty: Certainty::Chosen, ..self.docs[index].text.format.clone() };
                self.reopen(index, format);
            }
            MenuAction::ToggleFinalNewline => {
                let text = &mut self.docs[index].text;
                text.format.ensure_final_newline = !text.format.ensure_final_newline;
            }
            MenuAction::SetDisplay(display) => {
                self.docs[index].text.format.display = display;
                if let Some(path) = self.docs[index].path.clone() {
                    let format = self.docs[index].text.format.clone();
                    self.encoding_prefs.set(&path, &format);
                }
            }
            MenuAction::SetStartState(state) => {
                self.docs[index].text.format.state = state;
                if let Some(path) = self.docs[index].path.clone() {
                    let format = self.docs[index].text.format.clone();
                    self.encoding_prefs.set(&path, &format);
                }
            }
            MenuAction::ToggleSourceView => self.view.source_view = !self.view.source_view,
            MenuAction::AskInsertByte => self.dialog = Dialog::InsertByte { value: String::new(), error: String::new() },
        }
    }

    /// The encoding windows. Returns whether one of them is what's open.
    fn encoding_dialog(&mut self, ctx: &Context) -> bool {
        let index = self.active;
        let current = self.docs[index].text.format.clone();
        match &mut self.dialog {
            Dialog::Picker(picker) => match encoding_ui::picker_window(ctx, picker, &current) {
                Outcome::Open => {}
                Outcome::Cancel => self.close_dialog(),
                Outcome::Accept => {
                    let purpose = picker.purpose;
                    let target = picker.format(&current);
                    self.close_dialog();
                    match purpose {
                        Purpose::Reopen => self.reopen(index, target),
                        Purpose::Convert => self.convert_to(index, target),
                        Purpose::SaveCopy => self.save_copy(index, target, Lossy::Refuse),
                    }
                }
            },
            Dialog::Presets(picker) => match encoding_ui::preset_window(ctx, picker) {
                Outcome::Open => {}
                Outcome::Cancel => self.close_dialog(),
                Outcome::Accept => {
                    let preset = &PRESETS[picker.selected];
                    let for_new_files = picker.for_new_files;
                    self.close_dialog();
                    if for_new_files {
                        self.new_file_preset = preset;
                    } else {
                        self.reopen(index, preset.format());
                    }
                }
            },
            Dialog::Lossy(report, target) => match encoding_ui::lossy_window(ctx, report) {
                LossyChoice::Waiting => {}
                LossyChoice::Cancel => self.close_dialog(),
                LossyChoice::GoTo(line, column) => {
                    self.close_dialog();
                    self.docs[index].text.move_caret_to_char(line, column);
                }
                LossyChoice::Export(lossy) => {
                    let target = target.clone();
                    self.close_dialog();
                    self.save_copy(index, target, lossy);
                }
                LossyChoice::SaveCopyInstead => {
                    self.close_dialog();
                    self.begin_pick(Purpose::SaveCopy);
                }
            },
            Dialog::InsertByte { value, error } => {
                let mut chosen = None;
                let mut cancelled = ctx.input(|i| i.key_pressed(Key::Escape));
                egui::Window::new("Insert byte").collapsible(false).resizable(false).anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0]).show(ctx, |ui| {
                    ui.label("Byte value in hexadecimal, such as 9B or 1B 5B:");
                    ui.text_edit_singleline(value).request_focus();
                    if !error.is_empty() {
                        ui.label(error.as_str());
                    }
                    let (ok, cancel) = ok_cancel(ui);
                    cancelled |= cancel;
                    if ok || ui.input(|i| i.key_pressed(Key::Enter)) {
                        chosen = Some(value.clone());
                    }
                });
                if cancelled {
                    self.close_dialog();
                } else if let Some(text) = chosen {
                    match encoding_ui::parse_bytes(&text) {
                        Some(bytes) => {
                            self.close_dialog();
                            self.insert_bytes(&bytes);
                        }
                        None => {
                            if let Dialog::InsertByte { error, .. } = &mut self.dialog {
                                *error = "That isn't a whole number of bytes in hexadecimal.".to_string();
                            }
                        }
                    }
                }
            }
            Dialog::WrapColumn { value, error, focus } => {
                let mut go = false;
                let mut cancelled = ctx.input(|i| i.key_pressed(Key::Escape));
                egui::Window::new("Wrap to line ends").collapsible(false).resizable(false).anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0]).show(ctx, |ui| {
                    ui.label("Break long lines at a space, so they end by this column:");
                    let field = ui.text_edit_singleline(value);
                    if *focus {
                        field.request_focus();
                        *focus = false;
                    }
                    if !error.is_empty() {
                        ui.label(error.as_str());
                    }
                    let (ok, cancel) = ok_cancel(ui);
                    cancelled |= cancel;
                    go = ok || ui.input(|i| i.key_pressed(Key::Enter));
                });
                if cancelled {
                    self.close_dialog();
                } else if go {
                    match value.trim().parse::<usize>().ok().filter(|column| (reformat::LEAST_WIDTH..=reformat::MOST_WIDTH).contains(column)) {
                        Some(column) => {
                            self.close_dialog();
                            self.change_lines(|rows, _| reformat::wrap_to_line_ends(rows, column));
                        }
                        None => *error = format!("That has to be a whole number from {} to {}.", reformat::LEAST_WIDTH, reformat::MOST_WIDTH),
                    }
                }
            }
            Dialog::ParagraphSetup { width, left, first, align, error } => {
                let mut go = false;
                let mut cancelled = ctx.input(|i| i.key_pressed(Key::Escape));
                egui::Window::new("Paragraph setup").collapsible(false).resizable(false).anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0]).show(ctx, |ui| {
                    egui::Grid::new("paragraph-grid").num_columns(2).show(ui, |ui| {
                        for (label, field) in [("Lines end by column:", &mut *width), ("Left margin:", &mut *left), ("First line indent:", &mut *first)] {
                            ui.label(label);
                            ui.text_edit_singleline(field);
                            ui.end_row();
                        }
                    });
                    ui.horizontal(|ui| {
                        ui.label("Alignment:");
                        for (name, value) in [("Left", Align::Left), ("Center", Align::Center), ("Right", Align::Right), ("Justify", Align::Justify)] {
                            ui.radio_value(align, value, name);
                        }
                    });
                    if !error.is_empty() {
                        ui.label(error.as_str());
                    }
                    let (ok, cancel) = ok_cancel(ui);
                    cancelled |= cancel;
                    go = ok || ui.input(|i| i.key_pressed(Key::Enter));
                });
                if cancelled {
                    self.close_dialog();
                } else if go {
                    let read = |text: &str| text.trim().parse::<usize>().ok().filter(|number| *number <= reformat::MOST_WIDTH);
                    match (read(width).filter(|number| *number >= reformat::LEAST_WIDTH), read(left), read(first)) {
                        (Some(width), Some(left), Some(first)) => {
                            self.prefs.paragraph = reformat::ParagraphSetup { width, left, first, align: *align };
                            self.close_dialog();
                        }
                        _ => *error = format!("Those have to be whole numbers, and lines have to end by column {} or more.", reformat::LEAST_WIDTH),
                    }
                }
            }
            Dialog::ProjectSettings { name, notes } => {
                let mut cancelled = ctx.input(|i| i.key_pressed(Key::Escape));
                let mut ok = false;
                egui::Window::new("Project settings").collapsible(false).resizable(false).anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0]).show(ctx, |ui| {
                    ui.label("Name:");
                    ui.text_edit_singleline(name);
                    ui.add_space(4.0);
                    ui.label("Notes:");
                    ui.add(egui::TextEdit::multiline(notes).desired_rows(5));
                    let (okay, cancel) = ok_cancel(ui);
                    ok = okay;
                    cancelled |= cancel;
                });
                if cancelled {
                    self.close_dialog();
                } else if ok {
                    if let Some(project) = &mut self.project {
                        if !name.trim().is_empty() {
                            project.name = name.trim().to_string();
                        }
                        project.notes.clone_from(notes);
                    }
                    self.save_project();
                    if matches!(self.dialog, Dialog::ProjectSettings { .. }) {
                        self.close_dialog();
                    }
                }
            }
            Dialog::RestorePick { backups, chosen } => {
                let mut cancelled = ctx.input(|i| i.key_pressed(Key::Escape));
                let mut go = false;
                egui::Window::new("Restore project").collapsible(false).resizable(false).anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0]).show(ctx, |ui| {
                    ui.label("Put the files back from this backup:");
                    egui::ScrollArea::vertical().max_height(220.0).show(ui, |ui| {
                        for (place, backup) in backups.iter().enumerate() {
                            ui.radio_value(chosen, place, format!("{} ({} files)", backup.name, backup.files.len()));
                        }
                    });
                    ui.add_space(4.0);
                    ui.label("This replaces the files on disk. A backup of how they are now is taken first.");
                    ui.label("Files open here with unsaved changes are left alone.");
                    ui.add_space(4.0);
                    ui.horizontal(|ui| {
                        go = ui.button("Restore").clicked();
                        cancelled |= ui.button("Cancel").clicked();
                    });
                });
                if cancelled {
                    self.close_dialog();
                } else if go {
                    let backup = backups[(*chosen).min(backups.len() - 1)].clone();
                    self.close_dialog();
                    self.restore_project(backup);
                }
            }
            Dialog::Compare(state) => {
                if compare::window(ctx, state) {
                    self.close_dialog();
                }
            }
            Dialog::ComparePick(others) => {
                let mut cancelled = ctx.input(|i| i.key_pressed(Key::Escape));
                let (mut open_file, mut from_disk) = (None, false);
                egui::Window::new("Compare").collapsible(false).resizable(false).anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0]).show(ctx, |ui| {
                    ui.label(format!("Compare {} with:", self.docs[index].name()));
                    ui.add_space(4.0);
                    for (other, name) in others.iter() {
                        if ui.button(name.as_str()).clicked() {
                            open_file = Some(*other);
                        }
                    }
                    from_disk = ui.button("A file on disk...").clicked();
                    ui.add_space(4.0);
                    cancelled |= ui.button("Cancel").clicked();
                });
                if let Some(other) = open_file {
                    let lines = self.lines_of_open_file(other);
                    self.close_dialog();
                    self.show_comparison(lines);
                } else if from_disk {
                    self.close_dialog();
                    if let Some(path) = rfd::FileDialog::new().set_title("Compare with").pick_file() {
                        let lines = Self::lines_of_file_on_disk(&path);
                        self.show_comparison(lines);
                    }
                } else if cancelled {
                    self.close_dialog();
                }
            }
            Dialog::ConfirmReplaceInFiles { files, matches } => {
                let mut go = false;
                let mut cancelled = ctx.input(|i| i.key_pressed(Key::Escape));
                if let Some(plan) = &self.replace_plan {
                    egui::Window::new("Replace in Files").collapsible(false).resizable(false).anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0]).show(ctx, |ui| {
                        ui.set_max_width(460.0);
                        ui.label(format!("Replace \"{}\" with \"{}\"?", plan.options.pattern, plan.options.replacement));
                        ui.label(format!("{matches} match{} in {} file{}.", if *matches == 1 { "" } else { "es" }, files.len(), if files.len() == 1 { "" } else { "s" }));
                        ui.add_space(6.0);
                        ui.label("Files open here change in their windows, unsaved, and can be undone.");
                        ui.label(if plan.backup { "The others are rewritten on disk now, each with a .bak copy of how it was." } else { "The others are rewritten on disk now, and that can't be undone." });
                        ui.add_space(6.0);
                        ui.horizontal(|ui| {
                            go = ui.button("Replace").clicked();
                            cancelled |= ui.button("Cancel").clicked();
                        });
                    });
                } else {
                    cancelled = true;
                }
                if cancelled {
                    self.close_dialog();
                } else if go {
                    let files = std::mem::take(files);
                    self.close_dialog();
                    self.start_replace_in_files(files);
                }
            }
            Dialog::Settings(state) => {
                let wrap_before = self.view.word_wrap;
                let markdown_before = self.prefs.markdown_view;
                let mut target = settings::Target {
                    view: &mut self.view,
                    prefs: &mut self.prefs,
                    file_tabs: &mut self.show_file_tabs,
                    status_bar: &mut self.show_status,
                    font: self.font_file.as_deref(),
                    new_file_preset: self.new_file_preset.name,
                    recent_files: self.panels.recent.len(),
                    language_servers: &mut self.lsp.on,
                };
                let outcome = settings::window(ctx, state, &mut target);
                if let Some(outcome) = outcome {
                    state.file_types.carry_out(outcome, associations::register, associations::remove, associations::open_windows_page);
                }
                if self.prefs.markdown_view != markdown_before {
                    self.apply_markdown_view();
                }
                if self.view.word_wrap != wrap_before {
                    // Word wrap has more to do than change a value: it also ends column mode.
                    self.view.word_wrap = wrap_before;
                    self.toggle_word_wrap();
                }
                match outcome {
                    Some(settings::Outcome::Close) => self.close_dialog(),
                    Some(settings::Outcome::ChooseFont) => self.dialog = Dialog::Fonts { error: String::new() },
                    Some(settings::Outcome::ChoosePreset) => self.begin_presets(),
                    Some(settings::Outcome::ClearRecent) => self.panels.recent.clear(),
                    Some(settings::Outcome::Themes) => self.open_theme_box(theme_host::ThemePage::Window),
                    Some(settings::Outcome::Assemblers) => self.open_assembler_box(),
                    Some(settings::Outcome::NewKinds) => self.open_new_kinds_box(),
                    Some(settings::Outcome::LanguageServers) => self.dialog = Dialog::LanguageServers(self.lsp.settings.clone()),
                    Some(settings::Outcome::SetFileTypes | settings::Outcome::RemoveFileTypes | settings::Outcome::OpenWindowsPage) | None => {}
                }
            }
            Dialog::ColumnRemove { cut, count, error, focus } => {
                let cut = *cut;
                let mut go = false;
                let mut cancelled = ctx.input(|i| i.key_pressed(Key::Escape));
                egui::Window::new(if cut { "Cut columns" } else { "Delete columns" }).collapsible(false).resizable(false).anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0]).show(ctx, |ui| {
                    ui.label(format!("Number of columns to {} on every line, starting at this line and column.", if cut { "cut" } else { "delete" }));
                    ui.label("Leave it at 0 to use the highlighted columns instead.");
                    ui.add_space(4.0);
                    let field = ui.text_edit_singleline(count);
                    if *focus {
                        field.request_focus();
                        *focus = false;
                    }
                    if !error.is_empty() {
                        ui.label(error.as_str());
                    }
                    let (ok, cancel) = ok_cancel(ui);
                    cancelled |= cancel;
                    go = ok || ui.input(|i| i.key_pressed(Key::Enter));
                });
                if cancelled {
                    self.close_dialog();
                } else if go {
                    let number = if count.trim().is_empty() { Some(0) } else { hex_buffer::parse_number(count) };
                    let doc = &mut self.docs[index];
                    let problem = match number {
                        None => Some("That isn't a whole number of columns.".to_string()),
                        Some(number) => match doc.text.select_columns(number) {
                            Err(problem) => Some(problem),
                            Ok(()) => {
                                if !cut || views::copy_text_selection(ctx, &mut doc.text, &mut self.clip) {
                                    doc.text.remove_selected_columns();
                                }
                                None
                            }
                        },
                    };
                    match problem {
                        Some(problem) => *error = problem,
                        None => self.close_dialog(),
                    }
                }
            }
            Dialog::CsvToFixed { rows, delimiter, error } => {
                let mut go = false;
                let mut cancelled = ctx.input(|i| i.key_pressed(Key::Escape));
                egui::Window::new("Convert to fixed width").collapsible(false).resizable(false).anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0]).show(ctx, |ui| {
                    ui.label(format!("Lines {} to {}: each value is padded to its column's widest.", rows.0 + 1, rows.1 + 1));
                    ui.add_space(4.0);
                    delimiter_choice(ui, delimiter);
                    if !error.is_empty() {
                        ui.label(error.as_str());
                    }
                    let (ok, cancel) = ok_cancel(ui);
                    cancelled |= cancel;
                    go = ok || ui.input(|i| i.key_pressed(Key::Enter));
                });
                if cancelled {
                    self.close_dialog();
                } else if go {
                    let (top, bottom) = *rows;
                    let doc = &mut self.docs[index];
                    match columns::to_fixed(&doc.text.rows_text(top, bottom), *delimiter) {
                        Ok(converted) => {
                            doc.text.replace_rows(top, converted);
                            self.close_dialog();
                        }
                        Err(place) => *error = format!("Line {} has a quote that's never closed, so nothing was changed.", top + place + 1),
                    }
                }
            }
            Dialog::FixedToCsv { rows, delimiter, starts, error, focus } => {
                let mut go = false;
                let mut cancelled = ctx.input(|i| i.key_pressed(Key::Escape));
                egui::Window::new("Convert fixed width to CSV").collapsible(false).resizable(false).anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0]).show(ctx, |ui| {
                    ui.label(format!("Lines {} to {}: the padding is removed from each value.", rows.0 + 1, rows.1 + 1));
                    ui.add_space(4.0);
                    ui.label("Columns start at (change these if a value got split):");
                    let field = ui.text_edit_singleline(starts);
                    if *focus {
                        field.request_focus();
                        *focus = false;
                    }
                    ui.add_space(4.0);
                    delimiter_choice(ui, delimiter);
                    if !error.is_empty() {
                        ui.label(error.as_str());
                    }
                    let (ok, cancel) = ok_cancel(ui);
                    cancelled |= cancel;
                    go = ok || ui.input(|i| i.key_pressed(Key::Enter));
                });
                if cancelled {
                    self.close_dialog();
                } else if go {
                    let (top, bottom) = *rows;
                    match columns::parse_starts(starts) {
                        Ok(places) => {
                            let doc = &mut self.docs[index];
                            let converted = columns::to_csv(&doc.text.rows_text(top, bottom), &places, *delimiter);
                            doc.text.replace_rows(top, converted);
                            self.close_dialog();
                        }
                        Err(problem) => *error = problem,
                    }
                }
            }
            Dialog::HexInsertDelete { count, delete, fill, error, focus } => {
                let mut go = false;
                let mut cancelled = ctx.input(|i| i.key_pressed(Key::Escape));
                egui::Window::new("Hex insert/delete").collapsible(false).resizable(false).anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0]).show(ctx, |ui| {
                    ui.horizontal(|ui| {
                        ui.radio_value(delete, false, "Insert");
                        ui.radio_value(delete, true, "Delete");
                    });
                    ui.add_space(4.0);
                    ui.label("Number of bytes (use 0x for hex):");
                    let field = ui.text_edit_singleline(count);
                    if *focus {
                        field.request_focus();
                        *focus = false;
                    }
                    ui.add_enabled_ui(!*delete, |ui| {
                        ui.label("Fill new bytes with this value, in hex:");
                        ui.text_edit_singleline(fill);
                    });
                    if !error.is_empty() {
                        ui.label(error.as_str());
                    }
                    let (ok, cancel) = ok_cancel(ui);
                    cancelled |= cancel;
                    go = ok || ui.input(|i| i.key_pressed(Key::Enter));
                });
                if cancelled {
                    self.close_dialog();
                } else if go {
                    let delete = *delete;
                    let outcome = hex_buffer::parse_run(count, delete, fill);
                    let problem = match outcome {
                        Ok((count, value)) => {
                            let bytes = &mut self.docs[index].hex;
                            if !delete {
                                bytes.insert_run(count, value);
                                None
                            } else if bytes.delete_run(count) == 0 {
                                Some("There are no bytes after the cursor to delete.".to_string())
                            } else {
                                None
                            }
                        }
                        Err(problem) => Some(problem),
                    };
                    match problem {
                        Some(problem) => {
                            if let Dialog::HexInsertDelete { error, .. } = &mut self.dialog {
                                *error = problem;
                            }
                        }
                        None => self.close_dialog(),
                    }
                }
            }
            Dialog::InsertFrom { set, table, byte } => {
                let (set, table, byte) = (*set, *table, *byte);
                let entry = set.entry(table, byte);
                let meant = entry.and_then(|entry| entry.unicode).filter(|_| entry.is_some_and(|entry| entry.kind == charset::Kind::Printable));
                let placed = meant.map(|c| current.charset.for_text(c, current.state));
                let fits = placed.filter(|c| if current.charset.holds_characters() { codec::fits(*c, current.charset) } else { charset::raw_byte(*c).is_some() || current.charset.table(current.state).is_some_and(|t| t.byte_of(*c).is_some()) });
                let mut text = None;
                let mut cancelled = ctx.input(|i| i.key_pressed(Key::Escape));
                egui::Window::new("Insert from another encoding").collapsible(false).resizable(false).anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0]).show(ctx, |ui| {
                    ui.label(format!("Byte {byte:02X} in {} goes into a document in {}.", set.name, current.charset.name));
                    ui.add_space(4.0);
                    let label = meant.map_or_else(|| "It has no character to convert".to_string(), |c| format!("Insert the character \"{c}\" ({})", charset::char_name(c)));
                    if ui.add_enabled(fits.is_some(), Button::new(label)).clicked() {
                        text = fits.map(String::from);
                    }
                    if ui.button(format!("Insert the raw byte {byte:02X}")).on_hover_text("Exactly this byte. It may mean something else in this document's encoding").clicked() {
                        text = Some(char_table::text_for_byte(current.charset, byte));
                    }
                    if ui.button("Cancel").clicked() {
                        cancelled = true;
                    }
                });
                if let Some(text) = text {
                    self.close_dialog();
                    self.docs[index].text.insert_exact(&text);
                } else if cancelled {
                    self.close_dialog();
                }
            }
            Dialog::Fonts { error } => {
                let mut chosen = None;
                let mut browse = false;
                let mut close = ctx.input(|i| i.key_pressed(Key::Escape));
                let current = self.font_file.clone();
                egui::Window::new("Editor font").collapsible(false).resizable(false).anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0]).show(ctx, |ui| {
                    ui.set_width(320.0);
                    ui.label(RichText::new("Columns depend on every character being the same width, so only fonts like that can be used.").size(12.5).color(theme::text_dim()));
                    ui.add_space(4.0);
                    for (name, path) in installed_fonts() {
                        let on = current.as_deref().map_or(name == "Consolas", |file| same_path(file, &path));
                        if ui.add(Button::selectable(on, name)).clicked() {
                            chosen = Some(path);
                        }
                    }
                    ui.add_space(4.0);
                    browse = ui.button("Another font file...").clicked();
                    if !error.is_empty() {
                        ui.label(RichText::new(error.as_str()).color(theme::rainbow_mistake()));
                    }
                    ui.add_space(4.0);
                    close |= ui.button("Close").clicked();
                });
                if browse {
                    chosen = rfd::FileDialog::new().set_title("Choose a font").add_filter("Fonts", &["ttf", "otf", "ttc"]).pick_file();
                }
                if let Some(path) = chosen {
                    match std::fs::read(&path) {
                        Ok(bytes) if is_fixed_pitch(bytes.clone()) => {
                            install_fonts(ctx, Some(&path));
                            self.font_file = Some(path);
                        }
                        Ok(_) => {
                            if let Dialog::Fonts { error } = &mut self.dialog {
                                *error = "That font can't be used: its characters aren't all the same width, or it can't be read.".to_string();
                            }
                        }
                        Err(problem) => {
                            if let Dialog::Fonts { error } = &mut self.dialog {
                                *error = problem.to_string();
                            }
                        }
                    }
                }
                if close {
                    self.close_dialog();
                }
            }
            Dialog::Color { rgb, form } => {
                let mut insert = None;
                let mut close = ctx.input(|i| i.key_pressed(Key::Escape));
                egui::Window::new("Insert color").collapsible(false).resizable(false).anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0]).show(ctx, |ui| {
                    ui.horizontal(|ui| {
                        egui::color_picker::color_edit_button_srgb(ui, rgb);
                        ui.label(RichText::new(tools::color_text(*rgb, *form)).monospace());
                    });
                    ui.add_space(4.0);
                    for (index, name) in tools::COLOR_FORMS.iter().enumerate() {
                        ui.radio_value(form, index, *name);
                    }
                    ui.add_space(6.0);
                    ui.horizontal(|ui| {
                        if ui.button("Insert").clicked() {
                            insert = Some(tools::color_text(*rgb, *form));
                        }
                        close |= ui.button("Cancel").clicked();
                    });
                });
                if let Some(text) = insert {
                    self.close_dialog();
                    self.type_at_cursor(&text);
                } else if close {
                    self.close_dialog();
                }
            }
            _ => return false,
        }
        true
    }

    fn close_dialog(&mut self) {
        self.dialog = Dialog::None;
        self.pending_focus = true;
    }

    fn tab_strip(&mut self, ui: &mut Ui) {
        let mut close = None;
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing = egui::vec2(4.0, 2.0);
            for i in 0..if self.no_file() { 0 } else { self.docs.len() } {
                let numbers = self.docs[i].window_numbers();
                let several = numbers.len() > 1;
                for number in numbers {
                    let doc = &self.docs[i];
                    let name = if several { format!("{}:{number}", doc.name()) } else { doc.name() };
                    let (chosen, closing) = tabs::file_tab(ui, &name, doc.is_modified(), i == self.active && number == doc.window_number);
                    if chosen {
                        self.active = i;
                        self.docs[i].switch_window(number);
                        self.pending_focus = true;
                    }
                    if closing {
                        close = Some((i, number));
                    }
                }
            }
        });
        if let Some((i, number)) = close {
            if self.docs[i].windows.is_empty() {
                self.close_tab(i);
            } else {
                self.docs[i].close_window(number);
                self.pending_focus = true;
            }
        }
    }

    fn status_bar(&self, ui: &mut Ui) -> (Option<MenuAction>, bool) {
        if self.no_file() {
            ui.label(RichText::new("No file open. Open one from the File menu, or drop one here.").color(theme::text_faint()));
            return (None, false);
        }
        let doc = &self.docs[self.active];
        let mut action = None;
        let mut toggle_lock = false;
        let faint = |words: String| RichText::new(words).color(theme::text_faint());
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 18.0;
            if scripting::recording() {
                ui.colored_label(theme::rainbow_mistake(), "\u{25CF} Recording a macro").on_hover_text("Choose Tools, Macros, Record or stop, or press Ctrl+Shift+R, when you're done");
            }
            if let Some(source) = &doc.scanning {
                ui.colored_label(theme::accent(), format!("Finding lines: {:.0}%", source.progress() * 100.0)).on_hover_text(format!("{} lines so far. You can read and move around now, and editing starts when this finishes", doc.text.line_count()));
            }
            if let Some(job) = &doc.saving {
                let share = job.written.load(Ordering::Relaxed) as f32 / job.total.max(1) as f32;
                ui.colored_label(theme::accent(), format!("Saving: {:.0}%", (share * 100.0).min(100.0)));
            }
            if let Some(status) = self.remote_status() {
                ui.colored_label(theme::accent(), status);
            }
            if let Some(blame) = &self.git.blame_line {
                ui.label(faint(blame.clone()));
            }
            if let Some(view) = &doc.live {
                let words = match &view.problem {
                    Some(problem) => problem.clone(),
                    None => format!("Following the {} as it runs", view.machine.name()),
                };
                ui.colored_label(theme::accent(), words);
            }
            if let Some(status) = self.run_status() {
                ui.colored_label(theme::accent(), status);
                if !self.running.sessions.is_empty() && ui.small_button("Stop").on_hover_text("Stops the program running, closing the emulator").clicked() {
                    ui.ctx().data_mut(|d| d.insert_temp(egui::Id::unique(run_host::STOP_ASKED), true));
                }
            }
            if let Some((status, more)) = self.send_status() {
                let label = ui.colored_label(theme::accent(), status);
                if !more.is_empty() {
                    label.on_hover_text(more);
                }
                if !self.sends.jobs.is_empty() && ui.small_button("Stop").on_hover_text("Stop sending. Nothing half-sent is left on a card or folder").clicked() {
                    for job in &self.sends.jobs {
                        job.stop();
                    }
                }
            }
            if let Some(job) = &self.big_job {
                ui.colored_label(theme::accent(), format!("{}: {:.0}%", job.title, (job.share() * 100.0).min(100.0)));
            }
            // Only shown when something's wrong: if edits aren't being kept, the person needs to know.
            let kept = self.recovery_failed.clone().or_else(|| self.recovery.as_ref().and_then(recovery::Session::problem)).filter(|_| self.screenshot.is_none());
            if let Some(problem) = kept {
                ui.colored_label(theme::rainbow_mistake(), "Edits aren't being kept for recovery. Save often.").on_hover_text(problem);
            }
            ui.label(if doc.hex_mode { doc.hex.position_text() } else { doc.text.position_text() });
            // A language server's problem with the line.
            if doc.text.code.is_none()
                && let Some((_, problem)) = doc.text.intel.as_ref().and_then(|i| i.diagnostics.iter().chain(&i.build_problems).find(|(l, _)| *l == doc.text.caret_line))
            {
                ui.colored_label(theme::problem_mark(), problem.clone());
            }
            // In assembler source: the line's problem, or its address, bytes and timing once it's been assembled.
            if let Some(code) = &doc.text.code {
                match code.problem_at(doc.text.caret_line) {
                    Some(problem) => {
                        ui.colored_label(theme::problem_mark(), problem.to_string());
                    }
                    None => {
                        if let Some(words) = self.line_assembled(doc.id, doc.text.caret_line) {
                            ui.label(faint(words));
                        }
                    }
                }
            }
            if !self.print_status.is_empty() {
                ui.label(faint(self.print_status.clone()));
            }
            if let Some(why) = doc.autosave_skipped.filter(|_| self.prefs.autosave) {
                ui.label(faint("Not saved by itself".to_string())).on_hover_text(format!("Autosave is leaving this file for you to save: {why}."));
            }
            if let Some(filter) = &doc.filter {
                ui.colored_label(theme::accent(), format!("Showing lines with \"{}\": {} of {}", filter.needle, filter.lines.len(), doc.text.line_count()));
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.spacing_mut().item_spacing.x = 18.0;
                let (modified, size, read_only) = doc.status_file_parts();
                if !doc.hex_mode && doc.text.column_mode() {
                    ui.colored_label(theme::accent(), "Column mode");
                }
                let overwriting = if doc.hex_mode { !doc.hex.insert_mode } else { doc.text.overwrite };
                if overwriting {
                    ui.colored_label(theme::accent(), "Overwriting");
                }
                if let Some(zx) = doc.text.zx.as_ref().filter(|_| !doc.hex_mode) {
                    if zx.graph {
                        ui.label(RichText::new("GRAPH").strong().color(theme::accent())).on_hover_text("GRAPH mode, as on the Spectrum: 1 to 8 type block graphics, with Shift the inverted ones, and A to U type UDGs. 9, Escape or Ctrl+Shift+9 leaves it");
                    }
                    if let Some(from) = zx.udgs_from.as_ref().filter(|_| zx.has_pictures()) {
                        ui.label(faint("UDG pictures".into())).on_hover_text(format!("UDGs are drawn as they look, with the pictures from {from}. Put the cursor in one to edit it as text"));
                    }
                }
                let locked = read_only || doc.text.read_only;
                let lock = RichText::new(if locked { "Read-only" } else { "Editable" }).color(if locked { theme::change_unsaved() } else { theme::text_faint() });
                if ui.add(egui::Label::new(lock).sense(egui::Sense::click())).on_hover_text("Click to lock this file against changes, or unlock it").clicked() {
                    toggle_lock = true;
                }
                let size_label = ui.label(faint(size));
                if let Some(modified) = modified {
                    size_label.on_hover_text(modified);
                }
                let can_edit = !doc.is_busy() && !doc.hex_mode && self.pending.is_none();
                let format = &doc.text.format;
                let remembered = doc.path.as_deref().is_some_and(|path| self.encoding_prefs.has(path));
                if !doc.hex_mode {
                    if format.display != Display::Plain || format.charset.keep_bytes {
                        let shown = profile_status_name(format);
                        let profile = encoding_ui::status_menu(ui, shown, "The active display profile", |ui| encoding_ui::display_menu(ui, format, self.view.source_view));
                        action = action.or(profile);
                    }
                    let counts = doc.text.eol_counts();
                    let name = eol_status_name(format, &counts, doc.text.insert_eol());
                    let on_disk = doc.text.is_disk_backed();
                    let eol = encoding_ui::status_menu(ui, name, &counts.describe(), |ui| encoding_ui::eol_menu(ui, &counts, doc.text.insert_eol(), format, can_edit, on_disk));
                    action = action.or(eol);
                }
                let encoding = encoding_ui::status_menu(ui, format.encoding_name(), format.certainty.describe(), |ui| encoding_ui::encoding_menu(ui, format, remembered, can_edit));
                action = action.or(encoding);
                if doc.hex_mode {
                    ui.label(if doc.hex.ebcdic { "Hex, EBCDIC" } else { "Hex" });
                } else {
                    if doc.text.is_disk_backed() {
                        ui.label(faint("Read from disk as needed".into())).on_hover_text("This file is too big to hold in memory, so it's read a part at a time");
                    }
                    ui.label(doc.language_name());
                }
            });
        });
        (action, toggle_lock)
    }

    // ---------- Panels ----------

    fn find_panel(&mut self, ui: &mut Ui) {
        let hex = self.docs[self.active].hex_mode;
        let mut close = false;
        let (mut next, mut previous, mut replace, mut replace_all, mut in_files) = (false, false, false, false, false);
        let (mut list_lines, mut preview) = (false, false);
        Panel::right("find-panel").exact_size(330.0).show(ui, |ui| {
            ui.add_space(4.0);
            ui.horizontal(|ui| {
                ui.label(RichText::new("Find and replace").strong().size(14.5));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    close = ui.add(Button::new(RichText::new("\u{D7}").color(theme::text_dim())).frame(false)).on_hover_text("Close").clicked();
                });
            });
            ui.add_space(4.0);
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 0.0;
                for (tab, label) in [(FindTab::Find, "Find"), (FindTab::Replace, "Replace"), (FindTab::InFiles, "Find in Files"), (FindTab::ReplaceInFiles, "Replace in Files")] {
                    let selected = self.find.tab == tab;
                    let text = RichText::new(label).color(if selected { egui::Color32::WHITE } else { theme::text_dim() });
                    let fill = if selected { theme::accent() } else { theme::ribbon() };
                    if ui.add(Button::new(text).fill(fill).corner_radius(7.0)).clicked() {
                        self.find.tab = tab;
                        self.find.status.clear();
                    }
                }
            });
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                ui.label("Find what:");
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| find_lists(ui, &mut self.find));
            });
            let find_box = ui.add(egui::TextEdit::singleline(&mut self.find.find).desired_width(f32::INFINITY));
            if self.find.focus_box {
                find_box.request_focus();
                self.find.focus_box = false;
            }
            let entered = find_box.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter));
            if entered {
                find_box.request_focus();
            }
            ui.add_space(6.0);
            let in_files_tab = matches!(self.find.tab, FindTab::InFiles | FindTab::ReplaceInFiles);
            if matches!(self.find.tab, FindTab::Replace | FindTab::ReplaceInFiles) {
                ui.label("Replace with:");
                ui.add(egui::TextEdit::singleline(&mut self.find.replace).desired_width(f32::INFINITY));
                ui.add_space(6.0);
            }
            if in_files_tab {
                egui::Frame::group(ui.style()).show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.label("In:");
                    ui.radio_value(&mut self.find.scope, FileScope::Folder, "Files in a folder");
                    ui.radio_value(&mut self.find.scope, FileScope::OpenFiles, "Open files");
                    ui.radio_value(&mut self.find.scope, FileScope::Favorites, "Favorite files");
                    let has_project = self.project.is_some();
                    if !has_project && self.find.scope == FileScope::Project {
                        self.find.scope = FileScope::Folder;
                    }
                    ui.add_enabled_ui(has_project, |ui| ui.radio_value(&mut self.find.scope, FileScope::Project, "Project files")).response.on_disabled_hover_text("Open a project from Tools, Project first");
                });
                ui.add_space(6.0);
                if self.find.scope == FileScope::Favorites {
                    let mut gone = None;
                    if self.find.favorite_files.is_empty() {
                        ui.label(RichText::new("No favorite files yet.").color(theme::text_dim()));
                    }
                    for (place, path) in self.find.favorite_files.iter().enumerate() {
                        ui.horizontal(|ui| {
                            if ui.small_button("\u{D7}").on_hover_text("Take off the list").clicked() {
                                gone = Some(place);
                            }
                            let name = path.file_name().map_or_else(|| path.display().to_string(), |name| name.to_string_lossy().into_owned());
                            ui.label(name).on_hover_text(path.display().to_string());
                        });
                    }
                    if let Some(place) = gone {
                        self.find.favorite_files.remove(place);
                    }
                    ui.horizontal(|ui| {
                        let this_file = self.docs[self.active].path.clone();
                        if ui.add_enabled(this_file.is_some(), Button::new("Add this file")).clicked()
                            && let Some(path) = this_file
                            && !self.find.favorite_files.contains(&path)
                        {
                            self.find.favorite_files.push(path);
                        }
                        if ui.button("Add files...").clicked()
                            && let Some(paths) = rfd::FileDialog::new().set_title("Add favorite files").pick_files()
                        {
                            for path in paths {
                                if !self.find.favorite_files.contains(&path) {
                                    self.find.favorite_files.push(path);
                                }
                            }
                        }
                    });
                    ui.add_space(4.0);
                }
                if self.find.scope == FileScope::Folder {
                    ui.label("In files/types:");
                    ui.add(egui::TextEdit::singleline(&mut self.find.file_types).desired_width(f32::INFINITY).hint_text("*.txt; *.log"));
                    ui.add_space(4.0);
                    ui.label("Directory:");
                    ui.horizontal(|ui| {
                        ui.add(egui::TextEdit::singleline(&mut self.find.folder).desired_width(ui.available_width() - 34.0));
                        if ui.button("...").on_hover_text("Choose a folder").clicked()
                            && let Some(folder) = rfd::FileDialog::new().set_title("Search in folder").pick_folder()
                        {
                            self.find.folder = folder.display().to_string();
                        }
                    });
                    ui.checkbox(&mut self.find.recursive, "Include subfolders");
                    ui.add_space(4.0);
                }
            }
            ui.checkbox(&mut self.find.match_case, "Match case");
            if hex && !in_files_tab {
                ui.checkbox(&mut self.find.hex_bytes, "Hex bytes");
            } else {
                ui.checkbox(&mut self.find.whole_word, "Whole word");
                ui.checkbox(&mut self.find.regex, "Regular expression");
            }
            if self.find.tab == FindTab::ReplaceInFiles {
                ui.checkbox(&mut self.find.keep_backups, "Keep a .bak copy of each file changed on disk");
            }
            ui.add_space(8.0);
            ui.horizontal_wrapped(|ui| match self.find.tab {
                FindTab::Find => {
                    next = ui.button("Find Next").clicked() || entered;
                    previous = ui.button("Find Previous").clicked();
                    list_lines = ui.add_enabled(!hex, Button::new("List lines")).on_hover_text("Lists each line containing the find string, in the results below").clicked();
                }
                FindTab::Replace => {
                    next = ui.button("Find Next").clicked() || entered;
                    replace = ui.button("Replace").clicked();
                    replace_all = ui.button("Replace All").clicked();
                }
                FindTab::InFiles => {
                    in_files = ui.button("Find").clicked() || entered;
                }
                FindTab::ReplaceInFiles => {
                    preview = ui.button("Preview").on_hover_text("Lists every match. Nothing changes until you go ahead from the results").clicked() || entered;
                }
            });
            if !self.find.status.is_empty() {
                ui.add_space(6.0);
                ui.label(&self.find.status);
            }
        });
        if next {
            self.run_find(false);
        }
        if previous {
            self.run_find(true);
        }
        if replace {
            self.run_replace();
        }
        if replace_all {
            self.run_replace_all();
        }
        if in_files {
            self.start_find_in_files(false);
        }
        if preview {
            self.start_find_in_files(true);
        }
        if list_lines {
            self.list_matching_lines();
        }
        if close {
            self.find.open = false;
            self.pending_focus = true;
        }
    }

    fn results_panel(&mut self, ui: &mut Ui) {
        if self.job.is_none() && !self.script_output.is_empty() {
            self.script_output_panel(ui);
            return;
        }
        let Some(job) = self.job.clone() else {
            let mut close = false;
            Panel::bottom("results-panel").default_size(120.0).resizable(true).show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.label("Output window: results from Find in Files appear here.");
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        close = ui.small_button("\u{D7}").on_hover_text("Close").clicked();
                    });
                });
            });
            self.show_results = !close;
            return;
        };
        let finished = job.finished.load(Ordering::SeqCst);
        if !finished {
            ui.ctx().request_repaint_after(std::time::Duration::from_millis(100));
        }
        let hits = job.hits.lock().unwrap().clone();
        let mut chosen = None;
        let mut close = false;
        let mut ask_to_replace = false;
        let ready_to_replace = finished && self.replace_plan.is_some() && self.replace_job.is_none() && !hits.is_empty() && !job.truncated.load(Ordering::Relaxed);
        let title = self.results_title;
        Panel::bottom("results-panel").default_size(190.0).resizable(true).show(ui, |ui| {
            ui.horizontal(|ui| {
                let searched = job.files_searched.load(Ordering::Relaxed);
                let unreadable = job.files_unreadable.load(Ordering::Relaxed);
                let mut summary = format!(
                    "{} {}: {} match{} in {} file{}",
                    title,
                    if finished { "finished" } else { "searching" },
                    hits.len(),
                    if hits.len() == 1 { "" } else { "es" },
                    searched,
                    if searched == 1 { "" } else { "s" }
                );
                if job.truncated.load(Ordering::Relaxed) {
                    summary += &format!(" (stopped at {})", find_files::MAX_HITS);
                }
                if unreadable > 0 {
                    summary += &format!(", {unreadable} couldn't be read");
                }
                ui.label(summary);
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    close = ui.small_button("\u{D7}").on_hover_text("Close").clicked();
                    if !finished && ui.small_button("Stop").clicked() {
                        job.cancel.store(true, Ordering::Relaxed);
                    }
                    if ready_to_replace {
                        ask_to_replace = ui.button("Replace these...").on_hover_text("Asks first").clicked();
                    }
                });
            });
            ui.separator();
            let row = ui.text_style_height(&egui::TextStyle::Monospace) + 2.0;
            egui::ScrollArea::both().id_salt("results").auto_shrink([false, false]).show_rows(ui, row, hits.len(), |ui, range| {
                for hit in &hits[range] {
                    let label = format!("{}({}): {}", hit.path.display(), hit.line + 1, hit.text);
                    let text = RichText::new(label).monospace();
                    if ui.add(egui::Label::new(text).wrap_mode(egui::TextWrapMode::Extend).sense(egui::Sense::click())).clicked() {
                        chosen = Some(hit.clone());
                    }
                }
            });
        });
        if let Some(hit) = chosen {
            self.open_hit(&hit);
        }
        if ask_to_replace {
            let mut files: Vec<PathBuf> = hits.iter().map(|hit| hit.path.clone()).collect();
            files.dedup();
            self.dialog = Dialog::ConfirmReplaceInFiles { files, matches: hits.len() };
        }
        if close {
            self.replace_plan = None;
            job.cancel.store(true, Ordering::Relaxed);
            self.show_results = false;
            self.job = None;
            self.pending_focus = true;
        }
    }

    fn dialogs(&mut self, ctx: &Context) {
        self.addon_panels_ui(ctx);
        if self.palette_dialog(ctx) || self.addons_dialog(ctx) || self.remote_dialog(ctx) || self.theme_dialog(ctx) || self.send_dialog(ctx) || self.devices_dialog(ctx) || self.run_dialog(ctx) || self.assembler_dialog(ctx) || self.new_kinds_dialog(ctx) || self.rename_dialog(ctx) || self.disassemble_dialog(ctx) || self.new_image_dialog(ctx) || self.language_servers_dialog(ctx) || self.tasks_dialog(ctx) || self.git_history_dialog(ctx) || self.git_wizard_dialog(ctx) || self.script_dialog(ctx) || self.file_tool_dialogs(ctx) || self.assistant_dialogs(ctx) || self.recovery_dialog(ctx) || self.encoding_dialog(ctx) {
            return;
        }
        let mut close = false;
        let mut goto = None;
        let mut save_basic = None;
        let mut fill = None;
        let mut number = None;
        let mut show_features = false;
        let title = match &self.dialog {
            Dialog::None => return,
            Dialog::GoTo { .. } => "Go To".to_string(),
            Dialog::Fill { .. } => "Insert or fill columns".to_string(),
            Dialog::SaveBasic { .. } => "Save as a BASIC program".to_string(),
            Dialog::Number { .. } => "Insert Number".to_string(),
            Dialog::Message { title, .. } => title.clone(),
            Dialog::About { .. } => "About Limoniter".to_string(),
            _ => return,
        };
        let hex = self.docs[self.active].hex_mode;
        let enter = ctx.input(|i| i.key_pressed(Key::Enter));
        if ctx.input(|i| i.key_pressed(Key::Escape)) {
            close = true;
        }
        egui::Window::new(title).collapsible(false).resizable(false).anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0]).show(ctx, |ui| match &mut self.dialog {
            Dialog::None => {}
            Dialog::GoTo { value, error } => {
                ui.label(if hex { "Offset (decimal, or 0x for hex):" } else { "Line number:" });
                ui.text_edit_singleline(value).request_focus();
                if !error.is_empty() {
                    ui.label(error.as_str());
                }
                let (ok, cancel) = ok_cancel(ui);
                close |= cancel;
                if ok || enter {
                    goto = Some(value.clone());
                }
            }
            Dialog::SaveBasic { machine, run_from, error } => {
                match machine {
                    basic::Machine::Commodore(cbm) => {
                        ui.label("Which machine is it for?");
                        for choice in basic::CbmMachine::ALL {
                            ui.radio_value(cbm, choice, choice.name());
                        }
                    }
                    _ => {
                        ui.label("Run it from line (leave this empty for a program that doesn't start by itself):");
                        ui.text_edit_singleline(run_from).request_focus();
                    }
                }
                if !error.is_empty() {
                    ui.label(error.as_str());
                }
                let (ok, cancel) = ok_cancel(ui);
                close |= cancel;
                if ok || enter {
                    save_basic = Some((*machine, run_from.trim().to_string()));
                }
            }
            Dialog::Fill { value, skip_last } => {
                ui.label("Text to put on every line, starting at this line and column:");
                ui.text_edit_singleline(value).request_focus();
                ui.checkbox(skip_last, "Leave an empty last line alone");
                let (ok, cancel) = ok_cancel(ui);
                close |= cancel;
                if ok || enter {
                    fill = Some((value.clone(), *skip_last));
                }
            }
            Dialog::Number { first, step, error } => {
                egui::Grid::new("number-grid").num_columns(2).show(ui, |ui| {
                    ui.label("First number:");
                    ui.text_edit_singleline(first);
                    ui.end_row();
                    ui.label("Add each line:");
                    ui.text_edit_singleline(step);
                    ui.end_row();
                });
                if !error.is_empty() {
                    ui.label(error.as_str());
                }
                let (ok, cancel) = ok_cancel(ui);
                close |= cancel;
                if ok || enter {
                    number = Some((first.clone(), step.clone()));
                }
            }
            Dialog::Message { body, .. } => {
                ui.label(body.as_str());
                ui.add_space(6.0);
                close |= ui.button("OK").clicked() || enter;
            }
            Dialog::About { notices } => {
                let (ok, features) = about_page(ui, notices.as_deref());
                close |= ok || enter;
                show_features |= features;
            }
            _ => {}
        });

        if show_features {
            close = true;
            self.open_features();
        }
        if let Some(value) = goto {
            let doc = self.doc();
            let found = if doc.hex_mode { doc.hex.go_to(&value) } else { doc.text.go_to_numbered(&value) };
            if found {
                scripting::note_line(format!("go_to_place({});", scripting::quoted(&value)));
                close = true;
            } else if let Dialog::GoTo { error, .. } = &mut self.dialog {
                *error = format!("\"{value}\" isn't a valid position.");
            }
        }
        if let Some((machine, run_from)) = save_basic {
            match run_from.parse::<u16>() {
                _ if run_from.is_empty() => {
                    self.dialog = Dialog::None;
                    self.save_tokenised(machine, None);
                }
                Ok(line) if line <= 9999 => {
                    self.dialog = Dialog::None;
                    self.save_tokenised(machine, Some(line));
                }
                _ => {
                    if let Dialog::SaveBasic { error, .. } = &mut self.dialog {
                        *error = "A line to run from is a number from 0 to 9999.".to_string();
                    }
                }
            }
        }
        if let Some((value, skip_last)) = fill {
            self.fill_skip_last = skip_last;
            if !value.is_empty() {
                self.doc().text.insert_column_fill(&value, skip_last);
            }
            close = true;
        }
        if let Some((first, step)) = number {
            let message = match (first.trim().parse::<i64>(), step.trim().parse::<i64>()) {
                (Ok(a), Ok(b)) => {
                    if self.doc().text.insert_column_numbers(a, b) {
                        close = true;
                        String::new()
                    } else {
                        "Those numbers get too large to count.".to_string()
                    }
                }
                _ => "Please enter whole numbers.".to_string(),
            };
            if let Dialog::Number { error, .. } = &mut self.dialog {
                *error = message;
            }
        }
        if close {
            self.dialog = Dialog::None;
            self.pending_focus = true;
        }
    }

    fn continue_screenshot(&mut self, ctx: &Context) {
        let Some(run) = &mut self.screenshot else {
            return;
        };
        ctx.request_repaint();
        run.frames += 1;
        if run.frames < SCREENSHOT_STEPS_AT {
            return;
        }
        if run.frames == SCREENSHOT_STEPS_AT {
            run.steps = std::env::var("LIMONITER_SHOT_STEPS").unwrap_or_default().split(',').filter(|step| !step.is_empty()).map(str::to_string).collect();
        }
        // Each step waits for whatever the one before started to finish, so large files can be shown at work's end.
        if self.shot_busy() {
            if let Some(run) = &mut self.screenshot {
                run.quiet = 0;
            }
            return;
        }
        let Some(run) = &mut self.screenshot else {
            return;
        };
        if let Some(step) = run.steps.pop_front() {
            run.quiet = 0;
            self.screenshot_step(&step);
            return;
        }
        run.quiet += 1;
        if run.saved.load(Ordering::SeqCst) {
            ctx.send_viewport_cmd(ViewportCommand::Close);
        } else if run.frames >= SCREENSHOT_FRAMES && run.quiet >= 30 && !run.requested {
            run.requested = true;
            let (path, saved) = (run.path.clone(), run.saved.clone());
            ctx.request_screenshot(move |image| {
                if let Err(problem) = save_png(&path, &image) {
                    eprintln!("Couldn't save {}: {problem}", path.display());
                    std::process::exit(1);
                }
                saved.store(true, Ordering::SeqCst);
            });
        }
    }

    /// Whether anything is being worked on in the background, for a picture to wait for.
    fn shot_busy(&self) -> bool {
        self.big_job.is_some() || self.docs.iter().any(|doc| doc.is_busy() || doc.table.as_deref().is_some_and(|table| table.working()))
    }

    /// A picture can show the app partway through something: LIMONITER_SHOT_STEPS lists what to do first. Steps
    /// with a value follow it after a colon.
    fn screenshot_step(&mut self, step: &str) {
        if let Some((name, value)) = step.split_once(':') {
            match name {
                "open-entry" => self.open_from_browser(self.active, value.parse().unwrap_or(0)),
                // File properties shown, as Info shows it beside an image.
                "properties" => self.panels.file_properties = value != "off",
                // Files waiting to go on the image in front, as if dropped on it; semicolons between.
                "image-add" => {
                    let paths: Vec<PathBuf> = value.split(';').map(PathBuf::from).collect();
                    self.drop_on_image(&paths);
                    // "image-add:...;" with "add" as its last part adds them straight away.
                    if value.ends_with(";add")
                        && let Some(browser) = self.docs[self.active].browser.as_mut()
                    {
                        browser.confirm_add_for_picture();
                        self.note_image_changed(self.active);
                    }
                }
                "screen-designer" => {
                    // A picture brought in, for a picture.
                    let mut designer = crate::screen_designer::Designer::default();
                    if let Ok(bytes) = std::fs::read(value)
                        && let Ok((w, h, rgba)) = crate::screen_designer::read_png(&bytes)
                    {
                        designer.screen = crate::screen_designer::Screen::from_picture(w, h, &rgba);
                    }
                    self.screen_designer = Some(Box::new(designer));
                }
                "pixel-editor" => {
                    // A few invaders, or a sprite, for a picture.
                    let kind = if value == "sprites" { crate::pixel_editor::Kind::Sprites } else { crate::pixel_editor::Kind::Characters };
                    let mut editor = crate::pixel_editor::Editor::new(kind);
                    if kind == crate::pixel_editor::Kind::Characters {
                        editor.sheet = crate::pixel_editor::Sheet::from_bytes(kind, &[0x18, 0x3C, 0x7E, 0xDB, 0xFF, 0x24, 0x5A, 0xA5, 0x18, 0x3C, 0x7E, 0xDB, 0xFF, 0x5A, 0x81, 0x42, 0x24, 0x18, 0x3C, 0x5A, 0xFF, 0xBD, 0xA5, 0x18]);
                    } else {
                        let ball: Vec<u8> = (0..256i32).map(|i| {
                            let (x, y) = (i % 16, i / 16);
                            let d = (x * 2 - 15).pow(2) + (y * 2 - 15).pow(2);
                            if d < 120 { if (x + y) % 5 == 0 { 0xFF } else { 0xE0 } } else if d < 200 { 0x80 } else { crate::pixel_editor::TRANSPARENT }
                        }).collect();
                        editor.sheet = crate::pixel_editor::Sheet::from_bytes(kind, &ball);
                    }
                    self.pixel_editor = Some(Box::new(editor));
                }
                "debug-at" => {
                    // Stopped at a line, with watches, a condition and a window on memory, for a picture.
                    let line: usize = value.parse().unwrap_or(1);
                    highlight::load_now();
                    self.doc().resolved_syntax = None;
                    self.doc().update_syntax();
                    self.assembly_tick();
                    if let Some(code) = &mut self.doc().text.code {
                        code.breakpoints.insert(line.saturating_sub(1));
                        code.conditions.insert(line.saturating_sub(1), "B=3".into());
                        code.watches = vec!["A".into(), "[HL]".into(), "HL - 22528".into()];
                        code.memory_at = "HL".into();
                    }
                    self.run_on_target(run_host::Target::BuiltIn);
                    self.run_until_stopped_for_screenshot();
                }
                "run-frames" => self.run_frames_for_screenshot(value.parse().unwrap_or(50)),
                "terminal" => {
                    // A command typed in the terminal, and a moment for it to answer.
                    if self.terminal.is_none() {
                        self.toggle_terminal();
                    }
                    if let Some(terminal) = self.terminal.as_mut() {
                        std::thread::sleep(Duration::from_millis(1500));
                        terminal.send(format!("{value}\r").as_bytes());
                        std::thread::sleep(Duration::from_millis(2500));
                    }
                }
                "profile" => {
                    // Counted for a number of frames, then left to read.
                    for session in &mut self.running.sessions {
                        if let run_host::SessionKind::BuiltIn { machine, .. } = &mut session.kind {
                            machine.set_profiling(true);
                        }
                    }
                    self.run_frames_for_screenshot(value.parse().unwrap_or(100));
                    for session in &mut self.running.sessions {
                        if let run_host::SessionKind::BuiltIn { machine, .. } = &mut session.kind {
                            machine.set_profiling(false);
                        }
                    }
                }
                "basic-debug" => {
                    // A BASIC listing run on the built-in Spectrum, stopped at a line's breakpoint, for a picture.
                    let line: usize = value.parse().unwrap_or(1);
                    self.assembly_tick();
                    let number = crate::zx_basic_debug::line_number(&self.doc().text.line(line.saturating_sub(1)));
                    if let Some(code) = &mut self.doc().text.code {
                        code.breakpoints.insert(line.saturating_sub(1));
                    }
                    self.run_on_target(run_host::Target::BuiltIn);
                    for session in &mut self.running.sessions {
                        if let run_host::SessionKind::BuiltIn { machine, .. } = &mut session.kind {
                            machine.set_basic_breakpoints(Some(number.into_iter().collect()));
                            for _ in 0..50 * 10 {
                                if machine.frame().is_err() || machine.basic_stopped.is_some() {
                                    break;
                                }
                            }
                        }
                    }
                }
                "git-wizard" => {
                    self.open_git_wizard();
                    if let Some(wizard) = &mut self.git.wizard {
                        use git_wizard::Step;
                        wizard.step = [Step::Welcome, Step::Install, Step::Identity, Step::Project, Step::Online, Step::Done][value.parse::<usize>().unwrap_or(0).min(5)];
                    }
                }
                "compare-with" => {
                    let lines = Self::lines_of_file_on_disk(Path::new(value));
                    self.show_comparison(lines);
                }
                "convert" => {
                    if let Some(charset) = charset::by_id(value) {
                        let target = TextFormat { charset, ..self.docs[self.active].text.format.clone() };
                        self.convert_to(self.active, target);
                    }
                }
                // The Send box, with the device in that place in the list chosen.
                "send" => {
                    self.screenshot_step("send");
                    if let Ok(device) = value.parse() {
                        self.open_send_box(device);
                    }
                }
                _ => self.table_step(name, value),
            }
            return;
        }
        {
            match step {
                "close" => self.close_tab(self.active),
                "ebcdic" => self.set_view(Some(true)),
                "select" => {
                    let bytes = &mut self.doc().hex;
                    bytes.move_caret(8, false);
                    bytes.move_caret(40, true);
                }
                "end" => {
                    let bytes = &mut self.doc().hex;
                    let len = bytes.len();
                    bytes.move_caret(len.saturating_sub(20), false);
                    bytes.move_caret(len, true);
                }
                "settings" => self.dialog = Dialog::Settings(settings::WindowState::default()),
                "settings-editing" => self.dialog = Dialog::Settings(settings::WindowState { section: 1, ..settings::WindowState::default() }),
                "settings-types" => {
                    let mut file_types = settings::FileTypesState::from_registered(associations::registered(), associations::this_program_shown());
                    file_types.ticked.extend(["txt", "log", "md", "csv", "asm"].map(str::to_string));
                    self.dialog = Dialog::Settings(settings::WindowState { section: 5, file_types, ..settings::WindowState::default() });
                }
                "preview" => self.doc().preview_view = settings::PreviewView::Split,
                "about" => self.about(),
                "features" => self.open_features(),
                "palette" => {
                    let mut state = menus::PaletteState::new();
                    state.query_for_picture("line");
                    self.dialog = Dialog::Palette(state);
                }
                menu if menu.starts_with("menu-") => self.shown_menu = menu["menu-".len()..].parse().ok().map(|index| (index, false)),
                "history" => self.edit_tools_screenshot_step("history"),
                "part-history" => self.part_screenshot_step(),
                "tags" => self.tags_screenshot_step(),
                "assistant" | "assistant-review" | "assistant-history" => self.assistant_screenshot_step(step),
                "help" => self.open_help(),
                "snippets" | "run-tool" | "tool-output" | "spelling" | "spell-box" | "rename" | "split-box" | "cycle" => self.file_tools_screenshot_step(step),
                "narrow" => self.edit_tools_screenshot_step("narrow"),
                "text-object" => self.edit_tools_screenshot_step("text-object"),
                "preview-full" => self.doc().preview_view = settings::PreviewView::Full,
                "settings-files" => self.dialog = Dialog::Settings(settings::WindowState { section: 4, ..settings::WindowState::default() }),
                "addons" | "addon-panel" => self.script_screenshot_step(step),
                step if step.starts_with("window-theme=") => self.themes.window = step["window-theme=".len()..].to_string(),
                step if step.starts_with("document-theme=") => self.doc().theme_override = Some(step["document-theme=".len()..].to_string()),
                "themes" => self.open_theme_box(theme_host::ThemePage::Documents),
                "themes-window" => self.open_theme_box(theme_host::ThemePage::Window),
                "themes-rules" => self.open_theme_box(theme_host::ThemePage::Rules),
                "basic-listing" => self.open_basic_listing(),
                "graph-mode" => self.toggle_graph_mode(),
                "new-kinds" => self.open_new_kinds_box(),
                "code-intel" => {
                    // The outline, a problem marked as it's typed, and words offered to finish one being typed.
                    highlight::load_now();
                    self.doc().update_syntax();
                    self.panels.functions = true;
                    self.refresh_intel();
                    let doc = &mut self.docs[self.active];
                    let line = doc.text.line_count().saturating_sub(1);
                    doc.text.move_caret(line, 0, false);
                    doc.text.type_text("        ld hl,co");
                    self.refresh_intel();
                    let doc = &mut self.docs[self.active];
                    crate::views::offer_completions(&doc.text, &mut doc.text_state, true);
                }
                "git-panel" => {
                    self.git.open = true;
                    self.git.blame_on = true;
                }
                "amiga-ansi" => {
                    let doc = self.doc();
                    doc.text.format.charset = charset::known("amiga-text");
                    doc.text.format.display = codec::Display::AmigaConsole;
                    doc.preview_view = settings::PreviewView::Split;
                    self.preview_of = None;
                    self.palette_editor.pen = Some(3);
                    self.palette_editor.hex = format!("{:03X}", self.amiga_palette.pens[3]);
                }
                "as-spectrum-basic" => {
                    let stem = self.doc().name();
                    self.doc().listing_of = Some((basic::Machine::Spectrum, stem));
                }
                "run-builtin" => {
                    // Source is known for what it is first, as it would be a moment after opening.
                    highlight::load_now();
                    self.doc().update_syntax();
                    self.assembly_tick();
                    self.run_on_target(run_host::Target::BuiltIn);
                }
                "assembler-box" => {
                    highlight::load_now();
                    self.doc().resolved_syntax = None;
                    self.doc().update_syntax();
                    self.open_assembler_box();
                }
                "assemble" => {
                    highlight::load_now();
                    self.doc().resolved_syntax = None;
                    self.doc().update_syntax();
                    self.assembly_tick();
                    self.run_on_machine();
                    self.dialog = Dialog::None;
                }
                // Stops a program at line 6 of its source, as a breakpoint set there does, and waits for it to.
                "debug-builtin" => {
                    highlight::load_now();
                    self.doc().resolved_syntax = None;
                    self.doc().update_syntax();
                    self.assembly_tick();
                    if let Some(code) = &mut self.doc().text.code {
                        code.breakpoints.insert(5);
                    }
                    self.run_on_target(run_host::Target::BuiltIn);
                    self.run_until_stopped_for_screenshot();
                }
                "table" | "table-block" => {
                    self.check_for_table();
                    self.toggle_table();
                    if let Some(state) = self.doc().table.as_deref_mut() {
                        state.cursor = (2, 2);
                        if step == "table-block" {
                            state.anchor = Some((1, 3));
                            state.cursor = (6, 4);
                        }
                    }
                }
                "devices" | "send" => {
                    use crate::deploy::{Device, Place};
                    self.sends.devices = vec![
                        Device { name: "Gotek".into(), place: Place::Card { label: "GOTEK".into(), folder: "GAMES/WIP".into() }, preset: None, eject: true },
                        Device { name: "Spectrum Next".into(), place: Place::NextSync { folder: "/home/me".into() }, preset: None, eject: false },
                        Device { name: "Ultimate 64".into(), place: Place::Ultimate { host: "192.168.1.64".into() }, preset: None, eject: false },
                        Device { name: "MiSTer".into(), place: Place::Sftp { address: "sftp://root@mister/media/fat/games/C64".into(), key_file: String::new() }, preset: None, eject: false },
                        Device { name: "VICE".into(), place: Place::Command { line: "x64sc -autostart {file}".into() }, preset: Some("commodore".into()), eject: false },
                    ];
                    if step == "devices" {
                        self.open_devices_box();
                        if let Dialog::Devices(state) = &mut self.dialog {
                            state.selected = 1;
                        }
                    } else {
                        self.open_send_box(2);
                    }
                }
                "remote" => {
                    self.open_remote_box(None);
                    if let Dialog::Remote(state) = &mut self.dialog {
                        state.address = "sftp://ann@files.example.org/home/ann/projects".into();
                        state.key_file = "C:\\Users\\ann\\.ssh\\id_rsa".into();
                        state.folder = Some("/home/ann/projects".into());
                        state.status = "5 item(s)".into();
                        let entry = |name: &str, folder: bool, size: u64| crate::remote::Entry { name: name.into(), folder, size, modified: 0 };
                        state.entries = vec![entry("archive", true, 0), entry("website", true, 0), entry("notes.txt", false, 2_310), entry("server.conf", false, 18_204), entry("trip-log.csv", false, 4_812_331)];
                        state.file_name = "notes.txt".into();
                    }
                }
                "macro-panels" => self.script_screenshot_step("macro-panels"),
                "macro-record" => self.script_screenshot_step("macro-record"),
                "macro-save" => self.script_screenshot_step("macro-save"),
                "macro-times" => self.script_screenshot_step("macro-times"),
                "script-running" => self.script_screenshot_step("script-running"),
                "script-output" => self.script_screenshot_step("script-output"),
                "edit-top" => self.doc().text.type_text("edited in the real program "),
                "compare" => {
                    let doc = &self.docs[self.active];
                    let mut other = doc.text.all_lines();
                    for line in [3, 9, 10] {
                        if line < other.len() {
                            other[line] = Arc::from(format!("{} (changed)", other[line]));
                        }
                    }
                    other.remove(6.min(other.len() - 1));
                    self.show_comparison(Ok(("the other file".to_string(), crate::big_jobs::Side::Lines(LineStore::from_vec(other)))));
                }
                "folds" => {
                    let text = &mut self.doc().text;
                    text.toggle_fold(4);
                    text.toggle_fold(14);
                }
                "selections" => {
                    let text = &mut self.doc().text;
                    text.move_caret(5, 4, false);
                    text.move_caret(5, 10, true);
                    let _ = text.make_selections();
                }
                "project" => {
                    let path = self.docs[self.active].path.clone().unwrap_or_default();
                    let base = path.parent().map_or_else(PathBuf::new, Path::to_path_buf);
                    let mut sample = Project::new(&base.join("sample.limproj"));
                    sample.add([path, base.join("src").join("main.rs"), base.join("src").join("view").join("draw.rs"), base.join("notes.md")]);
                    self.project = Some(sample);
                    self.panels.project = true;
                }
                "wrap-column" => self.dialog = Dialog::WrapColumn { value: "78".into(), error: String::new(), focus: true },
                "paragraph-setup" => self.dialog = Dialog::ParagraphSetup { width: "78".into(), left: "0".into(), first: "0".into(), align: Align::Left, error: String::new() },
                "split-across" => self.toggle_split(false),
                "split-down" => self.toggle_split(true),
                "duplicate" => self.doc().duplicate_window(),
                "changes" => {
                    let text = &mut self.doc().text;
                    for line in [2, 3, 4, 9] {
                        text.move_caret(line, 0, false);
                        text.type_text("edited ");
                    }
                    text.move_caret(0, 0, false);
                }
                "filter" => {
                    let text = &mut self.doc().text;
                    text.move_caret(5, 4, false);
                    text.move_caret(5, 10, true);
                    self.toggle_filter();
                }
                "replace-files" => {
                    self.find.open = true;
                    self.find.tab = FindTab::ReplaceInFiles;
                    self.find.find = "needle".into();
                    self.find.replace = "pin".into();
                }
                "column" => self.doc().text.set_column_mode(true),
                "column-select" => {
                    let text = &mut self.doc().text;
                    text.set_column_mode(true);
                    let _ = text.select_columns(12);
                }
                "cut" => self.dialog = Dialog::ColumnRemove { cut: true, count: "0".into(), error: String::new(), focus: true },
                "fill" => self.dialog = Dialog::Fill { value: String::new(), skip_last: true },
                "csv-fixed" => self.dialog = Dialog::CsvToFixed { rows: (0, 4), delimiter: ',', error: String::new() },
                "fixed-csv" => self.dialog = Dialog::FixedToCsv { rows: (0, 4), delimiter: ',', starts: "1, 11, 15".into(), error: String::new(), focus: true },
                "insert-delete" => self.dialog = Dialog::HexInsertDelete { count: "32".into(), delete: false, fill: "00".into(), error: String::new(), focus: true },
                _ => {}
            }
        }
    }

    /// A document read again from its file: an image being browsed opens in the browser again, back at its top,
    /// and anything else as a file.
    fn reopened(&self, index: usize, path: &Path) -> Result<Document, String> {
        if self.docs[index].browser.is_some() {
            return image_browser::image_document(path);
        }
        Document::open(path, self.encoding_prefs.get(path))
    }

    /// When the window comes back into focus, offers to reload any file another program has changed.
    /// A file counts as changed if its size or modified time differs.
    fn check_for_changed_files(&mut self) {
        if self.screenshot.is_some() {
            return;
        }
        for index in 0..self.docs.len() {
            let doc = &mut self.docs[index];
            if doc.is_busy() {
                continue;
            }
            let Some(path) = doc.path.clone() else {
                continue;
            };
            let now = file_stamp(&path);
            if now.is_none() || now == doc.disk_stamp {
                continue;
            }
            // Asking once per change, whatever the answer, so that it doesn't keep coming back.
            doc.disk_stamp = now;
            let modified = doc.is_modified();
            let question = format!("{} has been changed by another application.\n\nDo you want to reload it?\n", path.display());
            if !ask_yes_no("File Changed", &question) {
                continue;
            }
            if modified && !ask_yes_no("File Changed", "Reload file and lose changes?") {
                continue;
            }
            match self.reopened(index, &path) {
                Ok(mut fresh) => {
                    fresh.preview_view = self.docs[index].preview_view;
                    self.docs[index] = fresh;
                    self.pending_focus = true;
                }
                Err(e) => warn(&format!("Couldn't open {}.\n\n{e}", path.display())),
            }
        }
    }

    fn handle_window_events(&mut self, ctx: &Context) {
        // A script is working on the file in front, which has to stay the file in front until it's finished.
        if self.script.is_none() {
            if self.prefs.watch_files && ctx.input(|i| i.raw.events.iter().any(|e| matches!(e, egui::Event::WindowFocused(true)))) {
                self.check_for_changed_files();
            }
            let dropped: Vec<PathBuf> = ctx.input(|i| i.raw.dropped_files.iter().map(|f| f.path().to_path_buf()).collect());
            // Files dropped on an image that can be changed are put on it; anywhere else they open.
            if !self.drop_on_image(&dropped) {
                for path in dropped {
                    self.open_path(&path);
                }
            }
        }

        if ctx.input(|i| i.viewport().close_requested()) && !self.allow_close {
            self.stop_script();
            let mut proceed = true;
            for index in 0..self.docs.len() {
                if !self.confirm_close(index) {
                    proceed = false;
                    break;
                }
            }
            if proceed {
                self.allow_close = true;
            } else {
                ctx.send_viewport_cmd(ViewportCommand::CancelClose);
            }
        }

        let doc = &self.docs[self.active];
        let name = doc.path.as_ref().map_or_else(|| doc.name(), |p| p.display().to_string());
        let title = if self.no_file() { "Limoniter".to_string() } else { format!("{}{} - Limoniter", name, if doc.is_modified() { " *" } else { "" }) };
        if title != self.window_title {
            ctx.send_viewport_cmd(ViewportCommand::Title(title.clone()));
            self.window_title = title;
        }
    }
}

impl eframe::App for Limoniter {
    /// If drawing a frame ever fails, the very latest edits are written out before anything else happens.
    fn ui(&mut self, ui: &mut Ui, frame: &mut eframe::Frame) {
        recovery::IN_FRAME.store(true, Ordering::SeqCst);
        let attempt = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| self.draw_frame(ui)));
        recovery::IN_FRAME.store(false, Ordering::SeqCst);
        let window_theme = theme::window_theme();
        if self.title_bar_theme.as_ref() != Some(&window_theme) {
            self.title_bar_theme = Some(window_theme);
            theme::colour_title_bar(&*frame);
        }
        if attempt.is_err() {
            self.emergency_save();
            crate::tell_about_crash();
            std::process::exit(1);
        }
    }

    /// A normal exit, once any unsaved changes have been dealt with. Nothing's left to recover.
    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        self.finish_history_job();
        if let Some(listening) = &self.listening {
            listening.stop();
        }
        self.stop_tool();
        self.stop_split();
        self.stop_assistant();
        if self.allow_close {
            self.close_project();
        }
        if self.allow_close
            && let Some(session) = self.recovery.take()
        {
            session.finish();
        }
        // The files large changes were written to are read from only while the window is open.
        if self.allow_close {
            self.docs.clear();
            self.large_edits = None;
        }
    }
}

impl Limoniter {
    fn draw_frame(&mut self, ui: &mut Ui) {
        let ctx = ui.ctx().clone();
        NO_FILE.set(self.no_file());
        self.apply_themes(&ctx);
        text_buffer::set_edit_prefs(self.prefs.edit);
        hex_buffer::set_bytes_per_row(self.prefs.hex_row);
        self.apply_tab_size();
        self.apply_source_view();
        // Columns are counted with the tokens of whichever document is in front.
        charset_widths(&self.docs[self.active].text.format);
        if self.waker.is_none() {
            self.waker = Some(ctx.clone());
        }
        self.note_file_use(&ctx);
        self.check_background_work(&ctx);
        self.continue_pending(&ctx);
        self.start_pending_play(ui);
        self.start_queued_addon(ui);
        self.continue_script(ui);
        // Everything but the Stop button waits while a script is working on the files.
        if self.script.is_some() {
            ui.disable();
        }
        // Ctrl+wheel changes the text size, as it does in most editors.
        let zoom = ctx.input(|i| i.zoom_delta());
        if zoom != 1.0 && ctx.input(|i| i.modifiers.command) {
            self.view.font_size = (self.view.font_size * zoom).clamp(8.0, 40.0);
        }
        self.shortcuts(ui);

        let mut chosen = None;
        Panel::top("menu-bar").frame(egui::Frame::new().fill(theme::ribbon()).inner_margin(egui::Margin { left: 10, right: 10, top: 5, bottom: 5 })).show(ui, |ui| {
            chosen = self.top_bar(ui);
        });
        if self.show_status {
            let (mut action, mut toggle_lock) = (None, false);
            Panel::bottom("status").frame(egui::Frame::new().fill(theme::ribbon()).inner_margin(egui::Margin { left: 12, right: 12, top: 4, bottom: 4 })).show(ui, |ui| (action, toggle_lock) = self.status_bar(ui));
            if ui.ctx().data_mut(|d| d.remove_temp::<bool>(egui::Id::unique(run_host::STOP_ASKED))).unwrap_or(false) {
                self.stop_all_sessions();
            }
            if let Some(action) = action {
                self.run_menu_action(action);
            }
            if toggle_lock && self.docs[self.active].filter.is_some() {
                self.tell_read_only();
            } else if toggle_lock {
                let on = !self.docs[self.active].text.read_only;
                self.doc().set_read_only(on);
            }
        }
        // The side panels reach up to the toolbar, so the file tabs only span the text, as in the original.
        if self.find.open {
            self.find_panel(ui);
        }
        self.side_panels(ui);
        self.git_panel(ui);
        self.assistant_panel(ui);
        self.machine_panel(ui);
        self.remote_debug_panel(ui);
        let full_preview = self.docs[self.active].preview_view != settings::PreviewView::Text && self.preview_panel(ui);
        self.full_preview_shown = full_preview;
        if self.show_file_tabs && !self.no_file() {
            Panel::top("tabs").show_separator_line(false).frame(egui::Frame::new().fill(theme::window()).inner_margin(egui::Margin { left: 8, right: 8, top: 6, bottom: 0 })).show(ui, |ui| {
                self.tab_strip(ui);
            });
        }
        self.narrow_bar(ui);
        if !self.no_file() {
            self.check_for_table();
            self.table_offer_bar(ui);
            self.git_offer_bar(ui);
            self.conflict_bar(ui);
        }
        if self.terminal.is_some() {
            self.terminal_panel(ui);
        }
        if self.test_panel.is_some() {
            self.tests_panel(ui);
        }
        if self.show_results {
            self.results_panel(ui);
        }
        if let Some(command) = chosen.or_else(|| self.pending_command.take()) {
            self.run(command, ui);
        }
        if self.script.is_none()
            && let Some(path) = self.open_request.take()
        {
            self.open_path(&path);
        }

        if self.pending_focus && matches!(self.dialog, Dialog::None) && !self.find.focus_box {
            let pane = self.docs[self.active].split.as_ref().map_or(0, |split| split.active);
            ctx.memory_mut(|m| m.request_focus(views::pane_id(pane)));
            self.pending_focus = false;
        }

        let mut context_command = None;
        let view = self.view;
        let input_on = self.script.is_none() && matches!(self.dialog, Dialog::None) && self.pending.is_none() && !self.docs[self.active].is_busy();
        egui::CentralPanel::default().frame(egui::Frame::new().fill(theme::editor_back())).show(ui, |ui| {
            let index = self.active;
            // A command from this frame may have changed what's in front since the preview was prepared.
            if full_preview && !self.docs[index].placeholder && self.docs[index].preview_view == settings::PreviewView::Full {
                self.full_preview(ui);
                return;
            }
            if self.docs[index].browser.is_some() {
                self.browser_view(ui);
                return;
            }
            let doc = &mut self.docs[index];
            if doc.placeholder {
                return;
            }
            doc.update_syntax();
            doc.refresh_fold_view();
            let folded = doc.filter.is_none() && doc.fold_view_of.is_some();
            let total = doc.text.line_count();
            let filter = doc.filter.as_ref().map(|filter| views::Shown::Lines(filter.lines.as_slice())).or(folded.then_some(views::Shown::AllBut { hidden: doc.fold_view.as_slice(), total }));
            if doc.table.is_some() && !doc.hex_mode {
                self.table_view(ui, input_on);
                return;
            }
            context_command = if doc.hex_mode {
                views::hex_view(ui, &mut doc.hex, &mut doc.hex_state, &doc.text.format, view, input_on)
            } else if let Some(split) = &mut doc.split {
                draw_split(ui, &mut doc.text, &mut doc.text_state, split, &mut self.clip, filter, folded, view, input_on)
            } else {
                views::text_pane(ui, &mut doc.text, &mut doc.text_state, &mut self.clip, view, input_on, Pane { filter, folded, ..Pane::default() })
            };
        });
        if let Some(command) = context_command {
            self.run(
                match command {
                    ContextCommand::Cut => Command::Cut,
                    ContextCommand::Copy => Command::Copy,
                    ContextCommand::Paste => Command::Paste,
                    ContextCommand::SelectAll => Command::SelectAll,
                    ContextCommand::Tag(colour) => Command::Tag(colour),
                    ContextCommand::Untag => Command::Untag,
                    ContextCommand::Assist => Command::Assist(assistant::Task::Fix),
                },
                ui,
            );
        }

        if let Some(notice) = self.docs[self.active].text.notice.take() {
            self.message("Limoniter", notice);
        }
        if let Some(notice) = self.docs[self.active].hex.notice.take() {
            self.message("Limoniter", notice);
        }
        self.dialogs(&ctx);
        self.draw_cycle(&ctx);
        self.draw_assistant_note(&ctx);
        self.pixel_editor_window(&ctx);
        self.screen_designer_window(&ctx);
        self.recovery_tick(&ctx);
        let doc = &self.docs[self.active];
        if !doc.hex_mode && !doc.placeholder && doc.scanning.is_none() {
            self.jumps.note(Place { doc: doc.id, line: doc.text.caret_line, col: doc.text.caret_col });
        }
        self.handle_window_events(&ctx);
        self.continue_screenshot(&ctx);
        self.save_settings_if_changed();
    }
}

const HELP: &str = "Finding things to do\n\
    F1 finds any command by what it does. Every command is also in the menus, which are arranged by what they're for.\n\n\
    Column editing\n\
    Alt+C turns Column Mode on and off, while Word wrap is off.\n\
    Hold Alt and drag, or press Shift+Alt+arrows, to select a block at any time.\n\
    Typing, Backspace and Delete then work on every selected line.\n\
    Edit, Columns fills, cuts, deletes and justifies columns, numbers them, and adds up numbers.\n\
    Format, Table changes comma-separated text to fixed-width columns and back.\n\n\
    Hex mode\n\
    Ctrl+H switches between text and hex. View, Hex also offers EBCDIC.\n\
    Insert or delete bytes adds or removes some at the cursor, and Copy the selected rows copies them as they look on screen.\n\
    Tab moves between the hex and character panes.\n\
    Insert switches between overwriting and inserting bytes.\n\
    Ctrl+G goes to an offset; use 0x for a hex offset.\n\
    View, Hex, String explorer lists the readable text in the bytes and says how each string is kept: a length before it, a zero after it, padding to a fixed width, a last character with its top bit set, or a $ at its end. Clicking one selects it.\n\n\
    Settings\n\
    Tools, Settings has the dark or light theme, and how editing, hex mode, searching and files behave.\n\
    Its File types page offers Limoniter to Windows as the default editor for the types you choose.\n\n\
    Laying out text\n\
    The Format menu reindents by brackets, breaks long lines, joins them back, and reformats and aligns paragraphs.\n\
    Format, Lay out code arranges HTML, XML, JSON, JavaScript, CSS, code and SQL, and splits or joins EDI files.\n\n\
    Remote files\n\
    File, Open a remote file connects to a server over SFTP: type sftp://user@host/folder, add a key file or a password, and Connect. Click folders to go into them, and double-click a file to open it. File, Save to a server puts the file in front there.\n\
    The file is edited in a copy on this computer, and each Save sends it back. If someone has changed it on the server since, you're asked first. If sending fails, your changes are still saved here, and the message says where.\n\
    The first time you connect to a server its key's fingerprint is shown: trust it only if it's what the server's owner says. A server whose key has changed is refused. A password is kept, if you ask, in Windows Credential Manager.\n\n\
    Table view\n\
    View, Table view shows comma-, tab-, semicolon- or pipe-separated data as rows and columns, and a file that looks like such data is offered as a table when it opens. The text is still the file: changing a cell changes only that field, and Undo takes it back.\n\
    Type or double-click to change a cell. Enter keeps it and moves down, Tab moves across, and Esc leaves it as it was. F6 shows a file as a table and back.\n\
    Shift and the arrow keys, Shift+click or dragging select a block of cells. Copy puts it on the clipboard as a spreadsheet does, and a block copied from a spreadsheet pastes in, adding rows past the end. Delete empties the block. The figures for the numbers selected, or the cursor's column, are shown above the table.\n\
    Clicking a heading shows the rows in order of that column, and typing in the box above shows only the rows containing it. Neither changes the file; Sort the file by this column does. Rows with the wrong number of fields are marked by their number.\n\n\
    Themes\n\
    The window and the documents have separate themes. View, Theme chooses the window's, the usual one for documents, and one for the file in front alone.\n\
    Each document's theme can also be chosen by rules: by its file ending, its language, its character set, or being in hex or table view. View, Theme, Themes and rules sets them; the first rule that fits wins. Commodore, Atari, Spectrum, BBC and Amiga text start out in their machine's colours.\n\
    Themes that come with Limoniter can be copied and changed, colour by colour, or made from three colours at once. A theme can be saved to a file and opened elsewhere.\n\n\
    Send to device\n\
    File, Send to device sends a copy of the file to the machine it's for, and the file goes on saving where it always does. Manage devices sets up where: a card or stick found by its label, a folder, an SFTP or FTP server, a ZX Spectrum Next, an Ultimate 64, or a command line of your own with {file} where the copy goes.\n\
    After the first time, Ctrl+Alt+S sends the file in front again, to the same place and under the same name. A device can write the copy in a machine's own character set and line endings.\n\
    For a Spectrum Next, sending makes the file ready and Limoniter waits: on the Next, type .sync and this computer's address the first time, then just .sync. The Next needs NextSync's .sync command. An Ultimate 64 runs a .prg or .crt, plays a .sid or .mod, or puts a disk image in drive A as soon as it arrives.\n\
    On a card the copy is flushed before it's renamed into place, so the card can come out at once, and it can be ejected for you. If the copy there has changed since it was sent, you're asked before it's replaced.\n\n\
    Projects\n\
    Tools, Project keeps a set of files together, with backups. Find in Files can search them.\n\n\
    Macros and scripts\n\
    Tools, Macros records what you type and which commands you use, as a macro, and plays it back. Ctrl+Shift+R records and Ctrl+Shift+P plays.\n\
    Clicks in the text and panel buttons aren't recorded, so move with the keys while recording. A command that opens a box asks again when played.\n\
    A macro is a script in the Rhai language, which you can open and change. New script explains what scripts can do.\n\
    Undo takes back everything one macro or script did, in one step.\n\n\
    Add-ons\n\
    An add-on is a script that adds commands of its own to Tools, Add-ons and to the command box, and can run by itself when a file is opened, just before Save writes it, or just before Send to device sends it. New add-on makes one to start from, explaining itself.\n\
    Its first lines say what it adds: //! name: and //! about: describe it, //! command: Its title = function adds a command, and //! on_open:, //! on_save:, //! on_send: or //! on_run: function names what runs when. In on_send, send_device() names the device and set_send_name(\"NAME\") changes the name it's sent as.\n\
    Add-ons live in the add-ons folder beside your scripts, and in a shared folder if one is set in Manage add-ons, so a team can keep theirs in one place. A function can show its results with show_panel(\"title\", text) in a panel of its own.\n\
    A new add-on is off until it's turned on in Manage add-ons, and one whose file changes is turned off until it's looked at again. Like any script, it can't reach other files, programs or the network.\n\n\
    Text objects\n\
    Ctrl+Shift+Q selects what's inside the quotes around the cursor, and pressing it again takes the quotes too. B does brackets, T tags, W a word, L a line and G a paragraph.\n\
    With something selected, typing replaces it and Delete removes it. The Edit menu does the same, under Select, Replace inside and Delete.\n\n\
    Edit history\n\
    If you undo and then type something else, what you undid isn't lost. Edit, Edit history, or Ctrl+Shift+H, shows every state with its branches, and clicking one goes there.\n\
    Saving keeps the history with the file for next time, unless that's turned off in Settings under Editing. Hex mode has plain undo only.\n\
    Edit, History of this part, or Ctrl+Alt+H, lists only the changes to the selected lines, or to the function the cursor is in, wherever those lines have moved since. Any one can be taken back without undoing what was done after it anywhere else, or the part can be put back as it was before it.\n\n\
    Narrowing\n\
    View, Narrow to the selection, or Ctrl+Shift+N, shows only the selected lines, or this paragraph. Find, Replace All, Select All and every other command then work on only those lines.\n\
    The bar above the text shows the way back. Saving always saves the whole file.\n\n\
    Markdown\n\
    View, Preview shows a Markdown file beside its text, or filling the whole text area. Settings chooses how they open.\n\n\
    Pasting\n\
    Edit, Swap with clipboard, or Ctrl+Shift+V, puts the selection on the clipboard and what was there in its place. Copy before paste, in Settings under Editing, makes every paste over a selection do that.\n\n\
    Files\n\
    A file opened from Explorer opens as a tab in the Limoniter window already open. Settings, Files can give each one a window of its own instead.\n\
    Ctrl+Tab goes to the file used last. Hold Ctrl and press Tab again to go further back, and let go to stay there. Ctrl+Shift+Tab goes the other way.\n\
    File, This file renames the file, shows it in Explorer, opens it in its usual program, or splits it into parts beside it. File, Save the selection as writes just the selection.\n\n\
    Coding assistant\n\
    Tools, Assistant, or Ctrl+Shift+A, asks Claude Code or Codex to fix, clean up, explain or write code, for the selected lines or the function the cursor is in. It uses the tool installed on this computer and signed in to, which sends the code to Anthropic or OpenAI.\n\
    What comes back is shown as a change first, and only goes in when you say so. One Undo takes it out. Explanations go to the Output panel.\n\n\
    Colour tags\n\
    Select some lines, or put the cursor in a function or paragraph, and press Ctrl+Shift+1 to 6, or right-click and choose Colour tag, to mark that section in a colour. It shows as a block down the left of the text, and as a band right across the document map, which can be clicked to go there.\n\
    F4 and Shift+F4 go to the next and previous tagged section, and Search, Colour tags lists them all. Ctrl+Shift+0 takes a tag away. Tags move with their lines, and a saved file's tags are there again next time it's opened.\n\n\
    Snippets\n\
    Ctrl+J chooses a snippet to put in, or makes one. In a snippet, $CURSOR is where the cursor goes, $SELECTION is what was selected, and $DATE, $TIME and $FILE are filled in.\n\n\
    Running commands\n\
    F5 runs a command as the Command Prompt would (sh on macOS and Linux), in the file's folder, with what it prints in the Output panel. %FILE%, %NAME%, %STEM% and %DIR% stand for the file.\n\n\
    Spelling\n\
    View, Show, Spelling mistakes underlines them, using Windows' own dictionary, or hunspell or aspell on macOS and Linux. F7 goes to the next one and offers suggestions.\n\n\
    Seeing more\n\
    View, Split shows a file in two panes, or opens a second tab on it.\n\
    Search, Only show lines with the selection shows just the lines containing the selected text. Editing is off until it's ended.\n\n\
    Getting around\n\
    Alt+Left and Alt+Right go back and forward through where the cursor has been.\n\n\
    Encodings and line endings\n\
    Click the encoding or line ending in the status bar to change them.\n\
    Saving never changes bytes you didn't edit.\n\
    View, Panels, Characters follows this document's encoding.\n\n\
    If Limoniter ever stops unexpectedly\n\
    Copies of unsaved edits are kept as you type, and offered back at the next start.\n\
    File, Recover unsaved edits offers them again if you chose to decide later.";

/// Offers a copy of each document that's changed, and drops those no longer needed.
/// Returns how long until a document held back for being costly to copy can be copied.
fn offer_changes(session: &mut recovery::Session, docs: &mut [Document]) -> Option<Duration> {
    let now = std::time::Instant::now();
    let mut wake: Option<Duration> = None;
    let mut open = std::collections::HashSet::with_capacity(docs.len());
    for doc in docs {
        open.insert(doc.id);
        if doc.scanning.is_some() {
            continue;
        }
        if !doc.is_modified() {
            doc.recovery_key = None;
            session.remove(doc.id);
            continue;
        }
        let key = doc.recovery_key_now();
        if doc.recovery_key == Some(key) {
            continue;
        }
        if now < doc.recovery_next {
            let left = doc.recovery_next - now;
            wake = Some(wake.map_or(left, |known| known.min(left)));
            continue;
        }
        let started = std::time::Instant::now();
        let snapshot = doc.snapshot();
        let cost = started.elapsed();
        session.offer(snapshot);
        doc.recovery_key = Some(key);
        // Copying a small document costs nothing, but a huge one mustn't make typing stutter.
        doc.recovery_next = if cost > Duration::from_millis(1) { now + cost * 50 } else { now };
    }
    session.keep_only(&open);
    wake
}

/// Writes each changed document straight away, each on its own, so that trouble with one can't cost the others.
fn save_everything_now(session: &mut recovery::Session, docs: &[Document]) {
    for doc in docs {
        let attempt = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            if doc.scanning.is_none() && doc.is_modified() {
                let _ = session.write_now(&doc.snapshot());
            }
        }));
        drop(attempt);
    }
}

/// Whether a file is still exactly as it was when a recovery copy referred to it.
/// After a large file open in hex is saved: it's read from the saved file from now on.
fn finish_hex_save(doc: &mut Document, path: PathBuf) {
    note_saved_to(doc, &path);
    match crate::byte_store::FileBytes::open(&path) {
        Ok(file) => doc.hex.rebase(file),
        Err(e) => doc.hex.notice = Some(format!("The file was saved, but couldn't be opened again: {e}")),
    }
    doc.hex.mark_saved();
    doc.disk_stamp = file_stamp(&path);
    doc.resolved_syntax = None;
    doc.path = Some(path);
    doc.carried = false;
}

/// After a save: a remote file saved into its own copy is to be sent back; saved anywhere else, it's now a file
/// of this computer's own.
/// Why a file with unsaved changes is never saved by autosave, if it isn't: quick to check, so checked every frame.
fn autosave_never(doc: &Document) -> Option<&'static str> {
    if doc.path.is_none() {
        return Some("it hasn't been saved before");
    }
    if doc.browser.is_some() || doc.text.read_only {
        return Some("it's read-only");
    }
    if doc.remote.is_some() {
        return Some("it's on a server");
    }
    if doc.narrow.is_some() {
        return Some("only part of it is showing");
    }
    if (!doc.hex_mode && doc.text.is_disk_backed()) || (doc.hex_mode && doc.hex.is_on_disk()) {
        return Some("it's too large to save over and over");
    }
    None
}

/// Why saving it now would need a question answered, if it would: checked only when a save is due.
fn autosave_not_now(doc: &Document) -> Option<&'static str> {
    if doc.path.as_deref().is_some_and(|path| file_stamp(path) != doc.disk_stamp) {
        return Some("another program has changed the file");
    }
    if !doc.hex_mode {
        if !doc.text.uneven_rows().is_empty() {
            return Some("some rows aren't the width this format needs");
        }
        if doc.text.encode(&doc.text.format, Lossy::Refuse).is_err() {
            return Some("some characters can't be written in its encoding");
        }
    }
    None
}

fn note_saved_to(doc: &mut Document, path: &Path) {
    match &doc.remote {
        Some(link) if same_path(&link.local, path) => doc.upload_wanted = true,
        Some(_) => doc.remote = None,
        None => {}
    }
}

fn same_file_as(wanted: &crate::line_store::SourceRef) -> bool {
    std::fs::metadata(&wanted.path).is_ok_and(|meta| meta.len() == wanted.len && meta.modified().ok() == wanted.modified)
}

/// Works out what to tell the person about a copy from an earlier run.
fn offer_item(found: recovery::Found) -> OfferItem {
    let file = found.file;
    match found.outcome {
        Err(problem) => {
            let name = file.file_name().map_or_else(String::new, |n| n.to_string_lossy().into_owned());
            OfferItem { selected: false, title: format!("A damaged copy ({name})"), detail: file.display().to_string(), note: format!("It can't be recovered. {problem} It stays in place until you discard it."), content: None, file }
        }
        Ok((meta, body)) => {
            let when = panels::format_time(Ok(std::time::UNIX_EPOCH + Duration::from_secs(meta.taken)));
            let size = match &body {
                recovery::Body::Hex(bytes) => format!("{} bytes", bytes.len()),
                recovery::Body::HexOnDisk { pieces, .. } => format!("{} bytes", pieces.iter().map(|p| p.len).sum::<usize>()),
                recovery::Body::Text(image) => format!("{} edited line(s)", image.pieces.iter().map(|piece| if let crate::line_store::ImagePiece::Memory(lines) = piece { lines.len() } else { 0 }).sum::<usize>()),
            };
            let place = meta.path.as_ref().map_or_else(|| "Not saved yet".to_string(), |path| path.display().to_string());
            let (note, selected) = compare_with_file(&meta, &body);
            OfferItem { selected, title: meta.name.clone(), detail: format!("{place}\nKept {when}, {size}"), note, content: Some((meta, body)), file }
        }
    }
}

/// Whether the file on disk has moved on since, or already holds these edits.
fn compare_with_file(meta: &recovery::Meta, body: &recovery::Body) -> (String, bool) {
    let Some(path) = &meta.path else {
        return (String::new(), true);
    };
    let now = file_stamp(path);
    if now.is_none() {
        return ("The file is no longer there. Saving will create it again.".to_string(), true);
    }
    let on_disk = std::fs::metadata(path).ok().filter(|m| m.len() <= DISK_THRESHOLD).and_then(|_| std::fs::read(path).ok());
    let kept = match body {
        recovery::Body::Hex(bytes) => Some(bytes.clone()),
        recovery::Body::HexOnDisk { .. } => None,
        recovery::Body::Text(image) if image.source.is_none() => {
            let lines: Vec<Arc<str>> = image.pieces.iter().flat_map(|piece| if let crate::line_store::ImagePiece::Memory(lines) = piece { lines.clone() } else { Vec::new() }).collect();
            let eols: std::collections::BTreeMap<usize, Eol> = image.eols.iter().copied().collect();
            let base = image.base;
            meta.format().ok().and_then(|format| codec::encode(&lines, &|i| eols.get(&i).copied().unwrap_or(base), &format).ok())
        }
        recovery::Body::Text(_) => None,
    };
    if on_disk.is_some() && on_disk == kept {
        return ("The file on disk already holds exactly this.".to_string(), false);
    }
    if meta.file_stamp.is_some() && now != meta.file_stamp {
        return ("The file on disk has changed since these edits. Saving them would replace that version.".to_string(), true);
    }
    (String::new(), true)
}

/// Where copies of unsaved edits are kept.
fn recovery_root() -> Option<PathBuf> {
    if let Some(path) = std::env::var_os("LIMONITER_SETTINGS") {
        let mut folder = path;
        folder.push(".recovery");
        return Some(PathBuf::from(folder));
    }
    settings::data_folder().map(|dir| dir.join("recovery"))
}

/// The history and favourites menus beside the find box.
fn find_lists(ui: &mut Ui, find: &mut FindState) {
    ui.menu_button("Favourites", |ui| {
        ui.set_min_width(220.0);
        if ui.add_enabled(!find.find.is_empty(), Button::new("Add this search").frame_when_inactive(false)).clicked() {
            note_search(&mut find.favourites, &find.find.clone());
        }
        if !find.favourites.is_empty() {
            ui.separator();
        }
        let mut remove = None;
        for (index, text) in find.favourites.iter().enumerate() {
            ui.horizontal(|ui| {
                if ui.small_button("\u{D7}").on_hover_text("Remove from favourites").clicked() {
                    remove = Some(index);
                }
                if ui.add(Button::new(text.as_str()).frame_when_inactive(false)).clicked() {
                    find.find.clone_from(text);
                }
            });
        }
        if let Some(index) = remove {
            find.favourites.remove(index);
        }
    });
    ui.menu_button("History", |ui| {
        ui.set_min_width(220.0);
        if find.history.is_empty() {
            ui.label(RichText::new("Earlier searches will be listed here.").color(theme::text_dim()));
        }
        for text in &find.history {
            if ui.add(Button::new(text.as_str()).frame_when_inactive(false)).clicked() {
                find.find.clone_from(text);
            }
        }
    });
}

/// Keeps column counting on one document's tokens.
fn charset_widths(format: &TextFormat) {
    text_buffer::set_cell_profile(format.charset);
}

/// Whether a file in this format can be left on disk and read a block at a time.
fn readable_a_block_at_a_time(format: &TextFormat) -> bool {
    // EBCDIC's line endings aren't the bytes a block-at-a-time read splits on.
    format.newline == NewlinePolicy::Standard && !format.charset.keep_bytes && !format.charset.ebcdic && (format.charset.id == "utf-8" || !format.charset.holds_characters())
}

/// A size as it's easiest to read: 812 bytes, 25.3 KB, 1.2 GB.
fn short_size(bytes: u64) -> String {
    const KB: f64 = 1024.0;
    let value = bytes as f64;
    match value {
        _ if value >= KB * KB * KB => format!("{:.1} GB", value / (KB * KB * KB)),
        _ if value >= KB * KB => format!("{:.1} MB", value / (KB * KB)),
        _ if value >= KB => format!("{:.1} KB", value / KB),
        _ => format!("{bytes} byte{}", if bytes == 1 { "" } else { "s" }),
    }
}

fn eol_status_name(format: &TextFormat, counts: &codec::EolCounts, insert: Eol) -> String {
    if format.newline.is_rows() && !format.charset.holds_characters() {
        format!("{} col rows", if let NewlinePolicy::FixedRows { width, .. } = format.newline { width } else { 0 })
    } else if counts.is_mixed() {
        "Mixed".to_string()
    } else {
        insert_or_found(counts, insert).name().to_string()
    }
}

fn insert_or_found(counts: &codec::EolCounts, insert: Eol) -> Eol {
    counts.predominant(insert)
}

fn profile_status_name(format: &TextFormat) -> String {
    if format.display != Display::Plain {
        return format.display.name().to_string();
    }
    format.charset.group.clone()
}

/// What to say when a search finds nothing.
fn search_not_found(pattern: &str) -> String {
    format!("Search string '{pattern}' not found.")
}

/// How a language's comments are marked, for Reindent, which only suits languages that go by brackets.
fn reindent_marks(language: &str) -> Result<reformat::Marks<'static>, String> {
    let no_marks = reformat::Marks::default();
    match language {
        "No highlighting" | "JSON" => return Ok(no_marks),
        "HTML" | "XML" | "SVG" => return Err("Reindent goes by brackets, and this goes by tags. Use Format, Lay out code, HTML or XML.".to_string()),
        _ => {}
    }
    match tools::comment_marks(language) {
        Some(marks) if marks.line == Some("//") || language == "CSS" => Ok(reformat::Marks { line: marks.line, block: marks.block, preprocessor: matches!(language, "C" | "C++" | "C#" | "Objective-C" | "Objective-C++") }),
        _ => Err(format!("Reindent goes by brackets, which doesn't suit {language}. Its indents could mean something, or have nothing to go by.")),
    }
}

/// What's said when a backup or restore finishes.
fn project_summary(result: &Result<project::Report, String>, restoring: bool) -> String {
    const MOST_LISTED: usize = 12;
    let report = match result {
        Ok(report) => report,
        Err(problem) => return if restoring { format!("Nothing was restored, because a backup of how things are now couldn't be made first.\n\n{problem}") } else { problem.clone() },
    };
    let files = |count: usize| if count == 1 { "1 file".to_string() } else { format!("{count} files") };
    let mut lines = vec![if restoring { format!("Put back {}.", files(report.done)) } else { format!("Copied {} to {}.", files(report.done), report.folder.display()) }];
    if restoring {
        lines.push("How they were before is kept in a backup ending in before-restore.".to_string());
    }
    if !report.skipped.is_empty() {
        lines.push(String::new());
        lines.push(format!("{} left out:", files(report.skipped.len())));
        for (path, reason) in report.skipped.iter().take(MOST_LISTED) {
            lines.push(format!("{}: {reason}", path.display()));
        }
        if report.skipped.len() > MOST_LISTED {
            lines.push(format!("and {} more", report.skipped.len() - MOST_LISTED));
        }
    }
    lines.join("\n")
}

fn delimiter_choice(ui: &mut Ui, delimiter: &mut char) {
    ui.horizontal(|ui| {
        ui.label("Delimiter:");
        for (label, value) in columns::DELIMITERS {
            ui.radio_value(delimiter, value, label);
        }
    });
}

/// Draws both panes of a split window, with a divider between them that can be dragged.
#[allow(clippy::too_many_arguments)]
fn draw_split(ui: &mut Ui, text: &mut TextBuffer, first: &mut ViewState, split: &mut Split, clip: &mut ClipState, filter: Option<views::Shown<'_>>, folded: bool, view: ViewOptions, input_on: bool) -> Option<ContextCommand> {
    const GAP: f32 = 5.0;
    let whole = ui.available_rect_before_wrap();
    let (a, gap, b) = if split.vertical {
        let x = whole.min.x + ((whole.width() - GAP) * split.share).round();
        (egui::Rect::from_min_max(whole.min, egui::pos2(x, whole.max.y)), egui::Rect::from_min_max(egui::pos2(x, whole.min.y), egui::pos2(x + GAP, whole.max.y)), egui::Rect::from_min_max(egui::pos2(x + GAP, whole.min.y), whole.max))
    } else {
        let y = whole.min.y + ((whole.height() - GAP) * split.share).round();
        (egui::Rect::from_min_max(whole.min, egui::pos2(whole.max.x, y)), egui::Rect::from_min_max(egui::pos2(whole.min.x, y), egui::pos2(whole.max.x, y + GAP)), egui::Rect::from_min_max(egui::pos2(whole.min.x, y + GAP), whole.max))
    };
    let cursor = if split.vertical { egui::CursorIcon::ResizeHorizontal } else { egui::CursorIcon::ResizeVertical };
    let handle = ui.interact(gap, egui::Id::unique("split-divider"), egui::Sense::drag()).on_hover_cursor(cursor);
    if handle.dragged()
        && let Some(pointer) = handle.interact_pointer_pos()
    {
        let share = if split.vertical { (pointer.x - whole.min.x) / whole.width().max(1.0) } else { (pointer.y - whole.min.y) / whole.height().max(1.0) };
        split.share = share.clamp(0.1, 0.9);
    }
    ui.painter().rect_filled(gap, 0.0, theme::border());
    // A press picks its pane before either is drawn, so that the right one follows the cursor.
    if input_on && let Some(pointer) = ui.input(|i| i.pointer.primary_pressed().then(|| i.pointer.interact_pos()).flatten()) {
        if a.contains(pointer) {
            split.active = 0;
        } else if b.contains(pointer) {
            split.active = 1;
        }
    }
    let active = split.active;
    let mut command = ui.scope_builder(egui::UiBuilder::new().max_rect(a), |ui| views::text_pane(ui, text, first, clip, view, input_on, Pane { number: 0, active: active == 0, filter, folded })).inner;
    // Both panes use the one set of colors, which is told when the text changes.
    std::mem::swap(&mut first.colours, &mut split.second.colours);
    let second = ui.scope_builder(egui::UiBuilder::new().max_rect(b), |ui| views::text_pane(ui, text, &mut split.second, clip, view, input_on, Pane { number: 1, active: active == 1, filter, folded })).inner;
    std::mem::swap(&mut first.colours, &mut split.second.colours);
    command = command.or(second);
    command
}

/// What's said when a Replace in Files finishes.
fn replace_summary(report: &ReplaceReport) -> String {
    let plural = |count: usize, one: &str, many: &str| if count == 1 { one.to_string() } else { format!("{count} {many}") };
    let mut lines = vec![format!(
        "Made {}.",
        plural(report.replaced, "1 replacement", "replacements")
    )];
    if report.files_changed > 0 {
        lines.push(format!("{} changed on disk.", plural(report.files_changed, "1 file", "files")));
    }
    if report.backups > 0 {
        lines.push(format!("{} kept beside them, ending in .bak.", plural(report.backups, "1 copy of how it was", "copies of how they were")));
    }
    if report.open_files_changed > 0 {
        lines.push(format!("{} changed in {} window, not saved yet.", plural(report.open_files_changed, "1 open file", "open files"), if report.open_files_changed == 1 { "its" } else { "their" }));
    }
    const MOST_LISTED: usize = 12;
    if !report.skipped.is_empty() {
        lines.push(String::new());
        lines.push(format!("{} left as it was:", plural(report.skipped.len(), "1 file", "files")));
        for (path, reason) in report.skipped.iter().take(MOST_LISTED) {
            lines.push(format!("{}: {reason}", path.display()));
        }
        if report.skipped.len() > MOST_LISTED {
            lines.push(format!("and {} more", report.skipped.len() - MOST_LISTED));
        }
    }
    lines.join("\n")
}

/// A picture in the preview: as large as fits, in whole pixels where it can be, its pixels the shape they were on
/// the machine's screen, with what the file says about it below. True when Save as PNG is clicked.
/// A picture whose palette cycles can be played: `cycling` is when it started, while it plays.
fn draw_picture(ui: &mut Ui, picture: &crate::pictures::Picture, texture: &mut egui::TextureHandle, cycling: &mut Option<f64>) -> bool {
    let mut save = false;
    let options = egui::TextureOptions { magnification: egui::TextureFilter::Nearest, minification: egui::TextureFilter::Linear, ..egui::TextureOptions::default() };
    let now = ui.input(|i| i.time);
    ui.horizontal(|ui| {
        let summary: Vec<&str> = picture.info.iter().filter(|(label, _)| label == "Size" || label == "Colours").map(|(_, value)| value.as_str()).collect();
        ui.label(RichText::new(summary.join(" \u{B7} ")).color(theme::text_dim()).size(12.5));
        save = ui.button("Save as PNG\u{2026}").on_hover_text("Save the picture as a PNG, pixel for pixel, with its colours as saved").clicked();
        if picture.cycling.is_some() {
            let (label, hint) = if cycling.is_some() { ("\u{23F8} Stop cycling", "Stop the colours, and show the picture as it was saved") } else { ("\u{25B6} Cycle colours", "Play the picture's colour cycling, as Deluxe Paint did") };
            if ui.button(label).on_hover_text(hint).clicked() {
                if cycling.take().is_some() {
                    texture.set(egui::ColorImage::from_rgba_unmultiplied([picture.width, picture.height], &picture.rgba), options);
                } else {
                    *cycling = Some(now);
                }
            }
        }
    });
    if let (Some(started), Some(palette)) = (*cycling, &picture.cycling) {
        let mut frame = picture.rgba.clone();
        palette.frame(now - started, &mut frame);
        texture.set(egui::ColorImage::from_rgba_unmultiplied([picture.width, picture.height], &frame), options);
        // Drawn again in time for the fastest range's next step, and no sooner than a screen refresh.
        ui.ctx().request_repaint_after(std::time::Duration::from_secs_f32((1.0 / palette.fastest().max(1.0)).max(1.0 / 60.0)));
    }
    ui.add_space(4.0);
    egui::ScrollArea::vertical().id_salt("preview-picture").auto_shrink([false, false]).show(ui, |ui| {
        let shown = egui::vec2(picture.width as f32 * picture.aspect, picture.height as f32);
        let room = egui::vec2(ui.available_width(), (ui.available_height() - 20.0).max(120.0));
        let fit = (room.x / shown.x).min(room.y / shown.y);
        let scale = if fit >= 1.0 { fit.floor() } else { fit };
        ui.vertical_centered(|ui| {
            ui.add(egui::Image::new(&*texture).fit_to_exact_size(shown * scale));
        });
        ui.add_space(6.0);
        egui::Grid::new("picture-info").num_columns(2).spacing([12.0, 3.0]).show(ui, |ui| {
            for (label, value) in &picture.info {
                ui.label(RichText::new(label).color(theme::text_dim()).size(12.5));
                ui.add(egui::Label::new(RichText::new(value).size(12.5)).wrap());
                ui.end_row();
            }
        });
    });
    save
}

fn ok_cancel(ui: &mut Ui) -> (bool, bool) {
    ui.add_space(6.0);
    ui.horizontal(|ui| (ui.button("OK").clicked(), ui.button("Cancel").clicked())).inner
}

fn file_stamp(path: &Path) -> Option<(u64, Option<std::time::SystemTime>)> {
    let meta = std::fs::metadata(path).ok()?;
    Some((meta.len(), meta.modified().ok()))
}

/// Reads one on/off setting, with a fallback if it's missing or unreadable.
fn setting_flag(settings: &str, name: &str, fallback: bool) -> bool {
    settings
        .lines()
        .filter_map(|line| line.split_once('='))
        .find(|(key, _)| key.trim() == name)
        .and_then(|(_, value)| value.trim().parse().ok())
        .unwrap_or(fallback)
}

fn preset_from_settings(settings: &str) -> &'static Preset {
    settings
        .lines()
        .filter_map(|line| line.split_once('='))
        .find(|(key, _)| key.trim() == "new_file_preset")
        .and_then(|(_, value)| codec::preset_by_id(value.trim()))
        .unwrap_or(&PRESETS[0])
}

/// Where the settings are kept between runs.
fn settings_path() -> Option<PathBuf> {
    if let Some(path) = std::env::var_os("LIMONITER_SETTINGS") {
        return Some(PathBuf::from(path));
    }
    settings::data_folder().map(|dir| dir.join("settings.txt"))
}

/// The explicit encoding choices sit beside the settings.
fn encoding_prefs_path() -> Option<PathBuf> {
    settings_path().map(|path| path.with_file_name(format!("{}.encodings", path.file_name().map_or_else(String::new, |n| n.to_string_lossy().into_owned()))))
}

/// The folder the settings file is in, where the open window says where it can be reached.
pub(crate) fn settings_folder() -> Option<PathBuf> {
    settings_path().and_then(|path| path.parent().map(Path::to_path_buf))
}

pub(crate) fn read_settings_file() -> String {
    settings_path().and_then(|path| std::fs::read_to_string(path).ok()).unwrap_or_default()
}

/// Whether a file starts like ordinary text, rather than binary data or UTF-16.
/// Whether the start of a file looks like binary data rather than text.
fn head_looks_binary(path: &Path) -> std::io::Result<bool> {
    let file = std::fs::File::open(path)?;
    let mut head = vec![0u8; 65536];
    let got = crate::byte_store::read_full_at(&file, &mut head, 0)?;
    Ok(codec::looks_binary(&head[..got]))
}

fn is_plain_text_start(path: &Path) -> std::io::Result<bool> {
    use std::io::Read;
    let mut head = vec![0u8; 65536];
    let mut file = std::fs::File::open(path)?;
    let mut filled = 0;
    while filled < head.len() {
        let got = file.read(&mut head[filled..])?;
        if got == 0 {
            break;
        }
        filled += got;
    }
    let head = &head[..filled];
    let found = codec::detect(head);
    let single_or_utf8 = (found.charset.id == "utf-8" || !found.charset.holds_characters()) && !found.charset.ebcdic;
    Ok(!codec::looks_binary(head) && single_or_utf8)
}

pub(crate) fn same_path(a: &Path, b: &Path) -> bool {
    a.as_os_str().eq_ignore_ascii_case(b.as_os_str())
}

fn warn(message: &str) {
    MessageDialog::new().set_title("Limoniter").set_level(MessageLevel::Warning).set_description(message).set_buttons(MessageButtons::Ok).show();
}

/// The kinds of file the Open box offers to show, all files first. A Mac's Open box has no list of kinds, so
/// there it shows everything.
const OPEN_FILTERS: &[(&str, &[&str])] = &[
    ("Text files", &["txt", "text", "md", "markdown", "log", "ini", "cfg", "conf", "nfo", "diz", "me", "1st", "doc", "asc"]),
    ("CSV and TSV data", &["csv", "tsv", "tab", "psv"]),
    ("Source code", &["c", "h", "cpp", "hpp", "cs", "rs", "py", "js", "ts", "java", "go", "html", "htm", "css", "json", "xml", "yaml", "yml", "toml", "sql", "sh", "bat", "cmd", "ps1", "asm", "s", "a65", "z80", "pas", "cob", "cbl", "for", "f", "rexx", "jcl", "lua", "pl", "rb", "php"]),
    ("BASIC programs", &["prg", "p00", "bas", "bbc", "tap"]),
    ("ANSI art, PETSCII and teletext", &["ans", "ice", "diz", "nfo", "xb", "pcb", "avt", "seq", "ep1", "tti", "ttix"]),
    ("Pictures", &["iff", "ilbm", "lbm", "bbm", "pic", "scr", "brush", "pcd", "koa", "kla", "art", "ocp", "dd", "pi1", "pi2", "pi3", "pc1", "pc2", "pc3", "neo", "pntg", "pnt", "mac", "ff9", "spr", "pcx", "pcc", "img", "ximg", "hgr"]),
    ("Disk, tape and archive images", &crate::media::IMAGE_ENDINGS),
];

fn with_open_filters(dialog: rfd::FileDialog) -> rfd::FileDialog {
    if cfg!(target_os = "macos") {
        return dialog;
    }
    OPEN_FILTERS.iter().fold(dialog.add_filter("All files", &["*"]), |dialog, (name, endings)| dialog.add_filter(*name, endings))
}

fn ask_yes_no(title: &str, question: &str) -> bool {
    let answer = MessageDialog::new().set_title(title).set_level(MessageLevel::Warning).set_description(question).set_buttons(MessageButtons::YesNo).show();
    answer == MessageDialogResult::Yes
}

/// Uses Consolas for the editor where Windows has it; egui's own font stays as the fallback.
fn install_fonts(ctx: &Context, chosen: Option<&Path>) {
    let mut fonts = egui::FontDefinitions::default();
    let wanted = chosen.map_or_else(|| PathBuf::from(r"C:\Windows\Fonts\consola.ttf"), Path::to_path_buf);
    if let Ok(bytes) = std::fs::read(&wanted) {
        fonts.font_data.insert("consolas".into(), Arc::new(egui::FontData::from_owned(bytes)));
        fonts.families.entry(egui::FontFamily::Monospace).or_default().insert(0, "consolas".into());
    }
    // Windows' own typeface for the menus and labels, which reads better than the built-in one.
    if let Ok(bytes) = std::fs::read(r"C:\Windows\Fonts\segoeui.ttf") {
        fonts.font_data.insert("segoe-ui".into(), Arc::new(egui::FontData::from_owned(bytes)));
        fonts.families.entry(egui::FontFamily::Proportional).or_default().insert(0, "segoe-ui".into());
    }
    // Retro graphics, cut down from GNU Unifont to the characters in the data file's tables.
    for (name, bytes) in [("legacy-bmp", &include_bytes!("../data/legacy-bmp.otf")[..]), ("legacy-upper", &include_bytes!("../data/legacy-upper.otf")[..])] {
        fonts.font_data.insert(name.into(), Arc::new(egui::FontData::from_static(bytes)));
        for family in [egui::FontFamily::Monospace, egui::FontFamily::Proportional] {
            fonts.families.entry(family).or_default().push(name.into());
        }
    }
    preview::add_bold_font(&mut fonts);
    ctx.set_fonts(fonts);
}

#[cfg(all(test, windows))]
#[path = "big_log_tests.rs"]
mod big_log_tests;

#[path = "script_host.rs"]
mod script_host;

#[path = "addon_host.rs"]
mod addon_host;

#[path = "remote_host.rs"]
mod remote_host;

#[path = "deploy_host.rs"]
mod deploy_host;

#[path = "run_host.rs"]
mod run_host;

#[path = "asm_host.rs"]
mod asm_host;

#[path = "zx_host.rs"]
mod zx_host;

#[path = "new_kinds.rs"]
mod new_kinds;

#[path = "intel_host.rs"]
mod intel_host;

#[path = "disasm_host.rs"]
mod disasm_host;

#[path = "pixel_host.rs"]
mod pixel_host;

#[path = "lsp_host.rs"]
mod lsp_host;

#[path = "tasks_host.rs"]
mod tasks_host;

#[path = "live_host.rs"]
mod live_host;

#[path = "git_host.rs"]
mod git_host;

#[path = "git_wizard.rs"]
mod git_wizard;

#[cfg(test)]
#[path = "git_host_tests.rs"]
mod git_host_tests;

#[cfg(test)]
#[path = "run_host_tests.rs"]
mod run_host_tests;

#[path = "table_view.rs"]
mod table_view;

#[path = "theme_host.rs"]
mod theme_host;

#[cfg(test)]
#[path = "large_file_tests.rs"]
mod large_file_tests;

#[cfg(test)]
#[path = "autosave_tests.rs"]
mod autosave_tests;

#[path = "menus.rs"]
mod menus;

#[path = "edit_tools.rs"]
mod edit_tools;

#[path = "file_tools.rs"]
mod file_tools;

#[path = "region_panel.rs"]
mod region_panel;

#[path = "color_tags.rs"]
mod color_tags;

#[path = "assistant.rs"]
mod assistant;

#[path = "assistant_log.rs"]
mod assistant_log;

#[cfg(test)]
#[path = "region_panel_tests.rs"]
mod region_panel_tests;

#[cfg(test)]
#[path = "file_tools_tests.rs"]
mod file_tools_tests;

#[path = "image_browser.rs"]
mod image_browser;

#[cfg(test)]
#[path = "edit_tools_tests.rs"]
mod edit_tools_tests;

#[cfg(test)]
#[path = "script_tests.rs"]
mod script_tests;

#[cfg(test)]
mod tests {
    use super::*;

    /// Each call has a folder of its own, so tests running at the same time never share a file.
    fn temp_file(tag: &str, data: &[u8]) -> PathBuf {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let dir = std::env::temp_dir().join(format!("limoniter-app-{tag}-{}-{}", std::process::id(), NEXT.fetch_add(1, Ordering::Relaxed)));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("file.txt");
        std::fs::write(&path, data).unwrap();
        path
    }

    /// As a crash would: the snapshot goes to a file, and only that file is read back.
    fn through_a_copy(doc: &Document) -> (recovery::Meta, recovery::Body) {
        let root = std::env::temp_dir().join(format!("limoniter-app-copy-{}-{}", std::process::id(), doc.id));
        let _ = std::fs::remove_dir_all(&root);
        let mut session = recovery::Session::start(&root).unwrap();
        session.write_now(&doc.snapshot()).unwrap();
        session.abandon();
        recovery::wait_until_let_go(&root);
        let (orphans, found) = recovery::find_orphans(&root);
        let outcome = found.into_iter().next().unwrap().outcome.unwrap();
        drop(orphans);
        std::fs::remove_dir_all(&root).unwrap();
        outcome
    }

    fn wait_for_scan(doc: &mut Document) {
        if let Some(source) = doc.scanning.take() {
            while !source.is_finished() {
                std::thread::sleep(Duration::from_millis(1));
            }
            doc.text.refresh_from_source();
        }
    }

    #[test]
    fn a_recovered_document_has_its_edits_settings_and_place_and_leaves_the_file_alone() {
        let original = b"caf\x82\r\nkeep \x81\nlast\x1A";
        let path = temp_file("text", original);
        let format = TextFormat { charset: charset::known("cp437"), ..TextFormat::default() };
        let mut doc = Document::open(&path, Some(format)).unwrap();
        doc.text.move_caret(1, 2, false);
        doc.text.type_text("XY");
        doc.text.toggle_bookmark(1);
        let edited = doc.text.encode(&doc.text.format, Lossy::Refuse).unwrap();

        let (meta, body) = through_a_copy(&doc);
        let back = Document::from_recovered(meta, body).unwrap();
        assert_eq!(back.text.encode(&back.text.format, Lossy::Refuse).unwrap(), edited);
        assert_eq!(back.text.format.charset.id, "cp437");
        assert_eq!(back.path.as_deref(), Some(path.as_path()));
        assert!(back.is_modified(), "it comes back as unsaved changes");
        assert_eq!(back.text.caret_char_position(), doc.text.caret_char_position());
        assert!(back.text.is_bookmarked(1));
        assert_eq!(std::fs::read(&path).unwrap(), original, "the file on disk hasn't been touched");
        std::fs::remove_dir_all(path.parent().unwrap()).unwrap();
    }

    #[test]
    fn untitled_text_comes_back_too() {
        let mut doc = Document::untitled("Edit3".into(), &PRESETS[0]);
        doc.text.type_text("never saved anywhere");
        let (meta, body) = through_a_copy(&doc);
        let back = Document::from_recovered(meta, body).unwrap();
        assert_eq!(back.name(), "Edit3");
        assert!(back.path.is_none());
        assert_eq!(&*back.text.line(0), "never saved anywhere");
        assert!(back.is_modified());
    }

    /// The whole app, with no window. A screenshot run leaves settings and recovery alone, so nothing of the person's is touched.
    #[test]
    fn the_open_box_offers_kinds_of_file_including_csv() {
        let csv = OPEN_FILTERS.iter().find(|(name, _)| name.contains("CSV")).expect("CSV and TSV are offered");
        assert!(csv.1.contains(&"csv") && csv.1.contains(&"tsv"));
        for (name, endings) in OPEN_FILTERS {
            assert!(!endings.is_empty(), "{name}");
            for (i, ending) in endings.iter().enumerate() {
                assert!(ending.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit()), "{name}: {ending}");
                assert!(!endings[..i].contains(ending), "{name} lists {ending} twice");
            }
        }
    }

    pub(super) fn app_with(settings: &str) -> (Limoniter, Context) {
        let mut app = Limoniter::build(settings.to_string(), None, Some(PathBuf::from("never-written.png")));
        app.new_document();
        (app, Context::default())
    }

    fn input(events: Vec<egui::Event>) -> egui::RawInput {
        egui::RawInput { screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1600.0, 900.0))), events, ..Default::default() }
    }

    pub(super) fn draw(app: &mut Limoniter, ctx: &Context, events: Vec<egui::Event>) -> egui::FullOutput {
        let mut output = ctx.run_ui(input(events), |ui| app.draw_frame(ui));
        output.textures_delta.clear();
        output
    }

    fn command(app: &mut Limoniter, ctx: &Context, command: Command) -> egui::FullOutput {
        let mut output = ctx.run_ui(input(Vec::new()), |ui| app.run(command, ui));
        output.textures_delta.clear();
        output
    }

    fn key(key: Key, modifiers: egui::Modifiers) -> egui::Event {
        egui::Event::Key { key, physical_key: None, pressed: true, repeat: false, modifiers: if modifiers.ctrl { modifiers | egui::Modifiers::COMMAND } else { modifiers } }
    }

    const EVERY_PANEL: &str = "file_properties=true\njson_manager=true\nxml_manager=true\nascii_table=true\nencoding_preview=true\nclipboard_history=false\nfile_explorer=true\nopen_files=true\nbookmarks=true\nfile_lists=true\nmacros_panel=true\nscripts_panel=true\nhistory_panel=true\n";

    #[test]
    fn closing_the_only_file_opens_nothing_in_its_place() {
        let (mut app, ctx) = app_with(EVERY_PANEL);
        draw(&mut app, &ctx, Vec::new());
        assert!(!app.no_file());
        app.close_tab(0);
        assert!(app.no_file());
        assert_eq!(app.untitled_count, 1, "no new Edit file was made");
        // Every tab of the ribbon, every panel, the title and the status bar all cope with there being no file.
        for menu in 0..menus::MENUS.len() {
            app.shown_menu = Some((menu, true));
            draw(&mut app, &ctx, Vec::new());
            draw(&mut app, &ctx, Vec::new());
        }
        assert_eq!(app.window_title, "Limoniter");
    }

    #[test]
    fn with_no_file_open_keys_and_commands_change_nothing() {
        let (mut app, ctx) = app_with(EVERY_PANEL);
        app.close_tab(0);
        draw(&mut app, &ctx, vec![egui::Event::Text("typed".into()), egui::Event::Paste("pasted".into())]);
        draw(&mut app, &ctx, vec![key(Key::W, egui::Modifiers::CTRL), key(Key::H, egui::Modifiers::CTRL), key(Key::Z, egui::Modifiers::CTRL), key(Key::Delete, egui::Modifiers::NONE)]);
        for each in [Command::CloseTab, Command::ToggleHex, Command::ViewEbcdic, Command::HexInsertDelete, Command::Upper, Command::GenerateGuid, Command::ToggleBookmark, Command::ToggleReadOnly, Command::InsertDate, Command::SelectAll, Command::Find, Command::GoTo] {
            command(&mut app, &ctx, each);
            assert!(matches!(app.dialog, Dialog::None));
        }
        app.find.find = "^".into();
        app.find.replace = "x".into();
        app.find.regex = true;
        app.run_replace_all();
        app.run_replace();
        app.run_find(false);
        assert_eq!(app.find.status, "No file is open.");
        for action in [Action::Paste("text".into()), Action::InsertCharacter('x'), Action::InsertBytes(vec![1]), Action::AskInsertByte, Action::GoToLine(3)] {
            let mut action = Some(action);
            let _ = ctx.run_ui(input(Vec::new()), |ui| {
                if let Some(action) = action.take() {
                    app.run_panel_action(action, ui);
                }
            });
        }
        assert!(app.no_file());
        assert!(!app.docs[0].is_modified());
        assert!(!app.docs[0].hex_mode);
        assert_eq!(app.docs[0].text.line_count(), 1);
        assert!(app.docs[0].text.line(0).is_empty());
    }

    #[test]
    fn the_string_explorer_selects_a_string_with_its_length_in_hex_mode() {
        let (mut app, ctx) = app_with("strings_panel=true\n");
        assert!(app.panels.strings, "the panel is remembered in the settings");
        // Text first: the panel offers to show the bytes.
        draw(&mut app, &ctx, Vec::new());
        let mut action = Some(Action::ViewHex);
        let mut frame = ctx.run_ui(input(Vec::new()), |ui| {
            if let Some(action) = action.take() {
                app.run_panel_action(action, ui);
            }
        });
        frame.textures_delta.clear();
        assert!(app.docs[app.active].hex_mode);
        let bytes = b"\x90\x91\x05Hello\x05World\x04Next\x92\x00\x00\x00\x01\x02\x03\x00\xFF".to_vec();
        app.docs.push(Document::from_image_file("strings.bin".into(), bytes.clone(), None));
        app.active = app.docs.len() - 1;
        assert!(app.docs[app.active].hex_mode);
        draw(&mut app, &ctx, Vec::new());
        let found = crate::strings_explorer::find(&bytes, &Default::default());
        assert_eq!((found[0].start, found[0].end), (2, 8));
        let mut action = Some(Action::SelectBytes { start: found[0].start, end: found[0].end });
        let mut frame = ctx.run_ui(input(Vec::new()), |ui| {
            if let Some(action) = action.take() {
                app.run_panel_action(action, ui);
            }
        });
        frame.textures_delta.clear();
        let hex = &app.docs[app.active].hex;
        assert_eq!((hex.anchor.min(hex.caret), hex.anchor.max(hex.caret)), (2, 8), "the length, the text and nothing more");
        draw(&mut app, &ctx, Vec::new());
    }

    #[test]
    fn a_new_file_or_an_opened_one_ends_the_empty_window() {
        let (mut app, ctx) = app_with("");
        app.close_tab(0);
        command(&mut app, &ctx, Command::New);
        assert!(!app.no_file());
        assert_eq!(app.docs.len(), 1);
        assert_eq!(app.docs[0].name(), "Edit2");
        assert_eq!(app.active, 0);
        draw(&mut app, &ctx, Vec::new());

        app.close_tab(0);
        let file = std::env::temp_dir().join(format!("limoniter-closing-{}.txt", std::process::id()));
        std::fs::write(&file, "content").unwrap();
        assert!(app.open_path(&file));
        assert!(!app.no_file());
        assert_eq!(app.docs.len(), 1);
        assert_eq!(&*app.docs[0].text.line(0), "content");
        draw(&mut app, &ctx, Vec::new());
        drop(app);
        std::fs::remove_file(&file).unwrap();
    }

    #[test]
    fn closing_one_of_several_files_keeps_the_rest() {
        let (mut app, _ctx) = app_with("");
        app.new_document();
        app.close_tab(0);
        assert!(!app.no_file());
        assert_eq!(app.docs.len(), 1);
        assert_eq!(app.docs[0].name(), "Edit2");
    }

    #[test]
    fn the_hex_menu_moves_between_regular_hex_and_ebcdic_and_keeps_the_file() {
        let (mut app, ctx) = app_with("");
        app.doc().text.type_text("ABC");
        command(&mut app, &ctx, Command::ViewEbcdic);
        assert!(app.docs[0].hex_mode && app.docs[0].hex.ebcdic);
        assert_eq!(app.docs[0].hex.bytes(), b"ABC", "reading it as EBCDIC doesn't change it");
        assert!(app.docs[0].is_modified());
        draw(&mut app, &ctx, Vec::new());
        command(&mut app, &ctx, Command::ViewHex);
        assert!(app.docs[0].hex_mode && !app.docs[0].hex.ebcdic);
        command(&mut app, &ctx, Command::ViewEbcdic);
        command(&mut app, &ctx, Command::ViewRegular);
        assert!(!app.docs[0].hex_mode);
        assert_eq!(&*app.docs[0].text.line(0), "ABC");
        command(&mut app, &ctx, Command::ToggleHex);
        assert!(app.docs[0].hex_mode && !app.docs[0].hex.ebcdic, "the shortcut goes to plain hex");
        command(&mut app, &ctx, Command::ViewRegular);
        command(&mut app, &ctx, Command::ViewRegular);
        assert!(!app.docs[0].hex_mode, "asking for the mode it's already in changes nothing");
    }

    #[test]
    fn insert_delete_runs_through_its_dialog() {
        let (mut app, ctx) = app_with("");
        command(&mut app, &ctx, Command::ViewHex);
        app.doc().hex.load(vec![1, 2, 3, 4]);
        app.doc().hex.move_caret(1, false);
        command(&mut app, &ctx, Command::HexInsertDelete);
        assert!(matches!(&app.dialog, Dialog::HexInsertDelete { count, fill, delete: false, .. } if count == "1" && fill == "00"));
        draw(&mut app, &ctx, Vec::new());

        let enter = |app: &mut Limoniter| {
            draw(app, &ctx, vec![key(Key::Enter, egui::Modifiers::NONE)]);
        };
        app.dialog = Dialog::HexInsertDelete { count: "2".into(), delete: false, fill: "ZZ".into(), error: String::new(), focus: false };
        enter(&mut app);
        assert!(matches!(&app.dialog, Dialog::HexInsertDelete { error, .. } if !error.is_empty()), "a bad value keeps the dialog open");
        assert_eq!(app.docs[0].hex.bytes(), [1, 2, 3, 4]);

        app.dialog = Dialog::HexInsertDelete { count: "2".into(), delete: false, fill: "AB".into(), error: String::new(), focus: false };
        enter(&mut app);
        assert!(matches!(app.dialog, Dialog::None));
        assert_eq!(app.docs[0].hex.bytes(), [1, 0xAB, 0xAB, 2, 3, 4]);

        app.dialog = Dialog::HexInsertDelete { count: "2".into(), delete: true, fill: "00".into(), error: String::new(), focus: false };
        enter(&mut app);
        assert_eq!(app.docs[0].hex.bytes(), [1, 2, 3, 4]);

        app.doc().hex.move_caret(4, false);
        app.dialog = Dialog::HexInsertDelete { count: "1".into(), delete: true, fill: "00".into(), error: String::new(), focus: false };
        enter(&mut app);
        assert!(matches!(&app.dialog, Dialog::HexInsertDelete { error, .. } if error.contains("no bytes")));
        assert_eq!(app.docs[0].hex.bytes(), [1, 2, 3, 4]);
    }

    #[test]
    fn insert_delete_offers_the_selection_size_and_respects_read_only() {
        let (mut app, ctx) = app_with("");
        command(&mut app, &ctx, Command::ViewHex);
        app.doc().hex.load(vec![0; 40]);
        app.doc().hex.move_caret(5, false);
        app.doc().hex.move_caret(30, true);
        command(&mut app, &ctx, Command::HexInsertDelete);
        assert!(matches!(&app.dialog, Dialog::HexInsertDelete { count, .. } if count == "25"));
        app.dialog = Dialog::None;
        app.doc().set_read_only(true);
        command(&mut app, &ctx, Command::HexInsertDelete);
        assert!(!matches!(app.dialog, Dialog::HexInsertDelete { .. }));
    }

    #[test]
    fn copy_selected_puts_the_laid_out_rows_on_the_clipboard_in_the_pane_in_use() {
        let (mut app, ctx) = app_with("");
        command(&mut app, &ctx, Command::ViewEbcdic);
        app.doc().hex.load(vec![0xC1, 0xC2, 0x00]);
        app.doc().hex.select_all();
        let output = command(&mut app, &ctx, Command::HexCopySelected);
        let copied = output.platform_output.commands.iter().find_map(|each| if let egui::OutputCommand::CopyText(text) = each { Some(text.clone()) } else { None });
        assert_eq!(copied.as_deref(), Some(format!("{:<59}| AB.", "00000000  C1 C2 00").as_str()));
    }

    /// Presses and lets go of the left mouse button, over the frames a real click takes.
    fn click_at(at: egui::Pos2) -> [Vec<egui::Event>; 3] {
        let button = |pressed| egui::Event::PointerButton { pos: at, button: egui::PointerButton::Primary, pressed, modifiers: egui::Modifiers::NONE };
        [vec![egui::Event::PointerMoved(at)], vec![button(true)], vec![button(false)]]
    }

    fn press_enter(app: &mut Limoniter, ctx: &Context) -> egui::FullOutput {
        draw(app, ctx, vec![key(Key::Enter, egui::Modifiers::NONE)])
    }

    fn column_app(lines: &str) -> (Limoniter, Context) {
        let (mut app, ctx) = app_with("");
        app.doc().text.type_text(lines);
        app.doc().text.set_column_mode(true);
        app.doc().text.move_caret(0, 1, false);
        (app, ctx)
    }

    fn lines_of(app: &Limoniter) -> Vec<String> {
        app.docs[0].text.all_lines().iter().map(|line| line.to_string()).collect()
    }

    fn copied(output: &egui::FullOutput) -> Option<String> {
        output.platform_output.commands.iter().find_map(|each| if let egui::OutputCommand::CopyText(text) = each { Some(text.clone()) } else { None })
    }

    #[test]
    fn cut_columns_goes_through_its_dialog_and_onto_the_clipboard() {
        let (mut app, ctx) = column_app("abcd\nefgh");
        command(&mut app, &ctx, Command::ColumnCut);
        assert!(matches!(&app.dialog, Dialog::ColumnRemove { cut: true, count, .. } if count == "0"));
        press_enter(&mut app, &ctx);
        assert!(matches!(&app.dialog, Dialog::ColumnRemove { error, .. } if !error.is_empty()), "0 with nothing highlighted is explained");
        assert_eq!(lines_of(&app), ["abcd", "efgh"]);
        app.dialog = Dialog::ColumnRemove { cut: true, count: "2".into(), error: String::new(), focus: false };
        let output = press_enter(&mut app, &ctx);
        assert!(matches!(app.dialog, Dialog::None));
        assert_eq!(lines_of(&app), ["ad", "eh"]);
        assert_eq!(copied(&output).as_deref(), Some("bc\r\nfg"));
        assert!(app.clip.is_block("bc\r\nfg"), "so it pastes back as columns");
    }

    #[test]
    fn delete_columns_leaves_the_clipboard_alone_and_rejects_a_bad_number() {
        let (mut app, ctx) = column_app("abcd\nefgh");
        command(&mut app, &ctx, Command::ColumnDelete);
        app.dialog = Dialog::ColumnRemove { cut: false, count: "lots".into(), error: String::new(), focus: false };
        press_enter(&mut app, &ctx);
        assert!(matches!(&app.dialog, Dialog::ColumnRemove { error, .. } if error.contains("whole number")));
        app.dialog = Dialog::ColumnRemove { cut: false, count: "1".into(), error: String::new(), focus: false };
        let output = press_enter(&mut app, &ctx);
        assert_eq!(lines_of(&app), ["acd", "egh"]);
        assert_eq!(copied(&output), None);
    }

    #[test]
    fn fill_remembers_the_empty_last_line_answer() {
        let (mut app, ctx) = column_app("ab\ncd\n");
        command(&mut app, &ctx, Command::ColumnFill);
        assert!(matches!(&app.dialog, Dialog::Fill { skip_last: true, .. }), "on to begin with");
        app.dialog = Dialog::Fill { value: "+".into(), skip_last: true };
        press_enter(&mut app, &ctx);
        assert_eq!(lines_of(&app), ["a+b", "c+d", ""]);
        app.doc().text.move_caret(0, 0, false);
        app.dialog = Dialog::Fill { value: "-".into(), skip_last: false };
        press_enter(&mut app, &ctx);
        assert_eq!(lines_of(&app), ["-a+b", "-c+d", "-"]);
        command(&mut app, &ctx, Command::ColumnFill);
        assert!(matches!(&app.dialog, Dialog::Fill { skip_last: false, .. }));
    }

    #[test]
    fn csv_convert_goes_both_ways_through_its_dialogs_and_undoes() {
        let (mut app, ctx) = app_with("");
        app.doc().text.type_text("name;qty\nBo;12\n");
        command(&mut app, &ctx, Command::CsvToFixed);
        assert!(matches!(&app.dialog, Dialog::CsvToFixed { rows: (0, 1), delimiter: ';', .. }), "the file without its empty last line, and the delimiter it uses");
        press_enter(&mut app, &ctx);
        assert!(matches!(app.dialog, Dialog::None));
        assert_eq!(lines_of(&app), ["name qty", "Bo   12 ", ""]);

        command(&mut app, &ctx, Command::FixedToCsv);
        assert!(matches!(&app.dialog, Dialog::FixedToCsv { rows: (0, 1), starts, .. } if starts == "1, 6"));
        if let Dialog::FixedToCsv { delimiter, .. } = &mut app.dialog {
            *delimiter = ';';
        }
        press_enter(&mut app, &ctx);
        assert_eq!(lines_of(&app), ["name;qty", "Bo;12", ""]);

        command(&mut app, &ctx, Command::Undo);
        assert_eq!(lines_of(&app), ["name qty", "Bo   12 ", ""]);
    }

    #[test]
    fn csv_convert_reports_problems_and_changes_nothing() {
        let (mut app, ctx) = app_with("");
        app.doc().text.type_text("a,b\nc,\"d");
        let before = lines_of(&app);
        command(&mut app, &ctx, Command::CsvToFixed);
        press_enter(&mut app, &ctx);
        assert!(matches!(&app.dialog, Dialog::CsvToFixed { error, .. } if error.contains("Line 2")));
        assert_eq!(lines_of(&app), before);
        app.dialog = Dialog::FixedToCsv { rows: (0, 1), delimiter: ',', starts: "5, 3".into(), error: String::new(), focus: false };
        press_enter(&mut app, &ctx);
        assert!(matches!(&app.dialog, Dialog::FixedToCsv { error, .. } if !error.is_empty()));
        assert_eq!(lines_of(&app), before);
    }

    #[test]
    fn column_commands_respect_read_only_and_hex_mode() {
        let (mut app, ctx) = column_app("abcd\nefgh");
        app.doc().set_read_only(true);
        for each in [Command::ColumnFill, Command::ColumnCut, Command::ColumnDelete, Command::CsvToFixed, Command::FixedToCsv] {
            command(&mut app, &ctx, each);
            assert!(matches!(app.dialog, Dialog::Message { .. }), "told it's read-only");
            app.dialog = Dialog::None;
        }
        command(&mut app, &ctx, Command::JustifyRight);
        assert_eq!(lines_of(&app), ["abcd", "efgh"]);
        app.dialog = Dialog::None;
        app.doc().set_read_only(false);
        command(&mut app, &ctx, Command::ToggleHex);
        for each in [Command::ColumnFill, Command::ColumnCut, Command::ColumnDelete, Command::CsvToFixed, Command::FixedToCsv, Command::JustifyLeft] {
            command(&mut app, &ctx, each);
            assert!(matches!(app.dialog, Dialog::None), "nothing happens in hex mode");
        }
    }

    #[test]
    fn the_settings_button_opens_with_or_without_a_file_and_closes_with_escape() {
        let (mut app, ctx) = app_with("");
        command(&mut app, &ctx, Command::Settings);
        assert!(matches!(app.dialog, Dialog::Settings(_)));
        draw(&mut app, &ctx, Vec::new());
        draw(&mut app, &ctx, vec![key(Key::Escape, egui::Modifiers::NONE)]);
        assert!(matches!(app.dialog, Dialog::None));
        app.close_tab(0);
        command(&mut app, &ctx, Command::Settings);
        assert!(matches!(app.dialog, Dialog::Settings(_)), "nothing in it needs a file");
        draw(&mut app, &ctx, Vec::new());
    }

    #[test]
    fn the_theme_changes_while_running_and_is_remembered() {
        let (mut app, ctx) = app_with("");
        draw(&mut app, &ctx, Vec::new());
        assert!(theme::is_light(), "light to begin with");
        let light_back = ctx.global_style().visuals.panel_fill;
        app.themes.window = "Limoniter Dark".into();
        for menu in 0..menus::MENUS.len() {
            app.shown_menu = Some((menu, true));
            draw(&mut app, &ctx, Vec::new());
        }
        assert!(!theme::is_light());
        assert_ne!(ctx.global_style().visuals.panel_fill, light_back);
        assert_eq!(ctx.global_style().visuals.panel_fill, theme::window());
        assert!(app.current_settings().contains("window_theme=Limoniter Dark"));
        let (again, _) = app_with(&app.current_settings());
        assert_eq!(again.themes.window, "Limoniter Dark", "someone who chose dark keeps it");
        app.themes.window = "Limoniter Light".into();
        draw(&mut app, &ctx, Vec::new());
        assert!(theme::is_light());
    }

    #[test]
    fn every_menu_and_panel_draws_in_light_with_a_file_in_each_mode() {
        let (mut app, ctx) = app_with(EVERY_PANEL);
        app.themes.window = "Limoniter Light".into();
        app.doc().text.type_text("some text\nmore");
        for menu in 0..menus::MENUS.len() {
            app.shown_menu = Some((menu, true));
            draw(&mut app, &ctx, Vec::new());
            command(&mut app, &ctx, Command::ToggleHex);
            draw(&mut app, &ctx, Vec::new());
            command(&mut app, &ctx, Command::ToggleHex);
        }
        theme::set_light(false);
    }

    #[test]
    fn hex_rows_change_how_the_file_is_laid_out_without_changing_it() {
        let (mut app, ctx) = app_with("");
        command(&mut app, &ctx, Command::ToggleHex);
        app.doc().hex.load((0..=255).cycle().take(5000).collect());
        app.doc().hex.move_caret(4000, false);
        for row in settings::HEX_ROWS {
            app.prefs.hex_row = row;
            draw(&mut app, &ctx, Vec::new());
            draw(&mut app, &ctx, vec![key(Key::ArrowDown, egui::Modifiers::NONE), key(Key::End, egui::Modifiers::NONE), key(Key::Home, egui::Modifiers::NONE)]);
            assert_eq!(hex_buffer::bytes_per_row(), row);
            assert_eq!(hex_buffer::ascii_cell(row - 1) + 1, hex_buffer::ascii_cell(row));
            assert_eq!(app.docs[0].hex.len(), 5000);
        }
        hex_buffer::set_bytes_per_row(16);
    }

    #[test]
    fn zero_bytes_open_as_hex_or_as_text_as_set() {
        let path = temp_file("nulls", b"MZ\x00\x01text");
        let hex = Document::open(&path, None).unwrap();
        assert!(hex.hex_mode, "as before");
        text_buffer::set_edit_prefs(text_buffer::EditPrefs { nulls_open_in_hex: false, ..text_buffer::EditPrefs::default() });
        let text = Document::open(&path, None).unwrap();
        text_buffer::set_edit_prefs(text_buffer::EditPrefs::default());
        assert!(!text.hex_mode);
    }

    #[test]
    fn opening_find_can_start_from_the_selection_or_not() {
        let (mut app, ctx) = app_with("");
        app.doc().text.type_text("hello world");
        app.doc().text.set_selection((0, 0), (0, 5), false);
        command(&mut app, &ctx, Command::Find);
        assert_eq!(app.find.find, "hello");
        app.find.find = "kept".into();
        app.prefs.find_from_selection = false;
        command(&mut app, &ctx, Command::Find);
        assert_eq!(app.find.find, "kept");
        app.prefs.find_from_selection = true;
        app.doc().text.select_all();
        app.doc().text.type_text("a\nb");
        app.doc().text.select_all();
        command(&mut app, &ctx, Command::Find);
        assert_eq!(app.find.find, "kept", "several lines are no use in a find box");
    }

    #[test]
    fn find_in_files_can_start_in_this_files_folder() {
        let (mut app, ctx) = app_with("");
        let path = temp_file("folder", b"text");
        assert!(app.open_path(&path));
        app.find.folder = "somewhere".into();
        command(&mut app, &ctx, Command::FindInFiles);
        assert_eq!(app.find.folder, "somewhere");
        app.prefs.find_in_file_folder = true;
        command(&mut app, &ctx, Command::FindInFiles);
        assert_eq!(PathBuf::from(&app.find.folder), path.parent().unwrap());
    }

    #[test]
    fn copying_and_cutting_with_nothing_selected_take_the_line_only_when_set() {
        let (mut app, ctx) = app_with("");
        app.doc().text.type_text("one\ntwo\n");
        app.doc().text.move_caret(1, 1, false);
        assert_eq!(copied(&command(&mut app, &ctx, Command::Copy)), None);
        app.prefs.edit.whole_line_copy = true;
        draw(&mut app, &ctx, Vec::new());
        assert_eq!(copied(&command(&mut app, &ctx, Command::Copy)).as_deref(), Some("two\r\n"));
        assert!(!app.docs[0].text.has_selection(), "Copy leaves the cursor as it was");
        assert_eq!(lines_of(&app), ["one", "two", ""]);
        assert_eq!(copied(&command(&mut app, &ctx, Command::Cut)).as_deref(), Some("two\r\n"));
        assert_eq!(lines_of(&app), ["one", ""]);
        command(&mut app, &ctx, Command::Undo);
        assert_eq!(lines_of(&app), ["one", "two", ""]);
        text_buffer::set_edit_prefs(text_buffer::EditPrefs::default());
    }

    #[test]
    fn word_wrap_from_settings_also_ends_column_mode() {
        let (mut app, ctx) = column_app("abc");
        command(&mut app, &ctx, Command::Settings);
        // The window sees the box change as it draws; this is what it does next.
        let before = app.view.word_wrap;
        app.toggle_word_wrap();
        assert_ne!(app.view.word_wrap, before);
        assert!(!app.docs[0].text.column_mode());
    }

    #[test]
    fn last_and_next_change_work_from_the_commands_and_the_marks_can_be_turned_off() {
        let (mut app, ctx) = app_with("");
        app.doc().text.set_content(vec!["0".into(), "1".into(), "2".into()]);
        app.doc().text.move_caret(1, 0, false);
        app.doc().text.type_text("x");
        app.doc().text.move_caret(0, 0, false);
        command(&mut app, &ctx, Command::NextChange);
        assert_eq!(app.docs[0].text.caret_line, 1);
        command(&mut app, &ctx, Command::LastChange);
        assert_eq!(app.docs[0].text.caret_line, 1, "the only one there is");
        draw(&mut app, &ctx, Vec::new());
        app.view.line_change = false;
        draw(&mut app, &ctx, Vec::new());
    }

    #[test]
    fn a_save_turns_the_change_marks_green() {
        let (mut app, ctx) = app_with("");
        let path = temp_file("marks", b"one\ntwo\n");
        assert!(app.open_path(&path));
        app.doc().text.type_text("x");
        assert_eq!(app.docs[app.active].text.change_at(0), Some(false));
        command(&mut app, &ctx, Command::Save);
        assert_eq!(app.docs[app.active].text.change_at(0), Some(true));
        assert!(!app.docs[app.active].is_modified());
    }

    fn wait_for_results(app: &mut Limoniter) {
        let job = app.job.clone().unwrap();
        while !job.finished.load(Ordering::SeqCst) {
            std::thread::sleep(Duration::from_millis(5));
        }
    }

    fn wait_for_replace(app: &mut Limoniter, ctx: &Context) {
        while app.replace_job.is_some() {
            std::thread::sleep(Duration::from_millis(5));
            draw(app, ctx, Vec::new());
        }
    }

    fn folder_app(tag: &str, what: &str, with: &str) -> (Limoniter, Context, PathBuf) {
        let dir = std::env::temp_dir().join(format!("limoniter-replace-app-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let (mut app, ctx) = app_with("");
        app.find.folder = dir.display().to_string();
        app.find.file_types = "*.txt".into();
        app.find.find = what.into();
        app.find.replace = with.into();
        (app, ctx, dir)
    }

    #[test]
    fn replace_in_files_previews_first_and_changes_nothing_until_told() {
        let (mut app, ctx, dir) = folder_app("preview", "cat", "dog");
        app.find.keep_backups = false;
        std::fs::write(dir.join("a.txt"), "a cat\n").unwrap();
        std::fs::write(dir.join("b.txt"), "cat cat\n").unwrap();
        std::fs::write(dir.join("c.log"), "cat\n").unwrap();
        app.start_find_in_files(true);
        wait_for_results(&mut app);
        assert!(app.replace_plan.is_some());
        assert_eq!(std::fs::read(dir.join("a.txt")).unwrap(), b"a cat\n", "a preview changes nothing");

        let mut files: Vec<PathBuf> = app.job.as_ref().unwrap().hits.lock().unwrap().iter().map(|hit| hit.path.clone()).collect();
        files.dedup();
        assert_eq!(files.len(), 2, "only the files asked for");
        app.dialog = Dialog::ConfirmReplaceInFiles { files: files.clone(), matches: 3 };
        draw(&mut app, &ctx, Vec::new());
        draw(&mut app, &ctx, vec![key(Key::Escape, egui::Modifiers::NONE)]);
        assert!(matches!(app.dialog, Dialog::None));
        assert_eq!(std::fs::read(dir.join("a.txt")).unwrap(), b"a cat\n", "cancelling changes nothing");
        assert!(app.replace_plan.is_some(), "and the plan is still there to go ahead with");

        app.start_replace_in_files(files);
        wait_for_replace(&mut app, &ctx);
        assert_eq!(std::fs::read(dir.join("a.txt")).unwrap(), b"a dog\n");
        assert_eq!(std::fs::read(dir.join("b.txt")).unwrap(), b"dog dog\n");
        assert_eq!(std::fs::read(dir.join("c.log")).unwrap(), b"cat\n");
        assert!(matches!(&app.dialog, Dialog::Message { body, .. } if body.contains("3 replacements") && body.contains("2 files")), "the result is reported");
        assert!(app.replace_plan.is_none(), "it can't be done twice by accident");
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn open_files_change_in_their_windows_and_stay_undoable() {
        let (mut app, ctx, dir) = folder_app("open", "cat", "dog");
        let open = dir.join("open.txt");
        std::fs::write(&open, "cat\n").unwrap();
        std::fs::write(dir.join("shut.txt"), "cat\n").unwrap();
        assert!(app.open_path(&open));
        app.start_find_in_files(true);
        wait_for_results(&mut app);
        let mut files: Vec<PathBuf> = app.job.as_ref().unwrap().hits.lock().unwrap().iter().map(|hit| hit.path.clone()).collect();
        files.dedup();
        app.start_replace_in_files(files);
        wait_for_replace(&mut app, &ctx);
        assert_eq!(std::fs::read(&open).unwrap(), b"cat\n", "the open file's own copy on disk stays until it's saved");
        let index = app.doc_for_path(&open).unwrap();
        assert_eq!(&*app.docs[index].text.line(0), "dog");
        assert!(app.docs[index].is_modified());
        app.docs[index].text.undo();
        assert_eq!(&*app.docs[index].text.line(0), "cat");
        assert_eq!(std::fs::read(dir.join("shut.txt")).unwrap(), b"dog\n");
        assert_eq!(std::fs::read(dir.join("shut.txt.bak")).unwrap(), b"cat\n", "backups are on to begin with");
        assert!(matches!(&app.dialog, Dialog::Message { body, .. } if body.contains("not saved yet")));
        drop(app);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn open_files_that_cant_be_changed_are_left_and_reported() {
        let (mut app, ctx, dir) = folder_app("locked", "cat", "dog");
        let path = dir.join("a.txt");
        std::fs::write(&path, "cat\n").unwrap();
        assert!(app.open_path(&path));
        app.doc().set_read_only(true);
        app.replace_plan = Some(ReplacePlan { options: SearchOptions { pattern: "cat".into(), replacement: "dog".into(), ..SearchOptions::default() }, backup: false });
        app.start_replace_in_files(vec![path.clone()]);
        wait_for_replace(&mut app, &ctx);
        assert_eq!(&*app.docs[app.active].text.line(0), "cat");
        assert_eq!(std::fs::read(&path).unwrap(), b"cat\n");
        assert!(matches!(&app.dialog, Dialog::Message { body, .. } if body.contains("read-only")));
        drop(app);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn the_replace_tab_and_results_draw_with_each_place_to_search() {
        let (mut app, ctx, dir) = folder_app("draw", "cat", "dog");
        std::fs::write(dir.join("a.txt"), "cat\n").unwrap();
        app.find.open = true;
        app.find.tab = FindTab::ReplaceInFiles;
        for scope in [FileScope::Folder, FileScope::OpenFiles, FileScope::Favorites] {
            app.find.scope = scope;
            draw(&mut app, &ctx, Vec::new());
        }
        app.find.scope = FileScope::Folder;
        app.start_find_in_files(true);
        wait_for_results(&mut app);
        draw(&mut app, &ctx, Vec::new());
        draw(&mut app, &ctx, Vec::new());
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn favorite_files_are_searched_and_remembered() {
        let (mut app, _ctx, dir) = folder_app("favorites", "needle", "");
        std::fs::write(dir.join("a.txt"), "needle\n").unwrap();
        std::fs::write(dir.join("b.txt"), "needle\n").unwrap();
        app.find.scope = FileScope::Favorites;
        app.start_find_in_files(false);
        assert!(app.find.status.contains("favorite"), "there aren't any yet");
        app.find.favorite_files.push(dir.join("a.txt"));
        app.start_find_in_files(false);
        wait_for_results(&mut app);
        assert_eq!(app.job.as_ref().unwrap().hits.lock().unwrap().len(), 1, "only the favorite");
        let (again, _) = app_with(&app.current_settings());
        assert_eq!(again.find.favorite_files, [dir.join("a.txt")]);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn listing_gives_one_entry_per_line_and_goes_there() {
        let (mut app, ctx) = app_with("");
        app.doc().text.set_content(vec!["cat cat".into(), "dog".into(), "a cat".into()]);
        app.find.find = "cat".into();
        app.list_matching_lines();
        let hits = app.job.as_ref().unwrap().hits.lock().unwrap().clone();
        assert_eq!(hits.iter().map(|hit| hit.line).collect::<Vec<_>>(), [0, 2]);
        assert!(app.show_results);
        assert_eq!(app.results_title, "Lines containing the find string");
        draw(&mut app, &ctx, Vec::new());
        app.open_hit(&hits[1]);
        assert_eq!(app.docs[0].text.caret_line, 2);
    }

    #[test]
    fn listing_needs_something_to_look_for_and_a_text_view() {
        let (mut app, ctx) = app_with("");
        app.list_matching_lines();
        assert!(app.job.is_none());
        assert!(!app.find.status.is_empty());
        app.find.find = "x".into();
        command(&mut app, &ctx, Command::ToggleHex);
        app.list_matching_lines();
        assert!(app.job.is_none());
    }

    fn select_on_line(app: &mut Limoniter, line: usize, from: usize, to: usize) {
        app.doc().text.move_caret(line, from, false);
        app.doc().text.move_caret(line, to, true);
    }

    fn numbered_app() -> (Limoniter, Context) {
        let (mut app, ctx) = app_with("");
        app.doc().text.set_content(vec!["apple pie".into(), "plum".into(), "apple tart".into(), "pear".into()]);
        (app, ctx)
    }

    #[test]
    fn the_filter_shows_matching_lines_and_turns_editing_off_until_it_ends() {
        let (mut app, ctx) = numbered_app();
        select_on_line(&mut app, 0, 0, 5);
        command(&mut app, &ctx, Command::FilterSelection);
        let filter = app.docs[0].filter.as_ref().unwrap();
        assert_eq!(filter.lines, [0, 2]);
        assert_eq!(filter.needle, "apple");
        assert!(app.docs[0].text.read_only);
        draw(&mut app, &ctx, vec![egui::Event::Text("typed".into())]);
        assert_eq!(&*app.docs[0].text.line(0), "apple pie", "editing is off");
        assert!(!app.docs[0].is_modified(), "the file isn't changed");

        command(&mut app, &ctx, Command::ToggleReadOnly);
        assert!(app.docs[0].text.read_only, "and can't be turned on again behind its back");
        assert!(matches!(&app.dialog, Dialog::Message { body, .. } if body.contains("while filtering")));
        app.dialog = Dialog::None;

        command(&mut app, &ctx, Command::FilterSelection);
        assert!(app.docs[0].filter.is_none());
        assert!(!app.docs[0].text.read_only, "back as it was");
        draw(&mut app, &ctx, Vec::new());
    }

    #[test]
    fn a_read_only_file_stays_read_only_after_a_filter() {
        let (mut app, ctx) = numbered_app();
        app.doc().set_read_only(true);
        select_on_line(&mut app, 0, 0, 5);
        command(&mut app, &ctx, Command::FilterSelection);
        command(&mut app, &ctx, Command::FilterSelection);
        assert!(app.docs[0].text.read_only);
    }

    #[test]
    fn the_filter_needs_text_on_one_line_and_a_text_view() {
        let (mut app, ctx) = numbered_app();
        command(&mut app, &ctx, Command::FilterSelection);
        assert!(app.docs[0].filter.is_none());
        assert!(matches!(app.dialog, Dialog::Message { .. }));
        app.dialog = Dialog::None;
        app.doc().text.move_caret(0, 0, false);
        app.doc().text.move_caret(2, 2, true);
        command(&mut app, &ctx, Command::FilterSelection);
        assert!(app.docs[0].filter.is_none(), "several lines can't be looked for");
        app.dialog = Dialog::None;
        command(&mut app, &ctx, Command::ToggleHex);
        command(&mut app, &ctx, Command::FilterSelection);
        assert!(app.docs[0].filter.is_none());
    }

    #[test]
    fn the_filter_finds_exact_text() {
        assert_eq!(lines_containing(&["Apple", "apple", "pineapple", ""], "apple"), [1, 2]);
    }

    #[test]
    fn splitting_turns_on_changes_direction_and_turns_off() {
        let (mut app, ctx) = numbered_app();
        command(&mut app, &ctx, Command::SplitHorizontal);
        assert!(!app.docs[0].split.as_ref().unwrap().vertical);
        draw(&mut app, &ctx, Vec::new());
        command(&mut app, &ctx, Command::SplitVertical);
        assert!(app.docs[0].split.as_ref().unwrap().vertical, "the other button changes the direction");
        draw(&mut app, &ctx, Vec::new());
        command(&mut app, &ctx, Command::SplitVertical);
        assert!(app.docs[0].split.is_none(), "the same button ends it");
        draw(&mut app, &ctx, Vec::new());
    }

    #[test]
    fn both_panes_draw_every_mode_and_the_keyboard_follows_the_active_one() {
        let (mut app, ctx) = numbered_app();
        app.view.word_wrap = true;
        app.view.rainbow_parentheses = true;
        command(&mut app, &ctx, Command::SplitVertical);
        for _ in 0..3 {
            draw(&mut app, &ctx, Vec::new());
        }
        app.view.word_wrap = false;
        draw(&mut app, &ctx, vec![egui::Event::Text("A".into())]);
        draw(&mut app, &ctx, Vec::new());
        assert!(app.docs[0].text.line(0).starts_with('A'), "typing reaches the document from the first pane");
        app.docs[0].split.as_mut().unwrap().active = 1;
        app.pending_focus = true;
        draw(&mut app, &ctx, Vec::new());
        draw(&mut app, &ctx, Vec::new());
        draw(&mut app, &ctx, vec![egui::Event::Text("B".into())]);
        draw(&mut app, &ctx, Vec::new());
        assert!(app.docs[0].text.line(0).contains('B'), "and from the second, once it's the active one");
        assert_eq!(app.docs[0].text.line(0).matches('B').count(), 1, "not from both at once");
        command(&mut app, &ctx, Command::ToggleHex);
        draw(&mut app, &ctx, Vec::new());
        assert!(app.docs[0].split.is_some(), "hex mode shows one pane, and the split comes back afterwards");
        command(&mut app, &ctx, Command::ToggleHex);
        draw(&mut app, &ctx, Vec::new());
    }

    #[test]
    fn a_press_in_a_pane_makes_it_the_active_one() {
        let (mut app, ctx) = numbered_app();
        command(&mut app, &ctx, Command::SplitVertical);
        draw(&mut app, &ctx, Vec::new());
        draw(&mut app, &ctx, Vec::new());
        for events in click_at(egui::pos2(1400.0, 500.0)) {
            draw(&mut app, &ctx, events);
        }
        assert_eq!(app.docs[0].split.as_ref().unwrap().active, 1);
        for events in click_at(egui::pos2(200.0, 500.0)) {
            draw(&mut app, &ctx, events);
        }
        assert_eq!(app.docs[0].split.as_ref().unwrap().active, 0);
    }

    #[test]
    fn the_divider_can_be_dragged_but_not_out_of_sight() {
        let (mut app, ctx) = numbered_app();
        command(&mut app, &ctx, Command::SplitVertical);
        draw(&mut app, &ctx, Vec::new());
        draw(&mut app, &ctx, Vec::new());
        let button = |pos, pressed| egui::Event::PointerButton { pos, button: egui::PointerButton::Primary, pressed, modifiers: egui::Modifiers::NONE };
        let middle = egui::pos2(800.0, 500.0);
        draw(&mut app, &ctx, vec![egui::Event::PointerMoved(middle)]);
        draw(&mut app, &ctx, vec![button(middle, true)]);
        let far = egui::pos2(5.0, 500.0);
        draw(&mut app, &ctx, vec![egui::Event::PointerMoved(far)]);
        draw(&mut app, &ctx, vec![egui::Event::PointerMoved(far)]);
        draw(&mut app, &ctx, vec![button(far, false)]);
        assert_eq!(app.docs[0].split.as_ref().unwrap().share, 0.1);
        assert_eq!(app.docs[0].split.as_ref().unwrap().active, 0, "moving the divider doesn't change the pane");
    }

    #[test]
    fn a_filter_applies_to_both_panes() {
        let (mut app, ctx) = numbered_app();
        select_on_line(&mut app, 0, 0, 5);
        command(&mut app, &ctx, Command::SplitHorizontal);
        command(&mut app, &ctx, Command::FilterSelection);
        for _ in 0..3 {
            draw(&mut app, &ctx, Vec::new());
        }
    }

    #[test]
    fn a_duplicate_window_shares_edits_but_keeps_its_own_cursor() {
        let (mut app, ctx) = numbered_app();
        app.doc().text.move_caret(3, 1, false);
        command(&mut app, &ctx, Command::DuplicateWindow);
        assert_eq!(app.docs.len(), 1, "it's the one document");
        assert_eq!(app.all_tabs(), [(0, 1), (0, 2)]);
        assert_eq!(app.docs[0].window_number, 2);
        assert_eq!(app.docs[0].text.caret_line, 3, "it starts in the same place");
        app.doc().text.move_caret(0, 0, false);
        app.doc().text.type_text("X");
        draw(&mut app, &ctx, Vec::new());

        command(&mut app, &ctx, Command::NextTab);
        assert_eq!(app.docs[0].window_number, 1);
        assert_eq!(app.docs[0].text.caret_line, 3, "the first window's cursor is where it was");
        assert_eq!(&*app.docs[0].text.line(0), "Xapple pie", "and the edit shows there");
        draw(&mut app, &ctx, Vec::new());

        command(&mut app, &ctx, Command::PreviousTab);
        assert_eq!(app.docs[0].window_number, 2);
        assert_eq!(app.docs[0].text.caret_line, 0);
    }

    #[test]
    fn closing_a_duplicate_keeps_the_file_and_the_last_window_closes_it() {
        let (mut app, ctx) = app_with("");
        app.doc().text.type_text("unsaved");
        command(&mut app, &ctx, Command::DuplicateWindow);
        command(&mut app, &ctx, Command::CloseTab);
        assert_eq!(app.docs.len(), 1);
        assert!(app.docs[0].windows.is_empty());
        assert_eq!(app.docs[0].window_number, 1, "the other window is in front");
        assert_eq!(&*app.docs[0].text.line(0), "unsaved", "nothing was lost, and nothing was asked");
    }

    #[test]
    fn a_duplicate_windows_cursor_survives_the_file_shrinking() {
        let (mut app, ctx) = numbered_app();
        app.doc().text.move_caret(3, 0, false);
        command(&mut app, &ctx, Command::DuplicateWindow);
        app.doc().text.select_all();
        app.doc().text.delete_selection();
        command(&mut app, &ctx, Command::NextTab);
        draw(&mut app, &ctx, Vec::new());
        assert_eq!(app.docs[0].text.caret_line, 0, "it's kept inside the file");
    }

    #[test]
    fn sentence_and_title_case_run_from_their_commands_and_undo() {
        let (mut app, ctx) = app_with("");
        app.doc().text.set_content(vec!["the QUICK fox. it RAN".into()]);
        app.doc().text.select_all();
        command(&mut app, &ctx, Command::SentenceCase);
        assert_eq!(lines_of(&app), ["The quick fox. It ran"]);
        command(&mut app, &ctx, Command::TitleCase);
        assert_eq!(lines_of(&app), ["The Quick Fox. It Ran"]);
        command(&mut app, &ctx, Command::Undo);
        assert_eq!(lines_of(&app), ["The quick fox. It ran"]);
    }

    #[test]
    fn case_changes_respect_read_only_and_hex_mode() {
        let (mut app, ctx) = app_with("");
        app.doc().text.set_content(vec!["abc".into()]);
        app.doc().text.select_all();
        app.doc().set_read_only(true);
        for each in [Command::SentenceCase, Command::TitleCase] {
            command(&mut app, &ctx, each);
            assert_eq!(lines_of(&app), ["abc"]);
            assert!(matches!(app.dialog, Dialog::Message { .. }));
            app.dialog = Dialog::None;
        }
        app.doc().set_read_only(false);
        command(&mut app, &ctx, Command::ToggleHex);
        command(&mut app, &ctx, Command::TitleCase);
        assert_eq!(app.docs[0].hex.bytes(), b"abc");
    }

    #[test]
    fn folds_can_be_used_from_the_commands_and_a_jump_opens_them() {
        let (mut app, ctx) = app_with("");
        app.doc().text.set_content(vec!["top".into(), "    a".into(), "    b".into(), "next".into()]);
        command(&mut app, &ctx, Command::CollapseAll);
        assert!(app.docs[0].text.is_folded(0));
        draw(&mut app, &ctx, Vec::new());
        assert!(app.docs[0].fold_view_of.is_some());
        assert_eq!(app.docs[0].fold_view, [views::Hidden { first: 1, last: 2, shown_before: 1 }]);
        app.doc().text.go_to("3");
        draw(&mut app, &ctx, Vec::new());
        assert!(!app.docs[0].text.has_folds(), "going to a hidden line opens what's hiding it");
        command(&mut app, &ctx, Command::CollapseAll);
        command(&mut app, &ctx, Command::ExpandAll);
        draw(&mut app, &ctx, Vec::new());
        assert!(app.docs[0].fold_view_of.is_none());
    }

    #[test]
    fn hiding_needs_several_lines_and_says_so() {
        let (mut app, ctx) = app_with("");
        app.doc().text.set_content(vec!["a".into(), "b".into(), "c".into()]);
        command(&mut app, &ctx, Command::HideLines);
        assert!(matches!(app.dialog, Dialog::Message { .. }));
        app.dialog = Dialog::None;
        app.doc().text.set_selection((0, 0), (2, 1), false);
        command(&mut app, &ctx, Command::HideLines);
        assert!(app.docs[0].text.is_folded(0));
        app.doc().text.select_all();
        command(&mut app, &ctx, Command::ShowLines);
        assert!(!app.docs[0].text.has_folds());
    }

    #[test]
    fn folds_draw_in_both_panes_with_a_filter_winning() {
        let (mut app, ctx) = app_with("");
        app.doc().text.set_content(vec!["top".into(), "    a".into(), "next".into(), "top".into()]);
        command(&mut app, &ctx, Command::CollapseAll);
        command(&mut app, &ctx, Command::SplitVertical);
        for menu in [3, 2] {
            app.shown_menu = Some((menu, true));
            draw(&mut app, &ctx, Vec::new());
        }
        app.doc().text.set_selection((3, 0), (3, 3), false);
        command(&mut app, &ctx, Command::FilterSelection);
        draw(&mut app, &ctx, Vec::new());
        assert!(app.docs[0].filter.is_some());
    }

    #[test]
    fn make_selections_runs_from_its_command_and_explains_itself() {
        let (mut app, ctx) = app_with("");
        app.doc().text.set_content(vec!["cat cat".into(), "cat".into()]);
        command(&mut app, &ctx, Command::MakeSelections);
        assert!(matches!(app.dialog, Dialog::Message { .. }));
        app.dialog = Dialog::None;
        app.doc().text.set_selection((0, 0), (0, 3), false);
        command(&mut app, &ctx, Command::MakeSelections);
        assert_eq!(app.docs[0].text.multi_count(), 3);
        draw(&mut app, &ctx, vec![egui::Event::Text("x".into())]);
        draw(&mut app, &ctx, Vec::new());
        assert_eq!(lines_of(&app), ["x x", "x"]);
        command(&mut app, &ctx, Command::Undo);
        assert_eq!(lines_of(&app), ["cat cat", "cat"]);
    }

    #[test]
    fn copying_and_cutting_several_selections_go_through_the_commands() {
        let (mut app, ctx) = app_with("");
        app.doc().text.set_content(vec!["cat dog".into(), "cat".into()]);
        app.doc().text.set_selection((0, 0), (0, 3), false);
        command(&mut app, &ctx, Command::MakeSelections);
        assert_eq!(copied(&command(&mut app, &ctx, Command::Copy)).as_deref(), Some("cat\r\ncat"));
        assert_eq!(copied(&command(&mut app, &ctx, Command::Cut)).as_deref(), Some("cat\r\ncat"));
        assert_eq!(lines_of(&app), [" dog", ""]);
    }

    #[test]
    fn compare_lists_the_other_open_files_and_shows_their_unsaved_edits() {
        let (mut app, ctx) = app_with("");
        app.doc().text.set_content(vec!["same".into(), "ours".into()]);
        app.new_document();
        app.doc().text.set_content(vec!["same".into(), "theirs".into()]);
        app.active = 0;
        command(&mut app, &ctx, Command::Compare);
        let Dialog::ComparePick(others) = &app.dialog else {
            panic!("it asks which");
        };
        assert_eq!(others.len(), 1, "not this file itself");
        assert_eq!(others[0].0, 1);
        draw(&mut app, &ctx, Vec::new());
        let lines = app.lines_of_open_file(1);
        app.show_comparison(lines);
        let Dialog::Compare(state) = &app.dialog else {
            panic!("it's showing");
        };
        assert_eq!(state.differences, [1]);
        assert_eq!(state.current, Some(0), "it starts at the first difference");
        draw(&mut app, &ctx, Vec::new());
        draw(&mut app, &ctx, vec![key(Key::Escape, egui::Modifiers::NONE)]);
        assert!(matches!(app.dialog, Dialog::None));
        assert_eq!(lines_of(&app), ["same", "ours"], "comparing changes nothing");
    }

    #[test]
    fn compare_explains_what_it_cant_take() {
        let (mut app, ctx) = app_with("");
        command(&mut app, &ctx, Command::ToggleHex);
        command(&mut app, &ctx, Command::Compare);
        assert!(matches!(&app.dialog, Dialog::Message { body, .. } if body.contains("hex mode")));
        app.dialog = Dialog::None;
        command(&mut app, &ctx, Command::ToggleHex);
        app.new_document();
        command(&mut app, &ctx, Command::ToggleHex);
        app.active = 0;
        assert!(app.lines_of_open_file(1).is_err());
    }

    #[test]
    fn the_functions_and_tags_panels_draw_and_are_remembered() {
        let (mut app, ctx) = app_with("functions=true\ntags=true\n");
        assert!(app.panels.functions && app.panels.tags);
        app.doc().text.set_content(vec!["fn a() {}".into()]);
        for _ in 0..3 {
            draw(&mut app, &ctx, Vec::new());
        }
        app.close_tab(0);
        draw(&mut app, &ctx, Vec::new());
        assert!(app.current_settings().contains("functions=true"));
    }

    #[test]
    fn a_tag_goes_in_through_its_action_and_respects_read_only() {
        let (mut app, ctx) = app_with("");
        app.doc().text.set_content(vec!["word".into()]);
        app.doc().text.select_all();
        let mut output = ctx.run_ui(input(Vec::new()), |ui| app.run_panel_action(Action::WrapTag("<b>".into(), "</b>".into()), ui));
        output.textures_delta.clear();
        assert_eq!(lines_of(&app), ["<b>word</b>"]);
        app.doc().set_read_only(true);
        let mut output = ctx.run_ui(input(Vec::new()), |ui| app.run_panel_action(Action::WrapTag("<i>".into(), "</i>".into()), ui));
        output.textures_delta.clear();
        assert_eq!(lines_of(&app), ["<b>word</b>"]);
        assert!(matches!(app.dialog, Dialog::Message { .. }));
    }

    fn markdown_app() -> (Limoniter, Context, PathBuf) {
        let (mut app, ctx) = app_with("");
        let path = temp_file("preview.md", b"# Title\n\nSome *text*.\n");
        let renamed = path.with_extension("md");
        std::fs::rename(&path, &renamed).unwrap();
        assert!(app.open_path(&renamed));
        (app, ctx, renamed)
    }

    #[test]
    fn the_preview_shows_markdown_and_keeps_up_with_typing() {
        let (mut app, ctx, _) = markdown_app();
        command(&mut app, &ctx, Command::TogglePreview);
        assert_eq!(app.docs[app.active].preview_view, settings::PreviewView::Split);
        draw(&mut app, &ctx, Vec::new());
        assert_eq!(app.preview_blocks.len(), 2);
        assert_eq!(app.preview_blocks[0].kind, preview::BlockKind::Heading(1));
        app.doc().text.move_caret(0, 0, false);
        app.doc().text.type_text("## Sooner\n\n");
        draw(&mut app, &ctx, Vec::new());
        assert_eq!(app.preview_blocks[0].kind, preview::BlockKind::Heading(2), "it follows the edit without saving");
        assert!(app.docs[app.active].is_modified());
    }

    #[test]
    fn ansi_art_opens_as_text_by_its_ending_and_previews_from_its_exact_bytes() {
        let (mut app, ctx) = app_with("");
        // A SAUCE record holds zero bytes, which would otherwise send the file to hex mode.
        let mut data = b"\x1b[1;33mHi\x1b[0m\r\n".to_vec();
        data.push(0x1A);
        let mut record = vec![0u8; 128];
        record[..7].copy_from_slice(b"SAUCE00");
        record[7..12].copy_from_slice(b"Hello");
        record[94] = 1;
        record[95] = 1;
        record[96] = 80;
        data.extend_from_slice(&record);
        let path = temp_file("art.tmp", &data).with_extension("ans");
        std::fs::write(&path, &data).unwrap();
        assert!(app.open_path(&path));
        let doc = &app.docs[app.active];
        assert!(!doc.hex_mode);
        assert_eq!(doc.text.format.preset, Some("pc-ansi"));
        assert_eq!(doc.text.format.certainty, Certainty::Suggested);
        assert!(!doc.format_chosen, "a file ending isn't remembered as a choice");
        app.doc().preview_view = settings::PreviewView::Split;
        draw(&mut app, &ctx, Vec::new());
        let screen = app.preview_screen.as_ref().expect("the art is drawn");
        assert_eq!(screen.row_text(0), "Hi");
        assert_eq!(screen.rows[0][0].fg, crate::ansi_art::VGA[11]);
        assert!(screen.info.iter().any(|(key, value)| key == "Title" && value == "Hello"));
        assert_eq!(app.docs[app.active].text.encode(&app.docs[app.active].text.format, Lossy::Refuse).unwrap(), data, "nothing about the file changes");
        // In hex mode the preview still has the bytes to draw.
        app.doc().hex.load(data.clone());
        app.doc().hex_mode = true;
        app.preview_screen = None;
        draw(&mut app, &ctx, Vec::new());
        assert_eq!(app.preview_screen.as_ref().unwrap().row_text(0), "Hi");
    }

    #[test]
    fn petscii_and_teletext_files_get_their_screens() {
        let (mut app, ctx) = app_with("");
        let path = temp_file("menu.tmp", b"\x1cRED\x0d").with_extension("seq");
        std::fs::write(&path, b"\x1cRED\x0d").unwrap();
        assert!(app.open_path(&path));
        app.doc().preview_view = settings::PreviewView::Split;
        draw(&mut app, &ctx, Vec::new());
        let screen = app.preview_screen.as_ref().unwrap();
        assert_eq!((screen.row_text(0).as_str(), screen.rows[0][0].fg), ("RED", crate::retro_screens::C64[2]));
        let mut page = vec![0xFE, 0x01, 0x09, 0, 0, 0];
        page.extend([0x01, b'T', b'V']);
        page.resize(1006, b' ');
        page.extend([0, 0]);
        let path = temp_file("page.tmp", &page).with_extension("ep1");
        std::fs::write(&path, &page).unwrap();
        assert!(app.open_path(&path));
        app.doc().preview_view = settings::PreviewView::Split;
        draw(&mut app, &ctx, Vec::new());
        let screen = app.preview_screen.as_ref().unwrap();
        assert_eq!(screen.rows.len(), 25);
        assert_eq!((screen.rows[0][1].ch, screen.rows[0][1].fg), ('T', crate::retro_screens::TELETEXT[1]));
    }

    #[test]
    fn new_as_makes_a_file_of_each_kind_and_the_list_can_be_changed_and_kept() {
        let (mut app, _ctx) = app_with("");
        let at = |app: &Limoniter, name: &str| app.new_kinds.iter().position(|k| k.name == name).unwrap();
        app.new_of_kind(at(&app, "Spectrum BASIC"));
        let doc = &app.docs[app.active];
        assert!(doc.name().ends_with(".bas"), "{}", doc.name());
        assert_eq!(doc.language_name(), highlight::BASIC_SINCLAIR);
        assert!(matches!(doc.listing_of, Some((basic::Machine::Spectrum, _))), "F9 and Send tokenise it");
        assert!(app.is_spectrum_basic(app.active), "and GRAPH mode works in it");
        assert_eq!(&*doc.text.line(0), "10 PRINT \"Hello from Limoniter\"");
        app.new_of_kind(at(&app, "Z80 assembler"));
        assert_eq!(app.doc_cpu(app.active), Some(crate::assemblers::Cpu::Z80));
        app.new_of_kind(at(&app, "Commodore text (PETSCII)"));
        let doc = &app.docs[app.active];
        assert_eq!(doc.text.format.charset.id, "petscii-c64");
        assert_eq!(doc.text.format.display, Display::Petscii, "shown as PETSCII, as its preset is");
        assert_eq!(doc.text.insert_eol(), Eol::Cr);

        // The usual list isn't written to the settings, so later versions can add to it.
        assert_eq!(new_kinds::kinds_to_settings(&app.new_kinds), "");
        let mut kinds = new_kinds::NewKind::usual();
        kinds.truncate(1);
        kinds.push(new_kinds::NewKind { name: "QL SuperBASIC".into(), ending: "bas".into(), encoding: "iso-8859-1".into(), eol: Eol::Lf, language: String::new(), program: new_kinds::Program::None, starter: "100 REMark one\n110 PRINT 1".into() });
        let kept = new_kinds::kinds_to_settings(&kinds);
        assert_eq!(new_kinds::kinds_from_settings(&kept), kinds);
        assert!(new_kinds::kinds_from_settings("new_kinds=changed\n").is_empty(), "an emptied list stays empty");
        app.new_kinds = kinds;
        app.new_of_kind(1);
        let doc = &app.docs[app.active];
        assert_eq!(doc.text.format.charset.id, "iso-8859-1");
        assert_eq!(doc.text.all_lines().len(), 2);
        assert_eq!(doc.text.insert_eol(), Eol::Lf);
        assert!(app.current_settings().contains("new_kind=QL SuperBASIC"));
    }

    #[test]
    fn z80_source_opens_as_text_and_a_z80_snapshot_as_a_snapshot() {
        let (mut app, _ctx) = app_with("");
        let source = b"        org 32768\r\nstart:  ld a,2\r\n        ret\r\n";
        let path = temp_file("source.tmp", source).with_extension("z80");
        std::fs::write(&path, source).unwrap();
        assert!(app.open_path(&path));
        assert!(app.docs[app.active].browser.is_none(), "assembler source isn't a snapshot");
        assert!(crate::running::Runnable::from_file("source.z80", source.to_vec()).is_none(), "nor is it run as one");
        assert_eq!(&*app.docs[app.active].text.line(1), "start:  ld a,2");
    }

    #[test]
    fn a_tape_that_loads_udgs_lists_with_their_pictures_and_graph_mode_types_them() {
        let (mut app, _ctx) = app_with("");
        app.prefs.zx_notation = crate::zx_graphics::Notation::Bas2tap;
        let program = basic::tokenise_spectrum(&["10 LOAD \"\"CODE USR \"a\"", "20 PRINT \"{UDG-A}\u{2588}\""], basic::ZxDialect::Spectrum).unwrap();
        let mut tape = basic::spectrum_tape(&program, "ship", Some(10));
        let mut header = vec![3u8];
        header.extend(b"udgs      ");
        header.extend(8u16.to_le_bytes());
        header.extend(0xFF58u16.to_le_bytes());
        header.extend(0x8000u16.to_le_bytes());
        tape.extend(basic::tap_block(0x00, &header));
        tape.extend(basic::tap_block(0xFF, &[0x18, 0x3C, 0x7E, 0xFF, 0xFF, 0x7E, 0x3C, 0x18]));
        // Opened from the tape's list of files, the program brings the UDGs its tape loads.
        let path = temp_file("ship.tmp", &tape).with_extension("tap");
        std::fs::write(&path, &tape).unwrap();
        assert!(app.open_path(&path));
        app.open_from_browser(app.active, 0);
        app.open_basic_listing();
        let zx = app.docs[app.active].text.zx.as_ref().expect("UDGs from the tape");
        assert_eq!(zx.udgs[0], Some([0x18, 0x3C, 0x7E, 0xFF, 0xFF, 0x7E, 0x3C, 0x18]));
        assert_eq!(zx.udgs_from.as_deref(), Some("the tape it came from"));
        assert!(matches!(app.docs[app.active].listing_of, Some((basic::Machine::Spectrum, _))), "from a tape, it goes back as a tape, and runs");
        assert_eq!(app.docs[app.active].language_name(), highlight::BASIC_SINCLAIR, "coloured as Spectrum BASIC straight away");
        app.docs.push(Document::from_image_file("ship.tap".into(), tape, None));
        app.active = app.docs.len() - 1;
        app.open_basic_listing();
        let doc = &app.docs[app.active];
        assert_eq!(&*doc.text.line(1), "  20 PRINT \"{A}{+8}\"", "written as the setting says, bas2tap's way");
        let zx = doc.text.zx.as_ref().expect("a Spectrum listing");
        assert_eq!(zx.udgs[0], Some([0x18, 0x3C, 0x7E, 0xFF, 0xFF, 0x7E, 0x3C, 0x18]));
        assert_eq!(zx.udgs_from.as_deref(), Some("the tape"));
        assert!(app.is_spectrum_basic(app.active));
        app.toggle_graph_mode();
        assert!(app.docs[app.active].text.zx.as_ref().unwrap().graph);
        // It goes back to the same program.
        let lines = app.docs[app.active].text.all_lines();
        assert_eq!(basic::tokenise_spectrum(&lines, basic::ZxDialect::Spectrum).unwrap(), program);
        // GRAPH mode is only for Spectrum BASIC.
        app.docs.push(Document::untitled("notes".into(), &PRESETS[0]));
        app.active = app.docs.len() - 1;
        assert!(!app.is_spectrum_basic(app.active));
    }

    #[test]
    fn a_spectrum_tape_previews_as_a_listing_and_opens_as_basic_text() {
        let (mut app, ctx) = app_with("");
        let program = basic::tokenise_spectrum(&["10 PRINT \"hello\"", "20 GO TO 10"], basic::ZxDialect::Spectrum).unwrap();
        let tape = basic::spectrum_tape(&program, "hello", Some(10));
        let path = temp_file("tape.tmp", &tape).with_extension("tap");
        std::fs::write(&path, &tape).unwrap();
        assert!(app.open_path(&path));
        assert!(app.docs[app.active].browser.is_some(), "a tape opens as a list of its files");
        // Its bytes, shown as themselves, still preview as the listing.
        app.docs.push(Document::from_image_file("hello.tap".into(), tape.clone(), None));
        app.active = app.docs.len() - 1;
        assert!(app.docs[app.active].hex_mode, "a tape is binary, so it opens in hex");
        app.doc().preview_view = settings::PreviewView::Split;
        draw(&mut app, &ctx, Vec::new());
        let code: Vec<&preview::Block> = app.preview_blocks.iter().filter(|b| b.kind == preview::BlockKind::Code).collect();
        assert_eq!(code.len(), 1, "the listing is drawn from hex mode");
        assert_eq!(code[0].spans[0].text, "  10 PRINT \"hello\"\n  20 GO TO 10");
        let tabs = app.docs.len();
        app.open_basic_listing();
        assert_eq!(app.docs.len(), tabs + 1);
        let listed: Vec<String> = app.docs[app.active].text.all_lines().iter().map(|line| line.to_string()).collect();
        assert_eq!(listed, ["  10 PRINT \"hello\"", "  20 GO TO 10"]);
        assert_eq!(app.docs[app.active].name(), "hello.tap listing");
        // A .bas file that's a listing kept as text isn't taken for a program.
        let text = temp_file("listing.tmp", b"10 PRINT 1\r\n").with_extension("bas");
        std::fs::write(&text, b"10 PRINT 1\r\n").unwrap();
        assert!(app.open_path(&text));
        app.doc().preview_view = settings::PreviewView::Split;
        draw(&mut app, &ctx, Vec::new());
        assert!(app.preview_blocks.is_empty());
        assert!(app.preview_note.as_deref().is_some_and(|note| note.contains("listing kept as text")));
    }

    #[test]
    fn a_wide_table_leaves_the_preview_at_its_own_width() {
        let (mut app, ctx, _) = markdown_app();
        app.doc().preview_view = settings::PreviewView::Split;
        draw(&mut app, &ctx, Vec::new());
        let wide = format!("| {0} | {0} | {0} |", "plenty of words in a cell ".repeat(10));
        app.doc().text.set_content(vec![wide.clone(), "|---|---|---|".into(), wide]);
        let before = editor_width(&mut app, &ctx);
        for _ in 0..5 {
            draw(&mut app, &ctx, Vec::new());
        }
        assert!(matches!(app.preview_blocks[0].kind, preview::BlockKind::Table(_)));
        assert_eq!(editor_width(&mut app, &ctx), before, "the text beside it keeps its room");
    }

    /// How much room the panels are leaving for the text.
    fn editor_width(app: &mut Limoniter, ctx: &Context) -> f32 {
        let mut left = 0.0;
        let mut output = ctx.run_ui(input(Vec::new()), |ui| {
            app.preview_panel(ui);
            left = ui.available_width();
        });
        output.textures_delta.clear();
        left
    }

    #[test]
    fn the_preview_says_why_when_it_cant_show_a_file() {
        let (mut app, ctx) = app_with("");
        app.doc().text.set_content(vec!["# not markdown here".into()]);
        app.doc().preview_view = settings::PreviewView::Full;
        draw(&mut app, &ctx, Vec::new());
        assert!(app.preview_blocks.is_empty(), "a plain text file isn't guessed at");
        assert!(!app.full_preview_shown, "and with nothing to show, the text stays");
        app.close_tab(0);
        draw(&mut app, &ctx, Vec::new());
        let (mut hex_app, hex_ctx, _) = markdown_app();
        hex_app.doc().preview_view = settings::PreviewView::Full;
        command(&mut hex_app, &hex_ctx, Command::ToggleHex);
        draw(&mut hex_app, &hex_ctx, Vec::new());
        assert!(!hex_app.full_preview_shown, "hex mode still shows its bytes");
    }

    /// A Markdown file and a plain one, opened with a choice in the settings.
    fn opened_with(settings: &str) -> (Limoniter, Context, usize, usize) {
        let (mut app, ctx) = app_with(settings);
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let dir = std::env::temp_dir().join(format!("limoniter-markdown-view-{}-{}", std::process::id(), NEXT.fetch_add(1, Ordering::Relaxed)));
        std::fs::create_dir_all(&dir).unwrap();
        let (markdown, plain) = (dir.join("notes.md"), dir.join("notes.txt"));
        std::fs::write(&markdown, "# Title\n\nWords.\n").unwrap();
        std::fs::write(&plain, "# Title\n").unwrap();
        assert!(app.open_path(&markdown));
        let first = app.active;
        assert!(app.open_path(&plain));
        let second = app.active;
        (app, ctx, first, second)
    }

    #[test]
    fn markdown_files_open_as_the_setting_says_and_other_files_dont() {
        for (name, view) in [("", settings::PreviewView::Text), ("markup", settings::PreviewView::Text), ("split", settings::PreviewView::Split), ("full", settings::PreviewView::Full)] {
            let (app, _, markdown, plain) = opened_with(&format!("markdown_view={name}\n"));
            assert_eq!(app.docs[markdown].preview_view, view, "{name}");
            assert_eq!(app.docs[plain].preview_view, settings::PreviewView::Text, "{name}");
        }
    }

    #[test]
    fn the_preview_belongs_to_its_tab_and_a_new_setting_reaches_open_markdown_files() {
        let (mut app, ctx, markdown, plain) = opened_with("markdown_view=split\n");
        command(&mut app, &ctx, Command::SetPreview(settings::PreviewView::Full));
        assert_eq!(app.docs[plain].preview_view, settings::PreviewView::Full);
        assert_eq!(app.docs[markdown].preview_view, settings::PreviewView::Split, "changing one tab leaves the others");
        app.docs[plain].preview_view = settings::PreviewView::Text;
        app.prefs.markdown_view = settings::PreviewView::Full;
        app.apply_markdown_view();
        assert_eq!(app.docs[markdown].preview_view, settings::PreviewView::Full);
        assert_eq!(app.docs[plain].preview_view, settings::PreviewView::Text);
        assert!(app.current_settings().contains("markdown_view=full"));
    }

    #[test]
    fn full_screen_shows_the_preview_and_no_keys_reach_the_hidden_text() {
        let (mut app, ctx, markdown, _) = opened_with("markdown_view=full\n");
        app.active = markdown;
        draw(&mut app, &ctx, Vec::new());
        draw(&mut app, &ctx, Vec::new());
        assert!(app.full_preview_shown);
        assert_eq!(app.preview_blocks[0].kind, preview::BlockKind::Heading(1));
        draw(&mut app, &ctx, vec![egui::Event::Text("typed".into()), key(Key::D, egui::Modifiers::CTRL), key(Key::Delete, egui::Modifiers::NONE)]);
        assert_eq!(lines_of_doc(&app, markdown), ["# Title", "", "Words.", ""]);
        // A command can still get to it, and the preview follows.
        command(&mut app, &ctx, Command::SetPreview(settings::PreviewView::Text));
        draw(&mut app, &ctx, Vec::new());
        assert!(!app.full_preview_shown);
        draw(&mut app, &ctx, Vec::new());
        draw(&mut app, &ctx, vec![egui::Event::Text("typed".into())]);
        assert!(lines_of_doc(&app, markdown)[0].contains("typed"), "with the text back, typing works again");
    }

    fn lines_of_doc(app: &Limoniter, index: usize) -> Vec<String> {
        app.docs[index].text.all_lines().iter().map(|line| line.to_string()).collect()
    }

    fn paste_app(on: bool) -> (Limoniter, Context) {
        let (mut app, ctx) = app_with(if on { "copy_before_paste=true\n" } else { "" });
        set_text(&mut app, &["old text here"]);
        draw(&mut app, &ctx, Vec::new());
        app.doc().text.set_selection((0, 0), (0, 3), false);
        (app, ctx)
    }

    #[test]
    fn copy_before_paste_swaps_the_selection_with_the_clipboard() {
        let (mut app, ctx) = paste_app(true);
        let output = draw(&mut app, &ctx, vec![egui::Event::Paste("new".into())]);
        assert_eq!(lines_of(&app), ["new text here"]);
        assert_eq!(copied(&output).as_deref(), Some("old"));
        // Pasting that back swaps them again.
        app.doc().text.set_selection((0, 0), (0, 3), false);
        let output = draw(&mut app, &ctx, vec![egui::Event::Paste("old".into())]);
        assert_eq!(lines_of(&app), ["old text here"]);
        assert_eq!(copied(&output).as_deref(), Some("new"));
        command(&mut app, &ctx, Command::Undo);
        assert_eq!(lines_of(&app), ["new text here"]);
    }

    #[test]
    fn copy_before_paste_is_off_until_chosen() {
        let (mut app, ctx) = paste_app(false);
        let output = draw(&mut app, &ctx, vec![egui::Event::Paste("new".into())]);
        assert_eq!(lines_of(&app), ["new text here"]);
        assert_eq!(copied(&output), None);
    }

    #[test]
    fn copy_before_paste_leaves_the_clipboard_alone_with_nothing_selected_or_nothing_to_paste() {
        let (mut app, ctx) = paste_app(true);
        app.prefs.edit.whole_line_copy = true;
        draw(&mut app, &ctx, Vec::new());
        app.doc().text.move_caret(0, 3, false);
        let output = draw(&mut app, &ctx, vec![egui::Event::Paste("new".into())]);
        assert_eq!(lines_of(&app), ["oldnew text here"]);
        assert_eq!(copied(&output), None, "not even the whole line");
        let (mut app, ctx) = paste_app(true);
        let output = draw(&mut app, &ctx, vec![egui::Event::Paste(String::new())]);
        assert_eq!(copied(&output), None);
    }

    #[test]
    fn copy_before_paste_works_from_every_way_of_pasting() {
        // A column block keeps its shape on the way to the clipboard, and what's pasted is still recognised as a block.
        let (mut app, ctx) = app_with("copy_before_paste=true\n");
        set_text(&mut app, &["abcd", "efgh"]);
        draw(&mut app, &ctx, Vec::new());
        app.clip.remember("XY\nZW", true);
        app.doc().text.set_selection((0, 1), (1, 3), true);
        let output = draw(&mut app, &ctx, vec![egui::Event::Paste("XY\nZW".into())]);
        assert_eq!(lines_of(&app), ["aXYd", "eZWh"]);
        assert_eq!(copied(&output).as_deref(), Some("bc\r\nfg"));
        assert!(app.clip.is_block("bc\nfg"), "pasted back, it goes in as a block");

        // The Clipboard History panel.
        let (mut app, ctx) = paste_app(true);
        let mut output = ctx.run_ui(input(Vec::new()), |ui| app.run_panel_action(Action::Paste("new".into()), ui));
        output.textures_delta.clear();
        assert_eq!(lines_of(&app), ["new text here"]);
        assert_eq!(copied(&output).as_deref(), Some("old"));

        // Hex mode.
        let (mut app, ctx) = paste_app(true);
        command(&mut app, &ctx, Command::ToggleHex);
        draw(&mut app, &ctx, Vec::new());
        app.doc().hex.move_caret(0, false);
        app.doc().hex.move_caret(2, true);
        let output = draw(&mut app, &ctx, vec![egui::Event::Paste("41".into())]);
        assert!(copied(&output).is_some(), "the selected bytes go on the clipboard");
    }

    fn set_text(app: &mut Limoniter, lines: &[&str]) {
        app.doc().text.set_content(lines.iter().map(|line| line.to_string()).collect());
    }

    #[test]
    fn reindent_goes_by_the_language_and_says_when_it_doesnt_suit() {
        let (mut app, ctx) = app_with("");
        set_text(&mut app, &["{", "a", "}"]);
        command(&mut app, &ctx, Command::Reindent);
        assert_eq!(lines_of(&app), ["{", "\ta", "}"], "with no language it goes by the brackets");
        assert!(reindent_marks("C").unwrap().preprocessor);
        assert_eq!(reindent_marks("Rust").unwrap().line, Some("//"));
        assert!(reindent_marks("JSON").is_ok());
        assert_eq!(reindent_marks("CSS").unwrap().block, Some(("/*", "*/")));
        for language in ["Python", "YAML", "Makefile", "Bourne Again Shell (bash)", "HTML", "Assembly 6502"] {
            assert!(reindent_marks(language).is_err(), "{language}");
        }
    }

    #[test]
    fn wrap_and_join_run_on_the_selection_or_the_file_and_undo() {
        let (mut app, ctx) = app_with("");
        set_text(&mut app, &["one two three four", "keep this line as it is please"]);
        app.dialog = Dialog::WrapColumn { value: "3".into(), error: String::new(), focus: false };
        press_enter(&mut app, &ctx);
        assert!(matches!(&app.dialog, Dialog::WrapColumn { error, .. } if !error.is_empty()), "too narrow is explained");
        app.doc().text.set_selection((0, 0), (0, 5), false);
        app.dialog = Dialog::WrapColumn { value: "10".into(), error: String::new(), focus: false };
        press_enter(&mut app, &ctx);
        assert!(matches!(app.dialog, Dialog::None));
        assert_eq!(lines_of(&app), ["one two", "three four", "keep this line as it is please"], "only the selected line");
        app.doc().text.set_selection((0, 0), (2, 0), false);
        command(&mut app, &ctx, Command::LineEndsToWrap);
        assert_eq!(lines_of(&app)[0], "one two three four");
        command(&mut app, &ctx, Command::Undo);
        assert_eq!(lines_of(&app)[1], "three four");
    }

    #[test]
    fn line_endings_survive_wrapping_and_joining() {
        let (mut app, ctx) = app_with("");
        let path = temp_file("eols", b"one two three\r\nnext\nlast");
        assert!(app.open_path(&path));
        app.doc().text.move_caret(0, 0, false);
        app.doc().text.move_caret(0, 3, true);
        app.dialog = Dialog::WrapColumn { value: "10".into(), error: String::new(), focus: false };
        press_enter(&mut app, &ctx);
        command(&mut app, &ctx, Command::Save);
        assert_eq!(std::fs::read(&path).unwrap(), b"one two\r\nthree\r\nnext\nlast", "the other lines end as they did");
    }

    #[test]
    fn paragraph_tools_use_the_setup_and_remember_it() {
        let (mut app, ctx) = app_with("");
        set_text(&mut app, &["one two three four five", "six", "", "other"]);
        app.dialog = Dialog::ParagraphSetup { width: "5".into(), left: "0".into(), first: "0".into(), align: Align::Left, error: String::new() };
        press_enter(&mut app, &ctx);
        assert!(matches!(app.dialog, Dialog::ParagraphSetup { .. }), "too narrow stays open");
        app.dialog = Dialog::ParagraphSetup { width: "12".into(), left: "1".into(), first: "0".into(), align: Align::Left, error: String::new() };
        press_enter(&mut app, &ctx);
        assert!(matches!(app.dialog, Dialog::None));
        command(&mut app, &ctx, Command::ParagraphReformat);
        assert_eq!(lines_of(&app), [" one two", " three four", " five six", "", "other"], "only the cursor's paragraph");
        let settings = app.current_settings();
        assert!(settings.contains("paragraph_width=12"));
        let (again, _) = app_with(&settings);
        assert_eq!(again.prefs.paragraph.width, 12);
    }

    #[test]
    fn alignment_acts_on_the_paragraph_and_reformat_then_follows_it() {
        let (mut app, ctx) = app_with("");
        set_text(&mut app, &["ab"]);
        app.prefs.paragraph.width = 10;
        command(&mut app, &ctx, Command::AlignText(Align::Right));
        assert_eq!(lines_of(&app), ["        ab"]);
        assert_eq!(app.prefs.paragraph.align, Align::Right);
        app.doc().text.move_caret(0, 0, false);
        set_text(&mut app, &[""]);
        command(&mut app, &ctx, Command::ParagraphReformat);
        assert!(matches!(&app.dialog, Dialog::Message { body, .. } if body.contains("paragraph")), "a blank line has no paragraph");
    }

    #[test]
    fn layout_tools_respect_read_only_hex_and_several_selections() {
        let (mut app, ctx) = app_with("");
        set_text(&mut app, &["a a"]);
        app.doc().set_read_only(true);
        for each in [Command::Reindent, Command::WrapToLineEnds, Command::LineEndsToWrap, Command::ParagraphReformat, Command::AlignText(Align::Right), Command::Format(formatters::Kind::Js)] {
            command(&mut app, &ctx, each);
            assert!(matches!(app.dialog, Dialog::Message { .. }), "told it's read-only");
            app.dialog = Dialog::None;
        }
        app.doc().set_read_only(false);
        app.doc().text.set_selection((0, 0), (0, 1), false);
        app.doc().text.make_selections().unwrap();
        command(&mut app, &ctx, Command::AlignText(Align::Right));
        assert!(matches!(&app.dialog, Dialog::Message { body, .. } if body.contains("Esc")));
        assert_eq!(lines_of(&app), ["a a"]);
        app.dialog = Dialog::None;
        app.doc().text.end_multi();
        command(&mut app, &ctx, Command::ToggleHex);
        for each in [Command::Reindent, Command::ParagraphReformat, Command::Format(formatters::Kind::Xml)] {
            command(&mut app, &ctx, each);
            assert!(matches!(app.dialog, Dialog::None));
        }
        assert_eq!(app.docs[0].hex.bytes(), b"a a");
    }

    #[test]
    fn a_formatter_changes_the_file_or_the_selection_in_one_undoable_step() {
        let (mut app, ctx) = app_with("");
        set_text(&mut app, &["<a><b/></a>", ""]);
        command(&mut app, &ctx, Command::Format(formatters::Kind::Xml));
        assert_eq!(lines_of(&app), ["<a>", "\t<b/>", "</a>", ""]);
        assert_eq!(app.docs[0].text.caret_line, 0, "the cursor goes back to the top after the whole file changes");
        command(&mut app, &ctx, Command::Undo);
        assert_eq!(lines_of(&app), ["<a><b/></a>", ""]);
        set_text(&mut app, &["keep", "a();b();", "keep"]);
        app.doc().text.set_selection((1, 0), (1, 7), false);
        command(&mut app, &ctx, Command::Format(formatters::Kind::Js));
        assert_eq!(lines_of(&app), ["keep", "a();", "b();", "keep"]);
    }

    #[test]
    fn a_formatter_that_cant_says_why_and_changes_nothing() {
        let (mut app, ctx) = app_with("");
        set_text(&mut app, &["<a><b></a>"]);
        command(&mut app, &ctx, Command::Format(formatters::Kind::Xml));
        assert!(matches!(&app.dialog, Dialog::Message { body, .. } if body.contains("well-formed")));
        assert_eq!(lines_of(&app), ["<a><b></a>"]);
        assert!(!app.docs[0].is_modified());
        app.dialog = Dialog::None;
        set_text(&mut app, &["<a/>"]);
        let before = app.docs[0].text.version();
        command(&mut app, &ctx, Command::Format(formatters::Kind::Xml));
        assert_eq!(app.docs[0].text.version(), before, "already laid out is no edit");
    }

    #[test]
    fn the_tools_and_format_menus_draw_with_a_project_and_without_a_file() {
        let (mut app, ctx) = app_with("project_panel=true\n");
        for menu in [5, 4] {
            app.shown_menu = Some((menu, true));
            draw(&mut app, &ctx, Vec::new());
            draw(&mut app, &ctx, Vec::new());
        }
        app.close_tab(0);
        app.shown_menu = Some((5, false));
        draw(&mut app, &ctx, Vec::new());
    }

    /// Every command that the ribbon offered is still somewhere in the menus.
    #[test]
    fn nothing_was_lost_in_the_move_to_menus() {
        let reachable: Vec<Command> = menus::every_command().into_iter().map(|found| found.command).collect();
        let wanted = [
            Command::New, Command::Open, Command::Save, Command::SaveAs, Command::Print, Command::RecoverEdits, Command::Help, Command::ShowFileLists,
            Command::Paste, Command::Copy, Command::Cut, Command::Find, Command::Replace, Command::FindPrevious, Command::FindNext, Command::FilterSelection, Command::FindInFiles,
            Command::GoTo, Command::NavBack, Command::NavForward, Command::LastChange, Command::NextChange, Command::ToggleBookmark, Command::PreviousBookmark, Command::NextBookmark,
            Command::CopyPath, Command::Compare, Command::Settings, Command::SelectAll, Command::ToggleWrap, Command::ToggleReadOnly, Command::WordCount, Command::ShowHistory,
            Command::MoveLineUp, Command::MoveLineDown, Command::Duplicate, Command::Join, Command::ToggleColumn, Command::ColumnFill, Command::ColumnCut, Command::ColumnDelete,
            Command::JustifyLeft, Command::JustifyCenter, Command::JustifyRight, Command::ColumnNumber, Command::ColumnSum, Command::CsvToFixed, Command::FixedToCsv,
            Command::InsertDate, Command::InsertTime, Command::InsertDateTime, Command::InsertIsoDate, Command::InsertFileName, Command::InsertFilePath, Command::GenerateGuid,
            Command::InsertPageBreak, Command::InsertColor, Command::InsertFile, Command::SortAscending, Command::SortDescending, Command::ViewRegular, Command::ViewHex,
            Command::ViewEbcdic, Command::HexInsertDelete, Command::HexCopySelected, Command::Reindent, Command::Indent, Command::Outdent, Command::Upper, Command::Lower,
            Command::SentenceCase, Command::TitleCase, Command::TabsToSpaces, Command::SpacesToTabs, Command::WrapToLineEnds, Command::LineEndsToWrap, Command::TrimLeading,
            Command::TrimTrailing, Command::ParagraphReformat, Command::ParagraphSetup, Command::AlignText(Align::Left), Command::AlignText(Align::Center), Command::AlignText(Align::Right),
            Command::AlignText(Align::Justify), Command::ReopenAs, Command::ConvertTo, Command::SaveCopyAs, Command::ChoosePreset, Command::InsertByte, Command::ChooseFont,
            Command::ZoomIn, Command::ZoomOut, Command::MakeSelections, Command::Narrow, Command::Widen, Command::HideLines, Command::ShowLines, Command::CollapseAll,
            Command::ExpandAll, Command::AlwaysOnTop, Command::FullScreen, Command::Comment, Command::Uncomment, Command::GoToBrace, Command::SelectToBrace,
            Command::Format(formatters::Kind::Html), Command::Format(formatters::Kind::HtmlCompact), Command::Format(formatters::Kind::Xml), Command::Format(formatters::Kind::XmlCompact),
            Command::FormatJson, Command::CompactJson, Command::Format(formatters::Kind::Js), Command::Format(formatters::Kind::JsCompact), Command::Format(formatters::Kind::Css),
            Command::Format(formatters::Kind::CssCompact), Command::Format(formatters::Kind::CodeAttach), Command::Format(formatters::Kind::CodeBreak), Command::Format(formatters::Kind::Sql),
            Command::Format(formatters::Kind::EdiLines), Command::Format(formatters::Kind::EdiOneLine), Command::ComputeHash, Command::ConvertNumber, Command::ProjectOpen,
            Command::ProjectNew, Command::ProjectClose, Command::ProjectSettings, Command::ProjectAddActive, Command::ProjectAddFiles, Command::ProjectAddFolder, Command::ProjectBackup,
            Command::ProjectRestore, Command::DuplicateWindow, Command::SplitHorizontal, Command::SplitVertical, Command::PreviousTab, Command::NextTab, Command::NewInstance,
            Command::RecordMacro, Command::PlayMacro, Command::PlayMacroTimes, Command::LoadMacro, Command::NewScript, Command::LoadScript, Command::ManageAddOns, Command::NewAddOn, Command::CommandPalette, Command::About,
            Command::SetPreview(settings::PreviewView::Text), Command::SetPreview(settings::PreviewView::Split), Command::SetPreview(settings::PreviewView::Full),
        ];
        for command in wanted {
            let found = reachable.contains(&command) || matches!(command, Command::ToggleWrap | Command::ToggleReadOnly | Command::ToggleColumn | Command::AlwaysOnTop | Command::FullScreen);
            assert!(found, "{} isn't in any menu", wanted.iter().position(|each| *each == command).unwrap());
        }
        for action in text_objects::Action::ALL {
            for object in text_objects::Object::ALL {
                assert!(reachable.contains(&Command::TextObject(action, object)));
            }
        }
        for flag in [menus::Flag::WordWrap, menus::Flag::ReadOnly, menus::Flag::ColumnMode, menus::Flag::AlwaysOnTop, menus::Flag::FullScreen] {
            assert!(reachable.contains(&Command::Toggle(flag)));
        }
        let mut paths: Vec<String> = menus::every_command().into_iter().map(|found| found.path).collect();
        let count = paths.len();
        paths.sort();
        paths.dedup();
        assert_eq!(paths.len(), count, "no two menu items have the same place and name");
    }

    #[test]
    fn the_command_box_finds_a_command_by_its_words_and_does_it() {
        let (mut app, ctx) = app_with("");
        set_text(&mut app, &["pear", "apple"]);
        draw(&mut app, &ctx, Vec::new());
        draw(&mut app, &ctx, vec![key(Key::F1, egui::Modifiers::NONE)]);
        assert!(matches!(app.dialog, Dialog::Palette(_)));
        draw(&mut app, &ctx, vec![egui::Event::Text("sort a".into())]);
        draw(&mut app, &ctx, vec![key(Key::Enter, egui::Modifiers::NONE)]);
        assert!(matches!(app.dialog, Dialog::None));
        draw(&mut app, &ctx, Vec::new());
        assert_eq!(lines_of(&app), ["apple", "pear"]);
        let found = menus::matching("ruler", menus::every_command());
        assert!(found.first().is_some_and(|first| first.path.ends_with("Ruler")));
        assert!(menus::matching("no such thing at all", menus::every_command()).is_empty());
        // A switch from the box goes on and off.
        let before = app.view.ruler;
        command(&mut app, &ctx, Command::Toggle(menus::Flag::Ruler));
        assert_ne!(app.view.ruler, before);
        command(&mut app, &ctx, Command::Toggle(menus::Flag::Panel(menus::PanelFlag::Functions)));
        assert!(app.panels.functions);
        draw(&mut app, &ctx, vec![key(Key::F1, egui::Modifiers::NONE)]);
        draw(&mut app, &ctx, vec![key(Key::Escape, egui::Modifiers::NONE)]);
        assert!(matches!(app.dialog, Dialog::None), "Esc closes it");
    }

    fn project_folder(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("limoniter-app-project-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn wait_for_project_job(app: &mut Limoniter, ctx: &Context) {
        while app.project_job.is_some() {
            std::thread::sleep(Duration::from_millis(5));
            draw(app, ctx, Vec::new());
        }
    }

    #[test]
    fn a_project_is_made_added_to_closed_and_reopened_with_its_files() {
        let (mut app, ctx) = app_with("");
        let dir = project_folder("life");
        std::fs::write(dir.join("a.txt"), "aaa").unwrap();
        app.new_project(dir.join("p"));
        let path = dir.join("p.limproj");
        assert!(path.exists(), "the extension goes on, and it's on disk straight away");
        assert!(app.panels.project);
        assert!(app.open_path(&dir.join("a.txt")));
        command(&mut app, &ctx, Command::ProjectAddActive);
        assert_eq!(app.project.as_ref().unwrap().files, [dir.join("a.txt")]);
        draw(&mut app, &ctx, Vec::new());
        command(&mut app, &ctx, Command::ProjectClose);
        assert!(app.project.is_none());
        assert_eq!(app.docs.len(), 1, "its files stay open");
        assert_eq!(app.recent_projects, std::slice::from_ref(&path));
        app.close_tab(0);
        app.open_project(&path);
        assert_eq!(app.project.as_ref().unwrap().name, "p");
        assert_eq!(app.docs[app.active].path.as_deref(), Some(dir.join("a.txt").as_path()), "and come back with it");
        let (again, _) = app_with(&app.current_settings());
        assert_eq!(again.recent_projects, [path]);
        drop(app);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_broken_project_says_so_and_goes_off_the_recent_list() {
        let (mut app, _ctx) = app_with("");
        let dir = project_folder("broken");
        let path = dir.join("bad.limproj");
        std::fs::write(&path, "not one").unwrap();
        app.recent_projects = vec![path.clone()];
        app.open_project(&path);
        assert!(app.project.is_none());
        assert!(app.recent_projects.is_empty());
        assert!(matches!(&app.dialog, Dialog::Message { body, .. } if body.contains("Couldn't open")));
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn project_files_can_be_searched_and_replaced_in() {
        let (mut app, ctx, dir) = folder_app("project", "cat", "dog");
        std::fs::write(dir.join("in.txt"), "cat\n").unwrap();
        std::fs::write(dir.join("out.txt"), "cat\n").unwrap();
        app.find.keep_backups = false;
        app.find.scope = FileScope::Project;
        app.start_find_in_files(false);
        assert!(app.find.status.contains("project"), "there isn't one yet");
        app.new_project(dir.join("p.limproj"));
        app.start_find_in_files(false);
        assert!(app.find.status.contains("no files"));
        app.add_to_project([dir.join("in.txt")]);
        app.start_find_in_files(true);
        wait_for_results(&mut app);
        let mut files: Vec<PathBuf> = app.job.as_ref().unwrap().hits.lock().unwrap().iter().map(|hit| hit.path.clone()).collect();
        files.dedup();
        assert_eq!(files, [dir.join("in.txt")]);
        app.start_replace_in_files(files);
        wait_for_replace(&mut app, &ctx);
        assert_eq!(std::fs::read(dir.join("in.txt")).unwrap(), b"dog\n");
        assert_eq!(std::fs::read(dir.join("out.txt")).unwrap(), b"cat\n");
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn backup_and_restore_run_from_the_commands_and_open_files_keep_up() {
        let (mut app, ctx) = app_with("");
        let dir = project_folder("restore");
        std::fs::write(dir.join("a.txt"), "first\n").unwrap();
        std::fs::write(dir.join("b.txt"), "first\n").unwrap();
        app.new_project(dir.join("p.limproj"));
        app.add_to_project([dir.join("a.txt"), dir.join("b.txt")]);
        command(&mut app, &ctx, Command::ProjectRestore);
        assert!(matches!(&app.dialog, Dialog::Message { body, .. } if body.contains("no backups")));
        app.dialog = Dialog::None;
        command(&mut app, &ctx, Command::ProjectBackup);
        wait_for_project_job(&mut app, &ctx);
        assert!(matches!(&app.dialog, Dialog::Message { body, .. } if body.contains("Copied 3 files")));
        app.dialog = Dialog::None;

        // One open and untouched, and one open with unsaved changes.
        std::fs::write(dir.join("a.txt"), "second\n").unwrap();
        assert!(app.open_path(&dir.join("a.txt")));
        assert!(app.open_path(&dir.join("b.txt")));
        app.doc().text.type_text("unsaved ");
        command(&mut app, &ctx, Command::ProjectRestore);
        assert!(matches!(app.dialog, Dialog::RestorePick { .. }));
        draw(&mut app, &ctx, Vec::new());
        draw(&mut app, &ctx, vec![key(Key::Escape, egui::Modifiers::NONE)]);
        assert!(matches!(app.dialog, Dialog::None));
        assert_eq!(std::fs::read(dir.join("a.txt")).unwrap(), b"second\n", "cancelling changes nothing");

        let chosen = project::list_backups(app.project.as_ref().unwrap())[0].clone();
        app.restore_project(chosen);
        wait_for_project_job(&mut app, &ctx);
        assert_eq!(std::fs::read(dir.join("a.txt")).unwrap(), b"first\n");
        let a = app.doc_for_path(&dir.join("a.txt")).unwrap();
        assert_eq!(&*app.docs[a].text.line(0), "first", "the open file reads again");
        let b = app.doc_for_path(&dir.join("b.txt")).unwrap();
        assert!(app.docs[b].text.line(0).starts_with("unsaved"), "unsaved work is left alone");
        assert!(matches!(&app.dialog, Dialog::Message { body, .. } if body.contains("Put back 2 files") && body.contains("unsaved changes")));
        assert_eq!(project::list_backups(app.project.as_ref().unwrap()).len(), 2, "with a backup of how things were");
        drop(app);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn project_settings_open_and_what_they_hold_is_saved() {
        let (mut app, ctx) = app_with("");
        let dir = project_folder("settings");
        app.new_project(dir.join("p.limproj"));
        command(&mut app, &ctx, Command::ProjectSettings);
        assert!(matches!(&app.dialog, Dialog::ProjectSettings { name, .. } if name == "p"));
        draw(&mut app, &ctx, Vec::new());
        app.dialog = Dialog::ProjectSettings { name: "  ".into(), notes: "a note".into() };
        draw(&mut app, &ctx, Vec::new());
        let saved = |app: &mut Limoniter, name: &str, notes: &str| {
            if let Some(project) = &mut app.project {
                project.name = name.into();
                project.notes = notes.into();
            }
            app.save_project();
        };
        saved(&mut app, "Renamed", "a note");
        let loaded = Project::load(&dir.join("p.limproj")).unwrap();
        assert_eq!((loaded.name.as_str(), loaded.notes.as_str()), ("Renamed", "a note"));
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn menu_items_are_greyed_when_they_can_do_nothing() {
        let (mut app, ctx) = app_with("");
        set_text(&mut app, &["one two"]);
        draw(&mut app, &ctx, Vec::new());
        for command in [Command::Upper, Command::TitleCase, Command::Copy, Command::Cut, Command::ColumnFill, Command::HexInsertDelete, Command::Widen, Command::FindNext] {
            assert!(!app.can(command), "{} should be greyed", app.docs.len());
        }
        app.doc().text.set_selection((0, 0), (0, 3), false);
        for command in [Command::Upper, Command::TitleCase, Command::Copy, Command::Narrow, Command::CsvToFixed, Command::ToggleHex] {
            assert!(app.can(command));
        }
        command(&mut app, &ctx, Command::ToggleHex);
        assert!(app.can(Command::HexInsertDelete) && !app.can(Command::Upper) && app.can(Command::ViewEbcdic));
        app.close_tab(0);
        assert!(app.can(Command::New) && app.can(Command::Toggle(menus::Flag::Ruler)) && !app.can(Command::Save) && !app.can(Command::Toggle(menus::Flag::ReadOnly)));
    }

    #[test]
    fn a_refused_paste_tells_the_person() {
        let (mut app, ctx) = app_with("");
        command(&mut app, &ctx, Command::ViewEbcdic);
        app.doc().hex.ascii_pane = true;
        let _ = ctx.run_ui(input(Vec::new()), |ui| app.run_panel_action(Action::Paste("\u{20AC}".into()), ui));
        draw(&mut app, &ctx, Vec::new());
        assert!(matches!(&app.dialog, Dialog::Message { body, .. } if body.contains("EBCDIC")));
    }

    #[test]
    fn a_hex_document_comes_back_as_bytes() {
        let mut doc = Document::new("bin".into());
        doc.hex.load(vec![0, 1, 2, 255]);
        doc.hex_mode = true;
        doc.hex.insert_mode = true;
        doc.hex.move_caret(2, false);
        doc.hex.type_byte(0x7F);
        let (meta, body) = through_a_copy(&doc);
        let back = Document::from_recovered(meta, body).unwrap();
        assert!(back.hex_mode && back.is_modified());
        assert_eq!(back.hex.bytes(), [0, 1, 0x7F, 2, 255]);
    }

    #[test]
    fn a_huge_files_edits_come_back_on_the_same_file_or_alone_if_it_changed() {
        let mut data = Vec::new();
        for i in 0..1000 {
            data.extend_from_slice(format!("line {i}\r\n").as_bytes());
        }
        let path = temp_file("disk", &data);
        let mut doc = Document::new(String::new());
        doc.path = Some(path.clone());
        let source = Source::open_as(&path, None).unwrap();
        doc.text.set_source(source.clone());
        doc.scanning = Some(source);
        wait_for_scan(&mut doc);
        doc.text.move_caret(500, 0, false);
        doc.text.type_text("EDITED ");
        let edited = doc.text.encode(&doc.text.format, Lossy::Refuse).unwrap();
        let (meta, body) = through_a_copy(&doc);
        assert!(matches!(&body, recovery::Body::Text(image) if image.source.is_some()), "the copy holds only the edit");

        let mut back = Document::from_recovered(meta.clone(), body).unwrap();
        wait_for_scan(&mut back);
        assert_eq!(back.text.encode(&back.text.format, Lossy::Refuse).unwrap(), edited);
        assert!(back.is_modified());

        // Now the file changes underneath, so the untouched lines are gone: the edit is still kept.
        let (_, body) = through_a_copy(&doc);
        std::fs::write(&path, b"somebody else's new contents").unwrap();
        let alone = Document::from_recovered(meta, body).unwrap();
        assert!(alone.path.is_none(), "it can't be saved over the changed file by accident");
        assert!(alone.is_modified());
        assert!(alone.text.all_lines().iter().any(|line| line.starts_with("EDITED line 500")));
        assert!(alone.text.line(0).contains("has changed"));
        std::fs::remove_dir_all(path.parent().unwrap()).unwrap();
    }

    #[test]
    fn the_offer_says_how_a_copy_compares_with_the_file() {
        let path = temp_file("compare", b"on disk");
        let format = TextFormat::default();
        let mut doc = Document::open(&path, Some(format)).unwrap();
        doc.text.move_document_end(false);
        doc.text.type_text(" edited");
        let (mut meta, body) = through_a_copy(&doc);
        assert_eq!(compare_with_file(&meta, &body), (String::new(), true));

        std::fs::write(&path, b"on disk edited").unwrap();
        let (note, selected) = compare_with_file(&meta, &body);
        assert!(note.contains("already holds") && !selected, "no need to recover what was saved");

        std::fs::write(&path, b"something quite different now").unwrap();
        let (note, selected) = compare_with_file(&meta, &body);
        assert!(note.contains("has changed") && selected, "{note}");

        std::fs::remove_file(&path).unwrap();
        assert!(compare_with_file(&meta, &body).0.contains("no longer there"));
        meta.path = None;
        assert_eq!(compare_with_file(&meta, &body), (String::new(), true));
        std::fs::remove_dir_all(path.parent().unwrap()).unwrap();
    }

    fn names_left_behind(root: &Path) -> Vec<String> {
        let (orphans, found) = recovery::find_orphans(root);
        let names = found.into_iter().map(|item| item.outcome.unwrap().0.name).collect();
        recovery::tidy(orphans);
        names
    }

    #[test]
    fn only_unsaved_documents_are_kept_and_their_copies_follow_what_happens_to_them() {
        let root = std::env::temp_dir().join(format!("limoniter-app-offer-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let mut session = recovery::Session::start(&root).unwrap();
        let mut docs = vec![Document::untitled("Clean".into(), &PRESETS[0]), Document::untitled("Typed".into(), &PRESETS[0]), Document::untitled("Saved later".into(), &PRESETS[0]), Document::untitled("Closed later".into(), &PRESETS[0])];
        for doc in &mut docs[1..] {
            doc.text.type_text("some text");
        }
        offer_changes(&mut session, &mut docs);
        assert!(session.flush(Duration::from_secs(5)));
        let ids: Vec<u64> = docs.iter().map(|doc| doc.id).collect();
        assert!(!session.has_copy(ids[0]), "nothing to keep for a document with no changes");
        assert!(session.has_copy(ids[1]) && session.has_copy(ids[2]) && session.has_copy(ids[3]));

        docs[2].text.mark_saved();
        docs.remove(3);
        offer_changes(&mut session, &mut docs);
        assert!(session.flush(Duration::from_secs(5)));
        assert!(!session.has_copy(ids[2]), "saved");
        assert!(!session.has_copy(ids[3]), "closed");

        // Typing again makes a new copy; with no new typing, none is taken.
        let before = docs[1].recovery_key;
        offer_changes(&mut session, &mut docs);
        assert_eq!(docs[1].recovery_key, before);
        docs[1].text.type_text(" and more");
        offer_changes(&mut session, &mut docs);
        assert_ne!(docs[1].recovery_key, before);
        session.flush(Duration::from_secs(5));
        session.abandon();
        recovery::wait_until_let_go(&root);

        let (orphans, found) = recovery::find_orphans(&root);
        assert_eq!(found.len(), 1);
        let (meta, body) = found.into_iter().next().unwrap().outcome.unwrap();
        recovery::tidy(orphans);
        assert_eq!(meta.name, "Typed");
        let back = Document::from_recovered(meta, body).unwrap();
        assert_eq!(&*back.text.line(0), "some text and more", "the latest edit is what comes back");
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn when_the_window_fails_the_very_latest_edit_is_written_at_once() {
        let root = std::env::temp_dir().join(format!("limoniter-app-emergency-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let mut session = recovery::Session::start(&root).unwrap();
        let mut docs = vec![Document::untitled("A".into(), &PRESETS[0]), Document::untitled("B".into(), &PRESETS[0])];
        docs[0].text.type_text("old");
        offer_changes(&mut session, &mut docs);
        session.flush(Duration::from_secs(5));
        // Edited after the last hand-over, as if the failure came in the same frame.
        docs[0].text.type_text(" newest");
        save_everything_now(&mut session, &docs);
        // Nothing here waits: the copy has to be on the disk by the time this returns.
        session.abandon();
        assert_eq!(names_left_behind(&root), ["A"]);
        recovery::wait_until_let_go(&root);
        let (orphans, found) = recovery::find_orphans(&root);
        let (meta, body) = found.into_iter().next().unwrap().outcome.unwrap();
        recovery::tidy(orphans);
        let back = Document::from_recovered(meta, body).unwrap();
        assert_eq!(&*back.text.line(0), "old newest");
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn a_font_has_to_be_the_same_width_throughout() {
        let read = |name: &str| std::fs::read(PathBuf::from(r"C:\Windows\Fonts").join(name)).ok();
        if let Some(fixed) = read("consola.ttf") {
            assert!(is_fixed_pitch(fixed));
        }
        if let Some(proportional) = read("arial.ttf") {
            assert!(!is_fixed_pitch(proportional));
        }
        assert!(!is_fixed_pitch(b"this isn't a font at all".to_vec()), "a bad file is refused, not a crash");
    }

    #[test]
    fn searches_and_recent_files_go_through_the_settings_file_exactly() {
        let settings = "ruler=true\nfind_history=a=b  \nfind_history= spaced\nfind_favourite=x\nrecent_file=C:\\one=1.txt\n";
        assert_eq!(list_setting(settings, "find_history"), ["a=b  ", " spaced"]);
        assert_eq!(list_setting(settings, "find_favourite"), ["x"]);
        assert_eq!(list_setting(settings, "recent_file"), ["C:\\one=1.txt"]);
        assert!(list_setting(settings, "nothing").is_empty());
        let mut list = Vec::new();
        note_search(&mut list, "");
        note_search(&mut list, "one");
        note_search(&mut list, "two");
        note_search(&mut list, "one");
        assert_eq!(list, ["one", "two"]);
        for i in 0..50 {
            note_search(&mut list, &i.to_string());
        }
        assert_eq!(list.len(), MAX_SEARCHES);
    }

    #[test]
    fn commands_that_change_text_know_they_do() {
        for command in [Command::Comment, Command::InsertDate, Command::InsertFile, Command::Paste, Command::Undo, Command::Join] {
            assert!(command.edits());
        }
        for command in [Command::Copy, Command::Find, Command::NavBack, Command::ToggleWrap, Command::ToggleReadOnly, Command::ChooseFont, Command::GoTo] {
            assert!(!command.edits());
        }
    }

    #[test]
    fn a_damaged_copy_is_offered_for_discarding_and_never_selected() {
        let item = offer_item(recovery::Found { file: PathBuf::from("doc-1.rec"), outcome: Err("Its checksum doesn't match.".to_string()) });
        assert!(!item.selected && item.content.is_none());
        assert!(item.note.contains("checksum"));
    }

    #[test]
    fn a_failed_save_leaves_the_original_file_as_it_was() {
        let path = temp_file("save", b"precious");
        // A folder in the way of the temporary name makes the write fail.
        let mut temp = path.as_os_str().to_owned();
        temp.push(".limoniter-tmp");
        std::fs::create_dir(PathBuf::from(&temp)).unwrap();
        assert!(codec::write_file(&path, b"new contents").is_err());
        assert_eq!(std::fs::read(&path).unwrap(), b"precious");
        std::fs::remove_dir_all(path.parent().unwrap()).unwrap();
    }

    /// A file of any size opens in hex where it lies, and can be searched, edited, undone and saved.
    /// Run with LIMONITER_HUGE naming a large binary file that ends "LIMONITER-END-MARKER", and LIMONITER_HUGE_OUT
    /// a place to save the edited copy.
    #[test]
    #[ignore = "needs a huge file named in LIMONITER_HUGE"]
    fn a_huge_binary_file_is_edited_in_hex_where_it_lies() {
        let (Ok(source), Ok(out)) = (std::env::var("LIMONITER_HUGE"), std::env::var("LIMONITER_HUGE_OUT")) else {
            return;
        };
        let (source, out) = (PathBuf::from(source), PathBuf::from(out));
        let size = std::fs::metadata(&source).unwrap().len() as usize;
        let (mut app, ctx) = app_with("");
        let clock = std::time::Instant::now();
        assert!(app.open_path(&source));
        let index = app.active;
        assert!(app.docs[index].hex_mode && app.docs[index].hex.is_on_disk());
        assert_eq!(app.docs[index].hex.len(), size);
        draw(&mut app, &ctx, Vec::new());
        eprintln!("opened and drawn in {:?}", clock.elapsed());
        let marker = b"LIMONITER-END-MARKER";
        assert_eq!(app.docs[index].hex.read(size - marker.len(), marker.len()), marker);
        let clock = std::time::Instant::now();
        app.docs[index].hex.move_caret(0, false);
        let o = SearchOptions { pattern: "END-MARKER".into(), match_case: true, ..Default::default() };
        assert_eq!(app.docs[index].hex.find(&o), Ok(true));
        assert_eq!(app.docs[index].hex.anchor, size - 10);
        eprintln!("searched the whole file in {:?}", clock.elapsed());
        // From the Find panel it goes a stretch at a time, so the window keeps drawing.
        let clock = std::time::Instant::now();
        app.docs[index].hex.move_caret(0, false);
        app.find.find = "end-marker".into();
        app.run_find(false);
        let mut frames = 0;
        while app.pending.is_some() {
            app.continue_pending(&ctx);
            frames += 1;
        }
        assert_eq!(app.docs[index].hex.anchor, size - 10, "{}", app.find.status);
        eprintln!("searched again, ignoring case, in {:?} over {frames} frames", clock.elapsed());
        let hex = &mut app.docs[index].hex;
        let first = hex.byte(0);
        hex.move_caret(0, false);
        hex.type_hex_digit(0xA);
        hex.type_hex_digit(0x5);
        assert_eq!(hex.byte(0), 0xA5);
        let middle = size / 2;
        hex.move_caret(middle, false);
        hex.insert_run(5, 0xEE);
        assert_eq!(hex.len(), size + 5);
        hex.undo();
        assert_eq!(hex.len(), size);
        hex.redo();
        assert_eq!(hex.read(middle, 5), [0xEE; 5]);
        draw(&mut app, &ctx, Vec::new());
        let recovery::Body::HexOnDisk { added, .. } = app.docs[index].snapshot().body else {
            panic!("a huge file's recovery copy holds only its edits");
        };
        assert!(added.len() < 64, "{}", added.len());
        let clock = std::time::Instant::now();
        assert!(app.write_to(index, &out));
        assert!(app.wait_for_save(index));
        eprintln!("saved in {:?}", clock.elapsed());
        assert!(!app.docs[index].is_modified());
        assert_eq!(std::fs::metadata(&out).unwrap().len() as usize, size + 5);
        let saved = crate::byte_store::FileBytes::open(&out).unwrap();
        let saved = crate::byte_store::ByteStore::from_file(saved);
        assert_eq!(saved.byte(0), 0xA5);
        assert_ne!(first, 0xA5, "the test changed something");
        assert_eq!(saved.read(middle - 1, 7)[1..6], [0xEE; 5]);
        assert_eq!(saved.read(size + 5 - marker.len(), marker.len()), marker);
        // It now reads from the saved file, and can still be changed.
        assert_eq!(app.docs[index].hex.source().unwrap().path, out);
        assert_eq!(app.docs[index].hex.byte(0), 0xA5);
        command(&mut app, &ctx, Command::ToggleHex);
        eprintln!("switched to text: hex_mode={}", app.docs[index].hex_mode);
        std::fs::remove_file(&out).unwrap();
    }

    /// Remote files through the whole window, against a real server. LIMONITER_SFTP is sftp://user@host:port/folder,
    /// LIMONITER_SFTP_KEY a private key it accepts, and LIMONITER_SFTP_ROOT that folder as this computer sees it.
    #[test]
    #[ignore = "needs an SFTP server named in LIMONITER_SFTP"]
    fn a_remote_file_is_opened_edited_and_sent_back() {
        let (Ok(url), Ok(key), Ok(root)) = (std::env::var("LIMONITER_SFTP"), std::env::var("LIMONITER_SFTP_KEY"), std::env::var("LIMONITER_SFTP_ROOT")) else {
            return;
        };
        let root = PathBuf::from(root);
        std::fs::write(root.join("app-test.txt"), "one\ntwo\n").unwrap();
        let (mut app, ctx) = app_with("");
        app.remote_root = Some(std::env::temp_dir().join(format!("limoniter-remote-test-{}", std::process::id())));
        let settle = |app: &mut Limoniter| {
            let began = std::time::Instant::now();
            loop {
                draw(app, &ctx, Vec::new());
                if app.remote_jobs.is_empty() {
                    break;
                }
                assert!(began.elapsed() < Duration::from_secs(60), "the server never answered");
                std::thread::sleep(Duration::from_millis(20));
            }
        };
        app.open_remote_box(None);
        let Dialog::Remote(state) = &mut app.dialog else { panic!("the box is open") };
        state.address.clone_from(&url);
        state.key_file.clone_from(&key);
        app.remote_connect(None);
        settle(&mut app);
        // A server not met before: nothing happens until its key is trusted.
        let Dialog::Remote(state) = &mut app.dialog else { panic!("the box stays open") };
        let print = state.trust.clone().expect("its key is shown to be trusted");
        assert!(print.starts_with("SHA256:"));
        let site = state.site().unwrap().0;
        app.known_hosts.trust(&site, &print);
        app.remote_connect(None);
        settle(&mut app);
        let Dialog::Remote(state) = &app.dialog else { panic!("the box stays open") };
        assert!(state.entries.iter().any(|entry| entry.name == "app-test.txt"), "{}", state.status);
        let folder = state.folder.clone().unwrap();
        app.remote_fetch(site.clone(), crate::remote::join(&folder, "app-test.txt"));
        settle(&mut app);
        assert!(matches!(app.dialog, Dialog::None), "the box closes once the file is open");
        let index = app.active;
        assert!(app.docs[index].remote.is_some());
        assert_eq!(&*app.docs[index].text.line(1), "two");
        // Saved here, then sent back.
        app.docs[index].text.set_line_keeping_cursor(1, vec!["changed".into()]);
        command(&mut app, &ctx, Command::Save);
        settle(&mut app);
        assert_eq!(std::fs::read_to_string(root.join("app-test.txt")).unwrap(), "one\nchanged\n");
        // A new file saved to the server.
        app.new_document();
        let index = app.active;
        app.docs[index].text.set_content(vec!["brand new".into()]);
        app.remote_save(index, site, crate::remote::join(&folder, "app-new.txt"));
        settle(&mut app);
        assert_eq!(std::fs::read_to_string(root.join("app-new.txt")).unwrap(), "brand new");
        assert!(!app.docs[index].is_modified());
        // The site and trusted key are kept; no password is.
        let settings = app.current_settings();
        assert!(settings.contains("sftp_host_key=") && settings.contains("sftp_site="));
        std::fs::remove_file(root.join("app-test.txt")).unwrap();
        std::fs::remove_file(root.join("app-new.txt")).unwrap();
    }
}
